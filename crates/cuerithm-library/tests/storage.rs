use cuerithm_library::{Library, PreparationSettings, TrackQuery, TrackView};

#[cfg(unix)]
#[test]
fn library_resources_remain_bound_when_its_requested_directory_alias_changes() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let original = root.join("original");
    let replacement = root.join("replacement");
    std::fs::create_dir(&original).unwrap();
    std::fs::create_dir(&replacement).unwrap();
    let alias = root.join("library-alias");
    std::os::unix::fs::symlink(&original, &alias).unwrap();
    let library = Library::open(&alias.join("library.sqlite")).unwrap();
    std::fs::remove_file(&alias).unwrap();
    std::os::unix::fs::symlink(&replacement, &alias).unwrap();
    std::fs::write(library.data_dir().join("private-resource"), b"owned").unwrap();
    assert!(original.join("private-resource").is_file());
    assert!(!replacement.join("private-resource").exists());
    assert_eq!(library.data_dir(), original);
}

#[test]
fn engine_setup_migration_requeues_unfinished_work_without_reading_sources() {
    let d = tempfile::tempdir().unwrap();
    let lib = Library::open(&d.path().join("db")).unwrap();
    let path = d.path().join("offline.flac");
    std::fs::write(&path, b"source").unwrap();
    let id = lib
        .enqueue(
            std::slice::from_ref(&path),
            lib.settings().unwrap().revision,
        )
        .unwrap()[0]
        .clone();
    lib.claim_next().unwrap().unwrap();
    lib.fail(&id, "setup", "old missing model").unwrap();
    std::fs::remove_file(&path).unwrap();
    let revision = lib
        .save_settings(PreparationSettings {
            stability_millis: 50,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(lib.requeue_unfinished(&revision).unwrap(), 1);
    let job = lib.claim_next().unwrap().unwrap();
    assert_eq!(job.settings_revision, revision);
    assert_ne!(job.id, id);
}

#[test]
fn persists_settings_deduplicates_sources_and_recovers_running_jobs() {
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("library.sqlite");
    let path = d.path().join("演奏.flac");
    std::fs::write(&path, b"input").unwrap();
    let id;
    let rev;
    {
        let lib = Library::open(&db).unwrap();
        let settings = PreparationSettings {
            watch_folders: vec![d.path().to_owned()],
            ..Default::default()
        };
        rev = lib.save_settings(settings).unwrap();
        id = lib
            .enqueue(&[path.clone(), path.clone()], rev.clone())
            .unwrap()[0]
            .clone();
        assert_eq!(
            lib.enqueue(std::slice::from_ref(&path), rev.clone())
                .unwrap()[0],
            id
        );
        assert_eq!(lib.jobs().unwrap().len(), 1);
        let job = lib.claim_next().unwrap().unwrap();
        assert_eq!(job.id, id);
        assert_eq!(lib.settings().unwrap().settings.watch_folders, [d.path()]);
        assert!(
            Library::open(&db).is_err(),
            "a second process/owner must not reset running work"
        );
    }
    let lib = Library::open(&db).unwrap();
    let job = lib.claim_next().unwrap().unwrap();
    assert_eq!(job.id, id);
    assert_eq!(job.settings_revision, rev);
    let page = lib
        .tracks(TrackQuery {
            view: TrackView::Incoming,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(page.total, 1);
    assert!(page.rows[0].display_path.contains("演奏"));
}

#[test]
fn rejects_future_schema_without_replacing_it_and_keeps_query_order_stable() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("future.sqlite");
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.pragma_update(None, "user_version", 99).unwrap();
    drop(conn);
    assert!(Library::open(&path).is_err());
    let conn = rusqlite::Connection::open(path).unwrap();
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        99
    );
    let lib = Library::open(&d.path().join("current.sqlite")).unwrap();
    let rev = lib.settings().unwrap().revision;
    for name in ["Zulu.flac", "Alpha.flac", "Bravo.flac"] {
        let p = d.path().join(name);
        std::fs::write(&p, name).unwrap();
        lib.enqueue(&[p], rev.clone()).unwrap();
    }
    let query = TrackQuery {
        view: TrackView::Incoming,
        limit: 1,
        ..Default::default()
    };
    let one = lib.tracks(query.clone()).unwrap();
    assert_eq!(one.rows[0].title, "Alpha");
    let running = lib.claim_next().unwrap().unwrap();
    lib.set_stage(&running.id, "decoding").unwrap();
    lib.fail(&running.id, "decoding", "test failure").unwrap();
    let two = lib
        .tracks(TrackQuery {
            cursor: one.next_cursor,
            ..query.clone()
        })
        .unwrap();
    assert_eq!(two.rows[0].title, "Bravo");
    let filtered = lib
        .tracks(TrackQuery {
            search: "Zulu".into(),
            ..query
        })
        .unwrap();
    assert_eq!(filtered.total, 1);
    let cursor = two.next_cursor.unwrap();
    lib.save_settings(PreparationSettings {
        stability_millis: 40,
        ..Default::default()
    })
    .unwrap();
    // Settings/job-only updates do not invalidate a track-page cursor.
    assert!(
        lib.tracks(TrackQuery {
            view: TrackView::Incoming,
            limit: 1,
            cursor: Some(cursor.clone()),
            ..Default::default()
        })
        .is_ok()
    );
    let added = d.path().join("Delta.flac");
    std::fs::write(&added, b"delta").unwrap();
    lib.enqueue(&[added], lib.settings().unwrap().revision)
        .unwrap();
    assert!(
        lib.tracks(TrackQuery {
            view: TrackView::Incoming,
            limit: 1,
            cursor: Some(cursor),
            ..Default::default()
        })
        .is_err()
    );
    let accented = d.path().join("ÉCLAIR.flac");
    std::fs::write(&accented, b"source").unwrap();
    lib.enqueue(&[accented], lib.settings().unwrap().revision)
        .unwrap();
    assert_eq!(
        lib.tracks(TrackQuery {
            view: TrackView::Incoming,
            search: "éclair".into(),
            ..Default::default()
        })
        .unwrap()
        .total,
        1
    );
    assert_eq!(
        lib.track_ids(TrackQuery {
            view: TrackView::Incoming,
            limit: 1,
            ..Default::default()
        })
        .unwrap()
        .len(),
        5
    );
}

#[cfg(unix)]
#[test]
fn reopening_waits_for_a_briefly_inherited_child_lease() {
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("library.sqlite");
    let library = cuerithm_library::Library::open(&db).unwrap();
    // A concurrent decoder spawn can briefly inherit flock's open description.
    // The child performs only async-signal-safe operations and never uses SQLite.
    // It exits when the parent says so, so the lease is held for a fixed 25 ms
    // however long the scheduler takes to start the child.
    let mut pipe = [0; 2];
    assert_eq!(unsafe { libc::pipe(pipe.as_mut_ptr()) }, 0);
    let child = unsafe { libc::fork() };
    assert!(child >= 0);
    if child == 0 {
        let mut byte = 0u8;
        unsafe {
            libc::read(pipe[0], (&mut byte as *mut u8).cast(), 1);
            libc::_exit(0);
        }
    }
    let release = pipe[1];
    let releaser = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(25));
        unsafe {
            libc::write(release, [0u8].as_ptr().cast(), 1);
        }
    });
    drop(library);
    let reopened = cuerithm_library::Library::open(&db);
    releaser.join().unwrap();
    unsafe {
        libc::waitpid(child, std::ptr::null_mut(), 0);
        libc::close(pipe[0]);
        libc::close(pipe[1]);
    }
    assert!(
        reopened.is_ok(),
        "transient inherited lease must settle: {:?}",
        reopened.err()
    );
}

// A build without native tag writers must not silently resume past accepted
// editor/review work, even before a physical write journal has been created.
#[cfg(not(feature = "autotag"))]
#[test]
fn untagged_build_preserves_native_curation_before_child_allocation() {
    use cuerithm_library::*;
    for native_required in [true, false] {
        let temp = tempfile::tempdir().unwrap();
        let db = temp.path().join("library.sqlite");
        let source = temp.path().join("source.flac");
        std::fs::write(&source, b"curation guard fixture; never decoded").unwrap();
        let library = Library::open(&db).unwrap();
        library
            .enqueue(
                std::slice::from_ref(&source),
                library.settings().unwrap().revision,
            )
            .unwrap();
        let run = library.claim_next().unwrap().unwrap();
        let service = CurationService::new(library.clone(), None);
        service
            .update(CurationUpdateRequest {
                request_id: format!("pending-{native_required}"),
                taxonomy_revision: service.catalog().unwrap().revision,
                tracks: vec![CurationExpectedRevision {
                    track_id: run.track_id.clone(),
                    revision: 0,
                }],
                patch: CurationPatch {
                    notes: CurationValueEdit::Set {
                        value: "Pending note".into(),
                    },
                    ..Default::default()
                },
            })
            .unwrap();
        let sql = rusqlite::Connection::open(&db).unwrap();
        // Simulate a desired head accepted by a tagging-enabled application,
        // before its native child journal can have been allocated.
        sql.execute(
            "UPDATE curation_tracks SET native_required=?1 WHERE track_id=?2",
            rusqlite::params![native_required, run.track_id],
        )
        .unwrap();
        drop(service);
        drop(library);
        let opened = Library::open(&db);
        if native_required {
            let error = opened
                .err()
                .expect("accepted native curation needs the full writer build");
            assert!(error.to_string().contains("tagging-enabled"));
            let state: String = sql
                .query_row("SELECT state FROM jobs WHERE id=?1", [&run.id], |r| {
                    r.get(0)
                })
                .unwrap();
            assert_eq!(state, "running", "refusal must precede ownership recovery");
        } else {
            assert!(
                opened.is_ok(),
                "library-only pending curation remains available"
            );
        }
        assert_eq!(
            std::fs::read(&source).unwrap(),
            b"curation guard fixture; never decoded"
        );
    }
}

#[cfg(not(feature = "autotag"))]
#[test]
fn untagged_build_preserves_pending_native_decisions_before_recovery() {
    for (kind, state, completed) in [
        ("editor", "queued", false),
        ("editor", "claimed", false),
        ("editor", "committed", false),
        ("review", "pending", false),
        ("review", "claimed", false),
        ("review", "applied", false),
        ("editor", "committed", true),
        ("review", "applied", true),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("source.flac");
        std::fs::write(&path, b"guard fixture; never decoded").unwrap();
        let db = temp.path().join("library.sqlite");
        let library = Library::open(&db).unwrap();
        library
            .enqueue(
                std::slice::from_ref(&path),
                library.settings().unwrap().revision,
            )
            .unwrap();
        let run = library.claim_next().unwrap().unwrap();
        library
            .fail_for_run(&run, "review", "retained native decision")
            .unwrap();
        drop(library);
        let sql = rusqlite::Connection::open(&db).unwrap();
        sql.pragma_update(None, "foreign_keys", true).unwrap();
        if kind == "editor" {
            sql.execute(
                "INSERT INTO metadata_edit_drafts VALUES('draft',1,'draft','{}',0,0)",
                [],
            )
            .unwrap();
            sql.execute("INSERT INTO metadata_edit_batches VALUES('batch','draft',1,'save','complete','{}',NULL,0,0)", []).unwrap();
            sql.execute("INSERT INTO metadata_edit_overrides(job_id,track_id,draft_id,draft_revision,batch_id,source_hash,settings_revision,path_revision,metadata_revision,accepted_attempt,execution_source_proof_json,state,payload_json,final_json,created,updated) VALUES(?1,?2,'draft',1,'batch',?3,?4,0,0,?5,'{}',?6,'{}',?7,0,0)",rusqlite::params![run.id,run.track_id,run.source_sha256,run.settings_revision,run.attempt,state,completed.then_some("{}")]).unwrap();
        } else {
            sql.execute("INSERT INTO preparation_review_decisions(job_id,track_id,source_hash,settings_revision,generation,accepted_attempt,payload_json,binding_json,state,created,updated) VALUES(?1,?2,?3,?4,'generation',?5,'{}','{}',?6,0,0)",rusqlite::params![run.id,run.track_id,run.source_sha256,run.settings_revision,run.attempt,state]).unwrap();
        }
        if completed {
            sql.execute("UPDATE jobs SET state='completed' WHERE id=?1", [&run.id])
                .unwrap();
            sql.execute(
                "UPDATE tracks SET state='ready' WHERE id=?1",
                [&run.track_id],
            )
            .unwrap();
        }
        let before: String = sql
            .query_row(
                "SELECT json_array(state,stage,attempts,settings_revision) FROM jobs WHERE id=?1",
                [&run.id],
                |r| r.get(0),
            )
            .unwrap();
        let opened = Library::open(&db);
        if completed {
            assert!(
                opened.is_ok(),
                "completed {kind} history must remain readable"
            );
        } else {
            let error = opened
                .err()
                .expect("pending native decisions require a tagging-enabled build");
            assert!(
                error.to_string().contains("Pending metadata writes"),
                "{kind}/{state}: {error:#}"
            );
            let after: String = sql.query_row("SELECT json_array(state,stage,attempts,settings_revision) FROM jobs WHERE id=?1", [&run.id], |r| r.get(0)).unwrap();
            assert_eq!(
                after, before,
                "refused reopen must retain pending ownership"
            );
        }
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"guard fixture; never decoded"
        );
    }
}

fn remove_import_tables_for_legacy_fixture(conn: &rusqlite::Connection) {
    for table in [
        "import_preview_effects",
        "import_processing_receipts",
        "import_operations",
        "import_previews",
        "import_policies",
        "import_memberships",
        "import_collections",
        "import_applied_baselines",
        "import_active_fields",
        "import_bindings",
        "import_items",
        "import_observations",
        "import_snapshots",
        "import_blobs",
        "import_sources",
    ] {
        conn.execute(&format!("DROP TABLE {table}"), []).unwrap();
    }
}

#[test]
fn schema_eighteen_upgrade_keeps_organization_roots_and_preparation_state() {
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("app/library.sqlite");
    let music = d.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let music = music.canonicalize().unwrap();
    let source = music.join("authored.flac");
    std::fs::write(&source, b"authored migration source").unwrap();
    let library = Library::open(&db).unwrap();
    let root = library.register_organization_root(music).unwrap();
    let settings = library.settings().unwrap().revision;
    let job = library
        .enqueue(std::slice::from_ref(&source), settings.clone())
        .unwrap()[0]
        .clone();
    let track = library.jobs().unwrap()[0].track_id.clone();
    drop(library);
    // Recreate the schema-18 boundary without leaving later feature tables.
    let conn = rusqlite::Connection::open(&db).unwrap();
    remove_import_tables_for_legacy_fixture(&conn);
    conn.execute_batch("DROP INDEX cue_revisions_track_created; DROP TABLE accepted_musical_revisions; DROP TABLE portable_domain_records; DROP TABLE portable_library_meta; ALTER TABLE tracks DROP COLUMN selected_musical_revision; ALTER TABLE tracks DROP COLUMN source_locator_json; ALTER TABLE tracks DROP COLUMN origin; ALTER TABLE tracks DROP COLUMN analysis_status; DROP TRIGGER curation_category_insert; DROP TRIGGER curation_category_update; DROP TRIGGER curation_category_delete; DROP TRIGGER curation_value_insert; DROP TRIGGER curation_value_update; DROP TRIGGER curation_value_delete; DROP TRIGGER curation_smart_update; DROP TABLE curation_metadata_drafts; DROP TABLE curation_native_stale; DROP TABLE curation_publications; DROP TABLE curation_tracks; DROP TABLE curation_mutations; DROP TABLE curation_mappings; DROP TABLE curation_value_aliases; DROP TABLE curation_category_aliases; DROP TABLE curation_value_properties; DROP TABLE curation_group_properties; DROP TABLE preparation_preset_assignments; DROP TABLE preparation_preset_capture_authorities; DROP TABLE preparation_preset_folders; DROP TABLE preparation_preset_revisions; DROP TABLE preparation_presets; DROP TABLE catalog_response_cache; DROP TABLE catalog_request_starts; DROP TABLE catalog_rate_state; DELETE FROM meta WHERE key='curation_taxonomy_revision'; DROP TABLE metadata_edit_files; DROP TABLE metadata_edit_writes; DROP TABLE metadata_edit_rows; DROP TABLE metadata_edit_overrides; DROP TABLE metadata_edit_batches; DROP TABLE metadata_edit_previews; DROP TABLE metadata_edit_drafts; DROP TABLE metadata_artwork_assets; DROP TABLE preparation_match_generations; DROP TABLE preparation_match_previews; DROP TABLE preparation_review_decisions; DROP TABLE preparation_reuse; DROP TABLE preparation_review_artwork; DROP TABLE artifact_cleanup_moves; ALTER TABLE tag_writes DROP COLUMN editor_receipt_json; ALTER TABLE tag_writes DROP COLUMN input_source_proof_json; ALTER TABLE tag_writes DROP COLUMN restore_source_proof_json; ALTER TABLE tag_writes DROP COLUMN artifact_authorities_json; DROP TABLE native_reconciliation_plans; DROP TABLE manual_publication_operations; DROP TABLE manual_publication_state; DROP TABLE cue_templates; DROP TABLE native_playlist_bindings; DROP TABLE native_playlist_acknowledgements; DROP TABLE rekordbox_sync_operations; DROP TABLE native_sync_track_ownership; DROP TABLE sync_target_configurations; PRAGMA user_version=18;").unwrap();
    drop(conn);
    let library = Library::open(&db).unwrap();
    assert_eq!(library.organization_roots().unwrap()[0].id, root.id);
    assert_eq!(library.track_path(&track).unwrap(), source);
    assert_eq!(library.jobs().unwrap()[0].id, job);
    assert_eq!(library.settings().unwrap().revision, settings);
    library
        .save_cue_template(
            None,
            "Practice".into(),
            Default::default(),
            library.revision().unwrap(),
        )
        .unwrap();
    assert_eq!(library.cue_templates().unwrap().len(), 1);
    let conn = rusqlite::Connection::open(db).unwrap();
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 31);
    let count: i64 = conn
        .query_row(
            "SELECT count(*) FROM native_reconciliation_plans",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn schema_twenty_three_upgrade_preserves_released_preset_curation_and_catalog_data() {
    use cuerithm_library::*;
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("library.sqlite");
    let library = Library::open(&db).unwrap();
    let projection = library
        .project_preparation_preset(&library.settings().unwrap().settings)
        .unwrap();
    let preset = library
        .save_preparation_preset(PreparationPresetSave {
            id: None,
            expected_revision: None,
            name: "Released preset".into(),
            rules: projection.rules,
            connections: projection.connections,
        })
        .unwrap();
    let service = CurationService::new(library.clone(), None);
    let receipt = service
        .edit_taxonomy(CurationTaxonomyRequest {
            request_id: "released-curation".into(),
            expected_revision: service.catalog().unwrap().revision,
            change: CurationTaxonomyChange::CreateGroup {
                name: "Released Mood".into(),
                mode: CurationGroupMode::Single,
            },
        })
        .unwrap();
    drop(service);
    drop(library);
    let conn = rusqlite::Connection::open(&db).unwrap();
    remove_import_tables_for_legacy_fixture(&conn);
    conn.execute_batch("DROP INDEX cue_revisions_track_created; DROP TABLE accepted_musical_revisions; DROP TABLE portable_domain_records; DROP TABLE portable_library_meta; ALTER TABLE tracks DROP COLUMN selected_musical_revision; ALTER TABLE tracks DROP COLUMN source_locator_json; ALTER TABLE tracks DROP COLUMN origin; ALTER TABLE tracks DROP COLUMN analysis_status; INSERT INTO catalog_rate_state(service,manual_hold) VALUES('musicbrainz',1); DROP TABLE native_reconciliation_plans; DROP TABLE manual_publication_operations; DROP TABLE manual_publication_state; DROP TABLE cue_templates; DROP TABLE native_playlist_bindings; DROP TABLE native_playlist_acknowledgements; DROP TABLE rekordbox_sync_operations; DROP TABLE native_sync_track_ownership; DROP TABLE sync_target_configurations; PRAGMA user_version=23;").unwrap();
    drop(conn);
    let library = Library::open(&db).unwrap();
    assert_eq!(
        library.preparation_preset(&preset.id).unwrap().revision,
        preset.revision
    );
    let service = CurationService::new(library.clone(), None);
    assert!(
        service
            .catalog()
            .unwrap()
            .groups
            .iter()
            .any(|group| Some(&group.id) == receipt.created_id.as_ref()
                && group.name == "Released Mood")
    );
    let conn = rusqlite::Connection::open(&db).unwrap();
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
            .unwrap(),
        31
    );
    assert!(
        conn.query_row(
            "SELECT manual_hold FROM catalog_rate_state WHERE service='musicbrainz'",
            [],
            |row| row.get::<_, bool>(0)
        )
        .unwrap()
    );
    for table in [
        "native_reconciliation_plans",
        "manual_publication_operations",
        "native_playlist_bindings",
        "rekordbox_sync_operations",
    ] {
        assert!(
            conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                [table],
                |row| row.get::<_, bool>(0)
            )
            .unwrap(),
            "missing {table}"
        );
    }
}
