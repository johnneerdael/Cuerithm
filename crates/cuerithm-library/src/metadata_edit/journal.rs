use super::*;
use crate::{
    artifacts::{Artifact, owned_file},
    model::{bytes_path, now, path_bytes},
    publish_files,
};
use anyhow::{Context, Result, ensure};
use cuerithm_media::{CancellationToken, file_sha256};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
#[derive(Clone, Serialize, Deserialize)]
struct RetainedTemporary {
    path: crate::organization::NativePath,
    expected: Option<crate::organization::Fingerprint>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Write {
    id: String,
    batch: String,
    row_id: String,
    row: FrozenRow,
    publish: Vec<u8>,
    displaced: Vec<u8>,
    publication: Option<crate::organization::Fingerprint>,
    undo_of: Option<String>,
    #[serde(default)]
    override_of: Option<String>,
    #[serde(default)]
    retained_temporaries: Vec<RetainedTemporary>,
    /// What an ordinary save changed. A save that records it is undone by writing the previous
    /// values back, so it keeps no copy of the original audio once it has settled; a save from an
    /// earlier version has none and is undone from its retained original.
    #[serde(default)]
    editor_patch: Option<MetadataEditorPatch>,
    /// Original pictures the save replaced or removed, kept as artwork assets for Undo.
    #[serde(default)]
    previous_artwork: Vec<PreviousPicture>,
    /// The pictures above have been kept, so the original audio may go.
    #[serde(default)]
    artwork_kept: bool,
    /// The original audio has been deleted; nothing reads it any more.
    #[serde(default)]
    backup_retired: bool,
    /// The save left the Undo window: it stays in the history but can no longer be undone.
    #[serde(default)]
    undo_expired: bool,
}
impl Write {
    fn artifact(&self) -> Result<Artifact> {
        let stage = self.row.stage.as_ref().context("Missing staged metadata")?;
        Ok(Artifact {
            target: self.row.capture.path.path()?,
            staged: bytes_path(&stage.path)?,
            backup: Some(bytes_path(&stage.backup)?),
            before: Some(stage.before.clone()),
            after: stage.after.clone(),
            publish: bytes_path(&self.publish)?,
            displaced: bytes_path(&self.displaced)?,
        })
    }
}
impl MetadataEditService {
    pub(crate) fn verify_import_output(
        &self,
        batch: &str,
        path: &std::path::Path,
        before: &str,
    ) -> Result<()> {
        let id:String=self.library.conn()?.query_row("SELECT id FROM metadata_edit_writes WHERE batch_id=?1 AND state='committed' AND undo_of IS NULL AND override_of IS NULL",[batch],|r|r.get(0))?;
        let (_, write) = self.load_write(&id)?;
        ensure!(
            write.row.capture.path.path()? == path
                && write
                    .row
                    .stage
                    .as_ref()
                    .context("Missing tag proof")?
                    .before
                    == before,
            "Tag output does not descend from reviewed import media"
        );
        let parent = crate::organization::NativeParent::reopen(&write.row.capture.parent)?;
        let current = parent.fingerprint(std::path::Path::new(
            path.file_name().context("Missing source name")?,
        ))?;
        ensure!(
            write
                .publication
                .as_ref()
                .is_some_and(|known| crate::organization::same_native_file(&current, known)),
            "Tag publication native identity or bytes changed"
        );
        Ok(())
    }
    /// Own-operation organization Undo may restore the same tag output at a
    /// later path revision. Full active fields and native file identity must agree.
    pub(crate) fn rebase_import_undo(
        &self,
        batch: &str,
        item: &str,
        expected: &std::collections::BTreeMap<String, serde_json::Value>,
        organization_undo: &str,
    ) -> Result<()> {
        let id:String=self.library.conn()?.query_row("SELECT id FROM metadata_edit_writes WHERE batch_id=?1 AND track_id=?2 AND state='committed' AND undo_of IS NULL AND override_of IS NULL",params![batch,item],|r|r.get(0))?;
        let (_, mut write) = self.load_write(&id)?;
        let path = write.row.capture.path.path()?;
        let page = self
            .library
            .organization_batch_page(organization_undo, 0, 1)?;
        ensure!(
            page.total == 1 && page.rows[0].track_id == item && page.rows[0].outcome == "completed",
            "Import relocation Undo has no completed native proof"
        );
        let record = self.library.organization_record(
            page.rows[0]
                .operation_id
                .as_deref()
                .context("Missing native Undo record")?,
        )?;
        ensure!(
            record.phase == crate::organization::OrganizationPhase::Complete
                && record.undo_of.is_some(),
            "Native organization inverse is incomplete"
        );
        let file = record.files.first().context("Missing restored source")?;
        ensure!(
            file.destination.path()? == path,
            "Native inverse restored another path"
        );
        let parent = crate::organization::NativeParent::reopen(&file.destination_parent)?;
        let current = parent.fingerprint(std::path::Path::new(
            path.file_name().context("Missing restored source name")?,
        ))?;
        ensure!(
            crate::organization::same_native_file(
                &current,
                file.staged.as_ref().unwrap_or(&file.original)
            ) && current.sha256
                == write
                    .row
                    .stage
                    .as_ref()
                    .context("Missing original tag proof")?
                    .after,
            "Restored tag output differs from owned native inverse proof"
        );
        write.publication = Some(current);
        let mut conn = self.library.conn()?;
        let tx = conn.transaction()?;
        ensure!(
            crate::import::import_local_fields(&tx, item)? == *expected,
            "Later import edits prevent tag Undo"
        );
        let (path_revision, metadata_revision): (u64, u64) = tx.query_row(
            "SELECT path_revision,metadata_revision FROM tracks WHERE id=?1",
            [item],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        write.row.capture.public.path_revision = path_revision;
        write.row.capture.public.metadata_revision = metadata_revision
            .checked_sub(1)
            .context("Invalid tag revision")?;
        Self::validate_capture(
            &tx,
            &write.row.capture,
            Some(&write.row.stage.as_ref().context("Missing tag proof")?.after),
        )?;
        tx.execute("UPDATE metadata_edit_writes SET payload_json=?1,expected_path_revision=?2,expected_metadata_revision=?3 WHERE id=?4",params![bounded_json(&write)?,path_revision,write.row.capture.public.metadata_revision,id])?;
        tx.commit()?;
        Ok(())
    }
    pub fn apply(&self, preview: &str, cancel: &CancellationToken) -> Result<MetadataEditBatch> {
        self.apply_linked(preview, cancel, None)
    }
    pub(super) fn apply_linked(
        &self,
        preview: &str,
        cancel: &CancellationToken,
        curation: Option<&str>,
    ) -> Result<MetadataEditBatch> {
        self.apply_linked_with(preview, cancel, curation, || Ok(()))
    }
    pub(crate) fn apply_import_linked(
        &self,
        preview: &str,
        cancel: &CancellationToken,
        after_link: impl FnOnce() -> Result<()>,
    ) -> Result<MetadataEditBatch> {
        self.apply_linked_with(preview, cancel, None, after_link)
    }
    pub(super) fn apply_linked_with(
        &self,
        preview: &str,
        cancel: &CancellationToken,
        curation: Option<&str>,
        after_link: impl FnOnce() -> Result<()>,
    ) -> Result<MetadataEditBatch> {
        cancel.check()?;
        let text: String = self
            .library
            .conn()?
            .query_row(
                "SELECT payload_json FROM metadata_edit_previews WHERE id=?1",
                [preview],
                |r| r.get(0),
            )
            .optional()?
            .context("Metadata preview changed or already applied")?;
        let plan: FrozenPreview = serde_json::from_str(&text)?;
        ensure!(
            plan.public.can_apply
                || (curation.is_some()
                    && !plan.rows.is_empty()
                    && plan
                        .rows
                        .iter()
                        .all(|row| row.public.outcome == "unchanged")),
            "No eligible metadata changes"
        );
        for row in &plan.rows {
            Self::validate_capture(&*self.library.conn()?, &row.capture, None)?;
            Self::verify_source(&row.capture)?;
        }
        let public = MetadataEditBatch {
            id: uuid::Uuid::new_v4().to_string(),
            draft_id: plan.public.draft_id.clone(),
            draft_revision: plan.public.draft_revision,
            state: "saving".into(),
            rows: vec![],
            created: now(),
            can_undo: false,
            can_recover: false,
            error: None,
        };
        let json = bounded_json(&public)?;
        let overrides = plan
            .rows
            .iter()
            .map(|row| {
                if row.capture.public.state == "incoming" && row.public.outcome == "ready" {
                    Self::override_payload(row, &plan.patch).map(Some)
                } else {
                    Ok(None)
                }
            })
            .collect::<Result<Vec<_>>>()?;
        // A row that will be written keeps the change it was asked to make until its write
        // exists, so that a save interrupted in between is recovered the way it was started.
        let row_json = plan
            .rows
            .iter()
            .map(|row| {
                if row.public.outcome == "ready" && row.capture.public.state != "incoming" {
                    let mut kept = row.clone();
                    kept.patch = Some(plan.patch.clone());
                    bounded_json(&kept)
                } else {
                    bounded_json(row)
                }
            })
            .collect::<Result<Vec<_>>>()?;
        {
            let _organization = self
                .library
                .inner
                .organization_gate
                .lock()
                .map_err(|_| anyhow::anyhow!("Organization gate interrupted"))?;
            let _tag = self
                .library
                .inner
                .tag_gate
                .lock()
                .map_err(|_| anyhow::anyhow!("Publication gate interrupted"))?;
            let mut conn = self.library.conn()?;
            let tx = conn.transaction()?;
            ensure!(tx.execute("DELETE FROM metadata_edit_previews WHERE id=?1 AND state='preview' AND EXISTS(SELECT 1 FROM metadata_edit_drafts d WHERE d.id=metadata_edit_previews.draft_id AND d.revision=metadata_edit_previews.draft_revision AND d.state!='discarded')",[preview])?==1,"Metadata preview changed or already applied");
            tx.execute(
                "INSERT INTO metadata_edit_batches VALUES(?1,?2,?3,'apply','saving',?4,NULL,?5,?5)",
                params![
                    public.id,
                    public.draft_id,
                    public.draft_revision,
                    json,
                    public.created
                ],
            )?;
            if let Some(id) = curation {
                ensure!(plan.rows.len() == 1, "Curate child must own one track");
                crate::Library::link_curation_child_tx(
                    &tx,
                    id,
                    Some(preview),
                    &public.id,
                    &plan.rows[0].public.track_id,
                )?;
            }
            for (ordinal, row) in plan.rows.iter().enumerate() {
                let row_id = uuid::Uuid::new_v4().to_string();
                tx.execute("INSERT INTO metadata_edit_rows(id,batch_id,track_id,ordinal,state,payload_json) VALUES(?1,?2,?3,?4,?5,?6)",params![row_id,public.id,row.public.track_id,ordinal,row.public.outcome,row_json[ordinal]])?;
                if let Some((payload, proof)) = &overrides[ordinal] {
                    Self::queue_override_tx(&tx, &public.id, &row_id, row, payload, proof)?;
                }
            }
            tx.commit()?;
        }
        after_link()?;
        let saved = self.run_batch(&public.id, Some(&plan.patch), cancel)?;
        self.library.prune_metadata_undo()?;
        let _ = self.library.retire_incoming_originals();
        Ok(saved)
    }
    fn run_batch(
        &self,
        batch: &str,
        patch: Option<&MetadataEditorPatch>,
        cancel: &CancellationToken,
    ) -> Result<MetadataEditBatch> {
        let rows = self.load_rows(batch)?;
        for (id, mut row, state) in rows {
            if !matches!(state.as_str(), "ready" | "recovery_required") {
                continue;
            }
            if cancel.is_cancelled() {
                let pending:bool=self.library.conn()?.query_row("SELECT EXISTS(SELECT 1 FROM metadata_edit_writes WHERE row_id=?1 AND state IN ('prepared','publishing','recovery_required','conflict'))",[&id],|r|r.get(0))?;
                self.row_state(
                    &id,
                    if pending {
                        "recovery_required"
                    } else {
                        "cancelled"
                    },
                    Some("Metadata save cancelled"),
                    None,
                )?;
                continue;
            }
            let outcome = (|| -> Result<()> {
                if row.undo.is_none() && row.capture.public.state == "incoming" {
                    anyhow::bail!("Incoming override outbox is missing; create a fresh preview");
                }
                let existing:Option<String>=self.library.conn()?.query_row("SELECT id FROM metadata_edit_writes WHERE row_id=?1 ORDER BY created DESC LIMIT 1",[&id],|r|r.get(0)).optional()?;
                let write = if let Some(write) = existing {
                    write
                } else {
                    let (undo_of, override_of) = match &row.undo {
                        Some(UndoLink::Write(original)) => (Some(original.clone()), None),
                        Some(UndoLink::Override(job)) => (None, Some(job.clone())),
                        None => (None, None),
                    };
                    self.begin_write_owned(batch, &id, &row, undo_of, override_of, patch)?
                };
                self.publish_write(&write, cancel)?;
                row.public.outcome = "written".into();
                Ok(())
            })();
            if let Err(error) = outcome {
                let pending:bool=self.library.conn()?.query_row("SELECT EXISTS(SELECT 1 FROM metadata_edit_writes WHERE row_id=?1 AND state IN ('prepared','publishing','recovery_required'))",[&id],|r|r.get(0))?;
                self.row_state(
                    &id,
                    if pending {
                        "recovery_required"
                    } else if cancel.is_cancelled() {
                        "cancelled"
                    } else {
                        "failed"
                    },
                    Some(&format!("{error:#}")),
                    None,
                )?;
            }
        }
        self.finish_batch(batch)
    }
    fn load_rows(&self, batch: &str) -> Result<Vec<(String, FrozenRow, String)>> {
        let rows = {
            let conn = self.library.conn()?;
            let mut q=conn.prepare("SELECT id,payload_json,state FROM metadata_edit_rows WHERE batch_id=?1 ORDER BY ordinal")?;
            q.query_map([batch], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
        };
        rows.into_iter()
            .map(|(id, json, state)| Ok((id, serde_json::from_str(&json)?, state)))
            .collect()
    }
    pub(super) fn row_state(
        &self,
        id: &str,
        state: &str,
        error: Option<&str>,
        result: Option<&str>,
    ) -> Result<()> {
        self.library.conn()?.execute("UPDATE metadata_edit_rows SET state=?1,error=?2,result_json=COALESCE(?3,result_json) WHERE id=?4",params![state,error,result,id])?;
        Ok(())
    }
    pub(super) fn begin_write_owned(
        &self,
        batch: &str,
        row_id: &str,
        row: &FrozenRow,
        undo_of: Option<String>,
        override_of: Option<String>,
        patch: Option<&MetadataEditorPatch>,
    ) -> Result<String> {
        // A save recovered before its write existed has no patch from its caller: the change it
        // was asked to make is the one its row kept.
        let patch = patch.or(row.patch.as_ref());
        let stage = row
            .stage
            .as_ref()
            .context("Missing metadata staging evidence")?;
        super::previews::verify_stage(&self.library, stage)?;
        let workspace = bytes_path(&stage.workspace)?.canonicalize()?;
        ensure!(
            workspace.starts_with(self.library.data_dir().canonicalize()?),
            "Metadata staging leaves application data"
        );
        owned_file(&bytes_path(&stage.path)?, &workspace, &stage.after)?;
        owned_file(&bytes_path(&stage.backup)?, &workspace, &stage.before)?;
        let id = uuid::Uuid::new_v4().to_string();
        let parent = row
            .capture
            .path
            .path()?
            .parent()
            .context("Missing metadata parent")?
            .to_path_buf();
        let publish = parent.join(format!(".cuerithm-edit-{id}.tmp"));
        let displaced = if cfg!(windows) {
            parent.join(format!(".cuerithm-edit-{id}-before.tmp"))
        } else {
            publish.clone()
        };
        let write = Write {
            id: id.clone(),
            batch: batch.into(),
            row_id: row_id.into(),
            row: FrozenRow {
                patch: None,
                ..row.clone()
            },
            publish: path_bytes(&publish),
            displaced: path_bytes(&displaced),
            publication: None,
            editor_patch: patch
                .filter(|_| undo_of.is_none() && override_of.is_none())
                .cloned(),
            previous_artwork: vec![],
            artwork_kept: false,
            backup_retired: false,
            undo_expired: false,
            undo_of,
            override_of,
            retained_temporaries: vec![],
        };
        let payload = bounded_json(&write)?;
        publish_files::durable_assets(
            &[bytes_path(&stage.path)?, bytes_path(&stage.backup)?],
            &workspace,
            &self.library.data_dir().canonicalize()?,
        )?;
        let mut conn = self.library.conn()?;
        let tx = conn.transaction()?;
        Self::validate_capture(&tx, &row.capture, None)?;
        ensure_unreserved(&tx, &row.capture.path.0, None)?;
        if let Some(job) = &write.override_of {
            ensure!(tx.execute("UPDATE metadata_edit_overrides SET undo_batch_id=?1 WHERE job_id=?2 AND state='committed' AND undo_batch_id IS NULL",params![batch,job])?==1,"Incoming metadata edit was already undone or reserved");
        }
        tx.execute(
            "INSERT INTO metadata_edit_writes(id,batch_id,row_id,track_id,state,payload_json,workspace,error,undo_of,override_of,expected_path_revision,expected_metadata_revision,output_hash,created,updated) VALUES(?1,?2,?3,?4,'prepared',?5,?6,NULL,?8,?9,?10,?11,?12,?7,?7)",
            params![
                id,
                batch,
                row_id,
                row.public.track_id,
                payload,
                stage.workspace,
                now(),write.undo_of,write.override_of,row.capture.public.path_revision,row.capture.public.metadata_revision,stage.after
            ],
        )?;
        // The write holds the change now; the row does not need its own copy.
        tx.execute(
            "UPDATE metadata_edit_rows SET payload_json=json_remove(payload_json,'$.patch') WHERE id=?1",
            [row_id],
        )?;
        let f = write.artifact()?;
        tx.execute(
            "INSERT INTO metadata_edit_files VALUES(?1,0,?2,?3,?4,?5,?6,?7,?8)",
            params![
                id,
                path_bytes(&f.target),
                path_bytes(&f.staged),
                f.backup.as_ref().map(|p| path_bytes(p)),
                f.before,
                f.after,
                path_bytes(&f.publish),
                path_bytes(&f.displaced)
            ],
        )?;
        tx.commit()?;
        Ok(id)
    }
    fn load_write(&self, id: &str) -> Result<(String, Write)> {
        let (state, json): (String, String) = self.library.conn()?.query_row(
            "SELECT state,payload_json FROM metadata_edit_writes WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        Ok((state, serde_json::from_str(&json)?))
    }
    fn save_write(&self, write: &Write, state: &str, error: Option<&str>) -> Result<()> {
        let json = bounded_json(write)?;
        let mut conn = self.library.conn()?;
        let tx = conn.transaction()?;
        tx.execute("UPDATE metadata_edit_writes SET state=?1,payload_json=?2,error=?3,updated=?4 WHERE id=?5",params![state,json,error,now(),write.id])?;
        tx.execute(
            "UPDATE metadata_edit_files SET publish_path=?1,displaced_path=?2 WHERE write_id=?3",
            params![write.publish, write.displaced, write.id],
        )?;
        tx.commit()?;
        Ok(())
    }
    fn prepare_publication_copy(
        &self,
        write: &mut Write,
        parent: &crate::organization::NativeParent,
        cancel: &CancellationToken,
    ) -> Result<Artifact> {
        let mut artifact = write.artifact()?;
        let name = std::path::Path::new(
            artifact
                .publish
                .file_name()
                .context("Missing publication name")?,
        );
        let current = parent.optional(name);
        let reusable = current
            .as_ref()
            .ok()
            .and_then(|v| v.as_ref())
            .is_some_and(|f| {
                write.publication.as_ref().is_some_and(|expected| {
                    expected.identity == f.identity
                        && expected.sha256 == f.sha256
                        && f.sha256 == artifact.after
                })
            });
        if !matches!(current, Ok(None)) && !reusable {
            for (path, expected) in [
                (write.publish.clone(), write.publication.clone()),
                (write.displaced.clone(), None),
            ] {
                if !write.retained_temporaries.iter().any(|r| r.path.0 == path) {
                    write.retained_temporaries.push(RetainedTemporary {
                        path: crate::organization::NativePath(path),
                        expected,
                    });
                }
            }
            let folder = artifact
                .target
                .parent()
                .context("Missing publication folder")?;
            let nonce = uuid::Uuid::new_v4();
            let publish = folder.join(format!(".cuerithm-edit-{}-{nonce}.tmp", write.id));
            let displaced = if cfg!(windows) {
                folder.join(format!(".cuerithm-edit-{}-{nonce}-before.tmp", write.id))
            } else {
                publish.clone()
            };
            write.publish = path_bytes(&publish);
            write.displaced = path_bytes(&displaced);
            write.publication = None;
            self.save_write(write, "prepared", None)?;
            artifact = write.artifact()?;
        }
        let leaf = std::path::Path::new(
            artifact
                .publish
                .file_name()
                .context("Missing publication name")?,
        );
        let fingerprint = if reusable {
            parent.fingerprint(leaf)?
        } else {
            parent.copy_prepared(&artifact.staged, leaf, &artifact.after, cancel)?
        };
        ensure!(
            fingerprint.sha256 == artifact.after,
            "Metadata publication copy changed"
        );
        write.publication = Some(fingerprint);
        self.save_write(write, "publishing", None)?;
        Ok(artifact)
    }
    pub(super) fn publish_write(&self, id: &str, cancel: &CancellationToken) -> Result<()> {
        self.publish_write_with(id, cancel, || Ok(()))
    }
    fn publish_write_with(
        &self,
        id: &str,
        cancel: &CancellationToken,
        after_replace: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        self.publish_write_with_cleanup(id, cancel, after_replace, |_| Ok(()))
    }
    fn publish_write_with_cleanup(
        &self,
        id: &str,
        cancel: &CancellationToken,
        after_replace: impl FnOnce() -> Result<()>,
        mut cleanup_boundary: impl FnMut(crate::artifact_cleanup::CleanupBoundary) -> Result<()>,
    ) -> Result<()> {
        let _organization = self
            .library
            .inner
            .organization_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Organization gate interrupted"))?;
        let _tag = self
            .library
            .inner
            .tag_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Publication gate interrupted"))?;
        let (state, mut write) = self.load_write(id)?;
        if matches!(state.as_str(), "committed" | "undone") {
            let _ = self
                .library
                .cleanup_metadata_write_with(id, &mut cleanup_boundary);
            return Ok(());
        }
        let mut artifact = write.artifact()?;
        let stage = write
            .row
            .stage
            .as_ref()
            .context("Missing staged bytes")?
            .clone();
        super::previews::verify_stage(&self.library, &stage)?;
        let workspace = bytes_path(&stage.workspace)?;
        owned_file(&artifact.staged, &workspace, &artifact.after)?;
        owned_file(
            artifact
                .backup
                .as_ref()
                .context("Missing original backup")?,
            &workspace,
            artifact
                .before
                .as_deref()
                .context("Missing original hash")?,
        )?;
        let result = (|| -> Result<()> {
            cancel.check()?;
            let authority = crate::organization::NativeParent::reopen(&write.row.capture.parent)?;
            let target_leaf = std::path::PathBuf::from(
                artifact
                    .target
                    .file_name()
                    .context("Missing metadata target name")?,
            );
            Self::validate_capture(&*self.library.conn()?, &write.row.capture, None)?;
            ensure_unreserved(&*self.library.conn()?, &write.row.capture.path.0, Some(id))?;
            let (parent, actual) = crate::organization::capture_regular_source(&artifact.target)?;
            ensure!(
                parent.ancestors == write.row.capture.parent.ancestors,
                "Metadata parent authority changed"
            );
            let already = write.publication.as_ref().is_some_and(|expected| {
                expected.identity == actual.identity && expected.sha256 == actual.sha256
            });
            if !already {
                Self::verify_source(&write.row.capture)?;
                ensure!(
                    !std::fs::metadata(&artifact.target)?
                        .permissions()
                        .readonly(),
                    "Metadata source is read-only"
                );
                artifact = self.prepare_publication_copy(&mut write, &authority, cancel)?;
                cancel.check()?;
                Self::verify_source(&write.row.capture)?;
                authority.exchange(
                    std::path::Path::new(
                        artifact
                            .publish
                            .file_name()
                            .context("Missing publication name")?,
                    ),
                    &target_leaf,
                    std::path::Path::new(
                        artifact
                            .displaced
                            .file_name()
                            .context("Missing displaced name")?,
                    ),
                )?;
            }
            let displaced = authority.fingerprint(std::path::Path::new(
                artifact
                    .displaced
                    .file_name()
                    .context("Missing displaced name")?,
            ))?;
            ensure!(
                displaced.identity == write.row.capture.fingerprint.identity
                    && displaced.sha256 == write.row.capture.fingerprint.sha256,
                "Source native identity or bytes changed during metadata replacement; preserve both versions"
            );
            let published = authority.fingerprint(&target_leaf)?;
            ensure!(
                write.publication.as_ref().is_some_and(
                    |p| p.identity == published.identity && p.sha256 == published.sha256
                ) && published.sha256 == artifact.after,
                "Published metadata changed; preserve recovery bytes"
            );
            after_replace()?;
            let info_json = bounded_json(&stage.info)?;
            let native_json = bounded_json(&stage.native)?;
            let result_json = bounded_json(&write.row.public.after)?;
            let super::capture::Display {
                title,
                artist,
                album,
                genre,
            } = stage.display.clone();
            let mut conn = self.library.conn()?;
            let tx = conn.transaction()?;
            Self::validate_capture(&tx, &write.row.capture, None)?;
            ensure_unreserved(&tx, &write.row.capture.path.0, Some(id))?;
            tx.execute("UPDATE tracks SET title=?1,artist=?2,album=?3,genre=?4,source_hash=?5,metadata_json=?6,native_metadata_json=?7,search_text=?8,sort_title=?9,sort_artist=?10,updated=?11 WHERE id=?12",params![title,artist,album,genre,artifact.after,info_json,native_json,format!("{title} {artist} {genre}").to_lowercase(),title.to_lowercase(),artist.to_lowercase(),now(),write.row.public.track_id])?;
            tx.execute("UPDATE metadata_edit_writes SET state='committed',error='Metadata temporary cleanup pending',updated=?1 WHERE id=?2",params![now(),id])?;
            tx.execute(
                "UPDATE metadata_edit_rows SET state=?1,error=NULL,result_json=?2 WHERE id=?3",
                params![
                    if write.undo_of.is_some() || write.override_of.is_some() {
                        "undone"
                    } else {
                        "written"
                    },
                    result_json,
                    write.row_id
                ],
            )?;
            crate::Library::ack_curation_child_tx(&tx, &write.batch, &write.row.public.track_id)?;
            if let Some(original) = &write.undo_of {
                ensure!(tx.execute("UPDATE metadata_edit_writes SET state='undone',updated=?1 WHERE id=?2 AND state='committed'",params![now(),original])?==1,"Metadata save was already undone");
            }
            if let Some(job) = &write.override_of {
                ensure!(tx.execute("UPDATE metadata_edit_overrides SET state='undone',updated=?1 WHERE job_id=?2 AND state='committed' AND undo_batch_id=?3",params![now(),job,write.batch])?==1,"Incoming metadata Undo ownership changed");
            }
            crate::Library::bump(&tx)?;
            tx.commit()?;
            Ok(())
        })();
        if let Err(error) = result {
            let message = format!("{error:#}");
            let pending = write.publication.is_some();
            if !pending {
                self.save_write(&write, "failed", Some(&message))?;
            } else {
                self.save_write(&write, "recovery_required", Some(&message))?;
            }
            return Err(error);
        }
        let _ = self
            .library
            .cleanup_metadata_write_with(id, &mut cleanup_boundary);
        Ok(())
    }
    pub fn recover(&self, batch: &str, cancel: &CancellationToken) -> Result<MetadataEditBatch> {
        cancel.check()?;
        let state: String = self.library.conn()?.query_row(
            "SELECT state FROM metadata_edit_batches WHERE id=?1",
            [batch],
            |r| r.get(0),
        )?;
        ensure!(state != "saving", "Metadata save is already running");
        ensure!(self.library.conn()?.execute("UPDATE metadata_edit_batches SET state='saving',updated=?1 WHERE id=?2 AND state!='saving'",params![now(),batch])?==1,"Metadata recovery is already running");
        {
            let _organization = self
                .library
                .inner
                .organization_gate
                .lock()
                .map_err(|_| anyhow::anyhow!("Organization gate interrupted"))?;
            let _tag = self
                .library
                .inner
                .tag_gate
                .lock()
                .map_err(|_| anyhow::anyhow!("Publication gate interrupted"))?;
            let ids = {
                let conn = self.library.conn()?;
                let mut query=conn.prepare("SELECT id FROM metadata_edit_writes WHERE batch_id=?1 AND state IN ('committed','undone') AND error IS NOT NULL")?;
                query
                    .query_map([batch], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            };
            for id in ids {
                cancel.check()?;
                let _ = self.library.cleanup_metadata_write(&id);
            }
        }
        self.run_batch(batch, None, cancel)
    }
    fn finish_batch(&self, id: &str) -> Result<MetadataEditBatch> {
        let mut batch = self.batch(id)?;
        batch.state = if batch.rows.iter().any(|r| r.outcome == "recovery_required") {
            "recovery_required"
        } else if batch.rows.iter().any(|r| r.outcome == "queued") {
            "queued"
        } else if batch
            .rows
            .iter()
            .any(|r| matches!(r.outcome.as_str(), "failed" | "cancelled"))
        {
            "partial"
        } else if batch.rows.iter().any(|r| r.outcome == "undone") {
            "undone"
        } else {
            "saved"
        }
        .into();
        let mut header = batch.clone();
        header.rows.clear();
        let json = bounded_json(&header)?;
        let mut conn = self.library.conn()?;
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE metadata_edit_batches SET state=?1,payload_json=?2,updated=?3 WHERE id=?4",
            params![batch.state, json, now(), id],
        )?;
        tx.execute(
            "UPDATE metadata_edit_drafts SET state=?1,updated=?2 WHERE id=?3 AND revision=?4",
            params![batch.state, now(), batch.draft_id, batch.draft_revision],
        )?;
        tx.commit()?;
        drop(conn);
        self.library.retire_unwritten_metadata_stages(id)?;
        self.library.retire_superseded_metadata_stages()?;
        self.batch(id)
    }
    pub fn history_page(&self, offset: usize, limit: usize) -> Result<MetadataEditHistoryPage> {
        ensure!(
            (1..=200).contains(&limit) && offset <= i64::MAX as usize,
            "History page requires limit1..200 and a valid offset"
        );
        let (total, headers) = {
            let conn = self.library.conn()?;
            let total: usize =
                conn.query_row("SELECT COUNT(*) FROM metadata_edit_batches", [], |r| {
                    r.get(0)
                })?;
            let mut q=conn.prepare("SELECT id,draft_id,draft_revision,state,created FROM metadata_edit_batches ORDER BY created DESC,rowid DESC LIMIT ?1 OFFSET ?2")?;
            let headers = q
                .query_map(params![limit, offset], |r| {
                    Ok(MetadataEditSummary {
                        id: r.get(0)?,
                        draft_id: r.get(1)?,
                        draft_revision: r.get(2)?,
                        state: r.get(3)?,
                        created: r.get(4)?,
                        can_undo: false,
                        can_recover: false,
                        error: None,
                        counts: Default::default(),
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            (total, headers)
        };
        let mut batches = vec![];
        for mut header in headers {
            let conn = self.library.conn()?;
            let mut q = conn.prepare(
                "SELECT state,COUNT(*) FROM metadata_edit_rows WHERE batch_id=?1 GROUP BY state",
            )?;
            header.counts = q
                .query_map([&header.id], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, u64>(1)?))
                })?
                .collect::<rusqlite::Result<_>>()?;
            header.error = current_batch_error(&conn, &header.id)?;
            let flags = eligibility(&conn, &header.id, &header.state)?;
            header.can_undo = flags.0;
            header.can_recover = flags.1;
            batches.push(header);
        }
        let page = MetadataEditHistoryPage {
            total,
            offset,
            limit,
            batches,
        };
        bounded_json(&page)?;
        Ok(page)
    }
    /// Compatibility wrapper; large durable histories use SQL-paged summaries.
    pub fn history(&self) -> Result<Vec<MetadataEditBatch>> {
        let ids = {
            let conn = self.library.conn()?;
            let bytes:u64=conn.query_row("SELECT (SELECT COALESCE(SUM(LENGTH(payload_json)+COALESCE(LENGTH(result_json),0)),0) FROM metadata_edit_rows)+(SELECT COALESCE(SUM(LENGTH(id)+LENGTH(payload_json)),0) FROM metadata_edit_batches)",[],|r|r.get(0))?;
            ensure!(
                bytes <= MAX_METADATA_JSON as u64,
                "Complete history exceeds64MiB; use paged history"
            );
            let mut q = conn
                .prepare("SELECT id FROM metadata_edit_batches ORDER BY created DESC,rowid DESC")?;
            q.query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let mut result = vec![];
        let mut bytes = 0usize;
        for id in ids {
            let batch = self.batch(&id)?;
            bytes = bytes.saturating_add(bounded_json(&batch)?.len());
            ensure!(
                bytes <= MAX_METADATA_JSON,
                "Complete history exceeds64MiB; use paged history"
            );
            result.push(batch);
        }
        Ok(result)
    }
    pub fn batch(&self, id: &str) -> Result<MetadataEditBatch> {
        {
            let bytes:u64=self.library.conn()?.query_row("SELECT COALESCE(SUM(LENGTH(payload_json)+COALESCE(LENGTH(result_json),0)),0) FROM metadata_edit_rows WHERE batch_id=?1",[id],|r|r.get(0))?;
            ensure!(
                bytes <= 2 * MAX_METADATA_JSON as u64,
                "Metadata batch detail exceeds bounded read budget"
            );
        }
        let (json, state): (String, String) = self.library.conn()?.query_row(
            "SELECT payload_json,state FROM metadata_edit_batches WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let mut batch: MetadataEditBatch = serde_json::from_str(&json)?;
        batch.state = state;
        batch.rows.clear();
        let rows = {
            let conn = self.library.conn()?;
            let mut q=conn.prepare("SELECT payload_json,state,error,result_json FROM metadata_edit_rows WHERE batch_id=?1 ORDER BY ordinal")?;
            q.query_map([id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
        };
        for (json, state, error, result) in rows {
            let frozen: FrozenRow = serde_json::from_str(&json)?;
            let mut row = frozen.public;
            row.outcome = state;
            row.error = error;
            if let Some(result) = result {
                row.after =
                    serde_json::from_str::<cuerithm_autotag::MetadataEditorReceipt>(&result)
                        .map(|r| r.after)
                        .or_else(|_| serde_json::from_str(&result))?;
            }
            for field in &mut row.fields {
                field.status = row.outcome.clone();
                field.reason = row.error.clone().or_else(|| field.reason.take());
            }
            batch.rows.push(row);
        }
        let conn = self.library.conn()?;
        batch.error = current_batch_error(&conn, id)?;
        let flags = eligibility(&conn, id, &batch.state)?;
        batch.can_undo = flags.0;
        batch.can_recover = flags.1;
        drop(conn);
        bounded_json(&batch)?;
        Ok(batch)
    }
    pub fn undo(&self, batch: &str, cancel: &CancellationToken) -> Result<MetadataEditBatch> {
        self.undo_linked(batch, cancel, None)
    }
    pub(super) fn undo_linked(
        &self,
        batch: &str,
        cancel: &CancellationToken,
        curation: Option<&str>,
    ) -> Result<MetadataEditBatch> {
        self.undo_linked_with(batch, cancel, curation, || Ok(()))
    }
    pub(crate) fn undo_linked_with(
        &self,
        batch: &str,
        cancel: &CancellationToken,
        curation: Option<&str>,
        after_link: impl FnOnce() -> Result<()>,
    ) -> Result<MetadataEditBatch> {
        let undo = self.journal_undo_linked(batch, cancel, curation)?;
        after_link()?;
        self.run_batch(&undo, None, cancel)
    }
    /// Stages the Undo of a save that recorded what it changed: the previous values, written into
    /// the file as it is now through the same checked write as the save, with the original
    /// pictures it replaced or removed read from the artwork the save kept. No copy of the
    /// original audio is involved.
    fn value_undo_row(
        &self,
        id: &str,
        old: &Write,
        cancel: &CancellationToken,
    ) -> Result<FrozenRow> {
        let patch = old
            .editor_patch
            .as_ref()
            .context("Missing the recorded metadata change")?;
        let old_stage = old.row.stage.as_ref().context("Missing prior staging")?;
        Self::validate_capture(
            &*self.library.conn()?,
            &old.row.capture,
            Some(&old_stage.after),
        )?;
        let capture = self.capture(&old.row.public.track_id, cancel)?;
        ensure!(
            capture.parent.ancestors == old.row.capture.parent.ancestors
                && old.publication.as_ref().is_some_and(|known| {
                    crate::organization::same_native_file(&capture.fingerprint, known)
                }),
            "Metadata Undo source native identity or bytes changed"
        );
        let receipt = cuerithm_autotag::MetadataEditorReceipt {
            before: old.row.public.before.clone(),
            after: old.row.public.after.clone(),
        };
        ensure!(
            capture.public.native == receipt.after,
            "Metadata changed after its saved revision"
        );
        let native = self.native_patch(patch)?;
        let inverse = self.inverse_patch(&native, &receipt, &mut |ordinal, identity| {
            if let Some(kept) = old.previous_artwork.iter().find(|p| p.ordinal == ordinal) {
                return self.library.metadata_artwork_content(&kept.asset_id);
            }
            // Not retired yet: the original file is still there.
            ensure!(
                !old.backup_retired,
                "The original artwork of this save is no longer available"
            );
            super::previews::verify_backup(&self.library, old_stage)?;
            cuerithm_autotag::metadata_artwork_bytes(
                &bytes_path(&old_stage.backup)?,
                &old_stage.info,
                identity,
            )
        })?;
        let (mut stage, after) = self.stage_row(&capture, &inverse, cancel)?;
        // Undo returns the track to the display values it had before the save.
        stage.display = old.row.capture.display.clone();
        Ok(FrozenRow {
            public: MetadataEditRow {
                track_id: old.row.public.track_id.clone(),
                title: old.row.public.title.clone(),
                before: capture.public.native.clone(),
                after,
                outcome: "ready".into(),
                fields: old.row.public.fields.clone(),
                error: None,
            },
            capture,
            stage: Some(stage),
            undo: Some(UndoLink::Write(id.to_string())),
            kept_pictures: None,
            original_retired: false,
            patch: None,
        })
    }
    #[cfg(test)]
    fn journal_undo(&self, batch: &str, cancel: &CancellationToken) -> Result<String> {
        self.journal_undo_linked(batch, cancel, None)
    }
    /// Stage every inverse row, then commit the Undo batch with all of its rows at once.
    ///
    /// Nothing is restored until this returns, so a crash can never leave recovery with only some of
    /// the rows and a batch that would finish as undone while other tracks were never reverted.
    fn journal_undo_linked(
        &self,
        batch: &str,
        cancel: &CancellationToken,
        curation: Option<&str>,
    ) -> Result<String> {
        cancel.check()?;
        let mut original = self.batch(batch)?;
        if curation.is_some() {
            original.can_undo =
                curation_eligibility(&*self.library.conn()?, batch, &original.state)?;
        }
        ensure!(
            original.can_undo,
            "No completed original metadata saves can be undone"
        );
        let ids = {
            let conn = self.library.conn()?;
            let mut q=conn.prepare("SELECT id FROM metadata_edit_writes WHERE batch_id=?1 AND state='committed' AND undo_of IS NULL AND override_of IS NULL AND COALESCE(json_extract(payload_json,'$.undo_expired'),0)=0 ORDER BY created,id")?;
            q.query_map([batch], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let undo = MetadataEditBatch {
            id: uuid::Uuid::new_v4().to_string(),
            draft_id: original.draft_id,
            draft_revision: original.draft_revision,
            state: "saving".into(),
            rows: vec![],
            created: now(),
            can_undo: false,
            can_recover: false,
            error: None,
        };
        let mut journaled = vec![];
        for (ordinal, id) in ids.iter().enumerate() {
            let row_id = uuid::Uuid::new_v4().to_string();
            let (_, old) = self.load_write(id)?;
            let result = (|| -> Result<FrozenRow> {
                cancel.check()?;
                if old.editor_patch.is_some() {
                    return self.value_undo_row(id, &old, cancel);
                }
                Self::validate_capture(
                    &*self.library.conn()?,
                    &old.row.capture,
                    Some(
                        &old.row
                            .stage
                            .as_ref()
                            .context("Missing prior staging")?
                            .after,
                    ),
                )?;
                let capture = self.capture(&old.row.public.track_id, cancel)?;
                ensure!(
                    capture.parent.ancestors == old.row.capture.parent.ancestors
                        && old.publication.as_ref().is_some_and(|known| {
                            crate::organization::same_native_file(&capture.fingerprint, known)
                        }),
                    "Metadata Undo source native identity or bytes changed"
                );
                let old_stage = old.row.stage.as_ref().context("Missing original backup")?;
                super::previews::verify_backup(&self.library, old_stage)?;
                owned_file(
                    &bytes_path(&old_stage.backup)?,
                    &bytes_path(&old_stage.workspace)?,
                    &old_stage.before,
                )?;
                let folder = self.library.data_dir().join("metadata-edits/staging");
                let directory = tempfile::Builder::new();
                #[cfg(unix)]
                let mut directory = directory;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    directory.permissions(std::fs::Permissions::from_mode(0o700));
                }
                let dir = directory.tempdir_in(&folder)?;
                let staged = dir.path().join("restore.audio");
                let backup = dir.path().join("current.audio");
                std::fs::copy(bytes_path(&old_stage.backup)?, &staged)?;
                std::fs::copy(capture.path.path()?, &backup)?;
                ensure!(
                    file_sha256(&backup)? == capture.fingerprint.sha256,
                    "Source changed during Undo"
                );
                let info = self.runtime.probe_cancellable(&staged, cancel)?;
                let native = cuerithm_autotag::read_metadata(&staged, &info)?;
                let stage = StoredStage {
                    path: path_bytes(&staged),
                    backup: path_bytes(&backup),
                    workspace: path_bytes(dir.path()),
                    before: capture.fingerprint.sha256.clone(),
                    after: old_stage.before.clone(),
                    info,
                    native,
                    display: old.row.capture.display.clone(),
                    workspace_authority: crate::organization::NativeParent::capture(dir.path())?.1,
                    staged_fingerprint: crate::organization::capture_regular_source(&staged)?.1,
                    backup_fingerprint: crate::organization::capture_regular_source(&backup)?.1,
                };
                let _ = dir.keep();
                Ok(FrozenRow {
                    public: MetadataEditRow {
                        track_id: old.row.public.track_id.clone(),
                        title: old.row.public.title.clone(),
                        before: old.row.public.after.clone(),
                        after: old.row.public.before.clone(),
                        outcome: "ready".into(),
                        fields: old.row.public.fields.clone(),
                        error: None,
                    },
                    capture,
                    stage: Some(stage),
                    undo: Some(UndoLink::Write(id.clone())),
                    kept_pictures: None,
                    original_retired: false,
                    patch: None,
                })
            })();
            match result {
                Ok(row) => journaled.push(JournaledRow {
                    id: row_id,
                    ordinal,
                    track_id: row.public.track_id.clone(),
                    state: "ready".into(),
                    payload: bounded_json(&row)?,
                    error: None,
                }),
                Err(error) => {
                    let mut row = old.row;
                    row.stage = None;
                    row.public.outcome = "failed".into();
                    row.public.error = Some(format!("{error:#}"));
                    journaled.push(JournaledRow {
                        id: row_id,
                        ordinal,
                        track_id: row.public.track_id.clone(),
                        state: "failed".into(),
                        payload: bounded_json(&row)?,
                        error: row.public.error.clone(),
                    });
                }
            }
        }
        journaled.extend(self.stage_incoming_undo(batch, ids.len(), cancel)?);
        ensure!(
            !journaled.is_empty(),
            "Metadata Undo has no owned inverse rows"
        );
        let bytes = journaled
            .iter()
            .fold(0usize, |bytes, row| bytes.saturating_add(row.payload.len()));
        ensure!(
            bytes <= MAX_METADATA_JSON,
            "Complete Undo exceeds64MiB; narrow the saved batch"
        );
        let json = bounded_json(&undo)?;
        let mut conn = self.library.conn()?;
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO metadata_edit_batches VALUES(?1,?2,?3,'undo','saving',?4,?5,?6,?6)",
            params![
                undo.id,
                undo.draft_id,
                undo.draft_revision,
                json,
                batch,
                undo.created
            ],
        )?;
        for row in &journaled {
            tx.execute("INSERT INTO metadata_edit_rows(id,batch_id,track_id,ordinal,state,payload_json,error) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![row.id,undo.id,row.track_id,row.ordinal,row.state,row.payload,row.error])?;
        }
        if let Some(id) = curation {
            crate::Library::link_curation_child_tx(
                &tx,
                id,
                None,
                &undo.id,
                &journaled[0].track_id,
            )?;
        }
        tx.commit()?;
        Ok(undo.id)
    }
}
/// One Undo row staged and ready to be committed with its batch.
pub(super) struct JournaledRow {
    pub id: String,
    pub ordinal: usize,
    pub track_id: String,
    pub state: String,
    pub payload: String,
    pub error: Option<String>,
}
fn ensure_unreserved(
    conn: &rusqlite::Connection,
    path: &[u8],
    exclude: Option<&str>,
) -> Result<()> {
    crate::maintenance::ensure_unused_unreserved(conn, &[path.to_vec()])?;
    ensure!(
        !conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM organization_reservations WHERE path=?1)",
            [path],
            |r| r.get::<_, bool>(0)
        )?,
        "Organization owns this path; finish its recovery first"
    );
    let busy:i64=conn.query_row("SELECT (SELECT COUNT(*) FROM tag_write_files f JOIN tag_writes w ON w.id=f.write_id WHERE f.target=?1 AND w.state='prepared')+(SELECT COUNT(*) FROM cleanup_write_files f JOIN cleanup_writes w ON w.id=f.write_id WHERE f.target=?1 AND w.state='prepared')+(SELECT COUNT(*) FROM maintenance_write_files f JOIN maintenance_writes w ON w.id=f.write_id WHERE f.target=?1 AND w.state IN ('prepared','conflict'))",[path],|r|r.get(0))?;
    ensure!(
        busy == 0,
        "Another publication owns this metadata destination"
    );
    ensure!(!conn.query_row("SELECT EXISTS(SELECT 1 FROM metadata_edit_files f JOIN metadata_edit_writes w ON w.id=f.write_id WHERE f.target=?1 AND w.state IN ('prepared','publishing','recovery_required','conflict') AND (?2 IS NULL OR w.id!=?2))",params![path,exclude],|r|r.get::<_,bool>(0))?,"Another metadata edit owns this source; recover it first");
    Ok(())
}
impl crate::Library {
    pub(crate) fn recover_metadata_edits(&self) -> Result<()> {
        self.conn()?.execute(
            "UPDATE metadata_edit_previews SET state='stale' WHERE state='preview'",
            [],
        )?;
        self.retire_metadata_previews()?;
        self.retire_superseded_metadata_stages()?;
        self.conn()?.execute("UPDATE metadata_edit_batches SET state='recovery_required',updated=?1 WHERE state='saving'",[now()])?;
        self.conn()?.execute("UPDATE metadata_edit_rows SET state='recovery_required',error='Metadata save interrupted; recover retained native evidence' WHERE state='ready' AND EXISTS(SELECT 1 FROM metadata_edit_writes w WHERE w.row_id=metadata_edit_rows.id AND w.state IN ('prepared','publishing','recovery_required'))",[])?;
        let _organization = self
            .inner
            .organization_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Organization gate interrupted"))?;
        let _tag = self
            .inner
            .tag_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Publication gate interrupted"))?;
        let ids = {
            let conn = self.conn()?;
            let mut q=conn.prepare("SELECT id FROM metadata_edit_writes WHERE state IN ('committed','undone') AND error IS NOT NULL")?;
            q.query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        for id in ids {
            let _ = self.cleanup_metadata_write(&id);
        }
        // Saves from earlier versions that are past the Undo window.
        let _ = self.prune_metadata_undo();
        let _ = self.retire_incoming_originals();
        Ok(())
    }
}

impl crate::Library {
    /// Classify only our exact published inode while its metadata acknowledgment is pending.
    pub(crate) fn metadata_edit_source_owner(
        &self,
        path: &std::path::Path,
        hash: &str,
    ) -> Result<Option<String>> {
        let rows = {
            let conn = self.conn()?;
            let mut q=conn.prepare("SELECT w.payload_json FROM metadata_edit_writes w JOIN metadata_edit_files f ON f.write_id=w.id WHERE f.target=?1 AND w.state IN ('prepared','publishing','recovery_required','conflict')")?;
            q.query_map([path_bytes(path)], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        for payload in rows {
            let write: Write = serde_json::from_str(&payload)?;
            let Some(expected) = write.publication else {
                continue;
            };
            if expected.sha256 != hash {
                continue;
            }
            let (parent, actual) = crate::organization::capture_regular_source(path)?;
            if parent.ancestors == write.row.capture.parent.ancestors
                && actual.identity == expected.identity
                && actual.sha256 == expected.sha256
            {
                return Ok(Some(write.row.public.track_id));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests;

fn current_batch_error(conn: &rusqlite::Connection, id: &str) -> Result<Option<String>> {
    let row:Option<String>=conn.query_row("SELECT error FROM metadata_edit_rows WHERE batch_id=?1 AND error IS NOT NULL ORDER BY ordinal LIMIT 1",[id],|row|row.get(0)).optional()?;
    if row.is_some() {
        return Ok(row);
    }
    Ok(conn.query_row("SELECT error FROM metadata_edit_writes WHERE batch_id=?1 AND state IN ('committed','undone') AND error IS NOT NULL ORDER BY created,id LIMIT 1",[id],|row|row.get(0)).optional()?)
}
fn eligibility_inner(conn: &rusqlite::Connection, id: &str, state: &str) -> Result<(bool, bool)> {
    let ready:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM metadata_edit_writes w JOIN tracks t ON t.id=w.track_id WHERE w.batch_id=?1 AND w.state='committed' AND w.undo_of IS NULL AND w.override_of IS NULL AND COALESCE(json_extract(w.payload_json,'$.undo_expired'),0)=0 AND t.state IN ('ready','imported') AND t.archived=0 AND t.path_revision=w.expected_path_revision AND t.metadata_revision=w.expected_metadata_revision+1 AND t.source_hash=w.output_hash AND NOT EXISTS(SELECT 1 FROM organization_reservations o WHERE o.path=t.path))",[id],|r|r.get(0))?;
    let incoming:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM metadata_edit_overrides o JOIN tracks t ON t.id=o.track_id WHERE o.batch_id=?1 AND o.state='committed' AND o.final_json IS NOT NULL AND o.undo_batch_id IS NULL AND t.state='ready' AND t.current_job=o.job_id AND t.path_revision=json_extract(o.final_json,'$.path_revision') AND t.metadata_revision=json_extract(o.final_json,'$.metadata_revision') AND NOT EXISTS(SELECT 1 FROM preparation_checkpoints c WHERE c.job_id=o.job_id AND c.state!='complete') AND NOT EXISTS(SELECT 1 FROM organization_reservations r WHERE r.path=t.path))",[id],|r|r.get(0))?;
    let cleanup_pending:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM metadata_edit_writes WHERE batch_id=?1 AND state IN ('committed','undone') AND error IS NOT NULL)",[id],|r|r.get(0))?;
    let recover=state!="saving"&&(cleanup_pending||state=="recovery_required"||conn.query_row("SELECT EXISTS(SELECT 1 FROM metadata_edit_rows WHERE batch_id=?1 AND state IN ('ready','recovery_required'))",[id],|r|r.get(0))?);
    Ok((ready || incoming, recover))
}

impl crate::Library {
    /// Caller retains organization then tag gates. Never touch a public audio leaf.
    fn cleanup_metadata_write(&self, id: &str) -> Result<()> {
        self.cleanup_metadata_write_with(id, |_| Ok(()))
    }
    fn cleanup_metadata_write_with(
        &self,
        id: &str,
        mut boundary: impl FnMut(crate::artifact_cleanup::CleanupBoundary) -> Result<()>,
    ) -> Result<()> {
        let payload:Option<String>=self.conn()?.query_row("SELECT payload_json FROM metadata_edit_writes WHERE id=?1 AND state IN ('committed','undone')",[id],|r|r.get(0)).optional()?;
        let Some(payload) = payload else {
            return Ok(());
        };
        let write: Write = serde_json::from_str(&payload)?;
        let result = (|| -> Result<()> {
            let stage = write
                .row
                .stage
                .as_ref()
                .context("Missing metadata backup proof")?;
            // Retiring starts only after the pictures are kept, so once they are, a missing
            // original is the retirement of an earlier pass, not a loss.
            let retiring = write.artwork_kept && !bytes_path(&stage.backup)?.exists();
            if !write.backup_retired && !retiring {
                super::previews::verify_backup(self, stage)?;
            }
            let staged = bytes_path(&stage.path)?;
            ensure!(
                staged.parent() == Some(bytes_path(&stage.workspace)?.as_path()),
                "Staged metadata leaves its workspace"
            );
            let mut cleanup_error = self
                .quarantine_artifact_with(
                    "metadata_edit",
                    id,
                    &staged,
                    &stage.workspace_authority,
                    &stage.staged_fingerprint,
                    &mut boundary,
                )
                .err();
            let artifact = write.artifact()?;
            let mut removed = std::collections::BTreeSet::new();
            for path in [&artifact.publish, &artifact.displaced] {
                if removed.insert(path)
                    && let Err(error) = self.quarantine_artifact_with(
                        "metadata_edit",
                        id,
                        path,
                        &write.row.capture.parent,
                        &write.row.capture.fingerprint,
                        &mut boundary,
                    )
                {
                    cleanup_error.get_or_insert(error);
                }
            }
            for retained in &write.retained_temporaries {
                let path = retained.path.path()?;
                ensure!(
                    path.parent()
                        == Some(
                            write
                                .row
                                .capture
                                .path
                                .path()?
                                .parent()
                                .context("Missing original parent")?
                        ),
                    "Retained metadata temporary leaves its native parent"
                );
                if let Some(expected) = &retained.expected {
                    if let Err(error) = self.quarantine_artifact_with(
                        "metadata_edit",
                        id,
                        &path,
                        &write.row.capture.parent,
                        expected,
                        &mut boundary,
                    ) {
                        cleanup_error.get_or_insert(error);
                    }
                } else {
                    let retained = self.retain_unknown_artifact(
                        "metadata_edit",
                        id,
                        &path,
                        &write.row.capture.parent,
                        None,
                        "Unproven interrupted metadata copy retained",
                    );
                    cleanup_error.get_or_insert_with(|| {
                        retained.err().unwrap_or_else(|| {
                            anyhow::anyhow!("Unproven interrupted metadata copy retained")
                        })
                    });
                }
            }
            if let Some(error) = cleanup_error {
                return Err(error);
            }
            // The original audio is kept only for a save that is undone from it. A save that
            // records what it changed is undone by writing the previous values back, and the
            // Undo of a save (of an Incoming one too) is the end of it: nothing is undone from
            // the copy it made.
            if (write.editor_patch.is_some()
                || write.undo_of.is_some()
                || write.override_of.is_some())
                && !write.backup_retired
            {
                self.retire_metadata_original(id, write.clone())?;
            }
            // The Incoming save it undid no longer needs the pictures kept for it.
            if let Some(job) = &write.override_of {
                self.release_incoming_save(job, false)?;
            }
            Ok(())
        })();
        let error = result
            .as_ref()
            .err()
            .map(|e| format!("Metadata temporary cleanup retained: {e:#}"));
        self.conn()?.execute("UPDATE metadata_edit_writes SET error=?1,updated=?2 WHERE id=?3 AND state IN ('committed','undone')",params![error,now(),id])?;
        result
    }
}

impl crate::Library {
    /// Keeps the pictures a save replaced or removed (artwork is part of the library), then
    /// deletes the original audio and records that in the journal. If a picture cannot be kept
    /// the original stays: nothing is retired that Undo would still need.
    fn retire_metadata_original(&self, id: &str, mut write: Write) -> Result<()> {
        let stage = write
            .row
            .stage
            .clone()
            .context("Missing metadata backup proof")?;
        let save = |write: &Write, clear_backup: bool| -> Result<()> {
            let mut conn = self.conn()?;
            let tx = conn.transaction()?;
            tx.execute(
                "UPDATE metadata_edit_writes SET payload_json=?1 WHERE id=?2",
                params![bounded_json(write)?, id],
            )?;
            if clear_backup {
                tx.execute(
                    "UPDATE metadata_edit_files SET backup=NULL WHERE write_id=?1",
                    [id],
                )?;
            }
            tx.commit()?;
            Ok(())
        };
        if !write.artwork_kept {
            let mut kept = vec![];
            if write.undo_of.is_none()
                && write.override_of.is_none()
                && write.row.public.before.artwork != write.row.public.after.artwork
            {
                let backup = bytes_path(&stage.backup)?;
                for picture in &write.row.public.before.artwork {
                    let one = cuerithm_autotag::metadata_artwork_bytes(
                        &backup,
                        &stage.info,
                        &picture.identity,
                    )
                    .and_then(|bytes| self.store_metadata_artwork(bytes));
                    match one {
                        Ok(asset) => kept.push(PreviousPicture {
                            ordinal: picture.identity.ordinal,
                            asset_id: asset.id,
                        }),
                        Err(_) => {
                            // A picture that cannot be kept: the original stays for Undo.
                            for done in kept {
                                let _ = self.drop_metadata_artwork(&done.asset_id);
                            }
                            return Ok(());
                        }
                    }
                }
            }
            write.previous_artwork = kept;
            write.artwork_kept = true;
            save(&write, false)?;
        }
        super::previews::retire_stage_backup(self, &stage)?;
        write.backup_retired = true;
        save(&write, true)?;
        // Undoing a save is the end of it: the pictures it kept are no longer needed.
        if let Some(original) = &write.undo_of {
            self.expire_metadata_undo(original)?;
        }
        Ok(())
    }
    /// Only the newest `REMOVAL_UNDO_LIMIT` saved batches can be undone. An older one keeps its
    /// history line (what changed, before and after) and loses its Undo: its kept pictures and,
    /// for a save from an earlier version, its retained original are deleted.
    pub(crate) fn prune_metadata_undo(&self) -> Result<()> {
        // A batch counts while any of its saves can still be undone: a committed write, or an
        // Incoming edit the worker has written (those have no physical write of their own). An
        // Incoming edit is written when it is committed and its final proof is recorded; until
        // then its row is still being finished, so it neither counts nor leaves the window.
        let (writes, jobs): (Vec<String>, Vec<String>) = {
            let conn = self.conn()?;
            let batches: Vec<String> = conn
                .prepare(
                    "SELECT b.id FROM metadata_edit_batches b WHERE b.kind='apply'
                       AND (EXISTS(SELECT 1 FROM metadata_edit_writes x WHERE x.batch_id=b.id AND x.state='committed'
                                    AND x.undo_of IS NULL AND x.override_of IS NULL
                                    AND COALESCE(json_extract(x.payload_json,'$.undo_expired'),0)=0)
                         OR EXISTS(SELECT 1 FROM metadata_edit_overrides o WHERE o.batch_id=b.id
                                    AND o.state='committed' AND o.undo_batch_id IS NULL
                                    AND o.final_json IS NOT NULL))
                     ORDER BY b.created DESC,b.rowid DESC LIMIT -1 OFFSET ?1",
                )?
                .query_map([crate::REMOVAL_UNDO_LIMIT as i64], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            let (mut writes, mut jobs) = (vec![], vec![]);
            for batch in batches {
                writes.extend(
                    conn.prepare(
                        "SELECT w.id FROM metadata_edit_writes w
                          WHERE w.batch_id=?1 AND w.state='committed' AND w.undo_of IS NULL AND w.override_of IS NULL
                            AND COALESCE(json_extract(w.payload_json,'$.undo_expired'),0)=0",
                    )?
                    .query_map([&batch], |r| r.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?,
                );
                jobs.extend(
                    conn.prepare(
                        "SELECT job_id FROM metadata_edit_overrides
                          WHERE batch_id=?1 AND state='committed' AND undo_batch_id IS NULL
                            AND final_json IS NOT NULL",
                    )?
                    .query_map([&batch], |r| r.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?,
                );
            }
            (writes, jobs)
        };
        for id in writes {
            self.expire_metadata_undo(&id)?;
        }
        for job in jobs {
            self.release_incoming_save(&job, true)?;
        }
        Ok(())
    }
    /// A save can no longer be undone (it was undone, or it left the Undo window): the pictures
    /// kept for it go, and so does the original audio of a save from an earlier version.
    pub(crate) fn expire_metadata_undo(&self, id: &str) -> Result<()> {
        let payload: Option<String> = self
            .conn()?
            .query_row(
                "SELECT payload_json FROM metadata_edit_writes WHERE id=?1",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        let Some(payload) = payload else {
            return Ok(());
        };
        let mut write: Write = serde_json::from_str(&payload)?;
        for picture in std::mem::take(&mut write.previous_artwork) {
            self.drop_metadata_artwork(&picture.asset_id)?;
        }
        if !write.backup_retired
            && let Some(stage) = write.row.stage.clone()
            && super::previews::verify_backup(self, &stage).is_ok()
            && super::previews::retire_stage_backup(self, &stage).is_ok()
        {
            write.backup_retired = true;
            self.conn()?.execute(
                "UPDATE metadata_edit_files SET backup=NULL WHERE write_id=?1",
                [id],
            )?;
        }
        write.undo_expired = true;
        self.conn()?.execute(
            "UPDATE metadata_edit_writes SET payload_json=?1 WHERE id=?2",
            params![bounded_json(&write)?, id],
        )?;
        Ok(())
    }
}

pub(super) fn curation_eligibility(
    conn: &rusqlite::Connection,
    id: &str,
    state: &str,
) -> Result<bool> {
    Ok(eligibility_inner(conn, id, state)?.0)
}

fn eligibility(conn: &rusqlite::Connection, id: &str, state: &str) -> Result<(bool, bool)> {
    let (mut undo, recover) = eligibility_inner(conn, id, state)?;
    let linked: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM curation_publications WHERE child_batch_id=?1)",
        [id],
        |row| row.get(0),
    )?;
    if linked {
        undo = false;
    }
    Ok((undo, recover))
}
