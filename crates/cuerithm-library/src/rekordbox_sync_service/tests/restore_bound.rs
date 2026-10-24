//! Only the newest ten syncs of a target keep a backup to restore from.
use super::*;

fn applied() -> Fixture {
    let f = fixture(true);
    publication::fixture_apply(
        &f.library,
        &f.id,
        &key(),
        &CancellationToken::default(),
        &|| Ok(()),
    )
    .unwrap();
    f
}
/// Saves a copy of the applied sync under a new identity, with its own folder of backup files.
fn copy_of(f: &Fixture, status: &str, created: i64) -> String {
    let mut op = load(&f.library, &f.id).unwrap();
    op.id = uuid::Uuid::new_v4().to_string();
    op.status = status.into();
    op.created = created;
    op.updated = created;
    let workspace = root(&f.library).join("operations").join(&op.id);
    fs::create_dir_all(&workspace).unwrap();
    fs::write(workspace.join("before.db"), vec![7u8; 4096]).unwrap();
    save(&f.library.conn().unwrap(), &op).unwrap();
    op.id
}
fn workspace(f: &Fixture, id: &str) -> PathBuf {
    root(&f.library).join("operations").join(id)
}
fn journal(f: &Fixture, id: &str) -> Value {
    let text: String = f
        .library
        .conn()
        .unwrap()
        .query_row(
            "SELECT journal_json FROM rekordbox_sync_operations WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .unwrap();
    serde_json::from_str(&text).unwrap()
}

#[test]
fn syncs_older_than_the_newest_ten_give_up_their_backup_but_stay_in_the_history() {
    let f = applied();
    let recent: Vec<String> = (0..10).map(|n| copy_of(&f, "applied", 100 + n)).collect();
    // Older than all of them: the one that was applied, and one in each other state.
    let prepared = copy_of(&f, "prepared", 0);
    let published = copy_of(&f, "native_published", 0);
    let freed = retention::prune_locked(&f.library, retention::RESTORE_LIMIT).unwrap();
    assert!(freed > 0);
    assert!(
        !workspace(&f, &f.id).exists(),
        "the eleventh newest is expired"
    );
    for id in &recent {
        assert!(workspace(&f, id).join("before.db").is_file());
        assert!(journal(&f, id).get("ledger_before").is_some());
    }
    // A sync that is still prepared or half published is never touched, whatever its age.
    for id in [&prepared, &published] {
        assert!(workspace(&f, id).join("before.db").is_file());
        assert!(journal(&f, id).get("ledger_before").is_some());
    }

    // The expired one is a summary: still listed, with its outcome, and no longer restorable.
    let summary = journal(&f, &f.id);
    assert_eq!(summary["restore_expired"], true);
    assert_eq!(summary["status"], "applied");
    assert!(summary.get("ledger_before").is_none() && summary.get("plan").is_none());
    let history = f.library.history_rekordbox_sync().unwrap();
    let entry = history.iter().find(|e| e.id == f.id).unwrap();
    assert!(!entry.restore_available && entry.outcome.is_some() && entry.status == "applied");
    assert!(
        history
            .iter()
            .find(|e| e.id == recent[0])
            .unwrap()
            .restore_available
    );
    let error = load(&f.library, &f.id).unwrap_err();
    assert!(
        format!("{error:#}").contains("older than the 10"),
        "{error:#}"
    );

    // Doing it again changes nothing, and the library upgrade can still read the summary.
    assert_eq!(
        retention::prune_locked(&f.library, retention::RESTORE_LIMIT).unwrap(),
        0
    );
    ledger::upgrade_operations(&f.library.conn().unwrap()).unwrap();
    assert_eq!(journal(&f, &f.id)["restore_expired"], true);
}

#[test]
fn a_restored_sync_gives_up_its_backup_at_once_and_a_target_counts_its_own_syncs() {
    let f = applied();
    let restored = copy_of(&f, "restored", 500);
    // Another target's newest syncs do not push this target's sync out of its ten.
    let other: Vec<String> = (0..10)
        .map(|n| {
            let id = copy_of(&f, "applied", 100 + n);
            f.library
                .conn()
                .unwrap()
                .execute(
                    "UPDATE rekordbox_sync_operations SET target_id='another-target' WHERE id=?1",
                    [&id],
                )
                .unwrap();
            id
        })
        .collect();
    retention::prune_locked(&f.library, retention::RESTORE_LIMIT).unwrap();
    assert!(!workspace(&f, &restored).exists());
    assert_eq!(journal(&f, &restored)["restore_expired"], true);
    assert!(workspace(&f, &f.id).exists());
    assert!(journal(&f, &f.id).get("ledger_before").is_some());
    assert!(other.iter().all(|id| workspace(&f, id).exists()));
}

#[test]
fn opening_the_library_applies_the_bound_to_a_long_history() {
    let f = applied();
    let recent: Vec<String> = (0..10).map(|n| copy_of(&f, "applied", 100 + n)).collect();
    let path = f._dir.path().canonicalize().unwrap().join("library.db");
    let Fixture {
        library, _dir, id, ..
    } = f;
    drop(library);
    let library = Library::open(&path).unwrap();
    let root = root(&library).join("operations");
    assert!(!root.join(&id).exists());
    assert!(recent.iter().all(|id| root.join(id).exists()));
    drop(_dir);
}
