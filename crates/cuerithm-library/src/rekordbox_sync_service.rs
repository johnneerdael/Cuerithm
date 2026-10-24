//! Frozen native rekordbox plans, immutable versioned assets and SQLite-last
//! publication. Runtime keys never enter durable operations.
use crate::*;
use anyhow::{Context, Result, ensure};
use cuerithm_dj_sync::{
    SyncTrack,
    transaction::{
        DirectoryChange, FilePublication, FileTransaction, directory_manifest, file_hash,
    },
};
use cuerithm_media::{CancellationToken, file_sha256_cancellable};
use cuerithm_rekordbox::*;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
};
mod types;
pub use types::*;
mod assets;
mod ledger;
pub(crate) use ledger::upgrade_operations as upgrade_operation_ledgers;
mod planning;
mod publication;
mod retention;
#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SourceProof {
    id: String,
    path: PathBuf,
    hash: String,
    epoch: u64,
    local_fields: BTreeMap<String, Value>,
    #[serde(default)]
    native_media: Option<cuerithm_dj_sync::NativeSourceDescriptor>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct FieldAcknowledgement {
    local_id: String,
    content_id: String,
    field: String,
    local: Value,
    decision: NativeFieldDecision,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CueOwnershipProof {
    uuid: Option<String>,
    content_uuid: Option<String>,
    raw_hash: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct AssetSource {
    path: PathBuf,
    hash: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Operation {
    version: u32,
    id: String,
    target: RekordboxSyncTarget,
    #[serde(default)]
    environment_snapshot: Option<TargetEnvironmentSnapshot>,
    #[serde(default)]
    installation_hash: Option<String>,
    #[serde(default)]
    target_roots: Vec<TargetPathSnapshot>,
    #[serde(default)]
    configuration: Option<SyncConfigurationBinding>,
    #[serde(default)]
    configuration_target: Option<RekordboxSyncTarget>,
    native_target_id: String,
    #[serde(default)]
    native_revision: u64,
    request: RekordboxSyncRequest,
    revision: u64,
    settings_revision: String,
    sources: Vec<SourceProof>,
    asset_sources: Vec<AssetSource>,
    before: NativeDatabaseState,
    after: NativeDatabaseState,
    plan: NativeWritePlan,
    resolved: Vec<NativeResolvedTrack>,
    playlist_plan: Option<NativePlaylistPlan>,
    receipts: Vec<NativePlaylistPublicationReceipt>,
    acknowledgements: Vec<FieldAcknowledgement>,
    preview: RekordboxSyncPreview,
    asset_journal_id: String,
    before_snapshot: Option<PathBuf>,
    before_snapshot_hash: Option<String>,
    ledger_before: ledger::NativeLedger,
    ledger_after_hash: Option<String>,
    status: String,
    created: i64,
    updated: i64,
    outcome: Option<RekordboxSyncOutcome>,
}
fn digest(value: &impl Serialize) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(value)?)))
}
fn root(library: &Library) -> PathBuf {
    library.data_dir().join("sync/rekordbox")
}
fn lock(library: &Library) -> Result<File> {
    crate::dj_sync_service::service_lock(&library.data_dir().join("sync"))
}
fn ensure_not_native_file(target: &Path, candidate: &Path) -> Result<()> {
    if let (Ok(a), Ok(b)) = (fs::metadata(target), fs::metadata(candidate)) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            ensure!(
                a.dev() != b.dev() || a.ino() != b.ino(),
                "Native database cannot be an audio source or analysis asset"
            );
        }
        #[cfg(not(unix))]
        {
            let _ = (a, b);
            ensure!(
                !same_file::is_same_file(target, candidate)?,
                "Native database cannot be an audio source or analysis asset"
            );
        }
    }
    Ok(())
}
fn ensure_closed() -> Result<()> {
    cuerithm_dj_sync::discovery::ensure_closed(cuerithm_dj_sync::DjSoftware::Rekordbox)
}
fn save(connection: &Connection, op: &Operation) -> Result<()> {
    connection.execute("INSERT INTO rekordbox_sync_operations(id,target_id,database_path,status,journal_json,created,updated) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(id) DO UPDATE SET status=excluded.status,journal_json=excluded.journal_json,updated=excluded.updated",params![op.id,op.target.id,op.target.database.to_str().context("Native database path is not Unicode")?,op.status,serde_json::to_string(op)?,op.created,op.updated])?;
    Ok(())
}
fn load(library: &Library, id: &str) -> Result<Operation> {
    ensure!(
        uuid::Uuid::parse_str(id)?.to_string() == id,
        "Invalid native operation identity"
    );
    let value: String = library.conn()?.query_row(
        "SELECT journal_json FROM rekordbox_sync_operations WHERE id=?1",
        [id],
        |r| r.get(0),
    )?;
    ensure!(
        !retention::is_expired(&value),
        "This sync is older than the {} kept for restore, so its backup was removed",
        retention::RESTORE_LIMIT
    );
    let op: Operation = serde_json::from_str(&value)?;
    ensure!(
        op.id == id && op.version == 1,
        "Unsupported native operation journal"
    );
    Ok(op)
}
fn ensure_settled(library: &Library, database: &Path) -> Result<()> {
    let busy:bool=library.conn()?.query_row("SELECT EXISTS(SELECT 1 FROM rekordbox_sync_operations WHERE database_path=?1 AND status IN ('prepared','publishing','native_published','restoring'))",[database.to_str().context("Native path is not Unicode")?],|r|r.get(0))?;
    ensure!(
        !busy,
        "Discard the existing preview or recover the interrupted rekordbox operation first"
    );
    Ok(())
}
fn targets(paths: &DiscoveryPaths, cancel: &CancellationToken) -> Result<Vec<RekordboxSyncTarget>> {
    let discovery = discover_targets(paths, cancel)?;
    let Some(database) = discovery
        .database
        .filter(|d| d.state == DatabaseLocationState::Present)
    else {
        return Ok(vec![]);
    };
    let database = database.path.canonicalize()?;
    let asset_root = database
        .parent()
        .context("Native asset root missing")?
        .join("share");
    let options_file = paths.agent_options_file()?;
    discovery
        .installations
        .into_iter()
        .filter(|i| matches!(i.major, 6 | 7))
        .map(|installation| {
            let id = stable_target_id(
                &installation,
                &database,
                &options_file,
                &asset_root,
                &TargetEnvironment::Native,
            )?;
            Ok(RekordboxSyncTarget {
                id,
                installation,
                database: database.clone(),
                options_file: options_file.clone(),
                asset_root: asset_root.clone(),
                environment: TargetEnvironment::Native,
            })
        })
        .collect()
}
fn stable_target_id(
    installation: &TargetInstallation,
    database: &Path,
    options: &Path,
    assets: &Path,
    environment: &TargetEnvironment,
) -> Result<String> {
    let mut identity_environment = environment.clone();
    if let TargetEnvironment::WindowsCompatibility { installation, .. } = &mut identity_environment
    {
        *installation = None;
    }
    digest(&json!([
        installation.major,
        database,
        options,
        assets,
        identity_environment
    ]))
}
fn capture_target_roots(target: &RekordboxSyncTarget) -> Result<Vec<TargetPathSnapshot>> {
    let mut paths = BTreeSet::from([
        target
            .database
            .parent()
            .context("Database parent missing")?
            .to_path_buf(),
        target.asset_root.clone(),
    ]);
    for path in [&target.options_file, &target.installation.executable] {
        if let Some(parent) = path.parent().filter(|p| p.is_dir()) {
            paths.insert(parent.to_path_buf());
        }
    }
    paths
        .into_iter()
        .map(|path| TargetPathSnapshot::capture(&path))
        .collect()
}
fn verify_environment(op: &Operation, cancel: &CancellationToken) -> Result<()> {
    if let Some(snapshot) = &op.environment_snapshot {
        snapshot.verify()?;
    }
    for proof in &op.target_roots {
        proof.verify()?;
    }
    if let Some(hash) = &op.installation_hash {
        ensure!(
            file_sha256_cancellable(&op.target.installation.executable, cancel)? == *hash,
            "Installed rekordbox executable changed; create a new preview"
        );
    }
    Ok(())
}
// Preserve native references in records and identities; project only file access
// paths used for matching/import. An unmapped foreign track stays unavailable.
fn project_collection(
    paths: &TargetPathResolver<'_>,
    collection: &mut RekordboxCollection,
) -> Result<()> {
    if paths.is_native() {
        return Ok(());
    }
    for track in &mut collection.tracks {
        track.source.resolved_path = track
            .source
            .resolved_path
            .as_deref()
            .and_then(|p| paths.to_host(Path::new(p)).ok())
            .and_then(|p| p.to_str().map(str::to_owned));
    }
    Ok(())
}
fn resolve_target(
    library: &Library,
    id: &str,
    paths: &DiscoveryPaths,
    cancel: &CancellationToken,
) -> Result<RekordboxSyncTarget> {
    let mut configured = library
        .configured_rekordbox_targets()?
        .into_iter()
        .filter(|target| target.id == id);
    if let Some(selected) = configured.next() {
        let selected_shape = digest(&selected)?;
        for candidate in configured {
            ensure!(
                digest(&candidate)? == selected_shape,
                "Choose a saved library setup: multiple distinct configurations refer to this rekordbox library"
            );
        }
        cancel.check()?;
        return refresh_installation(selected);
    }
    targets(paths, cancel)?
        .into_iter()
        .find(|t| t.id == id)
        .context("Selected rekordbox installation or database changed; discover targets again")
}
fn refresh_installation(mut target: RekordboxSyncTarget) -> Result<RekordboxSyncTarget> {
    if matches!(
        target.installation.version_source.as_str(),
        "version_resource" | "bundle_plist"
    ) {
        let inspected = inspect_manual_installation(&target.installation.executable, None)?;
        ensure!(
            inspected.major == target.installation.major,
            "Installed rekordbox application family changed; configure the updated installation"
        );
        target.installation.version = inspected.version;
        target.installation.major = inspected.major;
        target.installation.version_source = inspected.version_source;
    }
    Ok(target)
}
fn resolve_selected_target(
    library: &Library,
    target_id: &str,
    configuration: Option<&SyncConfigurationBinding>,
    paths: &DiscoveryPaths,
    cancel: &CancellationToken,
) -> Result<RekordboxSyncTarget> {
    if let Some(binding) = configuration {
        cancel.check()?;
        let selected = library
            .list_sync_target_configurations()?
            .into_iter()
            .find(|saved| saved.id == binding.id)
            .context("Saved target setup was removed; create a new preview")?;
        ensure!(
            selected.revision == binding.revision,
            "Saved target setup changed; create a new preview"
        );
        let SavedSyncTarget::Rekordbox { target } = selected.target else {
            anyhow::bail!("Selected setup is not a native rekordbox target");
        };
        ensure!(
            target.id == target_id,
            "Selected saved setup belongs to a different native library"
        );
        return refresh_installation(target);
    }
    resolve_target(library, target_id, paths, cancel)
}
fn resolve_request_target(
    library: &Library,
    request: &RekordboxSyncRequest,
    paths: &DiscoveryPaths,
    cancel: &CancellationToken,
) -> Result<RekordboxSyncTarget> {
    resolve_selected_target(
        library,
        &request.target_id,
        request.configuration.as_ref(),
        paths,
        cancel,
    )
}
fn same_native_audio_resolved(
    paths: &TargetPathResolver<'_>,
    native: &str,
    physical: &Path,
) -> bool {
    physical.canonicalize().ok().is_some_and(|physical| {
        paths
            .to_host(Path::new(native))
            .ok()
            .and_then(|path| path.canonicalize().ok())
            == Some(physical)
    })
}
fn audio_reference_resolved(
    paths: &TargetPathResolver<'_>,
    physical: &Path,
    remote: Option<&ImportedTrack>,
    relocate: bool,
) -> Result<PathBuf> {
    if !relocate
        && let Some(stored) = remote
            .and_then(|track| track.source.path.as_deref())
            .filter(|stored| same_native_audio_resolved(paths, stored, physical))
    {
        return Ok(PathBuf::from(stored));
    }
    Ok(cuerithm_dj_sync::destination_reference(
        &paths.to_native(physical)?,
    ))
}
#[cfg(test)]
fn same_native_audio(target: &RekordboxSyncTarget, native: &str, physical: &Path) -> bool {
    target.environment.resolver().is_ok_and(|paths| {
        let matches = same_native_audio_resolved(&paths, native, physical);
        paths.verify().is_ok() && matches
    })
}
#[cfg(test)]
fn audio_reference(
    target: &RekordboxSyncTarget,
    physical: &Path,
    remote: Option<&ImportedTrack>,
    relocate: bool,
) -> Result<PathBuf> {
    let paths = target.environment.resolver()?;
    let reference = audio_reference_resolved(&paths, physical, remote, relocate)?;
    paths.verify()?;
    Ok(reference)
}
fn selected_configuration(
    library: &Library,
    request: &RekordboxSyncRequest,
    runtime_target: &RekordboxSyncTarget,
) -> Result<(
    Option<SyncConfigurationBinding>,
    Option<RekordboxSyncTarget>,
)> {
    let configurations = library.list_sync_target_configurations()?;
    if let Some(binding) = &request.configuration {
        let selected = configurations
            .into_iter()
            .find(|saved| saved.id == binding.id)
            .context("Saved target setup was removed; create a new preview")?;
        ensure!(
            selected.revision == binding.revision,
            "Saved target setup changed; create a new preview"
        );
        let SavedSyncTarget::Rekordbox { target } = selected.target else {
            anyhow::bail!("Selected setup is not a native rekordbox target");
        };
        ensure!(
            target.id == request.target_id && target.id == runtime_target.id,
            "Selected saved setup belongs to a different native library"
        );
        return Ok((Some(binding.clone()), Some(target)));
    }
    let captured = configurations
        .into_iter()
        .find_map(|saved| match saved.target {
            SavedSyncTarget::Rekordbox { target } if target.id == runtime_target.id => Some((
                SyncConfigurationBinding {
                    id: saved.id,
                    revision: saved.revision,
                },
                target,
            )),
            _ => None,
        });
    Ok(captured.unzip())
}
fn key_for(
    target: &RekordboxSyncTarget,
    platform: DesktopPlatform,
    cancel: &CancellationToken,
) -> Result<DatabaseKey> {
    resolve_database_key(
        &NativeKeySource::for_environment(&target.installation, &target.environment, platform)?,
        &target.options_file,
        KeyProviderOptions::default(),
        cancel,
    )
}
fn require_capability(
    target: &RekordboxSyncTarget,
    schema: &DatabaseSchema,
    cancel: &CancellationToken,
) -> Result<()> {
    ensure!(
        accepted_write_capability_cancellable(&target.installation, schema, cancel)?.can_write(),
        "This rekordbox library format is not supported for syncing changes"
    );
    Ok(())
}
fn verify_local_rows(connection: &Connection, op: &Operation) -> Result<()> {
    let revision: u64 =
        connection.query_row("SELECT value FROM meta WHERE key='revision'", [], |r| {
            r.get(0)
        })?;
    ensure!(
        revision == op.revision,
        "Library changed; create a new rekordbox preview"
    );
    let selected: String = connection.query_row(
        "SELECT revision FROM selected_settings WHERE singleton=1",
        [],
        |r| r.get(0),
    )?;
    ensure!(
        selected == op.settings_revision,
        "Preparation settings changed; create a new rekordbox preview"
    );
    for source in &op.sources {
        let (epoch,ready,path,hash):(u64,bool,Vec<u8>,String)=connection.query_row("SELECT e.epoch,t.state='ready' AND t.archived=0,t.path,t.source_hash FROM tracks t JOIN maintenance_track_epochs e ON e.id=t.id WHERE t.id=?1",[&source.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
        ensure!(
            ready
                && hash == source.hash
                && crate::model::bytes_path(&path)? == source.path
                && epoch == source.epoch
                && crate::native_sync::local_fields(connection, &source.id)? == source.local_fields,
            "Prepared track changed; create a new rekordbox preview"
        );
    }
    if let Some(plan) = &op.playlist_plan {
        ensure!(
            crate::playlists::revision(connection)? == plan.playlist_revision
                && crate::native_playlist_sync::source_fingerprint(connection)?
                    == plan.source_fingerprint,
            "Local playlist or MyTag changed; create a new rekordbox preview"
        );
    }
    Ok(())
}
fn verify_native_observation(connection: &Connection, op: &Operation) -> Result<()> {
    let revision: u64 = connection.query_row(
        "SELECT revision FROM native_targets WHERE id=?1",
        [&op.native_target_id],
        |row| row.get(0),
    )?;
    ensure!(
        revision == op.native_revision,
        "Recorded native observation changed; create a new rekordbox preview"
    );
    Ok(())
}
fn verify_source_bytes(op: &Operation, cancel: &CancellationToken) -> Result<()> {
    for source in &op.sources {
        cancel.check()?;
        ensure_not_native_file(&op.target.database, &source.path)?;
        ensure!(
            file_sha256_cancellable(&source.path, cancel)? == source.hash,
            "Audio source changed; create a new rekordbox preview"
        );
    }
    for asset in &op.asset_sources {
        cancel.check()?;
        ensure_not_native_file(&op.target.database, &asset.path)?;
        ensure!(
            file_sha256_cancellable(&asset.path, cancel)? == asset.hash,
            "Original native asset changed; create a new rekordbox preview"
        );
    }
    Ok(())
}
fn verify_sources(library: &Library, op: &Operation, cancel: &CancellationToken) -> Result<()> {
    {
        let connection = library.conn()?;
        verify_local_rows(&connection, op)?;
    }
    verify_source_bytes(op, cancel)
}
impl Library {
    pub fn configure_rekordbox_sync_target(
        &self,
        request: RekordboxManualTarget,
    ) -> Result<RekordboxSyncTarget> {
        request.environment.validate()?;
        request.environment.freeze()?.verify()?;
        let installation =
            inspect_manual_installation(&request.executable, request.version_hint.as_deref())?;
        if !request.environment.is_native() {
            ensure!(
                installation
                    .executable
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.eq_ignore_ascii_case("rekordbox.exe")),
                "Select the Windows rekordbox executable for Wine/Proton"
            );
        }
        for path in [&request.database, &request.options_file] {
            ensure!(
                path.is_absolute()
                    && fs::symlink_metadata(path)?.is_file()
                    && !fs::symlink_metadata(path)?.file_type().is_symlink(),
                "Select regular database and agent options files"
            );
        }
        ensure!(
            request.asset_root.is_absolute() && request.asset_root.is_dir(),
            "Select the native share asset directory"
        );
        if let TargetEnvironment::WindowsCompatibility {
            installation: Some(selected),
            ..
        } = &request.environment
        {
            let selected = selected.canonicalize()?;
            ensure!(
                if selected.is_dir() {
                    installation.executable.starts_with(&selected)
                } else {
                    installation.executable == selected
                },
                "Selected Windows installation and executable do not match"
            );
        }
        let database = request.database.canonicalize()?;
        let options_file = request.options_file.canonicalize()?;
        let asset_root = request.asset_root.canonicalize()?;
        let id = stable_target_id(
            &installation,
            &database,
            &options_file,
            &asset_root,
            &request.environment,
        )?;
        Ok(RekordboxSyncTarget {
            id,
            installation,
            database,
            options_file,
            asset_root,
            environment: request.environment,
        })
    }
    pub fn discover_rekordbox_targets(&self) -> Result<Vec<RekordboxSyncTarget>> {
        let mut available = self.configured_rekordbox_targets()?;
        available.extend(targets(
            &DiscoveryPaths::current()?,
            &CancellationToken::default(),
        )?);
        available.sort_by(|a, b| a.id.cmp(&b.id));
        available.dedup_by(|a, b| a.id == b.id);
        Ok(available)
    }
    /// Runtime-only direct importer access. Read permission never promotes native writes.
    pub fn prepare_rekordbox_import_source(
        &self,
        target_id: &str,
        cancel: &CancellationToken,
    ) -> Result<(
        crate::ImportSourceSelection,
        std::sync::Arc<cuerithm_dj_sync::import::readers::rekordbox::RekordboxReader>,
    )> {
        use cuerithm_dj_sync::import::ArtifactRequest;
        let paths = DiscoveryPaths::current()?;
        let target = resolve_selected_target(self, target_id, None, &paths, cancel)?;
        let environment = target.environment.freeze()?;
        let key = key_for(&target, paths.platform, cancel)?;
        let database = ReadOnlyDatabase::open(&target.database, &key)?;
        let root = target
            .database
            .parent()
            .context("Native database root missing")?
            .to_path_buf();
        let mut artifacts = vec![ArtifactRequest::sqlite("master.db")];
        let resolver = target.environment.resolver()?;
        let mut selected = BTreeSet::new();
        database.visit_collection_records(CollectionRecordLimits::default(), cancel, |record| {
            if record.table == "djmdContent"
                && let Some(TargetValue::Text(reference)) = record.values.get("AnalysisDataPath")
                && !reference.is_empty()
            {
                let relative = assets::asset_relative_resolved(&target, &resolver, reference)?;
                for extension in ["DAT", "EXT", "2EX"] {
                    let relative = relative.with_extension(extension);
                    let id = relative
                        .to_str()
                        .context("Native analysis reference is not Unicode")?
                        .replace('\\', "/");
                    if target.asset_root.join(&relative).is_file() && selected.insert(id.clone()) {
                        ensure!(
                            artifacts.len() < 100_000,
                            "Too many selected native artifacts"
                        );
                        let mut artifact = ArtifactRequest::stable(&id)
                            .with_selected_root(&id, &target.asset_root);
                        artifact.relative_path = relative;
                        artifacts.push(artifact);
                    }
                }
            }
            Ok(())
        })?;
        resolver.verify()?;
        environment.verify()?;
        Ok((
            crate::ImportSourceSelection {
                folder_inventory: false,
                root,
                artifacts,
            },
            std::sync::Arc::new(
                cuerithm_dj_sync::import::readers::rekordbox::RekordboxReader::with_provisioned_key(
                    key,
                    CollectionReadOptions::default(),
                ),
            ),
        ))
    }
    pub fn preflight_rekordbox_sync(
        &self,
        target_id: &str,
        cancel: &CancellationToken,
    ) -> Result<RekordboxSyncPreflight> {
        self.preflight_rekordbox_sync_configured(target_id, None, cancel)
    }
    pub fn preflight_rekordbox_sync_configured(
        &self,
        target_id: &str,
        configuration: Option<SyncConfigurationBinding>,
        cancel: &CancellationToken,
    ) -> Result<RekordboxSyncPreflight> {
        let paths = DiscoveryPaths::current()?;
        let target =
            resolve_selected_target(self, target_id, configuration.as_ref(), &paths, cancel)?;
        let mut result = RekordboxSyncPreflight {
            target: target.clone(),
            readable: false,
            writable: false,
            restrictions: vec![],
            schema_fingerprint: None,
            identity: None,
        };
        let inspected = (|| -> Result<()> {
            let key = key_for(&target, paths.platform, cancel)?;
            let db = ReadOnlyDatabase::open(&target.database, &key)?;
            let schema = db.schema()?;
            result.schema_fingerprint = Some(schema.fingerprint.clone());
            result.identity = Some(
                db.read_collection(CollectionReadOptions::default(), cancel)?
                    .identity,
            );
            result.readable = true;
            ensure_closed()?;
            ensure_settled(self, &target.database)?;
            require_capability(&target, &schema, cancel)?;
            result.writable = true;
            Ok(())
        })();
        if let Err(error) = inspected {
            cancel.check()?;
            result.restrictions.push(error.to_string());
        }
        Ok(result)
    }
    pub fn preview_rekordbox_sync(
        &self,
        request: RekordboxSyncRequest,
        runtime: &RekordboxSyncRuntime,
        cancel: &CancellationToken,
    ) -> Result<RekordboxSyncPreview> {
        let _lock = lock(self)?;
        cancel.check()?;
        let target = resolve_request_target(self, &request, &runtime.discovery_paths, cancel)?;
        ensure_settled(self, &target.database)?;
        ensure_closed()?;
        let key = key_for(&target, runtime.discovery_paths.platform, cancel)?;
        planning::preview(
            self,
            request,
            target,
            &key,
            runtime.media_runtime.as_ref(),
            cancel,
            planning::PreviewPermit::Production,
        )
    }
    pub fn discard_rekordbox_sync(&self, id: &str) -> Result<()> {
        let _lock = lock(self)?;
        let op = load(self, id)?;
        ensure!(
            op.status == "prepared",
            "Only an unpublished rekordbox preview can be discarded"
        );
        FileTransaction::discard(&root(self).join("assets"), &op.asset_journal_id)?;
        self.conn()?.execute(
            "DELETE FROM rekordbox_sync_operations WHERE id=?1 AND status='prepared'",
            [id],
        )?;
        let path = root(self).join("operations").join(id);
        if path.exists() {
            fs::remove_dir_all(path)?;
        }
        Ok(())
    }
    pub fn history_rekordbox_sync(&self) -> Result<Vec<RekordboxSyncHistoryEntry>> {
        /// The part of a journal the history shows; a sync that gave up its backup keeps only this.
        #[derive(Deserialize)]
        struct Row {
            id: String,
            target: RowTarget,
            status: String,
            created: i64,
            updated: i64,
            outcome: Option<RekordboxSyncOutcome>,
            #[serde(default)]
            restore_expired: bool,
        }
        #[derive(Deserialize)]
        struct RowTarget {
            id: String,
        }
        let connection = self.conn()?;
        let mut statement = connection.prepare(
            "SELECT journal_json FROM rekordbox_sync_operations ORDER BY created DESC,id DESC",
        )?;
        let rows = statement
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|s| {
                let row: Row = serde_json::from_str(&s)?;
                Ok(RekordboxSyncHistoryEntry {
                    restore_available: !row.restore_expired
                        && matches!(row.status.as_str(), "applied" | "native_published"),
                    id: row.id,
                    target_id: row.target.id,
                    status: row.status,
                    created: row.created,
                    updated: row.updated,
                    outcome: row.outcome,
                })
            })
            .collect()
    }
}
