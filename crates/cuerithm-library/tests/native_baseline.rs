//! The library keeps a lean copy of what it read from a DJ program's database: the identity and
//! hash of every record, the content only of the tables native playlist sync reads back, and an
//! observation only for DJ-software tracks linked to a library track. These tests show that every
//! decision the sync code takes from that copy is the same as with the earlier representation,
//! which kept the full JSON of every record and every observation.
use cuerithm_library::*;
use cuerithm_media::CancellationToken;
use cuerithm_rekordbox::*;
use rusqlite::Connection;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

const TABLES: [&str; 14] = [
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
];
const CONTENT_TABLES: [&str; 4] = [
    "djmdPlaylist",
    "djmdSongPlaylist",
    "djmdMyTag",
    "djmdSongMyTag",
];

fn record(
    table: &str,
    id: &str,
    fingerprint: &str,
    values: &[(&str, TargetValue)],
) -> TargetRecord {
    TargetRecord {
        table: table.into(),
        id: id.into(),
        fingerprint: fingerprint.into(),
        values: values
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect(),
    }
}
fn text(value: &str) -> TargetValue {
    TargetValue::Text(value.into())
}
fn track(dir: &Path, id: &str, present: bool, title: &str) -> ImportedTrack {
    let path = dir.join(format!("{id}.flac"));
    if present {
        std::fs::write(&path, format!("authored placeholder {id}")).unwrap();
    }
    serde_json::from_value(json!({
        "identity":{"database_id":"authored","content_id":id,"uuid":format!("uuid-{id}"),"master_song_id":format!("master-{id}")},
        "metadata":{"title":title,"artist":"Author","album":"Album","album_artist":"","genre":"House","label":"","key":"Am","comment":"","remixer":"","composer":"","lyricist":"","mix_name":""},
        "source":{"path":path,"resolved_path":path,"service_id":0},"bpm":128.0,"duration_seconds":10.0,"deleted":false,
        "cues":[{"id":format!("cue-{id}"),"uuid":null,"content_uuid":null,"kind":0,"slot":null,"memory":true,"start_seconds":1.5,"end_seconds":null,"active_loop":false,"comment":"","color":null,"color_table_index":null,"beat_loop_size":null,"raw_hash":format!("cue-{id}")}],
        "tag_ids":[],"raw_hash":format!("{id}{title}").repeat(2),"metadata_hash":id.repeat(64)
    })).unwrap()
}

/// A library with two tracks CueRithm can link (`a`, `b`), one whose file is missing (`c`), a
/// folder, a playlist with two members, a MyTag category and a MyTag, and noise in the tables whose
/// content the library does not keep.
fn collection(dir: &Path) -> RekordboxCollection {
    let mut collection: RekordboxCollection = serde_json::from_value(json!({
        "identity":{"database_id":"authored","device_id":"device","database_version":"7"},
        "schema_fingerprint":"a".repeat(64), "tracks":[],
        "playlists":[
            {"id":"folder","uuid":"folder-uuid","parent_id":"root","name":"Sets","attribute":1,"sequence":1,"deleted":false,"raw_hash":"f"},
            {"id":"list","uuid":"list-uuid","parent_id":"folder","name":"Set","attribute":0,"sequence":1,"deleted":false,"raw_hash":"l"}
        ], "playlist_links":[
            {"id":"first","playlist_id":"list","content_id":"b","position":1,"deleted":false,"raw_hash":"first"},
            {"id":"second","playlist_id":"list","content_id":"a","position":2,"deleted":false,"raw_hash":"second"}
        ],
        "tags":[
            {"id":"category","parent_id":"root","name":"Mood","attribute":1,"sequence":1,"deleted":false,"raw_hash":"c"},
            {"id":"tag","parent_id":"category","name":"Warm","attribute":0,"sequence":1,"deleted":false,"raw_hash":"t"}
        ], "tag_links":[
            {"id":"tag-link","tag_id":"tag","content_id":"a","deleted":false,"raw_hash":"tag-link"}
        ],"records":{},"issues":[]
    }))
    .unwrap();
    collection.tracks = vec![
        track(dir, "a", true, "A"),
        track(dir, "b", true, "B"),
        track(dir, "c", false, "C"),
    ];
    let mut records: BTreeMap<String, Vec<TargetRecord>> =
        TABLES.iter().map(|t| ((*t).to_owned(), vec![])).collect();
    for t in &collection.tracks {
        records.get_mut("djmdContent").unwrap().push(record(
            "djmdContent",
            &t.identity.content_id,
            &t.raw_hash,
            &[
                ("ID", text(&t.identity.content_id)),
                ("Opaque", TargetValue::Blob(vec![0, 255, 7])),
            ],
        ));
    }
    for n in 0..40 {
        records.get_mut("djmdCue").unwrap().push(record(
            "djmdCue",
            &format!("cue{n}"),
            &format!("cue-fp-{n}"),
            &[
                ("ContentID", text("a")),
                ("InMsec", TargetValue::Integer(n * 100)),
            ],
        ));
        records.get_mut("contentCue").unwrap().push(record(
            "contentCue",
            &format!("cc{n}"),
            &format!("cc-fp-{n}"),
            &[("Cues", TargetValue::Blob(vec![n as u8; 64]))],
        ));
    }
    let zero = ("rb_local_deleted", TargetValue::Integer(0));
    for (id, raw) in [("folder", "f"), ("list", "l")] {
        records.get_mut("djmdPlaylist").unwrap().push(record(
            "djmdPlaylist",
            id,
            raw,
            &[("ID", text(id)), zero.clone()],
        ));
    }
    for (id, raw) in [("category", "c"), ("tag", "t")] {
        records.get_mut("djmdMyTag").unwrap().push(record(
            "djmdMyTag",
            id,
            raw,
            &[("ID", text(id)), zero.clone()],
        ));
    }
    for id in ["first", "second"] {
        records.get_mut("djmdSongPlaylist").unwrap().push(record(
            "djmdSongPlaylist",
            id,
            id,
            &[("PlaylistID", text("list")), zero.clone()],
        ));
    }
    records.get_mut("djmdSongMyTag").unwrap().push(record(
        "djmdSongMyTag",
        "tag-link",
        "tag-link",
        &[("MyTagID", text("tag")), zero],
    ));
    collection.records = records;
    collection
}

fn observe(lib: &Library, input: &RekordboxCollection) -> NativeImportReport {
    lib.import_rekordbox_collection(
        input,
        NativeImportOptions {
            settings_revision: lib.settings().unwrap().revision,
            content_ids: None,
            initial_owners: BTreeMap::new(),
        },
        &CancellationToken::default(),
    )
    .unwrap()
}
fn import_playlists(lib: &Library, input: &RekordboxCollection, seen: &NativeImportReport) {
    lib.import_native_playlists(
        &seen.target_id,
        input,
        NativePlaylistImportOptions {
            native_revision: seen.revision,
            playlist_revision: lib.playlist_state().unwrap().revision,
            materialize_native_ids: BTreeSet::new(),
        },
    )
    .unwrap();
}

/// What the earlier version stored: the full JSON of every record and of every observation. It
/// is written after each observation of the database that plays the earlier version.
fn store_full_json(db: &Path, input: &RekordboxCollection) {
    let conn = Connection::open(db).unwrap();
    for (table, records) in &input.records {
        for record in records {
            conn.execute(
                "UPDATE native_records SET json=?1 WHERE table_name=?2 AND record_id=?3",
                rusqlite::params![serde_json::to_string(record).unwrap(), table, record.id],
            )
            .unwrap();
        }
    }
    for track in &input.tracks {
        conn.execute(
            "UPDATE native_track_links SET observation_json=?1 WHERE content_id=?2 AND observation_hash IS NOT NULL",
            rusqlite::params![serde_json::to_string(track).unwrap(), track.identity.content_id],
        )
        .unwrap();
    }
}

fn outcome<T: serde::Serialize>(result: anyhow::Result<T>) -> String {
    match result {
        Ok(value) => format!("ok {}", serde_json::to_string(&value).unwrap()),
        Err(error) => format!("err {error:#}"),
    }
}
/// Everything the sync code decides from the stored copy, for one observation.
fn decisions(lib: &Library, target: &str, list: &str, category: &str) -> Vec<String> {
    let revision = || lib.native_target(target).unwrap().revision;
    let plan = |playlist_ids: Vec<String>, category_ids: Vec<String>| {
        outcome(lib.plan_native_playlists(
            target,
            NativePlaylistExportOptions {
                native_revision: revision(),
                playlist_ids,
                category_ids,
                materialize_smart_playlists: BTreeSet::new(),
            },
        ))
    };
    let preview = |content: &str| {
        // The preview id is random; its content is the decision.
        match lib.preview_native_reconciliation(target, content) {
            Ok(preview) => format!("ok {}", serde_json::to_string(&preview.snapshot).unwrap()),
            Err(error) => format!("err {error:#}"),
        }
    };
    let state = lib.native_target(target).unwrap();
    vec![
        plan(vec![list.into()], vec![]),
        plan(vec![], vec![category.into()]),
        preview("a"),
        preview("b"),
        preview("c"),
        format!(
            "target records={} tracks={} revision={} issues={}",
            state.records,
            state.tracks,
            state.revision,
            state.issues.len()
        ),
    ]
}

fn link_without_observation(lib: &Library, target: &str, content: &str) -> Value {
    let mut link = serde_json::to_value(lib.native_track_link(target, content).unwrap()).unwrap();
    link["observation"] = Value::Null;
    link
}

#[test]
fn the_sync_decisions_are_the_same_as_with_the_full_copy() {
    let base = tempfile::tempdir().unwrap();
    let music = base.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let c0 = collection(&music);
    let (target, list);
    {
        let lib = Library::open(&base.path().join("library.sqlite")).unwrap();
        let seen = observe(&lib, &c0);
        import_playlists(&lib, &c0, &seen);
        target = seen.target_id.clone();
        let nodes = lib.playlist_state().unwrap().nodes;
        list = nodes.iter().find(|n| n.name == "Set").unwrap().id.clone();
    }
    let category: String = Connection::open(base.path().join("library.sqlite"))
        .unwrap()
        .query_row(
            "SELECT id FROM custom_tag_categories WHERE name='Mood'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let (slim, full) = (base.path().join("slim"), base.path().join("full"));
    for dir in [&slim, &full] {
        std::fs::create_dir(dir).unwrap();
        std::fs::copy(
            base.path().join("library.sqlite"),
            dir.join("library.sqlite"),
        )
        .unwrap();
    }
    store_full_json(&full.join("library.sqlite"), &c0);
    let lean = Library::open(&slim.join("library.sqlite")).unwrap();
    let old = Library::open(&full.join("library.sqlite")).unwrap();

    // The representation differs exactly where it should.
    let rows = |dir: &Path, sql: &str| -> i64 {
        Connection::open(dir.join("library.sqlite"))
            .unwrap()
            .query_row(sql, [], |r| r.get(0))
            .unwrap()
    };
    assert_eq!(
        rows(&slim, "SELECT COUNT(*) FROM native_records WHERE json!=''"),
        7
    );
    assert!(rows(&full, "SELECT COUNT(*) FROM native_records WHERE json!=''") > 80);

    // A sequence of observations: nothing changed, an unrelated track changed, the bound
    // playlist changed in each way a proof looks at, and an unreadable relationship flag.
    let mut steps: Vec<(&str, RekordboxCollection)> = vec![("unchanged", c0.clone())];
    let mut c1 = c0.clone();
    c1.tracks[0].metadata.title = "A edited in rekordbox".into();
    c1.tracks[0].raw_hash = "edited-a".repeat(8);
    c1.records.get_mut("djmdContent").unwrap()[0].fingerprint = c1.tracks[0].raw_hash.clone();
    steps.push(("unrelated track edited", c1.clone()));
    let mut c2 = c1.clone();
    c2.playlist_links.push(PlaylistLink {
        id: "third".into(),
        playlist_id: "list".into(),
        content_id: "c".into(),
        position: 3,
        deleted: Some(false),
        raw_hash: "third".into(),
    });
    c2.records.get_mut("djmdSongPlaylist").unwrap().push(record(
        "djmdSongPlaylist",
        "third",
        "third",
        &[
            ("PlaylistID", text("list")),
            ("rb_local_deleted", TargetValue::Integer(0)),
        ],
    ));
    steps.push(("member added to the bound playlist", c2.clone()));
    let mut c3 = c1.clone();
    c3.records.get_mut("djmdSongPlaylist").unwrap().push(record(
        "djmdSongPlaylist",
        "gone",
        "gone",
        &[
            ("PlaylistID", text("list")),
            ("rb_local_deleted", TargetValue::Integer(1)),
        ],
    ));
    steps.push(("a deleted member row appears", c3.clone()));
    let mut c4 = c1.clone();
    c4.records.get_mut("djmdSongPlaylist").unwrap().push(record(
        "djmdSongPlaylist",
        "odd",
        "odd",
        &[
            ("PlaylistID", text("list")),
            ("rb_local_deleted", TargetValue::Integer(7)),
        ],
    ));
    steps.push(("a member row with an unreadable deleted flag", c4));
    let mut c5 = c1.clone();
    c5.playlists[1].raw_hash = "renamed".into();
    c5.records.get_mut("djmdPlaylist").unwrap()[1].fingerprint = "renamed".into();
    steps.push(("the bound playlist record changed", c5));
    let mut c6 = c1.clone();
    c6.tag_links[0].raw_hash = "tag-link-2".into();
    c6.records.get_mut("djmdSongMyTag").unwrap()[0].fingerprint = "tag-link-2".into();
    steps.push(("a MyTag membership changed", c6));
    let mut c7 = c1.clone();
    c7.tracks[0].cues.clear();
    c7.tracks[0].raw_hash = "no-cues".repeat(8);
    c7.records.get_mut("djmdContent").unwrap()[0].fingerprint = c7.tracks[0].raw_hash.clone();
    c7.records.get_mut("djmdCue").unwrap().truncate(10);
    steps.push(("cue rows removed", c7));

    let mut kinds = BTreeSet::new();
    for (name, input) in steps {
        let a = observe(&lean, &input);
        let b = observe(&old, &input);
        store_full_json(&full.join("library.sqlite"), &input);
        assert_eq!(
            (
                a.changed,
                a.revision,
                &a.statuses,
                a.tracks_created,
                a.jobs_created
            ),
            (
                b.changed,
                b.revision,
                &b.statuses,
                b.tracks_created,
                b.jobs_created
            ),
            "{name}: the observation report differs"
        );
        let (lean_decisions, old_decisions) = (
            decisions(&lean, &target, &list, &category),
            decisions(&old, &target, &list, &category),
        );
        assert_eq!(lean_decisions, old_decisions, "{name}");
        kinds.extend(
            lean_decisions
                .iter()
                .map(|d| d.split(' ').next().unwrap().to_owned()),
        );
        for content in ["a", "b", "c"] {
            assert_eq!(
                link_without_observation(&lean, &target, content),
                link_without_observation(&old, &target, content),
                "{name}: link {content}"
            );
        }
    }
    // The comparison covered both outcomes, so it is not agreeing on a single refusal.
    assert!(kinds.contains("ok") && kinds.contains("err"), "{kinds:?}");
    drop((lean, old));

    // The only difference left: a DJ-software track with no library track keeps no observation.
    let observations = |dir: &Path| -> BTreeMap<String, bool> {
        let conn = Connection::open(dir.join("library.sqlite")).unwrap();
        let mut stmt = conn
            .prepare("SELECT content_id,observation_json IS NOT NULL FROM native_track_links")
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    };
    assert_eq!(
        observations(&slim),
        BTreeMap::from([("a".into(), true), ("b".into(), true), ("c".into(), false)])
    );
    assert_eq!(
        observations(&full),
        BTreeMap::from([("a".into(), true), ("b".into(), true), ("c".into(), true)])
    );
}

#[test]
fn identity_and_hash_stay_for_every_record_and_nothing_else_is_kept_for_the_other_tables() {
    let d = tempfile::tempdir().unwrap();
    let music = d.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let input = collection(&music);
    let lib = Library::open(&d.path().join("library.sqlite")).unwrap();
    let first = observe(&lib, &input);
    assert!(first.changed);
    let again = observe(&lib, &input);
    assert!(!again.changed, "an unchanged database is a no-op");
    assert_eq!(again.revision, first.revision);
    drop(lib);
    let conn = Connection::open(d.path().join("library.sqlite")).unwrap();
    let mut stored = BTreeMap::new();
    let mut stmt = conn
        .prepare("SELECT table_name,record_id,json,hash FROM native_records")
        .unwrap();
    for row in stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })
        .unwrap()
    {
        let (table, id, json, hash) = row.unwrap();
        stored.insert((table, id), (json, hash));
    }
    let expected: usize = input.records.values().map(Vec::len).sum();
    assert_eq!(stored.len(), expected, "one row per record");
    for (table, records) in &input.records {
        for record in records {
            let full = serde_json::to_string(record).unwrap();
            let (json, hash) = &stored[&(table.clone(), record.id.clone())];
            assert_eq!(hash, &format!("{:x}", Sha256::digest(full.as_bytes())));
            if CONTENT_TABLES.contains(&table.as_str()) {
                assert_eq!(json, &full);
            } else {
                assert_eq!(json, "");
            }
        }
    }
}

#[test]
fn a_changed_record_is_noticed_from_its_hash_alone() {
    let d = tempfile::tempdir().unwrap();
    let music = d.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let mut input = collection(&music);
    let lib = Library::open(&d.path().join("library.sqlite")).unwrap();
    let first = observe(&lib, &input);
    // Only a cue row of a table whose content is not kept changes.
    input.records.get_mut("djmdCue").unwrap()[3]
        .values
        .insert("InMsec".into(), TargetValue::Integer(999_999));
    let second = observe(&lib, &input);
    assert!(second.changed);
    assert_eq!(second.revision, first.revision + 1);
    drop(lib);
    let conn = Connection::open(d.path().join("library.sqlite")).unwrap();
    let hash: String = conn
        .query_row(
            "SELECT hash FROM native_records WHERE table_name='djmdCue' AND record_id='cue3'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        hash,
        format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_string(&input.records["djmdCue"][3])
                    .unwrap()
                    .as_bytes()
            )
        )
    );
}

#[test]
fn a_linked_track_gets_its_observation_back_when_it_was_lost() {
    // Undo of a removal links the row again; the next observation must restore what it needs.
    let d = tempfile::tempdir().unwrap();
    let music = d.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let input = collection(&music);
    let db = d.path().join("library.sqlite");
    let lib = Library::open(&db).unwrap();
    let seen = observe(&lib, &input);
    assert!(
        lib.native_track_link(&seen.target_id, "a")
            .unwrap()
            .observation
            .is_some()
    );
    Connection::open(&db)
        .unwrap()
        .execute(
            "UPDATE native_track_links SET observation_json=NULL WHERE content_id='a'",
            [],
        )
        .unwrap();
    assert!(
        lib.native_track_link(&seen.target_id, "a")
            .unwrap()
            .observation
            .is_none()
    );
    observe(&lib, &input);
    let link = lib.native_track_link(&seen.target_id, "a").unwrap();
    assert_eq!(link.observation.unwrap().metadata.title, "A");
    // An unlinked track does not get one.
    assert!(
        lib.native_track_link(&seen.target_id, "c")
            .unwrap()
            .observation
            .is_none()
    );
}

#[test]
fn schema_29_with_a_large_full_copy_upgrades_to_the_lean_form_and_keeps_every_decision_input() {
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("library.sqlite");
    let music = d.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let input = collection(&music);
    let (target, expected_link);
    {
        let lib = Library::open(&db).unwrap();
        let seen = observe(&lib, &input);
        import_playlists(&lib, &input, &seen);
        target = seen.target_id.clone();
        expected_link = link_without_observation(&lib, &target, "a");
    }
    // The earlier version's representation, plus a large baseline: a rekordbox library of
    // 30,000 records and 3,000 tracks that no library track is linked to.
    store_full_json(&db, &input);
    let big = "x".repeat(2800);
    let conn = Connection::open(&db).unwrap();
    conn.execute_batch("BEGIN").unwrap();
    conn.execute(
        "INSERT INTO native_targets(id,provider,database_id,device_id,identity_json,schema_fingerprint,observation_hash,issues_json,revision,created,updated) VALUES('bulk','rekordbox','bulk-db','','{}',?1,?1,'[]',1,0,0)",
        [&"b".repeat(64)],
    )
    .unwrap();
    {
        let mut records = conn
            .prepare("INSERT INTO native_records(target_id,table_name,record_id,json,hash) VALUES(?1,?2,?3,?4,?5)")
            .unwrap();
        for n in 0..30_000 {
            let table = ["djmdCue", "contentCue", "djmdAlbum"][n % 3];
            let json = format!("{{\"n\":{n},\"pad\":\"{big}\"}}");
            records
                .execute(rusqlite::params![
                    "bulk",
                    table,
                    format!("bulk{n}"),
                    json,
                    format!("{:x}", Sha256::digest(json.as_bytes()))
                ])
                .unwrap();
        }
        let mut links = conn
            .prepare("INSERT INTO native_track_links(target_id,content_id,local_track_id,identity_json,observation_json,observation_hash,baseline_json,status,updated) VALUES('bulk',?1,NULL,?2,?3,?4,'{}','not_in_local_library',0)")
            .unwrap();
        for n in 0..3_000 {
            let json = format!("{{\"pad\":\"{}\"}}", "y".repeat(5000));
            let identity =
                json!({"database_id":"bulk-db","content_id":format!("bulk{n}")}).to_string();
            links
                .execute(rusqlite::params![
                    format!("bulk{n}"),
                    identity,
                    json,
                    format!("hash{n}")
                ])
                .unwrap();
        }
    }
    conn.execute_batch("COMMIT").unwrap();
    let before = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap() };
    let kept_before = (
        before("SELECT COUNT(*) FROM native_records"),
        before("SELECT COUNT(*) FROM native_track_links"),
        before(
            "SELECT SUM(LENGTH(json)) FROM native_records WHERE table_name IN ('djmdPlaylist','djmdSongPlaylist','djmdMyTag','djmdSongMyTag')",
        ),
        before("SELECT SUM(LENGTH(json)) FROM native_records"),
    );
    assert!(kept_before.3 > 80_000_000, "the probe should be large");
    let hashes: Vec<String> = conn
        .prepare("SELECT hash FROM native_records ORDER BY table_name,record_id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    // The library is as the import release (schema 30) left it, with full copies.
    conn.pragma_update(None, "user_version", 30).unwrap();
    drop(conn);
    let started = std::time::Instant::now();
    let lib = Library::open(&db).unwrap();
    let elapsed = started.elapsed();
    eprintln!(
        "schema 30 to 31 with {} bytes of copies: {elapsed:?}",
        kept_before.3
    );
    assert!(elapsed < std::time::Duration::from_secs(60));
    drop(lib);
    let conn = Connection::open(&db).unwrap();
    let count = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap() };
    assert_eq!(count("PRAGMA user_version"), 31);
    assert_eq!(count("SELECT COUNT(*) FROM native_records"), kept_before.0);
    assert_eq!(
        count("SELECT COUNT(*) FROM native_track_links"),
        kept_before.1
    );
    assert_eq!(
        count(
            "SELECT SUM(LENGTH(json)) FROM native_records WHERE table_name IN ('djmdPlaylist','djmdSongPlaylist','djmdMyTag','djmdSongMyTag')"
        ),
        kept_before.2,
        "the records native playlist sync reads keep their content"
    );
    assert_eq!(
        count("SELECT SUM(LENGTH(json)) FROM native_records"),
        kept_before.2
    );
    assert_eq!(
        conn.prepare("SELECT hash FROM native_records ORDER BY table_name,record_id")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap(),
        hashes,
        "every record keeps its hash"
    );
    // Linked tracks keep their observation, unlinked ones only its hash; baselines are untouched.
    assert_eq!(
        count("SELECT COUNT(*) FROM native_track_links WHERE observation_json IS NOT NULL"),
        2
    );
    assert_eq!(
        count("SELECT COUNT(*) FROM native_track_links WHERE observation_hash IS NULL"),
        0
    );
    let lib = Library::open(&db).unwrap();
    assert_eq!(link_without_observation(&lib, &target, "a"), expected_link);
    assert!(
        lib.native_track_link(&target, "a")
            .unwrap()
            .observation
            .is_some()
    );
    // The upgraded library still decides as before, and a repeated observation changes nothing.
    let again = observe(&lib, &input);
    assert!(!again.changed);
}
