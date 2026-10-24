use crate::{model::*, *};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard},
};

pub(crate) struct Inner {
    pub conn: Mutex<Connection>,
    pub import_runtime: Mutex<crate::import::ImportRuntime>,
    operation_gate: RwLock<()>,
    operation_owner: Mutex<Option<std::thread::ThreadId>>,
    pub organization_gate: Mutex<()>,
    #[cfg(feature = "autotag")]
    pub tag_gate: Mutex<()>,
    #[cfg(feature = "autotag")]
    pub catalog_runtime: Mutex<Option<std::sync::Weak<cuerithm_autotag::CatalogRuntime>>>,
    #[cfg(feature = "autotag")]
    pub artifact_cleanup_gate: Mutex<()>,
    pub _lease: File,
    pub root: PathBuf,
    pub installation_root: PathBuf,
}
#[derive(Clone)]
pub struct Library {
    pub(crate) inner: Arc<Inner>,
}
#[derive(Debug)]
pub struct LibraryBusy;
impl std::fmt::Display for LibraryBusy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Library operation is busy; retry after it finishes")
    }
}
impl std::error::Error for LibraryBusy {}
/// A short exclusive library operation. Keep the lease on this thread; background
/// connections fail busy until it is dropped, while owner methods remain usable.
pub struct LibraryOperationLease<'a> {
    inner: &'a Inner,
    _gate: RwLockWriteGuard<'a, ()>,
}
impl Drop for LibraryOperationLease<'_> {
    fn drop(&mut self) {
        *self
            .inner
            .operation_owner
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = None;
    }
}
pub(crate) struct ConnectionGuard<'a> {
    connection: MutexGuard<'a, Connection>,
    // Field order releases the SQLite mutex before its shared operation lease.
    _operation: Option<RwLockReadGuard<'a, ()>>,
}
impl std::ops::Deref for ConnectionGuard<'_> {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        &self.connection
    }
}
impl std::ops::DerefMut for ConnectionGuard<'_> {
    fn deref_mut(&mut self) -> &mut Connection {
        &mut self.connection
    }
}
impl Library {
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_scoped(path, None)
    }
    pub(crate) fn open_in_installation(path: &Path, installation: &Path) -> Result<Self> {
        Self::open_scoped(path, Some(installation))
    }
    fn open_scoped(path: &Path, installation: Option<&Path>) -> Result<Self> {
        let root = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        std::fs::create_dir_all(root)?;
        // Bind private staging to the opened library's real directory. Native
        // publication must not follow aliases such as macOS /var on each write.
        let root = root
            .canonicalize()
            .context("Resolve library data directory")?;
        let mut lock_path = path.as_os_str().to_owned();
        lock_path.push(".lock");
        let lease = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)?;
        // A decoder fork can briefly inherit flock's open-file description
        // before exec closes it. Keep exclusion, but let that transient settle.
        let lock_started = std::time::Instant::now();
        loop {
            match fs2::FileExt::try_lock_exclusive(&lease) {
                Ok(()) => break,
                Err(error)
                    if error.raw_os_error() == fs2::lock_contended_error().raw_os_error()
                        && lock_started.elapsed() < std::time::Duration::from_millis(250) =>
                {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Err(error) => return Err(error).context("This library is already open"),
            }
        }
        let installation_root = installation.unwrap_or(&root).canonicalize()?;
        crate::library_backup::ensure_library_open_safe(&installation_root)?;
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        ensure!(
            (0..=31).contains(&version),
            "Library schema is newer than this application"
        );
        if version == 0 {
            let tables:i64=conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",[],|r|r.get(0))?;
            ensure!(tables == 0, "This is not a Cuerithm library");
            // Must be set before the first table exists.
            crate::lean::enable_incremental_vacuum(&conn)?;
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE;\n{}\nCOMMIT;",
                include_str!("../migrations/001_initial.sql")
            ))?;
        }
        let app: i64 = conn.query_row("PRAGMA application_id", [], |r| r.get(0))?;
        ensure!(app == 1129665093, "This is not a Cuerithm library");
        if version < 2 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE;\n{}\nCOMMIT;",
                include_str!("../migrations/002_tag_writes.sql")
            ))?;
        }
        if version < 3 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE;\n{}\nCOMMIT;",
                include_str!("../migrations/003_preparation_cache.sql")
            ))?;
        }
        if version < 4 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE;\n{}\nCOMMIT;",
                include_str!("../migrations/004_cleanup.sql")
            ))?;
        }
        if version < 5 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE;\n{}\nCOMMIT;",
                include_str!("../migrations/005_cleanup_preparation.sql")
            ))?;
        }
        if version < 6 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE;\n{}\nCOMMIT;",
                include_str!("../migrations/006_playlists.sql")
            ))?;
        }
        if version < 7 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE;\n{}\nCOMMIT;",
                include_str!("../migrations/007_maintenance.sql")
            ))?;
        }
        if version < 8 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE;\n{}\nCOMMIT;",
                include_str!("../migrations/008_maintenance_writes.sql")
            ))?;
        }
        if version < 9 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE;\n{}\nCOMMIT;",
                include_str!("../migrations/009_unused_removal.sql")
            ))?;
        }
        if version < 10 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE;\n{}\nCOMMIT;",
                include_str!("../migrations/010_unused_retention.sql")
            ))?;
        }
        if version < 11 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE;\n{}\nCOMMIT;",
                include_str!("../migrations/011_native_import.sql")
            ))?;
        }
        if version < 12 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE;\n{}\nCOMMIT;",
                include_str!("../migrations/012_automatic_playlists.sql")
            ))?;
        }
        if version < 13 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE;\n{}\nCOMMIT;",
                include_str!("../migrations/013_catalog_access.sql")
            ))?;
        }
        if version < 14 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE;\n{}\nCOMMIT;",
                include_str!("../migrations/014_organization.sql")
            ))?;
        }
        if version < 15 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE;\n{}\nCOMMIT;",
                include_str!("../migrations/015_match_evidence.sql")
            ))?;
        }
        if version < 16 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE;\n{}\nCOMMIT;",
                include_str!("../migrations/016_organization_workflows.sql")
            ))?;
        }
        if version < 17 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE;\n{}\nCOMMIT;",
                include_str!("../migrations/017_organization_references.sql")
            ))?;
        }
        if version < 18 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE;\n{}\nCOMMIT;",
                include_str!("../migrations/018_organization_copy_conflicts.sql")
            ))?;
        }
        if version < 19 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE;\n{}\nCOMMIT;",
                include_str!("../migrations/019_metadata_edits.sql")
            ))?;
        }
        if version < 20 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE;\n{}\nCOMMIT;",
                include_str!("../migrations/020_match_review.sql")
            ))?;
        }
        if version < 21 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE;\n{}\nCOMMIT;",
                include_str!("../migrations/021_preparation_presets.sql")
            ))?;
        }
        if version < 22 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE;\n{}\nCOMMIT;",
                include_str!("../migrations/022_curation.sql")
            ))?;
        }
        if version < 23 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE;\n{}\nCOMMIT;",
                include_str!("../migrations/023_catalog_runtime.sql")
            ))?;
        }
        if version < 24 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE;\n{}\nCOMMIT;",
                include_str!("../migrations/024_native_reconciliation.sql")
            ))?;
        }
        if version < 25 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE;\n{}\nCOMMIT;",
                include_str!("../migrations/025_manual_publication.sql")
            ))?;
        }
        if version < 26 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE;\n{}\nCOMMIT;",
                include_str!("../migrations/026_native_playlist_sync.sql")
            ))?;
        }
        if version < 27 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE;\n{}\nCOMMIT;",
                include_str!("../migrations/027_rekordbox_sync.sql")
            ))?;
        }
        if version < 28 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE;\n{}\nCOMMIT;",
                include_str!("../migrations/028_sync_target_configurations.sql")
            ))?;
        }
        if version < 29 {
            crate::portable::migrate_identity(&conn)?;
        }
        if version < 30 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE;\n{}\nCOMMIT;",
                include_str!("../migrations/030_library_import.sql")
            ))?;
        }
        if version < 31 {
            // The saved sync operations are converted first, while the records still hold the
            // full copies their recorded digests were computed from.
            conn.execute_batch("BEGIN IMMEDIATE")?;
            let upgraded = crate::upgrade_operation_ledgers(&conn).and_then(|()| {
                conn.execute_batch(include_str!("../migrations/031_lean_native_records.sql"))
                    .map_err(Into::into)
            });
            match upgraded {
                Ok(()) => conn.execute_batch("COMMIT")?,
                Err(error) => {
                    let _ = conn.execute_batch("ROLLBACK");
                    return Err(error);
                }
            }
        }
        #[cfg(not(feature = "autotag"))]
        {
            let pending: i64 = conn.query_row(
                "SELECT (SELECT COUNT(*) FROM tag_writes WHERE state='prepared') + (SELECT COUNT(*) FROM cleanup_writes WHERE state='prepared') + (SELECT COUNT(*) FROM maintenance_writes WHERE state='prepared') + (SELECT COUNT(*) FROM metadata_edit_writes WHERE state IN ('prepared','publishing','recovery_required','conflict')) + (SELECT COUNT(*) FROM metadata_edit_overrides o JOIN jobs j ON j.id=o.job_id WHERE j.state IN ('queued','running','attention') AND (o.state IN ('queued','claimed') OR (o.state='committed' AND o.final_json IS NULL))) + (SELECT COUNT(*) FROM preparation_review_decisions d JOIN jobs j ON j.id=d.job_id WHERE j.state IN ('queued','running','attention') AND d.state IN ('pending','claimed','applied'))",
                [],
                |r| r.get(0),
            )?;
            let curation_pending: i64 = conn.query_row(
                "SELECT COUNT(*) FROM curation_tracks c WHERE (c.native_required=1 AND c.revision!=c.acknowledged_revision) OR EXISTS(SELECT 1 FROM curation_publications p WHERE p.track_id=c.track_id AND p.native_required=1 AND p.state IN ('saving','waiting_incoming','recovery_required','error'))",
                [],
                |row| row.get(0),
            )?;
            ensure!(
                pending == 0 && curation_pending == 0,
                "Pending metadata writes require a tagging-enabled build for recovery"
            );
        }

        conn.pragma_update(None, "foreign_keys", true)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "FULL")?;
        crate::lean::configure_wal(&conn)?;
        conn.execute(
            "UPDATE jobs SET state='queued',stage='queued',error=NULL WHERE state='running'",
            [],
        )?;
        // A previous process cannot still own an applying batch once this
        // exclusive library lease has been acquired. Keep cancellation durable.
        conn.execute("UPDATE organization_batch_rows SET outcome=CASE WHEN operation_id IS NULL THEN 'failed' ELSE 'recovery_required' END,error=COALESCE(error,'Organization was interrupted; inspect recovery before continuing') WHERE outcome IN ('ready','applying') AND batch_id IN (SELECT id FROM organization_batches WHERE state='applying')",[])?;
        conn.execute("UPDATE organization_batches SET state='recovery_required',updated=?1 WHERE state='applying'",[now()])?;
        let lib = Self {
            inner: Arc::new(Inner {
                conn: Mutex::new(conn),
                import_runtime: Mutex::new(Default::default()),
                operation_gate: RwLock::new(()),
                operation_owner: Mutex::new(None),
                organization_gate: Mutex::new(()),
                #[cfg(feature = "autotag")]
                tag_gate: Mutex::new(()),
                #[cfg(feature = "autotag")]
                catalog_runtime: Mutex::new(None),
                #[cfg(feature = "autotag")]
                artifact_cleanup_gate: Mutex::new(()),
                _lease: lease,
                root: root.to_path_buf(),
                installation_root,
            }),
        };
        let count = lib
            .conn()?
            .query_row("SELECT COUNT(*) FROM settings", [], |r| r.get::<_, i64>(0))?;
        if count == 0 {
            lib.save_settings(PreparationSettings::default())?;
        }
        #[cfg(feature = "autotag")]
        {
            lib.recover_tag_writes()?;
            lib.recover_cleanup_writes()?;
            lib.recover_duplicate_writes()?;
        }
        lib.recover_unused_removals()?;
        lib.recover_organization()?;
        #[cfg(feature = "autotag")]
        {
            lib.recover_metadata_edits()?;
            lib.recover_artifact_cleanup(100)?;
            // Cleanup previews and Undo journals that earlier versions never trimmed.
            let _ = lib.prune_cleanup();
        }
        lib.recover_curation()?;
        let _ = lib.cleanup_relocation_indexes();
        // Saved scans, reviews and previews that earlier versions never trimmed.
        let _ = lib.prune_saved_reports();
        // Original audio that earlier versions kept after a write had settled.
        let _ = lib.sweep_legacy_originals();
        // The restore copies of syncs older than the ten kept for each DJ program.
        let _ = lib.prune_rekordbox_sync_history();
        let _ = lib.prune_dj_sync_history();
        // Waveforms of bytes that have since changed, and scratch folders a crash left behind.
        lib.trim_caches();
        // A log left by a process that was killed can be as large as its last transaction,
        // and an older library holds space that its earlier versions never gave back.
        lib.reclaim_storage_when_worthwhile();
        lib.checkpoint_wal_when_idle();
        Ok(lib)
    }
    pub fn exclusive_operation(&self) -> Result<LibraryOperationLease<'_>> {
        let gate = self
            .inner
            .operation_gate
            .try_write()
            .map_err(|_| anyhow::Error::new(LibraryBusy))?;
        // Once the write lease is held, no new job can be claimed. Refuse
        // existing work before exposing busy errors to an in-flight DSP worker.
        let running: bool = self
            .inner
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("Library connection interrupted"))?
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM jobs WHERE state IN ('queued','running'))",
                [],
                |r| r.get(0),
            )?;
        if running {
            return Err(anyhow::Error::new(LibraryBusy));
        }
        *self
            .inner
            .operation_owner
            .lock()
            .map_err(|_| anyhow::anyhow!("Library operation owner interrupted"))? =
            Some(std::thread::current().id());
        Ok(LibraryOperationLease {
            inner: &self.inner,
            _gate: gate,
        })
    }
    pub(crate) fn conn(&self) -> Result<ConnectionGuard<'_>> {
        let owner = *self
            .inner
            .operation_owner
            .lock()
            .map_err(|_| anyhow::anyhow!("Library operation owner interrupted"))?;
        let operation = if owner == Some(std::thread::current().id()) {
            None
        } else {
            Some(
                self.inner
                    .operation_gate
                    .try_read()
                    .map_err(|_| anyhow::Error::new(LibraryBusy))?,
            )
        };
        let connection = self
            .inner
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("Library connection interrupted"))?;
        Ok(ConnectionGuard {
            connection,
            _operation: operation,
        })
    }
    pub fn data_dir(&self) -> &Path {
        &self.inner.root
    }
    pub(crate) fn installation_dir(&self) -> &Path {
        &self.inner.installation_root
    }
    pub fn save_settings(&self, settings: PreparationSettings) -> Result<SettingsRevision> {
        self.save_settings_migrating(settings, &[])
    }
    pub fn save_settings_migrating(
        &self,
        settings: PreparationSettings,
        obsolete: &[SettingsRevision],
    ) -> Result<SettingsRevision> {
        self.save_settings_with_assignments_inner(settings, obsolete, None, &[])
    }
    pub fn save_settings_with_preparation_assignments(
        &self,
        settings: PreparationSettings,
        expected_revision: &str,
        changes: &[crate::PreparationFolderChange],
    ) -> Result<SettingsRevision> {
        self.save_settings_with_assignments_inner(settings, &[], Some(expected_revision), changes)
    }
    fn save_settings_with_assignments_inner(
        &self,
        settings: PreparationSettings,
        obsolete: &[SettingsRevision],
        expected_revision: Option<&str>,
        changes: &[crate::PreparationFolderChange],
    ) -> Result<SettingsRevision> {
        settings.validate()?;
        let assignments: Option<crate::PreparedPreparationAssignments> = if changes.is_empty() {
            None
        } else {
            Some(self.prepare_preparation_assignment_changes(&settings, changes)?)
        };
        if settings.organization.enabled()
            && matches!(
                settings.organization.mode,
                crate::OrganizationSettingMode::Move | crate::OrganizationSettingMode::Copy
            )
        {
            self.organization_root_authority(
                settings
                    .organization
                    .destination_root_id
                    .as_deref()
                    .context("Choose an organization destination")?,
            )?;
        }
        let models = camelot_core::Engine::new(&settings.engine)?.model_identity();
        let revision = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(
                &json!({"settings":settings,"models":models})
            )?)
        );
        let snapshot = SettingsSnapshot {
            revision: revision.clone(),
            settings,
            models,
            preset_capture: None,
        };
        let encoded = serde_json::to_string(&snapshot)?;
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        if let Some(expected) = expected_revision {
            let current: String = tx.query_row(
                "SELECT revision FROM selected_settings WHERE singleton=1",
                [],
                |row| row.get(0),
            )?;
            ensure!(
                current == expected,
                "Settings changed; reload before saving folder assignments"
            );
        }
        tx.execute(
            "INSERT OR IGNORE INTO settings VALUES (?1,?2,?3)",
            params![revision, encoded, now()],
        )?;
        tx.execute(
            "INSERT INTO selected_settings VALUES (1,?1) ON CONFLICT(singleton) DO UPDATE SET revision=excluded.revision",
            params![revision],
        )?;
        if let Some(assignments) = assignments.as_ref() {
            Self::apply_preparation_assignment_changes_tx(&tx, assignments)?;
        }
        Self::requeue_unfinished_tx(&tx, &revision, Some(obsolete))?;
        tx.commit()?;
        Ok(revision)
    }
    pub fn unfinished_settings(&self) -> Result<Vec<SettingsSnapshot>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT DISTINCT s.json FROM settings s JOIN jobs j ON j.settings_revision=s.revision JOIN tracks t ON t.current_job=j.id WHERE t.state='incoming' AND t.archived=0")?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|s| serde_json::from_str(&s).map_err(Into::into))
            .collect()
    }
    pub fn settings(&self) -> Result<SettingsSnapshot> {
        let conn = self.conn()?;
        let text:String=conn.query_row("SELECT s.json FROM settings s JOIN selected_settings a ON s.revision=a.revision WHERE a.singleton=1",[],|r|r.get(0))?;
        Ok(serde_json::from_str(&text)?)
    }
    pub fn settings_revision(&self, id: &str) -> Result<SettingsSnapshot> {
        let text: String =
            self.conn()?
                .query_row("SELECT json FROM settings WHERE revision=?1", [id], |r| {
                    r.get(0)
                })?;
        Ok(serde_json::from_str(&text)?)
    }
    pub fn revision(&self) -> Result<u64> {
        Ok(self
            .conn()?
            .query_row("SELECT value FROM meta WHERE key='revision'", [], |r| {
                r.get(0)
            })?)
    }
    pub(crate) fn bump(conn: &Connection) -> Result<()> {
        conn.execute("UPDATE meta SET value=value+1 WHERE key='revision'", [])?;
        Ok(())
    }
    pub fn tracks(&self, query: TrackQuery) -> Result<TrackPage> {
        crate::playlists::queries::tracks(self, query)
    }
    pub fn track_ids(&self, query: TrackQuery) -> Result<Vec<TrackId>> {
        crate::playlists::queries::track_ids(self, query)
    }
    pub fn track(&self, id: &str) -> Result<TrackSummary> {
        crate::import::projection::track_summary_tx(&*self.conn()?, id)
    }
    pub fn source_hash(&self, id: &str) -> Result<String> {
        self.conn()?.query_row("SELECT source_hash FROM tracks WHERE id=?1 UNION ALL SELECT COALESCE(source_hash,'') FROM import_items WHERE id=?1 AND state='imported' LIMIT 1", [id], |r| r.get::<_, Option<String>>(0))?.context("Track content hash is unknown")
    }
    pub fn counts(&self) -> Result<LibraryCounts> {
        Ok(self.conn()?.query_row(&format!("WITH {} SELECT COUNT(CASE WHEN state IN ('ready','imported') AND archived=0 THEN 1 END),COUNT(CASE WHEN state='incoming' AND archived=0 THEN 1 END),COUNT(CASE WHEN archived=1 THEN 1 END) FROM library_tracks",crate::import::projection::LIBRARY_TRACKS_CTE),[],|r|Ok(LibraryCounts{tracks:r.get(0)?,incoming:r.get(1)?,archive:r.get(2)?}))?)
    }
    pub fn track_path(&self, id: &str) -> Result<PathBuf> {
        let conn = self.conn()?;
        let local: Option<Option<Vec<u8>>> = conn
            .query_row("SELECT path FROM tracks WHERE id=?1", [id], |r| r.get(0))
            .optional()?;
        if let Some(bytes) = local {
            return bytes_path(
                &bytes.context("Track has no local file binding; relocate it first")?,
            );
        }
        crate::import::projection::imported_path(&conn, id)?
            .context("Imported media is unavailable; resolve its source location before playback")
    }
    /// Unlinked tracks and definitely absent files cannot be previewed. A bound
    /// file that cannot be inspected keeps the existing transient-error behavior.
    pub fn track_file_missing(&self, id: &str) -> Result<bool> {
        let conn = self.conn()?;
        let local: Option<Option<Vec<u8>>> = conn
            .query_row("SELECT path FROM tracks WHERE id=?1", [id], |r| r.get(0))
            .optional()?;
        let path = match local {
            Some(bytes) => bytes.as_deref().map(bytes_path).transpose()?,
            None => crate::import::projection::imported_path(&conn, id)?,
        };
        Ok(path.is_none_or(|path| matches!(path.try_exists(), Ok(false))))
    }
    pub fn analysis(&self, id: &str) -> Result<Option<camelot_core::FileAnalysis>> {
        let text: Option<String> = self
            .conn()?
            .query_row("SELECT CASE WHEN analysis_status='stale' THEN NULL ELSE analysis_json END FROM tracks WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .optional()?
            .flatten();
        text.map(|s| serde_json::from_str(&s).map_err(Into::into))
            .transpose()
    }
    pub fn cue_revision_count(&self, id: &str) -> Result<u64> {
        Ok(self.conn()?.query_row(
            "SELECT COUNT(*) FROM cue_revisions WHERE track_id=?1",
            [id],
            |r| r.get(0),
        )?)
    }

    pub fn set_archived(&self, id: &str, archived: bool) -> Result<()> {
        if self.archive_imported_reference(id, archived)? {
            return Ok(());
        }
        let current: bool =
            self.conn()?
                .query_row("SELECT archived FROM tracks WHERE id=?1", [id], |r| {
                    r.get(0)
                })?;
        if current == archived {
            return Ok(());
        }
        let _organization = self.inner.organization_gate.try_lock().map_err(|_| {
            anyhow::anyhow!("File publication is running; wait before changing archive status")
        })?;
        #[cfg(feature = "autotag")]
        let _tag = self.inner.tag_gate.try_lock().map_err(|_| {
            anyhow::anyhow!("File publication is running; wait before changing archive status")
        })?;
        let pending_publication: bool = self.conn()?.query_row(
            "SELECT EXISTS(SELECT 1 FROM metadata_edit_writes WHERE track_id=?1 AND state IN ('prepared','publishing','recovery_required','conflict') AND json_type(payload_json,'$.publication')='object')",
            [id],
            |row| row.get(0),
        )?;
        ensure!(
            !pending_publication,
            "Recover the pending metadata publication before changing archive status"
        );
        let restore = if archived {
            None
        } else {
            let path = self.track_path(id)?;
            let hash = cuerithm_media::file_sha256(&path);
            Some((path, hash, self.settings()?.revision))
        };
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        // A concurrent archive/restore may have completed while hashing the source.
        let current: bool = tx.query_row("SELECT archived FROM tracks WHERE id=?1", [id], |r| {
            r.get(0)
        })?;
        if current == archived {
            return Ok(());
        }
        ensure!(
            tx.execute(
                "UPDATE tracks SET archived=?1,updated=?2 WHERE id=?3",
                params![archived, now(), id]
            )? == 1,
            "Track is missing"
        );
        if archived {
            tx.execute("UPDATE jobs SET state='superseded' WHERE track_id=?1 AND state IN ('queued','running','attention')",[id])?;
        } else if let Some((path, hash, revision)) = restore {
            match hash {
                Ok(hash) => {
                    Self::enqueue_one(&tx, &path, &hash, &revision)?;
                }
                Err(error) => {
                    let hash: String =
                        tx.query_row("SELECT source_hash FROM tracks WHERE id=?1", [id], |r| {
                            r.get(0)
                        })?;
                    let job = Self::enqueue_one(&tx, &path, &hash, &revision)?;
                    tx.execute(
                        "UPDATE jobs SET state='attention',stage='waiting',error=?1 WHERE id=?2",
                        params![format!("Source unavailable: {error:#}"), job],
                    )?;
                    tx.execute("UPDATE tracks SET state='incoming' WHERE id=?1", [id])?;
                }
            }
        }
        Self::bump(&tx)?;
        tx.commit()?;
        Ok(())
    }
}

pub(crate) fn summary_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<TrackSummary> {
    let path: Option<Vec<u8>> = r.get(1)?;
    let path = path.as_deref().map(bytes_path).transpose().map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            1,
            rusqlite::types::Type::Blob,
            e.into_boxed_dyn_error(),
        )
    })?;
    let mut metadata: std::collections::BTreeMap<String, serde_json::Value> = r
        .get::<_, Option<String>>(12)?
        .map(|s| {
            serde_json::from_str::<cuerithm_media::MediaInfo>(&s)
                .map(|info| info.columns())
                .unwrap_or_else(|_| serde_json::from_str(&s).unwrap_or_default())
        })
        .unwrap_or_default();
    metadata.insert("library.path_bound".into(), path.is_some().into());
    metadata.insert(
        "library.content_hash_known".into(),
        r.get::<_, Option<String>>(11)?.is_some().into(),
    );
    metadata.insert("library.date_added".into(), r.get::<_, i64>(13)?.into());
    metadata.insert("library.date_modified".into(), r.get::<_, i64>(14)?.into());
    if let Some(status) = r.get::<_, Option<String>>(16)? {
        metadata.insert("library.tag_status".into(), status.into());
    }
    Ok(TrackSummary {
        import_state: None,
        metadata,
        id: r.get(0)?,
        source_sha256: r.get::<_, Option<String>>(11)?.unwrap_or_default(),
        display_path: path
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
        title: r.get(2)?,
        artist: r.get(3)?,
        album: r.get(4)?,
        genre: r.get(5)?,
        state: r.get(6)?,
        archived: r.get(15)?,
        key: r.get(7)?,
        bpm: r.get(8)?,
        energy: r.get(9)?,
        duration: r.get(10)?,
    })
}
