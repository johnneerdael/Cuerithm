//! One recoverable file/SQLite publication for duplicate donor, archive and reference work.
use super::{
    deletion,
    donor_state::{self, TrackImage},
    files::{HashedFile, current_state, ensure_available},
    merge::{self, SavedPreview},
    purge, track_removal, undo_window, *,
};
use crate::{
    Library,
    artifacts::{Artifact, cleanup_temporaries, current_hash, owned_file, rollback, safe_target},
    model::{bytes_path, path_bytes},
    publish_files,
    record_snapshot::{RecordImage, RestoreReport},
};
use anyhow::{Context, Result, ensure};
use cuerithm_autotag::{
    AnalysisTags, AutotagConfig, MatchStatus, StagedTagWrite, TagInput, TagMatchOutcome,
    TagWritePlan, TransferPatch, TransferSnapshot,
};
use cuerithm_maintenance::{MergeDisposition, MergePlan};
use cuerithm_media::{CancellationToken, MediaRuntime};
use rusqlite::{Connection, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::Arc,
};

pub struct DuplicateMergeService {
    lib: Library,
    runtime: MediaRuntime,
    trash: Arc<dyn FileTrash>,
    #[cfg(test)]
    hook: Option<fn(&str)>,
}
/// How a verified duplicate file left its folder.
enum Disposal {
    Trashed,
    Permanent,
}
/// A file this operation intended to move to the Trash, kept in the journal so an
/// interrupted operation can say which files may already be there.
#[derive(Clone, Serialize, Deserialize)]
struct TrashIntent {
    track_id: String,
    path: Vec<u8>,
}
struct StagedImport {
    target: String,
    donor: String,
    record: cuerithm_maintenance::ImportRecord,
    /// The keeper's tags as they were before the donor's values were written.
    previous: TransferSnapshot,
    native: TransferSnapshot,
    staged: StagedTagWrite,
}
/// The tag values a donor copy replaced and wrote. Undo writes `before` back into the keeper's
/// file, so no copy of the original audio has to be kept.
#[derive(Clone, Serialize, Deserialize)]
struct TransferRecord {
    before: TransferSnapshot,
    after: TransferSnapshot,
}
#[derive(Serialize, Deserialize)]
struct Payload {
    schema: String,
    preview_id: String,
    kind: String,
    undo_of: Option<String>,
    result: DuplicateMergeResult,
    before: BTreeMap<String, TrackImage>,
    after: BTreeMap<String, TrackImage>,
    playlists: Vec<DuplicatePlaylistChange>,
    epochs: BTreeMap<String, u64>,
    undone_by: Option<String>,
    /// Library records removed together with their files; the minimum Undo needs. Records
    /// saved in a format this build does not read are treated as no longer stored.
    #[serde(default, deserialize_with = "saved_records")]
    removed: Option<RecordImage>,
    #[serde(default)]
    trash_intent: Vec<TrashIntent>,
    /// The removed records are no longer kept, so the removal is final.
    #[serde(default)]
    undo_expired: bool,
    /// Source hashes whose cached cover was kept because Undo may need it: with the file in
    /// the Trash, that cache can be the only copy of the picture left.
    #[serde(default)]
    artwork_hashes: Vec<String>,
    /// Files that had already left their place (to the Trash, or deleted) when an operation was
    /// rolled back, so their records were kept. Shown under Recovery until acknowledged.
    #[serde(default)]
    interrupted_deletions: Vec<String>,
    #[serde(default)]
    deletions_acknowledged: bool,
    /// Per keeper track: the tag values before and after a donor copy. An operation that records
    /// them is undone by writing the previous values back; one from an earlier version has none
    /// and is undone from its retained original.
    #[serde(default)]
    transfers: BTreeMap<String, TransferRecord>,
}
impl Payload {
    /// What a donor copy recorded for the file at `path`, with the track it belongs to.
    fn transfer_for(&self, path: &std::path::Path) -> Option<(&String, &TransferRecord)> {
        self.transfers.iter().find(|(id, _)| {
            self.before
                .get(*id)
                .and_then(|image| image.row.path().ok())
                .is_some_and(|p| p == path)
        })
    }
    /// Drop what only Undo of a removal needs, keeping the summary in `result`. Returns the
    /// covers that were being kept for it.
    fn forget_removed(&mut self) -> Vec<String> {
        let removed = self.result.removed_ids.clone();
        self.removed = None;
        self.before.retain(|id, _| !removed.contains(id));
        std::mem::take(&mut self.artwork_hashes)
    }
}
fn saved_records<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<RecordImage>, D::Error> {
    let value = <Option<serde_json::Value> as Deserialize>::deserialize(deserializer)?;
    Ok(value.and_then(|value| serde_json::from_value(value).ok()))
}
/// A merge that fell out of the shared Undo window (see [`undo_window`]): the stored records
/// are dropped and Undo is no longer offered. The summary stays in `result`. Returns the
/// covers it was holding.
pub(super) fn drop_saved_records(conn: &rusqlite::Connection, id: &str) -> Result<Vec<String>> {
    let text: String = conn.query_row(
        "SELECT payload_json FROM maintenance_writes WHERE id=?1",
        [id],
        |r| r.get(0),
    )?;
    let mut payload: Payload = serde_json::from_str(&text)?;
    let covers = payload.forget_removed();
    payload.undo_expired = true;
    conn.execute(
        "UPDATE maintenance_writes SET payload_json=?1 WHERE id=?2",
        params![serde_json::to_string(&payload)?, id],
    )?;
    Ok(covers)
}
/// An interrupted operation changed no library record, but it may already have moved files to
/// the Trash. Say which, so the user can find them.
/// Files this operation meant to move to the Trash that are no longer at their place.
fn gone_files(write: &Write) -> Vec<String> {
    write
        .payload
        .trash_intent
        .iter()
        .filter_map(|item| bytes_path(&item.path).ok())
        .filter(|path| !path.exists())
        .map(|path| path.display().to_string())
        .collect()
}
fn interruption(write: &Write, base: &str) -> anyhow::Error {
    let gone = gone_files(write);
    if gone.is_empty() {
        anyhow::anyhow!("{base}")
    } else {
        anyhow::anyhow!(
            "{base}; these files are no longer at their original location and may be in the Trash or deleted, and their library records were kept: {}",
            gone.join(", ")
        )
    }
}
struct Write {
    id: String,
    state: String,
    payload: Payload,
    files: Vec<(Artifact, PathBuf)>,
}
impl Write {
    /// Undo of this operation writes recorded tag values back, so its original is not kept.
    fn records_values(&self) -> bool {
        self.payload.kind == "undo" || !self.payload.transfers.is_empty()
    }
}
fn load(lib: &Library, id: &str) -> Result<Write> {
    let conn = lib.conn()?;
    let (state, text): (String, String) = conn.query_row(
        "SELECT state,payload_json FROM maintenance_writes WHERE id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let mut q=conn.prepare("SELECT target,staged,backup,before_hash,after_hash,publish_path,displaced_path,workspace FROM maintenance_write_files WHERE write_id=?1 ORDER BY ordinal")?;
    let rows = q
        .query_map([id], |r| {
            Ok((
                r.get::<_, Vec<u8>>(0)?,
                r.get::<_, Vec<u8>>(1)?,
                r.get::<_, Option<Vec<u8>>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Vec<u8>>(5)?,
                r.get::<_, Vec<u8>>(6)?,
                r.get::<_, Vec<u8>>(7)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let files: Vec<(Artifact, PathBuf)> = rows
        .into_iter()
        .map(
            |(target, staged, backup, before, after, publish, displaced, workspace)| {
                Ok((
                    Artifact {
                        target: bytes_path(&target)?,
                        staged: bytes_path(&staged)?,
                        backup: backup.map(|p| bytes_path(&p)).transpose()?,
                        before,
                        after,
                        publish: bytes_path(&publish)?,
                        displaced: bytes_path(&displaced)?,
                    },
                    bytes_path(&workspace)?,
                ))
            },
        )
        .collect::<Result<_>>()?;
    let payload: Payload = serde_json::from_str(&text)?;
    let allowed = payload
        .before
        .values()
        .map(|image| image.row.path().map(|p| path_bytes(&p)))
        .collect::<Result<BTreeSet<_>>>()?;
    let root = lib.data_dir().canonicalize()?;
    for (file, workspace) in &files {
        ensure!(
            allowed.contains(&path_bytes(&file.target)),
            "Recovery target does not match its track snapshot"
        );
        let staged = super::files::resolved_prefix(&file.staged)?;
        let backup = file
            .backup
            .as_ref()
            .map(|p| super::files::resolved_prefix(p))
            .transpose()?;
        // The backup of a settled operation that records its tag values was deleted on purpose,
        // and its private folder with it.
        ensure!(
            super::files::resolved_prefix(workspace)?.starts_with(&root)
                && staged.starts_with(workspace)
                && backup.as_ref().is_none_or(|p| p.starts_with(workspace)),
            "Recovery assets leave application data"
        );
        ensure!(
            file.publish.parent() == file.target.parent()
                && file.displaced.parent() == file.target.parent(),
            "Recovery temporary leaves source folder"
        );
    }

    ensure!(
        payload.schema == "duplicate-file-merge-v2",
        "Unsupported duplicate file journal"
    );
    Ok(Write {
        id: id.into(),
        state,
        payload,
        files,
    })
}
fn reserve(
    lib: &Library,
    id: &str,
    payload: &Payload,
    files: &[(Artifact, PathBuf)],
    expected: &MaintenanceStamp,
    cancel: &CancellationToken,
) -> Result<()> {
    let root = lib.data_dir().canonicalize()?;
    let allowed = payload
        .before
        .values()
        .map(|image| image.row.path().map(|p| path_bytes(&p)))
        .collect::<Result<BTreeSet<_>>>()?;
    for (file, workspace) in files {
        ensure!(
            allowed.contains(&path_bytes(&file.target)),
            "Publication target no longer matches its track snapshot"
        );
        ensure!(
            workspace.canonicalize()?.starts_with(&root),
            "Duplicate staging leaves application data"
        );
        safe_target(&file.target, &file.target)?;
        owned_file(&file.staged, workspace, &file.after)?;
        let backup = file
            .backup
            .as_ref()
            .context("Missing original duplicate backup")?;
        owned_file(
            backup,
            workspace,
            file.before.as_deref().context("Missing source hash")?,
        )?;
        publish_files::durable_assets(&[file.staged.clone(), backup.clone()], workspace, &root)?;
    }
    let mut conn = lib.conn()?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    ensure!(
        super::scan::stamp(&tx)? == *expected,
        "Library changed while staging duplicate work"
    );
    for file in files.iter().map(|(f, _)| f) {
        crate::maintenance::ensure_unused_unreserved(&tx, &[path_bytes(&file.target)])?;
        let busy:i64=tx.query_row("SELECT (SELECT COUNT(*) FROM tag_write_files f JOIN tag_writes w ON w.id=f.write_id WHERE f.target=?1 AND w.state='prepared')+(SELECT COUNT(*) FROM cleanup_write_files f JOIN cleanup_writes w ON w.id=f.write_id WHERE f.target=?1 AND w.state='prepared')+(SELECT COUNT(*) FROM maintenance_write_files f JOIN maintenance_writes w ON w.id=f.write_id WHERE f.target=?1 AND w.state IN ('prepared','conflict'))",[path_bytes(&file.target)],|r|r.get(0))?;
        ensure!(busy == 0, "Metadata destination is reserved");
        ensure!(
            current_hash(&file.target)? == file.before,
            "Duplicate source changed during staging"
        );
    }
    cancel.check()?;
    tx.execute("INSERT INTO maintenance_writes(id,kind,state,payload_json,created) VALUES(?1,?2,'prepared',?3,?4)",params![id,payload.kind,serde_json::to_string(payload)?,crate::now()])?;
    for (n, (f, workspace)) in files.iter().enumerate() {
        tx.execute(
            "INSERT INTO maintenance_write_files VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                id,
                n as u64,
                path_bytes(&f.target),
                path_bytes(&f.staged),
                f.backup.as_ref().map(|p| path_bytes(p)),
                f.before,
                f.after,
                path_bytes(&f.publish),
                path_bytes(&f.displaced),
                path_bytes(workspace)
            ],
        )?;
    }
    tx.commit()?;
    Ok(())
}
fn install(files: &[(Artifact, PathBuf)], cancel: &CancellationToken) -> Result<()> {
    for (file, workspace) in files {
        cancel.check()?;
        safe_target(&file.target, &file.target)?;
        owned_file(&file.staged, workspace, &file.after)?;
        ensure!(
            current_hash(&file.target)? == file.before,
            "Duplicate source changed before publication"
        );
        if file.before.as_ref() == Some(&file.after) {
            continue;
        }
        ensure!(
            !std::fs::metadata(&file.target)?.permissions().readonly(),
            "Duplicate target is read only"
        );
        publish_files::copy_prepared(&file.staged, &file.publish, &file.after)?;
        publish_files::replace(&file.publish, &file.target, &file.displaced)?;
        ensure!(
            current_hash(&file.displaced)? == file.before,
            "Source changed during duplicate exchange"
        );
        ensure!(
            current_hash(&file.target)?.as_deref() == Some(file.after.as_str()),
            "Published duplicate metadata changed"
        );
    }
    Ok(())
}
/// SQL condition: the operation moved audio files away before it was interrupted and nobody has
/// acknowledged that yet. It keeps History showing the operation after it was rolled back.
const UNACKNOWLEDGED_DELETIONS: &str = "(json_array_length(COALESCE(json_extract(payload_json,'$.interrupted_deletions'),'[]'))>0 AND COALESCE(json_extract(payload_json,'$.deletions_acknowledged'),0)=0)";

fn finish(
    lib: &Library,
    id: &str,
    files: &[(Artifact, PathBuf)],
    retire_backup: bool,
    error: Option<&anyhow::Error>,
) -> Result<()> {
    let artifacts = files.iter().map(|(f, _)| f.clone()).collect::<Vec<_>>();
    if let Some(error) = error {
        let recovered = artifacts
            .iter()
            .try_for_each(|f| safe_target(&f.target, &f.target))
            .and_then(|_| rollback(&artifacts));
        let message = recovered
            .as_ref()
            .err()
            .map(|e| format!("{error:#}; recovery needs attention: {e:#}"))
            .unwrap_or_else(|| format!("{error:#}"));
        lib.conn()?.execute(
            "UPDATE maintenance_writes SET state=?1,error=?2 WHERE id=?3",
            params![
                if recovered.is_ok() {
                    "rolled_back"
                } else {
                    "conflict"
                },
                message,
                id
            ],
        )?;
        if recovered.is_err() {
            return Ok(());
        }
    }
    match cleanup_temporaries(&artifacts, lib.data_dir(), retire_backup) {
        Ok(()) => {
            let conn = lib.conn()?;
            if retire_backup {
                conn.execute(
                    "UPDATE maintenance_write_files SET backup=NULL WHERE write_id=?1",
                    [id],
                )?;
            }
            conn.execute(
                "UPDATE maintenance_writes SET cleanup_complete=1 WHERE id=?1",
                [id],
            )?;
        }
        Err(error) => {
            lib.conn()?.execute(
                "UPDATE maintenance_writes SET error=?1 WHERE id=?2",
                params![format!("Recovery cleanup pending: {error:#}"), id],
            )?;
        }
    }
    Ok(())
}
/// Whether the duplicate and the track that keeps its audio are still free of other work, as far
/// as the library's records can tell; `Some(reason)` when they are not.
fn unreserved(
    conn: &Connection,
    file: &DuplicateDeletionFile,
    snapshots: &BTreeMap<String, TrackImage>,
    own_write: &str,
) -> Result<Option<String>> {
    let path_of = |id: &str| snapshots.get(id).and_then(|image| image.row.path().ok());
    // A track that is not part of the merge any more is reported by the checks that follow.
    let (Some(path), Some(keeper_path)) = (path_of(&file.track_id), path_of(&file.keeper_id))
    else {
        return Ok(None);
    };
    deletion::busy_now(
        conn,
        &file.track_id,
        &path,
        &file.keeper_id,
        &keeper_path,
        own_write,
    )
}
fn result(
    id: &str,
    preview: &DuplicateMergePreview,
    undo_of: Option<String>,
) -> DuplicateMergeResult {
    DuplicateMergeResult {
        id: id.into(),
        preview_id: preview.id.clone(),
        stamp: preview.stamp.clone(),
        archived_ids: vec![],
        restored_ids: vec![],
        affected_playlists: preview.playlists.iter().map(|p| p.id.clone()).collect(),
        undo_of,
        updated_ids: vec![],
        removed_ids: vec![],
        trashed: vec![],
        permanently_deleted: vec![],
        not_deleted: vec![],
        links_not_restored: 0,
    }
}
impl DuplicateMergeService {
    pub fn new(lib: Library, runtime: MediaRuntime) -> Self {
        Self::with_trash(lib, runtime, Arc::new(SystemTrash))
    }
    /// Use another Trash, which lets tests keep real files out of the user's Trash.
    pub fn with_trash(lib: Library, runtime: MediaRuntime, trash: Arc<dyn FileTrash>) -> Self {
        Self {
            lib,
            runtime,
            trash,
            #[cfg(test)]
            hook: None,
        }
    }
    pub fn apply(&self, id: &str, cancel: &CancellationToken) -> Result<DuplicateMergeResult> {
        self.apply_confirmed(id, false, cancel)
    }
    /// Apply a reviewed preview. A preview that moves audio files to the Trash is applied only
    /// with `confirm_deletion`, and only the files it listed (and that still pass every check)
    /// are moved.
    pub fn apply_confirmed(
        &self,
        id: &str,
        confirm_deletion: bool,
        cancel: &CancellationToken,
    ) -> Result<DuplicateMergeResult> {
        cancel.check()?;
        let mut preview: SavedPreview = {
            let conn = self.lib.conn()?;
            merge::load(&conn, id, "duplicate_merge_plan")?
        };
        ensure!(
            preview.consumed_by.is_none(),
            "Duplicate preview already applied"
        );
        let deleting = preview.public.plan.disposition == MergeDisposition::Delete;
        ensure!(
            !deleting || confirm_deletion,
            "Confirm deleting the listed files before applying"
        );
        super::review::refresh(&self.lib, &mut preview.review, cancel)?;
        // Files can vanish between the scan and now; say which before the generic stale refusal.
        ensure_available(&*self.lib.conn()?, &preview.public.plan.participants())?;
        ensure!(
            !preview.review.review.stale,
            "Duplicate preview is stale; scan again"
        );
        // Check again, now, every file the preview listed. What is applied can only be a
        // subset of what the user reviewed.
        let mut plan = preview.public.plan.clone();
        let (mut doomed, mut not_deleted) = if deleting {
            self.reverify(&preview, &mut plan, cancel)?
        } else {
            (vec![], vec![])
        };
        let ids = plan.participants();
        let snapshots = {
            let mut conn = self.lib.conn()?;
            let tx = conn.transaction()?;
            ensure!(
                super::scan::stamp(&tx)? == preview.public.stamp,
                "Library changed; scan again"
            );
            let value = ids
                .iter()
                .map(|id| Ok((id.clone(), TrackImage::capture(&tx, id)?)))
                .collect::<Result<BTreeMap<_, _>>>()?;
            tx.commit()?;
            value
        };
        let cache = self.lib.data_dir().join("maintenance/staging");
        let mut staged = vec![];
        for copy in plan.groups.iter().flat_map(|g| &g.copies) {
            cancel.check()?;
            let source = &snapshots[&copy.from];
            let target = &snapshots[&copy.to];
            ensure!(
                source.row.text("state")? == Some("ready")
                    && target.row.text("state")? == Some("ready"),
                "Prepare incoming tracks before merging"
            );
            let source_path = source.row.path()?;
            let target_path = target.row.path()?;
            for (other_id, other) in &snapshots {
                if other_id != &copy.to {
                    ensure!(
                        other.row.path()?.canonicalize().ok().as_ref()
                            != Some(&target_path.canonicalize()?),
                        "Duplicate records alias the same physical path; file transfer needs review"
                    );
                }
            }
            let source_info = self.runtime.probe_cancellable(&source_path, cancel)?;
            let donor_native =
                cuerithm_autotag::read_transfer_metadata(&source_path, &source_info)?;
            ensure!(
                current_hash(&source_path)?.as_deref() == Some(source.row.hash()?),
                "Donor source changed; analyze again"
            );
            let audio = self
                .runtime
                .decode_cancellable(&target_path, &cache, cancel)?;
            ensure!(
                audio.source_sha256 == target.row.hash()?,
                "Recipient source changed; analyze again"
            );
            let target_native =
                cuerithm_autotag::read_transfer_metadata(&audio.snapshot_path, &audio.info)?;
            let imported = cuerithm_maintenance::import_record(
                &target.record(&target_native)?,
                &source.record(&donor_native)?,
            )?;
            let native =
                donor_state::native_after(&target_native, &donor_native, source, &imported)?;
            let mut cfg = AutotagConfig::default();
            cfg.options.tags.clear();
            cfg.write_energy = false;
            let input = TagInput {
                path: target_path,
                source_sha256: audio.source_sha256.clone(),
                media: audio.info.clone(),
                duration_seconds: audio.clock.duration_seconds(),
            };
            let outcome = TagMatchOutcome {
                status: MatchStatus::NoMatch,
                matches: vec![],
                attempted: vec![],
                warnings: vec![],
            };
            let write = TagWritePlan::build(input, &outcome, &cfg, &AnalysisTags::default())?
                .with_transfer(TransferPatch {
                    before: target_native.clone(),
                    after: native.clone(),
                })?
                .stage_prepared_cancellable(&self.runtime, &audio, &cache, cancel)?;
            cancel.check()?;
            std::fs::set_permissions(
                &write.backup_path,
                std::fs::metadata(&audio.source_path)?.permissions(),
            )?;
            staged.push(StagedImport {
                target: copy.to.clone(),
                donor: copy.from.clone(),
                record: imported,
                previous: target_native,
                native,
                staged: write,
            });
        }
        super::review::refresh(&self.lib, &mut preview.review, cancel)?;
        ensure!(
            !preview.review.review.stale,
            "Duplicate inputs changed while staging"
        );
        let _gate = self
            .lib
            .inner
            .tag_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Publication gate interrupted"))?;
        let id_op = uuid::Uuid::new_v4().to_string();
        let mut files = vec![];
        for (index, item) in staged.iter().enumerate() {
            let parent = item
                .staged
                .source_path
                .parent()
                .context("Missing source folder")?;
            let publish = parent.join(format!(".cuerithm-duplicate-{id_op}-{index}.tmp"));
            let displaced = if cfg!(windows) {
                publish.with_extension("before.tmp")
            } else {
                publish.clone()
            };
            files.push((
                Artifact {
                    target: item.staged.source_path.clone(),
                    staged: item.staged.path.clone(),
                    backup: Some(item.staged.backup_path.clone()),
                    before: Some(item.staged.source_before_sha256.clone()),
                    after: item.staged.source_after_sha256.clone(),
                    publish,
                    displaced,
                },
                item.staged.workspace().canonicalize()?,
            ));
        }
        let changed = preview
            .archives
            .iter()
            .map(|a| a.id.clone())
            .chain(staged.iter().map(|s| s.target.clone()))
            .chain(doomed.iter().map(|f| f.track_id.clone()))
            .collect::<BTreeSet<_>>();
        let before = changed
            .iter()
            .map(|id| (id.clone(), snapshots[id].clone()))
            .collect();
        let mut payload = Payload {
            schema: "duplicate-file-merge-v2".into(),
            preview_id: id.into(),
            kind: "apply".into(),
            undo_of: None,
            result: result(&id_op, &preview.public, None),
            before,
            after: BTreeMap::new(),
            playlists: preview.public.playlists.clone(),
            epochs: BTreeMap::new(),
            undone_by: None,
            removed: None,
            artwork_hashes: vec![],
            interrupted_deletions: vec![],
            deletions_acknowledged: false,
            transfers: staged
                .iter()
                .map(|s| {
                    (
                        s.target.clone(),
                        TransferRecord {
                            before: s.previous.clone(),
                            after: s.native.clone(),
                        },
                    )
                })
                .collect(),
            trash_intent: doomed
                .iter()
                .map(|f| {
                    Ok(TrashIntent {
                        track_id: f.track_id.clone(),
                        path: path_bytes(&snapshots[&f.track_id].row.path()?),
                    })
                })
                .collect::<Result<_>>()?,
            undo_expired: false,
        };
        // A donor copy records the tag values it replaced, so Undo does not need the original.
        let retire_backups = !payload.transfers.is_empty();
        reserve(
            &self.lib,
            &id_op,
            &payload,
            &files,
            &preview.public.stamp,
            cancel,
        )?;
        for item in &mut staged {
            item.staged.retain_for_journal();
        }
        #[cfg(test)]
        if let Some(hook) = self.hook {
            hook("prepared");
        }
        let mut trashed_log: Vec<String> = vec![];
        let mut permanent: Vec<String> = vec![];
        let mut orphans = None;
        let mut freed: Vec<String> = vec![];
        let outcome = (|| -> Result<()> {
            let mut conn = self.lib.conn()?;
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            ensure!(
                super::scan::stamp(&tx)? == preview.public.stamp,
                "Library changed before duplicate publication"
            );
            merge::check_review(&tx, &preview.public)?;
            ensure_available(&tx, &ids)?;
            let mut current: SavedPreview = merge::load(&tx, id, "duplicate_merge_plan")?;
            ensure!(
                current.consumed_by.is_none(),
                "Duplicate preview already applied"
            );
            ensure!(
                preview.review.scan.scope.time_current(crate::now()),
                "Smart scope expired; scan again"
            );
            for (id, before) in &snapshots {
                ensure!(
                    TrackImage::capture(&tx, id)? == *before,
                    "Duplicate track changed before publication"
                );
            }
            ensure!(
                merge::playlist_changes(&tx, &preview.public.plan)? == preview.public.playlists,
                "Playlist references changed"
            );
            install(&files, cancel)?;
            #[cfg(test)]
            if let Some(hook) = self.hook {
                hook("files");
            }
            let targets = files
                .iter()
                .map(|(f, _)| path_bytes(&f.target))
                .collect::<BTreeSet<_>>();
            let protected = preview
                .review
                .scan
                .files
                .iter()
                .zip(&preview.review.scan.report.tracks)
                .filter(|(f, track)| ids.contains(&track.id) && !targets.contains(&f.path))
                .map(|(file, _)| file.clone())
                .collect::<Vec<_>>();
            ensure!(
                super::files::proofs_current(&protected, cancel)?,
                "Donor or scope source changed during publication"
            );
            // Move the verified duplicates to the Trash. A record is removed below only for a
            // file that really is in the Trash; any other file stays where it is, with its
            // record, and is reported.
            if deleting {
                #[cfg(test)]
                if let Some(hook) = self.hook {
                    hook("trash");
                }
                cancel.check()?;
                let mut moved = vec![];
                for file in std::mem::take(&mut doomed) {
                    // First of all, with the write lock held, the library's own records: other work
                    // may have started on the duplicate or on the track that keeps its audio since
                    // they were checked. Removing the record would remove that work with it, so the
                    // file stays. The lock is held until the commit, so nothing can start between
                    // this look and the Trash or the permanent deletion that follows.
                    if let Some(reason) = unreserved(&tx, &file, &snapshots, &id_op)? {
                        not_deleted.push(DuplicateDeletionSkip {
                            track_id: file.track_id.clone(),
                            display_path: file.display_path.clone(),
                            keeper_id: Some(file.keeper_id.clone()),
                            reason: DuplicateSkipReason::Busy,
                            detail: Some(reason),
                        });
                        continue;
                    }
                    // The kept track's file as it is meant to be now: what was approved, or what a
                    // donor copy has just made of it.
                    let keeper_now = staged
                        .iter()
                        .find(|s| s.target == file.keeper_id)
                        .map(|s| s.staged.source_after_sha256.clone())
                        .unwrap_or_else(|| file.keeper_sha256.clone());
                    match self.dispose_verified(&file, &keeper_now, &snapshots, &plan) {
                        Ok(how) => {
                            trashed_log.push(file.display_path.clone());
                            if matches!(how, Disposal::Permanent) {
                                permanent.push(file.track_id.clone());
                            }
                            moved.push(file);
                        }
                        Err((reason, detail)) => not_deleted.push(DuplicateDeletionSkip {
                            track_id: file.track_id.clone(),
                            display_path: file.display_path.clone(),
                            keeper_id: Some(file.keeper_id.clone()),
                            reason,
                            detail: Some(detail),
                        }),
                    }
                }
                doomed = moved;
                for group in &mut plan.groups {
                    group
                        .delete_ids
                        .retain(|id| doomed.iter().any(|f| &f.track_id == id));
                }
                #[cfg(test)]
                if let Some(hook) = self.hook {
                    hook("trashed");
                }
            }
            for item in &staged {
                donor_state::write_import(
                    &tx,
                    &snapshots[&item.target],
                    &item.record,
                    &item.native,
                    &item.staged.metadata,
                    &item.staged.source_after_sha256,
                    &item.donor,
                )?;
            }
            for archive in &preview.archives {
                tx.execute(
                    "UPDATE tracks SET archived=?1,updated=?2 WHERE id=?3",
                    params![archive.after, crate::now(), archive.id],
                )?;
                if archive.after {
                    payload.result.archived_ids.push(archive.id.clone());
                } else {
                    payload.result.restored_ids.push(archive.id.clone());
                }
            }
            let playlists = if deleting {
                merge::playlist_changes(&tx, &plan)?
            } else {
                preview.public.playlists.clone()
            };
            merge::write_members(&tx, &playlists, false)?;
            payload.result.affected_playlists = playlists.iter().map(|p| p.id.clone()).collect();
            payload.playlists = playlists;
            let removed_ids = doomed
                .iter()
                .map(|f| f.track_id.clone())
                .collect::<BTreeSet<_>>();
            if !removed_ids.is_empty() {
                let removed =
                    track_removal::remove(&tx, &removed_ids.iter().cloned().collect::<Vec<_>>())?;
                payload.removed = Some(removed.image);
                payload.artwork_hashes = removed.orphans.covers();
                orphans = Some(removed.orphans);
            }
            for id in changed.iter().filter(|id| !removed_ids.contains(*id)) {
                payload
                    .after
                    .insert(id.clone(), TrackImage::capture(&tx, id)?);
            }
            payload.epochs = donor_state::epochs(
                &tx,
                &payload.after.keys().cloned().collect::<Vec<_>>(),
                &payload.result.affected_playlists,
            )?;
            payload.result.stamp = merge::advance(&tx)?;
            payload.result.updated_ids = staged.iter().map(|s| s.target.clone()).collect();
            payload.result.removed_ids = removed_ids.iter().cloned().collect();
            payload.result.trashed = doomed
                .iter()
                .filter(|f| !permanent.contains(&f.track_id))
                .cloned()
                .collect();
            payload.result.permanently_deleted = doomed
                .iter()
                .filter(|f| permanent.contains(&f.track_id))
                .cloned()
                .collect();
            payload.result.not_deleted = not_deleted.clone();
            current.consumed_by = Some(id_op.clone());
            // Once a file has gone to the Trash the library must record it, so a late
            // cancellation is ignored from here on.
            if trashed_log.is_empty() {
                cancel.check()?;
            }
            tx.execute(
                "UPDATE maintenance_writes SET state='committed',payload_json=?1 WHERE id=?2",
                params![serde_json::to_string(&payload)?, id_op],
            )?;
            tx.execute(
                "UPDATE maintenance_scans SET payload_json=?1 WHERE id=?2",
                params![serde_json::to_string(&current)?, id],
            )?;
            freed = undo_window::enforce(&tx)?;
            tx.commit()?;
            Ok(())
        })();
        let reported = outcome.as_ref().err().map(|error| {
            if trashed_log.is_empty() {
                anyhow::anyhow!("{error:#}")
            } else {
                anyhow::anyhow!(
                    "{error:#}; {} file(s) were already moved to the Trash or deleted permanently and their library records were kept: {}",
                    trashed_log.len(),
                    trashed_log.join(", ")
                )
            }
        });
        finish(&self.lib, &id_op, &files, retire_backups, reported.as_ref())?;
        outcome?;
        // Waveforms of content that is gone; rebuilt on demand if it returns. The covers of the
        // removed tracks stay until their saved records are dropped; those of removals that
        // just left the Undo window go now.
        if let Some(orphans) = &orphans {
            purge::cache_files(
                self.lib.data_dir(),
                orphans,
                std::slice::from_ref(&self.runtime.identity),
            );
        }
        purge::free_covers(&self.lib, &freed)?;
        self.lib.tidy_after_bulk_change();
        Ok(payload.result)
    }
    /// Undo of a removal only goes ahead when the removed tracks' identities and locations are
    /// still free, and nothing else sits where a removed track's file was: a file there must
    /// be the very file the record describes (for example restored from the Trash). Nothing
    /// is changed when this fails. The files are hashed here, before anything is staged or
    /// locked; the transaction that restores the records only re-checks them cheaply.
    fn verify_removed_world(
        &self,
        image: &RecordImage,
        cancel: &CancellationToken,
    ) -> Result<Vec<track_removal::OldLocation>> {
        track_removal::ensure_free(&*self.lib.conn()?, image)?;
        track_removal::observe_world(image, cancel)
    }
    /// Check the files the preview listed once more and drop any that no longer pass.
    fn reverify(
        &self,
        preview: &SavedPreview,
        plan: &mut MergePlan,
        cancel: &CancellationToken,
    ) -> Result<(Vec<DuplicateDeletionFile>, Vec<DuplicateDeletionSkip>)> {
        let shown = preview
            .public
            .deletion
            .as_ref()
            .context("This preview lists no files to delete")?;
        let now = deletion::plan(&self.lib, &preview.review.scan, plan, &*self.trash, cancel)?;
        let mut files = vec![];
        let mut skipped = shown.skipped.clone();
        skipped.extend(now.skipped);
        for mut file in now.files {
            let listed = shown.files.iter().find(|f| {
                f.track_id == file.track_id
                    && f.sha256 == file.sha256
                    && f.keeper_sha256 == file.keeper_sha256
                    && f.identity == file.identity
                    && f.keeper_id == file.keeper_id
            });
            match listed {
                // What the user confirmed stands. A file confirmed as "permanent" stays so
                // even if the Trash looks usable now (it is still tried first).
                Some(shown) if shown.permanent || !file.permanent => {
                    file.permanent = shown.permanent;
                    files.push(file);
                }
                Some(_) => skipped.push(DuplicateDeletionSkip {
                    track_id: file.track_id.clone(),
                    display_path: file.display_path.clone(),
                    keeper_id: Some(file.keeper_id.clone()),
                    reason: DuplicateSkipReason::TrashFailed,
                    detail: Some(
                        "The Trash can no longer take this file. Preview again to see it listed for permanent deletion."
                            .into(),
                    ),
                }),
                None => skipped.push(DuplicateDeletionSkip {
                    track_id: file.track_id.clone(),
                    display_path: file.display_path.clone(),
                    keeper_id: Some(file.keeper_id.clone()),
                    reason: DuplicateSkipReason::FileChanged,
                    detail: Some("The file is not the one the preview listed.".into()),
                }),
            }
        }
        for group in &mut plan.groups {
            group
                .delete_ids
                .retain(|id| files.iter().any(|f| &f.track_id == id));
        }
        Ok((files, skipped))
    }
    /// The last look before a file leaves: it is still the file that was verified, and it is
    /// not the kept track's file. Then it goes to the Trash. Only a file the user was told
    /// would be deleted permanently is deleted permanently, and only after the Trash has
    /// refused it; any other file the Trash refuses stays exactly where it is.
    fn dispose_verified(
        &self,
        file: &DuplicateDeletionFile,
        keeper_sha256: &str,
        snapshots: &BTreeMap<String, TrackImage>,
        plan: &MergePlan,
    ) -> std::result::Result<Disposal, (DuplicateSkipReason, String)> {
        let unavailable = |detail: &str| (DuplicateSkipReason::FileUnavailable, detail.to_owned());
        let path = snapshots
            .get(&file.track_id)
            .and_then(|image| image.row.path().ok())
            .ok_or_else(|| unavailable("The track is no longer part of this merge."))?;
        let keeper = plan
            .groups
            .iter()
            .find(|g| g.delete_ids.contains(&file.track_id))
            .map(|g| g.keeper.clone())
            .filter(|keeper| keeper == &file.keeper_id)
            .and_then(|keeper| snapshots.get(&keeper))
            .and_then(|image| image.row.path().ok())
            .ok_or_else(|| unavailable("The kept track is no longer part of this merge."))?;
        if !current_state(&path).is_available() {
            return Err(unavailable("The file is no longer available."));
        }
        if !current_state(&keeper).is_available() {
            return Err(unavailable("The kept track's file is no longer available."));
        }
        let keeper_changed = || {
            (
                DuplicateSkipReason::FileChanged,
                "The kept track's file changed after it was verified.".to_owned(),
            )
        };
        let file_changed = || {
            (
                DuplicateSkipReason::FileChanged,
                "The file changed after it was verified.".to_owned(),
            )
        };
        // The file that is supposed to keep the audio, first: it must still be what the deletion
        // was approved against, or what a donor copy made of it.
        let kept = HashedFile::open(&keeper)
            .ok()
            .filter(|kept| kept.hash == keeper_sha256)
            .ok_or_else(keeper_changed)?;
        // Then the copy. Its hash is taken through one open file, which also gives the identity
        // that the Trash call is checked against.
        let copy = HashedFile::open(&path)
            .ok()
            .filter(|copy| copy.hash == file.sha256)
            .ok_or_else(file_changed)?;
        if copy.same_file_as(&kept) {
            return Err((
                DuplicateSkipReason::SameFileAsKeeper,
                "This is the kept track's file.".into(),
            ));
        }
        #[cfg(test)]
        if let Some(hook) = self.hook {
            hook("verified");
        }
        // The Trash and the permanent deletion take a path. Immediately before each, both files
        // must still be the very files that were hashed: the copy has not been swapped for another
        // file under its name, and the survivor has not been replaced or edited since.
        let still = || {
            if !copy.still_at(&path) {
                Err(file_changed())
            } else if !kept.still_at(&keeper) {
                Err(keeper_changed())
            } else {
                Ok(())
            }
        };
        still()?;
        match self.trash.move_to_trash(&path) {
            Ok(()) => Ok(Disposal::Trashed),
            Err(reason) if file.permanent => {
                // The Trash took its time and refused; the file may have changed meanwhile.
                still()?;
                self.trash
                    .delete_permanently(&path)
                    .map(|()| Disposal::Permanent)
                    .map_err(|error| {
                        (
                            DuplicateSkipReason::TrashFailed,
                            format!("{reason}; permanent deletion also failed: {error}"),
                        )
                    })
            }
            Err(reason) => Err((DuplicateSkipReason::TrashFailed, reason)),
        }
    }
    /// Stages the keeper's file with the tag values from before the donor copy, written into the
    /// file as it is now through the same checked transfer as the copy itself.
    fn stage_value_restore(
        &self,
        target: &std::path::Path,
        expected_hash: &str,
        record: &TransferRecord,
        cancel: &CancellationToken,
    ) -> Result<StagedTagWrite> {
        let cache = self.lib.data_dir().join("maintenance/staging");
        let audio = self.runtime.decode_cancellable(target, &cache, cancel)?;
        ensure!(
            audio.source_sha256 == expected_hash,
            "Source changed; Undo needs review"
        );
        let current = cuerithm_autotag::read_transfer_metadata(&audio.snapshot_path, &audio.info)?;
        ensure!(current == record.after, "Later tag edits prevent Undo");
        let mut cfg = AutotagConfig::default();
        cfg.options.tags.clear();
        cfg.write_energy = false;
        let input = TagInput {
            path: target.to_path_buf(),
            source_sha256: audio.source_sha256.clone(),
            media: audio.info.clone(),
            duration_seconds: audio.clock.duration_seconds(),
        };
        let outcome = TagMatchOutcome {
            status: MatchStatus::NoMatch,
            matches: vec![],
            attempted: vec![],
            warnings: vec![],
        };
        let write = TagWritePlan::build(input, &outcome, &cfg, &AnalysisTags::default())?
            .with_transfer(TransferPatch {
                before: record.after.clone(),
                after: record.before.clone(),
            })?
            .stage_prepared_cancellable(&self.runtime, &audio, &cache, cancel)?;
        std::fs::set_permissions(
            &write.backup_path,
            std::fs::metadata(&audio.source_path)?.permissions(),
        )?;
        Ok(write)
    }
    pub fn undo(&self, id: &str, cancel: &CancellationToken) -> Result<DuplicateMergeResult> {
        cancel.check()?;
        let found: bool = self.lib.conn()?.query_row(
            "SELECT EXISTS(SELECT 1 FROM maintenance_writes WHERE id=?1)",
            [id],
            |r| r.get(0),
        )?;
        if !found {
            return self.lib.undo_duplicate_merge(id, cancel);
        }
        let original = load(&self.lib, id)?;
        ensure!(
            original.state == "committed"
                && original.payload.kind == "apply"
                && original.payload.undone_by.is_none(),
            "Duplicate operation is not available for Undo"
        );
        ensure!(
            !original.payload.undo_expired,
            "Undo is no longer available: the removed library records were kept only for the last {REMOVAL_UNDO_LIMIT} removals (duplicate merges that delete files and Missing & broken removals count together)"
        );
        ensure!(
            original.payload.removed.is_some() != original.payload.result.removed_ids.is_empty(),
            "The records removed by this merge are no longer stored; Undo needs review"
        );
        let removed_ids = original
            .payload
            .result
            .removed_ids
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        // Before anything is staged or reserved, check that the world is as the removal left it.
        let observed = match &original.payload.removed {
            Some(image) => self.verify_removed_world(image, cancel)?,
            None => vec![],
        };
        let tracks = original.payload.after.keys().cloned().collect::<Vec<_>>();
        let playlists = original.payload.result.affected_playlists.clone();
        let stamp = {
            let conn = self.lib.conn()?;
            ensure!(
                donor_state::epochs(&conn, &tracks, &playlists)? == original.payload.epochs,
                "Later relevant edits prevent Undo"
            );
            for (id, after) in &original.payload.after {
                ensure!(
                    TrackImage::capture(&conn, id)? == *after,
                    "Later track edits prevent Undo"
                );
            }
            super::scan::stamp(&conn)?
        };
        let work = tempfile::Builder::new().prefix("undo-").tempdir_in({
            let p = self.lib.data_dir().join("maintenance/staging");
            std::fs::create_dir_all(&p)?;
            p
        })?;
        let undo_id = uuid::Uuid::new_v4().to_string();
        let mut files = vec![];
        let mut value_restored: Vec<(String, StagedTagWrite)> = vec![];
        for (i, (file, _)) in original.files.iter().enumerate() {
            ensure!(
                current_hash(&file.target)?.as_deref() == Some(file.after.as_str()),
                "Source changed; Undo needs review"
            );
            if file.before.as_ref() == Some(&file.after) {
                continue;
            }
            cancel.check()?;
            let before = file
                .before
                .as_deref()
                .context("Missing original identity")?;
            // A donor copy that recorded the keeper's previous tag values is undone by writing
            // them back into the file as it is now; only an operation from an earlier version
            // is undone from its retained original.
            if let Some((track_id, record)) = original.payload.transfer_for(&file.target) {
                let staged = self.stage_value_restore(&file.target, &file.after, record, cancel)?;
                let publish = file
                    .target
                    .parent()
                    .unwrap()
                    .join(format!(".cuerithm-duplicate-{undo_id}-{i}.tmp"));
                let displaced = if cfg!(windows) {
                    publish.with_extension("before.tmp")
                } else {
                    publish.clone()
                };
                files.push((
                    Artifact {
                        target: file.target.clone(),
                        staged: staged.path.clone(),
                        backup: Some(staged.backup_path.clone()),
                        before: Some(file.after.clone()),
                        after: staged.source_after_sha256.clone(),
                        publish,
                        displaced,
                    },
                    staged.workspace().canonicalize()?,
                ));
                value_restored.push((track_id.clone(), staged));
                continue;
            }
            let old = file.backup.as_ref().context("Missing original backup")?;
            ensure!(
                current_hash(old)?.as_deref() == Some(before),
                "Original backup changed"
            );
            let stage = work.path().join(format!("restore-{i}"));
            let backup = work.path().join(format!("current-{i}"));
            publish_files::copy_prepared(old, &stage, before)?;
            publish_files::copy_prepared(&file.target, &backup, &file.after)?;
            let publish = file
                .target
                .parent()
                .unwrap()
                .join(format!(".cuerithm-duplicate-{undo_id}-{i}.tmp"));
            let displaced = if cfg!(windows) {
                publish.with_extension("before.tmp")
            } else {
                publish.clone()
            };
            files.push((
                Artifact {
                    target: file.target.clone(),
                    staged: stage,
                    backup: Some(backup),
                    before: Some(file.after.clone()),
                    after: before.into(),
                    publish,
                    displaced,
                },
                work.path().canonicalize()?,
            ));
        }
        let mut result = original.payload.result.clone();
        result.id = undo_id.clone();
        result.undo_of = Some(id.into());
        result.stamp = stamp.clone();
        std::mem::swap(&mut result.archived_ids, &mut result.restored_ids);
        // Removed records come back as restored ones; the files stay wherever the user has them.
        result
            .restored_ids
            .extend(std::mem::take(&mut result.removed_ids));
        result.trashed.clear();
        result.permanently_deleted.clear();
        result.not_deleted.clear();
        let mut payload = Payload {
            schema: "duplicate-file-merge-v2".into(),
            preview_id: original.payload.preview_id.clone(),
            kind: "undo".into(),
            undo_of: Some(id.into()),
            result,
            before: original.payload.after.clone(),
            after: original.payload.before.clone(),
            playlists: original.payload.playlists.clone(),
            epochs: BTreeMap::new(),
            undone_by: None,
            removed: None,
            trash_intent: vec![],
            undo_expired: false,
            artwork_hashes: vec![],
            interrupted_deletions: vec![],
            deletions_acknowledged: false,
            transfers: BTreeMap::new(),
        };
        let _gate = self
            .lib
            .inner
            .tag_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Publication gate interrupted"))?;
        reserve(&self.lib, &undo_id, &payload, &files, &stamp, cancel)?;
        let _kept = work.keep();
        for (_, staged) in &mut value_restored {
            staged.retain_for_journal();
        }
        let outcome = (|| -> Result<()> {
            let mut conn = self.lib.conn()?;
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            ensure!(
                donor_state::epochs(&tx, &tracks, &playlists)? == original.payload.epochs,
                "Later relevant edits prevent Undo"
            );
            let (state, text): (String, String) = tx.query_row(
                "SELECT state,payload_json FROM maintenance_writes WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let mut previous: Payload = serde_json::from_str(&text)?;
            ensure!(
                state == "committed" && previous.undone_by.is_none(),
                "Duplicate operation already undone"
            );
            for (id, after) in &original.payload.after {
                ensure!(
                    TrackImage::capture(&tx, id)? == *after,
                    "Later track edits prevent Undo"
                );
            }
            for p in &payload.playlists {
                let actual: Vec<String> = {
                    let mut q=tx.prepare("SELECT track_id FROM playlist_members WHERE playlist_id=?1 ORDER BY position")?;
                    q.query_map([&p.id], |r| r.get(0))?
                        .collect::<rusqlite::Result<_>>()?
                };
                ensure!(actual == p.after, "Later playlist edits prevent Undo");
            }
            // The removed records come back first: if the library no longer matches what the
            // removal left, this fails before any file is exchanged.
            if let Some(image) = &original.payload.removed {
                for location in &observed {
                    location.unchanged()?;
                }
                let report: RestoreReport = track_removal::restore(&tx, image)?;
                payload.result.links_not_restored = report.left_alone();
            }
            install(&files, cancel)?;
            #[cfg(test)]
            if let Some(hook) = self.hook {
                hook("files");
            }
            for (track_id, image) in &payload.after {
                if !removed_ids.contains(track_id) {
                    donor_state::restore(&tx, image)?;
                }
            }
            // The file Undo wrote has the old tags but not the old bytes: the library knows the
            // file it really has.
            for (track_id, staged) in &value_restored {
                tx.execute(
                    "UPDATE tracks SET source_hash=?1,metadata_json=?2 WHERE id=?3",
                    params![
                        staged.source_after_sha256,
                        serde_json::to_string(&staged.metadata)?,
                        track_id
                    ],
                )?;
            }
            merge::write_members(&tx, &payload.playlists, true)?;
            payload.result.stamp = merge::advance(&tx)?;
            payload.epochs = donor_state::epochs(&tx, &tracks, &playlists)?;
            previous.undone_by = Some(undo_id.clone());
            // The covers stay: the restored records use them again.
            previous.forget_removed();
            cancel.check()?;
            tx.execute(
                "UPDATE maintenance_writes SET state='undone',payload_json=?1 WHERE id=?2",
                params![serde_json::to_string(&previous)?, id],
            )?;
            tx.execute(
                "UPDATE maintenance_writes SET state='committed',payload_json=?1 WHERE id=?2",
                params![serde_json::to_string(&payload)?, undo_id],
            )?;
            tx.commit()?;
            Ok(())
        })();
        finish(&self.lib, &undo_id, &files, true, outcome.as_ref().err())?;
        outcome?;
        Ok(payload.result)
    }
}
impl Library {
    pub fn duplicate_recovery(&self) -> Result<Vec<DuplicateRecovery>> {
        let conn = self.conn()?;
        let mut q = conn.prepare(&format!(
            "SELECT id,kind,state,created,error,{UNACKNOWLEDGED_DELETIONS} FROM maintenance_writes WHERE state IN ('prepared','conflict') OR (cleanup_complete=0 AND error IS NOT NULL) OR {UNACKNOWLEDGED_DELETIONS} ORDER BY created,id"
        ))?;
        Ok(q.query_map([], |r| {
            Ok(DuplicateRecovery {
                id: r.get(0)?,
                kind: r.get(1)?,
                state: r.get(2)?,
                created: r.get(3)?,
                error: r.get(4)?,
                files_moved: r.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?)
    }
    pub fn retry_duplicate_recovery(
        &self,
        id: &str,
        cancel: &CancellationToken,
    ) -> Result<Vec<DuplicateRecovery>> {
        cancel.check()?;
        let _gate = self
            .inner
            .tag_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Publication gate interrupted"))?;
        let write = load(self, id)?;
        if matches!(write.state.as_str(), "prepared" | "conflict") {
            let error = interruption(&write, "Explicit duplicate recovery retry");
            let gone = gone_files(&write);
            finish(self, id, &write.files, write.records_values(), Some(&error))?;
            self.note_interrupted_deletions(id, &gone)?;
        } else {
            finish(self, id, &write.files, write.records_values(), None)?;
            // A rolled-back operation that had already moved files is shown until the DJ has
            // seen it; selecting it in History acknowledges it, and it does not come back.
            if write.state == "rolled_back" && !write.payload.interrupted_deletions.is_empty() {
                self.acknowledge_interrupted_deletions(id)?;
            }
        }
        self.duplicate_recovery()
    }
    /// Remembers which files an interrupted operation had already moved away, so that History
    /// keeps showing the operation after it was rolled back.
    fn note_interrupted_deletions(&self, id: &str, files: &[String]) -> Result<()> {
        if files.is_empty() {
            return Ok(());
        }
        self.conn()?.execute(
            "UPDATE maintenance_writes SET payload_json=json_set(payload_json,'$.interrupted_deletions',json(?1),'$.deletions_acknowledged',json('false')) WHERE id=?2",
            params![serde_json::to_string(files)?, id],
        )?;
        Ok(())
    }
    fn acknowledge_interrupted_deletions(&self, id: &str) -> Result<()> {
        self.conn()?.execute(
            "UPDATE maintenance_writes SET payload_json=json_set(payload_json,'$.deletions_acknowledged',json('true')) WHERE id=?1",
            params![id],
        )?;
        Ok(())
    }
    pub(crate) fn recover_duplicate_writes(&self) -> Result<()> {
        let ids = {
            let conn = self.conn()?;
            let mut q = conn.prepare(
                "SELECT id FROM maintenance_writes WHERE state='prepared' ORDER BY created,id",
            )?;
            q.query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        for id in ids {
            let write = load(self, &id)?;
            let error = interruption(&write, "Interrupted duplicate operation rolled back");
            let gone = gone_files(&write);
            finish(
                self,
                &write.id,
                &write.files,
                write.records_values(),
                Some(&error),
            )?;
            self.note_interrupted_deletions(&write.id, &gone)?;
        }
        let ids = {
            let conn = self.conn()?;
            let mut q=conn.prepare("SELECT id FROM maintenance_writes WHERE state IN ('committed','undone','rolled_back') AND cleanup_complete=0")?;
            q.query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        for id in ids {
            let write = load(self, &id)?;
            finish(self, &id, &write.files, write.records_values(), None)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DuplicateReviewEdit, DuplicateScanRequest, GroupEdit};
    use std::sync::Arc;
    fn fixture() -> (
        tempfile::TempDir,
        Library,
        MediaRuntime,
        DuplicateMergePreview,
        PathBuf,
        String,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::open(&dir.path().join("app/library.sqlite")).unwrap();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let runtime = MediaRuntime::from_bundle(&root.join("tools/media-runtime/bin")).unwrap();
        let mut ids = vec![];
        let mut paths = vec![];
        for index in 0..2 {
            let path = dir.path().join(format!("track-{index}.wav"));
            let mut w = hound::WavWriter::create(
                &path,
                hound::WavSpec {
                    channels: 1,
                    sample_rate: 8000,
                    bits_per_sample: 16,
                    sample_format: hound::SampleFormat::Int,
                },
            )
            .unwrap();
            for i in 0..16000 {
                w.write_sample(((i * 37) % 20001 - 10000) as i16).unwrap();
            }
            w.finalize().unwrap();
            let job = lib
                .enqueue(
                    std::slice::from_ref(&path),
                    lib.settings().unwrap().revision,
                )
                .unwrap()[0]
                .clone();
            let id = lib
                .jobs()
                .unwrap()
                .into_iter()
                .find(|j| j.id == job)
                .unwrap()
                .track_id;
            ids.push(id);
            paths.push(path);
        }
        {
            let conn = lib.conn().unwrap();
            conn.execute(
                "UPDATE tracks SET state='ready',duration=20,title='Before',artist='North'",
                [],
            )
            .unwrap();
            conn.execute("UPDATE tracks SET title='Donor' WHERE id=?1", [&ids[1]])
                .unwrap();
        }
        let token = CancellationToken::default();
        let scan = lib
            .scan_duplicates(DuplicateScanRequest::default(), &runtime, &token)
            .unwrap();
        let review = lib.begin_duplicate_review(&scan.id, &token).unwrap();
        let review = lib
            .edit_duplicate_review(
                &review.id,
                0,
                DuplicateReviewEdit::Group {
                    group_id: review.groups[0].id.clone(),
                    edit: GroupEdit::UseTags {
                        track_id: Some(ids[1].clone()),
                    },
                },
                &token,
            )
            .unwrap();
        let preview = lib
            .preview_duplicate_merge(
                &review.id,
                review.revision,
                vec![review.groups[0].id.clone()],
                &token,
            )
            .unwrap();
        (dir, lib, runtime, preview, paths[0].clone(), ids[0].clone())
    }
    fn crash_prepared(phase: &str) {
        if phase == "prepared" {
            panic!("simulated prepared interruption");
        }
    }
    fn crash_files(phase: &str) {
        if phase == "files" {
            panic!("simulated exchanged-file interruption");
        }
    }
    #[test]
    fn recovery_rolls_back_prepared_and_exchanged_apply_without_publishing_library_changes() {
        for hook in [crash_prepared as fn(&str), crash_files as fn(&str)] {
            let (dir, lib, runtime, preview, path, target) = fixture();
            let hash = cuerithm_media::file_sha256(&path).unwrap();
            let mut service = DuplicateMergeService::new(lib.clone(), runtime);
            service.hook = Some(hook);
            assert!(
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                    || service.apply(&preview.id, &CancellationToken::default())
                ))
                .is_err()
            );
            drop(service);
            drop(lib);
            let lib = Library::open(&dir.path().join("app/library.sqlite")).unwrap();
            assert_eq!(cuerithm_media::file_sha256(&path).unwrap(), hash);
            assert_eq!(lib.track(&target).unwrap().title, "Before");
            // Nothing had left its place, so there is nothing for the DJ to be told.
            assert!(lib.duplicate_recovery().unwrap().is_empty());
            assert_eq!(
                lib.conn()
                    .unwrap()
                    .query_row(
                        "SELECT state FROM maintenance_writes ORDER BY created DESC LIMIT 1",
                        [],
                        |r| r.get::<_, String>(0)
                    )
                    .unwrap(),
                "rolled_back"
            );
        }
    }
    #[test]
    fn interrupted_undo_recovers_to_the_committed_after_state_and_can_be_retried() {
        let (dir, lib, runtime, preview, path, target) = fixture();
        // Undo writes the recorded tags back; the audio and the tags are what must return.
        let content = |path: &std::path::Path| {
            let audio = runtime
                .decode_cancellable(path, dir.path(), &CancellationToken::default())
                .unwrap();
            let tags = cuerithm_autotag::read_transfer_metadata(&audio.snapshot_path, &audio.info)
                .unwrap();
            (audio.pcm_sha256().unwrap(), tags)
        };
        let original = content(&path);
        let mut service = DuplicateMergeService::new(lib.clone(), runtime.clone());
        let applied = service
            .apply(&preview.id, &CancellationToken::default())
            .unwrap();
        let after = cuerithm_media::file_sha256(&path).unwrap();
        service.hook = Some(crash_files);
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                || service.undo(&applied.id, &CancellationToken::default())
            ))
            .is_err()
        );
        drop(service);
        drop(lib);
        let lib = Library::open(&dir.path().join("app/library.sqlite")).unwrap();
        assert_eq!(cuerithm_media::file_sha256(&path).unwrap(), after);
        assert_eq!(lib.track(&target).unwrap().title, "Donor");
        DuplicateMergeService::new(lib, runtime.clone())
            .undo(&applied.id, &CancellationToken::default())
            .unwrap();
        assert_eq!(content(&path), original);
    }
    struct MustNotRun;
    impl FileTrash for MustNotRun {
        fn move_to_trash(&self, path: &std::path::Path) -> std::result::Result<(), String> {
            panic!("{} must not be moved to the Trash", path.display());
        }
    }
    static CHANGE_BEFORE_TRASH: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);
    fn change_at(phase: &str, wanted: &str) {
        if phase == wanted
            && let Some(path) = CHANGE_BEFORE_TRASH.lock().unwrap().clone()
        {
            let mut bytes = std::fs::read(&path).unwrap();
            bytes.push(0);
            std::fs::write(&path, bytes).unwrap();
        }
    }
    fn change_after_install(phase: &str) {
        change_at(phase, "files");
    }
    fn change_just_before_trash(phase: &str) {
        change_at(phase, "trash");
    }
    #[test]
    fn a_file_that_changes_after_verification_is_kept_with_its_record() {
        // Changed after publication began: the whole merge stops and nothing is moved.
        // Changed just before it would be moved: only that file is kept.
        for (hook, whole_merge_fails, keeper) in [
            (change_after_install as fn(&str), true, false),
            (change_just_before_trash as fn(&str), false, false),
            // The kept track's file changes instead: the copy is not deleted either, because
            // the file that was supposed to hold the audio is no longer the one approved.
            (change_after_install as fn(&str), true, true),
            (change_just_before_trash as fn(&str), false, true),
        ] {
            changed_duplicate_is_kept(hook, whole_merge_fails, keeper);
        }
    }
    /// Identical tracks with a Delete from disk preview ready to apply.
    struct DeleteReady {
        dir: tempfile::TempDir,
        lib: Library,
        runtime: MediaRuntime,
        preview: DuplicateMergePreview,
        ids: Vec<String>,
        /// Where each track of `ids` has its file.
        paths: Vec<PathBuf>,
        paths_zero: PathBuf,
        duplicate: PathBuf,
        token: CancellationToken,
    }
    fn delete_ready() -> DeleteReady {
        delete_ready_of(2)
    }
    fn delete_ready_of(count: usize) -> DeleteReady {
        let (dir, lib, runtime, _donor_preview, paths_zero, _target) = fixture();
        let token = CancellationToken::default();
        let mut ids = {
            let conn = lib.conn().unwrap();
            let mut q = conn
                .prepare("SELECT id FROM tracks ORDER BY rowid")
                .unwrap();
            q.query_map([], |r| r.get::<_, String>(0))
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap()
        };
        let duplicate = dir.path().join("track-1.wav");
        // Make the two tracks the same audio so the second can be deleted.
        std::fs::copy(&paths_zero, &duplicate).unwrap();
        let hash = cuerithm_media::file_sha256(&duplicate).unwrap();
        let mut paths = vec![paths_zero.clone(), duplicate.clone()];
        {
            let conn = lib.conn().unwrap();
            conn.execute(
                "UPDATE tracks SET source_hash=?1 WHERE id=?2",
                params![hash, ids[1]],
            )
            .unwrap();
            // A ready track has finished its preparation job.
            conn.execute("UPDATE jobs SET state='completed',stage='completed'", [])
                .unwrap();
        }
        for index in 2..count {
            let path = dir.path().join(format!("track-{index}.wav"));
            std::fs::copy(&paths_zero, &path).unwrap();
            let job = lib
                .enqueue(
                    std::slice::from_ref(&path),
                    lib.settings().unwrap().revision,
                )
                .unwrap()[0]
                .clone();
            let id = lib
                .jobs()
                .unwrap()
                .into_iter()
                .find(|j| j.id == job)
                .unwrap()
                .track_id;
            let conn = lib.conn().unwrap();
            conn.execute(
                "UPDATE tracks SET state='ready',duration=20,title='Before',artist='North' WHERE id=?1",
                [&id],
            )
            .unwrap();
            conn.execute("UPDATE jobs SET state='completed',stage='completed'", [])
                .unwrap();
            ids.push(id);
            paths.push(path);
        }
        let scan = lib
            .scan_duplicates(DuplicateScanRequest::default(), &runtime, &token)
            .unwrap();
        let review = lib.begin_duplicate_review(&scan.id, &token).unwrap();
        let preview = lib
            .preview_duplicate_merge_choice(
                &review.id,
                review.revision,
                vec![review.groups[0].id.clone()],
                Some(MergeDisposition::Delete),
                &token,
            )
            .unwrap();
        DeleteReady {
            dir,
            lib,
            runtime,
            preview,
            ids,
            paths,
            paths_zero,
            duplicate,
            token,
        }
    }
    fn changed_duplicate_is_kept(hook: fn(&str), whole_merge_fails: bool, change_keeper: bool) {
        let DeleteReady {
            dir: _dir,
            lib,
            runtime,
            preview,
            ids,
            paths_zero,
            duplicate,
            token,
            ..
        } = delete_ready();
        let listed = preview.deletion.as_ref().unwrap().files.clone();
        assert_eq!(listed.len(), 1);
        let removed = listed[0].track_id.clone();
        let removed_path = if removed == ids[1] {
            duplicate.clone()
        } else {
            paths_zero.clone()
        };
        let keeper_path = if removed == ids[1] {
            paths_zero.clone()
        } else {
            duplicate.clone()
        };
        assert_eq!(
            listed[0].keeper_id,
            if removed == ids[1] { &ids[0] } else { &ids[1] }.as_str()
        );
        *CHANGE_BEFORE_TRASH.lock().unwrap() = Some(if change_keeper {
            keeper_path.clone()
        } else {
            removed_path.clone()
        });
        let mut service =
            DuplicateMergeService::with_trash(lib.clone(), runtime, Arc::new(MustNotRun));
        service.hook = Some(hook);
        let result = service.apply_confirmed(&preview.id, true, &token);
        *CHANGE_BEFORE_TRASH.lock().unwrap() = None;
        if whole_merge_fails {
            assert!(result.is_err());
        } else {
            let result = result.unwrap();
            assert!(result.removed_ids.is_empty());
            assert!(result.trashed.is_empty());
            assert_eq!(result.not_deleted.len(), 1);
            assert_eq!(
                result.not_deleted[0].reason,
                DuplicateSkipReason::FileChanged
            );
            if change_keeper {
                assert!(
                    result.not_deleted[0]
                        .detail
                        .as_deref()
                        .unwrap_or_default()
                        .contains("kept track"),
                    "{:?}",
                    result.not_deleted[0]
                );
            }
        }
        assert!(removed_path.exists(), "the changed file stays where it was");
        assert!(
            lib.track(&removed).is_ok(),
            "its record stays in the library"
        );
        assert!(!lib.track(&removed).unwrap().archived);
    }
    static SWAP_CANDIDATE: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);
    /// Replaces the file at the path, as another program could, with a different file of the same
    /// size and modification time (so only the file's identity gives the swap away).
    fn replace_file(path: &std::path::Path) {
        let meta = std::fs::metadata(path).unwrap();
        let temp = path.with_extension("swap");
        std::fs::write(&temp, vec![0xA5u8; meta.len() as usize]).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&temp)
            .unwrap()
            .set_modified(meta.modified().unwrap())
            .unwrap();
        std::fs::rename(&temp, path).unwrap();
    }
    fn swap_candidate(phase: &str) {
        if phase == "verified"
            && let Some(path) = SWAP_CANDIDATE.lock().unwrap().clone()
        {
            replace_file(&path);
        }
    }
    /// The Trash refuses the file after swapping it, so the permanent fallback runs.
    struct SwapThenRefuse(PathBuf);
    impl FileTrash for SwapThenRefuse {
        fn can_trash(&self, _: &std::path::Path) -> bool {
            false
        }
        fn move_to_trash(&self, _: &std::path::Path) -> std::result::Result<(), String> {
            replace_file(&self.0);
            Err("The Trash refused the file".into())
        }
        fn delete_permanently(&self, path: &std::path::Path) -> std::result::Result<(), String> {
            panic!("{} must not be deleted permanently", path.display());
        }
    }
    /// The duplicate that a Delete from disk preview lists, and where it is.
    fn listed_duplicate(ready: &DeleteReady) -> (String, PathBuf) {
        let listed = ready.preview.deletion.as_ref().unwrap().files.clone();
        assert_eq!(listed.len(), 1);
        let removed = listed[0].track_id.clone();
        let path = if removed == ready.ids[1] {
            ready.duplicate.clone()
        } else {
            ready.paths_zero.clone()
        };
        (removed, path)
    }
    fn assert_replacement_survived(
        result: &DuplicateMergeResult,
        lib: &Library,
        removed: &str,
        path: &std::path::Path,
    ) {
        assert!(result.removed_ids.is_empty() && result.trashed.is_empty());
        assert!(result.permanently_deleted.is_empty());
        assert_eq!(result.not_deleted.len(), 1);
        assert_eq!(
            result.not_deleted[0].reason,
            DuplicateSkipReason::FileChanged
        );
        assert_eq!(
            result.not_deleted[0].detail.as_deref(),
            Some("The file changed after it was verified.")
        );
        assert!(
            std::fs::read(path).unwrap().iter().all(|b| *b == 0xA5),
            "the replacement is untouched"
        );
        assert!(lib.track(removed).is_ok(), "its record stays");
    }
    #[test]
    fn a_file_swapped_after_it_was_verified_is_not_trashed() {
        let ready = delete_ready();
        let (removed, path) = listed_duplicate(&ready);
        *SWAP_CANDIDATE.lock().unwrap() = Some(path.clone());
        let mut service = DuplicateMergeService::with_trash(
            ready.lib.clone(),
            ready.runtime.clone(),
            Arc::new(MustNotRun),
        );
        service.hook = Some(swap_candidate);
        let result = service.apply_confirmed(&ready.preview.id, true, &ready.token);
        *SWAP_CANDIDATE.lock().unwrap() = None;
        assert_replacement_survived(&result.unwrap(), &ready.lib, &removed, &path);
    }
    #[test]
    fn a_file_swapped_while_the_trash_refuses_it_is_not_deleted_permanently() {
        let ready = delete_ready();
        let (removed, path) = listed_duplicate(&ready);
        let trash = Arc::new(SwapThenRefuse(path.clone()));
        // The preview names the file as one the Trash cannot take, so it may be deleted for good,
        // but only the file that was verified.
        let preview = ready
            .lib
            .preview_duplicate_merge_choice_with(
                &ready.preview.review_id,
                ready.preview.review_revision,
                vec![ready.preview.plan.groups[0].group_id.clone()],
                Some(MergeDisposition::Delete),
                &*trash,
                &ready.token,
            )
            .unwrap();
        assert!(preview.deletion.as_ref().unwrap().files[0].permanent);
        let service = DuplicateMergeService::with_trash(
            ready.lib.clone(),
            ready.runtime.clone(),
            trash.clone(),
        );
        let result = service
            .apply_confirmed(&preview.id, true, &ready.token)
            .unwrap();
        assert_replacement_survived(&result, &ready.lib, &removed, &path);
    }
    static SWAP_KEEPER: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);
    fn swap_keeper(phase: &str) {
        if phase == "verified"
            && let Some(path) = SWAP_KEEPER.lock().unwrap().clone()
        {
            replace_file(&path);
        }
    }
    #[test]
    fn a_kept_file_swapped_after_it_was_verified_keeps_the_copy() {
        let ready = delete_ready();
        let (removed, path) = listed_duplicate(&ready);
        let keeper_path = if path == ready.duplicate {
            ready.paths_zero.clone()
        } else {
            ready.duplicate.clone()
        };
        *SWAP_KEEPER.lock().unwrap() = Some(keeper_path.clone());
        let mut service = DuplicateMergeService::with_trash(
            ready.lib.clone(),
            ready.runtime.clone(),
            Arc::new(MustNotRun),
        );
        service.hook = Some(swap_keeper);
        let result = service.apply_confirmed(&ready.preview.id, true, &ready.token);
        *SWAP_KEEPER.lock().unwrap() = None;
        let result = result.unwrap();
        assert!(result.removed_ids.is_empty() && result.trashed.is_empty());
        assert_eq!(result.not_deleted.len(), 1);
        assert_eq!(
            result.not_deleted[0].detail.as_deref(),
            Some("The kept track's file changed after it was verified.")
        );
        assert!(path.exists() && ready.lib.track(&removed).is_ok());
    }
    /// A Trash that really moves the file, into a folder of the test.
    struct MovesAside(PathBuf);
    impl FileTrash for MovesAside {
        fn move_to_trash(&self, path: &std::path::Path) -> std::result::Result<(), String> {
            std::fs::create_dir_all(&self.0).map_err(|e| e.to_string())?;
            std::fs::rename(path, self.0.join(path.file_name().unwrap())).map_err(|e| e.to_string())
        }
    }
    fn exit_after_trash(phase: &str) {
        if phase == "trashed" {
            panic!("simulated exit after a file went to the Trash");
        }
    }
    #[test]
    fn a_deletion_interrupted_after_the_file_went_to_the_trash_stays_visible_until_acknowledged() {
        let ready = delete_ready();
        let listed = ready.preview.deletion.as_ref().unwrap().files.clone();
        assert_eq!(listed.len(), 1);
        let removed = listed[0].track_id.clone();
        let removed_path = if removed == ready.ids[1] {
            ready.duplicate.clone()
        } else {
            ready.paths_zero.clone()
        };
        let database = ready.dir.path().join("app/library.sqlite");
        let mut service = DuplicateMergeService::with_trash(
            ready.lib.clone(),
            ready.runtime.clone(),
            Arc::new(MovesAside(ready.dir.path().join("moved-aside"))),
        );
        service.hook = Some(exit_after_trash);
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                service.apply_confirmed(&ready.preview.id, true, &ready.token)
            }))
            .is_err()
        );
        assert!(!removed_path.exists(), "the file had already gone");
        let DeleteReady { dir, lib, .. } = ready;
        drop(service);
        drop(lib);
        // Start-up recovery rolls the operation back; the record is kept, the file is gone.
        let lib = Library::open(&database).unwrap();
        assert!(lib.track(&removed).is_ok());
        let pending = lib.duplicate_recovery().unwrap();
        assert_eq!(pending.len(), 1, "{pending:?}");
        assert_eq!(pending[0].state, "rolled_back");
        assert!(
            pending[0].files_moved,
            "the file left its place: {pending:?}"
        );
        // The entry names the canonical path, which on Windows can differ from the test's
        // temp-dir spelling (a short 8.3 name), so accept either form.
        let shown = removed_path.display().to_string();
        let canonical = removed_path
            .parent()
            .unwrap()
            .canonicalize()
            .unwrap()
            .join(removed_path.file_name().unwrap())
            .display()
            .to_string();
        let error = pending[0].error.as_deref().unwrap_or_default();
        assert!(
            error.contains(&shown) || error.contains(&canonical),
            "the entry names the file: {:?}",
            pending[0].error
        );
        // It survives another start, until someone acknowledges it.
        drop(lib);
        let lib = Library::open(&database).unwrap();
        assert_eq!(lib.duplicate_recovery().unwrap().len(), 1);
        let after = lib
            .retry_duplicate_recovery(&pending[0].id, &CancellationToken::default())
            .unwrap();
        assert!(after.is_empty(), "acknowledged: {after:?}");
        drop(lib);
        let lib = Library::open(&database).unwrap();
        assert!(lib.duplicate_recovery().unwrap().is_empty());
        assert!(lib.track(&removed).is_ok(), "the record was never removed");
        drop(dir);
    }
    #[test]
    fn a_cleanup_failure_after_a_rollback_is_retried_not_reported_as_moved_files() {
        let (dir, lib, runtime, preview, _, _) = fixture();
        let mut service = DuplicateMergeService::new(lib.clone(), runtime);
        service.hook = Some(crash_files);
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                || service.apply(&preview.id, &CancellationToken::default())
            ))
            .is_err()
        );
        drop(service);
        drop(lib);
        let database = dir.path().join("app/library.sqlite");
        let lib = Library::open(&database).unwrap();
        assert!(lib.duplicate_recovery().unwrap().is_empty());
        // The rollback finished, but removing its temporary files failed.
        lib.conn()
            .unwrap()
            .execute(
                "UPDATE maintenance_writes SET cleanup_complete=0,error='Recovery cleanup pending: simulated' WHERE state='rolled_back'",
                [],
            )
            .unwrap();
        let pending = lib.duplicate_recovery().unwrap();
        assert_eq!(pending.len(), 1, "{pending:?}");
        assert_eq!(pending[0].state, "rolled_back");
        assert!(
            !pending[0].files_moved,
            "no audio file left its place, so this is a retry and not an acknowledgement: {pending:?}"
        );
        let after = lib
            .retry_duplicate_recovery(&pending[0].id, &CancellationToken::default())
            .unwrap();
        assert!(
            after.is_empty(),
            "the retry finished the cleanup: {after:?}"
        );
        drop(dir);
    }
    thread_local! {
        static LATE_WORK: std::cell::RefCell<Option<(Library, Vec<String>)>> =
            const { std::cell::RefCell::new(None) };
    }
    /// Another part of the app starts work in the library after the files were checked and the
    /// merge was reserved, but before the write transaction that removes the records opens.
    fn start_late_work(phase: &str) {
        if phase == "prepared"
            && let Some((lib, statements)) = LATE_WORK.with(|work| work.borrow_mut().take())
        {
            let conn = lib.conn().unwrap();
            for sql in statements {
                conn.execute_batch(&sql).unwrap();
            }
        }
    }
    /// A file organization of the track that has not finished.
    fn organizing(track: &str) -> String {
        format!(
            "INSERT INTO organization_operations VALUES('late-{track}','{track}','publishing','{{}}',NULL,0,0)"
        )
    }
    /// An unsaved metadata draft that lists the track.
    fn drafting(track: &str) -> String {
        format!(
            "INSERT INTO metadata_edit_drafts VALUES('late-draft-{track}',1,'editing','{{\"track_ids\":[\"{track}\"]}}',1,1)"
        )
    }
    /// A prepared write of another operation that targets the file.
    fn reserving(path: &std::path::Path) -> String {
        let hex = path_bytes(path)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        format!(
            "INSERT INTO maintenance_writes(id,kind,state,payload_json,created) VALUES('late-write','apply','prepared','{{}}',0);\
             INSERT INTO maintenance_write_files VALUES('late-write',0,X'{hex}',X'00',NULL,NULL,'aa',X'00',X'00',X'00')"
        )
    }
    /// Applies the Delete from disk preview, with a Trash that really moves files aside, while
    /// `statements` run against the library at the last moment before the records are removed.
    fn apply_with_late_work(
        ready: &DeleteReady,
        statements: Vec<String>,
    ) -> (Result<DuplicateMergeResult>, PathBuf) {
        let aside = ready.dir.path().join("moved-aside");
        LATE_WORK.with(|work| *work.borrow_mut() = Some((ready.lib.clone(), statements)));
        let mut service = DuplicateMergeService::with_trash(
            ready.lib.clone(),
            ready.runtime.clone(),
            Arc::new(MovesAside(aside.clone())),
        );
        service.hook = Some(start_late_work);
        let result = service.apply_confirmed(&ready.preview.id, true, &ready.token);
        LATE_WORK.with(|work| *work.borrow_mut() = None);
        (result, aside)
    }
    fn rows(lib: &Library, sql: &str) -> i64 {
        lib.conn()
            .unwrap()
            .query_row(sql, [], |r| r.get(0))
            .unwrap()
    }
    /// The duplicate was left exactly as it was, and the work that started on it was not touched.
    fn assert_left_alone(
        ready: &DeleteReady,
        result: &DuplicateMergeResult,
        aside: &std::path::Path,
        removed: &str,
        path: &std::path::Path,
    ) {
        assert!(result.removed_ids.is_empty(), "{result:?}");
        assert!(result.trashed.is_empty() && result.permanently_deleted.is_empty());
        assert_eq!(result.not_deleted.len(), 1, "{:?}", result.not_deleted);
        assert_eq!(result.not_deleted[0].track_id, removed);
        assert_eq!(result.not_deleted[0].reason, DuplicateSkipReason::Busy);
        assert!(path.exists(), "the file stays where it was");
        assert!(!aside.exists(), "nothing was moved to the Trash");
        assert!(ready.lib.track(removed).is_ok(), "its record stays");
    }
    #[test]
    fn a_duplicate_that_started_work_after_it_was_verified_is_kept_with_that_work() {
        let ready = delete_ready();
        let (removed, path) = listed_duplicate(&ready);
        let (result, aside) = apply_with_late_work(&ready, vec![organizing(&removed)]);
        let result = result.unwrap();
        assert_left_alone(&ready, &result, &aside, &removed, &path);
        assert!(ready.lib.duplicate_recovery().unwrap().is_empty());
        assert!(
            result.not_deleted[0]
                .detail
                .as_deref()
                .unwrap_or_default()
                .contains("organization"),
            "{:?}",
            result.not_deleted[0]
        );
        assert_eq!(
            rows(
                &ready.lib,
                &format!(
                    "SELECT COUNT(*) FROM organization_operations WHERE track_id='{removed}' AND state='publishing'"
                )
            ),
            1,
            "the operation that started was not abandoned"
        );
    }
    #[test]
    fn a_duplicate_another_operation_reserved_after_it_was_verified_is_kept_with_that_reservation()
    {
        let ready = delete_ready();
        let (removed, path) = listed_duplicate(&ready);
        // The library knows the file by its canonical location, which is what is reserved.
        let stored = PathBuf::from(&ready.preview.deletion.as_ref().unwrap().files[0].display_path);
        let (result, aside) = apply_with_late_work(&ready, vec![reserving(&stored)]);
        let result = result.unwrap();
        assert_left_alone(&ready, &result, &aside, &removed, &path);
        assert_eq!(
            result.not_deleted[0].detail.as_deref(),
            Some("Another operation is using this file.")
        );
        assert_eq!(
            rows(
                &ready.lib,
                "SELECT COUNT(*) FROM maintenance_writes WHERE id='late-write' AND state='prepared'"
            ),
            1
        );
    }
    #[test]
    fn a_kept_track_that_started_work_after_it_was_verified_keeps_the_copy() {
        let ready = delete_ready();
        let (removed, path) = listed_duplicate(&ready);
        let keeper = ready.preview.deletion.as_ref().unwrap().files[0]
            .keeper_id
            .clone();
        let (result, aside) = apply_with_late_work(&ready, vec![organizing(&keeper)]);
        let result = result.unwrap();
        assert_left_alone(&ready, &result, &aside, &removed, &path);
        assert!(
            result.not_deleted[0]
                .detail
                .as_deref()
                .unwrap_or_default()
                .contains("kept track"),
            "{:?}",
            result.not_deleted[0]
        );
        assert_eq!(
            rows(
                &ready.lib,
                &format!("SELECT COUNT(*) FROM organization_operations WHERE track_id='{keeper}'")
            ),
            1
        );
    }
    #[test]
    fn a_duplicate_that_joined_a_metadata_draft_after_it_was_verified_is_kept_with_that_draft() {
        let ready = delete_ready();
        let (removed, path) = listed_duplicate(&ready);
        let (result, aside) = apply_with_late_work(&ready, vec![drafting(&removed)]);
        let result = result.unwrap();
        assert_left_alone(&ready, &result, &aside, &removed, &path);
        assert!(
            result.not_deleted[0]
                .detail
                .as_deref()
                .unwrap_or_default()
                .contains("open metadata draft"),
            "{:?}",
            result.not_deleted[0]
        );
        assert_eq!(
            rows(
                &ready.lib,
                "SELECT COUNT(*) FROM metadata_edit_drafts WHERE state='editing'"
            ),
            1
        );
    }
    #[test]
    fn a_draft_that_lists_only_the_kept_track_does_not_keep_the_copy() {
        let ready = delete_ready();
        let (removed, path) = listed_duplicate(&ready);
        let keeper = ready.preview.deletion.as_ref().unwrap().files[0]
            .keeper_id
            .clone();
        let (result, _) = apply_with_late_work(&ready, vec![drafting(&keeper)]);
        let result = result.unwrap();
        assert_eq!(result.removed_ids, vec![removed]);
        assert!(!path.exists());
    }
    thread_local! {
        static EXCLUSIVE: std::cell::RefCell<Option<(Library, std::sync::mpsc::Receiver<()>)>> =
            const { std::cell::RefCell::new(None) };
        static EXCLUSIVE_HOLDER: std::cell::RefCell<Option<std::thread::JoinHandle<()>>> =
            const { std::cell::RefCell::new(None) };
    }
    /// A relocation or a portable restore takes the library's exclusive lease on its own thread
    /// and keeps it until released.
    fn exclusive_operation_elsewhere(phase: &str) {
        if phase == "prepared"
            && let Some((lib, release)) = EXCLUSIVE.with(|slot| slot.borrow_mut().take())
        {
            let (held, wait_for_it) = std::sync::mpsc::channel();
            let holder = std::thread::spawn(move || {
                let lease = lib.exclusive_operation().unwrap();
                held.send(()).unwrap();
                let _ = release.recv();
                drop(lease);
            });
            wait_for_it.recv().unwrap();
            EXCLUSIVE_HOLDER.with(|slot| *slot.borrow_mut() = Some(holder));
        }
    }
    #[test]
    fn an_exclusive_library_operation_stops_a_delete_merge_before_any_file_moves() {
        let ready = delete_ready();
        let (removed, path) = listed_duplicate(&ready);
        let (release, released) = std::sync::mpsc::channel();
        EXCLUSIVE.with(|slot| *slot.borrow_mut() = Some((ready.lib.clone(), released)));
        let mut service = DuplicateMergeService::with_trash(
            ready.lib.clone(),
            ready.runtime.clone(),
            Arc::new(MustNotRun),
        );
        service.hook = Some(exclusive_operation_elsewhere);
        let outcome = service.apply_confirmed(&ready.preview.id, true, &ready.token);
        drop(release);
        EXCLUSIVE_HOLDER
            .with(|slot| slot.borrow_mut().take())
            .unwrap()
            .join()
            .unwrap();
        let error = outcome.unwrap_err();
        assert!(
            error.downcast_ref::<crate::LibraryBusy>().is_some(),
            "{error:#}"
        );
        assert!(path.exists(), "no file moved");
        assert!(ready.lib.track(&removed).is_ok());
        // The reservation it had made is rolled back by the usual recovery.
        let pending = ready.lib.duplicate_recovery().unwrap();
        assert_eq!(pending.len(), 1, "{pending:?}");
        let after = ready
            .lib
            .retry_duplicate_recovery(&pending[0].id, &ready.token)
            .unwrap();
        assert!(after.is_empty(), "{after:?}");
        assert!(path.exists() && ready.lib.track(&removed).is_ok());
    }
    #[test]
    fn a_kept_file_another_operation_reserved_after_it_was_verified_keeps_the_copy() {
        let ready = delete_ready();
        let (removed, path) = listed_duplicate(&ready);
        let keeper = ready.preview.deletion.as_ref().unwrap().files[0]
            .keeper_display_path
            .clone();
        let (result, aside) =
            apply_with_late_work(&ready, vec![reserving(std::path::Path::new(&keeper))]);
        let result = result.unwrap();
        assert_left_alone(&ready, &result, &aside, &removed, &path);
        assert_eq!(
            result.not_deleted[0].detail.as_deref(),
            Some("Another operation is using the kept track's file.")
        );
    }
    /// The Trash cannot take the file, so the permanent deletion is tried. Both calls try to start
    /// work in the library from another connection while they run.
    struct ProbesTheLibrary {
        database: PathBuf,
        refused: std::sync::Mutex<Vec<bool>>,
    }
    impl ProbesTheLibrary {
        fn probe(&self) {
            let other = rusqlite::Connection::open(&self.database).unwrap();
            other.busy_timeout(std::time::Duration::ZERO).unwrap();
            let started = other.execute(
                "INSERT INTO organization_operations VALUES('probe','no-track','publishing','{}',NULL,0,0)",
                [],
            );
            self.refused.lock().unwrap().push(matches!(
                started,
                Err(rusqlite::Error::SqliteFailure(error, _))
                    if error.code == rusqlite::ErrorCode::DatabaseBusy
            ));
        }
    }
    impl FileTrash for ProbesTheLibrary {
        fn can_trash(&self, _: &std::path::Path) -> bool {
            false
        }
        fn move_to_trash(&self, _: &std::path::Path) -> std::result::Result<(), String> {
            self.probe();
            Err("The Trash refused the file".into())
        }
        fn delete_permanently(&self, path: &std::path::Path) -> std::result::Result<(), String> {
            self.probe();
            std::fs::remove_file(path).map_err(|e| e.to_string())
        }
    }
    #[test]
    fn no_other_work_can_start_between_the_last_look_and_the_trash_or_the_permanent_deletion() {
        let ready = delete_ready();
        let (removed, path) = listed_duplicate(&ready);
        let trash = Arc::new(ProbesTheLibrary {
            database: ready.dir.path().join("app/library.sqlite"),
            refused: Default::default(),
        });
        let preview = ready
            .lib
            .preview_duplicate_merge_choice_with(
                &ready.preview.review_id,
                ready.preview.review_revision,
                vec![ready.preview.plan.groups[0].group_id.clone()],
                Some(MergeDisposition::Delete),
                &*trash,
                &ready.token,
            )
            .unwrap();
        assert!(preview.deletion.as_ref().unwrap().files[0].permanent);
        let service = DuplicateMergeService::with_trash(
            ready.lib.clone(),
            ready.runtime.clone(),
            trash.clone(),
        );
        let result = service
            .apply_confirmed(&preview.id, true, &ready.token)
            .unwrap();
        assert_eq!(result.removed_ids, vec![removed.clone()], "{result:?}");
        assert_eq!(result.permanently_deleted.len(), 1);
        assert!(!path.exists());
        assert_eq!(
            *trash.refused.lock().unwrap(),
            vec![true, true],
            "the library took no new work while the Trash and the permanent deletion ran"
        );
    }
    #[test]
    fn only_the_duplicate_that_became_busy_is_kept_and_the_others_still_go_to_the_trash() {
        let ready = delete_ready_of(3);
        let listed = ready.preview.deletion.as_ref().unwrap().files.clone();
        assert_eq!(listed.len(), 2, "{listed:?}");
        let path_of = |id: &str| {
            let at = ready.ids.iter().position(|t| t == id).unwrap();
            ready.paths[at].clone()
        };
        let (busy, other) = (listed[0].track_id.clone(), listed[1].track_id.clone());
        let (result, aside) = apply_with_late_work(&ready, vec![organizing(&busy)]);
        let result = result.unwrap();
        assert_eq!(result.removed_ids, vec![other.clone()], "{result:?}");
        assert_eq!(result.trashed.len(), 1);
        assert_eq!(result.not_deleted.len(), 1);
        assert_eq!(result.not_deleted[0].track_id, busy);
        assert_eq!(result.not_deleted[0].reason, DuplicateSkipReason::Busy);
        assert!(path_of(&busy).exists() && ready.lib.track(&busy).is_ok());
        assert!(
            !path_of(&other).exists(),
            "the other copy went to the Trash"
        );
        assert!(aside.join(path_of(&other).file_name().unwrap()).exists());
        assert!(ready.lib.track(&other).is_err(), "its record was removed");
        assert_eq!(
            rows(
                &ready.lib,
                &format!("SELECT COUNT(*) FROM organization_operations WHERE track_id='{busy}'")
            ),
            1
        );
        assert!(ready.lib.duplicate_recovery().unwrap().is_empty());
    }
    #[test]
    fn recovery_preserves_a_later_user_file_and_retains_its_recovery_assets() {
        let (dir, lib, runtime, preview, path, _) = fixture();
        let mut service = DuplicateMergeService::new(lib.clone(), runtime);
        service.hook = Some(crash_files);
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                || service.apply(&preview.id, &CancellationToken::default())
            ))
            .is_err()
        );
        drop(service);
        drop(lib);
        std::fs::write(&path, b"later user version").unwrap();
        let lib = Library::open(&dir.path().join("app/library.sqlite")).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"later user version");
        let conn = lib.conn().unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT state FROM maintenance_writes ORDER BY created DESC LIMIT 1",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "conflict"
        );
        let backup: Vec<u8> = conn
            .query_row(
                "SELECT backup FROM maintenance_write_files LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(bytes_path(&backup).unwrap().is_file());
    }
}
