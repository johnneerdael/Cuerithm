use super::{
    files::{FileProof, file_state, proofs_current},
    *,
};
use crate::{
    Library,
    model::{bytes_path, path_bytes},
};
use anyhow::{Context, Result};
use cuerithm_media::{
    CancellationToken, FingerprintOutcome, MediaRuntime, file_sha256_cancellable,
};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
#[derive(Serialize, Deserialize)]
struct SavedBroken {
    report: BrokenScanReport,
    files: Vec<FileProof>,
    logic: String,
}
fn stale(report: &mut BrokenScanReport, reason: &str) {
    report.stale = true;
    report.stale_reason = Some(reason.into());
}
impl Library {
    pub fn scan_broken_tracks(
        &self,
        request: BrokenScanRequest,
        runtime: &MediaRuntime,
        cancel: &CancellationToken,
    ) -> Result<BrokenScanReport> {
        self.scan_broken_tracks_with_progress(request, runtime, cancel, |_| {})
    }
    pub fn scan_broken_tracks_with_progress(
        &self,
        request: BrokenScanRequest,
        runtime: &MediaRuntime,
        cancel: &CancellationToken,
        mut progress: impl FnMut(FileScanProgress),
    ) -> Result<BrokenScanReport> {
        cancel.check()?;
        let (stamp, rows) = {
            let mut conn = self.conn()?;
            let tx = conn.transaction()?;
            let stamp = super::scan::stamp(&tx)?;
            let mut statement = tx.prepare(
                "SELECT id,path,source_hash,archived FROM tracks WHERE (archived=0 OR ?1) AND path IS NOT NULL AND source_hash IS NOT NULL ORDER BY rowid",
            )?;
            let rows = statement
                .query_map([request.include_archived], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, Vec<u8>>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, bool>(3)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            drop(statement);
            tx.commit()?;
            (stamp, rows)
        };
        let total = rows.len();
        let mut files = vec![];
        let mut report = BrokenScanReport {
            id: uuid::Uuid::new_v4().to_string(),
            created: crate::now(),
            stamp,
            decoder_identity: request.inspect_audio.then(|| runtime.identity.clone()),
            request,
            tracks: vec![],
            stale: false,
            stale_reason: None,
        };
        progress(FileScanProgress {
            completed: 0,
            total: Some(total),
            current_path: None,
        });
        for (id, path, expected, archived) in rows {
            cancel.check()?;
            let path = bytes_path(&path)?;
            let observed = file_state(&path);
            let mut row = BrokenScanTrack {
                archived,
                id,
                display_path: path.to_string_lossy().into(),
                state: observed,
                detail: None,
                observed_sha256: None,
                source_changed: false,
            };
            if observed == MaintenanceFileState::Present {
                match file_sha256_cancellable(&path, cancel) {
                    Ok(hash) => {
                        row.source_changed = hash != expected;
                        row.observed_sha256 = Some(hash.clone());
                        if report.request.inspect_audio {
                            match runtime.fingerprint(
                                &path,
                                &self.data_dir().join("cache/fingerprints"),
                                cancel,
                            ) {
                                Ok(FingerprintOutcome::Audio(fp)) => {
                                    if fp.source_sha256 != hash {
                                        row.state = MaintenanceFileState::Changed;
                                    }
                                }
                                Ok(FingerprintOutcome::Video) => {
                                    row.state = MaintenanceFileState::Video;
                                }
                                Ok(FingerprintOutcome::NoAudio) => {
                                    row.state = MaintenanceFileState::NoAudio;
                                }
                                Ok(FingerprintOutcome::Symlink) => {
                                    row.state = MaintenanceFileState::Changed;
                                }
                                Err(error) => {
                                    cancel.check()?;
                                    row.state = super::files::decode_error_state(&error, &hash);
                                    row.detail = Some(error.to_string());
                                }
                            }
                        }
                    }
                    Err(error) => {
                        cancel.check()?;
                        row.state = MaintenanceFileState::Unavailable;
                        row.detail = Some(error.to_string());
                    }
                }
            }
            files.push(FileProof {
                path: path_bytes(&path),
                state: observed,
                hash: row.observed_sha256.clone(),
            });
            progress(FileScanProgress {
                completed: report.tracks.len() + 1,
                total: Some(total),
                current_path: Some(row.display_path.clone()),
            });
            report.tracks.push(row);
        }
        let mut saved = SavedBroken {
            report,
            files,
            logic: super::scan::logic_identity(),
        };
        self.refresh_broken(&mut saved, cancel)?;
        cancel.check()?;
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        if super::scan::stamp(&tx)? != saved.report.stamp {
            stale(&mut saved.report, "Library inputs changed; scan again");
        }
        tx.execute(
            "INSERT INTO maintenance_scans VALUES(?1,'broken',?2,?3)",
            params![
                saved.report.id,
                serde_json::to_string(&saved)?,
                saved.report.created
            ],
        )?;
        super::saved::prune(&tx)?;
        cancel.check()?;
        tx.commit()?;
        Ok(saved.report)
    }
    fn refresh_broken(&self, saved: &mut SavedBroken, cancel: &CancellationToken) -> Result<()> {
        cancel.check()?;
        let current = {
            let conn = self.conn()?;
            super::scan::stamp(&conn)?
        };
        if saved.logic != super::scan::logic_identity() || current != saved.report.stamp {
            stale(&mut saved.report, "Library or scanner changed; scan again");
        }
        if !proofs_current(&saved.files, cancel)? {
            stale(&mut saved.report, "Source files changed; scan again");
        }
        Ok(())
    }
    pub fn broken_scan(&self, id: &str) -> Result<BrokenScanReport> {
        self.broken_scan_cancellable(id, &CancellationToken::default())
    }
    pub fn broken_scan_cancellable(
        &self,
        id: &str,
        cancel: &CancellationToken,
    ) -> Result<BrokenScanReport> {
        cancel.check()?;
        let text: String = self
            .conn()?
            .query_row(
                "SELECT payload_json FROM maintenance_scans WHERE id=?1 AND kind='broken'",
                [id],
                |r| r.get(0),
            )
            .optional()?
            .context("Broken scan not found")?;
        let mut saved: SavedBroken = serde_json::from_str(&text)?;
        self.refresh_broken(&mut saved, cancel)?;
        Ok(saved.report)
    }
}
