//! Library ownership and acknowledgement around staged native DJ transactions.
use crate::{DjSyncScope, Library};

/// How many applied syncs of one target stay restorable.
pub const RESTORE_LIMIT: usize = crate::REMOVAL_UNDO_LIMIT;
use anyhow::{Context, Result, ensure};
use cuerithm_dj_sync::{
    DjSoftware, SyncLibrary, SyncOptions, SyncPreview, SyncTarget,
    transaction::{FileJournal, FileTransaction, JournalState, file_hash},
};
use fs2::FileExt;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
};
mod organization;
mod ownership;
pub(crate) use organization::{KnownDjReference, OrganizationOwnershipTransition};
pub(crate) use ownership::import_media_volume_id;
#[cfg(test)]
mod mixxx_tests;
#[cfg(test)]
mod ownership_tests;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DjSyncRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration: Option<crate::SyncConfigurationBinding>,
    pub target: SyncTarget,
    pub options: SyncOptions,
    pub scope: DjSyncScope,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DjSyncHistoryEntry {
    pub id: String,
    pub state: Option<JournalState>,
    pub target: Option<SyncTarget>,
    pub tracks: usize,
    pub playlists: usize,
    pub created: Option<i64>,
    pub error: Option<String>,
    pub needs_acknowledgement: bool,
    pub needs_restore_acknowledgement: bool,
    /// True once the sync is older than the ten kept for restore: its backup was removed.
    #[serde(default)]
    pub restore_expired: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Source {
    pub(crate) id: String,
    #[serde(with = "cuerithm_dj_sync::path_serde")]
    pub(crate) path: PathBuf,
    pub(crate) hash: String,
}
#[derive(Serialize, Deserialize)]
struct LibraryJournal {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    target_root: Option<cuerithm_rekordbox::TargetPathSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    selected_target: Option<SyncTarget>,
    version: u32,
    id: String,
    revision: u64,
    request: DjSyncRequest,
    sources: Vec<Source>,
    track_hashes: BTreeMap<String, String>,
    playlist_hashes: BTreeMap<String, String>,
    replace_baseline: bool,
    created: i64,
    acknowledged: bool,
    error: Option<String>,
    #[serde(default)]
    ownership_before: ownership::OwnershipLedger,
    #[serde(default)]
    ownership_after: Option<ownership::OwnershipLedger>,
    #[serde(default)]
    ownership_restored: Option<ownership::OwnershipLedger>,
    #[serde(default)]
    restore_acknowledged: bool,
    #[serde(default)]
    preview_registration: String,
    /// Set when the backup copies were removed because newer syncs of the target took their place.
    #[serde(default)]
    restore_expired: bool,
}
#[derive(Default, Serialize, Deserialize)]
struct Baseline {
    version: u32,
    target: Option<SyncTarget>,
    journal_id: String,
    options_hash: String,
    track_hashes: BTreeMap<String, String>,
    source_hashes: BTreeMap<String, String>,
    playlist_hashes: BTreeMap<String, String>,
}
impl Library {
    pub(crate) fn native_sync_library(
        &self,
        scope: DjSyncScope,
        runtime: Option<&cuerithm_media::MediaRuntime>,
    ) -> Result<SyncLibrary> {
        let revision = self.revision()?;
        let mut library = self.dj_sync_library(scope)?;
        let sources = capture_sources(&*self.conn()?, &library)?;
        measure_export_profiles(
            &self.data_dir().join("cache/dj-timing"),
            &mut library,
            &sources,
            runtime,
        )?;
        ensure!(
            self.revision()? == revision,
            "Library changed while measuring native sync timing; preview again"
        );
        Ok(library)
    }

    /// Stage exact native bytes on copies, retaining revision and source ownership
    /// evidence next to the file transaction. This never publishes staged bytes.
    pub fn preview_dj_sync(&self, request: DjSyncRequest) -> Result<SyncPreview> {
        self.preview_dj_sync_internal(request, None)
    }

    /// Read optional cover art through the application's bundled native decoder.
    pub fn preview_dj_sync_with_runtime(
        &self,
        request: DjSyncRequest,
        runtime: &cuerithm_media::MediaRuntime,
    ) -> Result<SyncPreview> {
        self.preview_dj_sync_internal(request, Some(runtime))
    }

    fn preview_dj_sync_internal(
        &self,
        mut request: DjSyncRequest,
        runtime: Option<&cuerithm_media::MediaRuntime>,
    ) -> Result<SyncPreview> {
        let root = self.data_dir().join("sync");
        let _lease = service_lock(&root)?;
        let selected_target = request.target.clone();
        let target_root = cuerithm_rekordbox::TargetPathSnapshot::capture(&if selected_target
            .software
            == DjSoftware::Mixxx
        {
            // A library, or its whole settings folder, may not exist yet.
            cuerithm_dj_sync::mixxx::watched_folder(&selected_target.path)?
        } else if selected_target.path.is_dir() {
            selected_target.path.clone()
        } else {
            selected_target
                .path
                .parent()
                .context("DJ library parent missing")?
                .to_path_buf()
        })?;
        request.target = canonical_target(request.target)?;
        ensure!(
            request.target.software != DjSoftware::Mixxx
                || request.options.mixxx.library != cuerithm_dj_sync::MixxxLibrary::Overwrite
                || matches!(request.scope, DjSyncScope::Full),
            "Replacing the whole Mixxx library needs the full library; choose the full scope"
        );
        if let Some(binding) = &request.configuration {
            self.verify_sync_target_configuration(
                binding,
                &crate::SavedSyncTarget::DjLibrary {
                    target: request.target.clone(),
                },
            )?;
        }
        self.ensure_organization_references_settled(&request.target)?;
        let preview_registration = ownership::registration(&request.target)?;
        ownership::ensure_settled(&root, &request.target)?;
        let revision = self.revision()?;
        let is_changed = matches!(request.scope, DjSyncScope::Changed);
        let mut library = self.dj_sync_library(if is_changed {
            DjSyncScope::Full
        } else {
            request.scope.clone()
        })?;
        let snapshot = self.smart_snapshot(crate::now(), None, false)?;
        let mut attributes: BTreeMap<_, _> = snapshot
            .tracks
            .into_iter()
            .map(|row| (row.track.id, row.track.fields))
            .collect();
        const NATIVE_ATTRIBUTES: &[&str] = &[
            "composer",
            "remixer",
            "mix",
            "grouping",
            "extra1",
            "extra2",
            "bitrate",
            "sizeBytes",
            "trackNumber",
            "dateAdded",
            "fileType",
            "color",
        ];
        for track in &mut library.tracks {
            if let Some(mut fields) = attributes.remove(&track.id) {
                fields.retain(|key, _| NATIVE_ATTRIBUTES.contains(&key.as_str()));
                for key in NATIVE_ATTRIBUTES {
                    fields
                        .entry((*key).into())
                        .or_insert(cuerithm_smartlists::FieldValue::Null);
                }
                track.attributes = fields;
            }
        }
        request.options.smart_context = Some(cuerithm_dj_sync::SmartExportContext {
            time_zone: snapshot.zone,
            native_time_zone: iana_time_zone::get_timezone()
                .context("Cannot determine native smart playlist timezone")?,
            key_mixing_mode: snapshot.preferences.key_mixing_mode,
        });
        let mut sources = {
            let connection = self.conn()?;
            ensure!(
                revision_of(&connection)? == revision,
                "Library changed while preparing sync; preview again"
            );
            capture_sources(&connection, &library)?
        };
        measure_export_profiles(
            &self.data_dir().join("cache/dj-timing"),
            &mut library,
            &sources,
            runtime,
        )?;
        if request.options.add_album_art
            && matches!(
                request.target.software,
                DjSoftware::EngineDj | DjSoftware::PioneerUsb
            )
        {
            let runtime = runtime.context("Artwork export requires the bundled media runtime")?;
            for (track, source) in library.tracks.iter_mut().zip(&sources) {
                ensure!(track.id == source.id, "Artwork source identity mismatch");
                ensure!(
                    file_hash(&source.path)?.as_deref() == Some(source.hash.as_str()),
                    "Audio changed before artwork extraction"
                );
                track.artwork = runtime.artwork(&track.path)?.map(|artwork| artwork.bytes);
                ensure!(
                    file_hash(&source.path)?.as_deref() == Some(source.hash.as_str()),
                    "Audio changed during artwork extraction"
                );
            }
        }
        // Serialize and hash outside the library mutex; large analyses and source
        // audio must never lengthen the projection's database read critical section.
        let all_tracks = track_hashes(&mut library)?;
        let all_playlists = playlist_hashes(&library)?;
        let baseline = read_baseline(&root, &request.target)?;
        let ownership_before = ownership::read(&root, &request.target)?;
        ownership::check_registration(&ownership_before, &request.target)?;
        let ownership_contexts =
            ownership::contexts(&*self.conn()?, &ownership_before, &request.scope)?;
        if matches!(request.scope, DjSyncScope::Full | DjSyncScope::Changed)
            && let Some(baseline) = &baseline
        {
            ensure!(
                baseline
                    .playlist_hashes
                    .keys()
                    .all(|id| all_playlists.contains_key(id) || ownership_before.contains(id)),
                "A removed playlist has no native ownership record. No deletion was acknowledged; reconcile this older sync first"
            );
        }
        let options_hash = export_options_hash(&request.options)?;
        let replace_baseline = matches!(request.scope, DjSyncScope::Full)
            || (is_changed
                && baseline
                    .as_ref()
                    .is_none_or(|saved| saved.options_hash != options_hash));
        if is_changed
            && let Some(baseline) = baseline
                .as_ref()
                .filter(|saved| saved.options_hash == options_hash)
        {
            select_changed(
                &mut library,
                &sources,
                &all_tracks,
                &all_playlists,
                baseline,
            );
            let selected: BTreeSet<_> = library
                .tracks
                .iter()
                .map(|track| track.id.as_str())
                .collect();
            sources.retain(|source| selected.contains(source.id.as_str()));
        }
        for source in &sources {
            ensure!(
                file_hash(&source.path)?.as_deref() == Some(source.hash.as_str()),
                "Prepared audio changed since analysis; prepare '{}' again before sync",
                source.path.display()
            );
        }
        let preview = cuerithm_dj_sync::prepare_sync_owned_with_runtime(
            &root,
            request.target.clone(),
            &library,
            &request.options,
            runtime,
            &ownership_contexts,
            &self.protected_sync_sources()?,
        )?;
        request.target = preview.target.clone();
        let mut saved = LibraryJournal {
            target_root: Some(target_root),
            selected_target: Some(selected_target),
            version: 1,
            id: preview.id.clone(),
            revision,
            request,
            sources,
            track_hashes: track_hashes(&mut library)?,
            playlist_hashes: playlist_hashes(&library)?,
            replace_baseline,
            created: crate::now(),
            acknowledged: false,
            error: None,
            ownership_before,
            ownership_after: None,
            ownership_restored: None,
            restore_acknowledged: false,
            preview_registration,
            restore_expired: false,
        };
        saved.ownership_after = Some(saved.ownership_before.next(
            cuerithm_dj_sync::sync_ownership(&root, &preview.id)?,
            &preview.id,
        ));
        ensure!(
            ownership::registration(&saved.request.target)? == saved.preview_registration,
            "DJ target was replaced while preparing sync"
        );
        if self.revision()? != revision {
            saved.error = Some("Library changed while preparing sync; preview again".into());
        }
        let transaction = FileTransaction::open(&root, &preview.id)?;
        atomic_json(&transaction.path().join("library.json"), &saved)?;
        ensure!(
            saved.error.is_none(),
            "Library changed while preparing sync; preview again"
        );
        Ok(preview)
    }

    fn protected_sync_sources(&self) -> Result<BTreeSet<PathBuf>> {
        let connection = self.conn()?;
        let paths = connection
            .prepare("SELECT path FROM tracks WHERE path IS NOT NULL")?
            .query_map([], |row| row.get::<_, Vec<u8>>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        paths
            .iter()
            .map(|path| crate::model::bytes_path(path))
            .collect()
    }

    /// Discard staged bytes only while the native transaction remains Prepared.
    pub fn discard_dj_sync(&self, id: &str) -> Result<()> {
        let root = self.data_dir().join("sync");
        let _lease = service_lock(&root)?;
        FileTransaction::discard(&root, id)
    }

    /// Publish a reviewed preview only while its captured library revision and
    /// source identities still match. Baselines acknowledge verified applies only.
    pub fn apply_dj_sync(&self, id: &str) -> Result<FileJournal> {
        let root = self.data_dir().join("sync");
        let _lease = service_lock(&root)?;
        let transaction = FileTransaction::open(&root, id)?;
        let mut saved = read_library_journal(&transaction)?;
        verify_target_selection(&saved)?;
        self.ensure_organization_references_settled(&saved.request.target)?;
        cuerithm_dj_sync::verify_sync_environment(&root, id)?;
        let result = (|| -> Result<FileJournal> {
            if transaction.inspect()?.state != JournalState::Applied
                && let Some(binding) = &saved.request.configuration
            {
                self.verify_sync_target_configuration(
                    binding,
                    &crate::SavedSyncTarget::DjLibrary {
                        target: saved.request.target.clone(),
                    },
                )?;
            }
            #[cfg(feature = "autotag")]
            let _tag_gate = if saved.request.target.software == DjSoftware::Serato4 {
                Some(
                    self.inner
                        .tag_gate
                        .lock()
                        .map_err(|_| anyhow::anyhow!("Publication gate interrupted"))?,
                )
            } else {
                None
            };
            let mut connection = self.conn()?;
            let tx =
                connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let current_publication = transaction.inspect()?;
            let recovering = current_publication.state == JournalState::Applied;
            ensure!(
                revision_of(&tx)? == saved.revision
                    || (recovering
                        && ownership::recovered_revision(&tx, id)? == Some(revision_of(&tx)?)),
                "Library changed after sync preview; preview again"
            );
            ensure!(!saved.acknowledged, "Sync preview was already acknowledged");
            let installed = ownership::verify_generation(
                &root,
                &saved.request.target,
                &saved.ownership_before,
                saved.ownership_after.as_ref(),
            )?;
            if recovering && installed {
                ownership::check_registration(
                    &ownership::read(&root, &saved.request.target)?,
                    &saved.request.target,
                )?;
            }
            if !recovering {
                ownership::check_registration(&saved.ownership_before, &saved.request.target)?;
                ensure!(
                    saved.preview_registration.is_empty()
                        || ownership::registration(&saved.request.target)?
                            == saved.preview_registration,
                    "DJ target was replaced after preview"
                );
            }
            validate_sources(
                &tx,
                &saved.sources,
                recovering.then_some(&current_publication),
                true,
            )?;
            // Keep the library write transaction through external publication, as
            // tag publication does: a local edit cannot race the final revision check.
            let journal = if recovering {
                cuerithm_dj_sync::verify_sync_applied(&root, id)?
            } else {
                cuerithm_dj_sync::apply_sync(&root, id)?
            };
            let source_hashes = update_source_hashes(&tx, &saved.sources, &journal, false)?;
            tx.execute("INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![ownership::ack_key(id),revision_of(&tx)?])?;
            tx.commit()?;
            drop(connection);
            if let Some(next) = &mut saved.ownership_after {
                next.registration = ownership::registration(&saved.request.target)?;
                // Save the recoverable transition before installing the durable ledger.
                atomic_json(&transaction.path().join("library.json"), &saved)?;
                ownership::save(
                    &root,
                    &saved.request.target,
                    saved.ownership_after.as_ref().unwrap(),
                )?;
            }
            let mut baseline = if saved.replace_baseline {
                Baseline::default()
            } else {
                read_baseline(&root, &saved.request.target)?.unwrap_or_default()
            };
            let options = export_options_hash(&saved.request.options)?;
            if saved.replace_baseline
                || baseline.track_hashes.is_empty()
                || baseline.options_hash == options
            {
                baseline.options_hash = options;
            } else {
                // A partial sync with different options creates mixed target state;
                // make the next Changed sync republish the full current projection.
                baseline.options_hash.clear();
            }
            baseline.version = 1;
            baseline.target = Some(saved.request.target.clone());
            baseline.journal_id = id.to_owned();
            baseline.track_hashes.extend(saved.track_hashes.clone());
            baseline
                .playlist_hashes
                .extend(saved.playlist_hashes.clone());
            if let Some(next) = &saved.ownership_after {
                for tombstone in &next.tombstones {
                    if tombstone.journal == id {
                        baseline.playlist_hashes.remove(&tombstone.source_id);
                    }
                }
            }
            baseline.source_hashes.extend(source_hashes);
            let path = baseline_path(&root, &saved.request.target)?;
            fs::create_dir_all(path.parent().context("Baseline folder missing")?)?;
            atomic_json(&path, &baseline)?;
            saved.acknowledged = true;
            Ok(journal)
        })();
        let outcome = finish(&transaction, &mut saved, result);
        if outcome.is_ok() {
            let _ = self.prune_dj_sync_locked(&root, RESTORE_LIMIT);
        }
        outcome
    }

    /// Restore verified backups, reconcile Serato's tag hashes, and invalidate
    /// acknowledgement for the whole target so its next Changed sync is complete.
    pub fn restore_dj_sync(&self, id: &str) -> Result<FileJournal> {
        let root = self.data_dir().join("sync");
        let _lease = service_lock(&root)?;
        let transaction = FileTransaction::open(&root, id)?;
        let mut saved = read_library_journal(&transaction)?;
        // A sync that has been restored needs no backup to finish or repeat its restore.
        ensure!(
            !saved.restore_expired || transaction.inspect()?.state == JournalState::Restored,
            "This sync is older than the {} kept for restore, so its backup was removed",
            RESTORE_LIMIT
        );
        verify_target_selection(&saved)?;
        self.ensure_organization_references_settled(&saved.request.target)?;
        cuerithm_dj_sync::verify_sync_environment(&root, id)?;
        let result = (|| -> Result<FileJournal> {
            let current = transaction.inspect()?;
            let ledger = ownership::read(&root, &saved.request.target)?;
            let installed_restore = if let Some(prior) = &saved.ownership_restored {
                digest(prior)? == digest(&ledger)?
            } else {
                false
            };
            ensure!(
                ledger.last_journal == id
                    || digest(&ledger)? == digest(&saved.ownership_before)?
                    || installed_restore,
                "A later sync owns this target; restore that sync first"
            );
            if installed_restore
                || (current.state == JournalState::Applied && ledger.last_journal == id)
            {
                ownership::check_registration(&ledger, &saved.request.target)?;
            }
            #[cfg(feature = "autotag")]
            let _tag_gate = if saved.request.target.software == DjSoftware::Serato4 {
                Some(
                    self.inner
                        .tag_gate
                        .lock()
                        .map_err(|_| anyhow::anyhow!("Publication gate interrupted"))?,
                )
            } else {
                None
            };
            let mut connection = self.conn()?;
            let tx =
                connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            // Restoring a backup depends on source identity, not whether the
            // track is still eligible to be included in a new sync.
            validate_sources(&tx, &saved.sources, Some(&current), false)?;
            // Clear even before a partial recovery: no old baseline can describe
            // a target whose previous publication/restoration was interrupted.
            invalidate_baseline(&root, &saved.request.target)?;
            let journal = if current.state == JournalState::Restored {
                cuerithm_dj_sync::verify_sync_restored(&root, id)?
            } else {
                cuerithm_dj_sync::restore_sync(&root, id)?
            };
            update_source_hashes(&tx, &saved.sources, &journal, true)?;
            tx.execute("DELETE FROM meta WHERE key=?1", [ownership::ack_key(id)])?;
            tx.commit()?;
            if !saved.ownership_before.stores.is_empty() || saved.ownership_after.is_some() {
                let mut prior = if installed_restore {
                    saved.ownership_restored.clone().unwrap()
                } else {
                    saved.ownership_before.clone()
                };
                prior.version = 1;
                if !installed_restore {
                    prior.generation = ledger.generation.max(prior.generation) + 1;
                }
                for event in ledger.tombstones {
                    if !prior.tombstones.contains(&event) {
                        prior.tombstones.push(event);
                    }
                }
                prior.registration = ownership::registration(&saved.request.target)?;
                saved.ownership_restored = Some(prior.clone());
                atomic_json(&transaction.path().join("library.json"), &saved)?;
                ownership::save(&root, &saved.request.target, &prior)?;
            }
            saved.acknowledged = false;
            saved.restore_acknowledged = true;
            Ok(journal)
        })();
        let outcome = finish(&transaction, &mut saved, result);
        if outcome.is_ok() {
            let _ = self.prune_dj_sync_locked(&root, RESTORE_LIMIT);
        }
        outcome
    }

    /// Frees the backups of finished syncs beyond the newest ten of each target. Run when the
    /// library opens; applying and restoring a sync do the same when they finish.
    pub(crate) fn prune_dj_sync_history(&self) -> Result<u64> {
        let root = self.data_dir().join("sync");
        if !root.exists() {
            return Ok(0);
        }
        let _lease = service_lock(&root)?;
        self.prune_dj_sync_locked(&root, RESTORE_LIMIT)
    }

    /// The bound itself, with the service lock held. A restored sync can never be restored
    /// again and gives up its copies at once; an applied one keeps them while it is among the
    /// newest `limit` of its target. A sync that is not settled (not yet acknowledged) is never
    /// touched.
    fn prune_dj_sync_locked(&self, root: &Path, limit: usize) -> Result<u64> {
        struct Finished {
            id: String,
            target: String,
            created: i64,
            restored: bool,
            expired: bool,
        }
        let mut finished = vec![];
        for entry in fs::read_dir(root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let id = entry.file_name().to_string_lossy().into_owned();
            if uuid::Uuid::parse_str(&id).is_ok_and(|parsed| parsed.to_string() == id) {
                let Ok(transaction) = FileTransaction::open(root, &id) else {
                    continue;
                };
                let (Ok(journal), Ok(saved)) =
                    (transaction.inspect(), read_library_journal(&transaction))
                else {
                    continue;
                };
                let settled = match journal.state {
                    JournalState::Applied => saved.acknowledged && saved.error.is_none(),
                    JournalState::Restored => saved.restore_acknowledged,
                    _ => false,
                };
                if settled {
                    finished.push(Finished {
                        target: digest(&saved.request.target)?,
                        created: saved.created,
                        restored: journal.state == JournalState::Restored,
                        expired: saved.restore_expired,
                        id,
                    });
                }
            }
        }
        finished.sort_by(|a, b| b.created.cmp(&a.created).then_with(|| a.id.cmp(&b.id)));
        let mut newest: BTreeMap<String, usize> = BTreeMap::new();
        let (mut freed, mut handled) = (0, 0);
        for item in finished {
            let beyond = if item.restored {
                true
            } else {
                let rank = newest.entry(item.target).or_default();
                *rank += 1;
                *rank > limit
            };
            if !beyond || item.expired || handled >= 50 {
                continue;
            }
            handled += 1;
            // One that cannot be cleaned now is tried again at the next pass.
            let Ok(bytes) = FileTransaction::expire_payload(root, &item.id) else {
                continue;
            };
            freed += bytes;
            let transaction = FileTransaction::open(root, &item.id)?;
            let mut saved = read_library_journal(&transaction)?;
            saved.restore_expired = true;
            atomic_json(&transaction.path().join("library.json"), &saved)?;
        }
        Ok(freed)
    }

    /// Include interrupted transactions and malformed journals in history instead
    /// of hiding recovery evidence. All reported paths remain in local storage.
    pub fn history_dj_sync(&self) -> Result<Vec<DjSyncHistoryEntry>> {
        let root = self.data_dir().join("sync");
        if !root.exists() {
            return Ok(Vec::new());
        }
        let _lease = service_lock(&root)?;
        let mut entries = Vec::new();
        for entry in fs::read_dir(&root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let id = entry.file_name().to_string_lossy().into_owned();
            if uuid::Uuid::parse_str(&id).is_err() {
                continue;
            }
            let mut item = DjSyncHistoryEntry {
                id: id.clone(),
                state: None,
                target: None,
                tracks: 0,
                playlists: 0,
                created: None,
                error: None,
                needs_acknowledgement: false,
                needs_restore_acknowledgement: false,
                restore_expired: false,
            };
            match FileTransaction::open(&root, &id) {
                Ok(transaction) => {
                    item.state = Some(transaction.inspect()?.state);
                    match cuerithm_dj_sync::sync_preview(&root, &id) {
                        Ok(preview) => {
                            item.target = Some(preview.target);
                            item.tracks = preview.tracks;
                            item.playlists = preview.playlists;
                        }
                        Err(error) => item.error = Some(format!("{error:#}")),
                    }
                    match read_library_journal(&transaction) {
                        Ok(saved) => {
                            item.needs_acknowledgement =
                                item.state == Some(JournalState::Applied) && !saved.acknowledged;
                            item.needs_restore_acknowledgement = item.state
                                == Some(JournalState::Restored)
                                && !saved.restore_acknowledged;
                            item.restore_expired = saved.restore_expired;
                            item.created = Some(saved.created);
                            if saved.error.is_some() {
                                item.error = saved.error;
                            }
                        }
                        Err(error) => item.error = Some(format!("{error:#}")),
                    }
                }
                Err(error) => item.error = Some(format!("{error:#}")),
            }
            entries.push(item);
        }
        entries.sort_by(|a, b| b.created.cmp(&a.created).then_with(|| a.id.cmp(&b.id)));
        Ok(entries)
    }
}
fn select_changed(
    library: &mut SyncLibrary,
    sources: &[Source],
    tracks: &BTreeMap<String, String>,
    playlists: &BTreeMap<String, String>,
    baseline: &Baseline,
) {
    let mut selected: BTreeSet<_> = tracks
        .iter()
        .filter(|(id, hash)| baseline.track_hashes.get(*id) != Some(*hash))
        .map(|(id, _)| id.clone())
        .collect();
    selected.extend(
        sources
            .iter()
            .filter(|source| {
                baseline.source_hashes.get(&source.id).map(String::as_str)
                    != Some(source.hash.as_str())
            })
            .map(|source| source.id.clone()),
    );
    let mut selected_playlists: BTreeSet<_> = playlists
        .iter()
        .filter(|(id, hash)| baseline.playlist_hashes.get(*id) != Some(*hash))
        .map(|(id, _)| id.clone())
        .collect();
    if !selected_playlists.is_empty() {
        // Native path references can include unchanged descendants. Carry the
        // complete tree when any playlist changes so ancestor moves are coherent.
        selected_playlists.extend(playlists.keys().cloned());
    }
    // Carry complete membership for every changed playlist, plus all ancestors.
    // A folder carries hierarchy only; its unchanged descendants need no rewrite.
    loop {
        let parents: Vec<_> = library
            .playlists
            .iter()
            .filter(|playlist| selected_playlists.contains(&playlist.id))
            .filter_map(|playlist| playlist.parent_id.clone())
            .filter(|id| !selected_playlists.contains(id))
            .collect();
        if parents.is_empty() {
            break;
        }
        selected_playlists.extend(parents);
    }
    library
        .playlists
        .retain(|playlist| selected_playlists.contains(&playlist.id));
    selected.extend(
        library
            .playlists
            .iter()
            .flat_map(|playlist| playlist.track_ids.iter().cloned()),
    );
    library.tracks.retain(|track| selected.contains(&track.id));
}
fn digest(value: &impl Serialize) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(value)?)))
}
fn export_options_hash(options: &SyncOptions) -> Result<String> {
    digest(&(options, cuerithm_dj_sync::implementation_sha256()))
}
pub(crate) fn measure_export_profiles(
    cache: &Path,
    library: &mut SyncLibrary,
    sources: &[Source],
    runtime: Option<&cuerithm_media::MediaRuntime>,
) -> Result<()> {
    for (track, source) in library.tracks.iter_mut().zip(sources) {
        ensure!(
            track.id == source.id,
            "Timing profile source identity mismatch"
        );
        let extension = track
            .path
            .extension()
            .and_then(|v| v.to_str())
            .unwrap_or("")
            .to_lowercase();
        let needed = matches!(extension.as_str(), "mp3" | "m4a" | "mp4");
        let reusable = track.export_timing.as_ref().is_some_and(|profile| {
            profile.profile == "dj-timeline-v1"
                && profile.source_sha256 == source.hash
                && profile.source_sample_rate == track.sample_rate
                && (!needed || runtime.is_some())
                && runtime.is_none_or(|runtime| {
                    profile.measurement_identity == runtime.export_timing_identity()
                })
                && match &profile.codec_profile {
                    cuerithm_media::CodecTimingProfile::Mp3 { .. } => extension == "mp3",
                    cuerithm_media::CodecTimingProfile::IsoAudio {
                        extension: measured,
                        ..
                    } => measured == &extension,
                    cuerithm_media::CodecTimingProfile::Direct => !needed,
                    cuerithm_media::CodecTimingProfile::Unmeasured { .. } => false,
                }
        });
        if reusable {
            continue;
        }
        if let Some(runtime) = runtime {
            ensure!(
                file_hash(&source.path)?.as_deref() == Some(source.hash.as_str()),
                "Audio changed before timing capture"
            );
            fs::create_dir_all(cache)?;
            let snapshot = tempfile::Builder::new()
                .prefix("timing-")
                .tempdir_in(cache)?;
            let mut path = snapshot.path().join("source");
            if !extension.is_empty() {
                path.set_extension(&extension);
            }
            fs::copy(&source.path, &path)?;
            ensure!(
                file_hash(&path)?.as_deref() == Some(source.hash.as_str()),
                "Audio changed during timing capture"
            );
            let info = runtime.probe(&path)?;
            track.export_timing = Some(runtime.export_timing(&path, &source.hash, &info)?);
            ensure!(
                file_hash(&source.path)?.as_deref() == Some(source.hash.as_str()),
                "Audio changed during timing measurement"
            );
        } else if needed {
            // MP3 classification is a native bounded read, but the public service
            // requires the same hash-verified runtime identity as prepared analysis.
            // Native facade conversion refuses unknown timing where it matters.
            track.export_timing = None;
        }
    }
    Ok(())
}
fn track_hashes(library: &mut SyncLibrary) -> Result<BTreeMap<String, String>> {
    library
        .tracks
        .iter_mut()
        .map(|track| {
            // Hash cover bytes directly instead of expanding each byte into JSON.
            // Reinstate the same buffer before returning, including serialization errors.
            let artwork = track.artwork.take();
            let artwork_hash = artwork
                .as_ref()
                .map(|bytes| format!("{:x}", Sha256::digest(bytes)));
            // File identity is acknowledged separately after Serato tag writes;
            // changing only that binding must not trigger endless re-exports.
            let timing = track.export_timing.take();
            let timing_semantic = timing.as_ref().map(|timing| {
                let mut profile = timing.clone();
                profile.source_sha256.clear();
                profile
            });
            let hash = digest(&(&*track, artwork_hash, timing_semantic));
            track.export_timing = timing;
            track.artwork = artwork;
            Ok((track.id.clone(), hash?))
        })
        .collect()
}
fn playlist_hashes(library: &SyncLibrary) -> Result<BTreeMap<String, String>> {
    library
        .playlists
        .iter()
        .enumerate()
        .map(|(position, playlist)| Ok((playlist.id.clone(), digest(&(position, playlist))?)))
        .collect()
}
fn verify_target_selection(saved: &LibraryJournal) -> Result<()> {
    if let Some(root) = &saved.target_root {
        root.verify()?;
    }
    if let Some(selected) = &saved.selected_target {
        let current = canonical_target(selected.clone())?;
        ensure!(
            current.path == saved.request.target.path
                && current.source_root == saved.request.target.source_root,
            "Selected DJ library location changed after preview"
        );
    }
    Ok(())
}
pub(crate) fn canonical_target(mut target: SyncTarget) -> Result<SyncTarget> {
    target.environment.validate()?;
    if target.software == DjSoftware::Mixxx {
        // A Mixxx that was never started has no library and maybe no folder yet; the
        // identity is the canonical place the library is or will be.
        target.path = cuerithm_dj_sync::mixxx::canonical_database_path(&target.path)?;
        target.source_root = target.source_root.map(fs::canonicalize).transpose()?;
        return Ok(target);
    }
    target.path = fs::canonicalize(target.path).context("DJ target was not found")?;
    // Normalize file/folder spellings so both selections share one durable baseline.
    target.path = match target.software {
        DjSoftware::VirtualDj if target.path.is_dir() => target.path.join("database.xml"),
        DjSoftware::DjayPro if target.path.is_dir() => target.path.join("MediaLibrary.db"),
        DjSoftware::EngineDj if target.path.is_dir() => {
            cuerithm_dj_sync::engine_dj::resolve_database_path(&target.path)?
        }
        DjSoftware::Serato4 if target.path.is_dir() => target.path.join("root.sqlite"),
        _ => target.path,
    };
    target.path = fs::canonicalize(target.path).context("DJ database was not found")?;
    target.source_root = target.source_root.map(fs::canonicalize).transpose()?;
    Ok(target)
}
fn baseline_path(root: &Path, target: &SyncTarget) -> Result<PathBuf> {
    Ok(root
        .join("baselines")
        .join(format!("{}.json", digest(&target.identity_target())?)))
}
fn read_baseline(root: &Path, target: &SyncTarget) -> Result<Option<Baseline>> {
    let path = baseline_path(root, target)?;
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let baseline: Baseline =
        serde_json::from_reader(file).context("Read acknowledged DJ baseline")?;
    ensure!(baseline.version == 1, "Unsupported DJ baseline version");
    ensure!(
        digest(
            &baseline
                .target
                .as_ref()
                .context("Baseline target missing")?
                .identity_target()
        )? == digest(&target.identity_target())?,
        "DJ baseline target mismatch"
    );
    Ok(Some(baseline))
}
fn invalidate_baseline(root: &Path, target: &SyncTarget) -> Result<()> {
    let path = baseline_path(root, target)?;
    match fs::remove_file(&path) {
        Ok(()) => sync_parent(&path)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}
fn revision_of(connection: &Connection) -> Result<u64> {
    Ok(
        connection.query_row("SELECT value FROM meta WHERE key='revision'", [], |row| {
            row.get(0)
        })?,
    )
}
pub(crate) fn capture_sources(
    connection: &Connection,
    library: &SyncLibrary,
) -> Result<Vec<Source>> {
    let mut query = connection.prepare(
        "SELECT path,source_hash FROM tracks WHERE id=?1 AND state='ready' AND archived=0",
    )?;
    library
        .tracks
        .iter()
        .map(|track| {
            let (path, hash): (Vec<u8>, String) =
                query.query_row([&track.id], |row| Ok((row.get(0)?, row.get(1)?)))?;
            let path = crate::model::bytes_path(&path)?;
            ensure!(
                path == track.path,
                "Track source changed while preparing sync"
            );
            Ok(Source {
                id: track.id.clone(),
                path,
                hash,
            })
        })
        .collect()
}
fn validate_sources(
    connection: &Connection,
    sources: &[Source],
    journal: Option<&FileJournal>,
    require_prepared: bool,
) -> Result<()> {
    for source in sources {
        let row: Option<(Vec<u8>, String, bool)> = connection
            .query_row(
                "SELECT path,source_hash,state='ready' AND archived=0 FROM tracks WHERE id=?1",
                [&source.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let (path, hash, prepared) = row.context("A synced track is no longer in the library")?;
        ensure!(
            !require_prepared || prepared,
            "A synced track is no longer prepared"
        );
        ensure!(
            crate::model::bytes_path(&path)? == source.path,
            "A synced source moved after preview"
        );
        let after = journal
            .and_then(|journal| {
                journal
                    .changes
                    .iter()
                    .find(|change| change.target == source.path)
            })
            .and_then(|change| change.after_sha256.as_ref());
        ensure!(
            hash == source.hash || after == Some(&hash),
            "A synced source identity changed after preview"
        );
        if !require_prepared {
            // Reject later audio edits before restoring any destination files.
            // An interrupted tag restore may contain either journal-owned image.
            let actual = match journal {
                Some(journal) => {
                    cuerithm_dj_sync::transaction::observed_file_hash(journal, &source.path)?
                }
                None => file_hash(&source.path)?,
            };
            ensure!(
                actual.as_deref() == Some(source.hash.as_str())
                    || after.is_some_and(|expected| actual.as_ref() == Some(expected)),
                "A synced source changed before restore"
            );
        }
    }
    Ok(())
}
fn update_source_hashes(
    connection: &Connection,
    sources: &[Source],
    journal: &FileJournal,
    restore: bool,
) -> Result<BTreeMap<String, String>> {
    let mut hashes = BTreeMap::new();
    let mut changed = false;
    for source in sources {
        let change = journal
            .changes
            .iter()
            .find(|change| change.target == source.path);
        let hash = if restore {
            source.hash.clone()
        } else {
            change
                .and_then(|change| change.after_sha256.clone())
                .unwrap_or_else(|| source.hash.clone())
        };
        if change.is_some() {
            let count = connection.execute(
                "UPDATE tracks SET source_hash=?1,updated=?2 WHERE id=?3 AND source_hash!=?1",
                params![hash, crate::now(), source.id],
            )?;
            if count != 0 {
                let text: Option<String> = connection.query_row(
                    "SELECT analysis_json FROM tracks WHERE id=?1",
                    [&source.id],
                    |row| row.get(0),
                )?;
                if let Some(text) = text {
                    let mut analysis: serde_json::Value = serde_json::from_str(&text)?;
                    if let Some(media) = analysis
                        .get_mut("media")
                        .and_then(serde_json::Value::as_object_mut)
                    {
                        media.insert("source_sha256".into(), hash.clone().into());
                        if let Some(profile) = media
                            .get_mut("export_timing")
                            .and_then(serde_json::Value::as_object_mut)
                        {
                            profile.insert("source_sha256".into(), hash.clone().into());
                        }
                        connection.execute(
                            "UPDATE tracks SET analysis_json=?1 WHERE id=?2",
                            params![serde_json::to_string(&analysis)?, source.id],
                        )?;
                    }
                }
            }
            changed |= count != 0;
        }
        hashes.insert(source.id.clone(), hash);
    }
    if changed {
        Library::bump(connection)?;
    }
    Ok(hashes)
}
fn read_library_journal(transaction: &FileTransaction) -> Result<LibraryJournal> {
    let saved: LibraryJournal =
        serde_json::from_reader(File::open(transaction.path().join("library.json"))?)
            .context("Read library sync ownership journal")?;
    ensure!(
        saved.version == 1 && saved.id == transaction.inspect()?.id,
        "Unsupported library sync journal"
    );
    Ok(saved)
}
fn finish<T>(
    transaction: &FileTransaction,
    saved: &mut LibraryJournal,
    result: Result<T>,
) -> Result<T> {
    saved.error = result.as_ref().err().map(|error| format!("{error:#}"));
    let persist = atomic_json(&transaction.path().join("library.json"), saved);
    match (result, persist) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(error
            .context("Sync completed but its library acknowledgement journal could not be saved")),
        (Err(error), Err(persist)) => {
            Err(error.context(format!("Could not persist sync error: {persist:#}")))
        }
    }
}
pub(crate) fn service_lock(root: &Path) -> Result<File> {
    fs::create_dir_all(root)?;
    let lock = File::options()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join("service.lock"))?;
    // Other threads can fork process inspection while this descriptor is open.
    // A child briefly retains the open-file-description lock until exec closes
    // its inherited descriptors, even after the owning operation returns.
    let started = std::time::Instant::now();
    loop {
        match lock.try_lock_exclusive() {
            Ok(()) => return Ok(lock),
            Err(error)
                if error.raw_os_error() == fs2::lock_contended_error().raw_os_error()
                    && started.elapsed() < std::time::Duration::from_millis(250) =>
            {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Err(error) => return Err(error).context("Another DJ sync operation is running"),
        }
    }
}
fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut temporary =
        tempfile::NamedTempFile::new_in(path.parent().context("Sync journal parent missing")?)?;
    serde_json::to_writer(&mut temporary, value)?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    sync_parent(path)
}
fn sync_parent(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path.parent().context("Sync journal parent missing")?)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cuerithm_media::MediaInfo;
    use serde_json::json;

    pub(super) fn fixture() -> (tempfile::TempDir, Library, SyncTarget) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let library = Library::open(&root.join("library.db")).unwrap();
        let source = root.join("authored.wav");
        fs::write(
            &source,
            b"Synthetic source bytes: no decoding or DSP in XML sync",
        )
        .unwrap();
        let source_hash = file_hash(&source).unwrap().unwrap();
        let media = MediaInfo {
            codec: "pcm_s16le".into(),
            sample_rate: 48000,
            channels: 2,
            bits_per_sample: Some(16),
            tags: BTreeMap::new(),
            properties: BTreeMap::new(),
        };
        let analysis = json!({"source_sample_rate":48000,"publication":{
            "grid":{"bpm":120.0,"startTime":0.125},"cues":[],
            "implementation_sha256":"synthetic","inputs_sha256":"synthetic"}});
        library.conn().unwrap().execute(
            "INSERT INTO tracks(id,path,title,artist,album,genre,source_hash,state,analysis_json,metadata_json,musical_key,bpm,duration,created,updated,sort_title)
             VALUES('a',?1,'Authored','Artist','Album','House',?2,'ready',?3,?4,'8A',120,120,1,1,'authored')",
            params![crate::model::path_bytes(&source),source_hash,analysis.to_string(),serde_json::to_string(&media).unwrap()]).unwrap();
        let home = root.join("VirtualDJ");
        fs::create_dir(&home).unwrap();
        fs::create_dir(home.join("MyLists")).unwrap();
        let database = home.join("database.xml");
        fs::write(&database, b"<VirtualDJ_Database Version=\"8.5\"/>").unwrap();
        let target = SyncTarget {
            environment: Default::default(),
            software: DjSoftware::VirtualDj,
            path: database,
            source_root: None,
        };
        (dir, library, target)
    }
    pub(super) fn request(target: &SyncTarget, scope: DjSyncScope) -> DjSyncRequest {
        DjSyncRequest {
            configuration: None,
            target: target.clone(),
            options: SyncOptions::default(),
            scope,
        }
    }
    fn windows_target(target: &SyncTarget, root: &Path) -> SyncTarget {
        let mut json = serde_json::to_value(target).unwrap();
        json["environment"] = json!({
            "kind":"windows_compatibility", "prefix":root,
            "installation":null,
            "mappings":[{"windows_root":"C:\\", "host_root":root}]
        });
        serde_json::from_value(json).unwrap()
    }
    #[test]
    fn mapped_sync_ignores_unrelated_unbound_portable_track() {
        let (_dir, library, target) = fixture();
        let mut unresolved = library.portable_tracks().unwrap().remove(0);
        unresolved.id = "unresolved".into();
        unresolved.locator.id = "locator:unresolved".into();
        unresolved.source_sha256 = None;
        unresolved.musical = None;
        unresolved
            .metadata
            .insert("comment".into(), json!("Retain imported metadata"));
        library
            .import_portable_tracks(&[unresolved.clone()])
            .unwrap();
        let jobs = library.jobs().unwrap().len();
        let preview = library
            .preview_dj_sync(request(
                &target,
                DjSyncScope::Modified {
                    track_ids: vec!["a".into()],
                },
            ))
            .unwrap();
        assert_eq!(preview.tracks, 1);
        library.apply_dj_sync(&preview.id).unwrap();
        assert!(
            fs::read_to_string(&target.path)
                .unwrap()
                .contains("Authored")
        );
        assert_eq!(library.local_track_binding("unresolved").unwrap(), None);
        let after = library
            .portable_tracks()
            .unwrap()
            .into_iter()
            .find(|t| t.id == "unresolved")
            .unwrap();
        assert_eq!(after.metadata, unresolved.metadata);
        assert_eq!(after.locator, unresolved.locator);
        assert_eq!(library.jobs().unwrap().len(), jobs);
    }
    #[test]
    fn windows_context_service_preserves_reference_and_foreign_tracks_then_noops_and_restores() {
        let (dir, library, native) = fixture();
        let root = dir.path().canonicalize().unwrap();
        let target = windows_target(&native, &root);
        let original = br#"<VirtualDJ_Database Version="8.5"><Song FilePath="c:/AUTHORED.wav" Custom="preserve"/><Song FilePath="D:\foreign.wav"/></VirtualDJ_Database>"#;
        fs::write(&target.path, original).unwrap();
        let source = library.track_path("a").unwrap();
        let before = fs::read(&source).unwrap();
        let preview = library
            .preview_dj_sync(request(&target, DjSyncScope::Full))
            .unwrap();
        library.apply_dj_sync(&preview.id).unwrap();
        let xml = fs::read_to_string(&target.path).unwrap();
        assert_eq!(
            xml.matches("<Song ").count(),
            2,
            "reuse equivalent Windows reference"
        );
        assert!(xml.contains("c:/AUTHORED.wav"));
        assert!(xml.contains("D:\\foreign.wav"));
        assert!(!xml.contains(root.to_str().unwrap()));
        assert_eq!(fs::read(&source).unwrap(), before);
        let repeated = library
            .preview_dj_sync(request(&target, DjSyncScope::Changed))
            .unwrap();
        assert_eq!(repeated.tracks, 0);
        library.discard_dj_sync(&repeated.id).unwrap();
        library.restore_dj_sync(&preview.id).unwrap();
        assert_eq!(fs::read(&target.path).unwrap(), original);
    }
    #[cfg(unix)]
    #[test]
    fn windows_context_service_refuses_changed_mapping_at_apply_recovery_and_restore() {
        use std::os::unix::fs::symlink;
        let (dir, library, native) = fixture();
        let root = dir.path().canonicalize().unwrap();
        let alias = root.join("mapped-root");
        let replacement = root.join("replacement");
        fs::create_dir(&replacement).unwrap();
        symlink(&root, &alias).unwrap();
        let mut json = serde_json::to_value(windows_target(&native, &root)).unwrap();
        json["environment"]["mappings"][0]["host_root"] = json!(&alias);
        let target: SyncTarget = serde_json::from_value(json).unwrap();
        let preview = library
            .preview_dj_sync(request(&target, DjSyncScope::Full))
            .unwrap();
        fs::remove_file(&alias).unwrap();
        symlink(&replacement, &alias).unwrap();
        assert!(library.apply_dj_sync(&preview.id).is_err());
        fs::remove_file(&alias).unwrap();
        symlink(&root, &alias).unwrap();
        cuerithm_dj_sync::apply_sync(&library.data_dir().join("sync"), &preview.id).unwrap();
        fs::remove_file(&alias).unwrap();
        symlink(&replacement, &alias).unwrap();
        assert!(library.apply_dj_sync(&preview.id).is_err());
        assert!(library.restore_dj_sync(&preview.id).is_err());
        fs::remove_file(&alias).unwrap();
        symlink(&root, &alias).unwrap();
        library.restore_dj_sync(&preview.id).unwrap();
    }
    #[test]
    fn captured_windows_configuration_changes_block_apply_but_not_captured_recovery_or_restore() {
        let (dir, library, native) = fixture();
        let target = windows_target(&native, &dir.path().canonicalize().unwrap());
        let saved = library
            .save_sync_target_configuration(crate::SaveSyncTargetConfiguration {
                id: None,
                expected_revision: None,
                name: "Decks".into(),
                target: crate::SavedSyncTarget::DjLibrary {
                    target: target.clone(),
                },
            })
            .unwrap();
        let mut selected = request(&target, DjSyncScope::Full);
        selected.configuration = Some(crate::SyncConfigurationBinding {
            id: saved.id.clone(),
            revision: 1,
        });
        let preview = library.preview_dj_sync(selected).unwrap();
        let before = fs::read(&target.path).unwrap();
        library
            .save_sync_target_configuration(crate::SaveSyncTargetConfiguration {
                id: Some(saved.id.clone()),
                expected_revision: Some(1),
                name: "Updated decks".into(),
                target: saved.target,
            })
            .unwrap();
        assert!(library.apply_dj_sync(&preview.id).is_err());
        assert_eq!(fs::read(&target.path).unwrap(), before);
        library.discard_dj_sync(&preview.id).unwrap();
        let mut selected = request(&target, DjSyncScope::Full);
        selected.configuration = Some(crate::SyncConfigurationBinding {
            id: saved.id.clone(),
            revision: 2,
        });
        let preview = library.preview_dj_sync(selected).unwrap();
        cuerithm_dj_sync::apply_sync(&library.data_dir().join("sync"), &preview.id).unwrap();
        library
            .delete_sync_target_configuration(&saved.id, 2)
            .unwrap();
        library.apply_dj_sync(&preview.id).unwrap();
        library.restore_dj_sync(&preview.id).unwrap();
        assert_eq!(fs::read(&target.path).unwrap(), before);
    }
    #[test]
    fn old_native_journal_without_environment_or_configuration_still_recovers_and_restores() {
        let (_dir, library, target) = fixture();
        let before = fs::read(&target.path).unwrap();
        let preview = library
            .preview_dj_sync(request(&target, DjSyncScope::Full))
            .unwrap();
        let operation = library.data_dir().join("sync").join(&preview.id);
        let request_path = operation.join("request.json");
        let mut old: serde_json::Value =
            serde_json::from_reader(File::open(&request_path).unwrap()).unwrap();
        for field in [
            "environment",
            "target_root",
            "selected_target",
            "related_roots",
        ] {
            old.as_object_mut().unwrap().remove(field);
        }
        assert!(old["target"].get("environment").is_none());
        atomic_json(&request_path, &old).unwrap();
        let library_path = operation.join("library.json");
        let mut old: serde_json::Value =
            serde_json::from_reader(File::open(&library_path).unwrap()).unwrap();
        old["request"]
            .as_object_mut()
            .unwrap()
            .remove("configuration");
        atomic_json(&library_path, &old).unwrap();
        cuerithm_dj_sync::apply_sync(&library.data_dir().join("sync"), &preview.id).unwrap();
        library.apply_dj_sync(&preview.id).unwrap();
        library.restore_dj_sync(&preview.id).unwrap();
        assert_eq!(fs::read(&target.path).unwrap(), before);
    }
    #[test]
    fn windows_installation_updates_keep_acknowledged_library_identity() {
        let (dir, library, native) = fixture();
        let root = dir.path().canonicalize().unwrap();
        let mut target = windows_target(&native, &root);
        let first = root.join("first.exe");
        let second = root.join("second.exe");
        fs::write(&first, b"authored first installation").unwrap();
        fs::write(&second, b"authored second installation").unwrap();
        if let cuerithm_rekordbox::TargetEnvironment::WindowsCompatibility {
            installation, ..
        } = &mut target.environment
        {
            *installation = Some(first);
        }
        let preview = library
            .preview_dj_sync(request(&target, DjSyncScope::Full))
            .unwrap();
        library.apply_dj_sync(&preview.id).unwrap();
        if let cuerithm_rekordbox::TargetEnvironment::WindowsCompatibility {
            installation, ..
        } = &mut target.environment
        {
            *installation = Some(second);
        }
        let next = library
            .preview_dj_sync(request(&target, DjSyncScope::Changed))
            .unwrap();
        assert_eq!(
            next.tracks, 0,
            "compatible installation selection retains prior baseline"
        );
        library.discard_dj_sync(&next.id).unwrap();
    }
    #[test]
    fn replaced_same_spelling_library_parent_refuses_apply_and_restore() {
        let (dir, library, target) = fixture();
        let home = target.path.parent().unwrap();
        let displaced = dir.path().join("original-home");
        let preview = library
            .preview_dj_sync(request(&target, DjSyncScope::Full))
            .unwrap();
        let replace = || {
            fs::rename(home, &displaced).unwrap();
            fs::create_dir(home).unwrap();
            fs::copy(displaced.join("database.xml"), &target.path).unwrap();
        };
        let reinstate = || {
            fs::remove_dir_all(home).unwrap();
            fs::rename(&displaced, home).unwrap();
        };
        replace();
        assert!(library.apply_dj_sync(&preview.id).is_err());
        reinstate();
        library.apply_dj_sync(&preview.id).unwrap();
        replace();
        assert!(library.restore_dj_sync(&preview.id).is_err());
        reinstate();
        library.restore_dj_sync(&preview.id).unwrap();
    }
    #[test]
    fn windows_owned_playlist_sync_survives_organization_and_changed_resync() {
        for modern in [false, true] {
            let (dir, library, native) = fixture();
            let root = dir.path().canonicalize().unwrap();
            if !modern {
                fs::remove_dir(native.path.parent().unwrap().join("MyLists")).unwrap();
            }
            let target = windows_target(&native, &root);
            library.conn().unwrap().execute_batch("INSERT INTO playlists(id,kind,name,position,last_edit_id) VALUES('owned','playlist','Owned',0,'fixture'); INSERT INTO playlist_members(playlist_id,track_id,position) VALUES('owned','a',0);").unwrap();
            let preview = library
                .preview_dj_sync(request(&target, DjSyncScope::Full))
                .unwrap();
            library.apply_dj_sync(&preview.id).unwrap();
            let source = library.track_path("a").unwrap();
            let destination = root.join("managed/new.wav");
            fs::create_dir(destination.parent().unwrap()).unwrap();
            let references = library.organization_reference_snapshot("a").unwrap();
            assert!(references.blockers.is_empty(), "{:?}", references.blockers);
            let operation = library
                .plan_organization(crate::organization::OrganizationRequest {
                    track_id: "a".into(),
                    mode: crate::organization::OrganizationMode::Move,
                    source_root: source.parent().unwrap().into(),
                    destination_root: destination.parent().unwrap().into(),
                    files: vec![(source.clone(), destination.clone())],
                })
                .unwrap();
            library
                .bind_organization_references(&operation, &references.digest)
                .unwrap();
            library.execute_organization(&operation).unwrap();
            let guard = library.repair_organization_references(&operation).unwrap();
            library
                .acknowledge_organization_references_checked(&operation, &guard)
                .unwrap();
            drop(guard);
            let cleanup = library
                .verify_acknowledged_organization_references(&operation)
                .unwrap();
            library
                .execute_organization_checked(&operation, &cleanup)
                .unwrap();
            drop(cleanup);
            let next = library
                .preview_dj_sync(request(&target, DjSyncScope::Changed))
                .unwrap();
            library.apply_dj_sync(&next.id).unwrap();
            assert!(
                fs::read_to_string(&target.path)
                    .unwrap()
                    .contains(r"C:\managed\new.wav")
            );
        }
    }
    #[test]
    fn raw_source_paths_hash_and_roundtrip_without_changing_source_bytes() {
        let (dir, library, _target) = fixture();
        #[cfg(unix)]
        let filename = {
            use std::os::unix::ffi::OsStringExt;
            std::ffi::OsString::from_vec(b"authored-\xff.wav".to_vec())
        };
        #[cfg(windows)]
        let filename = {
            use std::os::windows::ffi::OsStringExt;
            std::ffi::OsString::from_wide(&[0x61, 0xd800, 0x2e, 0x77, 0x61, 0x76])
        };
        let source = dir.path().canonicalize().unwrap().join(filename);
        library
            .conn()
            .unwrap()
            .execute(
                "UPDATE tracks SET path=? WHERE id='a'",
                [crate::model::path_bytes(&source)],
            )
            .unwrap();
        let original = dir.path().join("authored.wav");
        let before = fs::read(&original).unwrap();
        let mut export = library.dj_sync_library(DjSyncScope::Full).unwrap();
        assert_eq!(export.tracks[0].path, source);
        let hashes = track_hashes(&mut export).unwrap();
        let sources = vec![Source {
            id: "a".into(),
            path: source.clone(),
            hash: file_hash(&original).unwrap().unwrap(),
        }];
        let bytes = serde_json::to_vec(&sources).unwrap();
        let restored: Vec<Source> = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(restored[0].path, source);
        assert_eq!(hashes, track_hashes(&mut export).unwrap());
        export.tracks[0].path = dir.path().join("another.wav");
        assert_ne!(hashes, track_hashes(&mut export).unwrap());
        assert_eq!(fs::read(&original).unwrap(), before);
    }
    #[test]
    fn discarded_library_previews_release_staging_and_leave_sources_and_native_state_unchanged() {
        let (dir, library, target) = fixture();
        let source = dir.path().join("authored.wav");
        let source_before = fs::read(&source).unwrap();
        let native_before = fs::read(&target.path).unwrap();
        let revision = library.revision().unwrap();
        let preview = library
            .preview_dj_sync(request(&target, DjSyncScope::Full))
            .unwrap();
        let path = library.data_dir().join("sync").join(&preview.id);
        assert!(path.join("staged").is_dir());
        library.discard_dj_sync(&preview.id).unwrap();
        library.discard_dj_sync(&preview.id).unwrap();
        assert!(!path.exists());
        assert_eq!(library.revision().unwrap(), revision);
        assert_eq!(fs::read(&source).unwrap(), source_before);
        assert_eq!(fs::read(&target.path).unwrap(), native_before);
        assert!(
            !library
                .history_dj_sync()
                .unwrap()
                .iter()
                .any(|entry| entry.id == preview.id)
        );
    }

    #[test]
    fn discard_cannot_remove_native_applied_unacknowledged_or_restored_history() {
        let (_dir, library, target) = fixture();
        let preview = library
            .preview_dj_sync(request(&target, DjSyncScope::Full))
            .unwrap();
        let root = library.data_dir().join("sync");
        cuerithm_dj_sync::apply_sync(&root, &preview.id).unwrap();
        assert!(library.discard_dj_sync(&preview.id).is_err());
        assert!(root.join(&preview.id).join("backup/0").is_file());
        library.restore_dj_sync(&preview.id).unwrap();
        assert!(library.discard_dj_sync(&preview.id).is_err());
        assert!(root.join(&preview.id).join("library.json").is_file());
    }
    #[test]
    fn preview_apply_changed_and_restore_use_acknowledged_baselines() {
        let (_dir, library, target) = fixture();
        let initial = fs::read(&target.path).unwrap();
        let preview = library
            .preview_dj_sync(request(&target, DjSyncScope::Full))
            .unwrap();
        assert_eq!(fs::read(&target.path).unwrap(), initial);
        let root = library.data_dir().join("sync");
        assert!(read_baseline(&root, &target).unwrap().is_none());
        let before_apply = library
            .preview_dj_sync(request(&target, DjSyncScope::Changed))
            .unwrap();
        assert_eq!(before_apply.tracks, 1); // Preparing never acknowledges a baseline.
        assert_eq!(
            library.apply_dj_sync(&preview.id).unwrap().state,
            JournalState::Applied
        );
        assert_ne!(fs::read(&target.path).unwrap(), initial);
        assert!(read_baseline(&root, &target).unwrap().is_some());
        let changed = library
            .preview_dj_sync(request(&target, DjSyncScope::Changed))
            .unwrap();
        assert_eq!(changed.tracks, 0);
        assert_eq!(changed.playlists, 0);
        assert_eq!(
            library.restore_dj_sync(&preview.id).unwrap().state,
            JournalState::Restored
        );
        assert_eq!(fs::read(&target.path).unwrap(), initial);
        assert!(read_baseline(&root, &target).unwrap().is_none());
        assert_eq!(
            library
                .preview_dj_sync(request(&target, DjSyncScope::Changed))
                .unwrap()
                .tracks,
            1
        );
        let history = library.history_dj_sync().unwrap();
        assert!(
            history
                .iter()
                .any(|entry| entry.id == preview.id && entry.state == Some(JournalState::Restored))
        );
    }
    fn backup_files(root: &Path, id: &str) -> usize {
        fs::read_dir(root.join(id).join("backup")).unwrap().count()
    }
    #[test]
    fn only_the_newest_ten_syncs_of_a_target_keep_their_backups_and_a_restored_one_gives_its_up() {
        let (_dir, library, target) = fixture();
        let root = library.data_dir().join("sync");
        let mut ids = vec![];
        for round in 0..12 {
            // A sync that changes nothing has nothing to back up: change the track each time.
            library
                .conn()
                .unwrap()
                .execute(
                    "UPDATE tracks SET title=?1 WHERE id='a'",
                    [format!("Authored {round}")],
                )
                .unwrap();
            let preview = library
                .preview_dj_sync(request(&target, DjSyncScope::Full))
                .unwrap();
            library.apply_dj_sync(&preview.id).unwrap();
            ids.push(preview.id);
            std::thread::sleep(std::time::Duration::from_millis(3));
        }
        // The two oldest gave up their copies as soon as the twelfth was applied.
        for (index, id) in ids.iter().enumerate() {
            assert_eq!(backup_files(&root, id) == 0, index < 2, "sync {index}");
        }
        let history = library.history_dj_sync().unwrap();
        for (index, id) in ids.iter().enumerate() {
            let entry = history.iter().find(|entry| &entry.id == id).unwrap();
            assert_eq!(entry.restore_expired, index < 2);
            // Every one is still listed, with its state.
            assert_eq!(entry.state, Some(JournalState::Applied));
        }
        let error = library.restore_dj_sync(&ids[0]).unwrap_err();
        assert!(
            format!("{error:#}").contains("older than the 10"),
            "{error:#}"
        );
        // Pruning twice changes nothing more.
        assert_eq!(library.prune_dj_sync_history().unwrap(), 0);
        // The newest can be restored; once restored its copies are no longer needed.
        assert_eq!(
            library.restore_dj_sync(&ids[11]).unwrap().state,
            JournalState::Restored
        );
        assert_eq!(backup_files(&root, &ids[11]), 0);
        assert_ne!(backup_files(&root, &ids[10]), 0);
        // A sync that has not been acknowledged may still need its backup, however old it is.
        let path = root.join(&ids[2]).join("library.json");
        let mut saved: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        saved["acknowledged"] = json!(false);
        fs::write(&path, serde_json::to_vec(&saved).unwrap()).unwrap();
        library.prune_dj_sync_locked(&root, 0).unwrap();
        assert_ne!(backup_files(&root, &ids[2]), 0);
        assert_eq!(backup_files(&root, &ids[3]), 0);
    }
    #[test]
    fn changed_metadata_is_selected_from_the_durable_target_baseline() {
        let (_dir, library, target) = fixture();
        let preview = library
            .preview_dj_sync(request(&target, DjSyncScope::Full))
            .unwrap();
        library.apply_dj_sync(&preview.id).unwrap();
        {
            let conn = library.conn().unwrap();
            conn.execute("UPDATE tracks SET title='Changed' WHERE id='a'", [])
                .unwrap();
            Library::bump(&conn).unwrap();
        }
        let changed = library
            .preview_dj_sync(request(&target, DjSyncScope::Changed))
            .unwrap();
        assert_eq!(changed.tracks, 1);
        library.apply_dj_sync(&changed.id).unwrap();
        assert!(
            fs::read_to_string(&target.path)
                .unwrap()
                .contains("Changed")
        );
        assert_eq!(
            library
                .preview_dj_sync(request(&target, DjSyncScope::Changed))
                .unwrap()
                .tracks,
            0
        );
    }
    #[test]
    fn library_revision_and_source_changes_reject_stale_work_and_record_errors() {
        let (_dir, library, target) = fixture();
        let initial = fs::read(&target.path).unwrap();
        let preview = library
            .preview_dj_sync(request(&target, DjSyncScope::Full))
            .unwrap();
        Library::bump(&library.conn().unwrap()).unwrap();
        assert!(
            library
                .apply_dj_sync(&preview.id)
                .unwrap_err()
                .to_string()
                .contains("Library changed")
        );
        assert_eq!(fs::read(&target.path).unwrap(), initial);
        assert!(
            library
                .history_dj_sync()
                .unwrap()
                .iter()
                .find(|entry| entry.id == preview.id)
                .unwrap()
                .error
                .is_some()
        );
        let source = library.track_path("a").unwrap();
        fs::write(source, b"External audio modification").unwrap();
        assert!(
            library
                .preview_dj_sync(request(&target, DjSyncScope::Full))
                .unwrap_err()
                .to_string()
                .contains("changed since analysis")
        );
        assert_eq!(fs::read(&target.path).unwrap(), initial);
    }
    #[test]
    fn changed_playlist_carries_unchanged_members_and_folder_ancestors() {
        use cuerithm_dj_sync::{SyncPlaylist, SyncTrack};
        let mut library = SyncLibrary {
            tracks: vec![
                SyncTrack {
                    native_path: None,
                    id: "a".into(),
                    ..Default::default()
                },
                SyncTrack {
                    native_path: None,
                    id: "b".into(),
                    ..Default::default()
                },
            ],
            playlists: vec![
                SyncPlaylist {
                    id: "folder".into(),
                    folder: true,
                    ..Default::default()
                },
                SyncPlaylist {
                    id: "set".into(),
                    parent_id: Some("folder".into()),
                    track_ids: vec!["a".into(), "b".into()],
                    ..Default::default()
                },
            ],
        };
        let mut baseline = Baseline {
            track_hashes: track_hashes(&mut library).unwrap(),
            playlist_hashes: playlist_hashes(&library).unwrap(),
            ..Default::default()
        };
        baseline
            .playlist_hashes
            .insert("set".into(), "previous membership".into());
        let tracks = track_hashes(&mut library).unwrap();
        let playlists = playlist_hashes(&library).unwrap();
        select_changed(&mut library, &[], &tracks, &playlists, &baseline);
        assert_eq!(library.tracks.len(), 2);
        assert_eq!(library.playlists.len(), 2);
        assert_eq!(library.playlists[1].track_ids, ["a", "b"]);
    }
    #[test]
    fn audio_hash_ownership_follows_verified_apply_and_restore() {
        let (_dir, library, _target) = fixture();
        let source = library.track_path("a").unwrap();
        let before = library.source_hash("a").unwrap();
        let after_bytes = library.data_dir().join("synthetic-tagged-copy");
        fs::write(&after_bytes, b"Synthetic new tag bytes").unwrap();
        let after = file_hash(&after_bytes).unwrap().unwrap();
        let transaction = FileTransaction::create(&library.data_dir().join("sync")).unwrap();
        transaction
            .add(&source, Some(before.clone()), Some(&after_bytes))
            .unwrap();
        let sources = vec![Source {
            id: "a".into(),
            path: source.clone(),
            hash: before.clone(),
        }];
        let applied = transaction.apply(|| Ok(())).unwrap();
        {
            let conn = library.conn().unwrap();
            validate_sources(&conn, &sources, Some(&applied), true).unwrap();
            update_source_hashes(&conn, &sources, &applied, false).unwrap();
        }
        assert_eq!(library.source_hash("a").unwrap(), after);
        let restored = transaction.restore(|| Ok(())).unwrap();
        {
            let conn = library.conn().unwrap();
            validate_sources(&conn, &sources, Some(&restored), false).unwrap();
            update_source_hashes(&conn, &sources, &restored, true).unwrap();
        }
        assert_eq!(library.source_hash("a").unwrap(), before);
        assert_eq!(file_hash(&source).unwrap(), Some(before));
    }
    #[test]
    fn archived_tracks_can_restore_native_backups_and_own_tag_hashes() {
        let (_dir, library, target) = fixture();
        let initial = fs::read(&target.path).unwrap();
        let source = library.track_path("a").unwrap();
        let original_audio = fs::read(&source).unwrap();
        let original_hash = library.source_hash("a").unwrap();
        let preview = library
            .preview_dj_sync(request(&target, DjSyncScope::Full))
            .unwrap();
        let root = library.data_dir().join("sync");
        let transaction = FileTransaction::open(&root, &preview.id).unwrap();
        let tagged = library.data_dir().join("synthetic-archived-tagged-copy");
        fs::write(&tagged, b"Synthetic tag publication before archival").unwrap();
        transaction
            .add(&source, Some(original_hash.clone()), Some(&tagged))
            .unwrap();
        library.apply_dj_sync(&preview.id).unwrap();
        assert_ne!(library.source_hash("a").unwrap(), original_hash);
        library.set_archived("a", true).unwrap();

        assert_eq!(
            library.restore_dj_sync(&preview.id).unwrap().state,
            JournalState::Restored
        );
        assert_eq!(fs::read(&target.path).unwrap(), initial);
        assert_eq!(fs::read(&source).unwrap(), original_audio);
        assert_eq!(library.source_hash("a").unwrap(), original_hash);
        assert!(read_baseline(&root, &target).unwrap().is_none());
        assert!(
            library
                .conn()
                .unwrap()
                .query_row("SELECT archived FROM tracks WHERE id='a'", [], |row| {
                    row.get::<_, bool>(0)
                })
                .unwrap()
        );
        library.restore_dj_sync(&preview.id).unwrap();
    }
    #[test]
    fn archived_track_restore_still_rejects_changed_source_identity_or_bytes() {
        for change in ["path", "hash", "bytes"] {
            let (_dir, library, target) = fixture();
            let preview = library
                .preview_dj_sync(request(&target, DjSyncScope::Full))
                .unwrap();
            library.apply_dj_sync(&preview.id).unwrap();
            let applied = fs::read(&target.path).unwrap();
            library.set_archived("a", true).unwrap();
            match change {
                "path" => {
                    library
                        .conn()
                        .unwrap()
                        .execute(
                            "UPDATE tracks SET path=?1 WHERE id='a'",
                            [crate::model::path_bytes(
                                &library.data_dir().join("moved.wav"),
                            )],
                        )
                        .unwrap();
                }
                "hash" => {
                    library
                        .conn()
                        .unwrap()
                        .execute("UPDATE tracks SET source_hash='different' WHERE id='a'", [])
                        .unwrap();
                }
                "bytes" => {
                    fs::write(library.track_path("a").unwrap(), b"Later audio edit").unwrap()
                }
                _ => unreachable!(),
            }
            assert!(library.restore_dj_sync(&preview.id).is_err(), "{change}");
            assert_eq!(fs::read(&target.path).unwrap(), applied, "{change}");
        }
    }
    #[test]
    fn bitmap_digest_keeps_buffers_and_hashes_content_without_json_expansion() {
        use cuerithm_dj_sync::SyncTrack;
        let mut library = SyncLibrary {
            tracks: vec![SyncTrack {
                native_path: None,
                id: "art".into(),
                artwork: Some(vec![0, 1, 2, 3]),
                ..Default::default()
            }],
            ..Default::default()
        };
        let before = track_hashes(&mut library).unwrap();
        assert_eq!(library.tracks[0].artwork, Some(vec![0, 1, 2, 3]));
        library.tracks[0].artwork.as_mut().unwrap()[3] = 4;
        assert_ne!(before, track_hashes(&mut library).unwrap());
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::*;

    #[test]
    fn interrupted_external_publication_restores_without_acknowledging_a_baseline() {
        let (_dir, library, target) = super::tests::fixture();
        let initial = fs::read(&target.path).unwrap();
        let preview = library
            .preview_dj_sync(super::tests::request(&target, DjSyncScope::Full))
            .unwrap();
        let root = library.data_dir().join("sync");
        let transaction = FileTransaction::open(&root, &preview.id).unwrap();
        let source = library.track_path("a").unwrap();
        let original_audio = fs::read(&source).unwrap();
        let tagged = library.data_dir().join("synthetic-interrupted-tagged-copy");
        fs::write(&tagged, b"Synthetic partial publication").unwrap();
        transaction
            .add(&source, file_hash(&source).unwrap(), Some(&tagged))
            .unwrap();
        let calls = std::cell::Cell::new(0);
        let result = transaction.apply(|| {
            let count = calls.get();
            calls.set(count + 1);
            ensure!(count < 3, "Synthetic final verification interruption");
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(transaction.inspect().unwrap().state, JournalState::Applying);
        assert_ne!(fs::read(&source).unwrap(), original_audio);
        assert!(read_baseline(&root, &target).unwrap().is_none());
        let restored = library.restore_dj_sync(&preview.id).unwrap();
        assert_eq!(restored.state, JournalState::Restored);
        assert_eq!(fs::read(&source).unwrap(), original_audio);
        assert_eq!(fs::read(&target.path).unwrap(), initial);
        assert_eq!(
            file_hash(&source).unwrap().as_deref(),
            Some(library.source_hash("a").unwrap().as_str())
        );
        assert!(read_baseline(&root, &target).unwrap().is_none());
    }

    #[test]
    fn owned_deletion_is_staged_acknowledged_and_restored_with_its_ledger() {
        let (_dir, library, target) = super::tests::fixture();
        library.conn().unwrap().execute("INSERT INTO playlists(id,kind,name,position,last_edit_id) VALUES('gone','playlist','Gone',0,'fixture')",[]).unwrap();
        let preview = library
            .preview_dj_sync(super::tests::request(&target, DjSyncScope::Full))
            .unwrap();
        library.apply_dj_sync(&preview.id).unwrap();
        let root = library.data_dir().join("sync");
        let before = ownership::read(&root, &target).unwrap();
        let binding = before
            .stores
            .values()
            .flat_map(|(_, store)| &store.bindings)
            .find(|b| b.source_id == "gone")
            .unwrap();
        assert!(binding.created);
        let native = target.path.parent().unwrap().join(&binding.locator);
        assert!(native.exists());
        library
            .apply_playlist_edit(
                crate::PlaylistEdit::Remove { id: "gone".into() },
                library.playlist_state().unwrap().revision,
            )
            .unwrap();
        let removed = library
            .preview_dj_sync(super::tests::request(&target, DjSyncScope::Changed))
            .unwrap();
        assert_eq!(removed.tracks, 0);
        assert_eq!(removed.playlists, 0);
        assert!(native.exists());
        assert!(
            removed
                .files
                .iter()
                .any(|f| f.target == native && f.after_sha256.is_none())
        );
        library.apply_dj_sync(&removed.id).unwrap();
        assert!(!native.exists());
        assert!(
            !read_baseline(&root, &target)
                .unwrap()
                .unwrap()
                .playlist_hashes
                .contains_key("gone")
        );
        assert!(!ownership::read(&root, &target).unwrap().contains("gone"));
        library.restore_dj_sync(&removed.id).unwrap();
        assert!(native.exists());
        assert!(ownership::read(&root, &target).unwrap().contains("gone"));
    }
    #[test]
    fn legacy_hash_baseline_cannot_authorize_native_deletion() {
        let (_dir, library, target) = super::tests::fixture();
        library.conn().unwrap().execute("INSERT INTO playlists(id,kind,name,position,last_edit_id) VALUES('gone','playlist','Gone',0,'fixture')",[]).unwrap();
        let preview = library
            .preview_dj_sync(super::tests::request(&target, DjSyncScope::Full))
            .unwrap();
        library.apply_dj_sync(&preview.id).unwrap();
        let root = library.data_dir().join("sync");
        fs::remove_dir_all(root.join("ownership")).unwrap();
        library
            .apply_playlist_edit(
                crate::PlaylistEdit::Remove { id: "gone".into() },
                library.playlist_state().unwrap().revision,
            )
            .unwrap();
        let error = library
            .preview_dj_sync(super::tests::request(&target, DjSyncScope::Changed))
            .unwrap_err();
        assert!(error.to_string().contains("no native ownership record"));
        assert!(
            read_baseline(&root, &target)
                .unwrap()
                .unwrap()
                .playlist_hashes
                .contains_key("gone")
        );
    }

    #[test]
    fn short_lived_inherited_style_service_lease_can_settle_before_retry() {
        let dir = tempfile::tempdir().unwrap();
        let held = service_lock(dir.path()).unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(25));
            drop(held);
        });
        let next = service_lock(dir.path());
        release.join().unwrap();
        assert!(
            next.is_ok(),
            "brief inherited leases must settle before reporting a competing sync"
        );
    }
}

#[cfg(test)]
mod timing_profile_tests {
    use super::*;
    use cuerithm_media::{CodecTimingProfile, SourceExportTiming};
    #[test]
    fn old_analysis_backfill_uses_a_private_snapshot_and_current_runtime_without_dsp() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let runtime =
            cuerithm_media::MediaRuntime::from_bundle(&workspace.join("tools/media-runtime/bin"))
                .expect("Bundled runtime required for timing integration");
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("old.m4a");
        fs::copy(
            workspace.join("crates/cuerithm-media/tests/fixtures/timing-aac-44100.m4a"),
            &source,
        )
        .unwrap();
        let hash = file_hash(&source).unwrap().unwrap();
        let mut library = SyncLibrary {
            tracks: vec![cuerithm_dj_sync::SyncTrack {
                native_path: None,
                id: "track".into(),
                path: source.clone(),
                sample_rate: 44100,
                cues: vec![cuerithm_dj_sync::SyncCue {
                    start_seconds: 0.1,
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let sources = vec![Source {
            id: "track".into(),
            path: source.clone(),
            hash: hash.clone(),
        }];
        measure_export_profiles(
            &dir.path().join("private"),
            &mut library,
            &sources,
            Some(&runtime),
        )
        .unwrap();
        let measured = library.tracks[0].export_timing.as_ref().unwrap();
        assert_eq!(measured.source_sha256, hash);
        assert_eq!(
            measured.measurement_identity,
            runtime.export_timing_identity()
        );
        let CodecTimingProfile::IsoAudio { audio_delay_ms, .. } = measured.codec_profile else {
            panic!("Expected measured AAC timing")
        };
        assert_eq!(audio_delay_ms, 23.2);
        assert_eq!(library.tracks[0].cues[0].start_seconds, 0.1);
        assert_eq!(file_hash(&source).unwrap(), Some(hash));
        measure_export_profiles(&dir.path().join("private"), &mut library, &sources, None).unwrap();
        assert!(
            library.tracks[0].export_timing.is_none(),
            "Missing current runtime cannot silently reuse opaque stale measurement identities"
        );
    }
    #[test]
    fn semantic_baseline_excludes_own_tag_hash_changes_but_keeps_timing_measurements() {
        let profile = SourceExportTiming {
            decoded_sample_count: None,
            profile: "dj-timeline-v1".into(),
            source_sha256: "0".repeat(64),
            source_sample_rate: 44100,
            codec_profile: CodecTimingProfile::IsoAudio {
                extension: "m4a".into(),
                audio_delay_ms: 23.2,
                video_delay_ms: 0.,
            },
            measurement_identity: "synthetic".into(),
        };
        let mut library = SyncLibrary {
            tracks: vec![cuerithm_dj_sync::SyncTrack {
                native_path: None,
                id: "track".into(),
                export_timing: Some(profile),
                ..Default::default()
            }],
            ..Default::default()
        };
        let before = track_hashes(&mut library).unwrap();
        library.tracks[0]
            .export_timing
            .as_mut()
            .unwrap()
            .source_sha256 = "1".repeat(64);
        assert_eq!(before, track_hashes(&mut library).unwrap());
        if let CodecTimingProfile::IsoAudio { audio_delay_ms, .. } = &mut library.tracks[0]
            .export_timing
            .as_mut()
            .unwrap()
            .codec_profile
        {
            *audio_delay_ms = 21.3;
        }
        assert_ne!(before, track_hashes(&mut library).unwrap());
        assert_eq!(
            library.tracks[0]
                .export_timing
                .as_ref()
                .unwrap()
                .source_sha256,
            "1".repeat(64)
        );
    }
}
