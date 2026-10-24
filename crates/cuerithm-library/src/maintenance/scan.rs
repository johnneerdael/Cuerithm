use super::files::{FileProof, file_state, proofs_current, resolved_prefix};
use super::*;
use crate::{
    Library,
    model::{bytes_path, path_bytes},
    playlists::scope,
};
use anyhow::{Context, Result, ensure};
use cuerithm_maintenance::{Candidate, Matcher, ReviewTrack};
use cuerithm_media::{
    AudioFingerprint, CancellationToken, FingerprintOutcome, MediaRuntime, file_sha256_cancellable,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

struct Input {
    id: String,
    path: PathBuf,
    title: String,
    artist: String,
    duration: f64,
    archived: bool,
    hash: String,
    metadata: Option<String>,
    cues: Option<String>,
    ignored: bool,
}
#[derive(Serialize, Deserialize)]
pub(super) struct SavedScan {
    pub(super) report: DuplicateScanReport,
    #[serde(default)]
    pub(super) candidates: Vec<ReviewTrack>,
    pub(super) scope: scope::ScopeGuard,
    pub(super) files: Vec<FileProof>,
    engine: String,
    logic: String,
}

pub(super) fn logic_identity() -> String {
    let mut hash = Sha256::new();
    for bytes in [
        include_bytes!("scan.rs").as_slice(),
        include_bytes!("model.rs").as_slice(),
        include_bytes!("files.rs").as_slice(),
        include_bytes!("unused.rs").as_slice(),
        include_bytes!("removal.rs").as_slice(),
        include_bytes!("removal/folders.rs").as_slice(),
        include_bytes!("removal/legacy.rs").as_slice(),
        include_bytes!("broken.rs").as_slice(),
        include_bytes!("review.rs").as_slice(),
        include_bytes!("merge.rs").as_slice(),
        include_bytes!("donors.rs").as_slice(),
        include_bytes!("donor_state.rs").as_slice(),
        include_bytes!("../record_snapshot.rs").as_slice(),
        include_bytes!("track_removal.rs").as_slice(),
        include_bytes!("undo_window.rs").as_slice(),
        include_bytes!("purge.rs").as_slice(),
        include_bytes!("trash.rs").as_slice(),
        include_bytes!("deletion.rs").as_slice(),
        include_bytes!("../playlists/scope.rs").as_slice(),
        include_bytes!("../../../cuerithm-maintenance/src/lib.rs").as_slice(),
        include_bytes!("../../../cuerithm-maintenance/src/normalize.rs").as_slice(),
        include_bytes!("../../../cuerithm-maintenance/src/review.rs").as_slice(),
        include_bytes!("../../../cuerithm-maintenance/src/merge.rs").as_slice(),
        include_bytes!("../../../cuerithm-maintenance/src/import.rs").as_slice(),
    ] {
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    hash.update(env!("CUERITHM_SMART_ENGINE_ID").as_bytes());
    hash.update(cuerithm_media::source_sha256().as_bytes());
    format!("{:x}", hash.finalize())
}
fn engine_identity(runtime: &MediaRuntime) -> String {
    format!(
        "{:x}",
        Sha256::digest(format!(
            "duplicate-scan-v1:{}:{}",
            runtime.identity,
            cuerithm_media::source_sha256()
        ))
    )
}
pub(super) fn stamp(conn: &Connection) -> Result<MaintenanceStamp> {
    Ok(MaintenanceStamp {
        inputs: scope::stamp(conn)?,
        maintenance_revision: conn.query_row(
            "SELECT value FROM meta WHERE key='maintenance_revision'",
            [],
            |r| r.get(0),
        )?,
    })
}
fn cache_valid(f: &AudioFingerprint, hash: &str, runtime: &MediaRuntime) -> bool {
    f.source_sha256 == hash
        && f.decoder_identity == runtime.identity
        && f.algorithm == "native-s16le-md5-v1"
        && f.pcm_format == "s16le"
        && f.digest.len() == 32
        && f.digest.bytes().all(|b| b.is_ascii_hexdigit())
        && (8000..=384000).contains(&f.sample_rate)
        && (1..=32).contains(&f.channels)
        && f.audio_stream_count > f.audio_stream_index
}
impl Library {
    pub fn scan_duplicates(
        &self,
        request: DuplicateScanRequest,
        runtime: &MediaRuntime,
        cancel: &CancellationToken,
    ) -> Result<DuplicateScanReport> {
        self.scan_duplicates_with_progress(request, runtime, cancel, |_| {})
    }
    pub fn scan_duplicates_with_progress(
        &self,
        request: DuplicateScanRequest,
        runtime: &MediaRuntime,
        cancel: &CancellationToken,
        mut progress: impl FnMut(DuplicateScanProgress),
    ) -> Result<DuplicateScanReport> {
        cancel.check()?;
        ensure!(
            request.ignored_paths.len() <= 5000
                && request.ignored_paths.iter().all(|p| p.is_absolute()),
            "Ignored folders must be absolute paths"
        );
        ensure!(
            request.scope == DuplicateScanScope::FullLibrary || !request.playlist_ids.is_empty(),
            "Select playlists to scan"
        );
        ensure!(
            request.scope != DuplicateScanScope::FullLibrary || request.playlist_ids.is_empty(),
            "Full-library scope does not accept selected playlists"
        );
        let ignored_paths = request
            .ignored_paths
            .iter()
            .map(|p| resolved_prefix(p))
            .collect::<Result<Vec<_>>>()?;
        let engine = engine_identity(runtime);
        let (scope, selected) = scope::capture(self, &request.playlist_ids, cancel)?;
        let (inputs, captured, mut cached) = {
            let mut conn = self.conn()?;
            let tx = conn.transaction()?;
            let captured = stamp(&tx)?;
            ensure!(
                captured.inputs == scope.stamp,
                "Library changed while capturing duplicate inputs; scan again"
            );
            let mut statement = tx.prepare("SELECT id,path,title,artist,COALESCE(duration,0),archived,source_hash,metadata_json,(SELECT json FROM cue_revisions c WHERE c.track_id=t.id ORDER BY c.created DESC,c.rowid DESC LIMIT 1),EXISTS(SELECT 1 FROM maintenance_ignored_tracks i WHERE i.track_id=t.id) FROM tracks t WHERE t.path IS NOT NULL AND t.source_hash IS NOT NULL ORDER BY t.rowid")?;
            let rows = statement
                .query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, Vec<u8>>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, f64>(4)?,
                        r.get::<_, bool>(5)?,
                        r.get::<_, String>(6)?,
                        r.get::<_, Option<String>>(7)?,
                        r.get::<_, Option<String>>(8)?,
                        r.get::<_, bool>(9)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let mut inputs = vec![];
            for (id, path, title, artist, duration, archived, hash, metadata, cues, ignored) in rows
            {
                cancel.check()?;
                if (request.scope == DuplicateScanScope::SelectedPlaylists
                    && !selected.contains(&id))
                    || (request.scope != DuplicateScanScope::SelectedPlaylists && archived)
                {
                    continue;
                }
                let path = bytes_path(&path)?;
                if ignored_paths.iter().any(|folder| path.starts_with(folder)) {
                    continue;
                }
                ensure!(
                    duration.is_finite() && duration >= 0.,
                    "Invalid duration in duplicate input"
                );
                inputs.push(Input {
                    id,
                    path,
                    title,
                    artist,
                    duration,
                    archived,
                    hash,
                    metadata,
                    cues,
                    ignored,
                });
            }
            drop(statement);
            let mut statement = tx.prepare("SELECT track_id,source_hash,fingerprint_json FROM maintenance_fingerprints WHERE engine_identity=?1")?;
            let cached = statement
                .query_map([&engine], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        (r.get::<_, String>(1)?, r.get::<_, String>(2)?),
                    ))
                })?
                .collect::<rusqlite::Result<BTreeMap<_, _>>>()?;
            drop(statement);
            tx.commit()?;
            (inputs, captured, cached)
        };
        let mut report = DuplicateScanReport {
            id: uuid::Uuid::new_v4().to_string(),
            created: crate::now(),
            stamp: captured,
            request: request.clone(),
            stale: false,
            stale_reason: None,
            tracks: vec![],
            groups: vec![],
            skipped_unavailable: vec![],
            cache_hits: 0,
            fingerprints_computed: 0,
        };
        let mut files = vec![];
        let mut candidates = vec![];
        let mut review_tracks = vec![];
        let mut fingerprints: BTreeMap<(u32, u32, String), Vec<String>> = BTreeMap::new();
        let mut fingerprint_order = vec![];
        let total = inputs.len();
        progress(DuplicateScanProgress {
            completed: 0,
            total,
            track_id: None,
            cached: false,
        });
        for input in inputs {
            cancel.check()?;
            let observed_state = file_state(&input.path);
            let mut row = DuplicateScanTrack {
                id: input.id.clone(),
                display_path: input.path.to_string_lossy().into_owned(),
                title: input.title.clone(),
                artist: input.artist.clone(),
                archived: input.archived,
                state: observed_state,
                detail: None,
                observed_sha256: None,
                source_changed: false,
                fingerprint: None,
                cached: false,
            };
            if observed_state == MaintenanceFileState::Present {
                match file_sha256_cancellable(&input.path, cancel) {
                    Ok(hash) => {
                        row.source_changed = hash != input.hash;
                        row.observed_sha256 = Some(hash.clone());
                        if input.duration <= 15. || input.duration >= 1500. {
                            row.state = MaintenanceFileState::DurationSkipped;
                        } else {
                            if let Some((stored_hash, text)) = cached.remove(&input.id)
                                && stored_hash == hash
                                && let Ok(fp) = serde_json::from_str::<AudioFingerprint>(&text)
                                && cache_valid(&fp, &hash, runtime)
                            {
                                row.fingerprint = Some(fp);
                                row.cached = true;
                                report.cache_hits += 1;
                            }
                            if row.fingerprint.is_none() {
                                match runtime.fingerprint(
                                    &input.path,
                                    &self.data_dir().join("cache/fingerprints"),
                                    cancel,
                                ) {
                                    Ok(FingerprintOutcome::Audio(fp))
                                        if fp.source_sha256 == hash =>
                                    {
                                        row.fingerprint = Some(fp);
                                        report.fingerprints_computed += 1;
                                    }
                                    Ok(FingerprintOutcome::Audio(_)) => {
                                        row.state = MaintenanceFileState::Changed;
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
                    }
                    Err(error) => {
                        cancel.check()?;
                        row.state = MaintenanceFileState::Unavailable;
                        row.detail = Some(error.to_string());
                    }
                }
            }
            files.push(FileProof {
                path: path_bytes(&input.path),
                state: observed_state,
                hash: row.observed_sha256.clone(),
            });
            if !input.ignored
                && row.state.is_available()
                && let Some(fp) = &row.fingerprint
            {
                // Identical sample bytes can play differently at another native rate
                // or channel count. Codec/container differences do not affect PCM identity.
                let key = (fp.sample_rate, fp.channels, fp.digest.clone());
                if !fingerprints.contains_key(&key) {
                    fingerprint_order.push(key.clone());
                }
                fingerprints.entry(key).or_default().push(input.id.clone());
            }
            let metadata: serde_json::Value = input
                .metadata
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_default();
            let bitrate = metadata["properties"]["audio.bit_rate"]
                .as_str()
                .and_then(|s| s.parse::<f64>().ok())
                .filter(|v| v.is_finite() && *v >= 0.)
                .map(|v| v / 1000.);
            let cue_count = input
                .cues
                .as_deref()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                .and_then(|v| v["cues"].as_array().map(Vec::len))
                .unwrap_or(0);
            let candidate = Candidate {
                id: input.id,
                artist: input.artist,
                title: input.title,
                duration: input.duration,
                location: row.display_path.clone(),
                comparison_path: row.display_path.clone(),
                bitrate,
                cue_count,
                present: row.state.is_available(),
                ignored: input.ignored,
            };
            review_tracks.push(ReviewTrack {
                candidate: candidate.clone(),
                extra1: metadata["tags"]["extra1"].as_str().unwrap_or("").into(),
            });
            candidates.push(candidate);
            progress(DuplicateScanProgress {
                completed: report.tracks.len() + 1,
                total,
                track_id: Some(row.id.clone()),
                cached: row.cached,
            });
            report.tracks.push(row);
        }
        cancel.check()?;
        let index = candidates
            .iter()
            .map(|c| (c.id.clone(), c))
            .collect::<BTreeMap<_, _>>();
        let matcher = Matcher::new();
        let mut used = BTreeSet::new();
        for key in fingerprint_order {
            let ids = fingerprints.remove(&key).unwrap();
            if ids.len() > 1 {
                add_group(
                    &mut report,
                    &index,
                    &matcher,
                    DuplicateMatchMethod::Fingerprint,
                    ids,
                    &mut used,
                )?;
            }
        }
        // A track whose file is not available is never grouped by any match method. The
        // matcher skips it; Missing & broken is where the user deals with it.
        let grouped =
            matcher.available_metadata_groups(&candidates, request.tolerance, &[], &[])?;
        for ids in grouped.groups {
            cancel.check()?;
            let ids = ids
                .into_iter()
                .filter(|id| !used.contains(id))
                .collect::<Vec<_>>();
            if ids.len() > 1 {
                add_group(
                    &mut report,
                    &index,
                    &matcher,
                    DuplicateMatchMethod::ArtistTitle,
                    ids,
                    &mut used,
                )?;
            }
        }
        report.skipped_unavailable = report
            .tracks
            .iter()
            .filter(|t| !t.state.is_available())
            .map(|t| DuplicateSkippedTrack {
                id: t.id.clone(),
                state: t.state,
            })
            .collect();
        if request.scope == DuplicateScanScope::FullLibraryFiltered {
            report
                .groups
                .retain(|g| g.track_ids.iter().any(|id| selected.contains(id)));
        }
        let mut saved = SavedScan {
            report,
            candidates: review_tracks,
            scope,
            files,
            engine,
            logic: logic_identity(),
        };
        self.refresh_duplicate_staleness(&mut saved, cancel)?;
        cancel.check()?;
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        if stamp(&tx)? != saved.report.stamp {
            mark_stale(&mut saved.report, "Library inputs changed during scan");
        }
        // A new engine identity (every release has one) makes the old fingerprints useless.
        tx.execute(
            "DELETE FROM maintenance_fingerprints WHERE engine_identity!=?1",
            [&saved.engine],
        )?;
        for row in &saved.report.tracks {
            if let Some(fp) = &row.fingerprint {
                tx.execute("INSERT INTO maintenance_fingerprints VALUES(?1,?2,?3,?4,?5) ON CONFLICT(track_id,engine_identity) DO UPDATE SET source_hash=excluded.source_hash,fingerprint_json=excluded.fingerprint_json,created=excluded.created",params![row.id,saved.engine,fp.source_sha256,serde_json::to_string(fp)?,saved.report.created])?;
            }
        }
        cancel.check()?;
        tx.execute(
            "INSERT INTO maintenance_scans VALUES(?1,'duplicates',?2,?3)",
            params![
                saved.report.id,
                serde_json::to_string(&saved)?,
                saved.report.created
            ],
        )?;
        super::saved::prune(&tx)?;
        tx.commit()?;
        Ok(saved.report)
    }
    pub(super) fn refresh_duplicate_staleness(
        &self,
        saved: &mut SavedScan,
        cancel: &CancellationToken,
    ) -> Result<()> {
        if saved.logic != logic_identity() {
            mark_stale(&mut saved.report, "Duplicate logic changed; scan again");
        }
        let current = {
            let conn = self.conn()?;
            stamp(&conn)?
        };
        if current != saved.report.stamp || !saved.scope.current(self, cancel)? {
            mark_stale(&mut saved.report, "Library or scope changed; scan again");
        }
        if !proofs_current(&saved.files, cancel)? {
            mark_stale(&mut saved.report, "Source files changed; scan again");
        }
        Ok(())
    }
    pub fn duplicate_scan(&self, id: &str) -> Result<DuplicateScanReport> {
        Ok(self
            .load_duplicate_scan(id, &CancellationToken::default())?
            .report)
    }
    pub fn duplicate_scan_cancellable(
        &self,
        id: &str,
        cancel: &CancellationToken,
    ) -> Result<DuplicateScanReport> {
        Ok(self.load_duplicate_scan(id, cancel)?.report)
    }
    pub(super) fn load_duplicate_scan(
        &self,
        id: &str,
        cancel: &CancellationToken,
    ) -> Result<SavedScan> {
        cancel.check()?;
        let text: String = self
            .conn()?
            .query_row(
                "SELECT payload_json FROM maintenance_scans WHERE id=?1 AND kind='duplicates'",
                [id],
                |r| r.get(0),
            )
            .optional()?
            .context("Duplicate scan not found")?;
        let mut saved: SavedScan = serde_json::from_str(&text)?;
        self.refresh_duplicate_staleness(&mut saved, cancel)?;
        Ok(saved)
    }
}
fn mark_stale(report: &mut DuplicateScanReport, reason: &str) {
    report.stale = true;
    report.stale_reason = Some(reason.into());
}
fn add_group(
    report: &mut DuplicateScanReport,
    candidates: &BTreeMap<String, &Candidate>,
    matcher: &Matcher,
    method: DuplicateMatchMethod,
    ids: Vec<String>,
    used: &mut BTreeSet<String>,
) -> Result<()> {
    let tracks = ids
        .iter()
        .map(|id| {
            candidates
                .get(id)
                .map(|c| (*c).clone())
                .context("Missing duplicate candidate")
        })
        .collect::<Result<Vec<_>>>()?;
    let preference =
        matcher.preselect(&tracks, matches!(method, DuplicateMatchMethod::Fingerprint))?;
    let id = format!("{:x}", Sha256::digest(serde_json::to_vec(&(method, &ids))?));
    used.extend(ids.iter().cloned());
    report.groups.push(DuplicateScanGroup {
        id,
        method,
        track_ids: ids,
        suggested_keeper: preference.keeper,
        suggested_tags_from: preference.tags_from,
    });
    Ok(())
}
