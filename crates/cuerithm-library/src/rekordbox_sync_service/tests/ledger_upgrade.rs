//! A sync made before the library kept lean copies of the DJ database can still be restored
//! after the upgrade, and its saved ledger no longer carries those copies.
use super::*;

/// Gives every record and observation the full copy the earlier version kept.
fn store_full_copies(connection: &Connection) {
    connection
        .execute_batch(
            "UPDATE native_records SET json='{\"full copy of\":\"'||table_name||':'||record_id||'\",\"pad\":\"'||replace(hex(zeroblob(1000)),'0','a')||'\"}';
             UPDATE native_track_links SET observation_json='{\"full observation of\":\"'||content_id||'\",\"pad\":\"'||replace(hex(zeroblob(1000)),'0','a')||'\"}' WHERE observation_hash IS NOT NULL;",
        )
        .unwrap();
}
fn operation_hash(library: &Library, id: &str) -> Option<String> {
    load(library, id).unwrap().ledger_after_hash
}
fn lean_digest(library: &Library, target: &str) -> String {
    digest(&ledger::capture(&library.conn().unwrap(), target).unwrap()).unwrap()
}

/// An applied sync in the form the earlier version saved it: a full ledger before, and a digest
/// of the full rows after.
fn applied_before_the_upgrade() -> (Fixture, String, usize) {
    let mut f = fixture(true);
    let target = load(&f.library, &f.id).unwrap().native_target_id;
    let mut op = load(&f.library, &f.id).unwrap();
    let connection = f.library.conn().unwrap();
    store_full_copies(&connection);
    op.ledger_before = ledger::capture_as_before_schema_30(&connection, &target).unwrap();
    save(&connection, &op).unwrap();
    drop(connection);
    publication::fixture_apply(
        &f.library,
        &f.id,
        &key(),
        &CancellationToken::default(),
        &|| Ok(()),
    )
    .unwrap();
    let mut op = load(&f.library, &f.id).unwrap();
    let connection = f.library.conn().unwrap();
    store_full_copies(&connection);
    op.ledger_after_hash =
        Some(digest(&ledger::capture_as_before_schema_30(&connection, &target).unwrap()).unwrap());
    save(&connection, &op).unwrap();
    let full_size = serde_json::to_string(&op.ledger_before).unwrap().len();
    // Make it look like a schema 29 library.
    connection.pragma_update(None, "user_version", 30).unwrap();
    drop(connection);
    let original = f._dir.path().canonicalize().unwrap().join("library.db");
    drop(f.library);
    f.library = Library::open(&original).unwrap();
    (f, target, full_size)
}

#[test]
fn a_sync_made_before_the_upgrade_can_still_be_restored_and_its_ledger_is_lean() {
    let (f, target, full_size) = applied_before_the_upgrade();
    let op = load(&f.library, &f.id).unwrap();
    // The upgrade ran when the library was opened again.
    let connection = f.library.conn().unwrap();
    assert_eq!(
        connection
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        31
    );
    drop(connection);
    let lean_size = serde_json::to_string(&op.ledger_before).unwrap().len();
    assert!(
        lean_size * 3 < full_size,
        "the ledger shrank from {full_size} to {lean_size}"
    );
    assert!(
        !serde_json::to_string(&op.ledger_before)
            .unwrap()
            .contains("full copy of")
    );
    assert_eq!(
        operation_hash(&f.library, &f.id),
        Some(lean_digest(&f.library, &target))
    );
    let restored = publication::fixture_restore(
        &f.library,
        &f.id,
        &key(),
        &CancellationToken::default(),
        &|| Ok(()),
    )
    .unwrap();
    assert_eq!(restored.status, "restored");
    assert_eq!(title(&f), "Before");
}

#[test]
fn a_sync_whose_ledger_changed_before_the_upgrade_is_still_refused_after_it() {
    let mut f = fixture(true);
    let target = load(&f.library, &f.id).unwrap().native_target_id;
    publication::fixture_apply(
        &f.library,
        &f.id,
        &key(),
        &CancellationToken::default(),
        &|| Ok(()),
    )
    .unwrap();
    let mut op = load(&f.library, &f.id).unwrap();
    let connection = f.library.conn().unwrap();
    store_full_copies(&connection);
    op.ledger_after_hash =
        Some(digest(&ledger::capture_as_before_schema_30(&connection, &target).unwrap()).unwrap());
    save(&connection, &op).unwrap();
    // A later observation changed a record after the sync.
    connection
        .execute(
            "UPDATE native_records SET hash='changed later' WHERE record_id='1' AND table_name='djmdContent'",
            [],
        )
        .unwrap();
    connection.pragma_update(None, "user_version", 30).unwrap();
    drop(connection);
    let original = f._dir.path().canonicalize().unwrap().join("library.db");
    drop(f.library);
    f.library = Library::open(&original).unwrap();
    let error = publication::fixture_restore(
        &f.library,
        &f.id,
        &key(),
        &CancellationToken::default(),
        &|| Ok(()),
    )
    .unwrap_err();
    assert!(
        format!("{error:#}").contains("ownership changed after publication"),
        "{error:#}"
    );
    assert_eq!(title(&f), "After");
}

#[test]
fn a_sync_that_was_restored_keeps_no_ledger_after_the_upgrade() {
    let (f, _, _) = applied_before_the_upgrade();
    publication::fixture_restore(
        &f.library,
        &f.id,
        &key(),
        &CancellationToken::default(),
        &|| Ok(()),
    )
    .unwrap();
    // The upgrade drops the ledger of a sync that can never be restored again.
    ledger::upgrade_operations(&f.library.conn().unwrap()).unwrap();
    let op = load(&f.library, &f.id).unwrap();
    assert_eq!(op.status, "restored");
    assert!(op.ledger_before.is_empty());
    // And the library then gives up the rest of its backup: only a summary stays.
    let connection = f.library.conn().unwrap();
    connection.pragma_update(None, "user_version", 30).unwrap();
    drop(connection);
    let mut f = f;
    let original = f._dir.path().canonicalize().unwrap().join("library.db");
    drop(f.library);
    f.library = Library::open(&original).unwrap();
    let text: String = f
        .library
        .conn()
        .unwrap()
        .query_row(
            "SELECT journal_json FROM rekordbox_sync_operations WHERE id=?1",
            [&f.id],
            |r| r.get(0),
        )
        .unwrap();
    let summary: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(summary["status"], "restored");
    assert_eq!(summary["restore_expired"], true);
    assert!(summary.get("ledger_before").is_none());
}

#[test]
fn the_ledger_digest_does_not_depend_on_whether_the_copies_are_full() {
    let f = fixture(true);
    let target = load(&f.library, &f.id).unwrap().native_target_id;
    let lean = lean_digest(&f.library, &target);
    store_full_copies(&f.library.conn().unwrap());
    assert_eq!(lean_digest(&f.library, &target), lean);
    let full =
        digest(&ledger::capture_as_before_schema_30(&f.library.conn().unwrap(), &target).unwrap())
            .unwrap();
    assert_ne!(full, lean, "the earlier digest describes the full rows");
}
