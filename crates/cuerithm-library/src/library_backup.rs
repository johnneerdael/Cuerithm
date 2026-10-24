//! Same-installation preparation-state snapshots. Music files are never copied.
use crate::{
    Library, SettingsSnapshot,
    model::{bytes_path, now, path_bytes},
};
use anyhow::{Context, Result, ensure};
use rusqlite::{
    Connection, OpenFlags, OptionalExtension,
    backup::{Backup, StepResult},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const APPLICATION_ID: i64 = 1129665093;
const MAX_DATABASE: u64 = 1024 * 1024 * 1024;
const MAX_MANIFEST: u64 = 16 * 1024 * 1024;
const MAX_ENTRIES: usize = 50_000;
const MAX_DEPENDENCY_BYTES: u64 = 1024 * 1024 * 1024 * 1024;
const RESOURCE_DIRS: [&str; 6] = [
    "tagging",
    "cleanup",
    "maintenance",
    "sync",
    "organization-references",
    "metadata-edits",
];
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryBackupPreview {
    pub path: PathBuf,
    pub database_sha256: String,
    pub created: i64,
    pub tracks: u64,
    pub missing_sources: u64,
    pub schema: i64,
    pub music_included: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryRestoreRequest {
    pub id: String,
    pub created: i64,
    pub database_sha256: String,
    #[serde(default)]
    pub blocked_reason: Option<String>,
    #[serde(default)]
    pub mutation_started: bool,
    #[serde(default)]
    pub pre_restore: Option<PathBuf>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum LibraryRestoreStartup {
    NoPending,
    Applied {
        pre_restore: PathBuf,
    },
    BlockedBeforeMutation {
        request: LibraryRestoreRequest,
        reason: String,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Location {
    path: Vec<u8>,
    sha256: Option<String>,
    bytes: u64,
    kind: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format: u32,
    application: String,
    version: String,
    platform: String,
    installation: Vec<u8>,
    installation_id: String,
    #[serde(default)]
    data_directory: Option<Vec<u8>>,
    created: i64,
    schema: i64,
    database_sha256: String,
    music_included: bool,
    sources: Vec<Location>,
    models: Vec<Location>,
    resources: Vec<Location>,
    organization: Vec<OrganizationLocation>,
}
fn root_identity(root: &Path) -> Result<Vec<u8>> {
    Ok(path_bytes(&root.canonicalize()?))
}
fn snapshot_data_directory(manifest: &Manifest, installation: &Path) -> Result<PathBuf> {
    let data = manifest
        .data_directory
        .as_deref()
        .unwrap_or(&manifest.installation);
    let data = bytes_path(data)?;
    ensure!(
        data.canonicalize()? == data,
        "Backup data directory changed"
    );
    if data != installation {
        let id = data
            .file_name()
            .and_then(|s| s.to_str())
            .context("Invalid backup generation")?;
        ensure!(
            uuid::Uuid::parse_str(id).is_ok()
                && data.parent() == Some(installation.join("libraries").as_path()),
            "Backup data directory is outside this installation"
        );
        for directory in [data.as_path(), data.parent().unwrap()] {
            let meta = fs::symlink_metadata(directory)?;
            ensure!(
                meta.is_dir() && !meta.file_type().is_symlink(),
                "Unsafe backup data directory"
            );
        }
    }
    Ok(data)
}
fn managed_directory(root: &Path) -> Result<PathBuf> {
    let path = root.join("library-backups");
    if let Ok(meta) = fs::symlink_metadata(&path) {
        ensure!(
            meta.is_dir() && !meta.file_type().is_symlink(),
            "Managed backup directory cannot be a symbolic link or non-directory"
        );
    }
    Ok(path)
}
fn installation_id(root: &Path, create: bool) -> Result<String> {
    let path = root.join(".library-installation-id");
    if create && !path.exists() {
        atomic_json(&path, &uuid::Uuid::new_v4().to_string())?;
    }
    regular(&path, 1024)?;
    let id: String = serde_json::from_slice(&fs::read(path)?)?;
    ensure!(
        uuid::Uuid::parse_str(&id).is_ok(),
        "Library installation identity is unavailable"
    );
    Ok(id)
}
fn file_lease(path: &Path, message: &str) -> Result<File> {
    ensure!(
        !fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()),
        "Lock cannot be a symbolic link"
    );
    let file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    // Match Library::open: a decoder fork can inherit the open-file lock
    // briefly before exec. Retry only contention and keep real exclusion.
    let started = Instant::now();
    loop {
        match fs2::FileExt::try_lock_exclusive(&file) {
            Ok(()) => break,
            Err(error)
                if error.raw_os_error() == fs2::lock_contended_error().raw_os_error()
                    && started.elapsed() < Duration::from_millis(250) =>
            {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => return Err(error).context(message.to_owned()),
        }
    }
    Ok(file)
}
pub(crate) fn sync_lease(root: &Path) -> Result<File> {
    let directory = root.join("sync");
    fs::create_dir_all(&directory)?;
    ensure!(
        !fs::symlink_metadata(&directory)?.file_type().is_symlink(),
        "Sync directory cannot be a symbolic link"
    );
    file_lease(
        &directory.join("service.lock"),
        "Another DJ sync operation is running",
    )
}
fn readonly(path: &Path) -> Result<Connection> {
    regular(path, MAX_DATABASE)?;
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.pragma_update(None, "trusted_schema", false)?;
    Ok(conn)
}
fn regular(path: &Path, max: u64) -> Result<()> {
    let meta = fs::symlink_metadata(path)?;
    ensure!(
        meta.is_file() && !meta.file_type().is_symlink(),
        "Backup entries must be regular files"
    );
    ensure!(meta.len() <= max, "Backup exceeds the supported size limit");
    Ok(())
}
fn schema(conn: &Connection) -> Result<i64> {
    let app: i64 = conn.query_row("PRAGMA application_id", [], |r| r.get(0))?;
    ensure!(app == APPLICATION_ID, "This is not a CueRithm library");
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    ensure!(
        (27..=31).contains(&version),
        "Unsupported library backup schema"
    );
    Ok(version)
}
pub(crate) fn quiet_database(conn: &Connection) -> Result<()> {
    // Match startup's non-tagging recovery guard, including accepted edits on
    // attention jobs. Completed physical publications can still need cleanup.
    let metadata_pending: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM metadata_edit_writes WHERE state NOT IN ('committed','undone','failed','rolled_back') OR (state IN ('committed','undone') AND error IS NOT NULL))
         OR EXISTS(SELECT 1 FROM metadata_edit_batches WHERE state IN ('saving','queued','recovery_required'))
         OR EXISTS(SELECT 1 FROM metadata_edit_rows WHERE state IN ('ready','queued','recovery_required'))
         OR EXISTS(SELECT 1 FROM metadata_edit_previews WHERE state IN ('retiring','cleanup_required'))
         OR EXISTS(SELECT 1 FROM metadata_edit_overrides o JOIN jobs j ON j.id=o.job_id WHERE j.state IN ('queued','running','attention') AND (o.state IN ('queued','claimed') OR (o.state='committed' AND o.final_json IS NULL)))
         OR EXISTS(SELECT 1 FROM preparation_review_decisions d JOIN jobs j ON j.id=d.job_id WHERE j.state IN ('queued','running','attention') AND d.state IN ('pending','claimed','applied'))
         OR EXISTS(SELECT 1 FROM artifact_cleanup_moves WHERE state!='removed')",
        [], |row| row.get(0),
    )?;
    ensure!(
        !metadata_pending,
        "Finish metadata saves, accepted preparation reviews and auxiliary artifact cleanup before using a library backup"
    );
    // A mapped Curate head can own future physical writes before its metadata
    // child exists. Preserve that parent outbox through every restore boundary.
    let curation_pending: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM curation_tracks WHERE native_required=1 AND (revision!=acknowledged_revision OR active_publication_id IS NOT NULL))
         OR EXISTS(SELECT 1 FROM curation_publications WHERE native_required=1 AND state NOT IN ('saved','undone','discarded'))",
        [], |row| row.get(0),
    )?;
    ensure!(
        !curation_pending,
        "Finish or recover mapped Curate publications before using a library backup"
    );
    let native_pending: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM rekordbox_sync_operations WHERE status NOT IN ('applied','restored','discarded','rolled_back'))",
        [], |row| row.get(0),
    )?;
    ensure!(
        !native_pending,
        "Finish or recover native rekordbox sync before using a library backup"
    );
    let organization_pending:i64=conn.query_row("SELECT (SELECT COUNT(*) FROM organization_operations WHERE state NOT IN ('complete','unchanged','cancelled','undone') OR error IS NOT NULL)+(SELECT COUNT(*) FROM organization_reservations)+(SELECT COUNT(*) FROM organization_batches WHERE state NOT IN ('complete','partial','cancelled','undone'))+(SELECT COUNT(*) FROM organization_batch_rows WHERE outcome NOT IN ('completed','unchanged','skipped','blocked','failed','cancelled'))+(SELECT COUNT(*) FROM preparation_checkpoints WHERE state!='complete')+(SELECT COUNT(*) FROM organization_copy_conflicts WHERE state NOT IN ('committed','original_restored'))+(SELECT COUNT(*) FROM organization_alternates WHERE state='conflict')+(SELECT COUNT(*) FROM organization_reference_intents i JOIN organization_operations o ON o.id=i.operation_id WHERE i.error IS NOT NULL OR i.state IN ('repairing','repaired') OR (o.state!='cancelled' AND i.state!='acknowledged' AND EXISTS(SELECT 1 FROM json_each(i.payload_json,'$.entries') e WHERE json_extract(e.value,'$.reference.known.independent')=0)))",[],|row|row.get(0))?;
    ensure!(
        organization_pending == 0,
        "Finish organization and resolve its preparation, Copy-source or native-reference recovery before using a library backup"
    );
    let pending:i64=conn.query_row("SELECT (SELECT COUNT(*) FROM jobs WHERE state IN ('queued','running'))+(SELECT COUNT(*) FROM tag_writes WHERE state NOT IN ('committed','rolled_back') OR cleanup_complete=0)+(SELECT COUNT(*) FROM cleanup_writes WHERE state NOT IN ('committed','undone','rolled_back') OR cleanup_complete=0)+(SELECT COUNT(*) FROM maintenance_writes WHERE state NOT IN ('committed','undone','rolled_back') OR cleanup_complete=0)+(SELECT COUNT(*) FROM maintenance_scans WHERE (kind='unused_removal_operation' AND (json_extract(payload_json,'$.state') NOT IN ('committed','undone','rolled_back') OR COALESCE(json_extract(payload_json,'$.cleanup_complete'),0)=0)) OR (kind='unused_backup_disposal' AND (json_extract(payload_json,'$.result.state') NOT IN ('committed','rolled_back') OR COALESCE(json_extract(payload_json,'$.result.cleanup_pending'),1)=1)))",[],|r|r.get(0))?;
    ensure!(
        pending == 0,
        "Finish preparation and resolve tagging, cleanup or maintenance recovery before using a library backup"
    );
    Ok(())
}
/// The write kinds that delete their original once they have settled.
const RETIRING_BACKUPS: [&str; 3] = ["tag", "cleanup", "maintenance"];
fn settled(conn: &Connection, root: &Path) -> Result<()> {
    quiet_database(conn)?;
    // A changed committed publication keeps its original bytes for recovery or Undo until its
    // cleanup retires them, which clears the path from the journal.
    for table in ["tag", "cleanup", "maintenance"] {
        let query = format!(
            "SELECT f.backup,f.before_hash FROM {table}_write_files f JOIN {table}_writes w ON w.id=f.write_id WHERE w.state='committed' AND f.before_hash IS NOT NULL AND f.before_hash<>f.after_hash"
        );
        let mut q = conn.prepare(&query)?;
        for row in q.query_map([], |r| {
            Ok((r.get::<_, Option<Vec<u8>>>(0)?, r.get::<_, String>(1)?))
        })? {
            let (path, hash) = row?;
            let Some(path) = path else {
                // Retired: the write has settled and its original was deleted on purpose.
                ensure!(
                    RETIRING_BACKUPS.contains(&table),
                    "Recovery backup path is missing"
                );
                continue;
            };
            let path = bytes_path(&path)?;
            let observed = location(&path)?;
            ensure!(
                observed.sha256.as_deref() == Some(hash.as_str()),
                "A retained recovery backup is missing or changed"
            );
        }
    }
    let mut q = conn.prepare(
        "SELECT payload_json FROM maintenance_scans WHERE kind='unused_removal_operation'",
    )?;
    for json in q.query_map([], |r| r.get::<_, String>(0))? {
        let op: serde_json::Value = serde_json::from_str(&json?)?;
        if op["state"] == "committed" && op["undone_by"].is_null() && op["backups_disposed"] != true
        {
            for entry in op["files"].as_array().context("Invalid removal journal")? {
                let bytes: Vec<u8> = serde_json::from_value(entry["backup"].clone())?;
                if bytes.is_empty() {
                    // A removal made since the library stopped keeping copies: the file went
                    // to the Trash and there is nothing here to be missing or changed.
                    continue;
                }
                let saved = entry["entry"]["public"]["source_sha256"]
                    .as_str()
                    .context("Invalid removal recovery identity")?;
                ensure!(
                    location(&bytes_path(&bytes)?)?.sha256.as_deref() == Some(saved),
                    "An unused-file recovery backup is missing or changed"
                );
            }
        }
    }
    let sync = root.join("sync");
    if sync.exists() {
        for entry in fs::read_dir(sync)? {
            let entry = entry?;
            if uuid::Uuid::parse_str(&entry.file_name().to_string_lossy()).is_err() {
                continue;
            }
            ensure!(
                entry.file_type()?.is_dir(),
                "Invalid DJ sync recovery entry"
            );
            let transaction = cuerithm_dj_sync::transaction::FileTransaction::open(
                &root.join("sync"),
                &entry.file_name().to_string_lossy(),
            )?;
            use cuerithm_dj_sync::transaction::JournalState;
            let state = transaction.inspect()?.state;
            ensure!(
                matches!(state, JournalState::Applied | JournalState::Restored),
                "Resolve or discard pending DJ sync previews before using a library backup"
            );
            regular(&entry.path().join("library.json"), MAX_MANIFEST)?;
            let saved: serde_json::Value =
                serde_json::from_slice(&fs::read(entry.path().join("library.json"))?)?;
            ensure!(
                saved["error"].is_null()
                    && if state == JournalState::Applied {
                        saved["acknowledged"] == true
                    } else {
                        saved["restore_acknowledged"] == true
                    },
                "DJ sync recovery needs attention"
            );
        }
    }
    Ok(())
}
fn location(path: &Path) -> Result<Location> {
    let (sha256, bytes, kind) = match fs::symlink_metadata(path) {
        Ok(meta) => {
            ensure!(
                !meta.file_type().is_symlink(),
                "Symbolic links are not supported in library backup dependencies"
            );
            if meta.is_dir() {
                (None, 0, "directory")
            } else {
                ensure!(meta.is_file(), "Unsupported library dependency");
                ensure!(
                    meta.len() <= MAX_DEPENDENCY_BYTES,
                    "Library dependency exceeds size budget"
                );
                (Some(cuerithm_media::file_sha256(path)?), meta.len(), "file")
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (None, 0, "missing"),
        Err(e) => return Err(e.into()),
    };
    Ok(Location {
        path: path_bytes(path),
        sha256,
        bytes,
        kind: kind.into(),
    })
}
fn budget(items: &[Location]) -> Result<()> {
    ensure!(
        items.len() <= MAX_ENTRIES,
        "Too many library backup dependencies"
    );
    let bytes = items.iter().try_fold(0u64, |total, item| {
        total
            .checked_add(item.bytes)
            .context("Library backup size overflow")
    })?;
    ensure!(
        bytes <= MAX_DEPENDENCY_BYTES,
        "Library backup dependencies exceed size budget"
    );
    Ok(())
}
type BackupInventory = (
    Vec<Location>,
    Vec<Location>,
    Vec<Location>,
    Vec<OrganizationLocation>,
);
fn inventory(conn: &Connection, root: &Path) -> Result<BackupInventory> {
    #[cfg(test)]
    inventory_phase_tests::run_hook();
    let mut sources = BTreeMap::new();
    let mut q =
        conn.prepare("SELECT path,source_hash FROM tracks WHERE path IS NOT NULL ORDER BY id")?;
    for row in q.query_map([], |r| {
        Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, Option<String>>(1)?))
    })? {
        let (path, hash) = row?;
        let observed = location(&bytes_path(&path)?)?;
        ensure!(
            observed.kind == "missing"
                || hash
                    .as_deref()
                    .is_none_or(|hash| observed.sha256.as_deref() == Some(hash)),
            "A source file changed since its saved preparation. Reprepare it before using this backup"
        );
        sources.insert(path, observed);
    }
    let sources = sources.into_values().collect::<Vec<_>>();
    budget(&sources)?;
    let mut models = BTreeMap::new();
    let mut q = conn.prepare("SELECT json FROM settings")?;
    for row in q.query_map([], |r| r.get::<_, String>(0))? {
        let saved: SettingsSnapshot = serde_json::from_str(&row?)?;
        let e = &saved.settings.engine;
        for path in [
            &e.beat_model,
            &e.mel_model,
            &e.key_model,
            &e.key_neural_model,
            &e.key_openkeyscan_model,
            &e.energy_model,
        ]
        .into_iter()
        .flatten()
        {
            let observed = location(path)?;
            ensure!(
                observed.kind == "file",
                "An analysis model required by saved settings is unavailable"
            );
            models.insert(observed.path.clone(), observed);
        }
    }
    let models = models.into_values().collect::<Vec<_>>();
    budget(&models)?;
    let mut resources = Vec::new();
    for name in RESOURCE_DIRS {
        let directory = root.join(name);
        if !directory.exists() {
            continue;
        }
        for entry in walkdir::WalkDir::new(&directory)
            .follow_links(false)
            .max_depth(32)
        {
            let entry = entry?;
            if entry.path() == root.join("sync/service.lock") {
                continue;
            }
            ensure!(
                entry.depth() < 32,
                "Recovery resource tree exceeds depth limit"
            );
            ensure!(
                resources.len() < MAX_ENTRIES,
                "Too many retained recovery resources"
            );
            resources.push(location(entry.path())?);
        }
    }
    resources.sort_by(|a, b| a.path.cmp(&b.path));
    budget(&resources)?;
    Ok((
        sources,
        models,
        resources,
        organization_inventory(conn, root)?,
    ))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OrganizationLocation {
    path: Vec<u8>,
    parent: Vec<u8>,
    ancestors: Vec<crate::organization::Identity>,
    identity: Option<crate::organization::Identity>,
    sha256: Option<String>,
    bytes: u64,
    kind: String,
}
struct OrganizationLocations {
    entries: BTreeMap<(Vec<u8>, bool), OrganizationLocation>,
    bytes: u64,
    limit: u64,
}
impl OrganizationLocations {
    fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            bytes: 0,
            limit: MAX_DEPENDENCY_BYTES,
        }
    }
}
fn organization_location(
    path: &Path,
    directory: bool,
    namespace: bool,
    max_bytes: u64,
) -> Result<OrganizationLocation> {
    use crate::organization::Parent;
    ensure!(
        path.is_absolute()
            && !path.components().any(|part| matches!(
                part,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )),
        "Invalid organization dependency path"
    );
    let wanted = if directory {
        path
    } else {
        path.parent()
            .context("Organization dependency has no parent")?
    };
    let mut existing = wanted.to_path_buf();
    let (parent, authority) = loop {
        match Parent::capture(&existing) {
            Ok(value) => break value,
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
            {
                ensure!(
                    existing.pop(),
                    "Organization dependency volume is unavailable"
                )
            }
            Err(error) => return Err(error),
        }
    };
    let mut value = OrganizationLocation {
        path: path_bytes(path),
        parent: authority.path.0,
        ancestors: authority.ancestors,
        identity: None,
        sha256: None,
        bytes: 0,
        kind: "missing".into(),
    };
    if existing == wanted {
        if directory {
            value.identity = value.ancestors.last().cloned();
            value.kind = "directory".into();
            if namespace {
                use sha2::Digest;
                value.sha256 = Some(format!(
                    "{:x}",
                    sha2::Sha256::digest(serde_json::to_vec(
                        &cuerithm_dj_sync::transaction::directory_manifest(path)?
                    )?)
                ));
            }
        } else {
            match parent.read(Path::new(
                path.file_name()
                    .context("Organization dependency has no name")?,
            )) {
                Ok(mut input) => {
                    let actual = crate::organization::fingerprint_bounded(&mut input, max_bytes)?;
                    value.identity = Some(actual.identity);
                    value.sha256 = Some(actual.sha256);
                    value.bytes = actual.bytes;
                    value.kind = "file".into();
                }
                Err(error)
                    if error
                        .downcast_ref::<std::io::Error>()
                        .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) => {}
                Err(error) => return Err(error),
            }
        }
    }
    Ok(value)
}
fn remember_organization_path<'a>(
    locations: &'a mut OrganizationLocations,
    path: &Path,
    directory: bool,
    namespace: bool,
) -> Result<&'a OrganizationLocation> {
    let key = (path_bytes(path), namespace);
    if !locations.entries.contains_key(&key) {
        ensure!(
            locations.entries.len() < MAX_ENTRIES,
            "Too many organization backup dependencies"
        );
        let remaining = locations
            .limit
            .checked_sub(locations.bytes)
            .context("Organization backup byte budget exhausted")?;
        let value = organization_location(path, directory, namespace, remaining)?;
        locations.bytes = locations
            .bytes
            .checked_add(value.bytes)
            .context("Organization backup size overflow")?;
        ensure!(
            locations.bytes <= locations.limit,
            "Organization dependency exceeds remaining byte budget"
        );
        locations.entries.insert(key.clone(), value);
    }
    Ok(locations.entries.get(&key).unwrap())
}
fn required_organization_file(
    locations: &mut OrganizationLocations,
    path: &Path,
    parent: &crate::organization::Authority,
    expected: &crate::organization::Fingerprint,
    complete: bool,
) -> Result<()> {
    let observed = remember_organization_path(locations, path, false, false)?;
    ensure!(
        observed.parent == parent.path.0
            && observed.ancestors == parent.ancestors
            && observed.identity.as_ref() == Some(&expected.identity)
            && (!complete
                || (observed.sha256.as_deref() == Some(expected.sha256.as_str())
                    && observed.bytes == expected.bytes)),
        "An organization original, sidecar or recovery stage is missing, changed or replaced"
    );
    Ok(())
}
#[derive(Deserialize)]
struct BackupReferenceIntent {
    version: u8,
    entries: Vec<BackupReferenceEntry>,
}
#[derive(Deserialize)]
struct BackupReferenceEntry {
    journal: Option<String>,
    #[serde(default)]
    abandoned: Vec<String>,
    receipt: Option<BackupReferenceReceipt>,
}
#[derive(Deserialize)]
struct BackupReferenceReceipt {
    #[serde(with = "cuerithm_dj_sync::path_serde::map")]
    after_files: BTreeMap<PathBuf, Option<String>>,
    before: BackupReferenceBefore,
}
#[derive(Deserialize)]
struct BackupReferenceBefore {
    #[serde(default, with = "cuerithm_dj_sync::path_serde::map")]
    directories: BTreeMap<PathBuf, Option<Vec<cuerithm_dj_sync::transaction::DirectoryEntry>>>,
}
fn organization_transaction_resources(
    locations: &mut OrganizationLocations,
    root: &Path,
    id: &str,
    expected: cuerithm_dj_sync::transaction::JournalState,
) -> Result<()> {
    use cuerithm_dj_sync::transaction::FileTransaction;
    ensure!(
        uuid::Uuid::parse_str(id).is_ok_and(|uuid| uuid.to_string() == id),
        "Invalid organization native recovery journal ID"
    );
    let transaction = FileTransaction::open(root, id)?;
    let journal = transaction.inspect()?;
    ensure!(
        journal.id == id && journal.state == expected,
        "Organization native recovery is not settled"
    );
    for (index, change) in journal.changes.iter().enumerate() {
        for (folder, hash) in [
            ("backup", &change.before_sha256),
            ("staged", &change.after_sha256),
        ] {
            if let Some(hash) = hash {
                let path = transaction.path().join(folder).join(index.to_string());
                let file = remember_organization_path(locations, &path, false, false)?;
                ensure!(
                    file.sha256.as_deref() == Some(hash.as_str()),
                    "Organization native recovery bytes are missing or changed"
                );
            }
        }
    }
    Ok(())
}
// Keep these projections independent of the optional tagging decoder. Both
// tagging-enabled and engine-only builds must validate retained native evidence.
#[derive(Deserialize)]
struct BackupMetadataStage {
    path: Vec<u8>,
    backup: Vec<u8>,
    workspace: Vec<u8>,
    before: String,
    after: String,
    workspace_authority: crate::organization::Authority,
    staged_fingerprint: crate::organization::Fingerprint,
    backup_fingerprint: crate::organization::Fingerprint,
}
fn metadata_stage(
    locations: &mut OrganizationLocations,
    root: &Path,
    json: &str,
    staged_required: bool,
    backup_required: bool,
) -> Result<()> {
    let stage: BackupMetadataStage = serde_json::from_str(json)?;
    let workspace = bytes_path(&stage.workspace)?;
    ensure!(
        workspace.starts_with(root.canonicalize()?.join("metadata-edits/staging"))
            && stage.workspace_authority.path.path()? == workspace
            && stage.before == stage.backup_fingerprint.sha256
            && stage.after == stage.staged_fingerprint.sha256,
        "Invalid retained metadata workspace or recovery identity"
    );
    for (path, proof, required) in [
        (&stage.backup, &stage.backup_fingerprint, backup_required),
        (&stage.path, &stage.staged_fingerprint, staged_required),
    ] {
        let path = bytes_path(path)?;
        ensure!(
            path.parent() == Some(workspace.as_path()),
            "Metadata recovery file leaves its workspace"
        );
        if !required && !backup_required && path == bytes_path(&stage.backup)? {
            // The save settled and deleted its original on purpose.
            continue;
        }
        if required {
            required_organization_file(locations, &path, &stage.workspace_authority, proof, true)?;
        } else {
            remember_organization_path(locations, &path, false, false)?;
        }
    }
    Ok(())
}
fn metadata_inventory(
    conn: &Connection,
    root: &Path,
    locations: &mut OrganizationLocations,
) -> Result<()> {
    // Imported artwork also backs saved match-review decisions. Never bless
    // missing or modified bytes just because a new backup is being created.
    let mut query =
        conn.prepare("SELECT path,sha256,bytes FROM metadata_artwork_assets ORDER BY id")?;
    for row in query.query_map([], |row| {
        Ok((
            row.get::<_, Vec<u8>>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, u64>(2)?,
        ))
    })? {
        let (path, hash, bytes) = row?;
        let path = bytes_path(&path)?;
        ensure!(
            path.starts_with(root.canonicalize()?.join("metadata-edits/assets")),
            "Metadata artwork leaves application assets"
        );
        let observed = remember_organization_path(locations, &path, false, false)?;
        ensure!(
            observed.sha256.as_deref() == Some(hash.as_str()) && observed.bytes == bytes,
            "Retained metadata artwork is missing or changed"
        );
    }
    // Every physical committed/undone write retains an original with native
    // ownership proof. Failed pre-publication writes keep stage evidence too.
    let mut query = conn.prepare("SELECT json_extract(payload_json,'$.row.stage'),state,COALESCE(json_extract(payload_json,'$.backup_retired'),0) FROM metadata_edit_writes ORDER BY id")?;
    for row in query.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, bool>(2)?,
        ))
    })? {
        let (stage, state, retired) = row?;
        metadata_stage(locations, root, &stage, state == "failed", !retired)?;
    }
    // Incoming overrides have no physical metadata_edit_write. Their original
    // is retained in the frozen row for inverse artwork and comment edits. A save that was
    // undone or left the Undo window deletes it too, and until that has happened (a failed
    // delete is retried by a sweep) the file is still on disk and still bound here.
    let mut query = conn.prepare("SELECT json_extract(r.payload_json,'$.stage') FROM metadata_edit_rows r JOIN metadata_edit_overrides o ON o.batch_id=r.batch_id AND o.track_id=r.track_id WHERE r.state='written' AND o.state IN ('committed','expired','undone') AND COALESCE(json_extract(r.payload_json,'$.original_retired'),0)=0 AND json_type(r.payload_json,'$.stage')='object' ORDER BY r.id")?;
    for row in query.query_map([], |row| row.get::<_, String>(0))? {
        metadata_stage(locations, root, &row?, false, true)?;
    }
    let mut query = conn.prepare("SELECT json_extract(row.value,'$.stage') FROM metadata_edit_previews p,json_each(p.payload_json,'$.rows') row WHERE p.state='preview' AND json_type(row.value,'$.stage')='object' ORDER BY p.id,row.key")?;
    for row in query.query_map([], |row| row.get::<_, String>(0))? {
        metadata_stage(locations, root, &row?, true, true)?;
    }
    // Bind native identities for the entire retained tree, including workspace
    // directories used to reopen Undo artifacts after a same-installation restore.
    let directory = root.join("metadata-edits");
    if directory.exists() {
        for entry in walkdir::WalkDir::new(&directory)
            .follow_links(false)
            .max_depth(32)
        {
            let entry = entry?;
            ensure!(
                entry.depth() < 32,
                "Metadata resource tree exceeds depth limit"
            );
            remember_organization_path(locations, entry.path(), entry.file_type().is_dir(), false)?;
        }
    }
    // A settled publication cannot leave external temporary files for startup
    // recovery to mutate. Bind their absence and containing native authorities.
    let mut query = conn.prepare(
        "SELECT publish_path,displaced_path FROM metadata_edit_files ORDER BY write_id,ordinal",
    )?;
    for row in query.query_map([], |row| {
        Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?))
    })? {
        let (publish, displaced) = row?;
        for path in [publish, displaced] {
            ensure!(
                remember_organization_path(locations, &bytes_path(&path)?, false, false)?.kind
                    == "missing",
                "Resolve retained external metadata publication artifacts before using a library backup"
            );
        }
    }
    let mut query = conn.prepare("SELECT json_extract(t.value,'$.path') FROM metadata_edit_writes w,json_each(w.payload_json,'$.retained_temporaries') t ORDER BY w.id,t.key")?;
    for row in query.query_map([], |row| row.get::<_, String>(0))? {
        let path: Vec<u8> = serde_json::from_str(&row?)?;
        ensure!(
            remember_organization_path(locations, &bytes_path(&path)?, false, false)?.kind
                == "missing",
            "Resolve retained external metadata temporary copies before using a library backup"
        );
    }
    Ok(())
}

fn preparation_inventory(conn: &Connection, locations: &mut OrganizationLocations) -> Result<()> {
    // Preset rules, revisions, folder assignments and captured authorities are
    // in SQLite. Bind the external directories without adopting new authorities.
    let mut query = conn.prepare("SELECT path FROM preparation_preset_folders ORDER BY id")?;
    for path in query.query_map([], |row| row.get::<_, Vec<u8>>(0))? {
        remember_organization_path(locations, &bytes_path(&path?)?, true, false)?;
    }
    // Current native Curate source proofs authorize later mapped edits and Undo.
    // Old proofs after an unrelated edit stay historical; they grant no authority.
    let mut query = conn.prepare(
        "SELECT c.input_proof_json,c.input_json,t.path,t.source_hash,t.path_revision,t.metadata_revision
         FROM curation_tracks c JOIN tracks t ON t.id=c.track_id WHERE c.input_proof_json IS NOT NULL AND t.path IS NOT NULL AND t.source_hash IS NOT NULL",
    )?;
    #[derive(Deserialize)]
    struct Proof {
        path: crate::organization::NativePath,
        parent: crate::organization::Authority,
        fingerprint: crate::organization::Fingerprint,
    }
    #[derive(Deserialize)]
    struct Input {
        path: Vec<u8>,
        source_hash: String,
        path_revision: u64,
        metadata_revision: u64,
    }
    for row in query.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Vec<u8>>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, u64>(4)?,
            row.get::<_, u64>(5)?,
        ))
    })? {
        let (proof, input, path, hash, path_revision, metadata_revision) = row?;
        let input: Input = serde_json::from_str(&input)?;
        if input.path != path
            || input.source_hash != hash
            || input.path_revision != path_revision
            || input.metadata_revision != metadata_revision
        {
            continue;
        }
        let proof: Proof = serde_json::from_str(&proof)?;
        ensure!(
            proof.path.0 == path && proof.fingerprint.sha256 == hash,
            "Curate source proof does not match its saved input"
        );
        let path = proof.path.path()?;
        let observed = remember_organization_path(locations, &path, false, false)?;
        if observed.kind != "missing" {
            required_organization_file(locations, &path, &proof.parent, &proof.fingerprint, true)?;
        }
    }
    Ok(())
}

fn organization_inventory(conn: &Connection, root: &Path) -> Result<Vec<OrganizationLocation>> {
    use crate::organization::{
        Authority, Fingerprint, OrganizationMode, OrganizationPhase, OrganizationRecord,
    };
    use cuerithm_dj_sync::transaction::JournalState;
    let mut locations = OrganizationLocations::new();
    metadata_inventory(conn, root, &mut locations)?;
    preparation_inventory(conn, &mut locations)?;
    let mut records = BTreeMap::new();
    let mut query =
        conn.prepare("SELECT id,state,payload_json FROM organization_operations ORDER BY id")?;
    for row in query.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })? {
        let (id, state, json) = row?;
        let record: OrganizationRecord = serde_json::from_str(&json)?;
        ensure!(
            record.id == id && record.phase.key() == state && record.error.is_none(),
            "Organization recovery history is inconsistent"
        );
        for file in &record.files {
            for name in std::iter::once(&file.stage_name)
                .chain(std::iter::once(&file.quarantine_name))
                .chain(file.abandoned_stages.iter().map(|(name, _)| name))
            {
                ensure!(
                    matches!(
                        Path::new(name).components().next(),
                        Some(std::path::Component::Normal(_))
                    ) && Path::new(name).components().count() == 1,
                    "Invalid organization recovery file name"
                );
            }
            let source = file.source.path()?;
            let destination = file.destination.path()?;
            // Old destinations can legitimately be absent after subsequent moves
            // or Undo. Their actual namespace state is still bound to the backup.
            remember_organization_path(&mut locations, &source, false, false)?;
            remember_organization_path(&mut locations, &destination, false, false)?;
            let quarantine = file.source_parent.path.path()?.join(&file.quarantine_name);
            let stage = file.destination_parent.path.path()?.join(&file.stage_name);
            if file.quarantined {
                required_organization_file(
                    &mut locations,
                    &quarantine,
                    &file.source_parent,
                    &file.original,
                    true,
                )?;
            } else {
                remember_organization_path(&mut locations, &quarantine, false, false)?;
            }
            if !file.published
                && let Some(expected) = &file.staged
            {
                required_organization_file(
                    &mut locations,
                    &stage,
                    &file.destination_parent,
                    expected,
                    file.stage_complete,
                )?;
            } else {
                remember_organization_path(&mut locations, &stage, false, false)?;
            }
            for (name, expected) in &file.abandoned_stages {
                required_organization_file(
                    &mut locations,
                    &file.destination_parent.path.path()?.join(name),
                    &file.destination_parent,
                    expected,
                    true,
                )?;
            }
            if record.mode == OrganizationMode::Copy
                && record.phase == OrganizationPhase::Complete
                && record.undo_of.is_none()
            {
                // Sidecar originals have no alternate row, but remain Copy Undo inputs.
                if file.source != record.files[0].source {
                    required_organization_file(
                        &mut locations,
                        &source,
                        &file.source_parent,
                        &file.original,
                        true,
                    )?;
                }
            }
        }
        for directory in &record.created_directories {
            remember_organization_path(&mut locations, &directory.path.path()?, true, false)?;
        }
        records.insert(id, record);
    }
    let mut query = conn.prepare(
        "SELECT path,source_hash FROM organization_owned_sidecars ORDER BY track_id,path",
    )?;
    for row in query.query_map([], |row| {
        Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, String>(1)?))
    })? {
        let (path, hash) = row?;
        let actual = remember_organization_path(&mut locations, &bytes_path(&path)?, false, false)?;
        ensure!(
            actual.sha256.as_deref() == Some(hash.as_str()),
            "An owned organization sidecar is unavailable or changed"
        );
    }
    let mut query=conn.prepare("SELECT operation_id,path,state,source_hash,ignored_hash FROM organization_alternates WHERE state!='retired' ORDER BY operation_id")?;
    for row in query.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Vec<u8>>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<String>>(4)?,
        ))
    })? {
        let (id, path, state, hash, ignored) = row?;
        let record = records
            .get(&id)
            .context("Copy original lost its organization history")?;
        let file = record
            .files
            .first()
            .context("Copy original has no file proof")?;
        let path = bytes_path(&path)?;
        if state == "active" {
            required_organization_file(
                &mut locations,
                &path,
                &file.source_parent,
                &file.original,
                true,
            )?;
            ensure!(
                file.original.sha256 == hash,
                "Copy source ownership hash is inconsistent"
            );
        } else if state == "ignored" {
            #[derive(Deserialize)]
            struct Choice {
                source_parent: Authority,
                source: Fingerprint,
            }
            let saved:Option<(String,String)>=conn.query_row("SELECT state,payload_json FROM organization_copy_conflicts WHERE operation_id=?1",[&id],|row|Ok((row.get(0)?,row.get(1)?))).optional()?;
            if let Some((state, json)) = saved {
                ensure!(
                    state == "committed",
                    "Ignored Copy revision is not committed"
                );
                let choice: Choice = serde_json::from_str(&json)?;
                ensure!(
                    ignored.as_deref() == Some(choice.source.sha256.as_str()),
                    "Ignored Copy revision has inconsistent ownership proof"
                );
                required_organization_file(
                    &mut locations,
                    &path,
                    &choice.source_parent,
                    &choice.source,
                    true,
                )?;
            } else {
                // Match main's pre-proof ignored-Copy compatibility, while binding
                // this backup to the exact currently observed native object.
                let actual = remember_organization_path(&mut locations, &path, false, false)?;
                ensure!(
                    actual.kind == "file"
                        && actual.sha256 == ignored
                        && file.source.path()? == path,
                    "Legacy ignored Copy revision changed or is unavailable"
                );
            }
        } else {
            anyhow::bail!("Resolve organization Copy-source conflicts before using this backup");
        }
    }
    let mut query = conn.prepare("SELECT path FROM organization_roots ORDER BY id")?;
    for row in query.query_map([], |row| row.get::<_, Vec<u8>>(0))? {
        remember_organization_path(&mut locations, &bytes_path(&row?)?, true, false)?;
    }
    let mut query=conn.prepare("SELECT operation_id,state,payload_json FROM organization_reference_intents ORDER BY operation_id")?;
    for row in query.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })? {
        let (id, state, json) = row?;
        ensure!(
            uuid::Uuid::parse_str(&id).is_ok_and(|uuid| uuid.to_string() == id),
            "Invalid organization reference operation ID"
        );
        let intent: BackupReferenceIntent = serde_json::from_str(&json)?;
        ensure!(
            intent.version == 1,
            "Unsupported organization reference intent"
        );
        let journal_root = root.join("organization-references").join(&id);
        for entry in intent.entries {
            if let Some(journal) = entry.journal {
                ensure!(
                    state == "acknowledged",
                    "Organization reference repair needs acknowledgement"
                );
                organization_transaction_resources(
                    &mut locations,
                    &journal_root,
                    &journal,
                    JournalState::Applied,
                )?;
            } else {
                ensure!(
                    entry.receipt.is_none(),
                    "Organization reference receipt lost its recovery journal"
                );
            }
            for journal in entry.abandoned {
                organization_transaction_resources(
                    &mut locations,
                    &journal_root,
                    &journal,
                    JournalState::Restored,
                )?;
            }
            if let Some(receipt) = entry.receipt {
                for path in receipt.after_files.keys() {
                    remember_organization_path(&mut locations, path, false, false)?;
                }
                for path in receipt.before.directories.keys() {
                    remember_organization_path(&mut locations, path, true, true)?;
                }
            }
        }
    }
    let values = locations.entries.into_values().collect::<Vec<_>>();
    let bytes = values.iter().try_fold(0u64, |sum, value| {
        sum.checked_add(value.bytes)
            .context("Organization backup size overflow")
    })?;
    ensure!(
        values.len() <= MAX_ENTRIES && bytes <= MAX_DEPENDENCY_BYTES,
        "Organization dependencies exceed backup budget"
    );
    Ok(values)
}

fn copy_database(from: &Connection, destination: &Path) -> Result<()> {
    let mut to = Connection::open(destination)?;
    copy_connection(from, &mut to)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(destination, fs::Permissions::from_mode(0o600))?;
    }
    to.pragma_update(None, "journal_mode", "DELETE")?;
    to.close().map_err(|(_, e)| e)?;
    regular(destination, MAX_DATABASE)?;
    File::options()
        .read(true)
        .write(true)
        .open(destination)?
        .sync_all()?;
    Ok(())
}
fn copy_connection(from: &Connection, to: &mut Connection) -> Result<()> {
    let backup = Backup::new(from, to)?;
    let start = Instant::now();
    loop {
        match backup.step(256)? {
            StepResult::Done => break,
            StepResult::More => {}
            StepResult::Busy | StepResult::Locked => std::thread::sleep(Duration::from_millis(10)),
            _ => {}
        }
        ensure!(
            start.elapsed() < Duration::from_secs(60),
            "Library backup timed out; original database is preserved"
        );
    }
    Ok(())
}
fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let parent = path.parent().context("Missing backup parent")?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(&serde_json::to_vec_pretty(value)?)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| e.error)?;
    sync_directory(parent)
}
fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
struct PreparedSnapshot {
    directory: tempfile::TempDir,
    manifest: Manifest,
}
fn prepare_snapshot(
    conn: &Connection,
    root: &Path,
    installation: &Path,
    destination: &Path,
) -> Result<PreparedSnapshot> {
    settled(conn, root)?;
    let schema = schema(conn)?;
    let installation_id = installation_id(installation, true)?;
    let (sources, models, resources, organization) = inventory(conn, root)?;
    fs::create_dir_all(destination)?;
    ensure!(
        !fs::symlink_metadata(destination)?.file_type().is_symlink(),
        "Backup destination cannot be a symbolic link"
    );
    let temp = tempfile::Builder::new()
        .prefix(".cuerithm-backup-")
        .tempdir_in(destination)?;
    copy_database(conn, &temp.path().join("library.sqlite"))?;
    ensure!(
        (
            sources.clone(),
            models.clone(),
            resources.clone(),
            organization.clone()
        ) == inventory(conn, root)?,
        "Library dependencies changed while creating the backup; try again"
    );
    let created = now();
    let manifest = Manifest {
        format: 1,
        application: "CueRithm".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        platform: std::env::consts::OS.into(),
        installation: root_identity(installation)?,
        installation_id,
        data_directory: Some(path_bytes(root)),
        created,
        schema,
        database_sha256: cuerithm_media::file_sha256(&temp.path().join("library.sqlite"))?,
        music_included: false,
        sources,
        models,
        resources,
        organization,
    };
    atomic_json(&temp.path().join("manifest.json"), &manifest)?;
    regular(&temp.path().join("manifest.json"), MAX_MANIFEST)?;
    Ok(PreparedSnapshot {
        directory: temp,
        manifest,
    })
}
fn publish_snapshot(
    prepared: PreparedSnapshot,
    destination: &Path,
) -> Result<LibraryBackupPreview> {
    let path = destination.join(format!(
        "CueRithm-{}-{}",
        prepared.manifest.created,
        uuid::Uuid::new_v4()
    ));
    fs::rename(prepared.directory.path(), &path)?;
    sync_directory(destination)?;
    preview(&path, &prepared.manifest)
}
fn snapshot(
    conn: &Connection,
    root: &Path,
    installation: &Path,
    destination: &Path,
) -> Result<LibraryBackupPreview> {
    publish_snapshot(
        prepare_snapshot(conn, root, installation, destination)?,
        destination,
    )
}
fn preview(path: &Path, m: &Manifest) -> Result<LibraryBackupPreview> {
    let conn = readonly(&path.join("library.sqlite"))?;
    let (tracks, unresolved): (u64, u64) = conn.query_row(
        "SELECT COUNT(*),COUNT(*) FILTER(WHERE path IS NULL) FROM tracks",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    Ok(LibraryBackupPreview {
        path: path.to_path_buf(),
        database_sha256: m.database_sha256.clone(),
        created: m.created,
        tracks,
        missing_sources: m.sources.iter().filter(|s| s.kind == "missing").count() as u64
            + unresolved,
        schema: m.schema,
        music_included: false,
    })
}
fn inspect(bundle: &Path, root: &Path, current_schema: i64) -> Result<(Manifest, Connection)> {
    let meta = fs::symlink_metadata(bundle)?;
    ensure!(
        meta.is_dir() && !meta.file_type().is_symlink(),
        "Select a regular CueRithm backup folder"
    );
    let entries = fs::read_dir(bundle)?.collect::<std::io::Result<Vec<_>>>()?;
    ensure!(
        entries.len() == 2
            && entries.iter().all(|e| matches!(
                e.file_name().to_str(),
                Some("library.sqlite" | "manifest.json")
            )),
        "Backup contains unexpected entries"
    );
    regular(&bundle.join("manifest.json"), MAX_MANIFEST)?;
    regular(&bundle.join("library.sqlite"), MAX_DATABASE)?;
    let m: Manifest = serde_json::from_slice(&fs::read(bundle.join("manifest.json"))?)?;
    ensure!(
        m.format == 1 && m.application == "CueRithm" && !m.music_included,
        "Unsupported backup format"
    );
    ensure!(
        m.version == env!("CARGO_PKG_VERSION")
            && m.platform == std::env::consts::OS
            && m.installation == root_identity(root)?
            && m.installation_id == installation_id(root, false)?,
        "This backup requires the same CueRithm version, platform and library installation"
    );
    ensure!(
        cuerithm_media::file_sha256(&bundle.join("library.sqlite"))? == m.database_sha256,
        "Backup database hash does not match its manifest"
    );
    let conn = readonly(&bundle.join("library.sqlite"))?;
    ensure!(
        schema(&conn)? == current_schema && m.schema == current_schema,
        "Backup library schema differs from the current library"
    );
    let integrity: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
    ensure!(integrity == "ok", "Backup SQLite integrity check failed");
    let violations: bool = conn.prepare("PRAGMA foreign_key_check")?.exists([])?;
    ensure!(!violations, "Backup has broken library references");
    let data = snapshot_data_directory(&m, root)?;
    settled(&conn, &data)?;
    let (sources, models, resources, organization) = inventory(&conn, &data)?;
    ensure!(
        m.sources == sources,
        "Backup source locations have changed. Music files and tag edits are not restored"
    );
    ensure!(
        m.models == models && m.resources == resources,
        "Retained recovery resources or models have changed; this database-only backup cannot be restored safely"
    );
    ensure!(
        m.organization == organization,
        "Organization or metadata recovery resources, retained originals or native identities changed; this backup cannot restore them"
    );
    Ok((m, conn))
}
// This stamp covers mutations on the shared connection as well as commits by
// another SQLite connection. Application/schema identities are checked too; no
// file hashes or filesystem traversal occur while reading or comparing it.
#[derive(Debug, PartialEq, Eq)]
struct DatabaseStamp {
    total_changes: u64,
    data_version: i64,
    schema: i64,
    schema_cookie: i64,
    revision: u64,
}
impl DatabaseStamp {
    fn read(conn: &Connection) -> Result<Self> {
        Ok(Self {
            total_changes: conn.total_changes(),
            data_version: conn.query_row("PRAGMA data_version", [], |row| row.get(0))?,
            schema: schema(conn)?,
            schema_cookie: conn.query_row("PRAGMA schema_version", [], |row| row.get(0))?,
            revision: conn.query_row("SELECT value FROM meta WHERE key='revision'", [], |row| {
                row.get(0)
            })?,
        })
    }
}
struct CapturedDatabase {
    // Close the connection before deleting its private temporary directory.
    connection: Connection,
    _directory: tempfile::TempDir,
    stamp: DatabaseStamp,
}
impl Library {
    fn capture_backup_database(&self) -> Result<CapturedDatabase> {
        let directory = tempfile::Builder::new()
            .prefix("cuerithm-library-snapshot-")
            .tempdir()?;
        let path = directory.path().join("library.sqlite");
        let stamp = {
            let conn = self.conn()?;
            quiet_database(&conn)?;
            let stamp = DatabaseStamp::read(&conn)?;
            copy_database(&conn, &path)?;
            ensure!(
                stamp == DatabaseStamp::read(&conn)?,
                "Library changed while capturing its database; try again"
            );
            stamp
        };
        let connection = readonly(&path)?;
        Ok(CapturedDatabase {
            connection,
            _directory: directory,
            stamp,
        })
    }
    fn revalidate_backup_database(
        &self,
        captured: &CapturedDatabase,
    ) -> Result<crate::store::ConnectionGuard<'_>> {
        let conn = self.conn()?;
        ensure!(
            captured.stamp == DatabaseStamp::read(&conn)?,
            "Library changed while checking backup files; try again"
        );
        quiet_database(&conn)?;
        Ok(conn)
    }
    pub fn create_library_backup(&self, destination: &Path) -> Result<LibraryBackupPreview> {
        let _sync = sync_lease(self.data_dir())?;
        let _organization = self
            .inner
            .organization_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Organization gate interrupted"))?;
        #[cfg(feature = "autotag")]
        let _gate = self
            .inner
            .tag_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Publication gate interrupted"))?;
        #[cfg(feature = "autotag")]
        let _cleanup = self
            .inner
            .artifact_cleanup_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Artifact cleanup gate interrupted"))?;
        let captured = self.capture_backup_database()?;
        let prepared = prepare_snapshot(
            &captured.connection,
            self.data_dir(),
            self.installation_dir(),
            destination,
        )?;
        let _current = self.revalidate_backup_database(&captured)?;
        publish_snapshot(prepared, destination)
    }
    pub fn inspect_library_backup(&self, bundle: &Path) -> Result<LibraryBackupPreview> {
        let _sync = sync_lease(self.data_dir())?;
        let _organization = self
            .inner
            .organization_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Organization gate interrupted"))?;
        #[cfg(feature = "autotag")]
        let _gate = self
            .inner
            .tag_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Publication gate interrupted"))?;
        #[cfg(feature = "autotag")]
        let _cleanup = self
            .inner
            .artifact_cleanup_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Artifact cleanup gate interrupted"))?;
        let captured = self.capture_backup_database()?;
        settled(&captured.connection, self.data_dir())?;
        let (manifest, _) = inspect(bundle, self.installation_dir(), captured.stamp.schema)?;
        let _current = self.revalidate_backup_database(&captured)?;
        preview(bundle, &manifest)
    }
    pub fn stage_library_restore(
        &self,
        bundle: &Path,
        sha256: &str,
    ) -> Result<LibraryRestoreRequest> {
        let root = self.installation_dir();
        let _activation = file_lease(
            &root.join("generation-switch.lock"),
            "Another library switch is running",
        )?;
        ensure!(
            !root.join("library-activation.json").exists(),
            "Finish or cancel the scheduled library activation first"
        );
        let _sync = sync_lease(self.data_dir())?;
        let _organization = self
            .inner
            .organization_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Organization gate interrupted"))?;
        #[cfg(feature = "autotag")]
        let _gate = self
            .inner
            .tag_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Publication gate interrupted"))?;
        #[cfg(feature = "autotag")]
        let _cleanup = self
            .inner
            .artifact_cleanup_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Artifact cleanup gate interrupted"))?;
        let captured = self.capture_backup_database()?;
        settled(&captured.connection, self.data_dir())?;
        let schema = captured.stamp.schema;
        let (m, _) = inspect(bundle, root, schema)?;
        ensure!(
            m.database_sha256 == sha256,
            "Backup selection changed; inspect it again"
        );
        let directory = managed_directory(root)?;
        fs::create_dir_all(&directory)?;
        ensure!(
            !directory.join("pending.json").exists(),
            "A library restore is already scheduled"
        );
        let request = LibraryRestoreRequest {
            id: uuid::Uuid::new_v4().to_string(),
            created: now(),
            database_sha256: sha256.into(),
            blocked_reason: None,
            mutation_started: false,
            pre_restore: None,
        };
        let staging = directory.join(&request.id);
        fs::create_dir(&staging)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&staging, fs::Permissions::from_mode(0o700))?;
        }
        for name in ["library.sqlite", "manifest.json"] {
            let target = staging.join(name);
            fs::copy(bundle.join(name), &target)?;
            File::options()
                .read(true)
                .write(true)
                .open(target)?
                .sync_all()?;
        }
        inspect(&staging, root, schema)?;
        sync_directory(&staging)?;
        let _current = self.revalidate_backup_database(&captured)?;
        atomic_json(&directory.join("pending.json"), &request)?;
        Ok(request)
    }
    pub fn retry_library_restore(&self) -> Result<LibraryRestoreRequest> {
        let root = self.installation_dir();
        let _sync = sync_lease(self.data_dir())?;
        let _organization = self
            .inner
            .organization_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Organization gate interrupted"))?;
        #[cfg(feature = "autotag")]
        let _gate = self
            .inner
            .tag_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Publication gate interrupted"))?;
        #[cfg(feature = "autotag")]
        let _cleanup = self
            .inner
            .artifact_cleanup_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Artifact cleanup gate interrupted"))?;
        let captured = self.capture_backup_database()?;
        settled(&captured.connection, self.data_dir())?;
        let mut request = pending(root)?.context("No library restore is scheduled")?;
        ensure!(
            !request.mutation_started,
            "Restore recovery requires the retained pre-restore snapshot"
        );
        let directory = managed_directory(root)?;
        let (manifest, _) = inspect(&directory.join(&request.id), root, captured.stamp.schema)?;
        ensure!(
            manifest.database_sha256 == request.database_sha256,
            "Staged restore identity changed"
        );
        let _current = self.revalidate_backup_database(&captured)?;
        request.blocked_reason = None;
        atomic_json(&directory.join("pending.json"), &request)?;
        Ok(request)
    }
    pub fn pending_library_restore(&self) -> Result<Option<LibraryRestoreRequest>> {
        pending(self.installation_dir())
    }
    pub fn cancel_library_restore(&self) -> Result<()> {
        let _sync = sync_lease(self.data_dir())?;
        let Some(intent) = pending(self.installation_dir())? else {
            return Ok(());
        };
        ensure!(
            !intent.mutation_started && intent.pre_restore.is_none(),
            "A started restore requires recovery; its evidence cannot be cancelled"
        );
        let directory = managed_directory(self.installation_dir())?;
        let staged = directory.join(&intent.id);
        match fs::symlink_metadata(&staged) {
            Ok(meta) => {
                ensure!(
                    meta.is_dir() && !meta.file_type().is_symlink(),
                    "Staged restore directory changed"
                );
                // Validate the complete directory before removing any owned leaf.
                // Missing leaves permit retry after an interrupted cancellation.
                let mut files = Vec::new();
                for entry in fs::read_dir(&staged)? {
                    let entry = entry?;
                    ensure!(
                        matches!(
                            entry.file_name().to_str(),
                            Some("library.sqlite" | "manifest.json")
                        ),
                        "Staged restore contains an unowned entry"
                    );
                    ensure!(
                        entry.file_type()?.is_file(),
                        "Staged restore entry is not an owned regular file"
                    );
                    files.push(entry.path());
                }
                for file in files {
                    fs::remove_file(file)?;
                }
                fs::remove_dir(&staged)?;
                sync_directory(&directory)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        fs::remove_file(directory.join("pending.json"))?;
        sync_directory(&directory)?;
        Ok(())
    }
}
fn pending(root: &Path) -> Result<Option<LibraryRestoreRequest>> {
    let path = managed_directory(root)?.join("pending.json");
    if !path.exists() {
        return Ok(None);
    }
    regular(&path, MAX_MANIFEST)?;
    let request: LibraryRestoreRequest = serde_json::from_slice(&fs::read(path)?)?;
    ensure!(
        uuid::Uuid::parse_str(&request.id).is_ok(),
        "Invalid staged library restore ID"
    );
    Ok(Some(request))
}
pub(crate) fn ensure_library_open_safe(root: &Path) -> Result<()> {
    ensure!(
        pending(root)?.is_none_or(|request| !request.mutation_started),
        "Library restore stopped during database update; apply its pending restore before opening this library"
    );
    Ok(())
}
/// Call before `Library::open` or starting workers. A blocked preflight leaves the
/// original untouched and permits normal startup with a visible cancellable intent.
/// An error after durable mutation intent must stop startup for recovery.
pub fn apply_pending_library_restore(database: &Path) -> Result<LibraryRestoreStartup> {
    let root = database.parent().context("Library has no data directory")?;
    apply_pending_in_installation(database, root, &|_| Ok(()))
}
pub(crate) fn apply_pending_in_installation(
    database: &Path,
    root: &Path,
    activate: &dyn Fn(&Path) -> Result<()>,
) -> Result<LibraryRestoreStartup> {
    let Some(mut request) = pending(root)? else {
        return Ok(LibraryRestoreStartup::NoPending);
    };
    // Match Library::open's canonical private root (including macOS /var aliases).
    // Resolve only scheduled restores so first startup needs no existing data root.
    let directory_root = root.canonicalize()?;
    let root = directory_root.as_path();
    if let Some(reason) = request.blocked_reason.clone() {
        ensure!(
            !request.mutation_started,
            "Library restore stopped during database update; retain the pre-restore snapshot and stop startup"
        );
        return Ok(LibraryRestoreStartup::BlockedBeforeMutation { request, reason });
    }
    let directory = managed_directory(root)?;
    let target_data = (|| -> Result<PathBuf> {
        regular(
            &directory.join(&request.id).join("manifest.json"),
            MAX_MANIFEST,
        )?;
        let staged: Manifest = serde_json::from_slice(&fs::read(
            directory.join(&request.id).join("manifest.json"),
        )?)?;
        ensure!(
            staged.database_sha256 == request.database_sha256,
            "Staged restore identity changed"
        );
        snapshot_data_directory(&staged, root)
    })();
    let data = match target_data {
        Ok(data) => data,
        Err(error) if !request.mutation_started => {
            let reason = format!("{error:#}");
            request.blocked_reason = Some(reason.clone());
            atomic_json(&directory.join("pending.json"), &request)?;
            return Ok(LibraryRestoreStartup::BlockedBeforeMutation { request, reason });
        }
        Err(error) => return Err(error.context("Restore recovery data directory is unavailable")),
    };
    let target = data.join(
        database
            .file_name()
            .context("Library has no database name")?,
    );
    // A restore to a retained generation must also close the running generation.
    let _active_lease = if database.canonicalize()? != target.canonicalize()? {
        let mut lock = database.as_os_str().to_owned();
        lock.push(".lock");
        Some(file_lease(
            Path::new(&lock),
            "Close CueRithm before restoring this library",
        )?)
    } else {
        None
    };
    let database = target.as_path();
    let mut lock = database.as_os_str().to_owned();
    lock.push(".lock");
    let _lease = file_lease(
        Path::new(&lock),
        "Close CueRithm before restoring this library",
    )?;
    let _sync = sync_lease(&data)?;
    regular(database, MAX_DATABASE)?;
    let mut current = Connection::open(database)?;
    current.busy_timeout(Duration::from_secs(5))?;
    let directory = managed_directory(root)?;
    let validation = (|| -> Result<(Connection, PathBuf)> {
        settled(&current, &data)?;
        let version = schema(&current)?;
        let (manifest, source) = inspect(&directory.join(&request.id), root, version)?;
        ensure!(
            manifest.database_sha256 == request.database_sha256,
            "Staged restore identity changed"
        );
        let preserved = if request.mutation_started {
            let saved = request
                .pre_restore
                .as_ref()
                .context("Pre-restore recovery snapshot is missing")?;
            ensure!(
                !saved
                    .components()
                    .any(|part| matches!(part, std::path::Component::ParentDir))
                    && saved.canonicalize()?.starts_with(directory.canonicalize()?),
                "Invalid pre-restore recovery location"
            );
            inspect(saved, root, version)?;
            saved.clone()
        } else {
            let pre = directory.join(format!(
                "pre-restore-{}-{}",
                request.id,
                uuid::Uuid::new_v4()
            ));
            let temporary = tempfile::Builder::new()
                .prefix(".pre-restore-")
                .tempdir_in(&directory)?;
            let saved = snapshot(&current, &data, root, temporary.path())?;
            let name = saved
                .path
                .file_name()
                .context("Pre-restore snapshot has no name")?
                .to_owned();
            fs::rename(temporary.path(), &pre)?;
            sync_directory(&directory)?;
            let saved = pre.join(name);
            inspect(&saved, root, version)?;
            saved
        };
        Ok((source, preserved))
    })();
    let (source, preserved) = match validation {
        Ok(prepared) => prepared,
        Err(error) => {
            if request.mutation_started {
                return Err(error.context("Restore recovery stopped after database update intent; do not open this library until its pre-restore snapshot can be recovered"));
            }
            let reason = format!("{error:#}");
            request.blocked_reason = Some(reason.clone());
            atomic_json(&directory.join("pending.json"), &request)?;
            return Ok(LibraryRestoreStartup::BlockedBeforeMutation { request, reason });
        }
    };
    // Durable intent distinguishes a harmless failed preflight from a process crash
    // during SQLite's atomic update. Never replace its original recovery snapshot.
    request.pre_restore = Some(preserved.clone());
    request.mutation_started = true;
    atomic_json(&directory.join("pending.json"), &request)?;
    copy_connection(&source, &mut current)
        .context("Library restore stopped during database update; pre-restore snapshot retained")?;
    current.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    activate(&data)?;
    atomic_json(
        &directory.join(format!("applied-{}.json", request.id)),
        &request,
    )?;
    fs::remove_file(directory.join("pending.json"))?;
    sync_directory(&directory)?;
    Ok(LibraryRestoreStartup::Applied {
        pre_restore: preserved,
    })
}

#[cfg(test)]
mod inventory_phase_tests {
    use super::*;
    use std::cell::RefCell;
    #[test]
    fn generation_restore_retries_activation_without_losing_recovery() {
        let temp = tempfile::tempdir().unwrap();
        let manager = crate::LibraryGenerations::new(&temp.path().join("app")).unwrap();
        let legacy = manager.open_active().unwrap();
        let backup = legacy
            .create_library_backup(&temp.path().join("backups"))
            .unwrap();
        drop(legacy);
        let stage = manager.create_staging().unwrap();
        drop(Library::open(&stage.database).unwrap());
        let seal = manager.seal(&stage.id).unwrap();
        manager.schedule(&stage.id, "legacy", &seal).unwrap();
        let active = manager.open_active().unwrap();
        active
            .stage_library_restore(&backup.path, &backup.database_sha256)
            .unwrap();
        assert!(
            apply_pending_in_installation(&stage.database, manager.root(), &|_| Ok(())).is_err()
        );
        assert!(!pending(manager.root()).unwrap().unwrap().mutation_started);
        drop(active);
        let failure = apply_pending_in_installation(&stage.database, manager.root(), &|_| {
            anyhow::bail!("simulated activation interruption")
        });
        assert!(failure.is_err());
        let intent = pending(manager.root()).unwrap().unwrap();
        assert!(intent.mutation_started);
        assert!(intent.pre_restore.as_ref().unwrap().is_dir());
        assert!(Library::open_in_installation(&stage.database, manager.root()).is_err());
        let recovered = manager.open_active().unwrap();
        assert_eq!(manager.active_id().unwrap(), "legacy");
        assert!(recovered.pending_library_restore().unwrap().is_none());
        recovered.inspect_library_backup(&backup.path).unwrap();
        assert!(intent.pre_restore.unwrap().is_dir());
    }
    thread_local! {
        static HOOK: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
    }
    pub(super) fn run_hook() {
        let hook = HOOK.with(|hook| hook.borrow_mut().take());
        if let Some(hook) = hook {
            hook();
        }
    }
    fn during_inventory(action: impl FnOnce() + 'static) {
        HOOK.with(|hook| *hook.borrow_mut() = Some(Box::new(action)));
    }
    fn fixture() -> (tempfile::TempDir, Library) {
        let directory = tempfile::tempdir().unwrap();
        let library = Library::open(&directory.path().join("app/library.sqlite")).unwrap();
        let path = directory.path().join("authored.wav");
        fs::write(&path, b"authored source bytes").unwrap();
        library.conn().unwrap().execute("INSERT INTO tracks(id,path,title,source_hash,state,created,updated) VALUES('fixture',?1,'Authored',?2,'ready',0,0)",rusqlite::params![path_bytes(&path),cuerithm_media::file_sha256(&path).unwrap()]).unwrap();
        (directory, library)
    }
    #[test]
    fn schema_31_local_recovery_snapshot_retains_unresolved_records() {
        let (directory, library) = fixture();
        library.conn().unwrap().execute("INSERT INTO tracks(id,path,source_hash,title,state,created,updated) VALUES('unresolved',NULL,NULL,'Offline imported track','ready',0,0)",[]).unwrap();
        let saved = library
            .create_library_backup(&directory.path().join("backups"))
            .unwrap();
        assert_eq!(saved.schema, 31);
        assert_eq!(saved.tracks, 2);
        assert_eq!(saved.missing_sources, 1);
        let inspected = library.inspect_library_backup(&saved.path).unwrap();
        assert_eq!(inspected.tracks, 2);
        library
            .stage_library_restore(&saved.path, &saved.database_sha256)
            .unwrap();
        let database = library.data_dir().join("library.sqlite");
        drop(library);
        apply_pending_library_restore(&database).unwrap();
        let reopened = Library::open(&database).unwrap();
        assert_eq!(reopened.counts().unwrap().tracks, 2);
        assert!(
            reopened
                .local_track_binding("unresolved")
                .unwrap()
                .is_none()
        );
        assert!(
            reopened
                .source_hash_optional("unresolved")
                .unwrap()
                .is_none()
        );
    }
    fn query_during_inventory(library: &Library) {
        let library = library.clone();
        during_inventory(move || {
            // Inventory stays paused until an actual query on another thread
            // completes. try_lock makes the old retained-lock behavior fail
            // immediately instead of relying on sleeps or a timing threshold.
            std::thread::spawn(move || {
                let connection = library
                    .inner
                    .conn
                    .try_lock()
                    .expect("Source inventory must leave ordinary library queries available");
                let expected: u64 = connection
                    .query_row("SELECT value FROM meta WHERE key='revision'", [], |r| {
                        r.get(0)
                    })
                    .unwrap();
                drop(connection);
                assert_eq!(library.revision().unwrap(), expected);
            })
            .join()
            .unwrap();
        });
    }
    #[cfg(feature = "autotag")]
    #[test]
    fn metadata_artifact_cleanup_is_excluded_through_every_live_backup_inventory() {
        fn assert_cleanup_excluded(library: &Library) {
            let library = library.clone();
            during_inventory(move || {
                assert!(
                    matches!(
                        library.inner.artifact_cleanup_gate.try_lock(),
                        Err(std::sync::TryLockError::WouldBlock)
                    ),
                    "Backup resource proofs must exclude concurrent artifact cleanup"
                );
            });
        }
        let (d, library) = fixture();
        assert_cleanup_excluded(&library);
        let backup = library
            .create_library_backup(&d.path().join("backups"))
            .unwrap();
        assert_cleanup_excluded(&library);
        library.inspect_library_backup(&backup.path).unwrap();
        assert_cleanup_excluded(&library);
        library
            .stage_library_restore(&backup.path, &backup.database_sha256)
            .unwrap();
        assert_cleanup_excluded(&library);
        library.retry_library_restore().unwrap();
    }
    #[test]
    fn backup_inventory_keeps_library_queries_available() {
        let (d, library) = fixture();
        query_during_inventory(&library);
        library
            .create_library_backup(&d.path().join("backups"))
            .unwrap();
    }
    #[test]
    fn inspect_inventory_keeps_library_queries_available() {
        let (d, library) = fixture();
        let backup = library
            .create_library_backup(&d.path().join("backups"))
            .unwrap();
        query_during_inventory(&library);
        library.inspect_library_backup(&backup.path).unwrap();
    }
    #[test]
    fn stage_inventory_keeps_library_queries_available() {
        let (d, library) = fixture();
        let backup = library
            .create_library_backup(&d.path().join("backups"))
            .unwrap();
        query_during_inventory(&library);
        library
            .stage_library_restore(&backup.path, &backup.database_sha256)
            .unwrap();
    }
    #[test]
    fn retry_inventory_keeps_library_queries_available() {
        let (d, library) = fixture();
        let backup = library
            .create_library_backup(&d.path().join("backups"))
            .unwrap();
        library
            .stage_library_restore(&backup.path, &backup.database_sha256)
            .unwrap();
        query_during_inventory(&library);
        library.retry_library_restore().unwrap();
    }
    fn edit_during_inventory(library: &Library) {
        let library = library.clone();
        during_inventory(move || {
            let connection = library
                .inner
                .conn
                .try_lock()
                .expect("Editing during inventory must not block on its global connection");
            connection
                .execute(
                    "UPDATE tracks SET title='Edited during backup inventory' WHERE id='fixture'",
                    [],
                )
                .unwrap();
        });
    }
    #[test]
    fn backup_refuses_to_publish_after_a_library_edit_during_inventory() {
        let (d, library) = fixture();
        let destination = d.path().join("backups");
        edit_during_inventory(&library);
        let error = library.create_library_backup(&destination).unwrap_err();
        assert!(error.to_string().contains("Library changed"));
        assert_eq!(fs::read_dir(destination).unwrap().count(), 0);
    }
    #[test]
    fn inspection_refuses_a_stale_preview_after_a_library_edit() {
        let (d, library) = fixture();
        let backup = library
            .create_library_backup(&d.path().join("backups"))
            .unwrap();
        edit_during_inventory(&library);
        assert!(
            library
                .inspect_library_backup(&backup.path)
                .unwrap_err()
                .to_string()
                .contains("Library changed")
        );
    }
    #[test]
    fn staging_does_not_publish_intent_after_a_library_edit() {
        let (d, library) = fixture();
        let backup = library
            .create_library_backup(&d.path().join("backups"))
            .unwrap();
        edit_during_inventory(&library);
        assert!(
            library
                .stage_library_restore(&backup.path, &backup.database_sha256)
                .unwrap_err()
                .to_string()
                .contains("Library changed")
        );
        assert!(library.pending_library_restore().unwrap().is_none());
    }
    #[test]
    fn retry_does_not_clear_a_blocked_intent_after_a_library_edit() {
        let (d, library) = fixture();
        let backup = library
            .create_library_backup(&d.path().join("backups"))
            .unwrap();
        let mut request = library
            .stage_library_restore(&backup.path, &backup.database_sha256)
            .unwrap();
        request.blocked_reason = Some("Original blocked reason".into());
        atomic_json(
            &library.data_dir().join("library-backups/pending.json"),
            &request,
        )
        .unwrap();
        edit_during_inventory(&library);
        assert!(
            library
                .retry_library_restore()
                .unwrap_err()
                .to_string()
                .contains("Library changed")
        );
        assert_eq!(
            library
                .pending_library_restore()
                .unwrap()
                .unwrap()
                .blocked_reason
                .as_deref(),
            Some("Original blocked reason")
        );
    }
    #[test]
    fn external_sqlite_edits_during_inventory_invalidate_the_capture() {
        let (d, library) = fixture();
        let path = library.data_dir().join("library.sqlite");
        let original_changes = library.conn().unwrap().total_changes();
        during_inventory(move || {
            Connection::open(path)
                .unwrap()
                .execute(
                    "UPDATE tracks SET title='Independent connection edit' WHERE id='fixture'",
                    [],
                )
                .unwrap();
        });
        assert!(
            library
                .create_library_backup(&d.path().join("backups"))
                .unwrap_err()
                .to_string()
                .contains("Library changed")
        );
        assert_eq!(library.conn().unwrap().total_changes(), original_changes);
    }
    #[test]
    fn organization_inventory_enforces_remaining_budget_before_each_unique_file() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let first = root.join("one");
        let second = root.join("two");
        fs::write(&first, b"12").unwrap();
        fs::write(&second, b"34").unwrap();
        let mut locations = OrganizationLocations::new();
        locations.limit = 3;
        remember_organization_path(&mut locations, &first, false, false).unwrap();
        // Reusing a previously admitted file consumes no additional byte budget.
        remember_organization_path(&mut locations, &first, false, false).unwrap();
        assert_eq!(locations.bytes, 2);
        assert!(
            remember_organization_path(&mut locations, &second, false, false)
                .unwrap_err()
                .to_string()
                .contains("budget")
        );
        assert_eq!(locations.bytes, 2);
        assert_eq!(locations.entries.len(), 1);
    }
    #[test]
    fn cancelled_partial_organization_stages_remain_bound_without_requiring_complete_bytes() {
        use crate::organization::{OrganizationMode, OrganizationRequest};
        let (d, library) = fixture();
        let root = d.path().canonicalize().unwrap();
        let source = root.join("authored.wav");
        let destination = root.join("managed.wav");
        library
            .conn()
            .unwrap()
            .execute(
                "UPDATE tracks SET path=?1 WHERE id='fixture'",
                [path_bytes(&source)],
            )
            .unwrap();
        let operation = library
            .plan_organization(OrganizationRequest {
                track_id: "fixture".into(),
                mode: OrganizationMode::Move,
                source_root: root.clone(),
                destination_root: root,
                files: vec![(source, destination)],
            })
            .unwrap();
        assert!(
            library
                .execute_organization_with(&operation, |boundary| {
                    let _ = boundary;
                    let record = library
                        .organization_history()?
                        .into_iter()
                        .find(|record| record.id == operation)
                        .unwrap();
                    let file = &record.files[0];
                    let stage = file.destination_parent.path.path()?.join(&file.stage_name);
                    ensure!(stage.is_file(), "Expected an owned reserved stage");
                    fs::write(stage, b"authored partial stage bytes")?;
                    anyhow::bail!("Authored cancellation")
                })
                .is_err()
        );
        library.cancel_unstarted_organization(&operation).unwrap();
        let backup = library
            .create_library_backup(&d.path().join("backups"))
            .unwrap();
        library.inspect_library_backup(&backup.path).unwrap();
    }
    #[test]
    fn native_path_repair_receipts_roundtrip_and_missing_recovery_bytes_are_refused() {
        use crate::organization::{OrganizationMode, OrganizationRequest};
        use cuerithm_dj_sync::{DjSoftware, SyncTarget};
        let (d, library) = fixture();
        let root = d.path().canonicalize().unwrap();
        let source = root.join("authored.wav");
        let destination = root.join("organized.wav");
        library
            .conn()
            .unwrap()
            .execute(
                "UPDATE tracks SET path=?1 WHERE id='fixture'",
                [path_bytes(&source)],
            )
            .unwrap();
        let native = root.join("database.xml");
        fs::write(&native,format!(r#"<VirtualDJ_Database Version="8.5"><Song FilePath="{}"><Tags Author="Authored remote artist"/></Song></VirtualDJ_Database>"#,cuerithm_dj_sync::destination_reference(&source).display())).unwrap();
        library
            .seed_organization_reference_fixture(
                "fixture",
                SyncTarget {
                    environment: Default::default(),
                    software: DjSoftware::VirtualDj,
                    path: native.clone(),
                    source_root: None,
                },
                false,
            )
            .unwrap();
        let references = library.organization_reference_snapshot("fixture").unwrap();
        assert!(references.blockers.is_empty());
        let operation = library
            .plan_organization(OrganizationRequest {
                track_id: "fixture".into(),
                mode: OrganizationMode::Move,
                source_root: root.clone(),
                destination_root: root,
                files: vec![(source, destination)],
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
        library
            .execute_organization_checked(&operation, &guard)
            .unwrap();
        drop(guard);
        let backup = library
            .create_library_backup(&d.path().join("backups"))
            .unwrap();
        library.inspect_library_backup(&backup.path).unwrap();
        library
            .stage_library_restore(&backup.path, &backup.database_sha256)
            .unwrap();
        let database = library.data_dir().join("library.sqlite");
        drop(library);
        assert!(matches!(
            apply_pending_library_restore(&database).unwrap(),
            LibraryRestoreStartup::Applied { .. }
        ));
        let library = Library::open(&database).unwrap();
        let baseline = fs::read(&native).unwrap();
        fs::write(&native, b"changed authored native reference").unwrap();
        assert!(library.inspect_library_backup(&backup.path).is_err());
        fs::write(&native, baseline).unwrap();
        let recovery_root = library
            .data_dir()
            .join("organization-references")
            .join(operation);
        let journal = fs::read_dir(recovery_root)
            .unwrap()
            .map(|entry| entry.unwrap())
            .find(|entry| uuid::Uuid::parse_str(&entry.file_name().to_string_lossy()).is_ok())
            .unwrap();
        fs::remove_file(journal.path().join("backup/0")).unwrap();
        assert!(
            library
                .create_library_backup(&d.path().join("fresh"))
                .unwrap_err()
                .to_string()
                .contains("recovery bytes")
        );
    }
    #[test]
    fn current_curate_native_source_proof_rejects_equal_bytes_replacement() {
        use crate::*;
        let (d, lib) = fixture();
        let source = d.path().canonicalize().unwrap().join("authored.wav");
        let hash = cuerithm_media::file_sha256(&source).unwrap();
        lib.conn()
            .unwrap()
            .execute("UPDATE tracks SET path=?1", [path_bytes(&source)])
            .unwrap();
        let service = CurationService::new(lib.clone(), None);
        service
            .update(CurationUpdateRequest {
                request_id: "authored-native-proof".into(),
                taxonomy_revision: service.catalog().unwrap().revision,
                tracks: vec![CurationExpectedRevision {
                    track_id: "fixture".into(),
                    revision: 0,
                }],
                patch: CurationPatch {
                    notes: CurationValueEdit::Set {
                        value: "authored proof".into(),
                    },
                    ..Default::default()
                },
            })
            .unwrap();
        service
            .flush_pending(10, &cuerithm_media::CancellationToken::default())
            .unwrap();
        let (native_parent, parent) =
            organization::NativeParent::capture(source.parent().unwrap()).unwrap();
        let fingerprint = native_parent
            .fingerprint(Path::new(source.file_name().unwrap()))
            .unwrap();
        let proof = serde_json::json!({"path":organization::NativePath::new(&source), "parent":parent, "fingerprint":fingerprint}).to_string();
        lib.conn()
            .unwrap()
            .execute("UPDATE curation_tracks SET input_proof_json=?1", [&proof])
            .unwrap();
        let backup = lib
            .create_library_backup(&d.path().join("backups"))
            .unwrap();
        let replacement = source.with_extension("new");
        fs::write(&replacement, fs::read(&source).unwrap()).unwrap();
        fs::rename(&replacement, &source).unwrap();
        assert_eq!(cuerithm_media::file_sha256(&source).unwrap(), hash);
        assert!(lib.inspect_library_backup(&backup.path).is_err());
        assert!(lib.create_library_backup(&d.path().join("fresh")).is_err());
    }
}
