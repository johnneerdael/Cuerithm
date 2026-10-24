use cuerithm_library::{
    DuplicateMatchMethod, DuplicateScanRequest, DuplicateScanScope, Library, PlaylistEdit,
    PlaylistKind,
};
use cuerithm_media::{CancellationToken, MediaRuntime};
use std::path::{Path, PathBuf};

struct Fixture {
    dir: tempfile::TempDir,
    db: PathBuf,
    lib: Library,
    runtime: MediaRuntime,
    ids: Vec<String>,
    paths: Vec<PathBuf>,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("app/library.sqlite");
        let lib = Library::open(&db).unwrap();
        let root = std::env::var_os("CUERITHM_MEDIA_RUNTIME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/media-runtime/bin")
            });
        let runtime = MediaRuntime::from_bundle(&root).unwrap();
        let mut ids = vec![];
        let mut paths = vec![];
        for i in 0..4 {
            let path = dir.path().join(format!("track-{i}.wav"));
            wav(&path, if i == 2 { 43 } else { 37 });
            if i == 1 {
                let mut bytes = std::fs::read(&path).unwrap();
                bytes.extend_from_slice(b"JUNK\x04\0\0\0tag!");
                let size = (bytes.len() - 8) as u32;
                bytes[4..8].copy_from_slice(&size.to_le_bytes());
                std::fs::write(&path, bytes).unwrap();
            }
            let job = lib
                .enqueue(
                    std::slice::from_ref(&path),
                    lib.settings().unwrap().revision,
                )
                .unwrap()[0]
                .clone();
            ids.push(
                lib.jobs()
                    .unwrap()
                    .iter()
                    .find(|r| r.id == job)
                    .unwrap()
                    .track_id
                    .clone(),
            );
            paths.push(path);
        }
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute("UPDATE tracks SET title='Signal',artist='North',duration=20,state='ready',analysis_json='{\"untouched\":true}',metadata_json='{\"codec\":\"pcm_s16le\",\"sample_rate\":8000,\"channels\":1,\"tags\":{},\"properties\":{\"audio.bit_rate\":\"128000\"}}'",[]).unwrap();
        conn.execute("UPDATE tracks SET title='Other' WHERE id=?1", [&ids[2]])
            .unwrap();
        drop(conn);
        lib.set_archived(&ids[3], true).unwrap();
        Self {
            dir,
            db,
            lib,
            runtime,
            ids,
            paths,
        }
    }
    fn playlist(&self, id: &str, tracks: Vec<String>) {
        self.lib
            .apply_playlist_edit(
                PlaylistEdit::Create {
                    id: Some(id.into()),
                    parent_id: None,
                    kind: PlaylistKind::Playlist,
                    name: id.into(),
                    definition: None,
                },
                self.lib.playlist_state().unwrap().revision,
            )
            .unwrap();
        self.lib
            .apply_playlist_edit(
                PlaylistEdit::AddMembers {
                    id: id.into(),
                    track_ids: tracks,
                },
                self.lib.playlist_state().unwrap().revision,
            )
            .unwrap();
    }
    fn content(&self) -> Vec<String> {
        let conn = rusqlite::Connection::open(&self.db).unwrap();
        ["SELECT json_group_array(json_array(id,hex(path),title,artist,archived,source_hash,state,analysis_json,metadata_json,current_job)) FROM (SELECT * FROM tracks ORDER BY id)","SELECT json_group_array(json_array(id,state,stage,attempts,settings_revision)) FROM (SELECT * FROM jobs ORDER BY id)","SELECT json_group_array(json_array(id,track_id,job_id,json,created)) FROM (SELECT * FROM cue_revisions ORDER BY id)"].iter().map(|sql|conn.query_row(sql,[],|r|r.get(0)).unwrap()).collect()
    }
    #[cfg(feature = "autotag")]
    fn content_ignoring_file_identity(&self) -> Vec<String> {
        let conn = rusqlite::Connection::open(&self.db).unwrap();
        ["SELECT json_group_array(json_array(id,hex(path),title,artist,archived,state,analysis_json,current_job)) FROM (SELECT * FROM tracks ORDER BY id)","SELECT json_group_array(json_array(id,state,stage,attempts,settings_revision)) FROM (SELECT * FROM jobs ORDER BY id)","SELECT json_group_array(json_array(id,track_id,job_id,json,created)) FROM (SELECT * FROM cue_revisions ORDER BY id)"].iter().map(|sql|conn.query_row(sql,[],|r|r.get(0)).unwrap()).collect()
    }
    /// Every tag the keeper's file holds, and its decoded audio.
    #[cfg(feature = "autotag")]
    fn file_state(&self, index: usize) -> (cuerithm_autotag::TransferSnapshot, String) {
        let path = &self.paths[index];
        let tags =
            cuerithm_autotag::read_transfer_metadata(path, &self.runtime.probe(path).unwrap())
                .unwrap();
        let cache = tempfile::tempdir().unwrap();
        let pcm = self
            .runtime
            .decode(path, cache.path())
            .unwrap()
            .pcm_sha256()
            .unwrap();
        (tags, pcm)
    }
    /// The library describes the file the track really has.
    #[cfg(feature = "autotag")]
    fn assert_library_knows_file(&self, index: usize) {
        let recorded: String = rusqlite::Connection::open(&self.db)
            .unwrap()
            .query_row(
                "SELECT source_hash FROM tracks WHERE id=?1",
                [&self.ids[index]],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            recorded,
            cuerithm_media::file_sha256(&self.paths[index]).unwrap()
        );
    }
}
fn wav(path: &Path, seed: i32) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 8000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut out = hound::WavWriter::create(path, spec).unwrap();
    for i in 0..160000 {
        out.write_sample::<i16>(((i * seed) % 20001 - 10000) as i16)
            .unwrap();
    }
    out.finalize().unwrap();
}
fn sets(report: &cuerithm_library::DuplicateScanReport) -> Vec<std::collections::BTreeSet<String>> {
    report
        .groups
        .iter()
        .map(|g| g.track_ids.iter().cloned().collect())
        .collect()
}

#[test]
fn exact_audio_precedence_persistent_cache_and_read_only_scan_survive_restart() {
    let f = Fixture::new();
    let before = f.content();
    let hashes: Vec<_> = f
        .paths
        .iter()
        .map(|p| cuerithm_media::file_sha256(p).unwrap())
        .collect();
    let result = f
        .lib
        .scan_duplicates(
            DuplicateScanRequest::default(),
            &f.runtime,
            &CancellationToken::default(),
        )
        .unwrap();
    assert!(!result.stale);
    assert_eq!(result.tracks.len(), 3);
    assert_eq!(result.fingerprints_computed, 3);
    assert_eq!(result.cache_hits, 0);
    assert_eq!(
        sets(&result),
        vec![
            [f.ids[0].clone(), f.ids[1].clone()]
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>()
        ]
    );
    assert!(matches!(
        result.groups[0].method,
        DuplicateMatchMethod::Fingerprint
    ));
    assert_eq!(f.content(), before);
    assert_eq!(
        hashes,
        f.paths
            .iter()
            .map(|p| cuerithm_media::file_sha256(p).unwrap())
            .collect::<Vec<_>>()
    );
    assert_eq!(f.lib.duplicate_scan(&result.id).unwrap().id, result.id);
    let Fixture {
        dir,
        db,
        lib,
        runtime,
        ids,
        paths,
    } = f;
    drop(lib);
    let lib = Library::open(&db).unwrap();
    let second = lib
        .scan_duplicates(
            DuplicateScanRequest::default(),
            &runtime,
            &CancellationToken::default(),
        )
        .unwrap();
    assert_eq!(second.cache_hits, 3);
    assert_eq!(second.fingerprints_computed, 0);
    assert_eq!(
        sets(&second),
        vec![
            [ids[0].clone(), ids[1].clone()]
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>()
        ]
    );
    assert!(!second.stale);
    assert!(paths.iter().all(|p| p.exists()));
    drop(dir);
}

#[test]
fn selected_scope_keeps_explicit_archived_members_while_full_filter_keeps_outside_copies() {
    let f = Fixture::new();
    f.playlist("chosen", vec![f.ids[1].clone(), f.ids[3].clone()]);
    let mut request = DuplicateScanRequest {
        scope: DuplicateScanScope::SelectedPlaylists,
        playlist_ids: vec!["chosen".into()],
        ..Default::default()
    };
    let selected = f
        .lib
        .scan_duplicates(request.clone(), &f.runtime, &CancellationToken::default())
        .unwrap();
    assert_eq!(selected.tracks.len(), 2);
    assert_eq!(
        sets(&selected),
        vec![
            [f.ids[1].clone(), f.ids[3].clone()]
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>()
        ]
    );
    request.scope = DuplicateScanScope::FullLibraryFiltered;
    let filtered = f
        .lib
        .scan_duplicates(request, &f.runtime, &CancellationToken::default())
        .unwrap();
    assert_eq!(
        sets(&filtered),
        vec![
            [f.ids[0].clone(), f.ids[1].clone()]
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>()
        ]
    );
}

#[test]
fn source_edits_invalidate_cached_audio_even_at_the_same_size_and_timestamp() {
    let f = Fixture::new();
    f.lib
        .scan_duplicates(
            DuplicateScanRequest::default(),
            &f.runtime,
            &CancellationToken::default(),
        )
        .unwrap();
    let file = std::fs::File::options()
        .read(true)
        .write(true)
        .open(&f.paths[0])
        .unwrap();
    let modified = file.metadata().unwrap().modified().unwrap();
    let length = file.metadata().unwrap().len();
    wav(&f.paths[0], 53);
    file.set_times(std::fs::FileTimes::new().set_modified(modified))
        .unwrap();
    assert_eq!(std::fs::metadata(&f.paths[0]).unwrap().len(), length);
    let current = f
        .lib
        .scan_duplicates(
            DuplicateScanRequest::default(),
            &f.runtime,
            &CancellationToken::default(),
        )
        .unwrap();
    assert_eq!(current.cache_hits, 2);
    assert_eq!(current.fingerprints_computed, 1);
    assert!(
        !current
            .groups
            .iter()
            .any(|g| matches!(g.method, DuplicateMatchMethod::Fingerprint))
    );
}

#[test]
fn cancellation_never_saves_a_complete_report_and_library_edits_mark_results_stale() {
    let f = Fixture::new();
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert!(
        f.lib
            .scan_duplicates(DuplicateScanRequest::default(), &f.runtime, &cancel)
            .is_err()
    );
    let conn = rusqlite::Connection::open(&f.db).unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM maintenance_scans", [], |r| r
            .get::<_, u64>(0))
            .unwrap(),
        0
    );
    drop(conn);
    let mut changed = false;
    let report = f
        .lib
        .scan_duplicates_with_progress(
            DuplicateScanRequest::default(),
            &f.runtime,
            &CancellationToken::default(),
            |progress| {
                if progress.completed == 1 && !changed {
                    f.lib.set_archived(&f.ids[0], true).unwrap();
                    changed = true;
                }
            },
        )
        .unwrap();
    assert!(changed);
    assert!(report.stale);
    assert!(f.lib.duplicate_scan(&report.id).unwrap().stale);
}

#[test]
fn unanalysed_tracks_missing_parents_and_missing_files_are_distinct() {
    let f = Fixture::new();
    let conn = rusqlite::Connection::open(&f.db).unwrap();
    conn.execute("UPDATE tracks SET duration=NULL WHERE id=?1", [&f.ids[0]])
        .unwrap();
    std::fs::remove_file(&f.paths[1]).unwrap();
    std::fs::create_dir(f.dir.path().join("offline")).unwrap();
    let vanished = f.dir.path().join("offline/music.wav");
    wav(&vanished, 31);
    let job = f
        .lib
        .enqueue(
            std::slice::from_ref(&vanished),
            f.lib.settings().unwrap().revision,
        )
        .unwrap()[0]
        .clone();
    let offline_id = f
        .lib
        .jobs()
        .unwrap()
        .into_iter()
        .find(|j| j.id == job)
        .unwrap()
        .track_id;
    // A parent disappearing cannot prove whether the storage is disconnected.
    std::fs::remove_file(&vanished).unwrap();
    std::fs::remove_dir(f.dir.path().join("offline")).unwrap();
    let r = f
        .lib
        .scan_duplicates(
            DuplicateScanRequest::default(),
            &f.runtime,
            &CancellationToken::default(),
        )
        .unwrap();
    let state = |id: &str| r.tracks.iter().find(|t| t.id == id).unwrap().state;
    assert_eq!(
        state(&f.ids[0]),
        cuerithm_library::MaintenanceFileState::DurationSkipped
    );
    assert_eq!(
        state(&f.ids[1]),
        cuerithm_library::MaintenanceFileState::Missing
    );
    assert_eq!(
        state(&offline_id),
        cuerithm_library::MaintenanceFileState::Unavailable
    );
}

#[test]
fn saved_reports_recheck_source_content_and_cancelled_scans_do_not_save_reports() {
    let f = Fixture::new();
    let r = f
        .lib
        .scan_duplicates(
            DuplicateScanRequest::default(),
            &f.runtime,
            &CancellationToken::default(),
        )
        .unwrap();
    wav(&f.paths[0], 71);
    assert!(f.lib.duplicate_scan(&r.id).unwrap().stale);
    let cancel = CancellationToken::default();
    assert!(
        f.lib
            .scan_duplicates_with_progress(
                DuplicateScanRequest::default(),
                &f.runtime,
                &cancel,
                |p| {
                    if p.completed == 1 {
                        cancel.cancel();
                    }
                }
            )
            .is_err()
    );
    let conn = rusqlite::Connection::open(&f.db).unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM maintenance_scans", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        1
    );
}

#[test]
fn selected_smart_scope_evaluates_fresh_membership_without_publishing_members() {
    use cuerithm_library::SmartDefinition;
    use cuerithm_smartlists::{Operator, RuleValue, SmartRule};
    let f = Fixture::new();
    f.lib
        .apply_playlist_edit(
            PlaylistEdit::Create {
                id: Some("dynamic".into()),
                parent_id: None,
                kind: PlaylistKind::Smartlist,
                name: "Dynamic".into(),
                definition: Some(SmartDefinition {
                    match_all: true,
                    rules: vec![SmartRule {
                        id: "r".into(),
                        field: "title".into(),
                        operator: Operator::StringEquals,
                        value: RuleValue::Text("Signal".into()),
                        or: false,
                    }],
                }),
            },
            f.lib.playlist_state().unwrap().revision,
        )
        .unwrap();
    let request = DuplicateScanRequest {
        scope: DuplicateScanScope::SelectedPlaylists,
        playlist_ids: vec!["dynamic".into()],
        ..Default::default()
    };
    let r = f
        .lib
        .scan_duplicates(request.clone(), &f.runtime, &CancellationToken::default())
        .unwrap();
    assert_eq!(r.tracks.len(), 2);
    assert_eq!(r.groups.len(), 1);
    let conn = rusqlite::Connection::open(&f.db).unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM smart_playlist_members", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        0
    );
    conn.execute("UPDATE tracks SET title='Other' WHERE id=?1", [&f.ids[0]])
        .unwrap();
    drop(conn);
    assert!(f.lib.duplicate_scan(&r.id).unwrap().stale);
    let next = f
        .lib
        .scan_duplicates(request, &f.runtime, &CancellationToken::default())
        .unwrap();
    assert_eq!(next.tracks.len(), 1);
    assert!(next.groups.is_empty());
}

#[test]
fn ignores_are_component_scoped_and_persisted_ignores_invalidate_reports() {
    let f = Fixture::new();
    let r = f
        .lib
        .scan_duplicates(
            DuplicateScanRequest {
                ignored_paths: vec![f.dir.path().join("track-")],
                ..Default::default()
            },
            &f.runtime,
            &CancellationToken::default(),
        )
        .unwrap();
    assert_eq!(r.tracks.len(), 3);
    let conn = rusqlite::Connection::open(&f.db).unwrap();
    conn.execute(
        "INSERT INTO maintenance_ignored_tracks VALUES(?1)",
        [&f.ids[0]],
    )
    .unwrap();
    drop(conn);
    assert!(f.lib.duplicate_scan(&r.id).unwrap().stale);
    let next = f
        .lib
        .scan_duplicates(
            DuplicateScanRequest::default(),
            &f.runtime,
            &CancellationToken::default(),
        )
        .unwrap();
    assert!(next.groups.is_empty());
    let excluded = f
        .lib
        .scan_duplicates(
            DuplicateScanRequest {
                ignored_paths: vec![f.dir.path().to_owned()],
                ..Default::default()
            },
            &f.runtime,
            &CancellationToken::default(),
        )
        .unwrap();
    assert!(excluded.tracks.is_empty());
    assert!(excluded.groups.is_empty());
    assert!(!excluded.stale);
}

#[test]
fn edited_last_file_and_cancelled_final_callback_cannot_publish_a_fresh_report() {
    let f = Fixture::new();
    let report = f
        .lib
        .scan_duplicates_with_progress(
            DuplicateScanRequest::default(),
            &f.runtime,
            &CancellationToken::default(),
            |p| {
                if p.completed == p.total {
                    wav(&f.paths[0], 79);
                }
            },
        )
        .unwrap();
    assert!(report.stale);
    let cancel = CancellationToken::default();
    assert!(
        f.lib
            .scan_duplicates_with_progress(
                DuplicateScanRequest::default(),
                &f.runtime,
                &cancel,
                |p| {
                    if p.completed == p.total {
                        cancel.cancel();
                    }
                }
            )
            .is_err()
    );
    let conn = rusqlite::Connection::open(&f.db).unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM maintenance_scans", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        1
    );
}

#[test]
fn migration_from_v6_preserves_tracks_and_initializes_empty_maintenance() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("library.sqlite");
    let conn = rusqlite::Connection::open(&db).unwrap();
    for text in [
        include_str!("../migrations/001_initial.sql"),
        include_str!("../migrations/002_tag_writes.sql"),
        include_str!("../migrations/003_preparation_cache.sql"),
        include_str!("../migrations/004_cleanup.sql"),
        include_str!("../migrations/005_cleanup_preparation.sql"),
        include_str!("../migrations/006_playlists.sql"),
    ] {
        conn.execute_batch(text).unwrap();
    }
    conn.execute("INSERT INTO tracks(id,path,title,source_hash,state,analysis_json,created,updated) VALUES('preserve',?1,'Legacy','source','ready','{}',1,1)",[b"unix:/preserve.flac".to_vec()]).unwrap();
    let before: String = conn
        .query_row(
            "SELECT json_group_array(json_array(id,hex(path),analysis_json)) FROM tracks",
            [],
            |r| r.get(0),
        )
        .unwrap();
    drop(conn);
    let lib = Library::open(&db).unwrap();
    let conn = rusqlite::Connection::open(&db).unwrap();
    assert_eq!(
        conn.query_row("pragma user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        31
    );
    assert_eq!(
        conn.query_row(
            "SELECT json_group_array(json_array(id,hex(path),analysis_json)) FROM tracks",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        before
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM maintenance_scans", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        0
    );
    drop(lib);
}

#[test]
fn duplicate_candidate_order_follows_import_order_even_after_title_sort_changes() {
    let f = Fixture::new();
    let conn = rusqlite::Connection::open(&f.db).unwrap();
    conn.execute(
        "UPDATE tracks SET sort_title='zzz' WHERE id=?1",
        [&f.ids[0]],
    )
    .unwrap();
    conn.execute(
        "UPDATE tracks SET sort_title='aaa' WHERE id=?1",
        [&f.ids[1]],
    )
    .unwrap();
    drop(conn);
    let r = f
        .lib
        .scan_duplicates(
            DuplicateScanRequest::default(),
            &f.runtime,
            &CancellationToken::default(),
        )
        .unwrap();
    assert_eq!(
        r.tracks.iter().map(|t| t.id.clone()).collect::<Vec<_>>(),
        f.ids[..3]
    );
}

#[test]
fn duplicate_review_survives_restart_and_guards_revision_and_ignores() {
    use cuerithm_library::{DuplicateReviewEdit, GroupEdit};
    let mut f = Fixture::new();
    let before = f.content();
    let token = CancellationToken::default();
    let scan = f
        .lib
        .scan_duplicates(DuplicateScanRequest::default(), &f.runtime, &token)
        .unwrap();
    let review = f.lib.begin_duplicate_review(&scan.id, &token).unwrap();
    assert_eq!(review.revision, 0);
    assert_eq!(review.groups.len(), 1);
    let group = review.groups[0].id.clone();
    let chosen = f.ids[1].clone();
    let edit = |edit| DuplicateReviewEdit::Group {
        group_id: group.clone(),
        edit,
    };
    let selected = f
        .lib
        .edit_duplicate_review(
            &review.id,
            0,
            edit(GroupEdit::Select {
                track_id: chosen.clone(),
            }),
            &token,
        )
        .unwrap();
    assert_eq!(selected.groups[0].keeper.as_deref(), Some(chosen.as_str()));
    assert!(
        f.lib
            .edit_duplicate_review(&review.id, 0, edit(GroupEdit::Lock), &token)
            .is_err()
    );
    drop(f.lib);
    f.lib = Library::open(&f.db).unwrap();
    let reopened = f.lib.clone();
    let loaded = reopened.duplicate_review(&review.id, &token).unwrap();
    assert!(!loaded.stale);
    assert_eq!(loaded.revision, 1);
    assert_eq!(loaded.groups[0].keeper, selected.groups[0].keeper);
    let ignored = reopened
        .edit_duplicate_review(
            &review.id,
            1,
            edit(GroupEdit::ToggleIgnore {
                track_id: chosen.clone(),
            }),
            &token,
        )
        .unwrap();
    assert!(!ignored.stale);
    assert!(!ignored.groups[0].ready());
    assert!(f.lib.duplicate_scan(&scan.id).unwrap().stale);
    assert_eq!(f.content(), before);
    let fresh = f
        .lib
        .scan_duplicates(DuplicateScanRequest::default(), &f.runtime, &token)
        .unwrap();
    assert!(fresh.groups.is_empty());
    let restored = reopened
        .edit_duplicate_review(
            &review.id,
            2,
            edit(GroupEdit::ToggleIgnore { track_id: chosen }),
            &token,
        )
        .unwrap();
    assert!(restored.groups[0].ready());
    assert!(!restored.stale);
    assert!(
        !f.lib
            .scan_duplicates(DuplicateScanRequest::default(), &f.runtime, &token)
            .unwrap()
            .groups
            .is_empty()
    );
}

#[test]
fn duplicate_reviews_reject_source_library_and_concurrent_review_changes() {
    use cuerithm_library::{DuplicateReviewEdit, GroupEdit};
    let token = CancellationToken::default();
    let f = Fixture::new();
    let scan = f
        .lib
        .scan_duplicates(DuplicateScanRequest::default(), &f.runtime, &token)
        .unwrap();
    let one = f.lib.begin_duplicate_review(&scan.id, &token).unwrap();
    let two = f.lib.begin_duplicate_review(&scan.id, &token).unwrap();
    let edit = DuplicateReviewEdit::Group {
        group_id: one.groups[0].id.clone(),
        edit: GroupEdit::ToggleIgnore {
            track_id: f.ids[1].clone(),
        },
    };
    f.lib
        .edit_duplicate_review(&one.id, 0, edit, &token)
        .unwrap();
    assert!(f.lib.duplicate_review(&two.id, &token).unwrap().stale);
    let bad = DuplicateReviewEdit::Group {
        group_id: one.groups[0].id.clone(),
        edit: GroupEdit::Select {
            track_id: f.ids[2].clone(),
        },
    };
    assert!(
        f.lib
            .edit_duplicate_review(&one.id, 1, bad, &token)
            .is_err()
    );
    assert_eq!(f.lib.duplicate_review(&one.id, &token).unwrap().revision, 1);
    wav(&f.paths[0], 59);
    assert!(f.lib.duplicate_review(&one.id, &token).unwrap().stale);
    assert!(
        f.lib
            .edit_duplicate_review(
                &one.id,
                1,
                DuplicateReviewEdit::Group {
                    group_id: one.groups[0].id.clone(),
                    edit: GroupEdit::Lock
                },
                &token
            )
            .is_err()
    );
    let f = Fixture::new();
    let scan = f
        .lib
        .scan_duplicates(DuplicateScanRequest::default(), &f.runtime, &token)
        .unwrap();
    let review = f.lib.begin_duplicate_review(&scan.id, &token).unwrap();
    f.lib.set_archived(&f.ids[0], true).unwrap();
    assert!(f.lib.duplicate_review(&review.id, &token).unwrap().stale);
    assert!(f.lib.begin_duplicate_review(&scan.id, &token).is_err());
}

#[test]
fn duplicate_review_preferences_locks_copy_only_and_cancellation_are_read_only() {
    use cuerithm_library::{DuplicateReviewEdit, GroupEdit, Preference, ReviewState};
    let f = Fixture::new();
    let token = CancellationToken::default();
    let scan = f
        .lib
        .scan_duplicates(DuplicateScanRequest::default(), &f.runtime, &token)
        .unwrap();
    let review = f.lib.begin_duplicate_review(&scan.id, &token).unwrap();
    let before = f.content();
    let edit = |edit| DuplicateReviewEdit::Group {
        group_id: review.groups[0].id.clone(),
        edit,
    };
    let review = f
        .lib
        .edit_duplicate_review(
            &review.id,
            0,
            edit(GroupEdit::UseTags {
                track_id: Some(f.ids[1].clone()),
            }),
            &token,
        )
        .unwrap();
    let review = f
        .lib
        .edit_duplicate_review(&review.id, 1, edit(GroupEdit::Lock), &token)
        .unwrap();
    let review = f
        .lib
        .edit_duplicate_review(
            &review.id,
            2,
            edit(GroupEdit::Unlock { force: false }),
            &token,
        )
        .unwrap();
    assert_eq!(review.groups[0].state, ReviewState::CopyCues);
    let review = f
        .lib
        .edit_duplicate_review(
            &review.id,
            3,
            DuplicateReviewEdit::Prefer {
                preference: Preference::TitleContains {
                    value: "Signal".into(),
                },
                override_locked: false,
                use_best_cues: true,
            },
            &token,
        )
        .unwrap();
    assert_eq!(review.revision, 4);
    assert_eq!(f.content(), before);
    let cancelled = CancellationToken::default();
    cancelled.cancel();
    assert!(
        f.lib
            .edit_duplicate_review(&review.id, 4, edit(GroupEdit::Lock), &cancelled)
            .is_err()
    );
    assert_eq!(
        f.lib.duplicate_review(&review.id, &token).unwrap().revision,
        4
    );
}

#[test]
fn persistent_duplicate_ignores_can_be_removed_after_the_original_review_is_stale() {
    use cuerithm_library::{DuplicateReviewEdit, GroupEdit};
    let f = Fixture::new();
    let token = CancellationToken::default();
    let scan = f
        .lib
        .scan_duplicates(DuplicateScanRequest::default(), &f.runtime, &token)
        .unwrap();
    let review = f.lib.begin_duplicate_review(&scan.id, &token).unwrap();
    let ignored_id = f.ids[1].clone();
    f.lib
        .edit_duplicate_review(
            &review.id,
            0,
            DuplicateReviewEdit::Group {
                group_id: review.groups[0].id.clone(),
                edit: GroupEdit::ToggleIgnore {
                    track_id: ignored_id.clone(),
                },
            },
            &token,
        )
        .unwrap();
    f.lib.set_archived(&f.ids[2], true).unwrap();
    assert!(f.lib.duplicate_review(&review.id, &token).unwrap().stale);
    let ignored = f.lib.duplicate_ignores().unwrap();
    assert_eq!(ignored.tracks.len(), 1);
    assert_eq!(ignored.tracks[0].id, ignored_id);
    let before = f.content();
    assert!(
        f.lib
            .set_duplicate_ignores(
                vec![ignored_id.clone(), "outside".into()],
                false,
                ignored.stamp.clone()
            )
            .is_err()
    );
    assert_eq!(f.lib.duplicate_ignores().unwrap().tracks.len(), 1);
    let cleared = f
        .lib
        .set_duplicate_ignores(vec![ignored_id.clone()], false, ignored.stamp.clone())
        .unwrap();
    assert!(cleared.tracks.is_empty());
    assert!(
        f.lib
            .set_duplicate_ignores(vec![ignored_id], true, ignored.stamp)
            .is_err()
    );
    assert_eq!(f.content(), before);
    assert!(
        !f.lib
            .scan_duplicates(DuplicateScanRequest::default(), &f.runtime, &token)
            .unwrap()
            .groups
            .is_empty()
    );
}

#[test]
fn cancelling_saved_report_reload_and_export_uses_the_requested_token() {
    use cuerithm_library::{BrokenScanRequest, ExtensionMode, UnusedScanRequest};
    let f = Fixture::new();
    let token = CancellationToken::default();
    let duplicate = f
        .lib
        .scan_duplicates(DuplicateScanRequest::default(), &f.runtime, &token)
        .unwrap();
    let unused = f
        .lib
        .scan_unused_files(
            UnusedScanRequest {
                root: f.dir.path().to_path_buf(),
                extension_mode: ExtensionMode::All,
                extensions: vec![],
            },
            &token,
        )
        .unwrap();
    let broken = f
        .lib
        .scan_broken_tracks(BrokenScanRequest::default(), &f.runtime, &token)
        .unwrap();
    token.cancel();
    let errors = [
        f.lib
            .duplicate_scan_cancellable(&duplicate.id, &token)
            .unwrap_err(),
        f.lib
            .unused_scan_cancellable(&unused.id, &token)
            .unwrap_err(),
        f.lib
            .broken_scan_cancellable(&broken.id, &token)
            .unwrap_err(),
        f.lib
            .unused_scan_export_cancellable(&unused.id, &token)
            .unwrap_err(),
    ];
    assert!(errors.iter().all(|e| e.to_string().contains("cancelled")));
}

#[test]
fn duplicate_merge_replaces_playlists_archives_records_and_undo_survives_restart() {
    let mut f = Fixture::new();
    f.playlist(
        "affected",
        vec![f.ids[0].clone(), f.ids[2].clone(), f.ids[1].clone()],
    );
    f.playlist("unaffected", vec![f.ids[2].clone(), f.ids[0].clone()]);
    let token = CancellationToken::default();
    let before = f.content();
    let hashes = f
        .paths
        .iter()
        .map(|p| cuerithm_media::file_sha256(p).unwrap())
        .collect::<Vec<_>>();
    let scan = f
        .lib
        .scan_duplicates(DuplicateScanRequest::default(), &f.runtime, &token)
        .unwrap();
    let review = f.lib.begin_duplicate_review(&scan.id, &token).unwrap();
    let preview = f
        .lib
        .preview_duplicate_merge(
            &review.id,
            review.revision,
            vec![review.groups[0].id.clone()],
            &token,
        )
        .unwrap();
    assert_eq!(preview.playlists.len(), 1);
    assert_eq!(
        preview.playlists[0].after,
        vec![f.ids[2].clone(), f.ids[0].clone()]
    );
    let applied = f.lib.apply_duplicate_merge(&preview.id, &token).unwrap();
    assert_eq!(applied.archived_ids, vec![f.ids[1].clone()]);
    assert_eq!(
        f.lib.duplicate_merge_history().unwrap()[0].result.id,
        applied.id
    );
    assert_eq!(
        f.lib.manual_playlist_members("affected").unwrap(),
        vec![f.ids[2].clone(), f.ids[0].clone()]
    );
    assert_eq!(
        f.lib.manual_playlist_members("unaffected").unwrap(),
        vec![f.ids[2].clone(), f.ids[0].clone()]
    );
    assert!(f.lib.apply_duplicate_merge(&preview.id, &token).is_err());
    drop(f.lib);
    f.lib = Library::open(&f.db).unwrap();
    let undone = f.lib.undo_duplicate_merge(&applied.id, &token).unwrap();
    assert_eq!(undone.undo_of.as_deref(), Some(applied.id.as_str()));
    assert_eq!(
        f.lib.duplicate_merge_history().unwrap()[0]
            .undone_by
            .as_deref(),
        Some(undone.id.as_str())
    );
    assert_eq!(f.content(), before);
    assert_eq!(
        f.lib.manual_playlist_members("affected").unwrap(),
        vec![f.ids[0].clone(), f.ids[2].clone(), f.ids[1].clone()]
    );
    assert!(f.lib.undo_duplicate_merge(&applied.id, &token).is_err());
    assert_eq!(
        hashes,
        f.paths
            .iter()
            .map(|p| cuerithm_media::file_sha256(p).unwrap())
            .collect::<Vec<_>>()
    );
}

#[test]
fn duplicate_merge_preview_and_undo_reject_later_review_playlist_source_and_aba_changes() {
    use cuerithm_library::{DuplicateReviewEdit, GroupEdit};
    let f = Fixture::new();
    let token = CancellationToken::default();
    let scan = f
        .lib
        .scan_duplicates(DuplicateScanRequest::default(), &f.runtime, &token)
        .unwrap();
    let review = f.lib.begin_duplicate_review(&scan.id, &token).unwrap();
    let preview = f
        .lib
        .preview_duplicate_merge(&review.id, 0, vec![review.groups[0].id.clone()], &token)
        .unwrap();
    f.lib
        .edit_duplicate_review(
            &review.id,
            0,
            DuplicateReviewEdit::Group {
                group_id: review.groups[0].id.clone(),
                edit: GroupEdit::Lock,
            },
            &token,
        )
        .unwrap();
    assert!(f.lib.apply_duplicate_merge(&preview.id, &token).is_err());
    assert!(
        !rusqlite::Connection::open(&f.db)
            .unwrap()
            .query_row(
                "SELECT archived FROM tracks WHERE id=?1",
                [&f.ids[1]],
                |r| r.get::<_, bool>(0)
            )
            .unwrap()
    );
    let f = Fixture::new();
    let scan = f
        .lib
        .scan_duplicates(DuplicateScanRequest::default(), &f.runtime, &token)
        .unwrap();
    let review = f.lib.begin_duplicate_review(&scan.id, &token).unwrap();
    let preview = f
        .lib
        .preview_duplicate_merge(&review.id, 0, vec![review.groups[0].id.clone()], &token)
        .unwrap();
    wav(&f.paths[1], 79);
    assert!(f.lib.apply_duplicate_merge(&preview.id, &token).is_err());
    let f = Fixture::new();
    let scan = f
        .lib
        .scan_duplicates(DuplicateScanRequest::default(), &f.runtime, &token)
        .unwrap();
    let review = f.lib.begin_duplicate_review(&scan.id, &token).unwrap();
    let preview = f
        .lib
        .preview_duplicate_merge(&review.id, 0, vec![review.groups[0].id.clone()], &token)
        .unwrap();
    let applied = f.lib.apply_duplicate_merge(&preview.id, &token).unwrap();
    f.lib.set_archived(&f.ids[0], true).unwrap();
    f.lib.set_archived(&f.ids[0], false).unwrap();
    assert!(f.lib.undo_duplicate_merge(&applied.id, &token).is_err());
}

#[test]
fn duplicate_merge_cancellation_and_pending_file_transfers_never_partially_archive() {
    use cuerithm_library::{DuplicateReviewEdit, GroupEdit};
    let f = Fixture::new();
    let token = CancellationToken::default();
    let scan = f
        .lib
        .scan_duplicates(DuplicateScanRequest::default(), &f.runtime, &token)
        .unwrap();
    let review = f.lib.begin_duplicate_review(&scan.id, &token).unwrap();
    let review = f
        .lib
        .edit_duplicate_review(
            &review.id,
            0,
            DuplicateReviewEdit::Group {
                group_id: review.groups[0].id.clone(),
                edit: GroupEdit::UseTags {
                    track_id: Some(f.ids[1].clone()),
                },
            },
            &token,
        )
        .unwrap();
    let before = f.content();
    let preview = f
        .lib
        .preview_duplicate_merge(
            &review.id,
            review.revision,
            vec![review.groups[0].id.clone()],
            &token,
        )
        .unwrap();
    assert_eq!(preview.plan.groups[0].copies.len(), 1);
    assert!(
        f.lib
            .apply_duplicate_merge(&preview.id, &token)
            .unwrap_err()
            .to_string()
            .contains("file publication")
    );
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert!(f.lib.apply_duplicate_merge(&preview.id, &cancel).is_err());
    assert_eq!(f.content(), before);
}

#[test]
fn duplicate_merge_and_undo_transaction_failures_leave_no_partial_changes_or_consumed_token() {
    let f = Fixture::new();
    let token = CancellationToken::default();
    f.playlist("affected", vec![f.ids[0].clone(), f.ids[1].clone()]);
    let scan = f
        .lib
        .scan_duplicates(DuplicateScanRequest::default(), &f.runtime, &token)
        .unwrap();
    let review = f.lib.begin_duplicate_review(&scan.id, &token).unwrap();
    let preview = f
        .lib
        .preview_duplicate_merge(&review.id, 0, vec![review.groups[0].id.clone()], &token)
        .unwrap();
    let before = f.content();
    let stamp = f.lib.duplicate_ignores().unwrap().stamp;
    let conn = rusqlite::Connection::open(&f.db).unwrap();
    conn.execute_batch("CREATE TRIGGER fail_merge_insert BEFORE INSERT ON playlist_members BEGIN SELECT RAISE(ABORT,'injected membership failure'); END;").unwrap();
    assert!(f.lib.apply_duplicate_merge(&preview.id, &token).is_err());
    assert_eq!(f.content(), before);
    assert_eq!(f.lib.duplicate_ignores().unwrap().stamp, stamp);
    assert_eq!(
        f.lib.manual_playlist_members("affected").unwrap(),
        vec![f.ids[0].clone(), f.ids[1].clone()]
    );
    conn.execute_batch("DROP TRIGGER fail_merge_insert;")
        .unwrap();
    let applied = f.lib.apply_duplicate_merge(&preview.id, &token).unwrap();
    let after = f.content();
    let after_stamp = f.lib.duplicate_ignores().unwrap().stamp;
    conn.execute_batch("CREATE TRIGGER fail_undo_delete BEFORE DELETE ON playlist_members BEGIN SELECT RAISE(ABORT,'injected undo failure'); END;").unwrap();
    assert!(f.lib.undo_duplicate_merge(&applied.id, &token).is_err());
    assert_eq!(f.content(), after);
    assert_eq!(f.lib.duplicate_ignores().unwrap().stamp, after_stamp);
    conn.execute_batch("DROP TRIGGER fail_undo_delete;")
        .unwrap();
    f.lib.undo_duplicate_merge(&applied.id, &token).unwrap();
    assert_eq!(f.content(), before);
}

#[test]
fn duplicate_merge_can_keep_an_archived_playlist_member_without_archiving_ignored_members() {
    use cuerithm_library::{DuplicateReviewEdit, GroupEdit};
    let f = Fixture::new();
    let token = CancellationToken::default();
    f.playlist(
        "picked",
        vec![f.ids[3].clone(), f.ids[0].clone(), f.ids[1].clone()],
    );
    let request = DuplicateScanRequest {
        scope: DuplicateScanScope::SelectedPlaylists,
        playlist_ids: vec!["picked".into()],
        ..Default::default()
    };
    let scan = f.lib.scan_duplicates(request, &f.runtime, &token).unwrap();
    let review = f.lib.begin_duplicate_review(&scan.id, &token).unwrap();
    let edit = |edit| DuplicateReviewEdit::Group {
        group_id: review.groups[0].id.clone(),
        edit,
    };
    let review = f
        .lib
        .edit_duplicate_review(
            &review.id,
            0,
            edit(GroupEdit::Select {
                track_id: f.ids[3].clone(),
            }),
            &token,
        )
        .unwrap();
    let review = f
        .lib
        .edit_duplicate_review(
            &review.id,
            1,
            edit(GroupEdit::ToggleIgnore {
                track_id: f.ids[1].clone(),
            }),
            &token,
        )
        .unwrap();
    let before = f.content();
    let preview = f
        .lib
        .preview_duplicate_merge(
            &review.id,
            review.revision,
            vec![review.groups[0].id.clone()],
            &token,
        )
        .unwrap();
    let applied = f.lib.apply_duplicate_merge(&preview.id, &token).unwrap();
    assert_eq!(applied.archived_ids, vec![f.ids[0].clone()]);
    assert_eq!(applied.restored_ids, vec![f.ids[3].clone()]);
    assert_eq!(
        f.lib.manual_playlist_members("picked").unwrap(),
        vec![f.ids[3].clone(), f.ids[1].clone()]
    );
    f.lib.undo_duplicate_merge(&applied.id, &token).unwrap();
    assert_eq!(f.content(), before);
}

#[test]
fn duplicate_merge_preserves_smart_definitions_and_rebuilds_members_after_apply_and_undo() {
    use cuerithm_library::SmartDefinition;
    use cuerithm_smartlists::{Operator, RuleValue, SmartRule};
    let f = Fixture::new();
    let token = CancellationToken::default();
    f.lib
        .apply_playlist_edit(
            PlaylistEdit::Create {
                id: Some("smart".into()),
                parent_id: None,
                kind: PlaylistKind::Smartlist,
                name: "Smart".into(),
                definition: Some(SmartDefinition {
                    match_all: true,
                    rules: vec![SmartRule {
                        id: "r".into(),
                        field: "title".into(),
                        operator: Operator::StringEquals,
                        value: RuleValue::Text("Signal".into()),
                        or: false,
                    }],
                }),
            },
            f.lib.playlist_state().unwrap().revision,
        )
        .unwrap();
    let now = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64
    };
    f.lib.refresh_smart_lists(now()).unwrap();
    assert_eq!(f.lib.playlist_members("smart").unwrap().len(), 2);
    let tree = f.lib.playlist_state().unwrap().nodes;
    let scan = f
        .lib
        .scan_duplicates(DuplicateScanRequest::default(), &f.runtime, &token)
        .unwrap();
    let review = f.lib.begin_duplicate_review(&scan.id, &token).unwrap();
    let preview = f
        .lib
        .preview_duplicate_merge(&review.id, 0, vec![review.groups[0].id.clone()], &token)
        .unwrap();
    assert!(preview.playlists.is_empty());
    let applied = f.lib.apply_duplicate_merge(&preview.id, &token).unwrap();
    assert_eq!(f.lib.playlist_state().unwrap().nodes, tree);
    assert_eq!(
        rusqlite::Connection::open(&f.db)
            .unwrap()
            .query_row(
                "SELECT state FROM smart_playlist_status WHERE playlist_id='smart'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "stale"
    );
    f.lib.refresh_smart_lists(now()).unwrap();
    assert_eq!(
        f.lib.playlist_members("smart").unwrap(),
        vec![f.ids[0].clone()]
    );
    f.lib.undo_duplicate_merge(&applied.id, &token).unwrap();
    f.lib.refresh_smart_lists(now()).unwrap();
    assert_eq!(f.lib.playlist_members("smart").unwrap().len(), 2);
    assert_eq!(f.lib.playlist_state().unwrap().nodes, tree);
}

#[test]
fn duplicate_merge_undo_uses_a_supported_journal_version_across_implementation_updates() {
    let f = Fixture::new();
    let token = CancellationToken::default();
    let scan = f
        .lib
        .scan_duplicates(DuplicateScanRequest::default(), &f.runtime, &token)
        .unwrap();
    let review = f.lib.begin_duplicate_review(&scan.id, &token).unwrap();
    let preview = f
        .lib
        .preview_duplicate_merge(&review.id, 0, vec![review.groups[0].id.clone()], &token)
        .unwrap();
    let before = f.content();
    let result = f.lib.apply_duplicate_merge(&preview.id, &token).unwrap();
    let conn = rusqlite::Connection::open(&f.db).unwrap();
    conn.execute("UPDATE maintenance_scans SET payload_json=json_set(payload_json,'$.schema','future-v2') WHERE id=?1",[&result.id]).unwrap();
    assert!(
        f.lib
            .undo_duplicate_merge(&result.id, &token)
            .unwrap_err()
            .to_string()
            .contains("Unsupported")
    );
    conn.execute("UPDATE maintenance_scans SET payload_json=json_set(payload_json,'$.schema','duplicate-library-merge-v1','$.logic','older-compatible-build') WHERE id=?1",[&result.id]).unwrap();
    f.lib.undo_duplicate_merge(&result.id, &token).unwrap();
    assert_eq!(f.content(), before);
}

#[test]
fn duplicate_merge_batches_disjoint_groups_with_interleaved_playlist_references() {
    let mut f = Fixture::new();
    let token = CancellationToken::default();
    for index in 4..6 {
        let path = f.dir.path().join(format!("track-{index}.wav"));
        wav(&path, 71);
        let job = f
            .lib
            .enqueue(
                std::slice::from_ref(&path),
                f.lib.settings().unwrap().revision,
            )
            .unwrap()[0]
            .clone();
        let id = f
            .lib
            .jobs()
            .unwrap()
            .into_iter()
            .find(|j| j.id == job)
            .unwrap()
            .track_id;
        rusqlite::Connection::open(&f.db).unwrap().execute("UPDATE tracks SET title='Parallel',artist='South',duration=20,state='ready' WHERE id=?1",[&id]).unwrap();
        f.ids.push(id);
        f.paths.push(path);
    }
    f.playlist(
        "interleaved",
        vec![
            f.ids[0].clone(),
            f.ids[4].clone(),
            f.ids[2].clone(),
            f.ids[1].clone(),
            f.ids[5].clone(),
        ],
    );
    let before = f.content();
    let scan = f
        .lib
        .scan_duplicates(DuplicateScanRequest::default(), &f.runtime, &token)
        .unwrap();
    assert_eq!(scan.groups.len(), 2);
    let review = f.lib.begin_duplicate_review(&scan.id, &token).unwrap();
    let preview = f
        .lib
        .preview_duplicate_merge(
            &review.id,
            0,
            review.groups.iter().rev().map(|g| g.id.clone()).collect(),
            &token,
        )
        .unwrap();
    assert_eq!(
        preview.playlists[0].after,
        vec![f.ids[2].clone(), f.ids[0].clone(), f.ids[4].clone()]
    );
    let result = f.lib.apply_duplicate_merge(&preview.id, &token).unwrap();
    assert_eq!(
        result
            .archived_ids
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>(),
        [f.ids[1].clone(), f.ids[5].clone()]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
    );
    assert_eq!(
        f.lib.manual_playlist_members("interleaved").unwrap(),
        preview.playlists[0].after
    );
    f.lib.undo_duplicate_merge(&result.id, &token).unwrap();
    assert_eq!(f.content(), before);
}

#[cfg(feature = "autotag")]
fn donor_preview(f: &Fixture) -> cuerithm_library::DuplicateMergePreview {
    use cuerithm_library::{DuplicateReviewEdit, GroupEdit};
    let conn = rusqlite::Connection::open(&f.db).unwrap();
    let publication = serde_json::json!({"grid":{"bpm":128.75,"startTime":0.125},"cues":[{"position":0,"name":"Donor breakdown","startTime":8.,"type":"1","color":"blue"}],"implementation_sha256":"a".repeat(64),"inputs_sha256":"b".repeat(64)});
    conn.execute("UPDATE tracks SET title='Donor title',genre='House',bpm=136.25,musical_key='8A',energy=8,analysis_json=?1 WHERE id=?2",rusqlite::params![serde_json::json!({"raw_donor":true,"publication":publication}).to_string(),f.ids[1]]).unwrap();
    conn.execute_batch("INSERT INTO custom_tag_categories VALUES('category','Mood'); INSERT INTO custom_tags VALUES('old-tag','category','Old'),('donor-tag','category','Warmup');").unwrap();
    conn.execute(
        "INSERT INTO track_custom_tags VALUES(?1,'old-tag')",
        [&f.ids[0]],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO track_custom_tags VALUES(?1,'donor-tag')",
        [&f.ids[1]],
    )
    .unwrap();
    drop(conn);
    let token = CancellationToken::default();
    let scan = f
        .lib
        .scan_duplicates(DuplicateScanRequest::default(), &f.runtime, &token)
        .unwrap();
    let review = f.lib.begin_duplicate_review(&scan.id, &token).unwrap();
    let review = f
        .lib
        .edit_duplicate_review(
            &review.id,
            0,
            DuplicateReviewEdit::Group {
                group_id: review.groups[0].id.clone(),
                edit: GroupEdit::UseTags {
                    track_id: Some(f.ids[1].clone()),
                },
            },
            &token,
        )
        .unwrap();
    f.lib
        .preview_duplicate_merge(
            &review.id,
            review.revision,
            vec![review.groups[0].id.clone()],
            &token,
        )
        .unwrap()
}

#[cfg(feature = "autotag")]
#[test]
fn coordinated_donor_merge_publishes_file_and_library_cues_grid_tags_and_reversible_archives() {
    use cuerithm_library::DuplicateMergeService;
    let mut f = Fixture::new();
    f.playlist(
        "set",
        vec![f.ids[0].clone(), f.ids[2].clone(), f.ids[1].clone()],
    );
    let preview = donor_preview(&f);
    let before = f.content_ignoring_file_identity();
    let tags_before = f.file_state(0);
    let hashes = f
        .paths
        .iter()
        .map(|p| cuerithm_media::file_sha256(p).unwrap())
        .collect::<Vec<_>>();
    let token = CancellationToken::default();
    let service = DuplicateMergeService::new(f.lib.clone(), f.runtime.clone());
    let result = service.apply(&preview.id, &token).unwrap();
    assert!(
        f.lib
            .duplicate_merge_history()
            .unwrap()
            .iter()
            .any(|h| h.result.id == result.id)
    );
    assert!(service.apply(&preview.id, &token).is_err());
    assert_eq!(f.lib.track(&f.ids[0]).unwrap().title, "Donor title");
    assert_eq!(f.lib.track(&f.ids[0]).unwrap().bpm, Some(128.75));
    let info = f.runtime.probe(&f.paths[0]).unwrap();
    let native = cuerithm_autotag::read_transfer_metadata(&f.paths[0], &info).unwrap();
    assert_eq!(native.bpm, Some(128.75));
    assert_eq!(
        native.extra.get(&cuerithm_autotag::TransferText::Energy),
        Some(&vec!["8".to_owned()])
    );
    let conn = rusqlite::Connection::open(&f.db).unwrap();
    let analysis: serde_json::Value = serde_json::from_str(
        &conn
            .query_row(
                "SELECT analysis_json FROM tracks WHERE id=?1",
                [&f.ids[0]],
                |r| r.get::<_, String>(0),
            )
            .unwrap(),
    )
    .unwrap();
    assert_eq!(analysis["untouched"], true);
    assert_eq!(
        analysis["publication"]["cues"][0]["name"],
        "Donor breakdown"
    );
    let links: Vec<String> = conn
        .prepare("SELECT tag_id FROM track_custom_tags WHERE track_id=?1")
        .unwrap()
        .query_map([&f.ids[0]], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(links, vec!["donor-tag"]);
    let cuejob: Option<String> = conn
        .query_row(
            "SELECT job_id FROM cue_revisions WHERE track_id=?1 ORDER BY rowid DESC LIMIT 1",
            [&f.ids[0]],
            |r| r.get(0),
        )
        .unwrap();
    assert!(cuejob.is_none());
    // The merge recorded the keeper's previous tag values, so no copy of its audio is kept.
    let named: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM maintenance_write_files WHERE backup IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(named, 0, "the journal names no copy of the original");
    let kept: Vec<_> = walkdir::WalkDir::new(f.lib.data_dir().join("maintenance"))
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .collect();
    assert!(kept.is_empty(), "audio left in the data folder: {kept:?}");
    drop(conn);
    assert_ne!(cuerithm_media::file_sha256(&f.paths[0]).unwrap(), hashes[0]);
    assert_eq!(cuerithm_media::file_sha256(&f.paths[1]).unwrap(), hashes[1]);
    drop(service);
    drop(f.lib);
    f.lib = Library::open(&f.db).unwrap();
    let service = DuplicateMergeService::new(f.lib.clone(), f.runtime.clone());
    service.undo(&result.id, &token).unwrap();
    assert_eq!(f.lib.track(&f.ids[0]).unwrap().title, "Signal");
    // Undo wrote the keeper's previous tag values back: its tags and audio are what they were,
    // and the library describes the file it now has.
    assert_eq!(f.file_state(0), tags_before);
    f.assert_library_knows_file(0);
    assert_eq!(f.content_ignoring_file_identity(), before);
    assert!(
        walkdir::WalkDir::new(f.lib.data_dir().join("maintenance"))
            .into_iter()
            .filter_map(|e| e.ok())
            .all(|e| !e.file_type().is_file()),
        "Undo leaves no copy of the audio either"
    );
    assert!(service.undo(&result.id, &token).is_err());
}

#[cfg(feature = "autotag")]
#[test]
fn coordinated_merge_rolls_back_exchanged_files_when_sql_publication_fails() {
    use cuerithm_library::DuplicateMergeService;
    let f = Fixture::new();
    let preview = donor_preview(&f);
    let before = f.content_ignoring_file_identity();
    let hash = cuerithm_media::file_sha256(&f.paths[0]).unwrap();
    let conn = rusqlite::Connection::open(&f.db).unwrap();
    conn.execute_batch("CREATE TRIGGER fail_donor_publish BEFORE UPDATE OF source_hash ON tracks WHEN NEW.source_hash IS NOT OLD.source_hash BEGIN SELECT RAISE(ABORT,'injected publication failure'); END;").unwrap();
    let service = DuplicateMergeService::new(f.lib.clone(), f.runtime.clone());
    assert!(
        service
            .apply(&preview.id, &CancellationToken::default())
            .is_err()
    );
    assert_eq!(f.content_ignoring_file_identity(), before);
    assert_eq!(cuerithm_media::file_sha256(&f.paths[0]).unwrap(), hash);
    conn.execute_batch("DROP TRIGGER fail_donor_publish;")
        .unwrap();
    let applied = service
        .apply(&preview.id, &CancellationToken::default())
        .unwrap();
    service
        .undo(&applied.id, &CancellationToken::default())
        .unwrap();
    assert_eq!(f.content_ignoring_file_identity(), before);
    f.assert_library_knows_file(0);
}

#[cfg(feature = "autotag")]
#[test]
fn coordinated_copy_only_and_unrelated_incoming_edits_preserve_durable_undo() {
    use cuerithm_library::{DuplicateMergeService, DuplicateReviewEdit, GroupEdit};
    let f = Fixture::new();
    let preview = donor_preview(&f);
    let token = CancellationToken::default();
    let review = f
        .lib
        .edit_duplicate_review(
            &preview.review_id,
            preview.review_revision,
            DuplicateReviewEdit::Group {
                group_id: preview.plan.groups[0].group_id.clone(),
                edit: GroupEdit::Lock,
            },
            &token,
        )
        .unwrap();
    let review = f
        .lib
        .edit_duplicate_review(
            &review.id,
            review.revision,
            DuplicateReviewEdit::Group {
                group_id: review.groups[0].id.clone(),
                edit: GroupEdit::Unlock { force: false },
            },
            &token,
        )
        .unwrap();
    let preview = f
        .lib
        .preview_duplicate_merge(
            &review.id,
            review.revision,
            vec![review.groups[0].id.clone()],
            &token,
        )
        .unwrap();
    let service = DuplicateMergeService::new(f.lib.clone(), f.runtime.clone());
    let applied = service.apply(&preview.id, &token).unwrap();
    assert!(applied.archived_ids.is_empty());
    let conn = rusqlite::Connection::open(&f.db).unwrap();
    conn.execute(
        "UPDATE tracks SET title='Unrelated incoming result' WHERE id=?1",
        [&f.ids[2]],
    )
    .unwrap();
    service.undo(&applied.id, &token).unwrap();
    assert_eq!(f.lib.track(&f.ids[0]).unwrap().title, "Signal");
    assert_eq!(
        f.lib.track(&f.ids[2]).unwrap().title,
        "Unrelated incoming result"
    );
    assert!(
        !conn
            .query_row(
                "SELECT archived FROM tracks WHERE id=?1",
                [&f.ids[1]],
                |r| r.get::<_, bool>(0)
            )
            .unwrap()
    );
}

#[cfg(feature = "autotag")]
#[test]
fn coordinated_undo_rejects_relevant_entity_aba_and_preserves_external_file_edits() {
    use cuerithm_library::DuplicateMergeService;
    let f = Fixture::new();
    let preview = donor_preview(&f);
    let token = CancellationToken::default();
    let service = DuplicateMergeService::new(f.lib.clone(), f.runtime.clone());
    let applied = service.apply(&preview.id, &token).unwrap();
    let conn = rusqlite::Connection::open(&f.db).unwrap();
    let title = f.lib.track(&f.ids[0]).unwrap().title;
    conn.execute(
        "UPDATE tracks SET title='Later edit' WHERE id=?1",
        [&f.ids[0]],
    )
    .unwrap();
    conn.execute(
        "UPDATE tracks SET title=?1 WHERE id=?2",
        rusqlite::params![title, f.ids[0]],
    )
    .unwrap();
    assert!(service.undo(&applied.id, &token).is_err());
    let f = Fixture::new();
    let preview = donor_preview(&f);
    let service = DuplicateMergeService::new(f.lib.clone(), f.runtime.clone());
    let applied = service.apply(&preview.id, &token).unwrap();
    wav(&f.paths[0], 83);
    let hash = cuerithm_media::file_sha256(&f.paths[0]).unwrap();
    assert!(service.undo(&applied.id, &token).is_err());
    assert_eq!(cuerithm_media::file_sha256(&f.paths[0]).unwrap(), hash);
}

#[test]
fn schema8_migration_preserves_legacy_analysis_cue_ownership_and_enables_manual_cues() {
    let f = Fixture::new();
    let conn = rusqlite::Connection::open(&f.db).unwrap();
    let job = f
        .lib
        .jobs()
        .unwrap()
        .into_iter()
        .find(|j| j.track_id == f.ids[0])
        .unwrap()
        .id;
    conn.execute(
        "INSERT INTO cue_revisions VALUES('legacy-cue',?1,?2,'{\"cues\":[]}',1)",
        rusqlite::params![f.ids[0], job],
    )
    .unwrap();
    #[derive(Debug, PartialEq)]
    struct LegacyTrackSnapshot {
        analysis: Option<String>,
        metadata: Option<String>,
        native_metadata: Option<String>,
        source_hash: String,
        current_job: Option<String>,
        path: Vec<u8>,
    }
    let snapshot = |conn: &rusqlite::Connection| {
        conn.query_row("SELECT analysis_json,metadata_json,native_metadata_json,source_hash,current_job,path FROM tracks WHERE id=?1", [&f.ids[0]], |r| Ok(LegacyTrackSnapshot { analysis:r.get(0)?,metadata:r.get(1)?,native_metadata:r.get(2)?,source_hash:r.get(3)?,current_job:r.get(4)?,path:r.get(5)? })).unwrap()
    };
    let before = snapshot(&conn);
    drop(conn);
    let path = f.dir.path().join("legacy/library.sqlite");
    std::fs::create_dir(path.parent().unwrap()).unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    for text in [
        include_str!("../migrations/001_initial.sql"),
        include_str!("../migrations/002_tag_writes.sql"),
        include_str!("../migrations/003_preparation_cache.sql"),
        include_str!("../migrations/004_cleanup.sql"),
        include_str!("../migrations/005_cleanup_preparation.sql"),
        include_str!("../migrations/006_playlists.sql"),
        include_str!("../migrations/007_maintenance.sql"),
    ] {
        conn.execute_batch(text).unwrap();
    }
    conn.execute(
        "ATTACH DATABASE ?1 AS seed",
        [f.db.to_string_lossy().to_string()],
    )
    .unwrap();
    // Explicit historical columns keep the fixture at schema 7 even when the
    // current seed gains additional columns (such as organization revisions).
    for (table, columns) in [
        ("settings", "revision,json,created"),
        ("selected_settings", "singleton,revision"),
        (
            "tracks",
            "id,path,title,artist,album,genre,search_text,sort_title,sort_artist,source_hash,state,archived,current_job,analysis_json,metadata_json,musical_key,bpm,energy,duration,created,updated,tagging_json,cleanup_json,native_metadata_json",
        ),
        (
            "jobs",
            "id,track_id,source_hash,settings_revision,state,stage,error,attempts,created,updated",
        ),
        ("cue_revisions", "id,track_id,job_id,json,created"),
    ] {
        conn.execute(
            &format!("INSERT INTO {table} ({columns}) SELECT {columns} FROM seed.{table}"),
            [],
        )
        .unwrap();
    }
    conn.execute_batch("DETACH DATABASE seed;").unwrap();
    drop(conn);
    let lib = Library::open(&path).unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        31
    );
    assert_eq!(
        conn.query_row(
            "SELECT job_id FROM cue_revisions WHERE id='legacy-cue'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        job
    );
    let after = snapshot(&conn);
    assert_eq!(
        after, before,
        "migration must preserve legacy analysis, metadata, source and job identity"
    );
    conn.execute(
        "INSERT INTO cue_revisions VALUES('manual-cue',?1,NULL,'{\"cues\":[]}',2)",
        [&f.ids[0]],
    )
    .unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| r
            .get::<_, u32>(
            0
        ))
        .unwrap(),
        0
    );
    drop(lib);
}

#[cfg(feature = "autotag")]
#[test]
fn coordinated_donor_publication_and_undo_cover_all_fourteen_audio_layouts() {
    use cuerithm_library::DuplicateMergeService;
    let data = tempfile::tempdir().unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    tar::Archive::new(flate2::read::GzDecoder::new(
        std::fs::File::open(root.join("tools/fixtures/media.tar.gz")).unwrap(),
    ))
    .unpack(data.path())
    .unwrap();
    let cases: serde_json::Value =
        serde_json::from_slice(&std::fs::read(data.path().join("cases.json")).unwrap()).unwrap();
    assert_eq!(cases.as_array().unwrap().len(), 14);
    for case in cases.as_array().unwrap() {
        let f = Fixture::new();
        let source = data.path().join(case["file"].as_str().unwrap());
        for index in 0..2 {
            std::fs::copy(&source, &f.paths[index]).unwrap();
            let hash = cuerithm_media::file_sha256(&f.paths[index]).unwrap();
            rusqlite::Connection::open(&f.db)
                .unwrap()
                .execute(
                    "UPDATE tracks SET source_hash=?1 WHERE id=?2",
                    rusqlite::params![hash, f.ids[index]],
                )
                .unwrap();
        }
        let preview = donor_preview(&f);
        let before = f.content_ignoring_file_identity();
        let tags_before = f.file_state(0);
        let hash = cuerithm_media::file_sha256(&f.paths[0]).unwrap();
        let donor_hash = cuerithm_media::file_sha256(&f.paths[1]).unwrap();
        let token = CancellationToken::default();
        let service = DuplicateMergeService::new(f.lib.clone(), f.runtime.clone());
        let applied = service
            .apply(&preview.id, &token)
            .unwrap_or_else(|e| panic!("apply {}: {e:#}", case["file"]));
        assert_eq!(f.lib.track(&f.ids[0]).unwrap().bpm, Some(128.75));
        assert_eq!(
            cuerithm_media::file_sha256(&f.paths[1]).unwrap(),
            donor_hash
        );
        service
            .undo(&applied.id, &token)
            .unwrap_or_else(|e| panic!("undo {}: {e:#}", case["file"]));
        assert_eq!(f.content_ignoring_file_identity(), before);
        // Every tag and every decoded sample are what they were, in every format.
        assert_eq!(f.file_state(0), tags_before, "{}", case["file"]);
        f.assert_library_knows_file(0);
        let _ = hash;
    }
}

#[test]
fn exact_audio_groups_require_equal_native_rate_and_channels_even_with_cached_equal_digests() {
    let f = Fixture::new();
    for (index, &(sample_rate, channels)) in [(8000, 1), (8000, 1), (16000, 1), (8000, 2)]
        .iter()
        .enumerate()
    {
        let mut writer = hound::WavWriter::create(
            &f.paths[index],
            hound::WavSpec {
                channels,
                sample_rate,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        // The same interleaved s16le bytes play differently under the changed headers.
        for sample in 0..320000 {
            writer
                .write_sample::<i16>(((sample * 37) % 20001 - 10000) as i16)
                .unwrap();
        }
        writer.finalize().unwrap();
        let conn = rusqlite::Connection::open(&f.db).unwrap();
        conn.execute("UPDATE tracks SET archived=0,state='ready',title=?1,duration=?2,source_hash=?3 WHERE id=?4", rusqlite::params![
            format!("Different recording {index}"),
            320000. / f64::from(sample_rate) / f64::from(channels),
            cuerithm_media::file_sha256(&f.paths[index]).unwrap(), &f.ids[index],
        ]).unwrap();
    }
    let token = CancellationToken::default();
    let first = f
        .lib
        .scan_duplicates(DuplicateScanRequest::default(), &f.runtime, &token)
        .unwrap();
    assert!(!first.stale);
    assert_eq!(first.fingerprints_computed, 4);
    let fingerprints = first
        .tracks
        .iter()
        .map(|track| track.fingerprint.as_ref().unwrap())
        .collect::<Vec<_>>();
    assert!(
        fingerprints
            .iter()
            .all(|fp| fp.digest == fingerprints[0].digest)
    );
    assert_eq!(
        fingerprints
            .iter()
            .map(|fp| (fp.sample_rate, fp.channels))
            .collect::<Vec<_>>(),
        [(8000, 1), (8000, 1), (16000, 1), (8000, 2)]
    );
    let expected = vec![
        [f.ids[0].clone(), f.ids[1].clone()]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>(),
    ];
    assert_eq!(sets(&first), expected);
    assert!(matches!(
        first.groups[0].method,
        DuplicateMatchMethod::Fingerprint
    ));
    let second = f
        .lib
        .scan_duplicates(DuplicateScanRequest::default(), &f.runtime, &token)
        .unwrap();
    assert_eq!(second.cache_hits, 4);
    assert_eq!(second.fingerprints_computed, 0);
    assert_eq!(sets(&second), expected);
}

#[test]
fn duplicate_classifications_from_previous_logic_cannot_be_reviewed_or_applied() {
    let f = Fixture::new();
    let token = CancellationToken::default();
    let scan = f
        .lib
        .scan_duplicates(DuplicateScanRequest::default(), &f.runtime, &token)
        .unwrap();
    let review = f.lib.begin_duplicate_review(&scan.id, &token).unwrap();
    let preview = f
        .lib
        .preview_duplicate_merge(&review.id, 0, vec![review.groups[0].id.clone()], &token)
        .unwrap();
    let before = f.content();
    let conn = rusqlite::Connection::open(&f.db).unwrap();
    for (id, path) in [
        (&scan.id, "$.logic"),
        (&review.id, "$.scan.logic"),
        (&preview.id, "$.review.scan.logic"),
    ] {
        conn.execute("UPDATE maintenance_scans SET payload_json=json_set(payload_json,?1,'previous-digest-only-grouping') WHERE id=?2", rusqlite::params![path, id]).unwrap();
    }
    let outdated = f.lib.duplicate_scan(&scan.id).unwrap();
    assert!(outdated.stale);
    assert!(outdated.stale_reason.unwrap().contains("logic changed"));
    assert!(f.lib.begin_duplicate_review(&scan.id, &token).is_err());
    assert!(f.lib.duplicate_review(&review.id, &token).unwrap().stale);
    assert!(
        f.lib
            .preview_duplicate_merge(&review.id, 0, vec![review.groups[0].id.clone()], &token)
            .is_err()
    );
    assert!(f.lib.apply_duplicate_merge(&preview.id, &token).is_err());
    assert_eq!(f.content(), before);
}
