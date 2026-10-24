use cuerithm_library::{Library, LibraryRestoreStartup, apply_pending_library_restore};
use rusqlite::Connection;
use std::fs;
fn fixture() -> (tempfile::TempDir, Library, std::path::PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("app/library.sqlite");
    let lib = Library::open(&db).unwrap();
    (d, lib, db)
}
#[test]
fn snapshot_restores_only_after_close_and_preserves_original_database() {
    let (d, lib, db) = fixture();
    let destination = d.path().join("backups");
    let backup = lib.create_library_backup(&destination).unwrap();
    assert!(backup.path.join("manifest.json").is_file());
    assert_eq!(fs::read_dir(&backup.path).unwrap().count(), 2);
    let mut settings = lib.settings().unwrap().settings;
    settings.watch_folders = vec![d.path().join("generated-incoming")];
    lib.save_settings(settings).unwrap();
    lib.stage_library_restore(&backup.path, &backup.database_sha256)
        .unwrap();
    assert!(apply_pending_library_restore(&db).is_err());
    assert_eq!(lib.settings().unwrap().settings.watch_folders.len(), 1);
    drop(lib);
    let LibraryRestoreStartup::Applied {
        pre_restore: preserved,
    } = apply_pending_library_restore(&db).unwrap()
    else {
        panic!("Restore was not applied")
    };
    assert!(preserved.join("library.sqlite").is_file());
    let lib = Library::open(&db).unwrap();
    assert!(lib.settings().unwrap().settings.watch_folders.is_empty());
    assert!(matches!(
        apply_pending_library_restore(&db).unwrap(),
        LibraryRestoreStartup::NoPending
    ));
}
#[test]
fn backup_rejects_unsettled_publication_and_restore_rejects_changed_dependencies() {
    let (d, lib, _) = fixture();
    let resource = lib.data_dir().join("cleanup/retained-original");
    fs::create_dir_all(resource.parent().unwrap()).unwrap();
    fs::write(&resource, b"authored original").unwrap();
    let backup = lib
        .create_library_backup(&d.path().join("backups"))
        .unwrap();
    fs::write(&resource, b"changed").unwrap();
    assert!(lib.inspect_library_backup(&backup.path).is_err());
    let conn = Connection::open(lib.data_dir().join("library.sqlite")).unwrap();
    conn.execute("INSERT INTO maintenance_writes(id,kind,state,payload_json,created) VALUES('fixture','apply','conflict','{}',0)",[]).unwrap();
    assert!(
        lib.create_library_backup(&d.path().join("backups"))
            .is_err()
    );
}
#[test]
fn bundle_rejects_tampering_extra_entries_and_wrong_identity() {
    let (d, lib, _) = fixture();
    let backup = lib
        .create_library_backup(&d.path().join("backups"))
        .unwrap();
    fs::write(backup.path.join("extra"), b"unexpected").unwrap();
    assert!(lib.inspect_library_backup(&backup.path).is_err());
    fs::remove_file(backup.path.join("extra")).unwrap();
    let conn = Connection::open(backup.path.join("library.sqlite")).unwrap();
    conn.execute_batch("PRAGMA application_id=1;").unwrap();
    drop(conn);
    assert!(
        lib.stage_library_restore(&backup.path, &backup.database_sha256)
            .is_err()
    );
}
#[cfg(unix)]
#[test]
fn bundle_rejects_symlinks() {
    let (d, lib, _) = fixture();
    let b = lib
        .create_library_backup(&d.path().join("backups"))
        .unwrap();
    fs::rename(b.path.join("library.sqlite"), d.path().join("moved.sqlite")).unwrap();
    std::os::unix::fs::symlink(d.path().join("moved.sqlite"), b.path.join("library.sqlite"))
        .unwrap();
    assert!(lib.inspect_library_backup(&b.path).is_err());
}

fn insert_source(lib: &Library, path: &std::path::Path, hash: &str) {
    #[cfg(unix)]
    let bytes = {
        use std::os::unix::ffi::OsStrExt;
        [b"unix:".as_slice(), path.as_os_str().as_bytes()].concat()
    };
    #[cfg(windows)]
    let bytes = {
        use std::os::windows::ffi::OsStrExt;
        let mut out = b"windows:".to_vec();
        for unit in path.as_os_str().encode_wide() {
            out.extend(unit.to_le_bytes());
        }
        out
    };
    Connection::open(lib.data_dir().join("library.sqlite")).unwrap().execute("INSERT INTO tracks(id,path,title,source_hash,state,created,updated) VALUES('authored',?1,'Authored fixture',?2,'ready',0,0)",rusqlite::params![bytes,hash]).unwrap();
}
#[test]
fn backup_checks_actual_source_bytes_and_restart_revalidates_after_staging() {
    let (d, lib, db) = fixture();
    let source = d.path().join("authored.wav");
    fs::write(&source, b"generated media fixture").unwrap();
    let hash = cuerithm_media::file_sha256(&source).unwrap();
    insert_source(&lib, &source, &hash);
    let b = lib
        .create_library_backup(&d.path().join("backups"))
        .unwrap();
    assert_eq!(b.tracks, 1);
    assert_eq!(b.missing_sources, 0);
    lib.stage_library_restore(&b.path, &b.database_sha256)
        .unwrap();
    fs::write(&source, b"external tag edit after review").unwrap();
    drop(lib);
    assert!(matches!(
        apply_pending_library_restore(&db).unwrap(),
        LibraryRestoreStartup::BlockedBeforeMutation { .. }
    ));
    assert!(
        db.parent()
            .unwrap()
            .join("library-backups/pending.json")
            .is_file()
    );
    assert_eq!(
        fs::read(&source).unwrap(),
        b"external tag edit after review"
    );
    let lib = Library::open(&db).unwrap();
    assert!(
        lib.create_library_backup(&d.path().join("backups"))
            .is_err()
    );
    lib.cancel_library_restore().unwrap();
    assert!(lib.pending_library_restore().unwrap().is_none());
}
#[test]
fn backup_preserves_unavailable_source_status_and_refuses_returned_different_bytes() {
    let (d, lib, _) = fixture();
    let source = d.path().join("missing-authored.wav");
    insert_source(&lib, &source, &"a".repeat(64));
    let b = lib
        .create_library_backup(&d.path().join("backups"))
        .unwrap();
    assert_eq!(b.missing_sources, 1);
    assert_eq!(
        lib.inspect_library_backup(&b.path).unwrap().missing_sources,
        1
    );
    fs::write(source, b"a different recording").unwrap();
    assert!(lib.inspect_library_backup(&b.path).is_err());
}
#[test]
fn pending_sync_or_claimed_jobs_do_not_produce_clean_backups() {
    let (d, lib, _) = fixture();
    let sync = lib.data_dir().join("sync");
    fs::create_dir_all(&sync).unwrap();
    let lease = fs::File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(sync.join("service.lock"))
        .unwrap();
    fs2::FileExt::lock_exclusive(&lease).unwrap();
    assert!(
        lib.create_library_backup(&d.path().join("backups"))
            .is_err()
    );
    drop(lease);
    let source = d.path().join("fixture.wav");
    fs::write(&source, b"fixture").unwrap();
    insert_source(
        &lib,
        &source,
        &cuerithm_media::file_sha256(&source).unwrap(),
    );
    let conn = Connection::open(lib.data_dir().join("library.sqlite")).unwrap();
    conn.execute("INSERT INTO jobs(id,track_id,source_hash,settings_revision,state,stage,created,updated) SELECT 'fixture-job','authored','fixture',revision,'running','analysis',0,0 FROM selected_settings",[]).unwrap();
    assert!(
        lib.create_library_backup(&d.path().join("backups"))
            .is_err()
    );
}
#[test]
fn manifest_rejects_platform_installation_schema_and_missing_retained_recovery() {
    let (d, lib, _) = fixture();
    let b = lib
        .create_library_backup(&d.path().join("backups"))
        .unwrap();
    let path = b.path.join("manifest.json");
    let original = fs::read(&path).unwrap();
    let mut json: serde_json::Value = serde_json::from_slice(&original).unwrap();
    json["schema"] = serde_json::json!(999);
    fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
    assert!(lib.inspect_library_backup(&b.path).is_err());
    json = serde_json::from_slice(&original).unwrap();
    json["platform"] = serde_json::json!("different-platform");
    fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
    assert!(lib.inspect_library_backup(&b.path).is_err());
    fs::write(&path, &original).unwrap();
    let other = Library::open(&d.path().join("another/library.sqlite")).unwrap();
    assert!(other.inspect_library_backup(&b.path).is_err());
}

#[cfg(unix)]
#[test]
fn staging_refuses_a_redirected_managed_directory() {
    let (d, lib, _) = fixture();
    let b = lib
        .create_library_backup(&d.path().join("backups"))
        .unwrap();
    let redirected = d.path().join("unrelated");
    fs::create_dir(&redirected).unwrap();
    std::os::unix::fs::symlink(&redirected, lib.data_dir().join("library-backups")).unwrap();
    assert!(
        lib.stage_library_restore(&b.path, &b.database_sha256)
            .is_err()
    );
    assert_eq!(fs::read_dir(redirected).unwrap().count(), 0);
}
#[test]
fn unavailable_original_recovery_copy_blocks_backup_without_pruning_history() {
    let (d, lib, _) = fixture();
    let conn = Connection::open(lib.data_dir().join("library.sqlite")).unwrap();
    conn.execute("INSERT INTO maintenance_writes(id,kind,state,payload_json,created,cleanup_complete) VALUES('retained','apply','committed','{}',0,1)",[]).unwrap();
    // The journal names an original that is not there.
    conn.execute("INSERT INTO maintenance_write_files(write_id,ordinal,target,staged,backup,before_hash,after_hash,publish_path,displaced_path,workspace) VALUES('retained',0,X'00',X'00',CAST('unix:/an-original-that-is-gone' AS BLOB),'before','after',X'00',X'00',X'00')",[]).unwrap();
    assert!(
        lib.create_library_backup(&d.path().join("backups"))
            .is_err()
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM maintenance_writes", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn a_write_whose_original_was_retired_on_purpose_does_not_block_a_backup() {
    let (d, lib, _) = fixture();
    let conn = Connection::open(lib.data_dir().join("library.sqlite")).unwrap();
    conn.execute("INSERT INTO maintenance_writes(id,kind,state,payload_json,created,cleanup_complete) VALUES('settled','apply','committed','{}',0,1)",[]).unwrap();
    conn.execute("INSERT INTO maintenance_write_files(write_id,ordinal,target,staged,backup,before_hash,after_hash,publish_path,displaced_path,workspace) VALUES('settled',0,X'00',X'00',NULL,'before','after',X'00',X'00',X'00')",[]).unwrap();
    lib.create_library_backup(&d.path().join("backups"))
        .unwrap();
}

fn unused_removal_row(lib: &Library, id: &str, backup: &[u8]) {
    let payload = serde_json::json!({
        "state": "committed",
        "undone_by": null,
        "cleanup_complete": true,
        "backups_disposed": false,
        "files": [{
            "backup": backup,
            "entry": {"public": {"source_sha256": "f".repeat(64)}}
        }]
    });
    Connection::open(lib.data_dir().join("library.sqlite"))
        .unwrap()
        .execute(
            "INSERT INTO maintenance_scans VALUES(?1,'unused_removal_operation',?2,0)",
            rusqlite::params![id, payload.to_string()],
        )
        .unwrap();
}

#[test]
fn an_unused_file_removal_made_without_a_copy_does_not_block_a_backup_but_a_missing_copy_does() {
    let (d, lib, _) = fixture();
    // Removals since the library stopped keeping copies name none: the file is in the Trash.
    unused_removal_row(&lib, "trashed", &[]);
    lib.create_library_backup(&d.path().join("backups"))
        .unwrap();
    // An earlier version's removal still names its copy, which must be there and unchanged.
    unused_removal_row(&lib, "older", b"unix:/a-copy-that-is-gone");
    assert!(
        lib.create_library_backup(&d.path().join("backups-2"))
            .is_err()
    );
}

#[test]
fn blocked_restore_keeps_current_library_openable_and_requires_explicit_retry() {
    let (d, lib, db) = fixture();
    let source = d.path().join("generated.wav");
    fs::write(&source, b"original authored bytes").unwrap();
    insert_source(
        &lib,
        &source,
        &cuerithm_media::file_sha256(&source).unwrap(),
    );
    let b = lib
        .create_library_backup(&d.path().join("backups"))
        .unwrap();
    let mut settings = lib.settings().unwrap().settings;
    settings.watch_folders = vec![d.path().join("current preparation")];
    lib.save_settings(settings).unwrap();
    lib.stage_library_restore(&b.path, &b.database_sha256)
        .unwrap();
    fs::write(&source, b"external edits").unwrap();
    drop(lib);
    assert!(matches!(
        apply_pending_library_restore(&db).unwrap(),
        LibraryRestoreStartup::BlockedBeforeMutation { .. }
    ));
    let lib = Library::open(&db).unwrap();
    assert_eq!(lib.settings().unwrap().settings.watch_folders.len(), 1);
    assert!(
        lib.pending_library_restore()
            .unwrap()
            .unwrap()
            .blocked_reason
            .is_some()
    );
    assert!(lib.retry_library_restore().is_err());
    fs::write(&source, b"original authored bytes").unwrap();
    // Merely fixing the files does not silently retry a blocked request.
    assert!(matches!(
        apply_pending_library_restore(&db).unwrap(),
        LibraryRestoreStartup::BlockedBeforeMutation { .. }
    ));
    assert!(
        lib.retry_library_restore()
            .unwrap()
            .blocked_reason
            .is_none()
    );
    drop(lib);
    assert!(matches!(
        apply_pending_library_restore(&db).unwrap(),
        LibraryRestoreStartup::Applied { .. }
    ));
    let lib = Library::open(&db).unwrap();
    assert!(lib.settings().unwrap().settings.watch_folders.is_empty());
}
#[test]
fn interrupted_database_update_reuses_original_recovery_snapshot_and_never_claims_safe_block() {
    let (d, lib, db) = fixture();
    let b = lib
        .create_library_backup(&d.path().join("backups"))
        .unwrap();
    let mut settings = lib.settings().unwrap().settings;
    settings.watch_folders = vec![d.path().join("before restore")];
    lib.save_settings(settings).unwrap();
    let original = lib
        .create_library_backup(&lib.data_dir().join("library-backups/recovery-fixture"))
        .unwrap();
    let request = lib
        .stage_library_restore(&b.path, &b.database_sha256)
        .unwrap();
    let pending = lib.data_dir().join("library-backups/pending.json");
    let mut json: serde_json::Value = serde_json::from_slice(&fs::read(&pending).unwrap()).unwrap();
    json["mutation_started"] = true.into();
    json["pre_restore"] = serde_json::to_value(&original.path).unwrap();
    fs::write(&pending, serde_json::to_vec(&json).unwrap()).unwrap();
    drop(lib);
    let LibraryRestoreStartup::Applied { pre_restore } =
        apply_pending_library_restore(&db).unwrap()
    else {
        panic!("Interrupted restore was not recovered")
    };
    assert_eq!(pre_restore, original.path);
    let conn = Connection::open(pre_restore.join("library.sqlite")).unwrap();
    let saved: String = conn
        .query_row(
            "SELECT s.json FROM settings s JOIN selected_settings a ON a.revision=s.revision",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&saved).unwrap()["settings"]["watch_folders"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // Reintroduce the durable marker with a missing recovery snapshot: startup must
    // stop, rather than opening a possibly partially updated library as unchanged.
    json["id"] = request.id.into();
    json["pre_restore"] = serde_json::to_value(d.path().join("absent")).unwrap();
    fs::write(&pending, serde_json::to_vec(&json).unwrap()).unwrap();
    assert!(apply_pending_library_restore(&db).is_err());
}

#[test]
fn durable_recovery_pointer_rejects_parent_traversal() {
    let (d, lib, db) = fixture();
    let b = lib
        .create_library_backup(&d.path().join("backups"))
        .unwrap();
    lib.stage_library_restore(&b.path, &b.database_sha256)
        .unwrap();
    let directory = lib.data_dir().join("library-backups");
    let pending = directory.join("pending.json");
    let mut json: serde_json::Value = serde_json::from_slice(&fs::read(&pending).unwrap()).unwrap();
    json["mutation_started"] = true.into();
    json["pre_restore"] = serde_json::to_value(
        directory
            .join("../..")
            .join("backups")
            .join(b.path.file_name().unwrap()),
    )
    .unwrap();
    fs::write(&pending, serde_json::to_vec(&json).unwrap()).unwrap();
    drop(lib);
    assert!(apply_pending_library_restore(&db).is_err());
}

fn organized_fixture(
    mode: cuerithm_library::organization::OrganizationMode,
) -> (
    tempfile::TempDir,
    Library,
    std::path::PathBuf,
    std::path::PathBuf,
    std::path::PathBuf,
    String,
) {
    use cuerithm_library::organization::OrganizationRequest;
    let (d, lib, db) = fixture();
    let root = d.path().canonicalize().unwrap();
    let source_root = root.join("music");
    let destination_root = if mode == cuerithm_library::organization::OrganizationMode::Rename {
        source_root.clone()
    } else {
        root.join("managed")
    };
    fs::create_dir_all(&source_root).unwrap();
    fs::create_dir_all(&destination_root).unwrap();
    let source = source_root.join("authored.wav");
    let sidecar = source_root.join("authored.jpg");
    fs::write(&source, b"authored organization audio").unwrap();
    fs::write(&sidecar, b"authored owned artwork").unwrap();
    insert_source(
        &lib,
        &source,
        &cuerithm_media::file_sha256(&source).unwrap(),
    );
    let conn = Connection::open(&db).unwrap();
    #[cfg(unix)]
    let encoded = {
        use std::os::unix::ffi::OsStrExt;
        [b"unix:".as_slice(), sidecar.as_os_str().as_bytes()].concat()
    };
    #[cfg(windows)]
    let encoded = {
        use std::os::windows::ffi::OsStrExt;
        let mut b = b"windows:".to_vec();
        for n in sidecar.as_os_str().encode_wide() {
            b.extend(n.to_le_bytes());
        }
        b
    };
    conn.execute("INSERT INTO organization_owned_sidecars(track_id,path,source_hash,manifest_id,ordinal) VALUES('authored',?1,?2,'fixture',1)",rusqlite::params![encoded,cuerithm_media::file_sha256(&sidecar).unwrap()]).unwrap();
    drop(conn);
    let destination = destination_root.join("organized.wav");
    let destination_sidecar = destination_root.join("organized.jpg");
    let operation = lib
        .plan_organization(OrganizationRequest {
            track_id: "authored".into(),
            mode,
            source_root,
            destination_root,
            files: vec![
                (source.clone(), destination.clone()),
                (sidecar, destination_sidecar.clone()),
            ],
        })
        .unwrap();
    (d, lib, db, destination, destination_sidecar, operation)
}
fn finish_organization(lib: &Library, id: &str) {
    lib.execute_organization(id).unwrap();
    lib.acknowledge_organization_references(id).unwrap();
    lib.execute_organization(id).unwrap();
}
#[test]
fn settled_organization_with_owned_sidecars_remains_backed_up_and_restorable() {
    use cuerithm_library::organization::OrganizationMode;
    for mode in [
        OrganizationMode::Move,
        OrganizationMode::Copy,
        OrganizationMode::Rename,
    ] {
        let (d, lib, db, destination, sidecar, id) = organized_fixture(mode);
        finish_organization(&lib, &id);
        let backup = lib
            .create_library_backup(&d.path().join("backups"))
            .unwrap();
        assert_eq!(backup.schema, 31);
        assert_eq!(lib.inspect_library_backup(&backup.path).unwrap().tracks, 1);
        lib.stage_library_restore(&backup.path, &backup.database_sha256)
            .unwrap();
        drop(lib);
        assert!(matches!(
            apply_pending_library_restore(&db).unwrap(),
            LibraryRestoreStartup::Applied { .. }
        ));
        let reopened = Library::open(&db).unwrap();
        assert_eq!(reopened.track_path("authored").unwrap(), destination);
        assert_eq!(fs::read(sidecar).unwrap(), b"authored owned artwork");
        assert_eq!(reopened.organization_history().unwrap().len(), 1);
    }
}
#[test]
fn backup_rejects_unsettled_organization_boundaries_but_allows_completed_history() {
    use cuerithm_library::organization::OrganizationMode;
    let (d, lib, _, _, _, id) = organized_fixture(OrganizationMode::Move);
    for step in 0..3 {
        let error = lib
            .create_library_backup(&d.path().join("backups"))
            .unwrap_err();
        assert!(
            error.to_string().to_lowercase().contains("organization"),
            "{error:#}"
        );
        if step == 0 {
            lib.execute_organization(&id).unwrap();
        } else if step == 1 {
            lib.acknowledge_organization_references(&id).unwrap();
        }
    }
    lib.execute_organization(&id).unwrap();
    assert!(lib.create_library_backup(&d.path().join("backups")).is_ok());
}
#[test]
fn backup_manifest_binds_owned_sidecars_and_native_original_objects() {
    use cuerithm_library::organization::OrganizationMode;
    let (d, lib, _, _, sidecar, id) = organized_fixture(OrganizationMode::Move);
    finish_organization(&lib, &id);
    let backup = lib
        .create_library_backup(&d.path().join("backups"))
        .unwrap();
    fs::write(&sidecar, b"changed sidecar").unwrap();
    assert!(lib.inspect_library_backup(&backup.path).is_err());
    fs::write(&sidecar, b"authored owned artwork").unwrap();
    let record = lib.organization_history().unwrap().remove(0);
    let file = &record.files[0];
    let quarantine = file
        .source
        .path()
        .unwrap()
        .parent()
        .unwrap()
        .join(&file.quarantine_name);
    let preserved = quarantine.with_extension("preserved-fixture");
    fs::rename(&quarantine, &preserved).unwrap();
    fs::write(&quarantine, fs::read(&preserved).unwrap()).unwrap();
    assert!(lib.inspect_library_backup(&backup.path).is_err());
    assert!(
        lib.create_library_backup(&d.path().join("fresh")).is_err(),
        "Equal bytes from a different native original cannot restore recovery authority"
    );
}
#[test]
fn completed_copy_originals_and_native_reference_resources_are_required() {
    use cuerithm_library::organization::OrganizationMode;
    let (d, lib, _, _, _, id) = organized_fixture(OrganizationMode::Copy);
    finish_organization(&lib, &id);
    let resource = lib
        .data_dir()
        .join("organization-references/authored-retained/backup");
    fs::create_dir_all(resource.parent().unwrap()).unwrap();
    fs::write(&resource, b"authored native recovery bytes").unwrap();
    let backup = lib
        .create_library_backup(&d.path().join("backups"))
        .unwrap();
    fs::remove_file(&resource).unwrap();
    assert!(lib.inspect_library_backup(&backup.path).is_err());
    fs::write(&resource, b"authored native recovery bytes").unwrap();
    let original = lib.organization_history().unwrap()[0].files[0]
        .source
        .path()
        .unwrap();
    let preserved = original.with_extension("preserved-fixture");
    fs::rename(&original, &preserved).unwrap();
    fs::write(&original, fs::read(preserved).unwrap()).unwrap();
    assert!(lib.inspect_library_backup(&backup.path).is_err());
    assert!(lib.create_library_backup(&d.path().join("fresh")).is_err());
}
#[test]
fn repeated_moves_and_undo_do_not_require_obsolete_historical_destinations() {
    use cuerithm_library::organization::{OrganizationMode, OrganizationRequest};
    let (d, lib, _, destination, sidecar, first) = organized_fixture(OrganizationMode::Move);
    finish_organization(&lib, &first);
    let second_destination = destination.with_file_name("second.wav");
    let second_sidecar = sidecar.with_file_name("second.jpg");
    let second = lib
        .plan_organization(OrganizationRequest {
            track_id: "authored".into(),
            mode: OrganizationMode::Move,
            source_root: destination.parent().unwrap().into(),
            destination_root: destination.parent().unwrap().into(),
            files: vec![
                (destination.clone(), second_destination),
                (sidecar, second_sidecar),
            ],
        })
        .unwrap();
    finish_organization(&lib, &second);
    assert!(!destination.exists());
    assert!(lib.create_library_backup(&d.path().join("backups")).is_ok());
    let undo = lib.plan_organization_undo(&second).unwrap();
    finish_organization(&lib, &undo);
    assert!(
        lib.create_library_backup(&d.path().join("after-undo"))
            .is_ok()
    );
}

#[test]
fn pre_proof_ignored_copy_history_is_preserved_and_bound_to_observed_native_identity() {
    use cuerithm_library::organization::{AlternateResolution, OrganizationMode};
    let (d, lib, db, _, _, id) = organized_fixture(OrganizationMode::Copy);
    finish_organization(&lib, &id);
    let source = lib.organization_history().unwrap()[0].files[0]
        .source
        .path()
        .unwrap();
    fs::write(&source, b"explicitly ignored authored revision").unwrap();
    let hash = cuerithm_media::file_sha256(&source).unwrap();
    lib.organization_source_state(&source, &hash).unwrap();
    lib.resolve_organization_alternate(&id, AlternateResolution::IgnoreThisRevision)
        .unwrap();
    Connection::open(&db)
        .unwrap()
        .execute(
            "DELETE FROM organization_copy_conflicts WHERE operation_id=?1",
            [&id],
        )
        .unwrap();
    let backup = lib
        .create_library_backup(&d.path().join("backups"))
        .unwrap();
    lib.inspect_library_backup(&backup.path).unwrap();
    let preserved = source.with_extension("preserved-fixture");
    fs::rename(&source, &preserved).unwrap();
    fs::write(&source, fs::read(preserved).unwrap()).unwrap();
    assert!(lib.inspect_library_backup(&backup.path).is_err());
}
#[test]
fn preparation_attention_checkpoints_are_unsettled_even_when_jobs_are_not_running() {
    let (d, lib, db) = fixture();
    let source = d.path().join("authored.wav");
    fs::write(&source, b"checkpoint fixture").unwrap();
    let hash = cuerithm_media::file_sha256(&source).unwrap();
    insert_source(&lib, &source, &hash);
    let connection = Connection::open(&db).unwrap();
    let revision = lib.settings().unwrap().revision;
    connection.execute("INSERT INTO jobs(id,track_id,source_hash,settings_revision,state,stage,created,updated) VALUES('checkpoint-job','authored',?1,?2,'attention','organizing',0,0)",rusqlite::params![hash,revision]).unwrap();
    connection.execute("INSERT INTO preparation_checkpoints(job_id,track_id,settings_revision,input_hash,output_hash,attempt,payload_json,state,created,updated) VALUES('checkpoint-job','authored',?1,?2,?2,1,'{}','prepared',0,0)",rusqlite::params![revision,hash]).unwrap();
    for state in ["tagging", "prepared", "organizing"] {
        connection
            .execute("UPDATE preparation_checkpoints SET state=?1", [state])
            .unwrap();
        assert!(
            lib.create_library_backup(&d.path().join("backups"))
                .unwrap_err()
                .to_string()
                .to_lowercase()
                .contains("organization")
        );
    }
}
#[cfg(unix)]
#[test]
fn sparse_oversized_organization_original_is_rejected_before_reading_its_bytes() {
    use cuerithm_library::organization::OrganizationMode;
    let (d, lib, _, _, _, id) = organized_fixture(OrganizationMode::Move);
    finish_organization(&lib, &id);
    let record = lib.organization_history().unwrap().remove(0);
    let file = &record.files[0];
    let quarantine = file
        .source_parent
        .path
        .path()
        .unwrap()
        .join(&file.quarantine_name);
    let file = fs::OpenOptions::new().write(true).open(quarantine).unwrap();
    file.set_len(1024_u64.pow(4) + 1).unwrap();
    let result = lib.create_library_backup(&d.path().join("backups"));
    // Reclaim the authored sparse fixture immediately, even if the assertion fails.
    file.set_len(0).unwrap();
    assert!(result.unwrap_err().to_string().contains("budget"));
}

#[test]
fn cancelling_restore_removes_only_its_owned_staged_copy_and_is_repeatable() {
    let (d, lib, _) = fixture();
    let backup = lib
        .create_library_backup(&d.path().join("backups"))
        .unwrap();
    let intent = lib
        .stage_library_restore(&backup.path, &backup.database_sha256)
        .unwrap();
    let staged = lib.data_dir().join("library-backups").join(&intent.id);
    assert!(staged.join("library.sqlite").is_file());
    lib.cancel_library_restore().unwrap();
    assert!(!staged.exists(), "cancelled copies must not accumulate");
    assert!(
        backup.path.join("library.sqlite").is_file(),
        "the selected original backup stays intact"
    );
    assert!(lib.pending_library_restore().unwrap().is_none());
    lib.cancel_library_restore().unwrap();
}
#[test]
fn restore_cancellation_preserves_unowned_entries_and_started_recovery() {
    let (d, lib, _) = fixture();
    let backup = lib
        .create_library_backup(&d.path().join("backups"))
        .unwrap();
    let intent = lib
        .stage_library_restore(&backup.path, &backup.database_sha256)
        .unwrap();
    let staged = lib.data_dir().join("library-backups").join(&intent.id);
    fs::write(staged.join("unowned"), b"preserve this file").unwrap();
    assert!(lib.cancel_library_restore().is_err());
    assert!(staged.join("library.sqlite").exists());
    assert!(lib.pending_library_restore().unwrap().is_some());
    fs::remove_file(staged.join("unowned")).unwrap();
    let pending = lib.data_dir().join("library-backups/pending.json");
    let mut payload: serde_json::Value =
        serde_json::from_slice(&fs::read(&pending).unwrap()).unwrap();
    payload["mutation_started"] = true.into();
    fs::write(&pending, serde_json::to_vec(&payload).unwrap()).unwrap();
    assert!(lib.cancel_library_restore().is_err());
    assert!(staged.join("library.sqlite").exists());
    assert!(pending.exists());
}

#[test]
fn library_backup_refuses_unsettled_native_rekordbox_publication() {
    let (d, lib, db) = fixture();
    let conn = Connection::open(db).unwrap();
    conn.execute("INSERT INTO rekordbox_sync_operations(id,target_id,database_path,status,journal_json,created,updated) VALUES('authored','target','authored.db','publishing','{}',0,0)", []).unwrap();
    for status in ["prepared", "publishing", "native_published", "restoring"] {
        conn.execute("UPDATE rekordbox_sync_operations SET status=?1", [status])
            .unwrap();
        assert!(
            lib.create_library_backup(&d.path().join("backups"))
                .is_err(),
            "{status}"
        );
    }
    for status in ["applied", "restored", "discarded", "rolled_back"] {
        conn.execute("UPDATE rekordbox_sync_operations SET status=?1", [status])
            .unwrap();
        assert!(
            lib.create_library_backup(&d.path().join("backups")).is_ok(),
            "{status}"
        );
    }
}

#[test]
fn backup_refuses_metadata_recovery_and_auxiliary_cleanup() {
    let (d, lib, _) = fixture();
    let conn = Connection::open(lib.data_dir().join("library.sqlite")).unwrap();
    // A save can be interrupted before the first physical-write journal exists.
    conn.execute(
        "INSERT INTO metadata_edit_drafts VALUES('draft',1,'editing','{}',0,0)",
        [],
    )
    .unwrap();
    conn.execute("INSERT INTO metadata_edit_batches VALUES('batch','draft',1,'apply','saving','{}',NULL,0,0)", []).unwrap();
    assert!(
        lib.create_library_backup(&d.path().join("backups"))
            .is_err(),
        "interrupted metadata saves require recovery"
    );
    conn.execute("DELETE FROM metadata_edit_batches", [])
        .unwrap();
    conn.execute("INSERT INTO artifact_cleanup_moves(id,owner_kind,owner_id,source_path,quarantine_path,payload_json,state,observed_bytes,created,updated) VALUES('cleanup','metadata_edit','batch',X'00',X'00','{}','retained',0,0,0)", []).unwrap();
    assert!(
        lib.create_library_backup(&d.path().join("backups"))
            .is_err(),
        "unproven external cleanup must not be omitted"
    );
}

#[cfg(feature = "autotag")]
fn edited_metadata_fixture() -> (
    tempfile::TempDir,
    Library,
    std::path::PathBuf,
    cuerithm_media::MediaRuntime,
    String,
    String,
) {
    use cuerithm_autotag::{MetadataTextField, TextEdit};
    use cuerithm_library::{MetadataEditService, MetadataEditorPatch};
    let (d, lib, db) = fixture();
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = d.path().canonicalize().unwrap().join("authored.flac");
    fs::copy(
        repo.join("crates/cuerithm-media/tests/fixtures/covered.flac"),
        &path,
    )
    .unwrap();
    insert_source(&lib, &path, &cuerithm_media::file_sha256(&path).unwrap());
    let runtime =
        cuerithm_media::MediaRuntime::from_bundle(&repo.join("tools/media-runtime/bin")).unwrap();
    let service = MetadataEditService::new(lib.clone(), runtime.clone());
    let draft = service.create_draft(&["authored".into()]).unwrap();
    let mut patch = MetadataEditorPatch::default();
    patch.fields.text.insert(
        MetadataTextField::Title,
        TextEdit::Set {
            values: vec!["Saved metadata title".into()],
        },
    );
    let draft = service
        .update_draft(&draft.id, draft.revision, patch)
        .unwrap();
    let preview = service
        .preview(&draft.id, draft.revision, &Default::default())
        .unwrap();
    let batch = service.apply(&preview.id, &Default::default()).unwrap();
    assert_eq!(batch.state, "saved", "{batch:?}");
    let snapshot = service
        .snapshot(&["authored".into()])
        .unwrap()
        .tracks
        .remove(0);
    let asset = service
        .extract_artwork(
            "authored",
            &snapshot.source_sha256,
            snapshot.native.artwork[0].identity.clone(),
        )
        .unwrap();
    let draft = service.create_draft(&["authored".into()]).unwrap();
    let mut patch = MetadataEditorPatch::default();
    patch.artwork.push(cuerithm_library::MetadataArtworkChoice {
        target: None,
        asset_id: Some(asset.id),
        kind: Some(3),
        description: Some("Saved draft artwork".into()),
    });
    service
        .update_draft(&draft.id, draft.revision, patch)
        .unwrap();
    (d, lib, db, runtime, batch.id, draft.id)
}

#[cfg(feature = "autotag")]
#[test]
fn metadata_draft_artwork_history_and_undo_survive_same_installation_restore() {
    use cuerithm_library::MetadataEditService;
    let (d, lib, db, runtime, batch, draft) = edited_metadata_fixture();
    let service = MetadataEditService::new(lib.clone(), runtime.clone());
    let draft_before = serde_json::to_value(service.draft(&draft).unwrap()).unwrap();
    let history_before = serde_json::to_value(service.history().unwrap()).unwrap();
    let source = d.path().canonicalize().unwrap().join("authored.flac");
    let saved_hash = cuerithm_media::file_sha256(&source).unwrap();
    let backup = lib
        .create_library_backup(&d.path().join("backups"))
        .unwrap();
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(backup.path.join("manifest.json")).unwrap()).unwrap();
    let metadata_root = lib.data_dir().join("metadata-edits");
    let expected_path =
        cuerithm_library::organization::NativePath::new(&metadata_root.join("assets"));
    assert!(
        manifest["resources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|resource| resource["path"] == serde_json::to_value(&expected_path.0).unwrap()),
        "metadata assets must be bound to the manifest"
    );
    lib.stage_library_restore(&backup.path, &backup.database_sha256)
        .unwrap();
    drop(service);
    drop(lib);
    assert!(matches!(
        apply_pending_library_restore(&db).unwrap(),
        LibraryRestoreStartup::Applied { .. }
    ));
    let lib = Library::open(&db).unwrap();
    let service = MetadataEditService::new(lib.clone(), runtime);
    assert_eq!(
        serde_json::to_value(service.draft(&draft).unwrap()).unwrap(),
        draft_before
    );
    assert_eq!(
        serde_json::to_value(service.history().unwrap()).unwrap(),
        history_before
    );
    let restored_draft = service.draft(&draft).unwrap();
    assert!(
        !service
            .artwork_bytes(restored_draft.patch.artwork[0].asset_id.as_deref().unwrap())
            .unwrap()
            .is_empty()
    );
    assert_eq!(cuerithm_media::file_sha256(&source).unwrap(), saved_hash);
    let undo = service.undo(&batch, &Default::default()).unwrap();
    assert_eq!(undo.state, "undone", "{undo:?}");
    assert_ne!(cuerithm_media::file_sha256(&source).unwrap(), saved_hash);
}

#[cfg(feature = "autotag")]
#[test]
fn metadata_backup_refuses_changed_retained_artwork_and_a_settled_save_keeps_no_original() {
    let (d, lib, _db, _runtime, _batch, _draft) = edited_metadata_fixture();
    let backup = lib
        .create_library_backup(&d.path().join("backups"))
        .unwrap();
    let conn = Connection::open(lib.data_dir().join("library.sqlite")).unwrap();
    // A settled save deleted its original on purpose: nothing names one, and the folder is gone.
    let named: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM metadata_edit_files WHERE backup IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(named, 0);
    let encoded: Vec<u8> = conn
        .query_row(
            "SELECT path FROM metadata_artwork_assets LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let asset = cuerithm_library::organization::NativePath(encoded)
        .path()
        .unwrap();
    let artwork_bytes = fs::read(&asset).unwrap();
    fs::write(&asset, b"later external artwork edit").unwrap();
    assert!(lib.inspect_library_backup(&backup.path).is_err());
    assert!(
        lib.create_library_backup(&d.path().join("backups"))
            .is_err()
    );
    fs::write(&asset, artwork_bytes).unwrap();
    assert!(
        lib.create_library_backup(&d.path().join("backups")).is_ok(),
        "restoring the bytes makes the artwork whole again"
    );
}

#[test]
fn attention_job_metadata_and_review_outboxes_block_the_whole_restore_flow() {
    for (kind, state) in [
        ("override", "queued"),
        ("override", "claimed"),
        ("override", "committed"),
        ("review", "pending"),
        ("review", "claimed"),
        ("review", "applied"),
        ("write", "prepared"),
        ("write", "publishing"),
        ("write", "recovery_required"),
        ("write", "conflict"),
    ] {
        let (d, lib, db) = fixture();
        let source = d.path().join("fixture.wav");
        fs::write(&source, b"authored source").unwrap();
        insert_source(
            &lib,
            &source,
            &cuerithm_media::file_sha256(&source).unwrap(),
        );
        let backup = lib
            .create_library_backup(&d.path().join("backups"))
            .unwrap();
        lib.stage_library_restore(&backup.path, &backup.database_sha256)
            .unwrap();
        let conn = Connection::open(&db).unwrap();
        conn.execute("INSERT INTO jobs(id,track_id,source_hash,settings_revision,state,stage,created,updated) SELECT 'job','authored','fixture',revision,'attention','tagging',0,0 FROM selected_settings",[]).unwrap();
        conn.execute(
            "INSERT INTO metadata_edit_drafts VALUES('draft',1,'editing','{}',0,0)",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO metadata_edit_batches VALUES('batch','draft',1,'apply','saved','{}',NULL,0,0)",[]).unwrap();
        match kind {
            "override" => {
                conn.execute("INSERT INTO metadata_edit_overrides(job_id,track_id,draft_id,draft_revision,batch_id,source_hash,settings_revision,path_revision,metadata_revision,accepted_attempt,execution_source_proof_json,state,payload_json,created,updated) SELECT 'job','authored','draft',1,'batch','fixture',revision,0,0,0,'{}',?1,'{}',0,0 FROM selected_settings",[state]).unwrap();
            }
            "review" => {
                conn.execute("INSERT INTO preparation_review_decisions(job_id,track_id,source_hash,settings_revision,generation,accepted_attempt,payload_json,binding_json,state,created,updated) SELECT 'job','authored','fixture',revision,'generation',0,'{}','{}',?1,0,0 FROM selected_settings",[state]).unwrap();
            }
            "write" => {
                conn.execute("INSERT INTO metadata_edit_rows(id,batch_id,track_id,ordinal,state,payload_json) VALUES('row','batch','authored',0,'written','{}')",[]).unwrap();
                conn.execute("INSERT INTO metadata_edit_writes(id,batch_id,row_id,track_id,state,payload_json,workspace,expected_path_revision,expected_metadata_revision,output_hash,created,updated) VALUES('write','batch','row','authored',?1,'{}',X'00',0,0,'fixture',0,0)",[state]).unwrap();
            }
            _ => unreachable!(),
        }
        for result in [
            lib.create_library_backup(&d.path().join("blocked-backups"))
                .map(|_| ()),
            lib.inspect_library_backup(&backup.path).map(|_| ()),
            lib.stage_library_restore(&backup.path, &backup.database_sha256)
                .map(|_| ()),
        ] {
            let error = result.unwrap_err().to_string();
            assert!(
                error.contains("Finish metadata saves"),
                "{kind}/{state}: {error}"
            );
        }
        drop(conn);
        drop(lib);
        match apply_pending_library_restore(&db).unwrap() {
            LibraryRestoreStartup::BlockedBeforeMutation { reason, .. } => assert!(
                reason.contains("Finish metadata saves"),
                "{kind}/{state}: {reason}"
            ),
            other => panic!("{kind}/{state}: {other:?}"),
        }
    }
}

#[cfg(feature = "autotag")]
#[test]
fn metadata_restore_rechecks_retained_artwork_after_staging() {
    let (d, lib, db, _runtime, _batch, _draft) = edited_metadata_fixture();
    let backup = lib
        .create_library_backup(&d.path().join("backups"))
        .unwrap();
    lib.stage_library_restore(&backup.path, &backup.database_sha256)
        .unwrap();
    let conn = Connection::open(&db).unwrap();
    let encoded: Vec<u8> = conn
        .query_row(
            "SELECT path FROM metadata_artwork_assets LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let asset = cuerithm_library::organization::NativePath(encoded)
        .path()
        .unwrap();
    fs::write(&asset, b"externally changed after staging").unwrap();
    drop(conn);
    drop(lib);
    assert!(matches!(
        apply_pending_library_restore(&db).unwrap(),
        LibraryRestoreStartup::BlockedBeforeMutation { .. }
    ));
    assert_eq!(
        fs::read(asset).unwrap(),
        b"externally changed after staging"
    );
}

#[test]
fn fresh_installation_without_data_directory_has_no_pending_restore() {
    let d = tempfile::tempdir().unwrap();
    let data = d.path().join("not-created");
    assert!(matches!(
        apply_pending_library_restore(&data.join("library.sqlite")).unwrap(),
        LibraryRestoreStartup::NoPending
    ));
    assert!(!data.exists());
}

#[test]
fn mapped_curation_without_a_metadata_child_blocks_every_backup_boundary() {
    use cuerithm_library::*;
    for phase in ["create", "inspect", "stage", "retry", "startup"] {
        let (d, lib, db) = fixture();
        let source = d.path().join("authored.wav");
        fs::write(&source, b"opaque authored curation fixture").unwrap();
        insert_source(
            &lib,
            &source,
            &cuerithm_media::file_sha256(&source).unwrap(),
        );
        let backup = lib
            .create_library_backup(&d.path().join("backups"))
            .unwrap();
        if matches!(phase, "retry" | "startup") {
            lib.stage_library_restore(&backup.path, &backup.database_sha256)
                .unwrap();
        }
        let service = CurationService::new(lib.clone(), None);
        service
            .edit_taxonomy(CurationTaxonomyRequest {
                request_id: uuid::Uuid::new_v4().to_string(),
                expected_revision: service.catalog().unwrap().revision,
                change: CurationTaxonomyChange::PutMapping {
                    mapping: CurationMapping {
                        id: "notes".into(),
                        source: CurationMappingSource::Notes,
                        target: CurationMappingTarget::Comment {
                            language: "eng".into(),
                            description: "CueRithm notes".into(),
                            encoding: CurationEncoding::Values,
                        },
                        containers: vec![CurationContainer::Id3],
                        enabled: true,
                        allow_id3_upgrade: false,
                    },
                },
            })
            .unwrap();
        service
            .update(CurationUpdateRequest {
                request_id: uuid::Uuid::new_v4().to_string(),
                taxonomy_revision: service.catalog().unwrap().revision,
                tracks: vec![CurationExpectedRevision {
                    track_id: "authored".into(),
                    revision: 0,
                }],
                patch: CurationPatch {
                    notes: CurationValueEdit::Set {
                        value: "mapped intent".into(),
                    },
                    ..Default::default()
                },
            })
            .unwrap();
        let conn = Connection::open(&db).unwrap();
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM metadata_edit_batches", [], |r| r
                .get::<_, u64>(0))
                .unwrap(),
            0
        );
        drop(conn);
        let result = match phase {
            "create" => lib
                .create_library_backup(&d.path().join("blocked"))
                .map(|_| ()),
            "inspect" => lib.inspect_library_backup(&backup.path).map(|_| ()),
            "stage" => lib
                .stage_library_restore(&backup.path, &backup.database_sha256)
                .map(|_| ()),
            "retry" => lib.retry_library_restore().map(|_| ()),
            "startup" => {
                drop(service);
                drop(lib);
                let result = apply_pending_library_restore(&db).unwrap();
                assert!(matches!(
                    result,
                    LibraryRestoreStartup::BlockedBeforeMutation { .. }
                ));
                let conn = Connection::open(&db).unwrap();
                assert_eq!(conn.query_row("SELECT COUNT(*) FROM curation_tracks WHERE revision!=acknowledged_revision", [], |r| r.get::<_, u64>(0)).unwrap(), 1);
                assert_eq!(
                    fs::read(&source).unwrap(),
                    b"opaque authored curation fixture"
                );
                continue;
            }
            _ => unreachable!(),
        };
        assert!(
            result.is_err(),
            "{phase} must preserve physical Curate publication ownership before child allocation"
        );
        assert_eq!(
            fs::read(&source).unwrap(),
            b"opaque authored curation fixture"
        );
    }
}

#[test]
fn backup_restores_curate_presets_folder_assignments_and_library_catalog_state() {
    use cuerithm_library::*;
    let (d, lib, db) = fixture();
    let incoming = d.path().canonicalize().unwrap().join("incoming");
    fs::create_dir(&incoming).unwrap();
    let source = incoming.join("authored.flac");
    fs::write(&source, b"opaque authored local curation fixture").unwrap();
    insert_source(
        &lib,
        &source,
        &cuerithm_media::file_sha256(&source).unwrap(),
    );
    let service = CurationService::new(lib.clone(), None);
    service
        .update(CurationUpdateRequest {
            request_id: "authored-local-curation".into(),
            taxonomy_revision: service.catalog().unwrap().revision,
            tracks: vec![CurationExpectedRevision {
                track_id: "authored".into(),
                revision: 0,
            }],
            patch: CurationPatch {
                notes: CurationValueEdit::Set {
                    value: "warm opener".into(),
                },
                user_rating: CurationValueEdit::Set { value: 4 },
                ..Default::default()
            },
        })
        .unwrap();
    service
        .flush_pending(10, &cuerithm_media::CancellationToken::default())
        .unwrap();
    let mut settings = lib.settings().unwrap().settings;
    settings.watch_folders = vec![incoming.clone()];
    let projection = lib.project_preparation_preset(&settings).unwrap();
    let preset = lib
        .save_preparation_preset(PreparationPresetSave {
            id: None,
            expected_revision: None,
            name: "Warmup".into(),
            rules: projection.rules,
            connections: projection.connections,
        })
        .unwrap();
    let selected = lib.save_settings(settings.clone()).unwrap();
    let folders = lib
        .preparation_preset_folders_for_settings(&settings)
        .unwrap();
    lib.save_settings_with_preparation_assignments(
        settings,
        &selected,
        &[PreparationFolderChange {
            folder_id: folders[0].folder_id.clone(),
            expected_revision: None,
            choice: PreparationFolderChoice::Preset {
                preset_id: preset.id.clone(),
            },
        }],
    )
    .unwrap();
    let capture = lib.current_preparation_snapshot_for_path(&source).unwrap();
    assert_eq!(
        capture
            .preset_capture
            .as_ref()
            .unwrap()
            .preset_id
            .as_deref(),
        Some(preset.id.as_str())
    );
    let conn = Connection::open(&db).unwrap();
    conn.execute("INSERT INTO catalog_rate_state(service,manual_hold,not_before_ms) VALUES('musicbrainz',1,12345)", []).unwrap();
    conn.execute("INSERT INTO catalog_response_cache(key,body,expires_ms,used_ms) VALUES(?1,X'7B7D',99999,123)", ["a".repeat(64)]).unwrap();
    drop(conn);
    let backup = lib
        .create_library_backup(&d.path().join("backups"))
        .unwrap();
    assert_eq!(backup.schema, 31);
    lib.save_preparation_preset(PreparationPresetSave {
        id: Some(preset.id.clone()),
        expected_revision: Some(preset.revision.clone()),
        name: "Later rules".into(),
        rules: preset.rules.clone(),
        connections: preset.connections.clone(),
    })
    .unwrap();
    service
        .update(CurationUpdateRequest {
            request_id: "later-local-curation".into(),
            taxonomy_revision: service.catalog().unwrap().revision,
            tracks: vec![CurationExpectedRevision {
                track_id: "authored".into(),
                revision: 1,
            }],
            patch: CurationPatch {
                notes: CurationValueEdit::Set {
                    value: "later note".into(),
                },
                ..Default::default()
            },
        })
        .unwrap();
    service
        .flush_pending(10, &cuerithm_media::CancellationToken::default())
        .unwrap();
    Connection::open(&db)
        .unwrap()
        .execute_batch("DELETE FROM catalog_rate_state; DELETE FROM catalog_response_cache;")
        .unwrap();
    lib.stage_library_restore(&backup.path, &backup.database_sha256)
        .unwrap();
    drop(service);
    drop(lib);
    assert!(matches!(
        apply_pending_library_restore(&db).unwrap(),
        LibraryRestoreStartup::Applied { .. }
    ));
    let lib = Library::open(&db).unwrap();
    assert_eq!(
        lib.preparation_preset(&preset.id).unwrap().revision,
        preset.revision
    );
    assert_eq!(
        lib.current_preparation_snapshot_for_path(&source)
            .unwrap()
            .revision,
        capture.revision
    );
    let service = CurationService::new(lib.clone(), None);
    let track = &service.snapshot(&["authored".into()]).unwrap().tracks[0];
    assert_eq!(track.committed.notes, "warm opener");
    assert_eq!(track.committed.user_rating, Some(4));
    assert_eq!(service.history_page(0, 10).unwrap().total, 1);
    let conn = Connection::open(&db).unwrap();
    assert!(
        conn.query_row(
            "SELECT manual_hold FROM catalog_rate_state WHERE service='musicbrainz'",
            [],
            |r| r.get::<_, bool>(0)
        )
        .unwrap()
    );
    assert_eq!(
        conn.query_row("SELECT body FROM catalog_response_cache", [], |r| r
            .get::<_, Vec<u8>>(0))
            .unwrap(),
        b"{}"
    );
    assert_eq!(
        fs::read(&source).unwrap(),
        b"opaque authored local curation fixture"
    );
}

#[test]
fn preparation_folder_replacement_invalidates_backup_without_reconnecting_assignment() {
    let (d, lib, _) = fixture();
    let folder = d.path().canonicalize().unwrap().join("incoming");
    fs::create_dir(&folder).unwrap();
    let mut settings = lib.settings().unwrap().settings;
    settings.watch_folders = vec![folder.clone()];
    lib.save_settings(settings.clone()).unwrap();
    lib.preparation_preset_folders_for_settings(&settings)
        .unwrap();
    let backup = lib
        .create_library_backup(&d.path().join("backups"))
        .unwrap();
    fs::rename(&folder, folder.with_extension("retained")).unwrap();
    fs::create_dir(&folder).unwrap();
    assert!(lib.inspect_library_backup(&backup.path).is_err());
    assert!(
        lib.stage_library_restore(&backup.path, &backup.database_sha256)
            .is_err()
    );
    let folders = lib
        .preparation_preset_folders_for_settings(&settings)
        .unwrap();
    assert_ne!(folders[0].status, "ready");
}

#[test]
fn direct_library_open_refuses_interrupted_restore_mutation() {
    let (d, lib, db) = fixture();
    let backup = lib
        .create_library_backup(&d.path().join("backups"))
        .unwrap();
    lib.stage_library_restore(&backup.path, &backup.database_sha256)
        .unwrap();
    let pending = lib.data_dir().join("library-backups/pending.json");
    let mut intent: serde_json::Value =
        serde_json::from_slice(&fs::read(&pending).unwrap()).unwrap();
    intent["mutation_started"] = true.into();
    fs::write(&pending, serde_json::to_vec(&intent).unwrap()).unwrap();
    drop(lib);
    let error = Library::open(&db)
        .err()
        .expect("all direct callers must stop for restore recovery");
    assert!(
        error
            .to_string()
            .contains("restore stopped during database update")
    );
}
#[test]
fn installation_backups_survive_generation_activation_and_restore() {
    use cuerithm_library::LibraryGenerations;
    let temp = tempfile::tempdir().unwrap();
    let manager = LibraryGenerations::new(&temp.path().join("app")).unwrap();
    let legacy = manager.open_active().unwrap();
    std::fs::create_dir_all(legacy.data_dir().join("cleanup")).unwrap();
    std::fs::write(legacy.data_dir().join("cleanup/legacy-resource"), b"legacy").unwrap();
    let old = legacy
        .create_library_backup(&temp.path().join("backups"))
        .unwrap();
    // Legacy manifests predate the explicit data-directory field.
    let manifest_path = old.path.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest.as_object_mut().unwrap().remove("data_directory");
    std::fs::write(manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let identity = std::fs::read(manager.root().join(".library-installation-id")).unwrap();
    drop(legacy);
    let stage = manager.create_staging().unwrap();
    drop(Library::open(&stage.database).unwrap());
    let seal = manager.seal(&stage.id).unwrap();
    manager.schedule(&stage.id, "legacy", &seal).unwrap();
    let legacy = Library::open(&manager.database_path("legacy").unwrap()).unwrap();
    assert!(
        legacy
            .stage_library_restore(&old.path, &old.database_sha256)
            .is_err()
    );
    drop(legacy);
    let active = manager.open_active().unwrap();
    assert_ne!(active.data_dir(), manager.root());
    active.inspect_library_backup(&old.path).unwrap();
    std::fs::create_dir_all(active.data_dir().join("cleanup")).unwrap();
    std::fs::write(
        active.data_dir().join("cleanup/generation-resource"),
        b"generation",
    )
    .unwrap();
    let new = active
        .create_library_backup(&temp.path().join("backups"))
        .unwrap();
    assert!(!active.data_dir().join(".library-installation-id").exists());
    active
        .stage_library_restore(&old.path, &old.database_sha256)
        .unwrap();
    assert!(
        manager
            .schedule("legacy", &stage.id, "unused")
            .unwrap_err()
            .to_string()
            .contains("scheduled library backup restore")
    );
    assert!(manager.root().join("library-backups/pending.json").exists());
    assert!(
        !active
            .data_dir()
            .join("library-backups/pending.json")
            .exists()
    );
    drop(active);
    let restored = manager.open_active().unwrap();
    assert_eq!(manager.active_id().unwrap(), "legacy");
    restored.inspect_library_backup(&old.path).unwrap();
    restored.inspect_library_backup(&new.path).unwrap();
    assert_eq!(
        std::fs::read(manager.root().join(".library-installation-id")).unwrap(),
        identity
    );
    restored
        .stage_library_restore(&new.path, &new.database_sha256)
        .unwrap();
    drop(restored);
    let returned = manager.open_active().unwrap();
    assert_eq!(manager.active_id().unwrap(), stage.id);
    returned.inspect_library_backup(&old.path).unwrap();
    returned.inspect_library_backup(&new.path).unwrap();
    assert_eq!(
        std::fs::read(returned.data_dir().join("cleanup/generation-resource")).unwrap(),
        b"generation"
    );
    assert_eq!(
        std::fs::read(manager.root().join("cleanup/legacy-resource")).unwrap(),
        b"legacy"
    );
}
