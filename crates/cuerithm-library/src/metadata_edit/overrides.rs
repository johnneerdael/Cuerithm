use super::*;
use crate::{JobRun, Library, model::now};
use anyhow::{Context, Result, ensure};
use cuerithm_autotag::MetadataPatch;
use rusqlite::{OptionalExtension, params};
impl MetadataEditService {
    pub(super) fn override_payload(
        row: &FrozenRow,
        patch: &MetadataEditorPatch,
    ) -> Result<(String, String)> {
        let payload = bounded_json(patch)?;
        ensure!(
            payload.len() <= 2 * 1024 * 1024,
            "Metadata override exceeds2MiB"
        );
        let proof = bounded_json(&OverrideSource {
            path: row.capture.path.clone(),
            parent: row.capture.parent.clone(),
            fingerprint: row.capture.fingerprint.clone(),
        })?;
        Ok((payload, proof))
    }
    pub(super) fn queue_override_tx(
        tx: &rusqlite::Transaction<'_>,
        batch: &str,
        row_id: &str,
        row: &FrozenRow,
        payload: &str,
        source_proof: &str,
    ) -> Result<()> {
        let job = row
            .capture
            .job
            .as_deref()
            .context("Incoming track has no preparation owner")?;
        let attempt = row.capture.attempt.context("Missing preparation attempt")?;
        Self::validate_capture(tx, &row.capture, None)?;
        Library::queue_preparation_edit_tx(
            tx,
            &row.public.track_id,
            job,
            attempt,
            &row.capture.fingerprint.sha256,
        )?;
        let revision: String = tx.query_row(
            "SELECT settings_revision FROM jobs WHERE id=?1",
            [job],
            |r| r.get(0),
        )?;
        tx.execute("UPDATE metadata_edit_rows SET state='superseded',error='A newer saved metadata override replaced this queued revision' WHERE track_id=?1 AND state='queued'",[&row.public.track_id])?;
        let draft: (String, u64) = tx.query_row(
            "SELECT draft_id,draft_revision FROM metadata_edit_batches WHERE id=?1",
            [batch],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        tx.execute("INSERT INTO metadata_edit_overrides(job_id,track_id,draft_id,draft_revision,batch_id,source_hash,settings_revision,path_revision,metadata_revision,accepted_attempt,claimed_attempt,execution_source_proof_json,state,payload_json,created,updated) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,NULL,?13,'queued',?11,?12,?12) ON CONFLICT(job_id) DO UPDATE SET draft_id=excluded.draft_id,draft_revision=excluded.draft_revision,batch_id=excluded.batch_id,source_hash=excluded.source_hash,settings_revision=excluded.settings_revision,path_revision=excluded.path_revision,metadata_revision=excluded.metadata_revision,accepted_attempt=excluded.accepted_attempt,claimed_attempt=NULL,execution_source_proof_json=excluded.execution_source_proof_json,receipt_json=NULL,final_json=NULL,undo_batch_id=NULL,state='queued',payload_json=excluded.payload_json,updated=excluded.updated",params![job,row.public.track_id,draft.0,draft.1,batch,row.capture.fingerprint.sha256,revision,row.capture.public.path_revision,row.capture.public.metadata_revision,attempt,payload,now(),source_proof])?;
        tx.execute(
            "UPDATE metadata_edit_rows SET state='queued',error=NULL WHERE id=?1",
            [row_id],
        )?;
        Library::retire_superseded_batches_tx(tx, batch)?;
        Library::bump(tx)?;
        Ok(())
    }
}

impl Library {
    /// The original audio an accepted Incoming save keeps is only read by its Undo, and only for
    /// the pictures the save replaced or removed. Once the save has been written, those pictures
    /// become artwork assets (artwork is part of the library) and the original is deleted. A
    /// picture that cannot be kept leaves the original where it is: nothing is retired that Undo
    /// would still need. Run after every tag write commits and when the library opens.
    pub(crate) fn retire_incoming_originals(&self) -> Result<usize> {
        let rows: Vec<(String, String)> = {
            let conn = self.conn()?;
            let mut query = conn.prepare(
                "SELECT r.id,r.payload_json FROM metadata_edit_rows r
                   JOIN metadata_edit_overrides o ON o.batch_id=r.batch_id AND o.track_id=r.track_id
                  WHERE r.state='written' AND o.state IN ('committed','expired','undone')
                    AND COALESCE(json_extract(r.payload_json,'$.original_retired'),0)=0
                    AND json_type(r.payload_json,'$.stage.backup')='array'
                  ORDER BY r.rowid LIMIT 50",
            )?;
            query
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?
        };
        let save = |id: &str, row: &FrozenRow| -> Result<()> {
            self.conn()?.execute(
                "UPDATE metadata_edit_rows SET payload_json=?1 WHERE id=?2",
                params![bounded_json(row)?, id],
            )?;
            Ok(())
        };
        let mut retired = 0;
        'rows: for (id, text) in rows {
            let mut row: FrozenRow = serde_json::from_str(&text)?;
            let Some(stage) = row.stage.clone() else {
                continue;
            };
            if row.kept_pictures.is_none() {
                let mut kept = vec![];
                if row.public.before.artwork != row.public.after.artwork {
                    let backup = crate::model::bytes_path(&stage.backup)?;
                    for picture in &row.public.before.artwork {
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
                                for done in kept {
                                    let _ = self.drop_metadata_artwork(&done.asset_id);
                                }
                                continue 'rows;
                            }
                        }
                    }
                }
                row.kept_pictures = Some(kept);
                save(&id, &row)?;
            }
            if super::previews::retire_stage_backup(self, &stage).is_err() {
                continue;
            }
            row.original_retired = true;
            save(&id, &row)?;
            retired += 1;
        }
        Ok(retired)
    }

    /// An Incoming save can no longer be undone (it was undone, or `expire` says it left the
    /// window of ten): the pictures kept for it go, and so does its original audio if that is
    /// still there. With `expire` the save also stops being offered for Undo.
    pub(crate) fn release_incoming_save(&self, job: &str, expire: bool) -> Result<()> {
        // A save whose final proof is not recorded yet is still being finished (the worker has
        // committed it, an automatic Move may follow): it keeps its original and pictures.
        if expire {
            let finished: bool = self.conn()?.query_row(
                "SELECT EXISTS(SELECT 1 FROM metadata_edit_overrides WHERE job_id=?1 AND state='committed' AND undo_batch_id IS NULL AND final_json IS NOT NULL)",
                [job],
                |r| r.get(0),
            )?;
            if !finished {
                return Ok(());
            }
        }
        let row: Option<(String, String)> = self
            .conn()?
            .query_row(
                "SELECT r.id,r.payload_json FROM metadata_edit_rows r
                   JOIN metadata_edit_overrides o ON o.batch_id=r.batch_id AND o.track_id=r.track_id
                  WHERE o.job_id=?1",
                [job],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((id, text)) = row {
            let mut row: FrozenRow = serde_json::from_str(&text)?;
            for picture in row.kept_pictures.take().into_iter().flatten() {
                self.drop_metadata_artwork(&picture.asset_id)?;
            }
            // Nothing is left to keep; a later sweep only has the audio to retire.
            row.kept_pictures = Some(vec![]);
            if !row.original_retired
                && let Some(stage) = row.stage.clone()
                && super::previews::retire_stage_backup(self, &stage).is_ok()
            {
                row.original_retired = true;
            }
            self.conn()?.execute(
                "UPDATE metadata_edit_rows SET payload_json=?1 WHERE id=?2",
                params![bounded_json(&row)?, id],
            )?;
        }
        if expire {
            self.conn()?.execute(
                "UPDATE metadata_edit_overrides SET state='expired',updated=?1 WHERE job_id=?2 AND state='committed' AND undo_batch_id IS NULL AND final_json IS NOT NULL",
                params![now(), job],
            )?;
        }
        Ok(())
    }

    /// Project only retained Incoming originals; never decode historical snapshots.
    pub(crate) fn metadata_override_backup_paths_page(
        &self,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<Vec<u8>>> {
        ensure!(
            (1..=200).contains(&limit) && offset <= i64::MAX as usize,
            "Metadata backup page requires limit1..200 and a valid offset"
        );
        let paths = {
            let conn = self.conn()?;
            let mut query=conn.prepare("SELECT json_extract(r.payload_json,'$.stage.backup') FROM metadata_edit_rows r WHERE r.state IN ('queued','written','superseded') AND json_extract(r.payload_json,'$.capture.public.state')='incoming' AND COALESCE(json_extract(r.payload_json,'$.original_retired'),0)=0 AND json_type(r.payload_json,'$.stage.backup')='array' AND EXISTS(SELECT 1 FROM metadata_edit_overrides o WHERE o.job_id=json_extract(r.payload_json,'$.capture.job') AND o.track_id=r.track_id) ORDER BY r.rowid LIMIT ?1 OFFSET ?2")?;
            query
                .query_map(params![limit, offset], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        paths
            .into_iter()
            .map(|json| {
                ensure!(
                    json.len() <= 256 * 1024,
                    "Metadata backup path exceeds bounded native path size"
                );
                let path: Vec<u8> = serde_json::from_str(&json)?;
                ensure!(
                    crate::model::bytes_path(&path)?.is_absolute(),
                    "Metadata backup path is not absolute"
                );
                Ok(path)
            })
            .collect()
    }
    pub(crate) fn metadata_override_for_run(&self, run: &JobRun) -> Result<Option<MetadataPatch>> {
        let payload = {
            let mut conn = self.conn()?;
            let tx = conn.transaction()?;
            let row:Option<(String,String,u64,u64,u32,String)>=tx.query_row("SELECT payload_json,source_hash,path_revision,metadata_revision,accepted_attempt,settings_revision FROM metadata_edit_overrides WHERE job_id=?1 AND state IN ('queued','claimed')",[&run.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional()?;
            let Some((payload, hash, path_rev, meta_rev, accepted, settings_revision)) = row else {
                return Ok(None);
            };
            ensure!(
                Self::run_owns(&tx, run)?
                    && hash == run.source_sha256
                    && run.attempt > accepted
                    && settings_revision == run.settings_revision,
                "Saved metadata override no longer owns this preparation attempt"
            );
            ensure!(tx.query_row("SELECT path_revision=?2 AND metadata_revision=?3 AND source_hash=?4 FROM tracks WHERE id=?1",params![run.track_id,path_rev,meta_rev,hash],|r|r.get::<_,bool>(0))?,"Saved metadata override source changed");
            tx.execute("UPDATE metadata_edit_overrides SET state='claimed',claimed_attempt=?1,updated=?2 WHERE job_id=?3",params![run.attempt,now(),run.id])?;
            tx.commit()?;
            payload
        };
        let patch: MetadataEditorPatch = serde_json::from_str(&payload)?;
        Ok(Some(super::service::resolve_patch(self, &patch)?))
    }
    pub(crate) fn ack_metadata_override_tx(
        tx: &rusqlite::Transaction<'_>,
        run: &JobRun,
        source_before_sha256: &str,
        editor_receipt: Option<&str>,
    ) -> Result<()> {
        let row:Option<(String,String,u64)>=tx.query_row("SELECT batch_id,draft_id,draft_revision FROM metadata_edit_overrides WHERE job_id=?1 AND state='claimed' AND claimed_attempt=?2 AND source_hash=?3",params![run.id,run.attempt,run.source_sha256],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        let Some((batch, draft, revision)) = row else {
            return Ok(());
        };
        ensure!(
            source_before_sha256 == run.source_sha256,
            "Metadata override publication source differs"
        );
        let receipt = editor_receipt
            .context("Protected metadata override was not applied to staged output")?;
        tx.execute("UPDATE metadata_edit_overrides SET state='committed',updated=?1,receipt_json=?4 WHERE job_id=?2 AND claimed_attempt=?3",params![now(),run.id,run.attempt,receipt])?;
        tx.execute("UPDATE metadata_edit_rows SET state='written',error=NULL,result_json=?1 WHERE batch_id=?2 AND track_id=?3 AND state='queued'",params![receipt,batch,run.track_id])?;
        Self::settle_override_batch_tx(tx, &batch, &draft, revision)?;
        Self::ack_curation_child_tx(tx, &batch, &run.track_id)?;
        Ok(())
    }
    /// A newer saved override replaced a queued row. Any other batch left with nothing waiting
    /// becomes `partial`, and its draft follows so Pending stops listing it as queued.
    fn retire_superseded_batches_tx(
        tx: &rusqlite::Transaction<'_>,
        newer_batch: &str,
    ) -> Result<()> {
        tx.execute("UPDATE metadata_edit_batches SET state='partial',updated=?1 WHERE state='queued' AND id!=?2 AND EXISTS(SELECT 1 FROM metadata_edit_rows r WHERE r.batch_id=metadata_edit_batches.id AND r.state='superseded') AND NOT EXISTS(SELECT 1 FROM metadata_edit_rows r WHERE r.batch_id=metadata_edit_batches.id AND r.state IN ('queued','ready','recovery_required'))",params![now(),newer_batch])?;
        tx.execute("UPDATE metadata_edit_drafts SET state='partial',updated=?1 WHERE state='queued' AND EXISTS(SELECT 1 FROM metadata_edit_batches b WHERE b.draft_id=metadata_edit_drafts.id AND b.draft_revision=metadata_edit_drafts.revision AND b.state='partial' AND b.id!=?2)",params![now(),newer_batch])?;
        Ok(())
    }
    /// Settle a batch once no row is waiting, ranked as `finish_batch` ranks it: a failed,
    /// cancelled or superseded row makes it `partial`, otherwise `saved`, and the draft follows.
    /// Leaving a batch with a failed row `queued` would never be recomputed.
    fn settle_override_batch_tx(
        tx: &rusqlite::Transaction<'_>,
        batch: &str,
        draft: &str,
        revision: u64,
    ) -> Result<()> {
        tx.execute("UPDATE metadata_edit_batches SET state=CASE WHEN EXISTS(SELECT 1 FROM metadata_edit_rows WHERE batch_id=?2 AND state IN ('failed','cancelled','superseded')) THEN 'partial' ELSE 'saved' END,updated=?1 WHERE id=?2 AND NOT EXISTS(SELECT 1 FROM metadata_edit_rows WHERE batch_id=?2 AND state IN ('queued','ready','recovery_required'))",params![now(),batch])?;
        tx.execute("UPDATE metadata_edit_drafts SET state=(SELECT state FROM metadata_edit_batches WHERE id=?4),updated=?1 WHERE id=?2 AND revision=?3 AND EXISTS(SELECT 1 FROM metadata_edit_batches WHERE id=?4 AND state IN ('saved','partial'))",params![now(),draft,revision,batch])?;
        Ok(())
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct FinalOverride {
    pub path: crate::organization::NativePath,
    pub parent: crate::organization::Authority,
    pub fingerprint: crate::organization::Fingerprint,
    pub path_revision: u64,
    pub metadata_revision: u64,
}
impl Library {
    pub(crate) fn finalize_metadata_override_tx(
        tx: &rusqlite::Transaction<'_>,
        run: &JobRun,
        path: &crate::organization::NativePath,
        parent: &crate::organization::Authority,
        fingerprint: &crate::organization::Fingerprint,
    ) -> Result<()> {
        Self::finalize_curation_preparation_tx(tx, run, path, parent, fingerprint)?;
        let pending:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM metadata_edit_overrides WHERE job_id=?1 AND state='committed' AND claimed_attempt=?2 AND final_json IS NULL)",params![run.id,run.attempt],|r|r.get(0))?;
        if !pending {
            return Ok(());
        }
        let(path_bytes,hash,path_revision,metadata_revision,current_job):(Vec<u8>,String,u64,u64,Option<String>)=tx.query_row("SELECT path,source_hash,path_revision,metadata_revision,current_job FROM tracks WHERE id=?1",[&run.track_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;
        ensure!(
            path_bytes == path.0
                && hash == fingerprint.sha256
                && current_job.as_deref() == Some(run.id.as_str()),
            "Final metadata Undo proof differs from prepared track"
        );
        let proof = FinalOverride {
            path: path.clone(),
            parent: parent.clone(),
            fingerprint: fingerprint.clone(),
            path_revision,
            metadata_revision,
        };
        tx.execute("UPDATE metadata_edit_overrides SET final_json=?1,updated=?2 WHERE job_id=?3 AND claimed_attempt=?4 AND state='committed' AND final_json IS NULL",params![super::bounded_json(&proof)?,now(),run.id,run.attempt])?;
        Ok(())
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct OverrideSource {
    path: crate::organization::NativePath,
    parent: crate::organization::Authority,
    fingerprint: crate::organization::Fingerprint,
}
impl Library {
    pub(crate) fn verify_metadata_override_source_for_run(&self, run: &JobRun) -> Result<()> {
        let payload:Option<String>=self.conn()?.query_row("SELECT execution_source_proof_json FROM metadata_edit_overrides WHERE job_id=?1 AND state IN ('queued','claimed')",[&run.id],|r|r.get(0)).optional()?;
        let Some(payload) = payload else {
            return Ok(());
        };
        let expected: OverrideSource = serde_json::from_str(&payload)?;
        let (parent, actual) = crate::organization::capture_regular_source(&expected.path.path()?)?;
        ensure!(
            parent.ancestors == expected.parent.ancestors
                && actual.identity == expected.fingerprint.identity
                && actual.sha256 == expected.fingerprint.sha256
                && actual.sha256 == run.source_sha256,
            "Protected metadata override source native identity or bytes changed"
        );
        Ok(())
    }
    pub(crate) fn rebind_metadata_override_replay_tx(
        tx: &rusqlite::Transaction<'_>,
        run: &JobRun,
        journal_id: &str,
        path: &crate::organization::NativePath,
        parent: &crate::organization::Authority,
        fingerprint: &crate::organization::Fingerprint,
    ) -> Result<()> {
        let active:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM metadata_edit_overrides WHERE job_id=?1 AND state IN ('queued','claimed'))",[&run.id],|r|r.get(0))?;
        if !active {
            return Ok(());
        }
        let(input,restored):(Option<String>,Option<String>)=tx.query_row("SELECT input_source_proof_json,restore_source_proof_json FROM tag_writes WHERE id=?1 AND job_id=?2 AND track_id=?3 AND settings_revision=?4 AND before_hash=?5 AND state IN ('prepared','rolled_back')",params![journal_id,run.id,run.track_id,run.settings_revision,run.source_sha256],|r|Ok((r.get(0)?,r.get(1)?)))?;
        let expected = OverrideSource {
            path: path.clone(),
            parent: parent.clone(),
            fingerprint: fingerprint.clone(),
        };
        let matched = [input, restored]
            .into_iter()
            .flatten()
            .map(|json| serde_json::from_str::<OverrideSource>(&json))
            .collect::<serde_json::Result<Vec<_>>>()?
            .into_iter()
            .any(|proof| {
                proof.path == expected.path
                    && proof.parent.path == expected.parent.path
                    && proof.parent.ancestors == expected.parent.ancestors
                    && proof.fingerprint.identity == expected.fingerprint.identity
                    && proof.fingerprint.sha256 == expected.fingerprint.sha256
                    && proof.fingerprint.bytes == expected.fingerprint.bytes
            });
        ensure!(
            matched && fingerprint.sha256 == run.source_sha256,
            "Protected override replay source is not owned by its tag journal"
        );
        tx.execute("UPDATE metadata_edit_overrides SET execution_source_proof_json=?1,updated=?2 WHERE job_id=?3 AND state IN ('queued','claimed')",params![bounded_json(&expected)?,now(),run.id])?;
        Ok(())
    }
}

impl Library {
    pub(crate) fn metadata_override_for_query(
        &self,
        track: &str,
        job: &str,
        source: &str,
    ) -> Result<Option<MetadataPatch>> {
        let row:Option<(String,String)>=self.conn()?.query_row("SELECT o.payload_json,o.execution_source_proof_json FROM metadata_edit_overrides o JOIN tracks t ON t.id=o.track_id JOIN jobs j ON j.id=o.job_id WHERE o.track_id=?1 AND o.job_id=?2 AND o.source_hash=?3 AND o.state IN ('queued','claimed') AND t.current_job=o.job_id AND t.state='incoming' AND t.archived=0 AND t.source_hash=o.source_hash AND t.path_revision=o.path_revision AND t.metadata_revision=o.metadata_revision AND j.settings_revision=o.settings_revision",params![track,job,source],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        let Some((payload, proof)) = row else {
            return Ok(None);
        };
        let proof: OverrideSource = serde_json::from_str(&proof)?;
        let (parent, current) = crate::organization::capture_regular_source(&proof.path.path()?)?;
        ensure!(
            parent.ancestors == proof.parent.ancestors
                && current.identity == proof.fingerprint.identity
                && current.sha256 == proof.fingerprint.sha256
                && current.sha256 == source,
            "Protected editor query source changed"
        );
        let patch: MetadataEditorPatch = serde_json::from_str(&payload)?;
        Ok(Some(super::service::resolve_patch(self, &patch)?))
    }
}

#[cfg(test)]
mod settle_tests {
    use super::*;
    /// A queued batch with one row per given state, each on its own track.
    fn batch_with(states: &[&str]) -> (tempfile::TempDir, Library) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let lib = Library::open(&root.join("library.sqlite")).unwrap();
        {
            let conn = lib.conn().unwrap();
            conn.execute(
                "INSERT INTO metadata_edit_drafts VALUES('d',1,'queued','{}',1,1)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO metadata_edit_batches VALUES('b','d',1,'save','queued','{}',NULL,1,1)",
                [],
            )
            .unwrap();
        }
        for (index, state) in states.iter().enumerate() {
            let file = root.join(format!("track-{index}.wav"));
            std::fs::write(&file, format!("audio {index}")).unwrap();
            let job = lib
                .enqueue(
                    std::slice::from_ref(&file),
                    lib.settings().unwrap().revision,
                )
                .unwrap()
                .remove(0);
            let track = lib
                .jobs()
                .unwrap()
                .into_iter()
                .find(|j| j.id == job)
                .unwrap()
                .track_id;
            lib.conn()
                .unwrap()
                .execute(
                    "INSERT INTO metadata_edit_rows(id,batch_id,track_id,ordinal,state,payload_json) VALUES(?1,'b',?2,?3,?4,'{}')",
                    params![format!("r{index}"), track, index, state],
                )
                .unwrap();
        }
        (temp, lib)
    }
    fn states(conn: &rusqlite::Connection) -> (String, String) {
        let state = |table: &str, id: &str| {
            conn.query_row(
                &format!("SELECT state FROM {table} WHERE id=?1"),
                [id],
                |r| r.get::<_, String>(0),
            )
            .unwrap()
        };
        (
            state("metadata_edit_batches", "b"),
            state("metadata_edit_drafts", "d"),
        )
    }
    fn settle(lib: &Library) -> (String, String) {
        let mut conn = lib.conn().unwrap();
        let tx = conn.transaction().unwrap();
        Library::settle_override_batch_tx(&tx, "b", "d", 1).unwrap();
        tx.commit().unwrap();
        states(&conn)
    }
    fn retire(lib: &Library, newer_batch: &str) -> (String, String) {
        let mut conn = lib.conn().unwrap();
        let tx = conn.transaction().unwrap();
        Library::retire_superseded_batches_tx(&tx, newer_batch).unwrap();
        tx.commit().unwrap();
        states(&conn)
    }
    #[test]
    fn superseding_a_batchs_only_waiting_row_retires_the_batch_and_its_draft_together() {
        let (_t, lib) = batch_with(&["written", "superseded"]);
        assert_eq!(retire(&lib, "newer"), ("partial".into(), "partial".into()));
    }
    #[test]
    fn a_superseded_batch_with_another_row_still_waiting_stays_queued() {
        for waiting in ["queued", "ready", "recovery_required"] {
            let (_t, lib) = batch_with(&["superseded", waiting]);
            assert_eq!(
                retire(&lib, "newer"),
                ("queued".into(), "queued".into()),
                "{waiting}"
            );
        }
    }
    #[test]
    fn the_newer_batch_is_never_retired_by_its_own_override() {
        let (_t, lib) = batch_with(&["written", "superseded"]);
        assert_eq!(retire(&lib, "b"), ("queued".into(), "queued".into()));
    }
    #[test]
    fn a_settled_batch_with_no_other_outcome_is_saved() {
        let (_t, lib) = batch_with(&["written", "written"]);
        assert_eq!(settle(&lib), ("saved".into(), "saved".into()));
    }
    #[test]
    fn a_failed_cancelled_or_superseded_row_makes_the_settled_batch_partial_and_the_draft_follows()
    {
        for other in ["failed", "cancelled", "superseded"] {
            let (_t, lib) = batch_with(&["written", other]);
            assert_eq!(
                settle(&lib),
                ("partial".into(), "partial".into()),
                "{other}"
            );
        }
    }
    #[test]
    fn a_batch_still_waiting_on_a_row_does_not_settle() {
        for waiting in ["queued", "ready", "recovery_required"] {
            let (_t, lib) = batch_with(&["written", "failed", waiting]);
            assert_eq!(
                settle(&lib),
                ("queued".into(), "queued".into()),
                "{waiting}"
            );
        }
    }
}
