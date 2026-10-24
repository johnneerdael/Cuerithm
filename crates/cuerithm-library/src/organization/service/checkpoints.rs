//! Preparation checkpoints preserve one logical analysis/tag result through organization retries.
use super::{capture::FrozenRow, *};
use crate::organization::native::{Authority, Fingerprint, NativePath, Parent};
use crate::{
    JobRun,
    model::{now, path_bytes},
};
use anyhow::{Context, Result, ensure};
use cuerithm_media::{CancellationToken, file_sha256};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct CheckpointSource {
    path: NativePath,
    parent: Authority,
    fingerprint: Fingerprint,
}
impl CheckpointSource {
    #[cfg(feature = "autotag")]
    pub(crate) fn parts(&self) -> (&NativePath, &Authority, &Fingerprint) {
        (&self.path, &self.parent, &self.fingerprint)
    }
}
#[derive(Serialize, Deserialize)]
struct Payload {
    tag_write: Option<String>,
    evidence: Option<String>,
    source: Option<CheckpointSource>,
}
struct Checkpoint {
    output: String,
    attempt: u32,
    payload: Payload,
    state: String,
    batch: Option<String>,
}
fn evidence(conn: &Connection, track: &str) -> Result<String> {
    Ok(conn.query_row("SELECT json_array(analysis_json,metadata_json,native_metadata_json,tagging_json,title,artist,album,genre,musical_key,bpm,energy,duration,metadata_revision) FROM tracks WHERE id=?1",[track],|r|r.get(0))?)
}
impl Library {
    #[cfg(feature = "autotag")]
    pub(crate) fn queue_preparation_edit_tx(
        tx: &rusqlite::Transaction<'_>,
        track_id: &str,
        job_id: &str,
        attempt: u32,
        source_hash: &str,
    ) -> Result<crate::JobId> {
        ensure!(tx.query_row("SELECT t.current_job=j.id AND t.id=?2 AND t.state='incoming' AND t.archived=0 AND t.source_hash=?4 AND j.attempts=?3 AND j.state IN ('attention','queued') FROM jobs j JOIN tracks t ON t.id=j.track_id WHERE j.id=?1",params![job_id,track_id,attempt,source_hash],|r|r.get::<_,bool>(0))?,"Incoming source or preparation ownership changed; reload the edit");
        Self::retire_cancelled_checkpoint_batch_tx(tx, job_id)?;
        ensure!(!tx.query_row("SELECT EXISTS(SELECT 1 FROM preparation_checkpoints WHERE job_id=?1 AND state!='complete' AND (state!='prepared' OR batch_id IS NOT NULL))",[job_id],|r|r.get::<_,bool>(0))?,"Preparation has a staged or published organization operation; finish its captured recovery before editing metadata");
        ensure!(!tx.query_row("SELECT EXISTS(SELECT 1 FROM tag_writes WHERE job_id=?1 AND state IN ('prepared','conflict'))",[job_id],|r|r.get::<_,bool>(0))?,"Tag publication needs recovery before accepting another metadata decision");
        let (previous,revision,analysis):(String,String,Option<String>)=tx.query_row("SELECT j.source_hash,j.settings_revision,t.analysis_json FROM jobs j JOIN tracks t ON t.id=j.track_id WHERE j.id=?1",[job_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
        let analysis_source = analysis
            .and_then(|s| serde_json::from_str::<camelot_core::FileAnalysis>(&s).ok())
            .and_then(|r| r.media.map(|m| m.source_sha256))
            .unwrap_or(previous);
        tx.execute("INSERT INTO preparation_reuse(job_id,source_hash,analysis_source_hash,analysis_settings_revision) VALUES(?1,?2,?3,?4) ON CONFLICT(job_id) DO UPDATE SET source_hash=excluded.source_hash,analysis_source_hash=excluded.analysis_source_hash,analysis_settings_revision=excluded.analysis_settings_revision",params![job_id,source_hash,analysis_source,revision])?;
        tx.execute("DELETE FROM preparation_checkpoints WHERE job_id=?1 AND state='prepared' AND batch_id IS NULL",[job_id])?;
        ensure!(tx.execute("UPDATE jobs SET source_hash=?1,state='queued',stage='queued',error=NULL,updated=?2 WHERE id=?3 AND attempts=?4 AND state IN ('attention','queued')",params![source_hash,now(),job_id,attempt])?==1,"Preparation changed while accepting metadata");
        Ok(job_id.to_owned())
    }

    pub(crate) fn preparation_source_checkpoint(
        source: &std::path::Path,
        published: &std::path::Path,
    ) -> Result<CheckpointSource> {
        let (parent, authority) =
            Parent::capture(source.parent().context("Missing checkpoint parent")?)?;
        ensure!(
            source.parent() == published.parent(),
            "Publication inode leaves captured source parent"
        );
        let fingerprint = parent.fingerprint(std::path::Path::new(
            published.file_name().context("Missing publication name")?,
        ))?;
        Ok(CheckpointSource {
            path: NativePath::new(source),
            parent: authority,
            fingerprint,
        })
    }
    fn verify_checkpoint_source(source: &CheckpointSource) -> Result<()> {
        let parent = Parent::reopen(&source.parent)?;
        let path = source.path.path()?;
        let fingerprint = parent.fingerprint(std::path::Path::new(
            path.file_name().context("Missing checkpoint name")?,
        ))?;
        ensure!(
            crate::organization::native::matches(&fingerprint, &source.fingerprint),
            "Prepared source native identity changed; checkpoint cannot adopt replacement audio"
        );
        Ok(())
    }

    fn retire_cancelled_checkpoint_batch_tx(tx: &Transaction<'_>, job: &str) -> Result<()> {
        // A cancelled intent proves that no published destination remains. Keep
        // its history and private recovery bytes, but release this checkpoint's
        // link so an explicit Retry can build a fresh plan from the same result.
        tx.execute("UPDATE preparation_checkpoints SET batch_id=NULL,state='prepared',updated=?1 WHERE job_id=?2 AND state='organizing' AND batch_id IS NOT NULL AND EXISTS(SELECT 1 FROM organization_batches b WHERE b.id=preparation_checkpoints.batch_id AND b.state IN ('partial','cancelled','complete')) AND NOT EXISTS(SELECT 1 FROM organization_batch_rows r LEFT JOIN organization_operations o ON o.id=r.operation_id WHERE r.batch_id=preparation_checkpoints.batch_id AND (r.outcome NOT IN ('failed','cancelled','blocked','skipped','unchanged') OR (r.operation_id IS NOT NULL AND o.state!='cancelled')))",params![now(),job])?;
        Ok(())
    }
    /// Explicit fresh preparation may retire only a checkpoint which cannot own
    /// any published organization path. Attention recovery keeps its own action.
    pub(crate) fn invalidate_unpublished_preparation(&self, track: &str) -> Result<()> {
        let _organization = self
            .inner
            .organization_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Organization gate interrupted"))?;
        #[cfg(feature = "autotag")]
        let _tag = self
            .inner
            .tag_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Publication gate interrupted"))?;
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        let active:Option<String>=tx.query_row("SELECT job_id FROM preparation_checkpoints WHERE track_id=?1 AND state!='complete'",[track],|r|r.get(0)).optional()?;
        let Some(job) = active else { return Ok(()) };
        Self::retire_cancelled_checkpoint_batch_tx(&tx, &job)?;
        ensure!(tx.query_row("SELECT p.state='prepared' AND p.batch_id IS NULL AND j.state IN ('attention','queued') AND t.current_job=j.id AND t.archived=0 FROM preparation_checkpoints p JOIN jobs j ON j.id=p.job_id JOIN tracks t ON t.id=j.track_id WHERE p.job_id=?1",[&job],|r|r.get::<_,bool>(0))?,"Preparation owns a staged or published operation; retry its captured settings to finish recovery before preparing again");
        tx.execute("UPDATE jobs SET state='superseded' WHERE id=?1", [&job])?;
        tx.execute(
            "DELETE FROM preparation_checkpoints WHERE job_id=?1",
            [&job],
        )?;
        tx.commit()?;
        drop(conn);
        #[cfg(feature = "autotag")]
        let _ = self.cleanup_checkpoint_tags(&job);
        Ok(())
    }
    pub(crate) fn has_preparation_checkpoint(&self, job: &str) -> Result<bool> {
        Ok(self.conn()?.query_row("SELECT EXISTS(SELECT 1 FROM preparation_checkpoints WHERE job_id=?1 AND state!='complete')",[job],|r|r.get(0))?)
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn insert_preparation_checkpoint_tx(
        tx: &Transaction<'_>,
        job: &str,
        track: &str,
        input: &str,
        output: &str,
        attempt: u32,
        tag_write: Option<&str>,
        state: &str,
        source: Option<CheckpointSource>,
    ) -> Result<()> {
        let revision: String = tx.query_row(
            "SELECT settings_revision FROM jobs WHERE id=?1",
            [job],
            |r| r.get(0),
        )?;
        let payload = Payload {
            source,
            tag_write: tag_write.map(str::to_owned),
            evidence: if state == "prepared" {
                Some(evidence(tx, track)?)
            } else {
                None
            },
        };
        ensure!(tx.execute("INSERT INTO preparation_checkpoints(job_id,track_id,settings_revision,input_hash,output_hash,attempt,payload_json,state,created,updated) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?9) ON CONFLICT(job_id) DO UPDATE SET track_id=excluded.track_id,settings_revision=excluded.settings_revision,input_hash=excluded.input_hash,output_hash=excluded.output_hash,attempt=excluded.attempt,payload_json=excluded.payload_json,state=excluded.state,batch_id=NULL,created=excluded.created,updated=excluded.updated WHERE preparation_checkpoints.state='complete'",params![job,track,revision,input,output,attempt,serde_json::to_string(&payload)?,state,now()])?==1,"An active preparation checkpoint already owns this logical job");
        Ok(())
    }
    #[cfg(feature = "autotag")]
    pub(crate) fn prepare_checkpoint_tx(
        tx: &Transaction<'_>,
        job: &str,
        source: CheckpointSource,
    ) -> Result<()> {
        let (track, text): (String, String) = tx.query_row(
            "SELECT track_id,payload_json FROM preparation_checkpoints WHERE job_id=?1",
            [job],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let mut payload: Payload = serde_json::from_str(&text)?;
        payload.evidence = Some(evidence(tx, &track)?);
        payload.source = Some(source);
        tx.execute("UPDATE preparation_checkpoints SET state='prepared',payload_json=?1,updated=?2 WHERE job_id=?3 AND state='tagging'",params![serde_json::to_string(&payload)?,now(),job])?;
        Ok(())
    }
    fn preparation_checkpoint(&self, job: &str) -> Result<Option<Checkpoint>> {
        let row=self.conn()?.query_row("SELECT output_hash,attempt,payload_json,state,batch_id FROM preparation_checkpoints WHERE job_id=?1 AND state!='complete'",[job],|r|Ok((r.get::<_,String>(0)?,r.get::<_,u32>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,Option<String>>(4)?))).optional()?;
        row.map(|(output, attempt, json, state, batch)| {
            Ok(Checkpoint {
                output,
                attempt,
                payload: serde_json::from_str(&json)?,
                state,
                batch,
            })
        })
        .transpose()
    }
    pub(crate) fn retry_preparation_checkpoint(
        &self,
        job: &str,
        revision: Option<&str>,
    ) -> Result<bool> {
        if !self.has_preparation_checkpoint(job)? {
            return Ok(false);
        }
        let _organization = self
            .inner
            .organization_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Organization gate interrupted"))?;
        #[cfg(feature = "autotag")]
        let _tag = self
            .inner
            .tag_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Publication gate interrupted"))?;
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        Self::retire_cancelled_checkpoint_batch_tx(&tx, job)?;
        if let Some(revision) = revision {
            let old: String = tx.query_row(
                "SELECT settings_revision FROM preparation_checkpoints WHERE job_id=?1",
                [job],
                |r| r.get(0),
            )?;
            if old != revision {
                let load = |id: &str| -> Result<crate::SettingsSnapshot> {
                    let text: String =
                        tx.query_row("SELECT json FROM settings WHERE revision=?1", [id], |r| {
                            r.get(0)
                        })?;
                    Ok(serde_json::from_str(&text)?)
                };
                let before = load(&old)?;
                let after = load(revision)?;
                if !Self::settings_equal_except_solver(&before, &after)? {
                    let mut renamed = before;
                    renamed.settings.organization = after.settings.organization.clone();
                    ensure!(
                        Self::settings_equal_except_solver(&renamed, &after)?,
                        "Prepared checkpoint retains its analysis and tagging configuration; finish recovery before preparing with different analysis settings"
                    );
                    ensure!(tx.query_row("SELECT state='prepared' AND batch_id IS NULL FROM preparation_checkpoints WHERE job_id=?1",[job],|r|r.get::<_,bool>(0))?,"Organization has a durable operation; retry its original settings to finish recovery before changing naming rules");
                    tx.execute(
                        "UPDATE preparation_checkpoints SET settings_revision=?1 WHERE job_id=?2",
                        params![revision, job],
                    )?;
                    tx.execute("UPDATE jobs SET settings_revision=?1 WHERE id=?2 AND state IN ('attention','queued')",params![revision,job])?;
                }
                // Solver-only retries keep the original checkpoint, journal and
                // native reference revision; the accepted output needs no HTTP.
            }
        }
        ensure!(tx.execute("UPDATE jobs SET state='queued',stage='queued',error=NULL,updated=?1 WHERE id=?2 AND state IN ('attention','queued') AND EXISTS(SELECT 1 FROM tracks t JOIN preparation_checkpoints p ON p.track_id=t.id WHERE p.job_id=jobs.id AND t.current_job=jobs.id AND t.archived=0 AND t.state='incoming' AND p.state!='complete')",params![now(),job])?==1,"Preparation is running or no longer owns its checkpoint; finish recovery before replacing it");
        tx.commit()?;
        Ok(true)
    }
    fn checkpoint_owner(conn: &Connection, run: &JobRun) -> Result<()> {
        ensure!(conn.query_row("SELECT j.state='running' AND j.attempts=?2 AND j.settings_revision=?3 AND j.source_hash=?4 AND t.current_job=j.id AND t.archived=0 AND t.state='incoming' AND p.settings_revision=j.settings_revision AND p.input_hash=j.source_hash AND p.attempt=j.attempts FROM jobs j JOIN tracks t ON t.id=j.track_id JOIN preparation_checkpoints p ON p.job_id=j.id WHERE j.id=?1",params![run.id,run.attempt,run.settings_revision,run.source_sha256],|r|r.get::<_,bool>(0))?,"Preparation checkpoint was superseded");
        Ok(())
    }
    fn rebind_checkpoint_attempt(&self, run: &JobRun, checkpoint: &Checkpoint) -> Result<()> {
        let mut path = self.track_path(&run.track_id)?;
        if let Some(batch) = &checkpoint.batch {
            let operation: Option<String> = self
                .conn()?
                .query_row(
                    "SELECT operation_id FROM organization_batch_rows WHERE batch_id=?1",
                    [batch],
                    |r| r.get(0),
                )
                .optional()?
                .flatten();
            if let Some(operation) = operation {
                let record = self.organization_record(&operation)?;
                if let Some(main) = record.files.first()
                    && main.published
                {
                    crate::organization::publish::verify_destination(main)?;
                    path = main.destination.path()?;
                }
            }
        }
        if checkpoint.batch.is_none() {
            Self::verify_checkpoint_source(
                checkpoint
                    .payload
                    .source
                    .as_ref()
                    .context("Missing prepared native source proof")?,
            )?;
        }
        ensure!(
            file_sha256(&path)? == checkpoint.output,
            "Prepared audio changed; restore or explicitly prepare the changed revision"
        );
        let rows: Vec<(String, FrozenRow, Option<std::path::PathBuf>)> = if let Some(batch) =
            &checkpoint.batch
        {
            let entries: Vec<(String, String, Option<String>)> = {
                let conn = self.conn()?;
                let mut q=conn.prepare("SELECT id,payload_json,operation_id FROM organization_batch_rows WHERE batch_id=?1")?;
                q.query_map([batch], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                    .collect::<rusqlite::Result<_>>()?
            };
            entries
                .into_iter()
                .map(|(id, json, operation)| {
                    let dest = operation
                        .map(|id| {
                            self.organization_record(&id).and_then(|r| {
                                for f in &r.files {
                                    if f.published || f.unchanged {
                                        crate::organization::publish::verify_destination(f)?;
                                    }
                                }
                                r.files
                                    .first()
                                    .context("Missing organization audio")
                                    .and_then(|f| f.destination.path())
                            })
                        })
                        .transpose()?;
                    Ok((id, serde_json::from_str(&json)?, dest))
                })
                .collect::<Result<_>>()?
        } else {
            vec![]
        };
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        ensure!(tx.query_row("SELECT j.state='running' AND j.attempts=?2 AND j.settings_revision=?3 AND j.source_hash=?4 AND t.current_job=j.id AND t.archived=0 FROM jobs j JOIN tracks t ON t.id=j.track_id WHERE j.id=?1",params![run.id,run.attempt,run.settings_revision,run.source_sha256],|r|r.get::<_,bool>(0))?,"Preparation was superseded");
        ensure!(
            checkpoint.payload.evidence.as_ref() == Some(&evidence(&tx, &run.track_id)?),
            "Prepared metadata changed; checkpoint cannot adopt new naming evidence"
        );
        for (id, mut row, destination) in rows {
            let frozen = row
                .captured
                .as_mut()
                .context("Missing prepared row proof")?;
            ensure!(
                frozen.current_job.as_ref() == Some(&run.id)
                    && frozen.attempt == Some(checkpoint.attempt),
                "Preparation row ownership changed"
            );
            frozen.attempt = Some(run.attempt);
            Self::validate_frozen_track(&tx, &run.track_id, frozen, destination.as_deref())?;
            tx.execute(
                "UPDATE organization_batch_rows SET payload_json=?1 WHERE id=?2",
                params![serde_json::to_string(&row)?, id],
            )?;
        }
        ensure!(tx.execute("UPDATE preparation_checkpoints SET attempt=?1,updated=?2 WHERE job_id=?3 AND attempt=?4",params![run.attempt,now(),run.id,checkpoint.attempt])?==1,"Checkpoint changed");
        tx.commit()?;
        Ok(())
    }
    /// Returns false only when no result has been durably prepared yet.
    pub(crate) fn continue_preparation_organization(
        &self,
        run: &JobRun,
        stop: &AtomicBool,
    ) -> Result<bool> {
        #[allow(unused_mut)]
        let Some(mut checkpoint) = self.preparation_checkpoint(&run.id)? else {
            return Ok(false);
        };
        if checkpoint.state == "tagging" {
            #[cfg(feature = "autotag")]
            {
                ensure!(self.conn()?.execute(
                    "UPDATE preparation_checkpoints SET attempt=?1 WHERE job_id=?2 AND attempt=?3 AND EXISTS(SELECT 1 FROM jobs j JOIN tracks t ON t.id=j.track_id WHERE j.id=preparation_checkpoints.job_id AND j.state='running' AND j.attempts=?1 AND j.settings_revision=?4 AND j.source_hash=?5 AND t.current_job=j.id AND t.archived=0)",
                    params![run.attempt, run.id, checkpoint.attempt,run.settings_revision,run.source_sha256],
                )?==1,"Tag checkpoint attempt was superseded");
                self.replay_checkpoint_tag_write(
                    run,
                    checkpoint
                        .payload
                        .tag_write
                        .as_deref()
                        .context("Missing staged tag journal")?,
                )?;
                checkpoint = self
                    .preparation_checkpoint(&run.id)?
                    .context("Missing preparation checkpoint")?;
            }
            #[cfg(not(feature = "autotag"))]
            anyhow::bail!("Tag checkpoint requires a tagging-enabled build");
        }
        self.rebind_checkpoint_attempt(run, &checkpoint)?;
        self.set_stage_for_run(run, "organizing")?;
        let cancel = CancellationToken::default();
        let mut boundary = |_| -> Result<()> {
            ensure!(!stop.load(Ordering::Relaxed), "Preparation is stopping");
            Self::checkpoint_owner(&*self.conn()?, run)
        };
        boundary(crate::organization::publish::Boundary::Published)?;
        let batch = if let Some(id) = checkpoint.batch {
            let current = self
                .organization_batches()?
                .into_iter()
                .find(|b| b.id == id)
                .context("Missing preparation organization batch")?;
            if current.state == "recovery_required" || current.totals.recovery_required > 0 {
                self.recover_preparation_organization_batch_with(&id, &cancel, &mut boundary)?
            } else {
                current
            }
        } else {
            let preview = self.preview_organization_ids(
                OrganizationPreviewRequest {
                    scope: OrganizationScope::Tracks {
                        track_ids: vec![run.track_id.clone()],
                    },
                    settings: run.snapshot.settings.organization.clone(),
                },
                Some(vec![run.track_id.clone()]),
                &cancel,
            )?;
            ensure!(
                preview.totals.blocked == 0,
                "Required organization is blocked; review Incoming metadata, destination, collisions and storage"
            );
            self.apply_preparation_organization_preview_with(
                &preview.id,
                &preview.revision,
                &cancel,
                &mut boundary,
            )?
        };
        ensure!(
            batch.totals.failed == 0
                && batch.totals.blocked == 0
                && batch.totals.recovery_required == 0
                && batch.totals.cancelled == 0
                && matches!(batch.state.as_str(), "complete" | "partial"),
            "Organization requires recovery before this track can become Ready"
        );
        boundary(crate::organization::publish::Boundary::Published)?;
        let path = self.track_path(&run.track_id)?;
        ensure!(
            file_sha256(&path)? == checkpoint.output,
            "Organized audio changed before Ready promotion"
        );
        let _organization = self
            .inner
            .organization_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Organization gate interrupted"))?;
        #[cfg(feature = "autotag")]
        let _tag = self
            .inner
            .tag_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Publication gate interrupted"))?;
        let operations: Vec<String> = {
            let conn = self.conn()?;
            let mut q=conn.prepare("SELECT operation_id FROM organization_batch_rows WHERE batch_id=?1 AND operation_id IS NOT NULL")?;
            q.query_map([&batch.id], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?
        };
        #[cfg(feature = "autotag")]
        let mut final_proof = checkpoint.payload.source.clone();
        if operations.is_empty() {
            Self::verify_checkpoint_source(
                checkpoint
                    .payload
                    .source
                    .as_ref()
                    .context("Missing prepared native source proof")?,
            )?;
        }
        for id in operations {
            let record = self.organization_record(&id)?;
            for file in &record.files {
                crate::organization::publish::verify_destination(file)?;
            }
            #[cfg(feature = "autotag")]
            if let Some(main) = record.files.first() {
                final_proof = Some(CheckpointSource {
                    path: main.destination.clone(),
                    parent: main.destination_parent.clone(),
                    fingerprint: if main.unchanged {
                        main.original.clone()
                    } else {
                        main.staged
                            .clone()
                            .context("Missing known published organization identity")?
                    },
                });
            }
        }
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        Self::checkpoint_owner(&tx, run)?;
        ensure!(
            path_bytes(&path)
                == tx.query_row(
                    "SELECT path FROM tracks WHERE id=?1",
                    [&run.track_id],
                    |r| r.get::<_, Vec<u8>>(0)
                )?,
            "Organized path changed before Ready promotion"
        );
        ensure!(
            checkpoint.payload.evidence.as_ref() == Some(&evidence(&tx, &run.track_id)?),
            "Prepared metadata changed before Ready promotion"
        );
        let entries: Vec<(String, String)> = {
            let mut q = tx
                .prepare("SELECT id,payload_json FROM organization_batch_rows WHERE batch_id=?1")?;
            q.query_map([&batch.id], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?
        };
        for (id, json) in entries {
            let mut row: FrozenRow = serde_json::from_str(&json)?;
            if let Some(f) = row.captured.as_mut() {
                f.state = "ready".into();
            }
            tx.execute(
                "UPDATE organization_batch_rows SET payload_json=?1 WHERE id=?2",
                params![serde_json::to_string(&row)?, id],
            )?;
        }
        tx.execute(
            "UPDATE tracks SET state='ready',updated=?1 WHERE id=?2",
            params![now(), run.track_id],
        )?;
        tx.execute(
            "UPDATE jobs SET state='completed',stage='completed',error=NULL,updated=?1 WHERE id=?2",
            params![now(), run.id],
        )?;
        ensure!(tx.execute("UPDATE preparation_checkpoints SET state='complete',updated=?1 WHERE job_id=?2 AND batch_id=?3 AND attempt=?4",params![now(),run.id,batch.id,run.attempt])?==1,"Preparation batch binding changed");
        #[cfg(feature = "autotag")]
        Self::finish_protected_match_review_tx(&tx, run)?;
        #[cfg(feature = "autotag")]
        if let Some(proof) = final_proof {
            Self::finalize_metadata_override_tx(
                &tx,
                run,
                &proof.path,
                &proof.parent,
                &proof.fingerprint,
            )?;
        }
        Self::bump(&tx)?;
        tx.commit()?;
        drop(conn);
        #[cfg(feature = "autotag")]
        let _ = self.cleanup_checkpoint_tags(&run.id);
        // An Incoming save is finished here when an automatic Move followed it: only now can it
        // count in the window of ten saves that can be undone.
        #[cfg(feature = "autotag")]
        let _ = self.prune_metadata_undo();
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "autotag")]
    #[test]
    fn explicit_edit_queues_same_logical_job_without_claiming_an_attempt() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let library = Library::open(&root.join("data/library.sqlite")).unwrap();
        let source = root.join("source.flac");
        std::fs::write(&source, b"stable source").unwrap();
        let job = library
            .enqueue(&[source], library.settings().unwrap().revision)
            .unwrap()
            .remove(0);
        let run = library.claim_next().unwrap().unwrap();
        library
            .fail_for_run(&run, "matching", "requires review")
            .unwrap();
        let mut conn = library.conn().unwrap();
        let tx = conn.transaction().unwrap();
        let result = Library::queue_preparation_edit_tx(
            &tx,
            &run.track_id,
            &job,
            run.attempt,
            &run.source_sha256,
        );
        assert_eq!(result.unwrap(), job);
        let (state, attempt): (String, u32) = tx
            .query_row("SELECT state,attempts FROM jobs WHERE id=?1", [&job], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(state, "queued");
        assert_eq!(attempt, run.attempt);
        assert!(
            Library::queue_preparation_edit_tx(
                &tx,
                &run.track_id,
                &job,
                run.attempt + 1,
                &run.source_sha256
            )
            .is_err()
        );
    }
    #[test]
    fn a_completed_checkpoint_can_be_replaced_but_an_active_one_cannot() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let library = Library::open(&root.join("data/library.sqlite")).unwrap();
        let path = root.join("source.flac");
        std::fs::write(&path, b"same source prepared explicitly again").unwrap();
        let job = library
            .enqueue(&[path], library.settings().unwrap().revision)
            .unwrap()
            .remove(0);
        let run = library.claim_next().unwrap().unwrap();
        let insert = |tx: &Transaction<'_>| {
            Library::insert_preparation_checkpoint_tx(
                tx,
                &job,
                &run.track_id,
                &run.source_sha256,
                &run.source_sha256,
                run.attempt,
                None,
                "prepared",
                None,
            )
        };
        let mut conn = library.conn().unwrap();
        let tx = conn.transaction().unwrap();
        insert(&tx).unwrap();
        assert!(
            insert(&tx).is_err(),
            "active result identity must never be replaced"
        );
        tx.execute(
            "UPDATE preparation_checkpoints SET state='complete' WHERE job_id=?1",
            [&job],
        )
        .unwrap();
        assert!(
            insert(&tx).is_ok(),
            "explicit later preparation must replace a terminal checkpoint instead of failing the same job forever"
        );
    }
}

#[cfg(all(test, feature = "autotag"))]
mod retry_tests {
    use super::*;
    use crate::organization::publish::Boundary;
    use camelot_core::{
        AnalysisConfig, FileAnalysis, KeyEstimate, Mode, MusicalKey, TempoEstimate,
        energy::TrackEnergy,
        lexicon_cues::{StructuralCue, StructuralCueResult},
    };
    use cuerithm_cleanup::{Metadata, MetadataField};
    #[test]
    fn restart_rebinds_only_attempt_and_finishes_original_plan_without_duplicate_cues() {
        for (stop_at, solver_retry) in [Boundary::StageReserved, Boundary::Published]
            .into_iter()
            .flat_map(|at| [(at, false), (at, true)])
        {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().canonicalize().unwrap();
            let db = root.join("data/library.sqlite");
            let library = Library::open(&db).unwrap();
            let source = root.join("source.flac");
            std::fs::write(&source, b"opaque measured fixture audio").unwrap();
            let managed = root.join("managed");
            std::fs::create_dir(&managed).unwrap();
            let grant = library.register_organization_root(managed.clone()).unwrap();
            let mut settings = crate::PreparationSettings {
                tagging: Some(cuerithm_autotag::AutotagConfig::default()),
                organization: OrganizationSettings {
                    mode: OrganizationSettingMode::Move,
                    pattern: "{original_name} - managed".into(),
                    destination_root_id: Some(grant.id),
                    ..Default::default()
                },
                ..Default::default()
            };
            library.save_settings(settings.clone()).unwrap();
            let job = library
                .enqueue(
                    std::slice::from_ref(&source),
                    library.settings().unwrap().revision,
                )
                .unwrap()
                .remove(0);
            let old = library.claim_next().unwrap().unwrap();
            let mut analysis =
                camelot_core::analyze_pcm(&vec![0.; 5512], 11025, &AnalysisConfig::default())
                    .unwrap();
            analysis.duration_seconds = 1.;
            analysis.key = Some(KeyEstimate {
                key: MusicalKey::new(0, Mode::Major).unwrap(),
                camelot: "8B".into(),
                margin: 0.5,
                features: None,
                candidates: vec![],
            });
            analysis.tempo = Some(TempoEstimate {
                bpm: 120.,
                strength: None,
                candidates: vec![],
                beats_seconds: vec![0., 0.25],
                downbeats_seconds: None,
                downbeat_time: Some(0.),
                method: "fixture".into(),
            });
            analysis.energy = Some(TrackEnergy {
                level: 6,
                method: "fixture".into(),
            });
            analysis.lexicon_cues = Some(StructuralCueResult {
                cues: vec![StructuralCue {
                    time_seconds: 0.,
                    name: "Start".into(),
                    color: "Red".into(),
                }],
                emergency_loop: None,
            });
            let result = FileAnalysis {
                selection: None,
                selected_beatgrid: None,
                publication: None,
                analysis,
                complete: true,
                verified_integrity: None,
                source_sample_rate: 44100,
                beat_sample_rate: None,
                neural_key_sample_rate: None,
                openkeyscan_sample_rate: None,
                source_timing: None,
                elapsed_seconds: 0.,
                media: None,
            };
            let info = cuerithm_media::MediaInfo {
                codec: "flac".into(),
                sample_rate: 44100,
                channels: 2,
                bits_per_sample: Some(16),
                tags: Default::default(),
                properties: Default::default(),
            };
            let mut native = Metadata::default();
            native.set(MetadataField::Title, vec!["Accepted title".into()]);
            library
                .apply_prepared_analysis_for_run(&old, &result, &info, Some(&native))
                .unwrap();
            let preview = library
                .preview_organization_ids(
                    OrganizationPreviewRequest {
                        scope: OrganizationScope::Tracks {
                            track_ids: vec![old.track_id.clone()],
                        },
                        settings: settings.organization.clone(),
                    },
                    Some(vec![old.track_id.clone()]),
                    &CancellationToken::default(),
                )
                .unwrap();
            let interrupted = library
                .apply_preparation_organization_preview_with(
                    &preview.id,
                    &preview.revision,
                    &CancellationToken::default(),
                    |at| {
                        if at == stop_at {
                            anyhow::bail!("power loss after native publication")
                        };
                        Ok(())
                    },
                )
                .unwrap();
            assert!(matches!(
                interrupted.state.as_str(),
                "recovery_required" | "partial"
            ));
            assert!(source.exists());
            assert_eq!(
                managed.join("source - managed.flac").exists(),
                stop_at == Boundary::Published
            );
            library
                .fail_for_run(&old, "organizing", "fixture interruption")
                .unwrap();
            if solver_retry {
                settings.tagging.as_mut().unwrap().solver.endpoint =
                    Some("http://127.0.0.1:8191".into());
                settings.tagging.as_mut().unwrap().solver.timeout_seconds = 41;
            } else {
                settings.organization.pattern = "new-rule-{original_name}".into();
            }
            let requested_revision = library.save_settings(settings).unwrap();
            assert_eq!(library.jobs().unwrap()[0].id, job);
            drop(library);
            let library = Library::open(&db).unwrap();
            assert!(
                source.exists(),
                "startup cannot blindly retire an unacknowledged source"
            );
            let receipts = |library: &Library| {
                let conn = library.conn().unwrap();
                (conn.query_row("SELECT json_group_array(json_array(id,state,payload_json)) FROM organization_operations",[],|r|r.get::<_,String>(0)).unwrap(),conn.query_row("SELECT json_group_array(json_array(operation_id,state,digest,payload_json,verification_digest)) FROM organization_reference_intents",[],|r|r.get::<_,String>(0)).unwrap())
            };
            let before_retry = receipts(&library);
            library
                .retry(&job, solver_retry.then_some(requested_revision))
                .expect("solver-only retry must preserve the existing durable operation");
            assert_eq!(
                receipts(&library),
                before_retry,
                "Retry cannot rewrite native operation/reference evidence"
            );
            let current = library.claim_next().unwrap().unwrap();
            assert_eq!(current.attempt, old.attempt + 1);
            assert_eq!(current.settings_revision, old.settings_revision);
            assert!(
                library
                    .continue_preparation_organization(&old, &AtomicBool::new(false))
                    .is_err(),
                "expired worker cannot mutate a freshly claimed checkpoint"
            );
            library
                .continue_preparation_organization(&current, &AtomicBool::new(false))
                .unwrap();
            assert!(!source.exists());
            assert_eq!(
                library.track_path(&current.track_id).unwrap(),
                managed.join("source - managed.flac")
            );
            assert_eq!(library.cue_revision_count(&current.track_id).unwrap(), 1);
            assert_eq!(library.track(&current.track_id).unwrap().state, "ready");
            let batch = library
                .organization_batches()
                .unwrap()
                .into_iter()
                .find(|b| b.state == "complete")
                .unwrap();
            assert!(
                batch.can_undo,
                "Incoming history proof must follow the final Ready promotion"
            );
        }
    }
}
