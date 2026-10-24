mod catalog;
pub(crate) mod store;
pub use catalog::{CleanupArtistValue, CleanupCatalog, CleanupCatalogValue};
mod journal;
mod model;
use crate::{
    Library,
    model::{now, path_bytes},
};
use anyhow::{Context, Result, ensure};
use cuerithm_cleanup::{CleanupConfig, CustomTag, Metadata};
use cuerithm_media::{MediaRuntime, file_sha256};
pub use model::*;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[derive(Clone)]
pub struct CleanupService {
    library: Library,
    runtime: MediaRuntime,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Snapshot {
    track_id: String,
    path: Vec<u8>,
    source_sha256: String,
    row: serde_json::Value,
    metadata: Metadata,
    identity: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Proposal {
    public: CleanupProposal,
    before: Snapshot,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Plan {
    public: CleanupPreview,
    proposals: Vec<Proposal>,
}
pub(crate) fn custom_tags(conn: &Connection, id: &str) -> Result<Vec<CustomTag>> {
    let mut q=conn.prepare("SELECT c.name,t.name FROM track_custom_tags l JOIN custom_tags t ON t.id=l.tag_id JOIN custom_tag_categories c ON c.id=t.category_id WHERE l.track_id=?1 ORDER BY c.name,t.name")?;
    Ok(q.query_map([id], |r| {
        Ok(CustomTag {
            category: r.get(0)?,
            value: r.get(1)?,
        })
    })?
    .collect::<rusqlite::Result<_>>()?)
}
fn row_identity(conn: &Connection, id: &str) -> Result<serde_json::Value> {
    let row:String=conn.query_row("SELECT json_object('path',hex(path),'title',title,'artist',artist,'album',album,'genre',genre,'source_hash',source_hash,'state',state,'archived',archived,'current_job',current_job,'analysis_json',analysis_json,'metadata_json',metadata_json,'musical_key',musical_key,'bpm',bpm,'energy',energy,'duration',duration,'tagging_json',tagging_json,'cleanup_json',cleanup_json,'attempt',(SELECT attempts FROM jobs WHERE id=current_job),'cues',(SELECT group_concat(id) FROM (SELECT id FROM cue_revisions WHERE track_id=tracks.id ORDER BY id))) FROM tracks WHERE id=?1",[id],|r|r.get(0))?;
    Ok(
        serde_json::json!({"row":serde_json::from_str::<serde_json::Value>(&row)?,"tags":custom_tags(conn,id)?}),
    )
}
fn identity(value: &serde_json::Value) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(value)?)))
}
impl CleanupService {
    pub fn new(library: Library, runtime: MediaRuntime) -> Self {
        Self { library, runtime }
    }
    pub fn preview_cleanup(
        &self,
        scope: CleanupScope,
        config: CleanupConfig,
    ) -> Result<CleanupPreview> {
        config.validate()?;
        let ids = {
            let conn = self.library.conn()?;
            match scope {
                CleanupScope::AllReady => {
                    let mut q=conn.prepare("SELECT id FROM tracks WHERE state='ready' AND archived=0 ORDER BY sort_title,id")?;
                    q.query_map([], |r| r.get::<_, String>(0))?
                        .collect::<rusqlite::Result<Vec<_>>>()?
                }
                CleanupScope::TrackIds(ids) => {
                    ensure!(ids.len() <= 100_000, "Too many cleanup tracks");
                    let mut seen = std::collections::BTreeSet::new();
                    ids.into_iter()
                        .filter(|id| seen.insert(id.clone()))
                        .collect()
                }
            }
        };
        let mut public = CleanupPreview {
            id: uuid::Uuid::new_v4().to_string(),
            created: now(),
            proposals: vec![],
            unchanged: vec![],
            failed: vec![],
        };
        let mut proposals = vec![];
        for id in ids {
            let proposal = (|| -> Result<Option<Proposal>> {
                let (row, path, source) = {
                    let conn = self.library.conn()?;
                    let row = row_identity(&conn, &id)?;
                    ensure!(
                        row["row"]["state"] == "ready" && row["row"]["archived"] == 0,
                        "Track is not Ready"
                    );
                    let bytes: Option<Vec<u8>> =
                        conn.query_row("SELECT path FROM tracks WHERE id=?1", [&id], |r| r.get(0))?;
                    let path =
                        crate::model::bytes_path(bytes.as_deref().context(
                            "Local file is not linked; relocate the track before cleanup",
                        )?)?;
                    let source = row["row"]["source_hash"]
                        .as_str()
                        .context("Source hash is unknown; verify the local file before cleanup")?
                        .to_owned();
                    (row, path, source)
                };
                ensure!(
                    file_sha256(&path)? == source,
                    "Source changed; rescan the track"
                );
                let info = self.runtime.probe(&path)?;
                let mut metadata = cuerithm_autotag::read_metadata(&path, &info)?;
                metadata.custom_tags = serde_json::from_value(row["tags"].clone())?;
                let report = cuerithm_cleanup::apply(&config, &metadata)?;
                ensure!(
                    file_sha256(&path)? == source,
                    "Source changed during preview"
                );
                let current = row_identity(&*self.library.conn()?, &id)?;
                ensure!(current == row, "Library changed during preview");
                if !report.changed() {
                    return Ok(None);
                }
                let proposal = CleanupProposal {
                    id: uuid::Uuid::new_v4().to_string(),
                    track_id: id.clone(),
                    title: row["row"]["title"].as_str().unwrap_or("").into(),
                    report,
                };
                Ok(Some(Proposal {
                    public: proposal,
                    before: Snapshot {
                        track_id: id.clone(),
                        path: path_bytes(&path),
                        source_sha256: source,
                        identity: identity(&row)?,
                        row,
                        metadata,
                    },
                }))
            })();
            match proposal {
                Ok(Some(p)) => {
                    public.proposals.push(p.public.clone());
                    proposals.push(p)
                }
                Ok(None) => public.unchanged.push(id),
                Err(e) => public.failed.push(CleanupFailure {
                    track_id: id,
                    message: format!("{e:#}"),
                }),
            }
        }
        let plan = Plan {
            public: public.clone(),
            proposals,
        };
        self.library.conn()?.execute(
            "INSERT INTO cleanup_plans VALUES(?1,?2,?3)",
            params![public.id, serde_json::to_string(&plan)?, public.created],
        )?;
        self.library.prune_cleanup()?;
        Ok(public)
    }
    /// Stages a copy of the file as it is now with `changes` written into its tags, checks it
    /// and keeps the private workspace for the journal. Applying a cleanup and undoing one are
    /// the same checked write: every change names the value it expects to replace, so a tag
    /// that is no longer what the journal says is refused instead of overwritten.
    fn stage_changes(
        &self,
        path: std::path::PathBuf,
        source_sha256: &str,
        changes: Vec<cuerithm_cleanup::FieldChange>,
    ) -> Result<cuerithm_autotag::StagedTagWrite> {
        let cache = self.library.data_dir().join("cleanup");
        let audio = self.runtime.decode(&path, &cache)?;
        let input = cuerithm_autotag::TagInput {
            path,
            source_sha256: source_sha256.into(),
            media: audio.info.clone(),
            duration_seconds: audio.clock.duration_seconds(),
        };
        let mut config = cuerithm_autotag::AutotagConfig::default();
        config.options.tags.clear();
        config.write_energy = false;
        let outcome = cuerithm_autotag::TagMatchOutcome {
            status: cuerithm_autotag::MatchStatus::NoMatch,
            matches: vec![],
            attempted: vec![],
            warnings: vec![],
        };
        cuerithm_autotag::TagWritePlan::build(input, &outcome, &config, &Default::default())?
            .with_metadata_edits(changes)?
            .stage_prepared(&self.runtime, &audio, &cache)
    }
    fn stage_proposal(&self, batch_id: &str, proposal: &Proposal) -> Result<String> {
        let path = crate::model::bytes_path(&proposal.before.path)?;
        let mut staged = self.stage_changes(
            path,
            &proposal.before.source_sha256,
            proposal.public.report.changes.clone(),
        )?;
        let payload = journal::Payload {
            before: proposal.before.clone(),
            metadata: staged.metadata.clone(),
            report: proposal.public.report.clone(),
            post_identity: None,
            restore_row: None,
            undo_of: None,
        };
        journal::begin(
            &self.library,
            batch_id,
            &proposal.public.id,
            &payload,
            &mut staged,
        )
    }
    pub fn apply_cleanup(
        &self,
        plan_id: &str,
        proposal_ids: &[String],
    ) -> Result<CleanupBatchResult> {
        let plan: String = self.library.conn()?.query_row(
            "SELECT payload_json FROM cleanup_plans WHERE id=?1",
            [plan_id],
            |r| r.get(0),
        )?;
        let plan: Plan = serde_json::from_str(&plan)?;
        let selected: std::collections::BTreeSet<_> = proposal_ids.iter().collect();
        ensure!(
            selected.len() == proposal_ids.len(),
            "Duplicate cleanup selection"
        );
        ensure!(
            selected
                .iter()
                .all(|id| plan.proposals.iter().any(|p| &p.public.id == *id)),
            "Unknown cleanup proposal"
        );
        let mut result = CleanupBatchResult {
            id: uuid::Uuid::new_v4().to_string(),
            ..Default::default()
        };
        self.library.conn()?.execute(
            "INSERT INTO cleanup_batches VALUES(?1,?2,NULL,'apply','running',?3,?4)",
            params![result.id, plan_id, serde_json::to_string(&result)?, now()],
        )?;
        for proposal in plan
            .proposals
            .iter()
            .filter(|p| selected.contains(&p.public.id))
        {
            let id = &proposal.public.track_id;
            if let Err(e) = journal::matches(&self.library, &proposal.before) {
                result.conflicted.push(CleanupFailure {
                    track_id: id.clone(),
                    message: format!("{e:#}"),
                });
                continue;
            }
            let apply = self
                .stage_proposal(&result.id, proposal)
                .and_then(|write| journal::publish(&self.library, &write));
            match apply {
                Ok(()) => result.applied.push(id.clone()),
                Err(e) => {
                    let failure = CleanupFailure {
                        track_id: id.clone(),
                        message: format!("{e:#}"),
                    };
                    if journal::matches(&self.library, &proposal.before).is_err() {
                        result.conflicted.push(failure)
                    } else {
                        result.failed.push(failure)
                    }
                }
            }
        }
        self.library.conn()?.execute(
            "UPDATE cleanup_batches SET state='complete',result_json=?1 WHERE id=?2",
            params![serde_json::to_string(&result)?, result.id],
        )?;
        self.library.prune_cleanup()?;
        Ok(result)
    }
    fn undo_snapshot(&self, original: &journal::Journal) -> Result<Snapshot> {
        let row = row_identity(&*self.library.conn()?, &original.payload.before.track_id)?;
        let post = original
            .payload
            .post_identity
            .as_ref()
            .context("Missing cleanup post-apply identity")?;
        ensure!(&identity(&row)? == post, "Later library changes preserved");
        let path = crate::model::bytes_path(&original.payload.before.path)?;
        let source = original
            .files
            .first()
            .map(|f| f.after.clone())
            .unwrap_or_else(|| original.payload.before.source_sha256.clone());
        ensure!(file_sha256(&path)? == source, "Later file edit preserved");
        Ok(Snapshot {
            track_id: original.payload.before.track_id.clone(),
            path: original.payload.before.path.clone(),
            source_sha256: source,
            row,
            metadata: original.payload.report.metadata.clone(),
            identity: post.clone(),
        })
    }
    /// Undo writes the tag values the journal recorded before the batch into the file as it is
    /// now, through the same checked write as the batch itself. No copy of the original audio is
    /// involved, so none has to be kept.
    fn stage_undo(
        &self,
        batch_id: &str,
        write: &str,
        original: &journal::Journal,
        before: &Snapshot,
    ) -> Result<String> {
        let changes: Vec<_> = original
            .payload
            .report
            .changes
            .iter()
            .map(|c| cuerithm_cleanup::FieldChange {
                field: c.field,
                before: c.after.clone(),
                after: c.before.clone(),
            })
            .collect();
        let mut staged = self.stage_changes(
            crate::model::bytes_path(&before.path)?,
            &before.source_sha256,
            changes.clone(),
        )?;
        let report = cuerithm_cleanup::CleanupReport {
            original: before.metadata.clone(),
            metadata: original.payload.before.metadata.clone(),
            changes,
            applied_rules: vec!["undo".into()],
        };
        let payload = journal::Payload {
            before: before.clone(),
            metadata: staged.metadata.clone(),
            report,
            post_identity: None,
            restore_row: Some(original.payload.before.row.clone()),
            undo_of: Some(original.id.clone()),
        };
        journal::begin(&self.library, batch_id, write, &payload, &mut staged)
    }
    pub fn undo_cleanup(&self, batch_id: &str) -> Result<CleanupBatchResult> {
        let writes = {
            let conn = self.library.conn()?;
            let kind: String = conn.query_row(
                "SELECT kind FROM cleanup_batches WHERE id=?1",
                [batch_id],
                |r| r.get(0),
            )?;
            ensure!(
                kind == "apply",
                "Only applied cleanup batches can be undone"
            );
            let mut q=conn.prepare("SELECT id FROM cleanup_writes WHERE batch_id=?1 AND state IN ('committed','undone') ORDER BY created,id")?;
            q.query_map([batch_id], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let mut result = CleanupBatchResult {
            id: uuid::Uuid::new_v4().to_string(),
            ..Default::default()
        };
        self.library.conn()?.execute(
            "INSERT INTO cleanup_batches VALUES(?1,NULL,?2,'undo','running',?3,?4)",
            params![result.id, batch_id, serde_json::to_string(&result)?, now()],
        )?;
        for write in writes {
            let original = journal::load(&self.library, &write)?;
            let id = original.payload.before.track_id.clone();
            if original.state == "undone" {
                result.unchanged.push(id);
                continue;
            }
            let before = self.undo_snapshot(&original);
            let before = match before {
                Ok(value) => value,
                Err(e) => {
                    result.conflicted.push(CleanupFailure {
                        track_id: id,
                        message: format!("{e:#}"),
                    });
                    continue;
                }
            };
            let undo = self
                .stage_undo(&result.id, &write, &original, &before)
                .and_then(|write| journal::publish(&self.library, &write));
            match undo {
                Ok(()) => result.applied.push(id),
                Err(e) => {
                    let failure = CleanupFailure {
                        track_id: id,
                        message: format!("{e:#}"),
                    };
                    if journal::matches(&self.library, &before).is_err() {
                        result.conflicted.push(failure)
                    } else {
                        result.failed.push(failure)
                    }
                }
            }
        }
        self.library.conn()?.execute(
            "UPDATE cleanup_batches SET state='complete',result_json=?1 WHERE id=?2",
            params![serde_json::to_string(&result)?, result.id],
        )?;
        Ok(result)
    }
    pub fn cleanup_history(&self) -> Result<Vec<CleanupHistory>> {
        let conn = self.library.conn()?;
        let mut q=conn.prepare("SELECT b.id,b.kind,b.state,b.created,b.result_json,EXISTS(SELECT 1 FROM cleanup_writes w WHERE w.batch_id=b.id AND w.state='committed') FROM cleanup_batches b ORDER BY b.created DESC,b.id")?;
        let rows = q
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, bool>(5)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|(id, kind, state, created, result, can_undo)| {
                Ok(CleanupHistory {
                    can_undo,
                    id,
                    kind,
                    state,
                    created,
                    result: serde_json::from_str(&result)?,
                })
            })
            .collect()
    }
}

/// How many cleanup previews stay: a preview holds a row of the library for every track it
/// proposes a change to, and only the one the view shows is ever applied.
const KEEP_PLANS: usize = 3;

impl Library {
    /// Bounds what cleanup keeps. Previews beyond the newest few go. Only the newest
    /// `REMOVAL_UNDO_LIMIT` applied batches keep what Undo needs; an older batch keeps its history
    /// line (what changed, how many tracks) and loses its Undo. Run when a preview or a batch is
    /// saved and when the library opens, so a library from an earlier version is trimmed once.
    pub(crate) fn prune_cleanup(&self) -> Result<()> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        let old_plans: Vec<String> = tx
            .prepare(
                "SELECT id FROM cleanup_plans ORDER BY created DESC,rowid DESC LIMIT -1 OFFSET ?1",
            )?
            .query_map([KEEP_PLANS as i64], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        for id in old_plans {
            tx.execute(
                "UPDATE cleanup_batches SET plan_id=NULL WHERE plan_id=?1",
                [&id],
            )?;
            tx.execute("DELETE FROM cleanup_plans WHERE id=?1", [&id])?;
        }
        // Applied batches that can still be undone, newest first; the rest lose their journals.
        let expired: Vec<String> = tx
            .prepare(
                "SELECT b.id FROM cleanup_batches b
                  WHERE b.kind='apply' AND EXISTS(SELECT 1 FROM cleanup_writes w WHERE w.batch_id=b.id AND w.state='committed')
                  ORDER BY b.created DESC,b.rowid DESC LIMIT -1 OFFSET ?1",
            )?
            .query_map([crate::REMOVAL_UNDO_LIMIT as i64], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let mut files = vec![];
        for batch in expired {
            // The journals of the batch and of every Undo made from it, once they have settled.
            let writes: Vec<String> = tx
                .prepare(
                    "SELECT w.id FROM cleanup_writes w WHERE w.cleanup_complete=1 AND w.state IN ('committed','undone','rolled_back')
                       AND (w.batch_id=?1 OR w.batch_id IN (SELECT id FROM cleanup_batches WHERE source_batch_id=?1))",
                )?
                .query_map([&batch], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            for write in writes {
                for backup in tx
                    .prepare("SELECT backup FROM cleanup_write_files WHERE write_id=?1 AND backup IS NOT NULL")?
                    .query_map([&write], |r| r.get::<_, Vec<u8>>(0))?
                {
                    files.push(backup?);
                }
                tx.execute(
                    "DELETE FROM cleanup_write_files WHERE write_id=?1",
                    [&write],
                )?;
                tx.execute("DELETE FROM cleanup_writes WHERE id=?1", [&write])?;
            }
        }
        tx.commit()?;
        // A journal from an earlier version may still name an original; it goes with it.
        drop(conn);
        crate::legacy_originals::remove_owned_originals(self, "cleanup", &files);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
