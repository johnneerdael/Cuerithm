use super::{
    deletion,
    files::{FileProof, ensure_available, proofs_current},
    review::{SavedReview, read_review, refresh},
    scan::stamp,
    *,
};
use crate::Library;
use anyhow::{Context, Result, ensure};
use cuerithm_maintenance::{MergeDisposition, MergePlan};
use cuerithm_media::CancellationToken;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Serialize, Deserialize)]
pub(super) struct ArchiveChange {
    pub(super) id: String,
    pub(super) before: bool,
    pub(super) after: bool,
    pub(super) before_updated: i64,
    pub(super) after_updated: i64,
}
#[derive(Serialize, Deserialize)]
pub(super) struct SavedPreview {
    pub(super) public: DuplicateMergePreview,
    pub(super) review: SavedReview,
    pub(super) archives: Vec<ArchiveChange>,
    pub(super) consumed_by: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct Operation {
    schema: String,
    result: DuplicateMergeResult,
    archives: Vec<ArchiveChange>,
    playlists: Vec<DuplicatePlaylistChange>,
    files: Vec<FileProof>,
    logic: String,
    undone_by: Option<String>,
}
pub(super) fn load<T: DeserializeOwned>(conn: &Connection, id: &str, kind: &str) -> Result<T> {
    let text: String = conn
        .query_row(
            "SELECT payload_json FROM maintenance_scans WHERE id=?1 AND kind=?2",
            params![id, kind],
            |r| r.get(0),
        )
        .optional()?
        .context("Duplicate merge artifact not found")?;
    Ok(serde_json::from_str(&text)?)
}
fn members(conn: &Connection, id: &str) -> Result<Vec<String>> {
    let mut stmt = conn
        .prepare("SELECT track_id FROM playlist_members WHERE playlist_id=?1 ORDER BY position")?;
    Ok(stmt
        .query_map([id], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?)
}
pub(super) fn playlist_changes(
    conn: &Connection,
    plan: &MergePlan,
) -> Result<Vec<DuplicatePlaylistChange>> {
    let mut stmt = conn.prepare("SELECT id FROM playlists WHERE kind='playlist' ORDER BY id")?;
    let ids = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut replaced = BTreeMap::new();
    for group in &plan.groups {
        for id in group.replaced_ids() {
            ensure!(
                replaced.insert(id, &group.keeper).is_none(),
                "Overlapping replacements"
            );
        }
    }
    let mut result = vec![];
    for id in ids {
        let before = members(conn, &id)?;
        let affected = before
            .iter()
            .filter_map(|id| replaced.get(id))
            .map(|s| s.as_str())
            .collect::<BTreeSet<_>>();
        let mut seen = BTreeSet::new();
        let mut after = vec![];
        for member in &before {
            if let Some(keeper) = replaced.get(member) {
                if seen.insert(keeper.as_str()) {
                    after.push((*keeper).clone());
                }
            } else if !affected.contains(member.as_str()) {
                after.push(member.clone());
            }
        }
        if before != after {
            result.push(DuplicatePlaylistChange { id, before, after });
        }
    }
    Ok(result)
}
pub(super) fn write_members(
    conn: &Connection,
    changes: &[DuplicatePlaylistChange],
    undo: bool,
) -> Result<()> {
    for change in changes {
        conn.execute(
            "DELETE FROM playlist_members WHERE playlist_id=?1",
            [&change.id],
        )?;
        let rows = if undo { &change.before } else { &change.after };
        for (position, id) in rows.iter().enumerate() {
            conn.execute(
                "INSERT INTO playlist_members VALUES(?1,?2,?3)",
                params![change.id, id, position as u64],
            )?;
        }
    }
    if !changes.is_empty() {
        conn.execute(
            "UPDATE meta SET value=value+1 WHERE key='playlist_revision'",
            [],
        )?;
    }
    conn.execute(
        "UPDATE smart_playlist_status SET state='stale' WHERE state='ready'",
        [],
    )?;
    Ok(())
}
fn archives(conn: &Connection, plan: &MergePlan) -> Result<Vec<ArchiveChange>> {
    let mut desired = BTreeMap::new();
    for group in &plan.groups {
        for id in &group.archive_ids {
            desired.insert(id, true);
        }
        if group.unarchive_keeper {
            desired.insert(&group.keeper, false);
        }
    }
    let mut changes = vec![];
    for (id, after) in desired {
        let (before, updated): (bool, i64) = conn.query_row(
            "SELECT archived,updated FROM tracks WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if before != after {
            changes.push(ArchiveChange {
                id: id.clone(),
                before,
                after,
                before_updated: updated,
                after_updated: 0,
            });
        }
    }
    Ok(changes)
}
pub(super) fn check_review(conn: &Connection, preview: &DuplicateMergePreview) -> Result<()> {
    let revision:u64=conn.query_row("SELECT json_extract(payload_json,'$.review.revision') FROM maintenance_scans WHERE id=?1 AND kind='duplicate_review'",[&preview.review_id],|r|r.get(0))?;
    ensure!(
        revision == preview.review_revision,
        "Duplicate review changed; preview again"
    );
    Ok(())
}
pub(super) fn advance(conn: &Connection) -> Result<MaintenanceStamp> {
    conn.execute(
        "UPDATE meta SET value=value+1 WHERE key='maintenance_revision'",
        [],
    )?;
    Library::bump(conn)?;
    stamp(conn)
}
impl Library {
    pub fn duplicate_merge_history(&self) -> Result<Vec<DuplicateMergeHistory>> {
        let conn = self.conn()?;
        let mut q=conn.prepare("SELECT result,created,undone,expired FROM (SELECT json_extract(payload_json,'$.result') AS result,created,json_extract(payload_json,'$.undone_by') AS undone,0 AS expired,id FROM maintenance_scans WHERE kind='duplicate_merge' UNION ALL SELECT json_extract(payload_json,'$.result'),created,json_extract(payload_json,'$.undone_by'),COALESCE(json_extract(payload_json,'$.undo_expired'),0),id FROM maintenance_writes WHERE kind='apply' AND state IN ('committed','undone')) ORDER BY created DESC,id DESC")?;
        let rows = q
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|(json, created, undone_by, expired)| {
                Ok(DuplicateMergeHistory {
                    result: serde_json::from_str(&json)?,
                    created,
                    undone_by,
                    undo_expired: expired != 0,
                    undo_limit: DUPLICATE_UNDO_LIMIT,
                })
            })
            .collect()
    }
    /// Preview an Archive-only merge. See [`Library::preview_duplicate_merge_choice`].
    pub fn preview_duplicate_merge(
        &self,
        review_id: &str,
        expected_revision: u64,
        groups: Vec<String>,
        cancel: &CancellationToken,
    ) -> Result<DuplicateMergePreview> {
        self.preview_duplicate_merge_choice(
            review_id,
            expected_revision,
            groups,
            Some(MergeDisposition::Archive),
            cancel,
        )
    }
    /// Preview what a merge does to the other tracks of each selected group. The choice
    /// between deleting their files and archiving their records is the user's: with no
    /// choice there is no preview, and so nothing that can be applied.
    pub fn preview_duplicate_merge_choice(
        &self,
        review_id: &str,
        expected_revision: u64,
        groups: Vec<String>,
        choice: Option<MergeDisposition>,
        cancel: &CancellationToken,
    ) -> Result<DuplicateMergePreview> {
        self.preview_duplicate_merge_choice_with(
            review_id,
            expected_revision,
            groups,
            choice,
            &SystemTrash,
            cancel,
        )
    }
    /// As [`Library::preview_duplicate_merge_choice`], with the Trash that decides which
    /// files would have to be deleted permanently (tests supply their own).
    pub fn preview_duplicate_merge_choice_with(
        &self,
        review_id: &str,
        expected_revision: u64,
        groups: Vec<String>,
        choice: Option<MergeDisposition>,
        trash: &dyn FileTrash,
        cancel: &CancellationToken,
    ) -> Result<DuplicateMergePreview> {
        let disposition =
            choice.context("Choose Delete from disk or Archive only before previewing")?;
        cancel.check()?;
        let (mut review, _) = read_review(self, review_id)?;
        ensure!(
            review.review.revision == expected_revision,
            "Duplicate review changed; reload review"
        );
        refresh(self, &mut review, cancel)?;
        let archived = review
            .scan
            .report
            .tracks
            .iter()
            .filter(|t| t.archived)
            .map(|t| t.id.clone())
            .collect();
        let mut plan =
            MergePlan::build_with(&review.review.groups, &groups, &archived, disposition)?;
        // Files can vanish after a scan. Name the missing tracks before the generic stale
        // refusal, so the message tells the user where to look.
        ensure_available(&*self.conn()?, &plan.participants())?;
        ensure!(
            !review.review.stale,
            "Duplicate review is stale; scan again"
        );
        let deletion = if disposition == MergeDisposition::Delete {
            Some(deletion::plan(
                self,
                &review.scan,
                &mut plan,
                trash,
                cancel,
            )?)
        } else {
            None
        };
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        ensure!(
            stamp(&tx)? == review.review.stamp,
            "Library changed; scan again"
        );
        let public = DuplicateMergePreview {
            id: uuid::Uuid::new_v4().to_string(),
            review_id: review_id.into(),
            review_revision: expected_revision,
            created: crate::now(),
            stamp: review.review.stamp.clone(),
            playlists: playlist_changes(&tx, &plan)?,
            plan,
            deletion,
        };
        check_review(&tx, &public)?;
        let saved = SavedPreview {
            archives: archives(&tx, &public.plan)?,
            public: public.clone(),
            review,
            consumed_by: None,
        };
        cancel.check()?;
        tx.execute(
            "INSERT INTO maintenance_scans VALUES(?1,'duplicate_merge_plan',?2,?3)",
            params![public.id, serde_json::to_string(&saved)?, public.created],
        )?;
        super::saved::prune(&tx)?;
        tx.commit()?;
        Ok(public)
    }
    /// Archive/reference publication stage. Donor plans require the coordinated file
    /// journal before any part can be applied; they never fall back to library-only copies.
    pub fn apply_duplicate_merge(
        &self,
        id: &str,
        cancel: &CancellationToken,
    ) -> Result<DuplicateMergeResult> {
        cancel.check()?;
        let mut saved: SavedPreview = {
            let conn = self.conn()?;
            load(&conn, id, "duplicate_merge_plan")?
        };
        ensure!(
            saved.consumed_by.is_none(),
            "Duplicate preview already applied"
        );
        ensure!(
            saved.public.plan.disposition == MergeDisposition::Archive,
            "Deleting files needs the coordinated merge service; this preview cannot be applied here"
        );
        ensure!(
            saved.public.plan.groups.iter().all(|g| g.copies.is_empty()),
            "Tag/cue transfer requires coordinated file publication; this preview cannot be applied yet"
        );
        refresh(self, &mut saved.review, cancel)?;
        ensure_available(&*self.conn()?, &saved.public.plan.participants())?;
        ensure!(
            !saved.review.review.stale,
            "Duplicate merge preview is stale; scan again"
        );
        let mut conn = self.conn()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure!(
            stamp(&tx)? == saved.public.stamp,
            "Library changed; scan again"
        );
        ensure!(
            saved.review.scan.scope.time_current(crate::now()),
            "Smart scope expired; scan again"
        );
        check_review(&tx, &saved.public)?;
        ensure_available(&tx, &saved.public.plan.participants())?;
        let mut current: SavedPreview = load(&tx, id, "duplicate_merge_plan")?;
        ensure!(
            current.consumed_by.is_none(),
            "Duplicate preview already applied"
        );
        ensure!(
            playlist_changes(&tx, &saved.public.plan)? == saved.public.playlists,
            "Playlist references changed; preview again"
        );
        let now = crate::now();
        for change in &mut saved.archives {
            let (archived, updated): (bool, i64) = tx.query_row(
                "SELECT archived,updated FROM tracks WHERE id=?1",
                [&change.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            ensure!(
                archived == change.before && updated == change.before_updated,
                "Track changed; preview again"
            );
            change.after_updated = now;
            tx.execute(
                "UPDATE tracks SET archived=?1,updated=?2 WHERE id=?3",
                params![change.after, now, change.id],
            )?;
        }
        write_members(&tx, &saved.public.playlists, false)?;
        let result = DuplicateMergeResult {
            id: uuid::Uuid::new_v4().to_string(),
            preview_id: id.into(),
            stamp: advance(&tx)?,
            archived_ids: saved
                .archives
                .iter()
                .filter(|a| a.after)
                .map(|a| a.id.clone())
                .collect(),
            restored_ids: saved
                .archives
                .iter()
                .filter(|a| !a.after)
                .map(|a| a.id.clone())
                .collect(),
            affected_playlists: saved
                .public
                .playlists
                .iter()
                .map(|p| p.id.clone())
                .collect(),
            undo_of: None,
            updated_ids: vec![],
            removed_ids: vec![],
            trashed: vec![],
            permanently_deleted: vec![],
            not_deleted: vec![],
            links_not_restored: 0,
        };
        current.consumed_by = Some(result.id.clone());
        let operation = Operation {
            schema: "duplicate-library-merge-v1".into(),
            result: result.clone(),
            archives: saved.archives,
            playlists: saved.public.playlists,
            files: saved.review.scan.files,
            logic: super::scan::logic_identity(),
            undone_by: None,
        };
        cancel.check()?;
        tx.execute(
            "INSERT INTO maintenance_scans VALUES(?1,'duplicate_merge',?2,?3)",
            params![result.id, serde_json::to_string(&operation)?, now],
        )?;
        tx.execute(
            "UPDATE maintenance_scans SET payload_json=?1 WHERE id=?2",
            params![serde_json::to_string(&current)?, id],
        )?;
        tx.commit()?;
        Ok(result)
    }
    pub fn undo_duplicate_merge(
        &self,
        id: &str,
        cancel: &CancellationToken,
    ) -> Result<DuplicateMergeResult> {
        cancel.check()?;
        let mut operation: Operation = {
            let conn = self.conn()?;
            load(&conn, id, "duplicate_merge")?
        };
        ensure!(
            operation.undone_by.is_none(),
            "Duplicate merge already undone"
        );
        ensure!(
            operation.schema == "duplicate-library-merge-v1",
            "Unsupported duplicate merge journal; review recovery"
        );
        ensure!(
            proofs_current(&operation.files, cancel)?,
            "Source files changed; Undo needs review"
        );
        let mut conn = self.conn()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure!(
            stamp(&tx)? == operation.result.stamp,
            "Later library or playlist changes prevent Undo"
        );
        let current: Operation = load(&tx, id, "duplicate_merge")?;
        ensure!(
            current.undone_by.is_none(),
            "Duplicate merge already undone"
        );
        for change in &operation.archives {
            let actual: (bool, i64) = tx.query_row(
                "SELECT archived,updated FROM tracks WHERE id=?1",
                [&change.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            ensure!(
                actual == (change.after, change.after_updated),
                "Later track changes prevent Undo"
            );
        }
        for change in &operation.playlists {
            ensure!(
                members(&tx, &change.id)? == change.after,
                "Later playlist changes prevent Undo"
            );
        }
        for change in &operation.archives {
            tx.execute(
                "UPDATE tracks SET archived=?1,updated=?2 WHERE id=?3",
                params![change.before, change.before_updated, change.id],
            )?;
        }
        write_members(&tx, &operation.playlists, true)?;
        let result = DuplicateMergeResult {
            id: uuid::Uuid::new_v4().to_string(),
            preview_id: operation.result.preview_id.clone(),
            stamp: advance(&tx)?,
            archived_ids: operation
                .archives
                .iter()
                .filter(|a| a.before)
                .map(|a| a.id.clone())
                .collect(),
            restored_ids: operation
                .archives
                .iter()
                .filter(|a| !a.before)
                .map(|a| a.id.clone())
                .collect(),
            affected_playlists: operation.playlists.iter().map(|p| p.id.clone()).collect(),
            undo_of: Some(id.into()),
            updated_ids: vec![],
            removed_ids: vec![],
            trashed: vec![],
            permanently_deleted: vec![],
            not_deleted: vec![],
            links_not_restored: 0,
        };
        operation.undone_by = Some(result.id.clone());
        cancel.check()?;
        tx.execute(
            "UPDATE maintenance_scans SET payload_json=?1 WHERE id=?2",
            params![serde_json::to_string(&operation)?, id],
        )?;
        tx.execute(
            "INSERT INTO maintenance_scans VALUES(?1,'duplicate_merge_undo',?2,?3)",
            params![result.id, serde_json::to_string(&result)?, crate::now()],
        )?;
        tx.commit()?;
        Ok(result)
    }
}
