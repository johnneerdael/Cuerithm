//! One-time removal of the original audio that earlier versions kept in the data folder.
//!
//! Earlier versions kept the original bytes of every file they tagged, for ever, so that a write
//! could be rolled back. Once a write has settled (committed or rolled back, and its cleanup is
//! done) nothing reads that copy. This module deletes the ones that are left, a bounded number
//! per start, and only files the application itself created: the path must be in the private
//! folder of a journal row, inside the data folder, a regular file and not a link. Nothing is
//! ever touched in the user's music folders.
use crate::Library;
use anyhow::Result;
use rusqlite::params;
use std::path::Path;

/// Journals handled per start. A library with more simply continues at the next start.
pub const SWEEP_JOURNALS_PER_START: usize = 500;

/// What one sweep removed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct OriginalsSweep {
    pub files_removed: u64,
    pub bytes_freed: u64,
    /// Settled writes whose original is still waiting for a later sweep.
    pub more_to_do: bool,
}

/// The write kinds whose originals are no longer needed once they have settled: the table
/// prefix, the private folder of the data folder their workspaces live in, and the states that
/// count as settled. Earlier versions kept the original of every changed write in all of them.
const FAMILIES: [(&str, &str, &str, &str, &str); 3] = [
    (
        "tag",
        "tagging",
        "'committed','rolled_back'",
        "1",
        "w.workspace",
    ),
    (
        "cleanup",
        "cleanup",
        "'committed','rolled_back','undone'",
        "1",
        "w.workspace",
    ),
    // A duplicate merge from an earlier version is undone from its retained original until it
    // is undone or leaves the Undo window; an Undo, and an operation that recorded the keeper's
    // tag values (a map with at least one entry), never need one. Merges stage in
    // `maintenance/staging` only; the rest of `maintenance/` is not theirs.
    (
        "maintenance",
        "maintenance/staging",
        "'committed','rolled_back','undone'",
        "w.state='undone' OR json_extract(w.payload_json,'$.kind')='undo' OR COALESCE(json_extract(w.payload_json,'$.undo_expired'),0)=1 OR (json_type(w.payload_json,'$.transfers')='object' AND EXISTS(SELECT 1 FROM json_each(w.payload_json,'$.transfers')))",
        "f.workspace",
    ),
];
pub(crate) fn owned_regular_file(data: &Path, area: &str, path: &Path) -> Option<u64> {
    let in_area = data
        .join(area)
        .canonicalize()
        .ok()
        .zip(path.parent().and_then(|p| p.canonicalize().ok()))
        .is_some_and(|(area, parent)| parent.starts_with(area));
    let meta = std::fs::symlink_metadata(path).ok()?;
    (in_area && meta.is_file() && !meta.file_type().is_symlink()).then_some(meta.len())
}

/// Removes the originals a dropped journal named, if they are files this application owns.
#[cfg_attr(not(feature = "autotag"), allow(dead_code))]
pub(crate) fn remove_owned_originals(lib: &Library, area: &str, backups: &[Vec<u8>]) {
    let Ok(data) = lib.data_dir().canonicalize() else {
        return;
    };
    for backup in backups {
        let Ok(path) = crate::model::bytes_path(backup) else {
            continue;
        };
        if owned_regular_file(&data, area, &path).is_some()
            && std::fs::remove_file(&path).is_ok()
            && let Some(folder) = path.parent()
            && data.join(area).canonicalize().ok() != folder.canonicalize().ok()
        {
            let _ = std::fs::remove_dir(folder);
        }
    }
}

impl Library {
    /// Removes the retained originals of tag writes that have settled. See the module notes.
    pub(crate) fn sweep_legacy_originals(&self) -> Result<OriginalsSweep> {
        self.sweep_legacy_originals_with(SWEEP_JOURNALS_PER_START)
    }
    fn sweep_legacy_originals_with(&self, limit: usize) -> Result<OriginalsSweep> {
        let mut total = OriginalsSweep::default();
        for (prefix, area, states, only, workspace) in FAMILIES {
            let part = self.sweep_family(prefix, area, states, only, workspace, limit)?;
            total.files_removed += part.files_removed;
            total.bytes_freed += part.bytes_freed;
            total.more_to_do |= part.more_to_do;
        }
        self.record_originals_removed(total.files_removed, total.bytes_freed)?;
        Ok(total)
    }

    /// Adds to the running totals that [`Library::legacy_originals_removed`] reports.
    pub(crate) fn record_originals_removed(&self, files: u64, bytes: u64) -> Result<()> {
        if files == 0 {
            return Ok(());
        }
        let conn = self.conn()?;
        for (key, add) in [
            ("lean_originals_removed_files", files),
            ("lean_originals_removed_bytes", bytes),
        ] {
            conn.execute(
                "INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=value+excluded.value",
                params![key, add as i64],
            )?;
        }
        Ok(())
    }

    fn sweep_family(
        &self,
        prefix: &str,
        area: &str,
        states: &str,
        only: &str,
        workspace: &str,
        limit: usize,
    ) -> Result<OriginalsSweep> {
        let data = self.data_dir().canonicalize()?;
        let rows: Vec<(String, Vec<u8>, i64, Vec<u8>)> = {
            let conn = self.conn()?;
            let mut stmt = conn.prepare(&format!(
                "SELECT w.id,{workspace},f.ordinal,f.backup FROM {prefix}_write_files f JOIN {prefix}_writes w ON w.id=f.write_id
                  WHERE w.state IN ({states}) AND w.cleanup_complete=1 AND f.backup IS NOT NULL AND ({only})
                  ORDER BY w.created,w.id,f.ordinal LIMIT ?1"
            ))?;
            stmt.query_map([limit as i64 + 1], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?
            .collect::<rusqlite::Result<_>>()?
        };
        let mut sweep = OriginalsSweep {
            more_to_do: rows.len() > limit,
            ..Default::default()
        };
        for (id, workspace, ordinal, backup) in rows.into_iter().take(limit) {
            let (Ok(backup), Ok(workspace)) = (
                crate::model::bytes_path(&backup),
                crate::model::bytes_path(&workspace),
            ) else {
                continue;
            };
            let removed = match owned_regular_file(&data, area, &backup) {
                Some(bytes) => match std::fs::remove_file(&backup) {
                    Ok(()) => Some(bytes),
                    Err(_) => continue,
                },
                // Already gone, or not a file this application owns: only the stale path goes.
                None if !backup.exists() => None,
                None => continue,
            };
            if let Some(bytes) = removed {
                sweep.files_removed += 1;
                sweep.bytes_freed += bytes;
            }
            self.conn()?.execute(
                &format!(
                    "UPDATE {prefix}_write_files SET backup=NULL WHERE write_id=?1 AND ordinal=?2"
                ),
                params![id, ordinal],
            )?;
            // The private folders go once nothing is left in them.
            let _ = std::fs::remove_dir(&workspace);
            if let Some(folder) = backup.parent()
                && data.join(area).canonicalize().ok() != folder.canonicalize().ok()
            {
                let _ = std::fs::remove_dir(folder);
            }
        }
        Ok(sweep)
    }

    /// Running totals of what the start-up sweeps removed: (files, bytes).
    pub fn legacy_originals_removed(&self) -> Result<(u64, u64)> {
        let conn = self.conn()?;
        let read = |key: &str| -> Result<u64> {
            Ok(conn
                .query_row("SELECT value FROM meta WHERE key=?1", [key], |r| {
                    r.get::<_, i64>(0)
                })
                .unwrap_or(0) as u64)
        };
        Ok((
            read("lean_originals_removed_files")?,
            read("lean_originals_removed_bytes")?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A settled tag write whose original is still on disk, as an earlier version left it.
    fn legacy_write(lib: &Library, n: usize, size: usize) -> std::path::PathBuf {
        let source = lib.data_dir().join(format!("source{n}.flac"));
        std::fs::write(&source, format!("source {n}")).unwrap();
        lib.enqueue(
            std::slice::from_ref(&source),
            lib.settings().unwrap().revision,
        )
        .unwrap();
        let (job, track, revision): (String, String, String) = lib
            .conn()
            .unwrap()
            .query_row(
                "SELECT id,track_id,settings_revision FROM jobs ORDER BY rowid DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        let folder = lib.data_dir().join(format!("tagging/cuerithm-tags-{n}"));
        std::fs::create_dir_all(&folder).unwrap();
        let backup = folder.join("original.flac");
        std::fs::write(&backup, vec![1u8; size]).unwrap();
        let conn = lib.conn().unwrap();
        conn.execute(
            "INSERT INTO tag_writes(id,job_id,track_id,attempt,settings_revision,before_hash,after_hash,pcm_hash,frames,decoder,analysis_json,metadata_json,provenance_json,workspace,state,created,cleanup_complete)
             VALUES(?1,?2,?3,1,?4,'a','b','c',0,'d','{}','{}','{}',?5,'committed',?6,1)",
            params![
                format!("write{n}"),
                job,
                track,
                revision,
                crate::model::path_bytes(&folder),
                n as i64
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tag_write_files(write_id,ordinal,target,staged,backup,before_hash,after_hash,publish_path,displaced_path)
             VALUES(?1,0,X'00',X'00',?2,'a','b',X'00',X'00')",
            params![format!("write{n}"), crate::model::path_bytes(&backup)],
        )
        .unwrap();
        backup
    }

    #[test]
    fn one_start_only_handles_so_many_writes_and_the_next_continues() {
        let d = tempfile::tempdir().unwrap();
        let lib = Library::open(&d.path().join("library.sqlite")).unwrap();
        let backups: Vec<_> = (0..5).map(|n| legacy_write(&lib, n, 1000 + n)).collect();
        let first = lib.sweep_legacy_originals_with(2).unwrap();
        assert_eq!((first.files_removed, first.more_to_do), (2, true));
        assert_eq!(backups.iter().filter(|b| b.exists()).count(), 3);
        let second = lib.sweep_legacy_originals_with(2).unwrap();
        assert_eq!((second.files_removed, second.more_to_do), (2, true));
        let third = lib.sweep_legacy_originals_with(2).unwrap();
        assert_eq!((third.files_removed, third.more_to_do), (1, false));
        assert!(backups.iter().all(|b| !b.exists()));
        assert!(backups.iter().all(|b| !b.parent().unwrap().exists()));
        assert_eq!(
            first.bytes_freed + second.bytes_freed + third.bytes_freed,
            (0..5).map(|n| 1000 + n as u64).sum::<u64>()
        );
        assert_eq!(lib.legacy_originals_removed().unwrap(), (5, 5010));
        assert_eq!(
            lib.sweep_legacy_originals().unwrap(),
            OriginalsSweep::default()
        );
    }

    #[test]
    fn a_write_that_is_not_settled_keeps_its_original() {
        let d = tempfile::tempdir().unwrap();
        let lib = Library::open(&d.path().join("library.sqlite")).unwrap();
        let backup = legacy_write(&lib, 0, 100);
        for (state, complete) in [("prepared", 0), ("conflict", 0), ("committed", 0)] {
            lib.conn()
                .unwrap()
                .execute(
                    "UPDATE tag_writes SET state=?1,cleanup_complete=?2",
                    params![state, complete],
                )
                .unwrap();
            assert_eq!(
                lib.sweep_legacy_originals().unwrap(),
                OriginalsSweep::default()
            );
            assert!(backup.exists(), "{state} with cleanup_complete={complete}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_link_in_the_private_folder_is_never_followed() {
        let d = tempfile::tempdir().unwrap();
        let lib = Library::open(&d.path().join("library.sqlite")).unwrap();
        let backup = legacy_write(&lib, 0, 100);
        let music = d.path().join("users-music.flac");
        std::fs::write(&music, b"the user's music").unwrap();
        std::fs::remove_file(&backup).unwrap();
        std::os::unix::fs::symlink(&music, &backup).unwrap();
        assert_eq!(
            lib.sweep_legacy_originals().unwrap(),
            OriginalsSweep::default()
        );
        assert!(music.exists());
    }

    #[test]
    fn the_originals_of_settled_cleanups_go_the_same_way() {
        let d = tempfile::tempdir().unwrap();
        let lib = Library::open(&d.path().join("library.sqlite")).unwrap();
        let source = lib.data_dir().join("source.flac");
        std::fs::write(&source, "source").unwrap();
        lib.enqueue(
            std::slice::from_ref(&source),
            lib.settings().unwrap().revision,
        )
        .unwrap();
        let track: String = lib
            .conn()
            .unwrap()
            .query_row("SELECT id FROM tracks", [], |r| r.get(0))
            .unwrap();
        let folder = lib.data_dir().join("cleanup/cuerithm-tags-old");
        std::fs::create_dir_all(&folder).unwrap();
        let backup = folder.join("original.flac");
        std::fs::write(&backup, vec![1u8; 2048]).unwrap();
        {
            let conn = lib.conn().unwrap();
            conn.execute(
                "INSERT INTO cleanup_batches VALUES('batch',NULL,NULL,'apply','complete','{}',0)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO cleanup_writes(id,batch_id,track_id,proposal_id,before_hash,after_hash,payload_json,workspace,state,created,cleanup_complete)
                 VALUES('write','batch',?1,'p','a','b','{}',?2,'undone',0,1)",
                params![track, crate::model::path_bytes(&folder)],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO cleanup_write_files VALUES('write',0,X'00',X'00',?1,'a','b',X'00',X'00')",
                params![crate::model::path_bytes(&backup)],
            )
            .unwrap();
        }
        let swept = lib.sweep_legacy_originals().unwrap();
        assert_eq!((swept.files_removed, swept.bytes_freed), (1, 2048));
        assert!(!backup.exists() && !folder.exists());
        let named: i64 = lib
            .conn()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM cleanup_write_files WHERE backup IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(named, 0);
    }

    #[test]
    fn a_merge_from_an_earlier_version_keeps_its_original_only_while_it_can_still_be_undone() {
        let d = tempfile::tempdir().unwrap();
        let lib = Library::open(&d.path().join("library.sqlite")).unwrap();
        let mut originals = vec![];
        for (n, (payload, state)) in [
            // Still undoable from its original.
            (r#"{"kind":"apply"}"#, "committed"),
            // Left the Undo window.
            (r#"{"kind":"apply","undo_expired":true}"#, "committed"),
            // Already undone.
            (r#"{"kind":"apply"}"#, "undone"),
            // An Undo never needs one.
            (r#"{"kind":"undo"}"#, "committed"),
            // Recorded the keeper's tag values: undone by values.
            (
                r#"{"kind":"apply","transfers":{"a.flac":{"before":{},"after":{}}}}"#,
                "committed",
            ),
            // An empty map records nothing: the original is still what Undo reads.
            (r#"{"kind":"apply","transfers":{}}"#, "committed"),
        ]
        .into_iter()
        .enumerate()
        {
            let folder = lib
                .data_dir()
                .join(format!("maintenance/staging/cuerithm-tags-{n}"));
            std::fs::create_dir_all(&folder).unwrap();
            let original = folder.join("original.flac");
            std::fs::write(&original, vec![2u8; 300]).unwrap();
            let conn = lib.conn().unwrap();
            conn.execute(
                "INSERT INTO maintenance_writes(id,kind,state,payload_json,created,cleanup_complete) VALUES(?1,'apply',?2,?3,?4,1)",
                params![format!("merge{n}"), state, payload, n as i64],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO maintenance_write_files VALUES(?1,0,X'00',X'00',?2,'a','b',X'00',X'00',?3)",
                params![
                    format!("merge{n}"),
                    crate::model::path_bytes(&original),
                    crate::model::path_bytes(&folder)
                ],
            )
            .unwrap();
            originals.push(original);
        }
        let swept = lib.sweep_legacy_originals().unwrap();
        assert_eq!(swept.files_removed, 4);
        let present: Vec<bool> = originals.iter().map(|o| o.exists()).collect();
        assert_eq!(present, [true, false, false, false, false, true]);
    }

    #[test]
    fn a_merge_journal_naming_a_file_outside_the_staging_folder_is_never_swept() {
        let d = tempfile::tempdir().unwrap();
        let lib = Library::open(&d.path().join("library.sqlite")).unwrap();
        // The removal folder of the data folder belongs to unused-file removals, not to merges.
        let folder = lib.data_dir().join("maintenance/removal/remove-x");
        std::fs::create_dir_all(&folder).unwrap();
        let copy = folder.join("original-0");
        std::fs::write(&copy, vec![3u8; 300]).unwrap();
        let conn = lib.conn().unwrap();
        conn.execute(
            "INSERT INTO maintenance_writes(id,kind,state,payload_json,created,cleanup_complete) VALUES('merge','apply','undone',?1,0,1)",
            params![r#"{"kind":"apply"}"#],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO maintenance_write_files VALUES('merge',0,X'00',X'00',?1,'a','b',X'00',X'00',?2)",
            params![
                crate::model::path_bytes(&copy),
                crate::model::path_bytes(&folder)
            ],
        )
        .unwrap();
        drop(conn);
        let swept = lib.sweep_legacy_originals().unwrap();
        assert_eq!(swept.files_removed, 0);
        assert!(copy.exists(), "not a file a merge ever staged");
    }
}
