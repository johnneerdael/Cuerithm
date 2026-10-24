//! Native receipts for tag publication and retirement. Callers hold tag_gate;
//! cleanup additionally acquires cleanup_gate, then only short DB transactions.
use super::*;
use crate::organization::{NativeParent, NativePath, same_native_file as matches};
use std::path::Path;

pub(super) fn captured(path: &Path) -> Result<ArtifactRestoreProof> {
    let (parent, fingerprint) = crate::organization::capture_regular_source(path)?;
    Ok(ArtifactRestoreProof {
        path: NativePath::new(path),
        parent,
        fingerprint,
        from_backup: false,
        displaced: None,
    })
}
/// Whether the private file is still there. A backup that an earlier pass of the cleanup already
/// retired (or whose private folder is gone) is not an error: it is what cleanup is for.
fn present(proof: &ArtifactRestoreProof) -> bool {
    let (Ok(path), Ok(parent)) = (proof.path.path(), NativeParent::reopen(&proof.parent)) else {
        return false;
    };
    path.file_name()
        .is_some_and(|leaf| parent.optional(Path::new(leaf)).ok().flatten().is_some())
}
fn verify(proof: &ArtifactRestoreProof) -> Result<()> {
    let path = proof.path.path()?;
    let parent = NativeParent::reopen(&proof.parent)?;
    let observed = parent.fingerprint(Path::new(
        path.file_name().context("Missing artifact leaf")?,
    ))?;
    ensure!(
        matches(&observed, &proof.fingerprint),
        "Private tag artifact changed; all versions retained"
    );
    Ok(())
}
impl Library {
    fn save_tag_authorities(&self, journal: &Journal) -> Result<()> {
        ensure!(self.conn()?.execute("UPDATE tag_writes SET artifact_authorities_json=?1 WHERE id=?2 AND state='prepared'",params![serde_json::to_string(&journal.authorities)?,journal.id])? == 1, "Tag publication intent changed");
        Ok(())
    }
    pub(super) fn prepare_native_tag_artifacts(
        &self,
        journal: &mut Journal,
        parents: &[NativeParent],
        _run: &JobRun,
    ) -> Result<()> {
        if journal.authorities.is_empty() {
            ensure!(
                journal.input_source_proof.is_none(),
                "Protected tag publication lacks native receipts"
            );
            for file in &journal.files {
                if file.before.as_deref() != Some(file.after.as_str()) {
                    publish_files::copy_prepared(&file.staged, &file.publish, &file.after)?;
                }
            }
            return Ok(());
        }
        ensure!(
            parents.len() == journal.files.len(),
            "Missing tag publication authorities"
        );
        for (ordinal, parent) in parents.iter().enumerate() {
            let file = &journal.files[ordinal];
            let authority = journal.authorities[ordinal].clone();
            let stage = authority
                .private_staged
                .as_ref()
                .context("Missing staged native receipt; retain legacy assets")?;
            ensure!(
                stage.path == NativePath::new(&file.staged),
                "Staged artifact path changed"
            );
            verify(stage)?;
            if let Some(backup) = &authority.private_backup {
                verify(backup)?;
            }
            if file.before.as_deref() == Some(file.after.as_str()) {
                continue;
            }
            let leaf = Path::new(
                file.publish
                    .file_name()
                    .context("Missing publication leaf")?,
            );
            let visible = parent.optional(leaf)?;
            let reused = visible
                .as_ref()
                .zip(authority.published.as_ref())
                .is_some_and(|(actual, expected)| matches(actual, expected));
            let displaced_occupied = cfg!(windows)
                && parent
                    .optional(Path::new(
                        file.displaced
                            .file_name()
                            .context("Missing displaced leaf")?,
                    ))?
                    .is_some();
            if (visible.is_some() && !reused) || displaced_occupied {
                // A crash before the receipt, or an external leaf replacement,
                // cannot establish ownership merely by matching output bytes.
                for path in [&file.publish, &file.displaced] {
                    let observed = parent.optional(Path::new(
                        path.file_name().context("Missing temporary leaf")?,
                    ))?;
                    if let Some(observed) = observed {
                        if authority
                            .published
                            .as_ref()
                            .is_some_and(|known| matches(&observed, known))
                        {
                            let proof = ArtifactRestoreProof {
                                path: NativePath::new(path),
                                parent: authority.parent.clone(),
                                fingerprint: observed,
                                from_backup: false,
                                displaced: None,
                            };
                            journal.authorities[ordinal].retired.push(proof);
                        } else {
                            self.retain_unknown_artifact(
                                "tag_write",
                                &journal.id,
                                path,
                                &authority.parent,
                                Some(&observed),
                                "Unproven tag publication temporary retained",
                            )?;
                        }
                    }
                }
                let file = &mut journal.files[ordinal];
                file.publish = file
                    .target
                    .parent()
                    .context("Missing tag parent")?
                    .join(format!(".cuerithm-{}.tmp", uuid::Uuid::new_v4()));
                file.displaced = if cfg!(windows) {
                    file.publish.with_extension("before.tmp")
                } else {
                    file.publish.clone()
                };
                journal.authorities[ordinal].published = None;
                let mut conn = self.conn()?;
                let tx = conn.transaction()?;
                ensure!(tx.execute("UPDATE tag_writes SET artifact_authorities_json=?1 WHERE id=?2 AND state='prepared'",params![serde_json::to_string(&journal.authorities)?,journal.id])? == 1, "Tag publication intent changed");
                ensure!(tx.execute("UPDATE tag_write_files SET publish_path=?1,displaced_path=?2 WHERE write_id=?3 AND ordinal=?4",params![path_bytes(&file.publish),path_bytes(&file.displaced),journal.id,ordinal])? == 1, "Tag artifact intent changed");
                tx.commit()?;
            } else if reused {
                continue;
            }
            let file = &journal.files[ordinal];
            let own = parent.copy_prepared(
                &file.staged,
                Path::new(file.publish.file_name().context("Missing copy leaf")?),
                &file.after,
                &cuerithm_media::CancellationToken::default(),
            )?;
            journal.authorities[ordinal].published = Some(own);
            self.save_tag_authorities(journal)?;
        }
        Ok(())
    }
    pub(super) fn cleanup_native_tag_journal(&self, original: &Journal) -> Result<()> {
        let journal = self.load_tag_write(&original.id)?;
        ensure!(
            journal.authorities.len() == journal.files.len(),
            "Legacy tag assets lack native cleanup receipts; retained"
        );
        let mut errors = Vec::new();
        for (file, authority) in journal.files.iter().zip(&journal.authorities) {
            let changed = file.before.as_deref() != Some(file.after.as_str());
            if changed && file.before.is_some() {
                let backup = authority
                    .private_backup
                    .as_ref()
                    .context("Missing original native backup receipt")?;
                // While the original still exists only as the displaced file, the backup must be
                // intact. Once this cleanup has retired the backup, nothing needs it.
                if present(backup) {
                    verify(backup)?;
                    publish_files::durable_assets(
                        std::slice::from_ref(&backup.path.path()?),
                        &journal.workspace,
                        self.data_dir(),
                    )?;
                }
            }
            let mut proofs = authority.retired.clone();
            if let Some(stage) = &authority.private_staged {
                proofs.push(stage.clone());
            }
            if changed {
                for path in [&file.publish, &file.displaced] {
                    let parent = NativeParent::reopen(&authority.parent)?;
                    let observed = parent.optional(Path::new(
                        path.file_name().context("Missing tag temporary leaf")?,
                    ))?;
                    let known = [&authority.before, &authority.published]
                        .into_iter()
                        .flatten()
                        .find(|known| {
                            observed
                                .as_ref()
                                .is_some_and(|actual| matches(actual, known))
                        });
                    if let Some(known) = known {
                        proofs.push(ArtifactRestoreProof {
                            path: NativePath::new(path),
                            parent: authority.parent.clone(),
                            fingerprint: known.clone(),
                            from_backup: false,
                            displaced: None,
                        });
                    } else if let Some(observed) = observed {
                        self.retain_unknown_artifact(
                            "tag_write",
                            &journal.id,
                            path,
                            &authority.parent,
                            Some(&observed),
                            "Unknown tag recovery artifact retained",
                        )?;
                        errors.push("Unknown tag recovery artifact retained".to_owned());
                    }
                }
            }
            // The original audio is kept only until the write has settled: the backup goes
            // last, after the displaced original and every other private copy.
            if let Some(backup) = &authority.private_backup {
                proofs.push(backup.clone());
            }
            let mut seen = std::collections::HashSet::new();
            for proof in proofs {
                if seen.insert(proof.path.path()?)
                    && let Err(error) = self.quarantine_artifact(
                        "tag_write",
                        &journal.id,
                        &proof.path.path()?,
                        &proof.parent,
                        &proof.fingerprint,
                    )
                {
                    errors.push(format!("{error:#}"));
                }
            }
        }
        ensure!(errors.is_empty(), "{}", errors.join("; "));
        // Every backup is gone. Say so in the journal and take the empty private folder away.
        self.conn()?.execute(
            "UPDATE tag_write_files SET backup=NULL WHERE write_id=?1",
            [&journal.id],
        )?;
        let _ = std::fs::remove_dir(&journal.workspace);
        Ok(())
    }
    pub(super) fn rebind_restored_tag_execution(&self, journal: &Journal) -> Result<()> {
        let restored: Option<String> = self.conn()?.query_row(
            "SELECT restore_source_proof_json FROM tag_writes WHERE id=?1",
            [&journal.id],
            |r| r.get(0),
        )?;
        let Some(restored) = restored else {
            return Ok(());
        };
        let proof: ArtifactRestoreProof = serde_json::from_str(&restored)?;
        if verify(&proof).is_err() {
            return Ok(());
        }
        let snapshot: SettingsSnapshot = {
            let json: String = self.conn()?.query_row(
                "SELECT json FROM settings WHERE revision=?1",
                [&journal.revision],
                |r| r.get(0),
            )?;
            serde_json::from_str(&json)?
        };
        let run = JobRun {
            attempt: journal.attempt,
            id: journal.job.clone(),
            track_id: journal.track.clone(),
            path: proof.path.path()?,
            source_sha256: journal.before.clone(),
            settings_revision: journal.revision.clone(),
            snapshot,
        };
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        let owns:bool=tx.query_row("SELECT t.current_job=j.id AND t.archived=0 AND t.state='incoming' AND j.attempts=?2 AND j.settings_revision=?3 AND j.source_hash=?4 AND t.path=?5 FROM jobs j JOIN tracks t ON t.id=j.track_id WHERE j.id=?1",params![run.id,run.attempt,run.settings_revision,run.source_sha256,path_bytes(&run.path)],|r|r.get(0))?;
        if owns {
            Self::rebind_protected_match_replay_tx(&tx, &run, &journal.id, &proof)?;
            Self::rebind_metadata_override_replay_tx(
                &tx,
                &run,
                &journal.id,
                &proof.path,
                &proof.parent,
                &proof.fingerprint,
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}
