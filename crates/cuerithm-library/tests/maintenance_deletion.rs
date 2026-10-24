//! "Delete from disk" moves only verified duplicate audio files to the Trash, removes only the
//! records of files that really moved, and can be undone for the library records. The Trash here
//! is a folder in the test's temporary directory; no real Trash or music folder is involved.
#![cfg(feature = "autotag")]
#[path = "support/duplicates_fixture.rs"]
mod fixture;

use cuerithm_library::{
    DuplicateIdentity, DuplicateMergePreview, DuplicateMergeService, DuplicateReview,
    DuplicateReviewEdit, DuplicateSkipReason, DuplicateTolerance as Tolerance, FileTrash,
    GroupEdit, MergeDisposition, MetadataEditService,
};
use cuerithm_media::CancellationToken;
use fixture::*;
use std::sync::Arc;

fn token() -> CancellationToken {
    CancellationToken::default()
}
fn trash(f: &Fixture) -> Arc<FakeTrash> {
    Arc::new(FakeTrash::new(f.dir.path().join("fake-trash")))
}
fn service(f: &Fixture, trash: &Arc<FakeTrash>) -> DuplicateMergeService {
    DuplicateMergeService::with_trash(f.lib.clone(), f.runtime.clone(), trash.clone())
}
fn preview(
    f: &Fixture,
    review: &DuplicateReview,
    group: usize,
    choice: MergeDisposition,
) -> DuplicateMergePreview {
    preview_with(f, review, group, choice, &Capable)
}
fn preview_with(
    f: &Fixture,
    review: &DuplicateReview,
    group: usize,
    choice: MergeDisposition,
    trash: &dyn FileTrash,
) -> DuplicateMergePreview {
    f.lib
        .preview_duplicate_merge_choice_with(
            &review.id,
            review.revision,
            vec![review.groups[group].id.clone()],
            Some(choice),
            trash,
            &token(),
        )
        .unwrap()
}
fn hashes(f: &Fixture) -> Vec<String> {
    f.paths.iter().map(|p| hash(p)).collect()
}

#[test]
fn delete_from_disk_moves_the_verified_duplicate_removes_its_record_and_undo_restores_the_records()
{
    let f = Fixture::new();
    f.playlist(
        "set",
        vec![f.ids[0].clone(), f.ids[2].clone(), f.ids[1].clone()],
    );
    // The duplicate also carries a custom tag, a sync link and a rekordbox link.
    let conn = rusqlite::Connection::open(&f.db).unwrap();
    conn.execute_batch("INSERT INTO custom_tag_categories VALUES('mood','Mood'); INSERT INTO custom_tags VALUES('warm','mood','Warm'); INSERT INTO native_targets VALUES('desk','rekordbox','db','device','{}','fingerprint','observation','[]',1,1,1);").unwrap();
    conn.execute(
        "INSERT INTO track_custom_tags VALUES(?1,'warm')",
        [&f.ids[1]],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO sync_links VALUES(?1,'target','external-1','{}')",
        [&f.ids[1]],
    )
    .unwrap();
    conn.execute("INSERT INTO native_track_links VALUES('desk','content-1',?1,'{}',NULL,NULL,'{}','linked',1)", [&f.ids[1]]).unwrap();
    drop(conn);
    let before = f.content();
    // The library records canonical paths.
    let canonical = |i: usize| {
        f.paths[i]
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .into_owned()
    };
    let (duplicate_path, keeper_path) = (canonical(1), canonical(0));
    let sizes = std::fs::metadata(&f.paths[1]).unwrap().len();
    let untouched = [0, 2, 3]
        .into_iter()
        .map(|i| (i, hash(&f.paths[i])))
        .collect::<Vec<_>>();
    let review = f.review(Tolerance::None);
    let preview = preview(&f, &review, 0, MergeDisposition::Delete);

    // The preview lists exactly what will happen.
    assert_eq!(preview.plan.disposition, MergeDisposition::Delete);
    let plan = preview.deletion.as_ref().unwrap();
    assert_eq!(plan.files.len(), 1);
    let file = &plan.files[0];
    assert_eq!(file.track_id, f.ids[1]);
    assert_eq!(file.keeper_id, f.ids[0]);
    assert_eq!(file.size_bytes, sizes);
    assert_eq!(file.display_path, duplicate_path);
    assert_eq!(file.keeper_display_path, keeper_path);
    assert_eq!(file.identity, DuplicateIdentity::SameAudio);
    assert!(plan.skipped.is_empty());
    assert_eq!(plan.undo_limit, 10);
    assert_eq!(plan.impact.len(), 1);
    assert_eq!(plan.impact[0].track_id, f.ids[1]);
    assert!(
        plan.impact[0].other_records >= 1,
        "the analysis job leaves with it"
    );
    assert_eq!(preview.plan.groups[0].delete_ids, vec![f.ids[1].clone()]);
    assert!(preview.plan.groups[0].archive_ids.is_empty());
    // The kept track takes the removed record's place in the playlist.
    assert_eq!(
        preview.playlists[0].after,
        vec![f.ids[2].clone(), f.ids[0].clone()]
    );

    let stand_in = trash(&f);
    let service = service(&f, &stand_in);
    // Deleting needs a separate, explicit confirmation.
    assert!(service.apply(&preview.id, &token()).is_err());
    assert_eq!(f.content(), before);
    assert!(stand_in.taken().is_empty());

    let result = service
        .apply_confirmed(&preview.id, true, &token())
        .unwrap();
    assert_eq!(result.removed_ids, vec![f.ids[1].clone()]);
    assert!(result.archived_ids.is_empty());
    assert_eq!(result.trashed, vec![file.clone()]);
    assert!(result.not_deleted.is_empty());
    assert_eq!(stand_in.taken(), vec!["track-1.wav"]);
    assert!(!f.paths[1].exists(), "the duplicate went to the Trash");
    assert!(
        !f.track_exists(&f.ids[1]),
        "its record is gone, not archived"
    );
    assert!(f.track_exists(&f.ids[0]));
    assert!(!f.archived(&f.ids[0]));
    for (i, expected) in &untouched {
        assert_eq!(&hash(&f.paths[*i]), expected, "file {i} must be untouched");
    }
    assert_eq!(
        f.lib.manual_playlist_members("set").unwrap(),
        vec![f.ids[2].clone(), f.ids[0].clone()]
    );
    let history = f.lib.duplicate_merge_history().unwrap();
    assert_eq!(history[0].result.id, result.id);
    assert_eq!(history[0].result.removed_ids, vec![f.ids[1].clone()]);
    assert!(!history[0].undo_expired);
    assert_eq!(history[0].undo_limit, 10);
    // Nothing else records the merge as a pending recovery.
    assert!(f.lib.duplicate_recovery().unwrap().is_empty());

    // Undo brings back the records, every row that belonged to them and the playlist
    // position. The file is wherever the user keeps it (here: still in the stand-in Trash).
    let undone = service.undo(&result.id, &token()).unwrap();
    assert!(undone.restored_ids.contains(&f.ids[1]));
    assert!(undone.removed_ids.is_empty());
    assert_eq!(f.content(), before);
    assert_eq!(
        f.lib.manual_playlist_members("set").unwrap(),
        vec![f.ids[0].clone(), f.ids[2].clone(), f.ids[1].clone()]
    );
    assert!(
        !f.paths[1].exists(),
        "Undo does not take a file out of the Trash"
    );
    assert!(service.undo(&result.id, &token()).is_err());
    assert_eq!(
        f.lib.duplicate_merge_history().unwrap()[0]
            .undone_by
            .as_deref(),
        Some(undone.id.as_str())
    );
}

#[test]
fn nothing_can_be_previewed_without_a_choice_and_archive_only_never_touches_audio_files() {
    let f = Fixture::new();
    let review = f.review(Tolerance::None);
    let error = f
        .lib
        .preview_duplicate_merge_choice(
            &review.id,
            review.revision,
            vec![review.groups[0].id.clone()],
            None,
            &token(),
        )
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("Choose Delete from disk or Archive only"),
        "{error}"
    );

    let before = f.content();
    let files = hashes(&f);
    let preview = preview(&f, &review, 0, MergeDisposition::Archive);
    assert!(preview.deletion.is_none());
    assert_eq!(preview.plan.groups[0].archive_ids, vec![f.ids[1].clone()]);
    assert!(preview.plan.groups[0].delete_ids.is_empty());
    let stand_in = trash(&f);
    let service = service(&f, &stand_in);
    let result = service.apply(&preview.id, &token()).unwrap();
    assert_eq!(result.archived_ids, vec![f.ids[1].clone()]);
    assert!(result.removed_ids.is_empty());
    assert!(f.archived(&f.ids[1]));
    assert_eq!(hashes(&f), files, "archiving never touches a file");
    assert!(stand_in.taken().is_empty());
    service.undo(&result.id, &token()).unwrap();
    assert_eq!(f.content(), before);
}

#[test]
fn only_audio_proven_identical_to_the_kept_track_can_be_deleted() {
    let mut f = Fixture::new();
    // Same artist and title, different audio: a candidate by name only.
    let other = f.add_track("other-mix.wav", 71);
    f.playlist("mixes", vec![f.ids[2].clone(), f.ids[other].clone()]);
    let review = f.review(Tolerance::None);
    let group = review
        .groups
        .iter()
        .position(|g| g.tracks.iter().any(|t| t.candidate.id == f.ids[other]))
        .unwrap();
    assert!(!review.groups[group].exact_audio);
    let before = f.content();
    let files = hashes(&f);
    let preview = preview(&f, &review, group, MergeDisposition::Delete);
    let plan = preview.deletion.as_ref().unwrap();
    assert!(plan.files.is_empty());
    assert_eq!(plan.skipped.len(), 1);
    assert_eq!(
        plan.skipped[0].reason,
        DuplicateSkipReason::NotVerifiedIdentical
    );
    assert!(preview.plan.groups[0].delete_ids.is_empty());
    assert!(
        preview.playlists.is_empty(),
        "a skipped record is not replaced anywhere"
    );

    let stand_in = trash(&f);
    let result = service(&f, &stand_in)
        .apply_confirmed(&preview.id, true, &token())
        .unwrap();
    assert!(result.removed_ids.is_empty());
    assert_eq!(result.not_deleted.len(), 1);
    assert!(stand_in.taken().is_empty());
    assert_eq!(hashes(&f), files);
    assert_eq!(
        f.content(),
        before,
        "a skipped file changes nothing for its record"
    );
}

#[test]
fn the_kept_tracks_own_file_is_never_deleted_even_under_another_record() {
    let mut f = Fixture::new();
    // A second record for the same physical file as track 0.
    let alias = f.dir.path().join("alias.wav");
    std::fs::hard_link(&f.paths[0], &alias).unwrap();
    let alias_index = f.import(alias.clone());
    let review = f.review(Tolerance::None);
    assert_eq!(review.groups[0].tracks.len(), 3);
    let preview = preview(&f, &review, 0, MergeDisposition::Delete);
    let plan = preview.deletion.as_ref().unwrap();
    let keeper = preview.plan.groups[0].keeper.clone();
    // Whichever record is kept, the other name for the same file is left alone.
    let same = plan
        .skipped
        .iter()
        .filter(|s| s.reason == DuplicateSkipReason::SameFileAsKeeper)
        .collect::<Vec<_>>();
    assert_eq!(same.len(), 1, "{plan:?}");
    assert!(
        [f.ids[0].clone(), f.ids[alias_index].clone()].contains(&same[0].track_id)
            && same[0].track_id != keeper
    );
    assert_eq!(plan.files.len(), 1);
    assert_eq!(plan.files[0].track_id, f.ids[1]);

    let stand_in = trash(&f);
    let result = service(&f, &stand_in)
        .apply_confirmed(&preview.id, true, &token())
        .unwrap();
    assert_eq!(result.removed_ids, vec![f.ids[1].clone()]);
    assert_eq!(stand_in.taken(), vec!["track-1.wav"]);
    assert!(f.paths[0].exists() && alias.exists());
    assert!(f.track_exists(&f.ids[0]) && f.track_exists(&f.ids[alias_index]));
}

#[test]
fn a_file_another_remaining_record_still_uses_is_not_deleted() {
    let mut f = Fixture::new();
    // The library also lists the duplicate's file under another name. That record is
    // ignored for duplicates, so it is not part of the merge and stays in the library.
    let alias = f.dir.path().join("also-track-1.wav");
    std::fs::hard_link(&f.paths[1], &alias).unwrap();
    let alias_index = f.import(alias);
    let stamp = f.lib.duplicate_ignores().unwrap().stamp;
    f.lib
        .set_duplicate_ignores(vec![f.ids[alias_index].clone()], true, stamp)
        .unwrap();
    let review = f.review(Tolerance::None);
    assert_eq!(review.groups.len(), 1);
    let before = f.content();
    let preview = preview(&f, &review, 0, MergeDisposition::Delete);
    let plan = preview.deletion.as_ref().unwrap();
    assert!(plan.files.is_empty(), "{plan:?}");
    assert_eq!(plan.skipped.len(), 1);
    assert_eq!(
        plan.skipped[0].reason,
        DuplicateSkipReason::SharedWithRemainingRecord
    );
    assert_eq!(plan.skipped[0].track_id, f.ids[1]);
    let stand_in = trash(&f);
    let result = service(&f, &stand_in)
        .apply_confirmed(&preview.id, true, &token())
        .unwrap();
    assert!(result.removed_ids.is_empty());
    assert!(f.paths[1].exists());
    assert!(stand_in.taken().is_empty());
    assert_eq!(f.content(), before);
}

/// Rewrites the shown text of one path wherever a saved scan or review holds it, the way a lossy
/// conversion of a name that is not valid text would, and leaves the raw proofs alone.
fn garble_shown_path(f: &Fixture, shown: &str) {
    fn walk(value: &mut serde_json::Value, shown: &str) {
        match value {
            serde_json::Value::String(text) if text == shown => {
                *text = shown.replace(".wav", "\u{FFFD}.wav");
            }
            serde_json::Value::Array(items) => items.iter_mut().for_each(|v| walk(v, shown)),
            serde_json::Value::Object(map) => map.values_mut().for_each(|v| walk(v, shown)),
            _ => {}
        }
    }
    let conn = rusqlite::Connection::open(&f.db).unwrap();
    let saved: Vec<(String, String)> = conn
        .prepare("SELECT id,payload_json FROM maintenance_scans WHERE kind IN ('duplicates','duplicate_review')")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(!saved.is_empty());
    for (id, text) in saved {
        let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
        walk(&mut value, shown);
        conn.execute(
            "UPDATE maintenance_scans SET payload_json=?1 WHERE id=?2",
            rusqlite::params![value.to_string(), id],
        )
        .unwrap();
    }
}

#[test]
fn a_file_whose_shown_path_differs_from_its_raw_path_can_still_be_deleted() {
    // A file name that is not valid text is shown with replacement characters, so the shown
    // path can never be turned back into the file's real path. The scan's raw proof is what
    // the deletion check compares.
    let f = Fixture::new();
    let review = f.review(Tolerance::None);
    let shown = f.paths[1]
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    garble_shown_path(&f, &shown);
    let preview = preview(&f, &review, 0, MergeDisposition::Delete);
    let plan = preview.deletion.as_ref().unwrap();
    assert!(plan.skipped.is_empty(), "{:?}", plan.skipped);
    assert_eq!(plan.files.len(), 1);
    assert_eq!(plan.files[0].track_id, f.ids[1]);
    let stand_in = trash(&f);
    let result = service(&f, &stand_in)
        .apply_confirmed(&preview.id, true, &token())
        .unwrap();
    assert_eq!(result.removed_ids, vec![f.ids[1].clone()]);
    assert_eq!(stand_in.taken(), vec!["track-1.wav"]);
}

#[cfg(unix)]
#[test]
fn a_duplicate_with_a_file_name_that_is_not_valid_text_can_be_deleted() {
    use std::os::unix::ffi::OsStrExt;
    let mut f = Fixture::empty();
    f.add_track("keep.wav", 61);
    let odd = f
        .dir
        .path()
        .join(std::ffi::OsStr::from_bytes(b"odd-\xFF-name.wav"));
    // Some file systems (APFS on a Mac) refuse such a name; there the shown-path test above
    // covers the same comparison.
    if std::fs::File::create(&odd).is_err() {
        eprintln!("this file system refuses a name that is not valid text; skipped");
        return;
    }
    wav(&odd, 61);
    let copy = f.import(odd.clone());
    let review = f.review(Tolerance::None);
    let preview = preview(&f, &review, 0, MergeDisposition::Delete);
    let plan = preview.deletion.as_ref().unwrap();
    assert!(plan.skipped.is_empty(), "{:?}", plan.skipped);
    assert_eq!(plan.files.len(), 1);
    assert_eq!(plan.files[0].track_id, f.ids[copy]);
    let stand_in = trash(&f);
    let result = service(&f, &stand_in)
        .apply_confirmed(&preview.id, true, &token())
        .unwrap();
    assert_eq!(result.removed_ids, vec![f.ids[copy].clone()]);
    assert!(!odd.exists());
}

/// A track from a portable restore that is not linked to a local file yet: no path, no content hash.
fn add_unlinked_track(f: &mut Fixture) -> String {
    let index = f.add_track("portable.wav", 91);
    rusqlite::Connection::open(&f.db)
        .unwrap()
        .execute(
            "UPDATE tracks SET path=NULL,source_hash=NULL WHERE id=?1",
            [&f.ids[index]],
        )
        .unwrap();
    f.ids[index].clone()
}
fn linked(f: &Fixture, id: &str) -> bool {
    rows(
        f,
        &format!(
            "SELECT COUNT(*) FROM tracks WHERE id='{id}' AND path IS NULL AND source_hash IS NULL"
        ),
    ) == 0
}

#[test]
fn an_unlinked_track_does_not_stop_delete_from_disk_and_stays_untouched() {
    let mut f = Fixture::new();
    let unlinked = add_unlinked_track(&mut f);
    let review = f.review(Tolerance::None);
    assert_eq!(review.groups.len(), 1);
    let preview = preview(&f, &review, 0, MergeDisposition::Delete);
    let plan = preview.deletion.as_ref().unwrap();
    assert!(plan.skipped.is_empty(), "{:?}", plan.skipped);
    assert_eq!(plan.files.len(), 1);
    let stand_in = trash(&f);
    let service = service(&f, &stand_in);
    let result = service
        .apply_confirmed(&preview.id, true, &token())
        .unwrap();
    assert_eq!(result.removed_ids, vec![f.ids[1].clone()]);
    assert!(f.track_exists(&unlinked) && !linked(&f, &unlinked));
    // Undo brings the copy back and still leaves the unlinked track as it was.
    service.undo(&result.id, &token()).unwrap();
    assert!(f.track_exists(&f.ids[1]));
    assert!(f.track_exists(&unlinked) && !linked(&f, &unlinked));
}

#[test]
fn an_unlinked_track_does_not_stop_archive_only() {
    let mut f = Fixture::new();
    let unlinked = add_unlinked_track(&mut f);
    let review = f.review(Tolerance::None);
    let preview = preview(&f, &review, 0, MergeDisposition::Archive);
    let stand_in = trash(&f);
    let result = service(&f, &stand_in).apply(&preview.id, &token()).unwrap();
    assert_eq!(result.archived_ids, vec![f.ids[1].clone()]);
    assert!(f.paths[1].exists() && stand_in.taken().is_empty());
    assert!(f.track_exists(&unlinked) && !linked(&f, &unlinked));
}

#[test]
fn a_participant_that_lost_its_file_link_after_the_review_is_refused_in_plain_words() {
    let f = Fixture::new();
    let review = f.review(Tolerance::None);
    rusqlite::Connection::open(&f.db)
        .unwrap()
        .execute(
            "UPDATE tracks SET path=NULL,source_hash=NULL WHERE id=?1",
            [&f.ids[1]],
        )
        .unwrap();
    let error = f
        .lib
        .preview_duplicate_merge_choice_with(
            &review.id,
            review.revision,
            vec![review.groups[0].id.clone()],
            Some(MergeDisposition::Delete),
            &Capable,
            &token(),
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("not linked to a local file"), "{error}");
    assert!(f.paths[1].exists());
}

#[test]
fn a_preview_that_does_not_name_the_approved_keeper_file_deletes_nothing() {
    // A preview saved by an earlier build says nothing about the kept track's file. It cannot be
    // judged against what was approved, so its files stay.
    fn forget(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Array(items) => items.iter_mut().for_each(forget),
            serde_json::Value::Object(map) => {
                map.remove("keeper_sha256");
                map.values_mut().for_each(forget);
            }
            _ => {}
        }
    }
    let f = Fixture::new();
    let review = f.review(Tolerance::None);
    let preview = preview(&f, &review, 0, MergeDisposition::Delete);
    let conn = rusqlite::Connection::open(&f.db).unwrap();
    let text: String = conn
        .query_row(
            "SELECT payload_json FROM maintenance_scans WHERE id=?1",
            [&preview.id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(text.contains("keeper_sha256"));
    let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
    forget(&mut value);
    conn.execute(
        "UPDATE maintenance_scans SET payload_json=?1 WHERE id=?2",
        rusqlite::params![value.to_string(), preview.id],
    )
    .unwrap();
    drop(conn);
    let stand_in = trash(&f);
    let result = service(&f, &stand_in)
        .apply_confirmed(&preview.id, true, &token())
        .unwrap();
    assert!(result.removed_ids.is_empty() && result.trashed.is_empty());
    assert_eq!(result.not_deleted.len(), 1);
    assert!(stand_in.taken().is_empty() && f.paths[1].exists());
    assert!(f.track_exists(&f.ids[1]));
}

#[test]
fn portable_records_that_belong_to_a_deleted_track_leave_with_it_and_come_back_with_undo() {
    // Portable records refer to a track by a logical id inside their JSON, not by a foreign key.
    let f = Fixture::new();
    let conn = rusqlite::Connection::open(&f.db).unwrap();
    let mut expected = vec![];
    for (kind, id, track) in [
        ("tracks", f.ids[0].as_str(), f.ids[0].as_str()),
        ("tracks", f.ids[1].as_str(), f.ids[1].as_str()),
        ("external_links", "old-link", f.ids[1].as_str()),
        ("external_links", "kept-link", f.ids[0].as_str()),
    ] {
        let json = serde_json::json!({"id":id,"value":{"track_id":track}}).to_string();
        conn.execute(
            "INSERT INTO portable_domain_records VALUES(?1,?2,?3)",
            rusqlite::params![kind, id, json],
        )
        .unwrap();
        expected.push((kind.to_owned(), id.to_owned(), json));
    }
    drop(conn);
    let all = |f: &Fixture| -> Vec<(String, String, String)> {
        let conn = rusqlite::Connection::open(&f.db).unwrap();
        let mut q = conn
            .prepare("SELECT kind,id,json FROM portable_domain_records ORDER BY kind,id")
            .unwrap();
        q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    expected.sort();
    assert_eq!(all(&f), expected);
    let review = f.review(Tolerance::None);
    let preview = preview(&f, &review, 0, MergeDisposition::Delete);
    let stand_in = trash(&f);
    let service = service(&f, &stand_in);
    let result = service
        .apply_confirmed(&preview.id, true, &token())
        .unwrap();
    assert_eq!(result.removed_ids, vec![f.ids[1].clone()]);
    let kept: Vec<_> = expected
        .iter()
        .filter(|(_, id, _)| id == &f.ids[0] || id == "kept-link")
        .cloned()
        .collect();
    assert_eq!(
        all(&f),
        kept,
        "the deleted track's records left with it, the keeper's stayed"
    );
    service.undo(&result.id, &token()).unwrap();
    assert_eq!(
        all(&f),
        expected,
        "Undo put every one of them back as it was"
    );
}

#[test]
fn a_track_with_unfinished_file_work_is_not_deleted_by_a_merge() {
    let f = Fixture::new();
    // The duplicate is in the middle of a file organization that has not finished.
    rusqlite::Connection::open(&f.db)
        .unwrap()
        .execute(
            "INSERT INTO organization_operations VALUES('op',?1,'publishing','{}',NULL,0,0)",
            [&f.ids[1]],
        )
        .unwrap();
    let review = f.review(Tolerance::None);
    let before = f.content();
    let preview = preview(&f, &review, 0, MergeDisposition::Delete);
    let plan = preview.deletion.as_ref().unwrap();
    assert!(plan.files.is_empty(), "{plan:?}");
    assert_eq!(plan.skipped.len(), 1);
    assert_eq!(plan.skipped[0].reason, DuplicateSkipReason::Busy);
    assert_eq!(plan.skipped[0].track_id, f.ids[1]);
    assert!(
        plan.skipped[0]
            .detail
            .as_deref()
            .unwrap_or_default()
            .contains("file organization"),
        "{:?}",
        plan.skipped[0]
    );
    let stand_in = trash(&f);
    let result = service(&f, &stand_in)
        .apply_confirmed(&preview.id, true, &token())
        .unwrap();
    assert!(result.removed_ids.is_empty());
    assert!(f.paths[1].exists() && stand_in.taken().is_empty());
    assert_eq!(f.content(), before);
}

#[test]
fn a_kept_track_with_unfinished_file_work_keeps_its_duplicates_out_of_a_delete_merge() {
    let f = Fixture::new();
    // The track that keeps the audio is in the middle of a file organization.
    rusqlite::Connection::open(&f.db)
        .unwrap()
        .execute(
            "INSERT INTO organization_operations VALUES('op',?1,'publishing','{}',NULL,0,0)",
            [&f.ids[0]],
        )
        .unwrap();
    let review = f.review(Tolerance::None);
    let before = f.content();
    let preview = preview(&f, &review, 0, MergeDisposition::Delete);
    let plan = preview.deletion.as_ref().unwrap();
    assert!(plan.files.is_empty(), "{plan:?}");
    assert_eq!(plan.skipped.len(), 1);
    assert_eq!(plan.skipped[0].reason, DuplicateSkipReason::Busy);
    assert_eq!(plan.skipped[0].track_id, f.ids[1]);
    let detail = plan.skipped[0].detail.as_deref().unwrap_or_default();
    assert!(
        detail.contains("kept track") && detail.contains("file organization"),
        "{detail}"
    );
    let stand_in = trash(&f);
    let result = service(&f, &stand_in)
        .apply_confirmed(&preview.id, true, &token())
        .unwrap();
    assert!(result.removed_ids.is_empty());
    assert!(f.paths[1].exists() && stand_in.taken().is_empty());
    assert_eq!(f.content(), before);
}

/// A metadata draft over these tracks, made the way the app makes one.
fn draft_over(f: &Fixture, ids: &[&str]) -> (MetadataEditService, cuerithm_library::MetadataDraft) {
    let editor = MetadataEditService::new(f.lib.clone(), f.runtime.clone());
    let ids: Vec<String> = ids.iter().map(|id| id.to_string()).collect();
    let draft = editor.create_draft(&ids).unwrap();
    // It can be previewed now; this is what a deleted track would break.
    editor.preview(&draft.id, draft.revision, &token()).unwrap();
    (editor, draft)
}

#[test]
fn a_duplicate_in_an_open_metadata_draft_is_kept_until_the_draft_is_discarded() {
    let f = Fixture::new();
    // One draft over the duplicate and an unrelated track that stays in the library.
    let (editor, draft) = draft_over(&f, &[&f.ids[1], &f.ids[2]]);
    let review = f.review(Tolerance::None);
    let before = f.content();
    let blocked = preview(&f, &review, 0, MergeDisposition::Delete);
    let plan = blocked.deletion.as_ref().unwrap();
    assert!(plan.files.is_empty(), "{plan:?}");
    assert_eq!(plan.skipped.len(), 1);
    assert_eq!(plan.skipped[0].reason, DuplicateSkipReason::Busy);
    assert_eq!(plan.skipped[0].track_id, f.ids[1]);
    assert!(
        plan.skipped[0]
            .detail
            .as_deref()
            .unwrap_or_default()
            .contains("open metadata draft"),
        "{:?}",
        plan.skipped[0]
    );
    let stand_in = trash(&f);
    let result = service(&f, &stand_in)
        .apply_confirmed(&blocked.id, true, &token())
        .unwrap();
    assert!(result.removed_ids.is_empty());
    assert!(f.paths[1].exists() && stand_in.taken().is_empty());
    assert_eq!(f.content(), before);
    editor
        .preview(&draft.id, draft.revision, &token())
        .expect("the draft is still usable");
    // Once the draft is discarded the same duplicate can go.
    editor.discard_draft(&draft.id, draft.revision).unwrap();
    let review = f.review(Tolerance::None);
    let open = preview(&f, &review, 0, MergeDisposition::Delete);
    assert_eq!(open.deletion.as_ref().unwrap().files.len(), 1);
    let result = service(&f, &stand_in)
        .apply_confirmed(&open.id, true, &token())
        .unwrap();
    assert_eq!(result.removed_ids, vec![f.ids[1].clone()]);
}

#[test]
fn a_draft_that_lists_only_the_kept_track_does_not_stop_a_delete_merge() {
    let f = Fixture::new();
    // The kept track stays in the library, so the draft stays usable.
    let (editor, draft) = draft_over(&f, &[&f.ids[0], &f.ids[2]]);
    let review = f.review(Tolerance::None);
    let ready = preview(&f, &review, 0, MergeDisposition::Delete);
    assert_eq!(ready.deletion.as_ref().unwrap().files.len(), 1);
    let stand_in = trash(&f);
    let result = service(&f, &stand_in)
        .apply_confirmed(&ready.id, true, &token())
        .unwrap();
    assert_eq!(result.removed_ids, vec![f.ids[1].clone()]);
    editor
        .preview(&draft.id, draft.revision, &token())
        .expect("the draft over the kept track still works");
}

#[test]
fn an_open_metadata_draft_does_not_stop_an_archive_only_merge() {
    let f = Fixture::new();
    // Nothing is deleted, so a draft over the duplicate is not in danger.
    let (editor, draft) = draft_over(&f, &[&f.ids[1], &f.ids[2]]);
    let review = f.review(Tolerance::None);
    let archive = preview(&f, &review, 0, MergeDisposition::Archive);
    let stand_in = trash(&f);
    let result = service(&f, &stand_in)
        .apply_confirmed(&archive.id, false, &token())
        .unwrap();
    assert_eq!(result.archived_ids, vec![f.ids[1].clone()]);
    assert!(result.removed_ids.is_empty() && f.paths[1].exists());
    // The record and the draft are both still there (an archived track is not editable until it
    // is unarchived, which is how archiving always behaves).
    assert!(f.track_exists(&f.ids[1]));
    assert_eq!(editor.draft(&draft.id).unwrap().state, "editing");
}

#[test]
fn a_kept_track_that_is_still_being_prepared_keeps_its_duplicates_out_of_a_delete_merge() {
    let f = Fixture::new();
    rusqlite::Connection::open(&f.db)
        .unwrap()
        .execute(
            "UPDATE tracks SET state='incoming' WHERE id=?1",
            [&f.ids[0]],
        )
        .unwrap();
    let review = f.review(Tolerance::None);
    let preview = preview(&f, &review, 0, MergeDisposition::Delete);
    let plan = preview.deletion.as_ref().unwrap();
    assert!(plan.files.is_empty(), "{plan:?}");
    assert_eq!(plan.skipped.len(), 1);
    assert_eq!(plan.skipped[0].reason, DuplicateSkipReason::Busy);
    let detail = plan.skipped[0].detail.as_deref().unwrap_or_default();
    assert!(
        detail.contains("kept track") && detail.contains("still being prepared"),
        "{detail}"
    );
}

#[test]
fn a_trash_failure_leaves_that_file_and_its_record_untouched_and_removes_only_what_moved() {
    let mut f = Fixture::new();
    let third = f.add_track("track-4.wav", 37);
    f.playlist("both", vec![f.ids[1].clone(), f.ids[third].clone()]);
    let review = f.review(Tolerance::None);
    assert_eq!(review.groups[0].tracks.len(), 3);
    let preview = preview(&f, &review, 0, MergeDisposition::Delete);
    assert_eq!(preview.deletion.as_ref().unwrap().files.len(), 2);
    let before = f.content();
    let stand_in = trash(&f);
    stand_in.refuse("track-1.wav");
    let result = service(&f, &stand_in)
        .apply_confirmed(&preview.id, true, &token())
        .unwrap();

    assert_eq!(result.removed_ids, vec![f.ids[third].clone()]);
    assert_eq!(stand_in.taken(), vec!["track-4.wav"]);
    assert_eq!(result.not_deleted.len(), 1);
    let failed = &result.not_deleted[0];
    assert_eq!(failed.track_id, f.ids[1]);
    assert_eq!(failed.reason, DuplicateSkipReason::TrashFailed);
    assert!(f.paths[1].exists(), "the file stays where it was");
    assert!(f.track_exists(&f.ids[1]) && !f.archived(&f.ids[1]));
    assert!(!f.track_exists(&f.ids[third]));
    let members = f.lib.manual_playlist_members("both").unwrap();
    assert!(members.contains(&f.ids[1]) && members.contains(&f.ids[0]));
    assert!(!members.contains(&f.ids[third]));

    // Undo restores exactly what was removed.
    service(&f, &stand_in).undo(&result.id, &token()).unwrap();
    assert_eq!(f.content(), before);
}

#[test]
fn a_stale_report_cannot_delete_anything() {
    let f = Fixture::new();
    let review = f.review(Tolerance::None);
    let preview = preview(&f, &review, 0, MergeDisposition::Delete);
    let before = f.content();
    let files = hashes(&f);
    let stand_in = trash(&f);

    // The duplicate changes after the preview.
    let mut bytes = std::fs::read(&f.paths[1]).unwrap();
    bytes.push(0);
    std::fs::write(&f.paths[1], &bytes).unwrap();
    let changed = hash(&f.paths[1]);
    assert!(
        service(&f, &stand_in)
            .apply_confirmed(&preview.id, true, &token())
            .is_err()
    );
    assert_eq!(hash(&f.paths[1]), changed);
    assert!(stand_in.taken().is_empty());
    assert_eq!(f.content(), before);

    // The kept track's file disappears after the preview.
    std::fs::remove_file(&f.paths[0]).unwrap();
    let error = service(&f, &stand_in)
        .apply_confirmed(&preview.id, true, &token())
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("no longer has an available audio file"),
        "{error}"
    );
    assert!(stand_in.taken().is_empty());
    assert_eq!(f.content(), before);
    assert_eq!(hash(&f.paths[2]), files[2]);
}

#[test]
fn a_report_saved_before_the_file_disappeared_cannot_be_reviewed_or_deleted_from() {
    let f = Fixture::new();
    let scan = f.scan(Tolerance::None);
    std::fs::remove_file(&f.paths[1]).unwrap();
    let stand_in = trash(&f);
    assert!(f.lib.begin_duplicate_review(&scan.id, &token()).is_err());
    assert!(stand_in.taken().is_empty());
}

#[test]
fn delete_from_disk_with_a_donor_copies_tags_first_and_undo_restores_the_keepers_file() {
    let f = Fixture::new();
    let conn = rusqlite::Connection::open(&f.db).unwrap();
    let publication = serde_json::json!({"grid":{"bpm":128.75,"startTime":0.125},"cues":[{"position":0,"name":"Donor breakdown","startTime":8.,"type":"1","color":"blue"}],"implementation_sha256":"a".repeat(64),"inputs_sha256":"b".repeat(64)});
    conn.execute("UPDATE tracks SET title='Donor title',genre='House',bpm=136.25,musical_key='8A',energy=8,analysis_json=?1 WHERE id=?2",rusqlite::params![serde_json::json!({"raw_donor":true,"publication":publication}).to_string(),f.ids[1]]).unwrap();
    drop(conn);
    let review = f.review(Tolerance::None);
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
            &token(),
        )
        .unwrap();
    let keeper_before = hash(&f.paths[0]);
    let keeper_state = f.file_state(0);
    let before = f.content_ignoring_file_identity();
    let preview = preview(&f, &review, 0, MergeDisposition::Delete);
    assert_eq!(preview.plan.groups[0].copies.len(), 1);
    let stand_in = trash(&f);
    let service = service(&f, &stand_in);
    let result = service
        .apply_confirmed(&preview.id, true, &token())
        .unwrap();

    assert_eq!(result.removed_ids, vec![f.ids[1].clone()]);
    assert_eq!(result.updated_ids, vec![f.ids[0].clone()]);
    assert_eq!(f.lib.track(&f.ids[0]).unwrap().title, "Donor title");
    assert_ne!(
        hash(&f.paths[0]),
        keeper_before,
        "the keeper's tags were rewritten"
    );
    assert_eq!(stand_in.taken(), vec!["track-1.wav"]);
    assert!(!f.track_exists(&f.ids[1]));

    service.undo(&result.id, &token()).unwrap();
    assert_eq!(
        f.file_state(0),
        keeper_state,
        "the keeper's previous tag values are back, and its audio is untouched"
    );
    f.assert_library_knows_file(0);
    assert_eq!(f.content_ignoring_file_identity(), before);
    assert!(f.track_exists(&f.ids[1]));
}

#[test]
fn undo_data_is_kept_only_for_the_most_recent_merges_that_delete_files() {
    let mut f = Fixture::empty();
    let stand_in = trash(&f);
    let service = service(&f, &stand_in);
    let mut applied = vec![];
    for round in 0..12 {
        // A new pair of identical files, unrelated to every earlier pair.
        let keep = f.add_track(&format!("pair-{round}-a.wav"), 201 + 2 * round);
        let copy = f.add_track(&format!("pair-{round}-b.wav"), 201 + 2 * round);
        let words = [
            "alpha", "bravo", "delta", "echo", "golf", "kilo", "lima", "oscar", "papa", "romeo",
            "sierra", "tango",
        ];
        let title = format!("Distinct title {round} {}", words[round as usize]);
        let conn = rusqlite::Connection::open(&f.db).unwrap();
        conn.execute(
            "UPDATE tracks SET title=?1 WHERE id IN (?2,?3)",
            rusqlite::params![title, f.ids[keep], f.ids[copy]],
        )
        .unwrap();
        drop(conn);
        let review = f.review(Tolerance::None);
        assert_eq!(review.groups.len(), 1, "round {round}");
        let preview = preview(&f, &review, 0, MergeDisposition::Delete);
        let result = service
            .apply_confirmed(&preview.id, true, &token())
            .unwrap();
        assert_eq!(result.removed_ids.len(), 1, "round {round}");
        applied.push(result.id);
    }
    let history = f.lib.duplicate_merge_history().unwrap();
    assert_eq!(history.len(), 12);
    let expired = |id: &str| {
        history
            .iter()
            .find(|h| h.result.id == id)
            .unwrap()
            .undo_expired
    };
    // The two oldest removals are final; the ten newest can still be undone.
    assert!(expired(&applied[0]) && expired(&applied[1]));
    assert!(applied[2..].iter().all(|id| !expired(id)));
    for id in &applied[..2] {
        let error = service.undo(id, &token()).unwrap_err().to_string();
        assert!(error.contains("no longer available"), "{error}");
    }
    // What a final removal keeps is only its summary.
    let conn = rusqlite::Connection::open(&f.db).unwrap();
    let stored: (Option<String>, i64) = conn
        .query_row(
            "SELECT json_extract(payload_json,'$.removed'),(SELECT COUNT(*) FROM json_each(json_extract(payload_json,'$.before'))) FROM maintenance_writes WHERE id=?1",
            [&applied[0]],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(stored.0, None);
    assert_eq!(stored.1, 0);
    service.undo(&applied[2], &token()).unwrap();
}

// ---- Files the Trash cannot take -------------------------------------------------------

#[test]
fn a_file_the_trash_cannot_take_is_announced_in_the_preview_and_then_deleted_permanently() {
    let f = Fixture::new();
    f.playlist("set", vec![f.ids[0].clone(), f.ids[1].clone()]);
    let before = f.content();
    let review = f.review(Tolerance::None);
    let stand_in = trash(&f);
    stand_in.unavailable("track-1.wav");
    let preview = preview_with(&f, &review, 0, MergeDisposition::Delete, &*stand_in);
    let plan = preview.deletion.as_ref().unwrap();
    assert_eq!(plan.files.len(), 1);
    assert!(plan.files[0].permanent, "the preview names it as permanent");
    let keeper_hash = hash(&f.paths[0]);

    let service = service(&f, &stand_in);
    // Still refused without the explicit confirmation.
    assert!(service.apply(&preview.id, &token()).is_err());
    assert!(f.paths[1].exists());

    // The Trash is tried first; here it also refuses, so the announced file is deleted.
    stand_in.refuse("track-1.wav");
    let result = service
        .apply_confirmed(&preview.id, true, &token())
        .unwrap();
    assert_eq!(result.permanently_deleted.len(), 1);
    assert_eq!(result.permanently_deleted[0].track_id, f.ids[1]);
    assert!(result.trashed.is_empty() && result.not_deleted.is_empty());
    assert_eq!(result.removed_ids, vec![f.ids[1].clone()]);
    assert_eq!(stand_in.deleted(), vec!["track-1.wav"]);
    assert!(stand_in.taken().is_empty());
    assert!(!f.paths[1].exists(), "the file is gone for good");
    assert!(!f.track_exists(&f.ids[1]));
    assert_eq!(hash(&f.paths[0]), keeper_hash, "the keeper is untouched");
    assert_eq!(
        f.lib.manual_playlist_members("set").unwrap(),
        vec![f.ids[0].clone()]
    );
    let history = f.lib.duplicate_merge_history().unwrap();
    assert_eq!(history[0].result.permanently_deleted.len(), 1);

    // Undo brings the records back; it cannot bring a permanently deleted file back.
    service.undo(&result.id, &token()).unwrap();
    assert_eq!(f.content(), before);
    assert!(!f.paths[1].exists());
}

#[test]
fn a_file_announced_as_permanent_still_goes_to_the_trash_when_the_trash_works() {
    let f = Fixture::new();
    let review = f.review(Tolerance::None);
    let stand_in = trash(&f);
    stand_in.unavailable("track-1.wav");
    let preview = preview_with(&f, &review, 0, MergeDisposition::Delete, &*stand_in);
    assert!(preview.deletion.as_ref().unwrap().files[0].permanent);
    let result = service(&f, &stand_in)
        .apply_confirmed(&preview.id, true, &token())
        .unwrap();
    assert_eq!(result.trashed.len(), 1);
    assert!(result.permanently_deleted.is_empty());
    assert_eq!(stand_in.taken(), vec!["track-1.wav"]);
    assert!(stand_in.deleted().is_empty());
}

#[test]
fn a_trash_refusal_that_the_preview_did_not_announce_never_becomes_a_permanent_deletion() {
    let f = Fixture::new();
    let review = f.review(Tolerance::None);
    let stand_in = trash(&f);
    let preview = preview_with(&f, &review, 0, MergeDisposition::Delete, &*stand_in);
    assert!(!preview.deletion.as_ref().unwrap().files[0].permanent);
    let before = f.content();
    stand_in.refuse("track-1.wav");
    let result = service(&f, &stand_in)
        .apply_confirmed(&preview.id, true, &token())
        .unwrap();
    assert!(result.permanently_deleted.is_empty() && result.trashed.is_empty());
    assert_eq!(
        result.not_deleted[0].reason,
        DuplicateSkipReason::TrashFailed
    );
    assert!(f.paths[1].exists() && stand_in.deleted().is_empty());
    assert_eq!(f.content(), before);
}

#[test]
fn a_trash_that_stops_working_after_the_preview_leaves_the_file_until_it_is_previewed_again() {
    let f = Fixture::new();
    let review = f.review(Tolerance::None);
    let stand_in = trash(&f);
    let preview = preview_with(&f, &review, 0, MergeDisposition::Delete, &*stand_in);
    stand_in.unavailable("track-1.wav");
    let before = f.content();
    let result = service(&f, &stand_in)
        .apply_confirmed(&preview.id, true, &token())
        .unwrap();
    assert!(result.removed_ids.is_empty());
    assert_eq!(result.not_deleted.len(), 1);
    assert!(f.paths[1].exists() && stand_in.deleted().is_empty() && stand_in.taken().is_empty());
    assert_eq!(f.content(), before);
}

#[test]
fn every_safeguard_holds_when_the_trash_is_unavailable() {
    let mut f = Fixture::new();
    // A hard link of the duplicate used by a remaining record, a hard link of the keeper's
    // file, and a different mix: none may ever be deleted, permanently or otherwise.
    let shared = f.dir.path().join("also-track-1.wav");
    std::fs::hard_link(&f.paths[1], &shared).unwrap();
    let shared_index = f.import(shared);
    let stamp = f.lib.duplicate_ignores().unwrap().stamp;
    f.lib
        .set_duplicate_ignores(vec![f.ids[shared_index].clone()], true, stamp)
        .unwrap();
    let keeper_alias = f.dir.path().join("keeper-alias.wav");
    std::fs::hard_link(&f.paths[0], &keeper_alias).unwrap();
    let alias_index = f.import(keeper_alias);
    let mix = f.add_track("other-mix.wav", 71);
    let review = f.review(Tolerance::None);
    let before = f.content();
    let files = hashes(&f);
    let stand_in = trash(&f);
    for name in [
        "track-0.wav",
        "track-1.wav",
        "keeper-alias.wav",
        "also-track-1.wav",
        "track-2.wav",
        "other-mix.wav",
    ] {
        stand_in.unavailable(name);
    }
    for group in 0..review.groups.len() {
        let preview = preview_with(&f, &review, group, MergeDisposition::Delete, &*stand_in);
        let plan = preview.deletion.as_ref().unwrap();
        assert!(
            plan.files.iter().all(|file| ![
                f.ids[shared_index].as_str(),
                f.ids[mix].as_str(),
                f.ids[2].as_str()
            ]
            .contains(&file.track_id.as_str())),
            "{plan:?}"
        );
        // Nothing that is the keeper's own file is ever listed.
        let keeper = preview.plan.groups[0].keeper.clone();
        assert!(plan.files.iter().all(|file| file.track_id != keeper));
        assert!(
            plan.files
                .iter()
                .all(|file| file.track_id != f.ids[alias_index] || keeper != f.ids[0])
        );
    }
    let preview = preview_with(&f, &review, 0, MergeDisposition::Delete, &*stand_in);
    let plan = preview.deletion.as_ref().unwrap();
    // The duplicate whose file another record still uses is skipped, not deleted.
    assert!(
        plan.skipped.iter().any(|s| s.track_id == f.ids[1]
            && s.reason == DuplicateSkipReason::SharedWithRemainingRecord)
    );
    let result = service(&f, &stand_in)
        .apply_confirmed(&preview.id, true, &token())
        .unwrap();
    assert!(result.removed_ids.is_empty(), "{result:?}");
    assert_eq!(hashes(&f), files);
    assert!(stand_in.deleted().is_empty() && stand_in.taken().is_empty());
    assert_eq!(f.content(), before);
}

// ---- What a removal purges -------------------------------------------------------------

fn count(f: &Fixture, sql: &str, id: &str) -> i64 {
    rusqlite::Connection::open(&f.db)
        .unwrap()
        .query_row(sql, [id], |r| r.get(0))
        .unwrap()
}
/// Cover art, waveform cache files and a cached analysis result for one file content.
fn seed_caches(f: &Fixture, content_hash: &str) -> Vec<std::path::PathBuf> {
    let root = f.lib.data_dir();
    std::fs::create_dir_all(root.join("cache/artwork")).unwrap();
    std::fs::create_dir_all(root.join("cache/waveforms")).unwrap();
    let mut files = vec![root.join("cache/artwork").join(content_hash)];
    for bins in cuerithm_library::waveform::CACHED_RESOLUTIONS {
        let key =
            cuerithm_library::waveform::waveform_cache_key(content_hash, &f.runtime.identity, bins);
        files.push(root.join("cache/waveforms").join(format!("{key}.json")));
    }
    for file in &files {
        std::fs::write(file, b"cache").unwrap();
    }
    rusqlite::Connection::open(&f.db)
        .unwrap()
        .execute(
            "INSERT INTO preparation_analysis VALUES(?1,'settings','engine','decoder','{}')",
            [content_hash],
        )
        .unwrap();
    files
}

#[test]
fn removing_a_track_purges_what_undo_does_not_need_and_keeps_what_it_does() {
    let f = Fixture::new();
    let (keeper_hash, removed_hash) = (hash(&f.paths[0]), hash(&f.paths[1]));
    assert_ne!(keeper_hash, removed_hash);
    let keeper_caches = seed_caches(&f, &keeper_hash);
    let removed_caches = seed_caches(&f, &removed_hash);
    let review = f.review(Tolerance::None);
    // The scan cached a fingerprint for every track.
    let fingerprints = "SELECT COUNT(*) FROM maintenance_fingerprints WHERE track_id=?1";
    assert_eq!(count(&f, fingerprints, &f.ids[1]), 1);
    let preview = preview(&f, &review, 0, MergeDisposition::Delete);
    let stand_in = trash(&f);
    let service = service(&f, &stand_in);
    let result = service
        .apply_confirmed(&preview.id, true, &token())
        .unwrap();
    assert_eq!(result.removed_ids, vec![f.ids[1].clone()]);

    // Purged: rebuildable per-track rows, caches of content nobody else has, the stale saved
    // reports that list the removed music, and the bookkeeping row of the removed track.
    assert_eq!(count(&f, fingerprints, &f.ids[1]), 0);
    assert_eq!(
        count(&f, fingerprints, &f.ids[0]),
        1,
        "the keeper keeps its own"
    );
    let analysis = "SELECT COUNT(*) FROM preparation_analysis WHERE source_hash=?1";
    assert_eq!(count(&f, analysis, &removed_hash), 0);
    assert_eq!(count(&f, analysis, &keeper_hash), 1);
    // The waveforms are rebuilt on demand and go. The cover stays for Undo: with the file in the
    // Trash it can be the only copy of the picture left (the first file is the cover).
    assert!(removed_caches[0].exists(), "the cover is kept for Undo");
    assert!(removed_caches[1..].iter().all(|file| !file.exists()));
    assert!(keeper_caches.iter().all(|file| file.exists()));
    let reports = "SELECT COUNT(*) FROM maintenance_scans WHERE kind IN ('duplicates','duplicate_review','duplicate_merge_plan') AND ?1=?1";
    assert_eq!(count(&f, reports, "x"), 0);
    let epoch = "SELECT COUNT(*) FROM maintenance_track_epochs WHERE id=?1";
    assert_eq!(count(&f, epoch, &f.ids[1]), 0);
    assert_eq!(count(&f, epoch, &f.ids[0]), 1);
    // Kept for the bounded Undo: the record and what belongs to it, and the history entry.
    let payload: String = rusqlite::Connection::open(&f.db)
        .unwrap()
        .query_row(
            "SELECT payload_json FROM maintenance_writes WHERE id=?1",
            [&result.id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(payload.contains("\"jobs\"") && payload.contains("\"tracks\""));
    assert!(!payload.contains("maintenance_fingerprints"));
    assert_eq!(
        f.lib.duplicate_merge_history().unwrap()[0].result.id,
        result.id
    );

    // Undo restores the record, not the caches (they are rebuilt on demand), and the
    // bookkeeping row comes back with the record.
    service.undo(&result.id, &token()).unwrap();
    assert!(f.track_exists(&f.ids[1]));
    assert_eq!(count(&f, fingerprints, &f.ids[1]), 0);
    assert_eq!(count(&f, epoch, &f.ids[1]), 1);
    assert!(
        removed_caches[0].exists(),
        "the restored track still has its cover"
    );
    assert!(removed_caches[1..].iter().all(|file| !file.exists()));
}

#[test]
fn caches_of_content_that_a_remaining_track_still_has_are_not_purged() {
    let mut f = Fixture::empty();
    // Byte-identical files: the removed one shares its content hash with the keeper.
    let first = f.add_track("same-a.wav", 59);
    let second = f.add_track("same-b.wav", 59);
    assert_eq!(hash(&f.paths[first]), hash(&f.paths[second]));
    let shared = hash(&f.paths[first]);
    let caches = seed_caches(&f, &shared);
    let review = f.review(Tolerance::None);
    let preview = preview(&f, &review, 0, MergeDisposition::Delete);
    assert_eq!(
        preview.deletion.as_ref().unwrap().files[0].identity,
        DuplicateIdentity::SameFile
    );
    let stand_in = trash(&f);
    let result = service(&f, &stand_in)
        .apply_confirmed(&preview.id, true, &token())
        .unwrap();
    assert_eq!(result.removed_ids.len(), 1);
    assert!(
        caches.iter().all(|file| file.exists()),
        "the keeper still uses them"
    );
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) FROM preparation_analysis WHERE source_hash=?1",
            &shared
        ),
        1
    );
}

#[test]
fn archiving_purges_nothing() {
    let f = Fixture::new();
    let caches = seed_caches(&f, &hash(&f.paths[1]));
    let review = f.review(Tolerance::None);
    let preview = preview(&f, &review, 0, MergeDisposition::Archive);
    let stand_in = trash(&f);
    service(&f, &stand_in).apply(&preview.id, &token()).unwrap();
    assert!(caches.iter().all(|file| file.exists()));
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) FROM maintenance_fingerprints WHERE track_id=?1",
            &f.ids[1]
        ),
        1
    );
}

// ---- Undo only goes ahead when the world is as the removal left it ---------------------

fn rows(f: &Fixture, sql: &str) -> i64 {
    rusqlite::Connection::open(&f.db)
        .unwrap()
        .query_row(sql, [], |r| r.get(0))
        .unwrap()
}

#[test]
fn undo_works_when_unrelated_inserts_reused_the_removed_rows_positions() {
    let mut f = Fixture::empty();
    let keep = f.add_track("keep.wav", 61);
    let copy = f.add_track("copy.wav", 61);
    let conn = rusqlite::Connection::open(&f.db).unwrap();
    conn.execute("UPDATE tracks SET title='Pair'", []).unwrap();
    drop(conn);
    let review = f.review(Tolerance::None);
    let preview = preview(&f, &review, 0, MergeDisposition::Delete);
    let stand_in = trash(&f);
    let service = service(&f, &stand_in);
    let result = service
        .apply_confirmed(&preview.id, true, &token())
        .unwrap();
    assert_eq!(result.removed_ids, vec![f.ids[copy].clone()]);
    // The removed track held the highest row position, so a new track and its job take it.
    let newer = f.add_track("later.wav", 67);
    let reused = rows(
        &f,
        &format!("SELECT rowid FROM tracks WHERE id='{}'", f.ids[newer]),
    );
    assert_eq!(reused, 2, "the new track reuses the freed row position");
    let newer_title = rows(
        &f,
        &format!(
            "SELECT COUNT(*) FROM tracks WHERE id='{}' AND title='Signal'",
            f.ids[newer]
        ),
    );
    assert_eq!(newer_title, 1);

    service.undo(&result.id, &token()).unwrap();
    assert!(
        f.track_exists(&f.ids[copy])
            && f.track_exists(&f.ids[newer])
            && f.track_exists(&f.ids[keep])
    );
    assert_eq!(rows(&f, "SELECT COUNT(*) FROM tracks"), 3);
    assert_eq!(rows(&f, "SELECT COUNT(*) FROM jobs"), 3);
    assert_eq!(
        rows(
            &f,
            &format!("SELECT rowid FROM tracks WHERE id='{}'", f.ids[newer])
        ),
        reused,
        "the newer track keeps its row"
    );
    assert_eq!(
        rows(
            &f,
            &format!(
                "SELECT COUNT(*) FROM jobs WHERE track_id='{}'",
                f.ids[newer]
            )
        ),
        1
    );
}

#[test]
fn undo_leaves_link_rows_that_changed_after_the_removal_alone_and_never_takes_them_over() {
    let f = Fixture::new();
    let conn = rusqlite::Connection::open(&f.db).unwrap();
    conn.execute_batch("INSERT INTO native_targets VALUES('desk','rekordbox','db','device','{}','fingerprint','observation','[]',1,1,1);").unwrap();
    conn.execute(
        "INSERT INTO sync_links VALUES(?1,'target','external-1','{}')",
        [&f.ids[1]],
    )
    .unwrap();
    conn.execute("INSERT INTO native_track_links VALUES('desk','content-1',?1,'{}',NULL,NULL,'old baseline','linked',1)", [&f.ids[1]]).unwrap();
    conn.execute("INSERT INTO native_track_links VALUES('desk','content-2',?1,'{}',NULL,NULL,'untouched','linked',1)", [&f.ids[1]]).unwrap();
    drop(conn);
    let review = f.review(Tolerance::None);
    let preview = preview(&f, &review, 0, MergeDisposition::Delete);
    let stand_in = trash(&f);
    let service = service(&f, &stand_in);
    let result = service
        .apply_confirmed(&preview.id, true, &token())
        .unwrap();
    assert_eq!(
        rows(
            &f,
            "SELECT COUNT(*) FROM native_track_links WHERE local_track_id IS NOT NULL"
        ),
        0
    );
    // The rekordbox links stay as DJ-software tracks without a library track, exactly as
    // when Missing & broken removes a record.
    assert_eq!(
        rows(
            &f,
            "SELECT COUNT(*) FROM native_track_links WHERE status='not_in_local_library'"
        ),
        2
    );

    // The sync links the same external records to the kept track while the duplicate is gone.
    let conn = rusqlite::Connection::open(&f.db).unwrap();
    conn.execute(
        "INSERT INTO sync_links VALUES(?1,'target','external-1','{}')",
        [&f.ids[0]],
    )
    .unwrap();
    conn.execute("UPDATE native_track_links SET local_track_id=?1,baseline_json='newer baseline',status='linked',updated=2 WHERE content_id='content-1'", [&f.ids[0]]).unwrap();
    drop(conn);

    let undone = service.undo(&result.id, &token()).unwrap();
    assert_eq!(
        undone.links_not_restored, 2,
        "one sync link and one rekordbox link"
    );
    assert!(f.track_exists(&f.ids[1]));
    let keeper_link: (String, String) = rusqlite::Connection::open(&f.db)
        .unwrap()
        .query_row(
            "SELECT local_track_id,baseline_json FROM native_track_links WHERE content_id='content-1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(keeper_link, (f.ids[0].clone(), "newer baseline".to_owned()));
    // Links nobody touched come back with the record.
    assert_eq!(
        rows(
            &f,
            &format!(
                "SELECT COUNT(*) FROM native_track_links WHERE content_id='content-2' AND local_track_id='{}'",
                f.ids[1]
            )
        ),
        1
    );
    assert_eq!(
        rows(
            &f,
            &format!(
                "SELECT COUNT(*) FROM sync_links WHERE track_id='{}'",
                f.ids[1]
            )
        ),
        0,
        "the sync link now belongs to the kept track"
    );
}

fn moved_in_the_trash(stand_in: &FakeTrash, name: &str) -> std::path::PathBuf {
    stand_in.folder.join(name)
}

#[test]
fn undo_refuses_before_touching_anything_when_a_different_file_sits_at_a_removed_location() {
    let f = Fixture::new();
    let conn = rusqlite::Connection::open(&f.db).unwrap();
    let publication = serde_json::json!({"grid":{"bpm":128.75,"startTime":0.125},"cues":[{"position":0,"name":"Donor breakdown","startTime":8.,"type":"1","color":"blue"}],"implementation_sha256":"a".repeat(64),"inputs_sha256":"b".repeat(64)});
    conn.execute(
        "UPDATE tracks SET title='Donor title',analysis_json=?1 WHERE id=?2",
        rusqlite::params![
            serde_json::json!({"raw_donor":true,"publication":publication}).to_string(),
            f.ids[1]
        ],
    )
    .unwrap();
    drop(conn);
    let review = f.review(Tolerance::None);
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
            &token(),
        )
        .unwrap();
    let keeper_original = hash(&f.paths[0]);
    let keeper_state = f.file_state(0);
    let preview = preview(&f, &review, 0, MergeDisposition::Delete);
    let stand_in = trash(&f);
    let service = service(&f, &stand_in);
    let result = service
        .apply_confirmed(&preview.id, true, &token())
        .unwrap();
    let keeper_after_merge = hash(&f.paths[0]);
    assert_ne!(
        keeper_after_merge, keeper_original,
        "the donor copy rewrote the keeper's tags"
    );
    let before = f.content();
    let journal = rows(&f, "SELECT COUNT(*) FROM maintenance_writes");

    // Someone puts an unrelated file where the removed track's file was.
    std::fs::write(&f.paths[1], b"not the same audio").unwrap();
    let error = service.undo(&result.id, &token()).unwrap_err().to_string();
    assert!(error.contains("a different file is now at"), "{error}");
    assert_eq!(
        hash(&f.paths[0]),
        keeper_after_merge,
        "the keeper's file was not touched"
    );
    assert_eq!(f.content(), before);
    assert_eq!(
        rows(&f, "SELECT COUNT(*) FROM maintenance_writes"),
        journal,
        "nothing was staged or journaled"
    );
    assert!(f.lib.duplicate_recovery().unwrap().is_empty());
    assert_eq!(
        std::fs::read(&f.paths[1]).unwrap(),
        b"not the same audio",
        "the stranger's file is left alone"
    );

    // The file comes back from the Trash instead: now Undo is fine and restores the keeper.
    std::fs::remove_file(&f.paths[1]).unwrap();
    std::fs::rename(moved_in_the_trash(&stand_in, "track-1.wav"), &f.paths[1]).unwrap();
    service.undo(&result.id, &token()).unwrap();
    assert_eq!(
        f.file_state(0),
        keeper_state,
        "the keeper's previous tag values are back"
    );
    assert!(f.track_exists(&f.ids[1]));
}

#[test]
fn undo_refuses_when_the_restored_file_was_added_again_as_a_new_track() {
    let mut f = Fixture::new();
    let review = f.review(Tolerance::None);
    let preview = preview(&f, &review, 0, MergeDisposition::Delete);
    let stand_in = trash(&f);
    let service = service(&f, &stand_in);
    let result = service
        .apply_confirmed(&preview.id, true, &token())
        .unwrap();
    // The user restores the file from the Trash and the watch folder adds it as a new track.
    std::fs::rename(moved_in_the_trash(&stand_in, "track-1.wav"), &f.paths[1]).unwrap();
    f.import(f.paths[1].clone());
    let before = f.content();
    let journal = rows(&f, "SELECT COUNT(*) FROM maintenance_writes");
    let error = service.undo(&result.id, &token()).unwrap_err().to_string();
    assert!(error.contains("already used"), "{error}");
    assert_eq!(f.content(), before);
    assert_eq!(rows(&f, "SELECT COUNT(*) FROM maintenance_writes"), journal);
    assert!(
        !f.track_exists(&f.ids[1]),
        "the removed record stays removed"
    );
}
