use cuerithm_library::{Library, PlaylistEdit, PlaylistKind};
use cuerithm_smartlists::{Operator, PlaylistReference, RuleValue, SmartDefinition, SmartRule};
fn edit(lib: &Library, value: PlaylistEdit) -> cuerithm_library::PlaylistChange {
    lib.apply_playlist_edit(value, lib.playlist_state().unwrap().revision)
        .unwrap()
}
fn create(
    lib: &Library,
    id: &str,
    parent: Option<&str>,
    kind: PlaylistKind,
    name: &str,
    definition: Option<SmartDefinition>,
) {
    edit(
        lib,
        PlaylistEdit::Create {
            id: Some(id.into()),
            parent_id: parent.map(str::to_owned),
            kind,
            name: name.into(),
            definition,
        },
    );
}
fn smart(reference: &str, kind: PlaylistKind) -> SmartDefinition {
    SmartDefinition {
        match_all: true,
        rules: vec![SmartRule {
            id: "ref".into(),
            field: "playlist".into(),
            operator: match kind {
                PlaylistKind::Folder => Operator::PlaylistInSpecificPlaylistFolder,
                PlaylistKind::Playlist => Operator::PlaylistInSpecificPlaylist,
                PlaylistKind::Smartlist => Operator::PlaylistInSpecificSmartlist,
            },
            value: RuleValue::Playlist(PlaylistReference {
                id: reference.into(),
                kind,
                path: vec!["Same label".into()],
            }),
            or: false,
        }],
    }
}
fn tracks(lib: &Library, dir: &std::path::Path) -> Vec<String> {
    ["a", "b", "c", "d"]
        .map(|name| {
            let p = dir.join(format!("{name}.flac"));
            std::fs::write(&p, name).unwrap();
            let job = lib.enqueue(&[p], lib.settings().unwrap().revision).unwrap()[0].clone();
            lib.jobs()
                .unwrap()
                .iter()
                .find(|j| j.id == job)
                .unwrap()
                .track_id
                .clone()
        })
        .to_vec()
}
#[test]
fn durable_tree_membership_and_partial_order_preserve_audio_and_archive_intent() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("db");
    let lib = Library::open(&db).unwrap();
    let ids = tracks(&lib, dir.path());
    let jobs = lib.jobs().unwrap().len();
    create(&lib, "folder", None, PlaylistKind::Folder, "Folder", None);
    create(
        &lib,
        "list",
        Some("folder"),
        PlaylistKind::Playlist,
        "House",
        None,
    );
    edit(
        &lib,
        PlaylistEdit::AddMembers {
            id: "list".into(),
            track_ids: vec![
                ids[0].clone(),
                ids[1].clone(),
                ids[2].clone(),
                ids[3].clone(),
                ids[0].clone(),
            ],
        },
    );
    assert_eq!(lib.manual_playlist_members("list").unwrap(), ids);
    edit(
        &lib,
        PlaylistEdit::ReorderMembers {
            id: "list".into(),
            track_ids: vec![ids[3].clone(), ids[1].clone()],
        },
    );
    let order = vec![
        ids[0].clone(),
        ids[3].clone(),
        ids[2].clone(),
        ids[1].clone(),
    ];
    assert_eq!(lib.manual_playlist_members("list").unwrap(), order);
    lib.set_archived(&ids[3], true).unwrap();
    assert_eq!(lib.manual_playlist_members("list").unwrap(), order);
    assert_eq!(std::fs::read(dir.path().join("a.flac")).unwrap(), b"a");
    let conn_jobs = rusqlite::Connection::open(&db).unwrap();
    assert_eq!(
        conn_jobs
            .query_row("select count(*) from jobs", [], |r| r.get::<_, usize>(0))
            .unwrap(),
        jobs
    );
    drop(conn_jobs);
    let conn = rusqlite::Connection::open(&db).unwrap();
    assert_eq!(
        conn.query_row("select count(*) from cue_revisions", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    drop(conn);
    drop(lib);
    let lib = Library::open(&db).unwrap();
    assert_eq!(lib.manual_playlist_members("list").unwrap(), order);
    assert!(
        lib.playlist_state()
            .unwrap()
            .nodes
            .iter()
            .any(|n| n.id == "list" && n.parent_id.as_deref() == Some("folder"))
    );
}
#[test]
fn stale_edits_cycles_unknown_members_and_batch_failures_leave_state_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let lib = Library::open(&dir.path().join("db")).unwrap();
    create(&lib, "folder", None, PlaylistKind::Folder, "Root", None);
    create(
        &lib,
        "child",
        Some("folder"),
        PlaylistKind::Folder,
        "Child",
        None,
    );
    create(&lib, "list", None, PlaylistKind::Playlist, "List", None);
    let before = lib.playlist_state().unwrap();
    let revision = before.revision;
    for value in [
        PlaylistEdit::Move {
            id: "folder".into(),
            parent_id: Some("child".into()),
            position: 0,
        },
        PlaylistEdit::AddMembers {
            id: "list".into(),
            track_ids: vec!["unknown".into()],
        },
        PlaylistEdit::Batch {
            edits: vec![
                PlaylistEdit::Rename {
                    id: "list".into(),
                    name: "Temporary".into(),
                },
                PlaylistEdit::Move {
                    id: "list".into(),
                    parent_id: Some("list".into()),
                    position: 0,
                },
            ],
        },
    ] {
        assert!(lib.apply_playlist_edit(value, revision).is_err());
        assert_eq!(lib.playlist_state().unwrap(), before);
    }
    assert!(
        lib.apply_playlist_edit(
            PlaylistEdit::Rename {
                id: "list".into(),
                name: "Wrong".into()
            },
            revision - 1
        )
        .is_err()
    );
}
#[test]
fn smart_dependencies_include_folder_descendants_and_keep_stable_reference_ids_on_copy() {
    let dir = tempfile::tempdir().unwrap();
    let lib = Library::open(&dir.path().join("db")).unwrap();
    create(
        &lib,
        "folder",
        None,
        PlaylistKind::Folder,
        "Same label",
        None,
    );
    create(
        &lib,
        "other",
        None,
        PlaylistKind::Folder,
        "Same label",
        None,
    );
    create(
        &lib,
        "source",
        Some("other"),
        PlaylistKind::Smartlist,
        "Smart",
        Some(smart("folder", PlaylistKind::Folder)),
    );
    assert!(
        lib.apply_playlist_edit(
            PlaylistEdit::Move {
                id: "source".into(),
                parent_id: Some("folder".into()),
                position: 0
            },
            lib.playlist_state().unwrap().revision
        )
        .is_err()
    );
    let copy = edit(
        &lib,
        PlaylistEdit::Duplicate {
            id: "other".into(),
            parent_id: None,
            name: Some("Copied".into()),
        },
    );
    assert_eq!(copy.created_ids.len(), 2);
    let state = lib.playlist_state().unwrap();
    let copied = state
        .nodes
        .iter()
        .find(|n| copy.created_ids.contains(&n.id) && n.kind == PlaylistKind::Smartlist)
        .unwrap();
    assert_eq!(
        copied.definition,
        Some(smart("folder", PlaylistKind::Folder))
    );
    let change = edit(
        &lib,
        PlaylistEdit::Remove {
            id: "folder".into(),
        },
    );
    assert!(change.edit_id.is_some());
    assert_eq!(
        lib.playlist_state()
            .unwrap()
            .nodes
            .iter()
            .find(|n| n.id == "source")
            .unwrap()
            .definition,
        Some(smart("folder", PlaylistKind::Folder))
    );
    assert!(
        lib.apply_playlist_edit(
            PlaylistEdit::AddMembers {
                id: "source".into(),
                track_ids: vec![]
            },
            lib.playlist_state().unwrap().revision
        )
        .is_err()
    );
}
#[test]
fn restore_and_undo_keep_independent_deleted_children_and_refuse_newer_edits() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("db");
    let lib = Library::open(&db).unwrap();
    create(&lib, "f", None, PlaylistKind::Folder, "Folder", None);
    create(&lib, "old", Some("f"), PlaylistKind::Playlist, "Old", None);
    create(
        &lib,
        "active",
        Some("f"),
        PlaylistKind::Playlist,
        "Active",
        None,
    );
    edit(&lib, PlaylistEdit::Remove { id: "old".into() });
    let remove = edit(&lib, PlaylistEdit::Remove { id: "f".into() });
    let id = remove.edit_id.unwrap();
    drop(lib);
    let lib = Library::open(&db).unwrap();
    lib.undo_playlist_edit(&id, lib.playlist_state().unwrap().revision)
        .unwrap();
    let state = lib.playlist_state().unwrap();
    assert!(state.nodes.iter().find(|n| n.id == "old").unwrap().deleted);
    assert!(
        !state
            .nodes
            .iter()
            .find(|n| n.id == "active")
            .unwrap()
            .deleted
    );
    let change = edit(
        &lib,
        PlaylistEdit::Rename {
            id: "active".into(),
            name: "First".into(),
        },
    );
    edit(
        &lib,
        PlaylistEdit::Rename {
            id: "active".into(),
            name: "Newer".into(),
        },
    );
    assert!(
        lib.undo_playlist_edit(
            &change.edit_id.unwrap(),
            lib.playlist_state().unwrap().revision
        )
        .is_err()
    );
    let remove = edit(&lib, PlaylistEdit::Remove { id: "f".into() });
    assert!(remove.edit_id.is_some());
    edit(
        &lib,
        PlaylistEdit::Restore {
            id: "f".into(),
            parent_id: None,
            position: 0,
        },
    );
    let state = lib.playlist_state().unwrap();
    assert!(state.nodes.iter().find(|n| n.id == "old").unwrap().deleted);
    assert!(
        !state
            .nodes
            .iter()
            .find(|n| n.id == "active")
            .unwrap()
            .deleted
    );
}
#[test]
fn duplicate_replacement_preserves_the_first_occurrence_and_does_not_archive_tracks() {
    let dir = tempfile::tempdir().unwrap();
    let lib = Library::open(&dir.path().join("db")).unwrap();
    let ids = tracks(&lib, dir.path());
    create(&lib, "list", None, PlaylistKind::Playlist, "List", None);
    edit(
        &lib,
        PlaylistEdit::AddMembers {
            id: "list".into(),
            track_ids: ids.clone(),
        },
    );
    lib.replace_playlist_track_refs(
        &[ids[0].clone(), ids[2].clone()],
        &ids[3],
        lib.playlist_state().unwrap().revision,
    )
    .unwrap();
    assert_eq!(
        lib.manual_playlist_members("list").unwrap(),
        vec![ids[3].clone(), ids[1].clone()]
    );
    assert_eq!(lib.counts().unwrap().archive, 0);
}
#[test]
fn smart_input_revision_ignores_job_progress_and_materialized_outputs() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("db");
    let lib = Library::open(&db).unwrap();
    let initial = lib.smart_input_revision().unwrap();
    let ids = tracks(&lib, dir.path());
    let data = lib.smart_input_revision().unwrap();
    assert!(data > initial);
    let job = lib.claim_next().unwrap().unwrap();
    lib.set_stage(&job.id, "decoding").unwrap();
    assert_eq!(lib.smart_input_revision().unwrap(), data);
    create(
        &lib,
        "smart",
        None,
        PlaylistKind::Smartlist,
        "Smart",
        Some(SmartDefinition {
            match_all: true,
            rules: vec![],
        }),
    );
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute(
        "insert into smart_playlist_members(playlist_id,track_id,position) values('smart',?1,0)",
        [&ids[0]],
    )
    .unwrap();
    assert_eq!(lib.smart_input_revision().unwrap(), data);
    lib.set_archived(&ids[0], true).unwrap();
    assert!(lib.smart_input_revision().unwrap() > data);
}

#[test]
fn undo_creation_refuses_newer_references_instead_of_dangling_them() {
    let d = tempfile::tempdir().unwrap();
    let lib = Library::open(&d.path().join("db")).unwrap();
    let made = edit(
        &lib,
        PlaylistEdit::Create {
            id: Some("target".into()),
            parent_id: None,
            kind: PlaylistKind::Playlist,
            name: "Target".into(),
            definition: None,
        },
    );
    create(
        &lib,
        "dependent",
        None,
        PlaylistKind::Smartlist,
        "Depends",
        Some(smart("target", PlaylistKind::Playlist)),
    );
    let state = lib.playlist_state().unwrap();
    assert!(
        lib.undo_playlist_edit(&made.edit_id.unwrap(), state.revision)
            .is_err()
    );
    assert_eq!(lib.playlist_state().unwrap(), state);
}
#[test]
fn sql_failure_during_a_batch_rolls_back_every_node_and_history_row() {
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("db");
    let lib = Library::open(&db).unwrap();
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute_batch("CREATE TRIGGER injected_playlist_failure BEFORE INSERT ON playlists WHEN NEW.name='reject' BEGIN SELECT RAISE(FAIL,'injected'); END;").unwrap();
    let before = lib.playlist_state().unwrap();
    let edits = [("a-pass", "accept"), ("b-fail", "reject")]
        .map(|(id, name)| PlaylistEdit::Create {
            id: Some(id.into()),
            parent_id: None,
            kind: PlaylistKind::Playlist,
            name: name.into(),
            definition: None,
        })
        .to_vec();
    assert!(
        lib.apply_playlist_edit(PlaylistEdit::Batch { edits }, before.revision)
            .is_err()
    );
    assert_eq!(lib.playlist_state().unwrap(), before);
    assert_eq!(
        conn.query_row("select count(*) from playlist_edits", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}
#[test]
fn all_schema_inputs_zero_through_five_migrate_without_touching_stored_analysis() {
    for version in 0..=5 {
        let d = tempfile::tempdir().unwrap();
        let db = d.path().join("legacy");
        let conn = rusqlite::Connection::open(&db).unwrap();
        let migrations = [
            include_str!("../migrations/001_initial.sql"),
            include_str!("../migrations/002_tag_writes.sql"),
            include_str!("../migrations/003_preparation_cache.sql"),
            include_str!("../migrations/004_cleanup.sql"),
            include_str!("../migrations/005_cleanup_preparation.sql"),
        ];
        for migration in migrations.iter().take(version) {
            conn.execute_batch(migration).unwrap();
        }
        if version > 0 {
            let snapshot=serde_json::json!({"revision":"legacy","settings":cuerithm_library::PreparationSettings::default(),"models":{}}).to_string();
            conn.execute("insert into settings values('legacy',?1,1)", [snapshot])
                .unwrap();
            conn.execute("insert into selected_settings values(1,'legacy')", [])
                .unwrap();
            conn.execute("insert into tracks(id,path,title,source_hash,state,current_job,analysis_json,created,updated) values('track',?1,'Legacy','hash','ready','job','{\"retained\":true}',1,1)",[b"unix:/offline.flac".to_vec()]).unwrap();
            conn.execute("insert into jobs(id,track_id,source_hash,settings_revision,state,stage,created,updated) values('job','track','hash','legacy','completed','completed',1,1)",[]).unwrap();
            conn.execute(
                "insert into cue_revisions values('cue','track','job','[]',1)",
                [],
            )
            .unwrap();
        }
        drop(conn);
        let lib = Library::open(&db).unwrap();
        create(&lib, "list", None, PlaylistKind::Playlist, "Preserve", None);
        let conn = rusqlite::Connection::open(&db).unwrap();
        assert_eq!(
            conn.query_row("pragma user_version", [], |r| r.get::<_, u32>(0))
                .unwrap(),
            31
        );
        if version > 0 {
            assert_eq!(
                conn.query_row(
                    "select analysis_json from tracks where id='track'",
                    [],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
                "{\"retained\":true}"
            );
            assert_eq!(
                conn.query_row("select json from cue_revisions where id='cue'", [], |r| {
                    r.get::<_, String>(0)
                })
                .unwrap(),
                "[]"
            );
        }
    }
}

#[test]
fn typed_path_fallback_is_unambiguous_and_stable_ids_survive_renames() {
    let d = tempfile::tempdir().unwrap();
    let lib = Library::open(&d.path().join("db")).unwrap();
    create(&lib, "folder", None, PlaylistKind::Folder, "Same", None);
    create(&lib, "list", None, PlaylistKind::Playlist, "Same", None);
    let mut reference = PlaylistReference {
        id: String::new(),
        path: vec!["Same".into()],
        kind: PlaylistKind::Folder,
    };
    assert_eq!(
        lib.resolve_playlist_reference(&reference).unwrap(),
        "folder"
    );
    reference.id = "folder".into();
    edit(
        &lib,
        PlaylistEdit::Rename {
            id: "folder".into(),
            name: "Renamed".into(),
        },
    );
    assert_eq!(
        lib.resolve_playlist_reference(&reference).unwrap(),
        "folder"
    );
    edit(
        &lib,
        PlaylistEdit::Remove {
            id: "folder".into(),
        },
    );
    create(
        &lib,
        "replacement",
        None,
        PlaylistKind::Folder,
        "Same",
        None,
    );
    assert!(lib.resolve_playlist_reference(&reference).is_err());
    reference.id.clear();
    assert_eq!(
        lib.resolve_playlist_reference(&reference).unwrap(),
        "replacement"
    );
    create(&lib, "ambiguous", None, PlaylistKind::Folder, "Same", None);
    assert!(lib.resolve_playlist_reference(&reference).is_err());
}
#[test]
fn restoring_a_favorite_restores_its_slot_without_id_based_ties() {
    let d = tempfile::tempdir().unwrap();
    let lib = Library::open(&d.path().join("db")).unwrap();
    for (position, id) in ["first", "z-restored", "a-last"].iter().enumerate() {
        create(&lib, id, None, PlaylistKind::Playlist, id, None);
        edit(
            &lib,
            PlaylistEdit::Favorite {
                id: (*id).into(),
                position: Some(position as u32),
            },
        );
    }
    edit(
        &lib,
        PlaylistEdit::Remove {
            id: "z-restored".into(),
        },
    );
    edit(
        &lib,
        PlaylistEdit::Restore {
            id: "z-restored".into(),
            parent_id: None,
            position: 1,
        },
    );
    let mut favorite: Vec<_> = lib
        .playlist_state()
        .unwrap()
        .nodes
        .into_iter()
        .filter(|n| !n.deleted && n.favorite_position.is_some())
        .collect();
    favorite.sort_by_key(|n| n.favorite_position);
    assert_eq!(
        favorite.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(),
        ["first", "z-restored", "a-last"]
    );
}

#[test]
fn reference_operator_and_typed_kind_must_agree_before_storage() {
    let d = tempfile::tempdir().unwrap();
    let lib = Library::open(&d.path().join("db")).unwrap();
    create(&lib, "target", None, PlaylistKind::Playlist, "Target", None);
    let mut definition = smart("target", PlaylistKind::Playlist);
    definition.rules[0].operator = Operator::PlaylistInSpecificSmartlist;
    assert!(definition.validate_shape().is_err());
    let before = lib.playlist_state().unwrap();
    assert!(
        lib.apply_playlist_edit(
            PlaylistEdit::Create {
                id: Some("invalid".into()),
                parent_id: None,
                kind: PlaylistKind::Smartlist,
                name: "Invalid".into(),
                definition: Some(definition)
            },
            before.revision
        )
        .is_err()
    );
    assert_eq!(lib.playlist_state().unwrap(), before);
}
#[test]
fn data_counter_covers_native_tags_categories_cues_and_protected_genres_without_preference_noise() {
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("db");
    let lib = Library::open(&db).unwrap();
    let initial = lib.smart_input_revision().unwrap();
    let mut settings = lib.settings().unwrap().settings;
    settings.watch_folders = vec![d.path().join("Watch")];
    lib.save_settings(settings.clone()).unwrap();
    assert_eq!(lib.smart_input_revision().unwrap(), initial);
    let mut cleanup = cuerithm_cleanup::CleanupConfig::default();
    cleanup.genres.protected_values = ["House".into()].into();
    settings.cleanup = Some(cleanup);
    lib.save_settings(settings).unwrap();
    assert!(lib.smart_input_revision().unwrap() > initial);
    let ids = tracks(&lib, d.path());
    let conn = rusqlite::Connection::open(&db).unwrap();
    let mut prior = lib.smart_input_revision().unwrap();
    conn.execute("update tracks set title=title", []).unwrap();
    assert_eq!(lib.smart_input_revision().unwrap(), prior);
    for sql in [
        "insert into custom_tag_categories values('category','Mood')",
        "insert into custom_tags values('tag','category','Dark')",
        "update custom_tags set name='Bright' where id='tag'",
    ] {
        conn.execute(sql, []).unwrap();
        let next = lib.smart_input_revision().unwrap();
        assert!(next > prior);
        prior = next;
    }
    conn.execute("insert into track_custom_tags values(?1,'tag')", [&ids[0]])
        .unwrap();
    assert!(lib.smart_input_revision().unwrap() > prior);
    prior = lib.smart_input_revision().unwrap();
    let job = lib
        .jobs()
        .unwrap()
        .iter()
        .find(|j| j.track_id == ids[0])
        .unwrap()
        .id
        .clone();
    conn.execute(
        "insert into cue_revisions values('new-cue',?1,?2,'[]',1)",
        rusqlite::params![ids[0], job],
    )
    .unwrap();
    assert!(lib.smart_input_revision().unwrap() > prior);
    prior = lib.smart_input_revision().unwrap();
    conn.execute(
        "update tracks set native_metadata_json='{}' where id=?1",
        [&ids[0]],
    )
    .unwrap();
    assert!(lib.smart_input_revision().unwrap() > prior);
}

#[test]
fn undo_detects_newer_edits_even_when_they_return_to_the_same_value() {
    let d = tempfile::tempdir().unwrap();
    let lib = Library::open(&d.path().join("db")).unwrap();
    create(&lib, "list", None, PlaylistKind::Playlist, "Original", None);
    let old = edit(
        &lib,
        PlaylistEdit::Rename {
            id: "list".into(),
            name: "First".into(),
        },
    );
    edit(
        &lib,
        PlaylistEdit::Rename {
            id: "list".into(),
            name: "Newer".into(),
        },
    );
    edit(
        &lib,
        PlaylistEdit::Rename {
            id: "list".into(),
            name: "First".into(),
        },
    );
    let before = lib.playlist_state().unwrap();
    assert!(
        lib.undo_playlist_edit(&old.edit_id.unwrap(), before.revision)
            .is_err()
    );
    assert_eq!(lib.playlist_state().unwrap(), before);
}

#[test]
fn unicode_playlist_names_fit_the_same_typed_reference_path_limits() {
    let d = tempfile::tempdir().unwrap();
    let lib = Library::open(&d.path().join("db")).unwrap();
    let name = "🎵".repeat(200);
    create(&lib, "unicode", None, PlaylistKind::Playlist, &name, None);
    let mut definition = smart("unicode", PlaylistKind::Playlist);
    let RuleValue::Playlist(reference) = &mut definition.rules[0].value else {
        unreachable!()
    };
    reference.path = vec![name];
    assert!(definition.validate_shape().is_ok());
    create(
        &lib,
        "smart-unicode",
        None,
        PlaylistKind::Smartlist,
        "Unicode reference",
        Some(definition),
    );
}
