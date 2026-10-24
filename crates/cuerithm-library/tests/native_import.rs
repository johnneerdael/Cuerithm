use cuerithm_library::*;
use cuerithm_media::{CancellationToken, file_sha256};
use cuerithm_rekordbox::*;
use serde_json::{Value, json};
use sha2::Digest;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

fn track(db: &str, id: &str, path: &Path) -> ImportedTrack {
    serde_json::from_value(json!({
 "identity":{"database_id":db,"content_id":id,"uuid":format!("uuid-{id}"),"master_song_id":format!("master-{id}")},
 "metadata":{"title":format!("Target {id}"),"artist":"Original Artist","album":"Album","album_artist":"Album Artist","genre":"House","label":"Label","key":"Am","comment":"Comment","remixer":"Remixer","composer":"Composer","lyricist":"Lyricist","mix_name":"Extended","rating":4},
 "source":{"path":path.to_string_lossy(),"resolved_path":path.to_string_lossy(),"service_id":0,"analysis_path":"/PIONEER/USBANLZ/generated.DAT","artwork_path":"/Artwork/generated.jpg"},
 "bpm":128.5,"duration_seconds":240.25,"deleted":false,"cues":[],"tag_ids":[],"raw_hash":"a".repeat(64),"metadata_hash":"b".repeat(64)
 })).unwrap()
}
fn collection(db: &str, tracks: Vec<ImportedTrack>) -> RekordboxCollection {
    let mut records: BTreeMap<String, Vec<TargetRecord>> = [
        "djmdProperty",
        "agentRegistry",
        "djmdContent",
        "djmdArtist",
        "djmdAlbum",
        "djmdGenre",
        "djmdLabel",
        "djmdKey",
        "djmdCue",
        "contentCue",
        "djmdPlaylist",
        "djmdSongPlaylist",
        "djmdMyTag",
        "djmdSongMyTag",
    ]
    .into_iter()
    .map(|s| (s.to_owned(), vec![]))
    .collect();
    for t in &tracks {
        records.get_mut("djmdContent").unwrap().push(TargetRecord {
            table: "djmdContent".into(),
            id: t.identity.content_id.clone(),
            fingerprint: t.raw_hash.clone(),
            values: BTreeMap::from([
                (
                    "ID".into(),
                    TargetValue::Text(t.identity.content_id.clone()),
                ),
                ("DJPlayCount".into(), TargetValue::Integer(91)),
                ("Opaque".into(), TargetValue::Blob(vec![0, 255, 7])),
                ("Nullable".into(), TargetValue::Null),
            ]),
        });
    }
    RekordboxCollection {
        identity: DatabaseIdentity {
            database_id: db.into(),
            device_id: Some("device-1".into()),
            database_version: Some("7".into()),
            local_update_count: Some(17),
        },
        schema_fingerprint: "c".repeat(64),
        tracks,
        playlists: vec![],
        playlist_links: vec![],
        tags: vec![],
        tag_links: vec![],
        records,
        issues: vec![],
    }
}
fn options(lib: &Library) -> NativeImportOptions {
    NativeImportOptions {
        settings_revision: lib.settings().unwrap().revision,
        content_ids: None,
        initial_owners: BTreeMap::from([
            ("title".into(), NativeFieldOwner::Shared),
            ("bpm".into(), NativeFieldOwner::Local),
        ]),
    }
}
fn source(dir: &Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, b"unchanged source audio fixture").unwrap();
    p
}

#[test]
fn native_reconciliation_review_survives_restart_without_acknowledging_or_writing() {
    let d = tempfile::tempdir().unwrap();
    let path = source(d.path(), "review.flac");
    let before = file_sha256(&path).unwrap();
    let db = d.path().join("library.sqlite");
    let lib = Library::open(&db).unwrap();
    let input = collection("review-db", vec![track("review-db", "1", &path)]);
    let imported = lib
        .import_rekordbox_collection(&input, options(&lib), &CancellationToken::default())
        .unwrap();
    let preview = lib
        .preview_native_reconciliation(&imported.target_id, "1")
        .unwrap();
    assert!(matches!(
        preview.snapshot.fields["title"].result,
        NativeReconciliation::Conflict {
            reason: NativeConflict::InitialAcknowledgement,
            ..
        }
    ));
    let decisions = BTreeMap::from([("title".into(), NativeFieldDecision::AcceptEqual)]);
    let reviewed = lib
        .review_native_reconciliation(&preview.id, 0, decisions.clone())
        .unwrap();
    assert_eq!(reviewed.revision, 1);
    assert!(reviewed.stale_reason.is_none());
    assert!(
        lib.review_native_reconciliation(&preview.id, 0, decisions.clone())
            .is_err()
    );
    drop(lib);
    let lib = Library::open(&db).unwrap();
    let restored = lib.native_reconciliation_review(&preview.id).unwrap();
    assert_eq!(restored.decisions, decisions);
    assert_eq!(restored.revision, 1);
    assert!(restored.stale_reason.is_none());
    assert!(
        lib.native_track_link(&imported.target_id, "1")
            .unwrap()
            .fields
            .values()
            .all(|f| !f.acknowledged)
    );
    assert_eq!(file_sha256(&path).unwrap(), before);
}

#[test]
fn native_reconciliation_accepts_a_new_update_counter_for_the_same_recording() {
    let d = tempfile::tempdir().unwrap();
    let path = source(d.path(), "updated-review.flac");
    let lib = Library::open(&d.path().join("library.sqlite")).unwrap();
    let mut input = collection("review-db", vec![track("review-db", "1", &path)]);
    input.tracks[0].identity.local_usn = Some(1);
    let imported = lib
        .import_rekordbox_collection(&input, options(&lib), &CancellationToken::default())
        .unwrap();
    input.tracks[0].identity.local_usn = Some(2);
    input.tracks[0].metadata.title = "A legitimate edit".into();
    lib.import_rekordbox_collection(&input, options(&lib), &CancellationToken::default())
        .unwrap();
    let preview = lib
        .preview_native_reconciliation(&imported.target_id, "1")
        .unwrap();
    assert_eq!(
        preview.snapshot.fields["title"].remote,
        NativeFieldObservation::Present(json!(["A legitimate edit"]))
    );
}

#[test]
fn native_reconciliation_rejects_local_edits_even_when_the_value_changes_back() {
    let d = tempfile::tempdir().unwrap();
    let path = source(d.path(), "aba-review.flac");
    let db = d.path().join("library.sqlite");
    let lib = Library::open(&db).unwrap();
    let input = collection("review-db", vec![track("review-db", "1", &path)]);
    let imported = lib
        .import_rekordbox_collection(&input, options(&lib), &CancellationToken::default())
        .unwrap();
    let preview = lib
        .preview_native_reconciliation(&imported.target_id, "1")
        .unwrap();
    let local_id = &preview.snapshot.local_track_id;
    let sql = rusqlite::Connection::open(&db).unwrap();
    sql.execute(
        "UPDATE tracks SET title='temporary edit' WHERE id=?1",
        [local_id],
    )
    .unwrap();
    sql.execute("UPDATE tracks SET title='Target 1' WHERE id=?1", [local_id])
        .unwrap();
    assert!(
        lib.native_reconciliation_review(&preview.id)
            .unwrap()
            .stale_reason
            .is_some()
    );
    assert!(
        lib.review_native_reconciliation(
            &preview.id,
            0,
            BTreeMap::from([("title".into(), NativeFieldDecision::AcceptEqual)])
        )
        .is_err()
    );
}

#[test]
fn native_reconciliation_rejects_changed_observations_unknown_fields_and_false_equality() {
    let d = tempfile::tempdir().unwrap();
    let path = source(d.path(), "stale-review.flac");
    let lib = Library::open(&d.path().join("library.sqlite")).unwrap();
    let mut input = collection("review-db", vec![track("review-db", "1", &path)]);
    let imported = lib
        .import_rekordbox_collection(&input, options(&lib), &CancellationToken::default())
        .unwrap();
    let preview = lib
        .preview_native_reconciliation(&imported.target_id, "1")
        .unwrap();
    assert!(
        lib.review_native_reconciliation(
            &preview.id,
            0,
            BTreeMap::from([("not-a-field".into(), NativeFieldDecision::UseLocal)])
        )
        .is_err()
    );
    assert!(
        lib.review_native_reconciliation(
            &preview.id,
            0,
            BTreeMap::from([("bpm".into(), NativeFieldDecision::AcceptEqual)])
        )
        .is_err()
    );
    input.tracks[0].metadata.title = "A new remote title".into();
    lib.import_rekordbox_collection(&input, options(&lib), &CancellationToken::default())
        .unwrap();
    assert!(
        lib.native_reconciliation_review(&preview.id)
            .unwrap()
            .stale_reason
            .is_some()
    );
    assert!(
        lib.review_native_reconciliation(
            &preview.id,
            0,
            BTreeMap::from([("title".into(), NativeFieldDecision::UseLocal)])
        )
        .is_err()
    );
    let fresh = lib
        .preview_native_reconciliation(&imported.target_id, "1")
        .unwrap();
    assert!(
        lib.review_native_reconciliation(
            &fresh.id,
            0,
            BTreeMap::from([("title".into(), NativeFieldDecision::UseRemote)])
        )
        .is_ok()
    );
    input.tracks[0].identity.uuid = Some("different-recording".into());
    lib.import_rekordbox_collection(&input, options(&lib), &CancellationToken::default())
        .unwrap();
    assert!(
        lib.preview_native_reconciliation(&imported.target_id, "1")
            .is_err()
    );
}

#[test]
fn native_import_queues_new_sources_persists_scoped_ids_and_the_records_sync_reads_and_is_idempotent()
 {
    let d = tempfile::tempdir().unwrap();
    let path = source(d.path(), "song.flac");
    let before = file_sha256(&path).unwrap();
    let db = d.path().join("library.sqlite");
    let lib = Library::open(&db).unwrap();
    let mut input = collection("db-1", vec![track("db-1", "1", &path)]);
    for table in [
        "djmdPlaylist",
        "djmdSongPlaylist",
        "djmdMyTag",
        "djmdSongMyTag",
    ] {
        input.records.get_mut(table).unwrap().push(TargetRecord {
            table: table.into(),
            id: "r1".into(),
            fingerprint: "d".repeat(64),
            values: BTreeMap::from([
                ("Opaque".into(), TargetValue::Blob(vec![0, 255, 7])),
                ("Nullable".into(), TargetValue::Null),
            ]),
        });
    }
    let opts = options(&lib);
    let first = lib
        .import_rekordbox_collection(&input, opts.clone(), &CancellationToken::default())
        .unwrap();
    assert_eq!(first.tracks_created, 1);
    assert_eq!(first.jobs_created, 1);
    assert!(first.changed);
    assert_eq!(lib.counts().unwrap().incoming, 1);
    let link = lib.native_track_link(&first.target_id, "1").unwrap();
    let id = link.local_track_id.clone().unwrap();
    assert_eq!(lib.track(&id).unwrap().title, "Target 1");
    assert_eq!(link.fields["title"].remote, json!(["Target 1"]));
    assert_eq!(link.fields["title"].owner, NativeFieldOwner::Shared);
    assert!(!link.fields["title"].acknowledged);
    assert_eq!(link.fields["bpm"].local, Value::Null);
    assert_eq!(link.fields["bpm"].remote, json!(128.5));
    // The tables native playlist sync reads back keep their records losslessly.
    for table in [
        "djmdPlaylist",
        "djmdSongPlaylist",
        "djmdMyTag",
        "djmdSongMyTag",
    ] {
        let raw = lib
            .native_record(&first.target_id, table, "r1")
            .unwrap()
            .unwrap();
        assert_eq!(raw.values["Opaque"], TargetValue::Blob(vec![0, 255, 7]));
        assert_eq!(raw.values["Nullable"], TargetValue::Null);
        assert_eq!(raw.fingerprint, "d".repeat(64));
    }
    // Any other record keeps its identity and the hash of its content, not the content.
    assert!(
        lib.native_record(&first.target_id, "djmdContent", "1")
            .unwrap_err()
            .to_string()
            .contains("not kept")
    );
    assert!(
        lib.native_record(&first.target_id, "djmdContent", "missing")
            .unwrap()
            .is_none()
    );
    let sql = rusqlite::Connection::open(&db).unwrap();
    let (stored, hash): (String, String) = sql
        .query_row(
            "SELECT json,hash FROM native_records WHERE table_name='djmdContent' AND record_id='1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(stored, "");
    let record = &input.records["djmdContent"][0];
    assert_eq!(
        hash,
        format!(
            "{:x}",
            sha2::Sha256::digest(serde_json::to_string(record).unwrap().as_bytes())
        )
    );
    drop(sql);
    let again = lib
        .import_rekordbox_collection(&input, opts, &CancellationToken::default())
        .unwrap();
    assert!(!again.changed);
    assert_eq!(again.revision, first.revision);
    assert_eq!(again.tracks_created, 0);
    assert_eq!(lib.jobs().unwrap().len(), 1);
    assert_eq!(file_sha256(&path).unwrap(), before);
    drop(lib);
    let reopened = Library::open(&db).unwrap();
    assert_eq!(
        reopened
            .native_track_link(&first.target_id, "1")
            .unwrap()
            .local_track_id,
        Some(id)
    );
    assert_eq!(
        reopened
            .native_target(&first.target_id)
            .unwrap()
            .identity
            .database_id,
        "db-1"
    );
}
#[test]
fn existing_local_edits_and_first_field_baselines_survive_later_observations() {
    let d = tempfile::tempdir().unwrap();
    let p = source(d.path(), "local.flac");
    let db = d.path().join("db");
    let lib = Library::open(&db).unwrap();
    lib.enqueue(std::slice::from_ref(&p), lib.settings().unwrap().revision)
        .unwrap();
    let id = lib
        .track_ids(TrackQuery {
            view: TrackView::Incoming,
            ..Default::default()
        })
        .unwrap()[0]
        .clone();
    let sql = rusqlite::Connection::open(&db).unwrap();
    sql.execute(
        "UPDATE tracks SET state='ready',title='Local edit',bpm=127,analysis_json='{}' WHERE id=?1",
        [&id],
    )
    .unwrap();
    let opts = options(&lib);
    let mut input = collection("db-1", vec![track("db-1", "1", &p)]);
    let first = lib
        .import_rekordbox_collection(&input, opts.clone(), &CancellationToken::default())
        .unwrap();
    assert_eq!(first.tracks_created, 0);
    assert_eq!(lib.track(&id).unwrap().title, "Local edit");
    assert_eq!(lib.track(&id).unwrap().state, "ready");
    let baseline = lib.native_track_link(&first.target_id, "1").unwrap().fields;
    sql.execute(
        "UPDATE tracks SET title='Newer local edit' WHERE id=?1",
        [&id],
    )
    .unwrap();
    input.tracks[0].metadata.title = "Newer remote edit".into();
    let second = lib
        .import_rekordbox_collection(&input, opts, &CancellationToken::default())
        .unwrap();
    assert!(second.changed);
    assert_eq!(lib.track(&id).unwrap().title, "Newer local edit");
    let link = lib.native_track_link(&first.target_id, "1").unwrap();
    assert_eq!(link.fields, baseline);
    assert_eq!(
        link.observation.unwrap().metadata.title,
        "Newer remote edit"
    );
    assert_eq!(lib.jobs().unwrap().len(), 1);
}
#[test]
fn duplicate_native_paths_and_same_ids_in_distinct_databases_never_lose_external_identity() {
    let d = tempfile::tempdir().unwrap();
    let p = source(d.path(), "shared.flac");
    let lib = Library::open(&d.path().join("db")).unwrap();
    let opts = options(&lib);
    let a = lib
        .import_rekordbox_collection(
            &collection("db-A", vec![track("db-A", "1", &p), track("db-A", "2", &p)]),
            opts.clone(),
            &CancellationToken::default(),
        )
        .unwrap();
    let one = lib.native_track_link(&a.target_id, "1").unwrap();
    let two = lib.native_track_link(&a.target_id, "2").unwrap();
    assert_eq!(one.local_track_id, two.local_track_id);
    assert_eq!(a.tracks_created, 1);
    assert_eq!(lib.jobs().unwrap().len(), 1);
    let b = lib
        .import_rekordbox_collection(
            &collection("db-B", vec![track("db-B", "1", &p)]),
            opts,
            &CancellationToken::default(),
        )
        .unwrap();
    assert_ne!(a.target_id, b.target_id);
    assert_eq!(
        lib.native_track_link(&b.target_id, "1")
            .unwrap()
            .local_track_id,
        one.local_track_id
    );
    assert_eq!(lib.native_target(&a.target_id).unwrap().tracks, 2);
    assert_eq!(lib.native_target(&b.target_id).unwrap().tracks, 1);
}
#[test]
fn missing_unselected_deleted_and_identity_changed_records_are_retained_without_overwriting_tracks()
{
    let d = tempfile::tempdir().unwrap();
    let p = source(d.path(), "known.flac");
    let missing = d.path().join("missing.flac");
    let lib = Library::open(&d.path().join("db")).unwrap();
    let mut input = collection(
        "db-1",
        vec![track("db-1", "1", &p), track("db-1", "2", &missing)],
    );
    let opts = options(&lib);
    let first = lib
        .import_rekordbox_collection(&input, opts.clone(), &CancellationToken::default())
        .unwrap();
    assert_eq!(
        lib.native_track_link(&first.target_id, "2").unwrap().status,
        "missing_source"
    );
    assert_eq!(first.tracks_created, 1);
    source(d.path(), "missing.flac");
    let again = lib
        .import_rekordbox_collection(&input, opts.clone(), &CancellationToken::default())
        .unwrap();
    assert_eq!(again.tracks_created, 1);
    let old = lib.native_track_link(&first.target_id, "1").unwrap();
    input.tracks[0].identity.uuid = Some("different-object".into());
    lib.import_rekordbox_collection(&input, opts.clone(), &CancellationToken::default())
        .unwrap();
    let conflict = lib.native_track_link(&first.target_id, "1").unwrap();
    assert_eq!(conflict.status, "identity_changed");
    assert_eq!(conflict.local_track_id, old.local_track_id);
    assert_eq!(conflict.fields, old.fields);
    input.tracks.remove(0);
    input
        .records
        .get_mut("djmdContent")
        .unwrap()
        .retain(|r| r.id != "1");
    lib.import_rekordbox_collection(&input, opts.clone(), &CancellationToken::default())
        .unwrap();
    assert_eq!(
        lib.native_track_link(&first.target_id, "1").unwrap().status,
        "missing_from_target"
    );
    assert_eq!(lib.counts().unwrap().incoming, 2);
    input.tracks[0].deleted = Some(true);
    lib.import_rekordbox_collection(&input, opts, &CancellationToken::default())
        .unwrap();
    assert_eq!(
        lib.native_track_link(&first.target_id, "2").unwrap().status,
        "deleted_in_target"
    );
}
#[test]
fn sql_failure_rolls_back_tracks_jobs_records_targets_and_baselines_as_one_import() {
    let d = tempfile::tempdir().unwrap();
    let a = source(d.path(), "a.wav");
    let b = source(d.path(), "b.wav");
    let db = d.path().join("db");
    let lib = Library::open(&db).unwrap();
    let sql = rusqlite::Connection::open(&db).unwrap();
    sql.execute_batch("CREATE TRIGGER import_failure BEFORE INSERT ON native_track_links WHEN NEW.content_id='2' BEGIN SELECT RAISE(ABORT,'injected failure');END;").unwrap();
    let result = lib.import_rekordbox_collection(
        &collection("db-1", vec![track("db-1", "1", &a), track("db-1", "2", &b)]),
        options(&lib),
        &CancellationToken::default(),
    );
    assert!(result.is_err());
    assert_eq!(lib.counts().unwrap().incoming, 0);
    assert!(lib.jobs().unwrap().is_empty());
    let count: i64 = sql
        .query_row("SELECT COUNT(*) FROM native_targets", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
    assert_eq!(file_sha256(&a).unwrap(), file_sha256(&b).unwrap());
}
#[test]
fn malformed_scopes_selections_and_cancellation_do_not_publish_partial_observations() {
    let d = tempfile::tempdir().unwrap();
    let p = source(d.path(), "song.flac");
    let lib = Library::open(&d.path().join("db")).unwrap();
    let input = collection("db-1", vec![track("db-1", "1", &p)]);
    let opts = options(&lib);
    let token = CancellationToken::default();
    token.cancel();
    assert!(
        lib.import_rekordbox_collection(&input, opts.clone(), &token)
            .is_err()
    );
    let mut bad = input.clone();
    bad.tracks[0].identity.database_id = "other".into();
    assert!(
        lib.import_rekordbox_collection(&bad, opts.clone(), &CancellationToken::default())
            .is_err()
    );
    let mut opts = opts;
    opts.content_ids = Some(["unknown".into()].into());
    assert!(
        lib.import_rekordbox_collection(&input, opts, &CancellationToken::default())
            .is_err()
    );
    assert_eq!(lib.counts().unwrap().incoming, 0);
}

#[test]
fn changing_selection_can_link_previously_unselected_files_without_resetting_existing_ownership() {
    let d = tempfile::tempdir().unwrap();
    let a = source(d.path(), "one.wav");
    let b = source(d.path(), "two.wav");
    let lib = Library::open(&d.path().join("db")).unwrap();
    let input = collection("db-1", vec![track("db-1", "1", &a), track("db-1", "2", &b)]);
    let mut opts = options(&lib);
    opts.content_ids = Some(["1".into()].into());
    let first = lib
        .import_rekordbox_collection(&input, opts.clone(), &CancellationToken::default())
        .unwrap();
    assert_eq!(first.tracks_created, 1);
    assert_eq!(
        lib.native_track_link(&first.target_id, "2").unwrap().status,
        "unselected"
    );
    opts.content_ids = None;
    opts.initial_owners
        .insert("title".into(), NativeFieldOwner::Remote);
    let next = lib
        .import_rekordbox_collection(&input, opts, &CancellationToken::default())
        .unwrap();
    assert_eq!(next.tracks_created, 1);
    assert!(next.changed);
    assert_eq!(
        lib.native_track_link(&first.target_id, "1").unwrap().fields["title"].owner,
        NativeFieldOwner::Shared
    );
    assert_eq!(
        lib.native_track_link(&first.target_id, "2").unwrap().fields["title"].owner,
        NativeFieldOwner::Remote
    );
}
#[test]
fn schema_ten_migration_preserves_legacy_links_and_native_import_enforces_foreign_keys() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("db");
    let conn = rusqlite::Connection::open(&path).unwrap();
    for script in [
        include_str!("../migrations/001_initial.sql"),
        include_str!("../migrations/002_tag_writes.sql"),
        include_str!("../migrations/003_preparation_cache.sql"),
        include_str!("../migrations/004_cleanup.sql"),
        include_str!("../migrations/005_cleanup_preparation.sql"),
        include_str!("../migrations/006_playlists.sql"),
        include_str!("../migrations/007_maintenance.sql"),
        include_str!("../migrations/008_maintenance_writes.sql"),
        include_str!("../migrations/009_unused_removal.sql"),
        include_str!("../migrations/010_unused_retention.sql"),
    ] {
        conn.execute_batch(script).unwrap();
    }
    conn.execute("INSERT INTO tracks(id,path,title,source_hash,state,created,updated) VALUES('legacy',?1,'Legacy','hash','ready',0,0)",[b"unix:/unchanged/native/path.wav".as_slice()]).unwrap();
    conn.execute("INSERT INTO sync_links(track_id,target,external_id,baseline_json) VALUES('legacy','old-target','external','{\"keep\":true}')",[]).unwrap();
    drop(conn);
    let lib = Library::open(&path).unwrap();
    let sql = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        sql.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        31
    );
    assert_eq!(
        sql.query_row(
            "SELECT baseline_json FROM sync_links WHERE track_id='legacy'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "{\"keep\":true}"
    );
    assert_eq!(
        sql.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| r
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        0
    );
    assert_eq!(lib.counts().unwrap().tracks, 1);
}
#[test]
fn incomplete_observations_and_unrelated_registry_rows_never_replace_a_saved_snapshot() {
    let d = tempfile::tempdir().unwrap();
    let p = source(d.path(), "song.wav");
    let lib = Library::open(&d.path().join("db")).unwrap();
    let input = collection("db-1", vec![track("db-1", "1", &p)]);
    let opts = options(&lib);
    let first = lib
        .import_rekordbox_collection(&input, opts.clone(), &CancellationToken::default())
        .unwrap();
    let mut bad = input.clone();
    bad.records.remove("djmdCue");
    assert!(
        lib.import_rekordbox_collection(&bad, opts.clone(), &CancellationToken::default())
            .is_err()
    );
    let mut bad = input;
    bad.records
        .get_mut("agentRegistry")
        .unwrap()
        .push(TargetRecord {
            table: "agentRegistry".into(),
            id: "unrelated-token".into(),
            values: BTreeMap::new(),
            fingerprint: "d".repeat(64),
        });
    assert!(
        lib.import_rekordbox_collection(&bad, opts, &CancellationToken::default())
            .is_err()
    );
    assert_eq!(
        lib.native_target(&first.target_id).unwrap().revision,
        first.revision
    );
}

#[test]
fn mapped_local_paths_do_not_replace_the_native_remote_path_in_a_field_baseline() {
    let d = tempfile::tempdir().unwrap();
    let path = source(d.path(), "mapped.wav");
    let lib = Library::open(&d.path().join("db")).unwrap();
    let mut t = track("db-1", "1", &path);
    t.source.path = Some("/other-device/original.wav".into());
    let report = lib
        .import_rekordbox_collection(
            &collection("db-1", vec![t]),
            options(&lib),
            &CancellationToken::default(),
        )
        .unwrap();
    let link = lib.native_track_link(&report.target_id, "1").unwrap();
    assert_eq!(
        link.fields["source_path"].remote,
        json!("/other-device/original.wav")
    );
    assert_eq!(
        link.fields["source_path"].local,
        json!(path.canonicalize().unwrap().to_string_lossy())
    );
}
