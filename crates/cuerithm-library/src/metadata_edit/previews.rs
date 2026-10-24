use super::*;
use crate::{Library, model::bytes_path, organization::NativeParent};
use anyhow::{Context, Result, ensure};

pub(super) fn verify_stage(library: &Library, stage: &StoredStage) -> Result<()> {
    let workspace = bytes_path(&stage.workspace)?;
    ensure!(
        workspace.starts_with(library.data_dir().canonicalize()?)
            && stage.workspace_authority.path.path()? == workspace,
        "Metadata workspace leaves application data"
    );
    let parent = NativeParent::reopen(&stage.workspace_authority)?;
    for (path, expected) in [
        (&stage.path, &stage.staged_fingerprint),
        (&stage.backup, &stage.backup_fingerprint),
    ] {
        let path = bytes_path(path)?;
        ensure!(
            path.parent() == Some(workspace.as_path()),
            "Metadata artifact leaves its private workspace"
        );
        let current = parent.fingerprint(std::path::Path::new(
            path.file_name().context("Missing private artifact name")?,
        ))?;
        ensure!(
            current.identity == expected.identity
                && current.sha256 == expected.sha256
                && current.bytes == expected.bytes,
            "Private metadata artifact native identity or bytes changed"
        );
    }
    Ok(())
}
fn retire_stage(library: &Library, stage: &StoredStage) -> Result<()> {
    let workspace = bytes_path(&stage.workspace)?;
    ensure!(
        workspace.starts_with(library.data_dir().canonicalize()?)
            && stage.workspace_authority.path.path()? == workspace,
        "Unowned metadata workspace"
    );
    match NativeParent::reopen(&stage.workspace_authority) {
        Ok(_) => {}
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    let paths = [bytes_path(&stage.path)?, bytes_path(&stage.backup)?];
    let owner = workspace
        .file_name()
        .context("Missing metadata workspace name")?
        .to_str()
        .context("Invalid private workspace name")?;
    for (path, expected) in paths
        .iter()
        .zip([&stage.staged_fingerprint, &stage.backup_fingerprint])
    {
        ensure!(
            path.parent() == Some(workspace.as_path()),
            "Unowned metadata artifact path"
        );
        library.quarantine_artifact(
            "metadata_preview",
            owner,
            path,
            &stage.workspace_authority,
            expected,
        )?;
    }
    ensure!(
        std::fs::read_dir(&workspace)?.next().is_none(),
        "Unknown metadata workspace entry; preserve it"
    );
    let authority = crate::organization::Authority {
        path: crate::organization::NativePath::new(
            workspace
                .parent()
                .context("Missing metadata workspace parent")?,
        ),
        ancestors: stage.workspace_authority.ancestors
            [..stage.workspace_authority.ancestors.len() - 1]
            .to_vec(),
    };
    NativeParent::reopen(&authority)?.remove_private_directory(
        std::path::Path::new(
            workspace
                .file_name()
                .context("Missing metadata workspace name")?,
        ),
        &stage.workspace_authority,
    )?;
    Ok(())
}
/// Deletes the original audio a settled save kept and, with it, the private folder. The staged
/// copy has gone already. Nothing else in the folder is touched: an unknown entry keeps the
/// folder.
pub(super) fn retire_stage_backup(library: &Library, stage: &StoredStage) -> Result<()> {
    let workspace = bytes_path(&stage.workspace)?;
    ensure!(
        workspace.starts_with(library.data_dir().canonicalize()?)
            && stage.workspace_authority.path.path()? == workspace,
        "Unowned metadata workspace"
    );
    match NativeParent::reopen(&stage.workspace_authority) {
        Ok(_) => {}
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    let backup = bytes_path(&stage.backup)?;
    ensure!(
        backup.parent() == Some(workspace.as_path()),
        "Unowned metadata artifact path"
    );
    let owner = workspace
        .file_name()
        .context("Missing metadata workspace name")?
        .to_str()
        .context("Invalid private workspace name")?;
    library.quarantine_artifact(
        "metadata_original",
        owner,
        &backup,
        &stage.workspace_authority,
        &stage.backup_fingerprint,
    )?;
    if std::fs::read_dir(&workspace)?.next().is_some() {
        // Something else is in the folder: leave it for the user's recovery tools to explain.
        return Ok(());
    }
    let authority = crate::organization::Authority {
        path: crate::organization::NativePath::new(
            workspace
                .parent()
                .context("Missing metadata workspace parent")?,
        ),
        ancestors: stage.workspace_authority.ancestors
            [..stage.workspace_authority.ancestors.len() - 1]
            .to_vec(),
    };
    NativeParent::reopen(&authority)?.remove_private_directory(
        std::path::Path::new(
            workspace
                .file_name()
                .context("Missing metadata workspace name")?,
        ),
        &stage.workspace_authority,
    )?;
    Ok(())
}
impl Library {
    pub(crate) fn retire_unwritten_metadata_stages(&self, batch: &str) -> Result<()> {
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
        let rows = {
            let conn = self.conn()?;
            let mut query=conn.prepare("SELECT r.payload_json,r.state FROM metadata_edit_rows r WHERE r.batch_id=?1 AND r.state IN ('queued','unchanged','failed','cancelled') AND NOT EXISTS(SELECT 1 FROM metadata_edit_writes w WHERE w.row_id=r.id)")?;
            query
                .query_map([batch], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        for (payload, state) in rows {
            let row: FrozenRow = serde_json::from_str(&payload)?;
            if let Some(stage) = row.stage {
                if state == "queued" {
                    let path = bytes_path(&stage.path)?;
                    let _ = self.quarantine_artifact(
                        "metadata_preview",
                        batch,
                        &path,
                        &stage.workspace_authority,
                        &stage.staged_fingerprint,
                    );
                } else {
                    let _ = retire_stage(self, &stage);
                }
            }
        }
        Ok(())
    }
    /// A queued Incoming edit that a newer one replaced can never be undone or executed, so its
    /// retained original and workspace are removed; the row keeps its history without the stage.
    pub(crate) fn retire_superseded_metadata_stages(&self) -> Result<()> {
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
        let mut skipped = 0;
        loop {
            let rows = {
                let conn = self.conn()?;
                let mut query=conn.prepare("SELECT id,payload_json FROM metadata_edit_rows WHERE state='superseded' AND json_type(payload_json,'$.stage')='object' ORDER BY rowid LIMIT 50 OFFSET ?1")?;
                query
                    .query_map([skipped], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            };
            if rows.is_empty() {
                return Ok(());
            }
            for (id, payload) in rows {
                let retired = (|| -> Result<()> {
                    let row: FrozenRow = serde_json::from_str(&payload)?;
                    if let Some(stage) = row.stage {
                        retire_stage(self, &stage)?;
                    }
                    Ok(())
                })();
                if retired.is_ok() {
                    self.conn()?.execute("UPDATE metadata_edit_rows SET payload_json=json_set(payload_json,'$.stage',json('null')) WHERE id=?1",[id])?;
                } else {
                    skipped += 1;
                }
            }
        }
    }
    pub(crate) fn retire_metadata_previews(&self) -> Result<()> {
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
        let previews = {
            let conn = self.conn()?;
            let mut q=conn.prepare("SELECT id,payload_json FROM metadata_edit_previews WHERE state IN ('stale','discarded','retiring','cleanup_required')")?;
            q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        for (id, payload) in previews {
            let result = (|| -> Result<()> {
                let plan: FrozenPreview = serde_json::from_str(&payload)?;
                for row in plan.rows {
                    if let Some(stage) = row.stage {
                        let used: bool = self.conn()?.query_row(
                            "SELECT EXISTS(SELECT 1 FROM metadata_edit_writes WHERE workspace=?1)",
                            [&stage.workspace],
                            |r| r.get(0),
                        )?;
                        ensure!(
                            !used,
                            "Published metadata evidence cannot be retired with a preview"
                        );
                        retire_stage(self, &stage)?;
                    }
                }
                Ok(())
            })();
            match result {
                Ok(()) => {
                    let protected:bool=self.conn()?.query_row("SELECT EXISTS(SELECT 1 FROM curation_publications WHERE child_preview_id=?1 AND child_batch_id IS NULL AND state IN ('saving','error','recovery_required','waiting_incoming'))",[&id],|row|row.get(0))?;
                    if protected {
                        self.conn()?.execute("UPDATE metadata_edit_previews SET state='retired' WHERE id=?1 AND state!='applied'",[id])?;
                    } else {
                        self.conn()?.execute(
                            "DELETE FROM metadata_edit_previews WHERE id=?1 AND state!='applied'",
                            [id],
                        )?;
                    }
                }
                Err(error) => {
                    self.conn()?.execute("UPDATE metadata_edit_previews SET state='cleanup_required' WHERE id=?1 AND state!='applied'",[id])?;
                    let _ = error;
                }
            }
        }
        Ok(())
    }
}
pub(super) struct UnpersistedStages {
    pub library: Library,
    pub stages: Vec<StoredStage>,
    pub persisted: bool,
}
impl Drop for UnpersistedStages {
    fn drop(&mut self) {
        if !self.persisted {
            let Ok(_organization) = self.library.inner.organization_gate.lock() else {
                return;
            };
            let Ok(_tag) = self.library.inner.tag_gate.lock() else {
                return;
            };
            for stage in &self.stages {
                let _ = retire_stage(&self.library, stage);
            }
        }
    }
}

pub(super) fn verify_backup(library: &Library, stage: &StoredStage) -> Result<()> {
    let workspace = bytes_path(&stage.workspace)?;
    ensure!(
        workspace.starts_with(library.data_dir().canonicalize()?)
            && stage.workspace_authority.path.path()? == workspace,
        "Metadata backup leaves application data"
    );
    let parent = NativeParent::reopen(&stage.workspace_authority)?;
    let path = bytes_path(&stage.backup)?;
    ensure!(
        path.parent() == Some(workspace.as_path()),
        "Metadata backup leaves its private workspace"
    );
    let actual = parent.fingerprint(std::path::Path::new(
        path.file_name().context("Missing backup name")?,
    ))?;
    ensure!(
        actual.identity == stage.backup_fingerprint.identity
            && actual.sha256 == stage.before
            && actual.sha256 == stage.backup_fingerprint.sha256,
        "Durable original backup identity or bytes changed"
    );
    Ok(())
}
