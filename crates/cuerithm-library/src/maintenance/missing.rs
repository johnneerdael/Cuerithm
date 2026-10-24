//! Repairs for library records whose audio file is no longer there.
//!
//! Two actions work on the results of a "Missing & broken" scan, both as
//! preview, apply, history entry and undo:
//! - remove the library records of unavailable tracks (never a file);
//! - find the moved files by their SHA-256 and point the tracks at them.
//!
//! Nothing here reads a tag, a file name or a fingerprint as identity: a file is
//! "the same" only when its bytes hash to the track's stored source SHA-256.
use super::{
    merge::{advance, write_members},
    *,
};
use super::{purge, track_removal, undo_window};
use crate::{
    Library,
    model::{bytes_path, now, path_bytes},
    record_snapshot::RecordImage,
    relink::{Move, Scope, is_gone, stamp, work_in_flight},
};
use anyhow::{Context, Result, ensure};
use cuerithm_media::{CancellationToken, file_sha256_cancellable};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

/// More records than this are removed in several steps, so one history entry
/// stays small enough to hold everything needed to undo it.
pub const MISSING_REPAIR_LIMIT: usize = 1000;
/// Removals whose saved records are kept for Undo. Older ones are discarded when a
/// newer removal is applied; their history lines stay, without the track details. The
/// bound is shared with duplicate merges that delete files: see [`undo_window`].
pub const MISSING_UNDO_WINDOW: usize = undo_window::REMOVAL_UNDO_LIMIT;
pub(super) const PREVIEW_REMOVAL: &str = "missing_removal_preview";
pub(super) const PREVIEW_RELINK: &str = "missing_relink_preview";
pub(super) const OPERATION: &str = "missing_repair";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissingRepairKind {
    Remove,
    Relink,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissingRowOutcome {
    /// Will be changed when the preview is applied.
    Ready,
    /// Must stay as it is; the reason says why.
    Blocked,
    NotFound,
    Ambiguous,
    /// The matching file already belongs to another library track.
    AlreadyTracked,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissingRow {
    pub track_id: String,
    pub title: String,
    pub artist: String,
    pub archived: bool,
    pub display_path: String,
    pub file_state: MaintenanceFileState,
    pub outcome: MissingRowOutcome,
    pub reason: Option<String>,
    /// Where a relink would point the track.
    pub new_path: Option<String>,
    pub playlists: usize,
    pub sync_links: usize,
    pub rekordbox_links: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AffectedPlaylist {
    pub id: String,
    pub name: String,
    pub tracks: usize,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MissingImpact {
    /// Playlists that lose tracks and the number each loses.
    pub playlists: Vec<AffectedPlaylist>,
    pub playlist_memberships: usize,
    /// Links to DJ software that CueRithm keeps for these tracks (removed, and put back by Undo).
    pub sync_links: usize,
    /// rekordbox tracks matched to these records. They are left in place and unlinked.
    pub rekordbox_links: usize,
    pub archived: usize,
    /// Rows whose folder or drive could not be reached at all.
    pub unreachable: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissingPreview {
    pub id: String,
    pub kind: MissingRepairKind,
    pub scan_id: String,
    pub created: i64,
    pub rows: Vec<MissingRow>,
    pub ready: usize,
    pub impact: MissingImpact,
    /// What a relink searched.
    pub locations: Vec<String>,
    pub files_checked: usize,
    /// How many removals keep their saved records for Undo, duplicate merges that delete
    /// files and these removals counted together.
    #[serde(default = "undo_limit")]
    pub undo_limit: usize,
}
fn undo_limit() -> usize {
    MISSING_UNDO_WINDOW
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairedTrack {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub archived: bool,
    pub display_path: String,
    pub new_path: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkippedTrack {
    pub id: String,
    pub title: String,
    pub reason: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissingRepairEntry {
    pub id: String,
    pub kind: MissingRepairKind,
    pub preview_id: Option<String>,
    pub created: i64,
    pub undo_of: Option<String>,
    /// Tracks changed. `tracks` lists them until the removal's saved records are discarded.
    #[serde(default)]
    pub track_count: usize,
    pub tracks: Vec<RepairedTrack>,
    pub skipped: Vec<SkippedTrack>,
    /// Things worth knowing about what just happened, in plain words.
    #[serde(default)]
    pub notes: Vec<String>,
    pub playlist_ids: Vec<String>,
    pub playlist_memberships: usize,
    pub sync_links: usize,
    pub rekordbox_links: usize,
    pub stamp: MaintenanceStamp,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissingRepairHistory {
    pub entry: MissingRepairEntry,
    pub undone_by: Option<String>,
    /// The saved records were explicitly discarded, so the removal can no longer be undone.
    #[serde(default)]
    pub undo_discarded: bool,
    /// How many removals keep their saved records for Undo, duplicate merges that delete
    /// files and these removals counted together.
    #[serde(default = "undo_limit")]
    pub undo_limit: usize,
}

#[derive(Serialize, Deserialize)]
struct Match {
    track_id: String,
    from: Vec<u8>,
    to: Vec<u8>,
    hash: String,
    /// Owned artwork rows that followed the track, as (old path, new path).
    #[serde(default)]
    sidecars: Vec<(Vec<u8>, Vec<u8>)>,
}
#[derive(Serialize, Deserialize)]
struct SavedPreview {
    public: MissingPreview,
    epochs: BTreeMap<String, i64>,
    matches: Vec<Match>,
}
#[derive(Serialize, Deserialize, Default)]
struct Snapshot {
    image: RecordImage,
    playlists: Vec<DuplicatePlaylistChange>,
    relinks: Vec<Match>,
}
#[derive(Serialize, Deserialize)]
struct Operation {
    entry: MissingRepairEntry,
    snapshot: Snapshot,
    undone_by: Option<String>,
    #[serde(default)]
    undo_discarded: bool,
    /// Source hashes whose cached cover was kept because Undo may need it. With the
    /// audio missing, that cache can be the only copy of the picture left.
    #[serde(default)]
    artwork_hashes: Vec<String>,
}

/// Held while records or paths change, in the order admission takes them.
struct Gates<'a> {
    _organization: std::sync::MutexGuard<'a, ()>,
    #[cfg(feature = "autotag")]
    _tag: std::sync::MutexGuard<'a, ()>,
}
struct Record {
    id: String,
    title: String,
    artist: String,
    archived: bool,
    path: PathBuf,
    hash: String,
    epoch: i64,
}
fn record(conn: &Connection, id: &str) -> Result<Option<Record>> {
    type Row = (String, String, bool, Vec<u8>, String, Option<i64>);
    let row: Option<Row> = conn
        .query_row(
            "SELECT t.title,t.artist,t.archived,t.path,t.source_hash,e.epoch FROM tracks t LEFT JOIN maintenance_track_epochs e ON e.id=t.id WHERE t.id=?1 AND t.path IS NOT NULL AND t.source_hash IS NOT NULL",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .optional()?;
    row.map(|(title, artist, archived, path, hash, epoch)| {
        Ok(Record {
            id: id.into(),
            title,
            artist,
            archived,
            path: bytes_path(&path)?,
            hash,
            epoch: epoch.unwrap_or(0),
        })
    })
    .transpose()
}
fn count(conn: &Connection, sql: &str, id: &str) -> Result<usize> {
    Ok(conn.query_row(sql, [id], |r| r.get::<_, i64>(0))? as usize)
}
fn load<T: DeserializeOwned>(conn: &Connection, id: &str, kind: &str) -> Result<T> {
    let text: String = conn
        .query_row(
            "SELECT payload_json FROM maintenance_scans WHERE id=?1 AND kind=?2",
            params![id, kind],
            |r| r.get(0),
        )
        .optional()?
        .context(
            "This preview or record is gone (it may already have been applied); preview again",
        )?;
    Ok(serde_json::from_str(&text)?)
}
fn playlist_name(conn: &Connection, id: &str) -> String {
    conn.query_row("SELECT name FROM playlists WHERE id=?1", [id], |r| r.get(0))
        .unwrap_or_else(|_| id.into())
}
fn members(conn: &Connection, id: &str) -> Result<Vec<String>> {
    Ok(conn
        .prepare("SELECT track_id FROM playlist_members WHERE playlist_id=?1 ORDER BY position")?
        .query_map([id], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?)
}
/// What each playlist holds before and after `gone` leave it.
fn playlist_changes(
    conn: &Connection,
    gone: &BTreeSet<String>,
) -> Result<Vec<DuplicatePlaylistChange>> {
    let ids: Vec<String> = conn
        .prepare("SELECT DISTINCT playlist_id FROM playlist_members ORDER BY playlist_id")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let mut changes = Vec::new();
    for id in ids {
        let before = members(conn, &id)?;
        let after: Vec<String> = before
            .iter()
            .filter(|t| !gone.contains(*t))
            .cloned()
            .collect();
        if before != after {
            changes.push(DuplicatePlaylistChange { id, before, after });
        }
    }
    Ok(changes)
}
fn impact_of(conn: &Connection, rows: &[MissingRow]) -> Result<MissingImpact> {
    let ready: BTreeSet<String> = rows
        .iter()
        .filter(|r| r.outcome == MissingRowOutcome::Ready)
        .map(|r| r.track_id.clone())
        .collect();
    let mut impact = MissingImpact::default();
    for change in playlist_changes(conn, &ready)? {
        let lost = change.before.len() - change.after.len();
        impact.playlist_memberships += lost;
        impact.playlists.push(AffectedPlaylist {
            name: playlist_name(conn, &change.id),
            id: change.id,
            tracks: lost,
        });
    }
    for row in rows
        .iter()
        .filter(|r| r.outcome == MissingRowOutcome::Ready)
    {
        impact.sync_links += row.sync_links;
        impact.rekordbox_links += row.rekordbox_links;
        impact.archived += usize::from(row.archived);
        impact.unreachable += usize::from(row.file_state == MaintenanceFileState::Unavailable);
    }
    Ok(impact)
}

fn row_for(conn: &Connection, id: &str, state: MaintenanceFileState) -> Result<MissingRow> {
    let Some(r) = record(conn, id)? else {
        return Ok(MissingRow {
            track_id: id.into(),
            title: String::new(),
            artist: String::new(),
            archived: false,
            display_path: String::new(),
            file_state: state,
            outcome: MissingRowOutcome::Blocked,
            reason: Some("The track is no longer in the library".into()),
            new_path: None,
            playlists: 0,
            sync_links: 0,
            rekordbox_links: 0,
        });
    };
    Ok(MissingRow {
        track_id: r.id.clone(),
        title: r.title,
        artist: r.artist,
        archived: r.archived,
        display_path: r.path.to_string_lossy().into_owned(),
        file_state: state,
        outcome: MissingRowOutcome::Ready,
        reason: None,
        new_path: None,
        playlists: count(
            conn,
            "SELECT COUNT(*) FROM playlist_members WHERE track_id=?1",
            id,
        )?,
        sync_links: count(
            conn,
            "SELECT COUNT(*) FROM sync_links WHERE track_id=?1",
            id,
        )? + count(
            conn,
            "SELECT COUNT(*) FROM native_sync_track_ownership WHERE local_track_id=?1",
            id,
        )?,
        rekordbox_links: count(
            conn,
            "SELECT COUNT(*) FROM native_track_links WHERE local_track_id=?1",
            id,
        )?,
    })
}
fn block(row: &mut MissingRow, reason: impl Into<String>) {
    row.outcome = MissingRowOutcome::Blocked;
    row.reason = Some(reason.into());
}
/// A track can only be repaired while its file is truly not there.
fn unavailable_reason(conn: &Connection, row: &MissingRow, path: &Path) -> Result<Option<String>> {
    if !matches!(
        row.file_state,
        MaintenanceFileState::Missing | MaintenanceFileState::Unavailable
    ) {
        return Ok(Some("The file is available".into()));
    }
    if !is_gone(path) {
        return Ok(Some("The file is at its recorded location".into()));
    }
    Ok(work_in_flight(conn, &row.track_id)?.map(str::to_owned))
}

impl Operation {
    /// Drops what Undo would need, and the names of the tracks.
    /// Returns the covers that were being kept for it.
    fn discard_saved(&mut self) -> Vec<String> {
        self.snapshot = Snapshot::default();
        self.undo_discarded = true;
        self.entry.tracks.clear();
        self.entry.skipped.clear();
        std::mem::take(&mut self.artwork_hashes)
    }
}
/// Drops the saved records of a removal that has fallen out of the shared Undo window (see
/// [`undo_window`]). Its history line stays, without the track details. Returns the covers it
/// was holding.
pub(super) fn drop_saved_records(conn: &Connection, id: &str) -> Result<Vec<String>> {
    let mut operation: Operation = load(conn, id, OPERATION)?;
    let covers = operation.discard_saved();
    conn.execute(
        "UPDATE maintenance_scans SET payload_json=?1 WHERE id=?2",
        params![serde_json::to_string(&operation)?, id],
    )?;
    Ok(covers)
}

impl Library {
    /// The saved Missing & broken report, if the library has not changed since.
    /// A report only says which tracks to look at; every track is checked against
    /// the disk and the database again when it is previewed and when it is applied,
    /// so the whole library's files are not hashed again here.
    fn current_broken_scan(&self, scan_id: &str) -> Result<BrokenScanReport> {
        #[derive(Deserialize)]
        struct Saved {
            report: BrokenScanReport,
        }
        let conn = self.conn()?;
        let text: String = conn
            .query_row(
                "SELECT payload_json FROM maintenance_scans WHERE id=?1 AND kind='broken'",
                [scan_id],
                |r| r.get(0),
            )
            .optional()?
            .context("Broken scan not found")?;
        let report = serde_json::from_str::<Saved>(&text)?.report;
        ensure!(
            !report.stale && super::scan::stamp(&conn)? == report.stamp,
            "Library or maintenance inputs changed since this report; scan again"
        );
        Ok(report)
    }
    fn chosen(
        report: &BrokenScanReport,
        selected: &[String],
    ) -> Result<Vec<(String, MaintenanceFileState)>> {
        ensure!(!selected.is_empty(), "Select at least one track");
        ensure!(
            selected.len() <= MISSING_REPAIR_LIMIT,
            "Select at most {MISSING_REPAIR_LIMIT} tracks at a time"
        );
        let states: BTreeMap<&str, MaintenanceFileState> = report
            .tracks
            .iter()
            .map(|t| (t.id.as_str(), t.state))
            .collect();
        let mut seen = BTreeSet::new();
        let mut chosen = Vec::new();
        for id in selected {
            let state = states
                .get(id.as_str())
                .context("A selected track is not part of this report")?;
            if seen.insert(id.clone()) {
                chosen.push((id.clone(), *state));
            }
        }
        Ok(chosen)
    }
    fn save_preview(&self, saved: &SavedPreview, kind: &str) -> Result<()> {
        let conn = self.conn()?;
        // Previews that were never applied do not accumulate.
        conn.execute(
            "DELETE FROM maintenance_scans WHERE kind IN (?1,?2) AND created<?3",
            params![PREVIEW_REMOVAL, PREVIEW_RELINK, now() - 24 * 3600 * 1000],
        )?;
        conn.execute(
            "INSERT INTO maintenance_scans VALUES(?1,?2,?3,?4)",
            params![
                saved.public.id,
                kind,
                serde_json::to_string(saved)?,
                saved.public.created
            ],
        )?;
        Ok(())
    }

    /// Shows what removing the records of the selected unavailable tracks would change.
    pub fn preview_missing_removal(
        &self,
        scan_id: &str,
        selected: &[String],
        cancel: &CancellationToken,
    ) -> Result<MissingPreview> {
        let report = self.current_broken_scan(scan_id)?;
        let chosen = Self::chosen(&report, selected)?;
        let mut rows = Vec::new();
        let mut epochs = BTreeMap::new();
        {
            let conn = self.conn()?;
            for (id, state) in chosen {
                cancel.check()?;
                let mut row = row_for(&conn, &id, state)?;
                if let Some(record) = record(&conn, &id)? {
                    epochs.insert(id.clone(), record.epoch);
                    if let Some(reason) = unavailable_reason(&conn, &row, &record.path)? {
                        block(&mut row, reason);
                    } else if track_removal::in_open_draft(&conn, &id)? {
                        block(&mut row, track_removal::OPEN_DRAFT_REASON);
                    }
                }
                rows.push(row);
            }
        }
        let impact = {
            let conn = self.conn()?;
            impact_of(&conn, &rows)?
        };
        let public = MissingPreview {
            id: uuid::Uuid::new_v4().to_string(),
            kind: MissingRepairKind::Remove,
            scan_id: scan_id.into(),
            created: now(),
            ready: rows
                .iter()
                .filter(|r| r.outcome == MissingRowOutcome::Ready)
                .count(),
            rows,
            impact,
            locations: vec![],
            files_checked: 0,
            undo_limit: MISSING_UNDO_WINDOW,
        };
        self.save_preview(
            &SavedPreview {
                public: public.clone(),
                epochs,
                matches: vec![],
            },
            PREVIEW_REMOVAL,
        )?;
        Ok(public)
    }

    /// Removes the library records of the previewed tracks. Audio files, sidecars
    /// and every other table's unrelated rows are left alone. A row that changed
    /// since the preview is skipped, not guessed about.
    pub fn apply_missing_removal(
        &self,
        preview_id: &str,
        cancel: &CancellationToken,
    ) -> Result<MissingRepairEntry> {
        cancel.check()?;
        let saved: SavedPreview = {
            let conn = self.conn()?;
            load(&conn, preview_id, PREVIEW_REMOVAL)?
        };
        let _gates = self.repair_gates()?;
        let mut conn = self.conn()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut removed = Vec::new();
        let mut skipped = Vec::new();
        for row in &saved.public.rows {
            let skip = |reason: String| SkippedTrack {
                id: row.track_id.clone(),
                title: row.title.clone(),
                reason,
            };
            if row.outcome != MissingRowOutcome::Ready {
                skipped.push(skip(
                    row.reason.clone().unwrap_or_else(|| "Not removable".into()),
                ));
                continue;
            }
            let Some(current) = record(&tx, &row.track_id)? else {
                skipped.push(skip("The track is no longer in the library".into()));
                continue;
            };
            if saved.epochs.get(&row.track_id) != Some(&current.epoch) {
                skipped.push(skip("The track changed since the preview".into()));
            } else if !is_gone(&current.path) {
                skipped.push(skip("The file is back at its recorded location".into()));
            } else if let Some(reason) = work_in_flight(&tx, &row.track_id)? {
                skipped.push(skip(reason.into()));
            } else if track_removal::in_open_draft(&tx, &row.track_id)? {
                skipped.push(skip(track_removal::OPEN_DRAFT_REASON.into()));
            } else {
                removed.push((row.clone(), current));
            }
        }
        ensure!(
            !removed.is_empty(),
            "No records were removed. {}",
            skipped
                .first()
                .map(|s| format!("{}: {}", s.title, s.reason))
                .unwrap_or_default()
        );
        cancel.check()?;
        let ids: Vec<String> = removed
            .iter()
            .map(|(row, _)| row.track_id.clone())
            .collect();
        let gone: BTreeSet<String> = ids.iter().cloned().collect();
        let playlists = playlist_changes(&tx, &gone)?;
        write_members(&tx, &playlists, false)?;
        let track_removal::Removed { image, orphans } = track_removal::remove(&tx, &ids)?;
        let entry = MissingRepairEntry {
            id: uuid::Uuid::new_v4().to_string(),
            kind: MissingRepairKind::Remove,
            preview_id: Some(preview_id.into()),
            created: now(),
            undo_of: None,
            track_count: removed.len(),
            playlist_ids: playlists.iter().map(|c| c.id.clone()).collect(),
            playlist_memberships: playlists
                .iter()
                .map(|c| c.before.len() - c.after.len())
                .sum(),
            sync_links: removed.iter().map(|(r, _)| r.sync_links).sum(),
            rekordbox_links: removed.iter().map(|(r, _)| r.rekordbox_links).sum(),
            tracks: removed
                .iter()
                .map(|(row, _)| RepairedTrack {
                    id: row.track_id.clone(),
                    title: row.title.clone(),
                    artist: row.artist.clone(),
                    archived: row.archived,
                    display_path: row.display_path.clone(),
                    new_path: None,
                })
                .collect(),
            skipped,
            notes: vec![],
            stamp: advance(&tx)?,
        };
        // A preview is good for one apply, and it names the tracks it was made for. (Saved file
        // reports and other previews that name them went with the records.)
        tx.execute("DELETE FROM maintenance_scans WHERE id=?1", [preview_id])?;
        let operation = Operation {
            entry: entry.clone(),
            snapshot: Snapshot {
                image,
                playlists,
                relinks: vec![],
            },
            undone_by: None,
            undo_discarded: false,
            artwork_hashes: orphans.covers(),
        };
        tx.execute(
            "INSERT INTO maintenance_scans VALUES(?1,?2,?3,?4)",
            params![
                entry.id,
                OPERATION,
                serde_json::to_string(&operation)?,
                entry.created
            ],
        )?;
        let freed = undo_window::enforce(&tx)?;
        tx.commit()?;
        drop(conn);
        // Cache files go only after the records are really gone. The covers of the removed
        // tracks stay until their saved records are dropped; those of removals that just
        // left the Undo window go now.
        purge::cache_files(self.data_dir(), &orphans, &[]);
        purge::free_covers(self, &freed)?;
        self.tidy_after_bulk_change();
        Ok(entry)
    }

    fn repair_gates(&self) -> Result<Gates<'_>> {
        Ok(Gates {
            _organization: self
                .inner
                .organization_gate
                .lock()
                .map_err(|_| anyhow::anyhow!("Organization gate interrupted"))?,
            #[cfg(feature = "autotag")]
            _tag: self
                .inner
                .tag_gate
                .lock()
                .map_err(|_| anyhow::anyhow!("Publication gate interrupted"))?,
        })
    }

    /// Finds the selected tracks' files by content inside `locations` (folders or files).
    pub fn preview_missing_relink(
        &self,
        scan_id: &str,
        selected: &[String],
        locations: &[PathBuf],
        cancel: &CancellationToken,
        mut progress: impl FnMut(FileScanProgress),
    ) -> Result<MissingPreview> {
        ensure!(!locations.is_empty(), "Choose where to look for the files");
        let report = self.current_broken_scan(scan_id)?;
        let chosen = Self::chosen(&report, selected)?;
        let (mut rows, epochs, records) = {
            let conn = self.conn()?;
            let mut rows = Vec::new();
            let mut epochs = BTreeMap::new();
            let mut records = BTreeMap::new();
            for (id, state) in chosen {
                let mut row = row_for(&conn, &id, state)?;
                if let Some(record) = record(&conn, &id)? {
                    epochs.insert(id.clone(), record.epoch);
                    if let Some(reason) = unavailable_reason(&conn, &row, &record.path)? {
                        block(&mut row, reason);
                    }
                    records.insert(id.clone(), record);
                }
                rows.push(row);
            }
            (rows, epochs, records)
        };
        // Only files that could answer a question are worth hashing.
        let wanted: BTreeSet<&str> = rows
            .iter()
            .filter(|r| r.outcome == MissingRowOutcome::Ready)
            .map(|r| records[&r.track_id].hash.as_str())
            .collect();
        let mut found: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
        let mut names: BTreeMap<String, usize> = BTreeMap::new();
        let mut checked = 0usize;
        let mut files = BTreeSet::new();
        for location in locations {
            ensure!(location.exists(), "{} does not exist", location.display());
            if location.is_file() {
                files.insert(location.canonicalize()?);
                continue;
            }
            for entry in walkdir::WalkDir::new(location)
                .follow_links(false)
                .into_iter()
                .filter_entry(|e| !e.path().starts_with(self.data_dir()))
            {
                cancel.check()?;
                let Ok(entry) = entry else { continue };
                if entry.file_type().is_file()
                    && crate::watch::supported_path(entry.path())
                    && let Ok(path) = entry.path().canonicalize()
                {
                    files.insert(path);
                }
            }
        }
        let total = files.len();
        for path in files {
            cancel.check()?;
            progress(FileScanProgress {
                completed: checked,
                total: Some(total),
                current_path: Some(path.to_string_lossy().into_owned()),
            });
            checked += 1;
            let Ok(hash) = file_sha256_cancellable(&path, cancel) else {
                cancel.check()?;
                continue;
            };
            if let Some(name) = path.file_name() {
                *names
                    .entry(name.to_string_lossy().to_lowercase())
                    .or_default() += 1;
            }
            if wanted.contains(hash.as_str()) {
                found.entry(hash).or_default().push(path);
            }
        }
        let mut by_hash: BTreeMap<&str, usize> = BTreeMap::new();
        for row in rows
            .iter()
            .filter(|r| r.outcome == MissingRowOutcome::Ready)
        {
            *by_hash
                .entry(records[&row.track_id].hash.as_str())
                .or_default() += 1;
        }
        let mut matches = Vec::new();
        {
            let conn = self.conn()?;
            for row in rows
                .iter_mut()
                .filter(|r| r.outcome == MissingRowOutcome::Ready)
            {
                let record = &records[&row.track_id];
                let mut free = Vec::new();
                let mut tracked_by = None;
                for path in found.get(&record.hash).into_iter().flatten() {
                    let owner: Option<String> = conn
                        .query_row(
                            "SELECT title FROM tracks WHERE path=?1",
                            [path_bytes(path)],
                            |r| r.get(0),
                        )
                        .optional()?;
                    match owner {
                        Some(title) => tracked_by = Some(title),
                        None => free.push(path.clone()),
                    }
                }
                if by_hash[record.hash.as_str()] > 1 && !free.is_empty() {
                    row.outcome = MissingRowOutcome::Ambiguous;
                    row.reason = Some(
                        "Several selected tracks have identical content; select one at a time"
                            .into(),
                    );
                } else if free.len() > 1 {
                    row.outcome = MissingRowOutcome::Ambiguous;
                    row.reason = Some(format!(
                        "{} identical files were found; choose a narrower location",
                        free.len()
                    ));
                } else if let Some(path) = free.pop() {
                    row.new_path = Some(path.to_string_lossy().into_owned());
                    matches.push(Match {
                        track_id: record.id.clone(),
                        from: path_bytes(&record.path),
                        to: path_bytes(&path),
                        hash: record.hash.clone(),
                        sidecars: vec![],
                    });
                } else if let Some(title) = tracked_by {
                    row.outcome = MissingRowOutcome::AlreadyTracked;
                    row.reason = Some(format!(
                        "An identical file is already the library track \"{title}\". Use Duplicates to merge them"
                    ));
                } else {
                    row.outcome = MissingRowOutcome::NotFound;
                    let same_name = record
                        .path
                        .file_name()
                        .is_some_and(|n| names.contains_key(&n.to_string_lossy().to_lowercase()));
                    row.reason = Some(if same_name {
                        "A file with this name was found, but its content is different. It was not relinked".into()
                    } else {
                        "No file with identical content was found".into()
                    });
                }
            }
        }
        let impact = MissingImpact::default();
        let public = MissingPreview {
            id: uuid::Uuid::new_v4().to_string(),
            kind: MissingRepairKind::Relink,
            scan_id: scan_id.into(),
            created: now(),
            ready: matches.len(),
            rows,
            impact,
            locations: locations
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect(),
            files_checked: checked,
            undo_limit: MISSING_UNDO_WINDOW,
        };
        self.save_preview(
            &SavedPreview {
                public: public.clone(),
                epochs,
                matches,
            },
            PREVIEW_RELINK,
        )?;
        Ok(public)
    }

    /// Points the matched tracks at the files that were found. Each file is
    /// hashed again first: only the exact bytes the track recorded are linked.
    pub fn apply_missing_relink(
        &self,
        preview_id: &str,
        cancel: &CancellationToken,
    ) -> Result<MissingRepairEntry> {
        cancel.check()?;
        let saved: SavedPreview = {
            let conn = self.conn()?;
            load(&conn, preview_id, PREVIEW_RELINK)?
        };
        let mut skipped = Vec::new();
        let rows: BTreeMap<&str, &MissingRow> = saved
            .public
            .rows
            .iter()
            .map(|r| (r.track_id.as_str(), r))
            .collect();
        for row in saved
            .public
            .rows
            .iter()
            .filter(|r| r.outcome != MissingRowOutcome::Ready)
        {
            skipped.push(SkippedTrack {
                id: row.track_id.clone(),
                title: row.title.clone(),
                reason: row.reason.clone().unwrap_or_else(|| "Not relinked".into()),
            });
        }
        // Hash before taking any gate; hashing is the slow part.
        let mut verified = Vec::new();
        for m in &saved.matches {
            cancel.check()?;
            let row = rows[m.track_id.as_str()];
            let to = bytes_path(&m.to)?;
            let before = stamp(&to);
            let same = file_sha256_cancellable(&to, cancel).is_ok_and(|h| h == m.hash);
            cancel.check()?;
            if !same || before.is_none() || stamp(&to) != before {
                skipped.push(SkippedTrack {
                    id: row.track_id.clone(),
                    title: row.title.clone(),
                    reason: "The file changed since the preview and was not relinked".into(),
                });
            } else {
                verified.push((m, before));
            }
        }
        let _gates = self.repair_gates()?;
        let mut conn = self.conn()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut applied = Vec::new();
        let mut relinks = Vec::new();
        for (m, observed) in verified {
            let row = rows[m.track_id.as_str()];
            let reject = |reason: &str| SkippedTrack {
                id: row.track_id.clone(),
                title: row.title.clone(),
                reason: reason.into(),
            };
            let Some(current) = record(&tx, &m.track_id)? else {
                skipped.push(reject("The track is no longer in the library"));
                continue;
            };
            let to = bytes_path(&m.to)?;
            if saved.epochs.get(&m.track_id) != Some(&current.epoch) || current.hash != m.hash {
                skipped.push(reject("The track changed since the preview"));
                continue;
            }
            if stamp(&to) != observed {
                skipped.push(reject(
                    "The file changed since the preview and was not relinked",
                ));
                continue;
            }
            let moved = Move {
                track: &m.track_id,
                from: &current.path,
                to: &to,
                hash: &m.hash,
            };
            let followed = match Self::relink_in_tx(&tx, &moved, Scope::Any)? {
                Ok(done) => done.sidecars,
                Err(reason) => {
                    skipped.push(reject(&reason));
                    continue;
                }
            };
            applied.push(RepairedTrack {
                id: current.id.clone(),
                title: current.title.clone(),
                artist: current.artist.clone(),
                archived: current.archived,
                display_path: current.path.to_string_lossy().into_owned(),
                new_path: Some(to.to_string_lossy().into_owned()),
            });
            relinks.push(Match {
                track_id: m.track_id.clone(),
                from: m.from.clone(),
                to: m.to.clone(),
                hash: m.hash.clone(),
                sidecars: followed,
            });
        }
        ensure!(
            !applied.is_empty(),
            "No files were relinked. {}",
            skipped
                .first()
                .map(|s| format!("{}: {}", s.title, s.reason))
                .unwrap_or_default()
        );
        let entry = MissingRepairEntry {
            id: uuid::Uuid::new_v4().to_string(),
            kind: MissingRepairKind::Relink,
            preview_id: Some(preview_id.into()),
            created: now(),
            undo_of: None,
            track_count: applied.len(),
            tracks: applied,
            skipped,
            notes: vec![],
            playlist_ids: vec![],
            playlist_memberships: 0,
            sync_links: 0,
            rekordbox_links: 0,
            stamp: advance(&tx)?,
        };
        // A preview is good for one apply, and it names the tracks it was made for.
        tx.execute("DELETE FROM maintenance_scans WHERE id=?1", [preview_id])?;
        let operation = Operation {
            entry: entry.clone(),
            snapshot: Snapshot {
                image: RecordImage::default(),
                playlists: vec![],
                relinks,
            },
            undone_by: None,
            undo_discarded: false,
            artwork_hashes: vec![],
        };
        tx.execute(
            "INSERT INTO maintenance_scans VALUES(?1,?2,?3,?4)",
            params![
                entry.id,
                OPERATION,
                serde_json::to_string(&operation)?,
                entry.created
            ],
        )?;
        tx.commit()?;
        Ok(entry)
    }

    /// Repairs made from Missing & broken, newest first.
    pub fn missing_repair_history(&self) -> Result<Vec<MissingRepairHistory>> {
        let conn = self.conn()?;
        let mut query = conn.prepare(
            "SELECT json_extract(payload_json,'$.entry'),json_extract(payload_json,'$.undone_by'),COALESCE(json_extract(payload_json,'$.undo_discarded'),0) FROM maintenance_scans WHERE kind=?1 ORDER BY created DESC,rowid DESC",
        )?;
        let rows = query
            .query_map([OPERATION], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, bool>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|(entry, undone_by, undo_discarded)| {
                Ok(MissingRepairHistory {
                    entry: serde_json::from_str(&entry)?,
                    undone_by,
                    undo_discarded,
                    undo_limit: MISSING_UNDO_WINDOW,
                })
            })
            .collect()
    }

    /// Frees the saved records of a removal that has not been undone. The history
    /// line stays; the removal can no longer be undone. Like retained originals,
    /// saved records are kept until someone explicitly disposes of them, never
    /// automatically, and each removal holds at most `MISSING_REPAIR_LIMIT` tracks.
    pub fn discard_missing_repair_undo(&self, id: &str) -> Result<MissingRepairHistory> {
        let mut conn = self.conn()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut original: Operation = load(&tx, id, OPERATION)?;
        ensure!(
            original.entry.kind == MissingRepairKind::Remove && original.entry.undo_of.is_none(),
            "Only a removal has saved records to discard"
        );
        ensure!(
            original.undone_by.is_none(),
            "This removal was already undone"
        );
        ensure!(
            !original.undo_discarded,
            "The saved records were already discarded"
        );
        let covers = original.discard_saved();
        tx.execute(
            "UPDATE maintenance_scans SET payload_json=?1 WHERE id=?2",
            params![serde_json::to_string(&original)?, id],
        )?;
        tx.commit()?;
        drop(conn);
        purge::free_covers(self, &covers)?;
        self.tidy_after_bulk_change();
        Ok(MissingRepairHistory {
            entry: original.entry,
            undone_by: None,
            undo_discarded: true,
            undo_limit: MISSING_UNDO_WINDOW,
        })
    }

    /// Undoing a relink points a track back at its old location. If the file it was
    /// relinked to is still inside the same watch folder, the next scan follows it
    /// there again (one track, never a duplicate), which makes the Undo ineffective;
    /// say so rather than let it look like it stuck.
    fn watch_follow_notes(&self, operation: &Operation) -> Vec<String> {
        if operation.entry.kind != MissingRepairKind::Relink {
            return vec![];
        }
        let roots: Vec<PathBuf> = self
            .settings()
            .map(|s| s.settings.watch_folders)
            .unwrap_or_default()
            .iter()
            .filter_map(|folder| folder.canonicalize().ok())
            .collect();
        let following = operation
            .snapshot
            .relinks
            .iter()
            .filter_map(|m| Some((bytes_path(&m.from).ok()?, bytes_path(&m.to).ok()?)))
            .filter(|(from, to)| {
                to.is_file()
                    && roots
                        .iter()
                        .any(|root| from.starts_with(root) && to.starts_with(root))
            })
            .count();
        match following {
            0 => vec![],
            1 => vec!["This file is still inside a watch folder, where it now is, so CueRithm will follow it again the next time that folder is scanned (for example when the app starts). Move it back as well to keep the old location.".into()],
            n => vec![format!("{n} of these files are still inside a watch folder, where they now are, so CueRithm will follow them again the next time that folder is scanned (for example when the app starts). Move them back as well to keep the old locations.")],
        }
    }

    /// Reverses a removal or a relink as one step, or changes nothing and says why not.
    pub fn undo_missing_repair(
        &self,
        id: &str,
        cancel: &CancellationToken,
    ) -> Result<MissingRepairEntry> {
        cancel.check()?;
        let mut original: Operation = {
            let conn = self.conn()?;
            load(&conn, id, OPERATION)?
        };
        ensure!(
            original.undone_by.is_none() && original.entry.undo_of.is_none(),
            "This change was already undone"
        );
        ensure!(
            !original.undo_discarded,
            "The saved records of this removal were discarded, so it cannot be undone"
        );
        // Look at the old locations, hashing what is there, before any lock is taken.
        let observed = match original.entry.kind {
            MissingRepairKind::Remove => {
                track_removal::observe_world(&original.snapshot.image, cancel)?
            }
            MissingRepairKind::Relink => original
                .snapshot
                .relinks
                .iter()
                .map(|m| {
                    track_removal::observe_old_location(&bytes_path(&m.from)?, &m.hash, cancel)
                })
                .collect::<Result<Vec<_>>>()?,
        };
        let undo_notes = self.watch_follow_notes(&original);
        let _gates = self.repair_gates()?;
        let mut conn = self.conn()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for location in &observed {
            location.unchanged()?;
        }
        let mut playlist_ids = Vec::new();
        let mut undo_skipped = Vec::new();
        match original.entry.kind {
            MissingRepairKind::Remove => {
                let restored = track_removal::restore(&tx, &original.snapshot.image)?;
                // Rows that something changed after the removal stay as they are now.
                for (table, count) in restored.detached_kept {
                    let label = if table == "native_track_links" {
                        "rekordbox links".to_owned()
                    } else {
                        table
                    };
                    undo_skipped.push(SkippedTrack {
                        id: String::new(),
                        title: label,
                        reason: format!(
                            "{count} changed after the removal (rescanned or linked to another track) and were left as they are"
                        ),
                    });
                }
                for (table, count) in restored.links_kept {
                    let label = match table.as_str() {
                        "sync_links" => "sync links",
                        "native_sync_track_ownership" => "DJ-program sync records",
                        other => other,
                    };
                    undo_skipped.push(SkippedTrack {
                        id: String::new(),
                        title: label.to_owned(),
                        reason: format!(
                            "{count} linked to something else after the removal and were left as they are"
                        ),
                    });
                }
                let gone: BTreeSet<&String> = original.entry.tracks.iter().map(|t| &t.id).collect();
                for change in &original.snapshot.playlists {
                    let exists: bool = tx.query_row(
                        "SELECT EXISTS(SELECT 1 FROM playlists WHERE id=?1)",
                        [&change.id],
                        |r| r.get(0),
                    )?;
                    if !exists {
                        continue;
                    }
                    // Back where each track was; if the playlist was edited since,
                    // each lands at its old position or at the end.
                    let mut list = members(&tx, &change.id)?;
                    for (at, track) in change.before.iter().enumerate() {
                        if gone.contains(track) && !list.contains(track) {
                            list.insert(at.min(list.len()), track.clone());
                        }
                    }
                    write_members(
                        &tx,
                        &[DuplicatePlaylistChange {
                            id: change.id.clone(),
                            before: list,
                            after: vec![],
                        }],
                        true,
                    )?;
                    playlist_ids.push(change.id.clone());
                }
            }
            MissingRepairKind::Relink => {
                // Check every track before changing any: Undo is all or nothing.
                for m in &original.snapshot.relinks {
                    let current = record(&tx, &m.track_id)?
                        .filter(|r| path_bytes(&r.path) == m.to)
                        .with_context(|| "Cannot undo: a relinked track has changed since")?;
                    ensure!(
                        current.hash == m.hash,
                        "Cannot undo: the audio of \"{}\" changed after it was relinked (it was prepared again)",
                        current.title
                    );
                    let taken: bool = tx.query_row(
                        "SELECT EXISTS(SELECT 1 FROM tracks WHERE path=?1)",
                        [&m.from],
                        |r| r.get(0),
                    )?;
                    ensure!(
                        !taken,
                        "Cannot undo: another track now uses the old location"
                    );
                    if let Some(reason) = work_in_flight(&tx, &m.track_id)? {
                        anyhow::bail!("Cannot undo: {reason}");
                    }
                }
                for m in &original.snapshot.relinks {
                    tx.execute(
                        "UPDATE tracks SET path=?1,updated=?2 WHERE id=?3 AND path=?4 AND source_hash=?5",
                        params![m.from, now(), m.track_id, m.to, m.hash],
                    )?;
                    // Artwork rows that followed the track go back with it.
                    for (old, new) in &m.sidecars {
                        tx.execute(
                            "UPDATE OR IGNORE organization_owned_sidecars SET path=?1 WHERE track_id=?2 AND path=?3",
                            params![old, m.track_id, new],
                        )?;
                    }
                }
            }
        }
        let entry = MissingRepairEntry {
            id: uuid::Uuid::new_v4().to_string(),
            kind: original.entry.kind,
            preview_id: None,
            created: now(),
            undo_of: Some(id.into()),
            track_count: original.entry.track_count,
            tracks: original.entry.tracks.clone(),
            skipped: undo_skipped,
            notes: undo_notes,
            playlist_ids,
            playlist_memberships: original.entry.playlist_memberships,
            sync_links: original.entry.sync_links,
            rekordbox_links: original.entry.rekordbox_links,
            stamp: advance(&tx)?,
        };
        original.undone_by = Some(entry.id.clone());
        // The records are back in the library; the copy kept for Undo is no longer needed.
        original.snapshot = Snapshot::default();
        // The covers stay: the restored records use them again.
        original.artwork_hashes.clear();
        tx.execute(
            "UPDATE maintenance_scans SET payload_json=?1 WHERE id=?2",
            params![serde_json::to_string(&original)?, id],
        )?;
        let record_of_undo = Operation {
            entry: entry.clone(),
            snapshot: Snapshot::default(),
            undone_by: None,
            undo_discarded: false,
            artwork_hashes: vec![],
        };
        tx.execute(
            "INSERT INTO maintenance_scans VALUES(?1,?2,?3,?4)",
            params![
                entry.id,
                OPERATION,
                serde_json::to_string(&record_of_undo)?,
                entry.created
            ],
        )?;
        tx.commit()?;
        Ok(entry)
    }
}
