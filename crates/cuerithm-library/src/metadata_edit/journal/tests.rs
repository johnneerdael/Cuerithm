use super::*;
use crate::{Library, MetadataEditorPatch};
use cuerithm_autotag::{MetadataTextField, TextEdit};
fn add_ready_track(lib: &Library, path: &std::path::Path, step: f64) -> String {
    let mut wav = hound::WavWriter::create(
        path,
        hound::WavSpec {
            channels: 1,
            sample_rate: 44100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    for n in 0..44100 {
        wav.write_sample(((n as f64 * step).sin() * 4000.) as i16)
            .unwrap();
    }
    wav.finalize().unwrap();
    let job = lib
        .enqueue(
            std::slice::from_ref(&path.to_path_buf()),
            lib.settings().unwrap().revision,
        )
        .unwrap()
        .remove(0);
    let track = lib
        .jobs()
        .unwrap()
        .into_iter()
        .find(|j| j.id == job)
        .unwrap()
        .track_id;
    lib.conn().unwrap().execute("UPDATE tracks SET state='ready',analysis_json='{}',musical_key='8A',bpm=128,energy=7 WHERE id=?1",[&track]).unwrap();
    track
}
fn fixture() -> (
    tempfile::TempDir,
    MetadataEditService,
    String,
    std::path::PathBuf,
) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let path = root.join("source.wav");
    let lib = Library::open(&root.join("data/library.sqlite")).unwrap();
    let track = add_ready_track(&lib, &path, 0.1);
    let runtime = cuerithm_media::MediaRuntime::from_bundle(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/media-runtime/bin"),
    )
    .unwrap();
    (temp, MetadataEditService::new(lib, runtime), track, path)
}
fn reserved(service: &MetadataEditService, track: &str) -> (String, String) {
    let draft = service.create_draft(&[track.into()]).unwrap();
    let mut patch = MetadataEditorPatch::default();
    patch.fields.text.insert(
        MetadataTextField::Title,
        TextEdit::Set {
            values: vec!["New title".into()],
        },
    );
    let draft = service
        .update_draft(&draft.id, draft.revision, patch)
        .unwrap();
    let preview = service
        .preview(&draft.id, draft.revision, &CancellationToken::default())
        .unwrap();
    let text: String = service
        .library
        .conn()
        .unwrap()
        .query_row(
            "SELECT payload_json FROM metadata_edit_previews WHERE id=?1",
            [&preview.id],
            |r| r.get(0),
        )
        .unwrap();
    let frozen: FrozenPreview = serde_json::from_str(&text).unwrap();
    let batch = MetadataEditBatch {
        id: uuid::Uuid::new_v4().to_string(),
        draft_id: draft.id,
        draft_revision: draft.revision,
        state: "saving".into(),
        rows: vec![],
        created: now(),
        can_undo: false,
        can_recover: false,
        error: None,
    };
    let row = uuid::Uuid::new_v4().to_string();
    service
        .library
        .conn()
        .unwrap()
        .execute(
            "INSERT INTO metadata_edit_batches VALUES(?1,?2,?3,'apply','saving',?4,NULL,?5,?5)",
            params![
                batch.id,
                batch.draft_id,
                batch.draft_revision,
                bounded_json(&batch).unwrap(),
                batch.created
            ],
        )
        .unwrap();
    service.library.conn().unwrap().execute("INSERT INTO metadata_edit_rows(id,batch_id,track_id,ordinal,state,payload_json)VALUES(?1,?2,?3,0,'ready',?4)",params![row,batch.id,track,bounded_json(&frozen.rows[0]).unwrap()]).unwrap();
    service
        .library
        .conn()
        .unwrap()
        .execute(
            "UPDATE metadata_edit_previews SET state='applied' WHERE id=?1",
            [&preview.id],
        )
        .unwrap();
    let write = service
        .begin_write_owned(&batch.id, &row, &frozen.rows[0], None, None, None)
        .unwrap();
    (batch.id, write)
}
#[test]
fn published_metadata_recovers_after_reopen_without_adopting_equal_external_copy() {
    let (_temp, service, track, path) = fixture();
    let (batch, id) = reserved(&service, &track);
    let before = file_sha256(&path).unwrap();
    let error = service.publish_write_with(&id, &CancellationToken::default(), || {
        anyhow::bail!("synthetic crash after native replacement")
    });
    assert!(error.is_err());
    let (_, write) = service.load_write(&id).unwrap();
    let after = write.row.stage.as_ref().unwrap().after.clone();
    assert_ne!(before, after);
    assert_eq!(file_sha256(&path).unwrap(), after);
    assert_eq!(
        service
            .library
            .metadata_edit_source_owner(&path, &after)
            .unwrap(),
        Some(track.clone())
    );
    let root = service.library.data_dir().to_path_buf();
    let runtime = service.runtime.clone();
    drop(service);
    let lib = Library::open(&root.join("library.sqlite")).unwrap();
    lib.recover_metadata_edits().unwrap();
    let service = MetadataEditService::new(lib, runtime);
    let batch = service
        .recover(&batch, &CancellationToken::default())
        .unwrap();
    assert_eq!(batch.state, "saved", "{batch:?}");
    let artifact = service.load_write(&id).unwrap().1.artifact().unwrap();
    assert!(
        !artifact.staged.exists(),
        "committed staged copy is redundant"
    );
    assert!(!artifact.publish.exists());
    assert!(!artifact.displaced.exists());
    assert!(artifact.backup.as_ref().unwrap().exists());
    assert_eq!(batch.error, None);
    assert_eq!(service.history().unwrap()[0].error, None);
    assert_eq!(
        service
            .library
            .metadata_edit_source_owner(&path, &after)
            .unwrap(),
        None
    );
    let undo = service
        .undo(&batch.id, &CancellationToken::default())
        .unwrap();
    assert_eq!(undo.state, "undone", "{undo:?}");
    assert_eq!(file_sha256(&path).unwrap(), before);
}

#[test]
fn archive_cannot_invalidate_a_native_publication_between_exchange_and_commit() {
    let (_temp, service, track, path) = fixture();
    let (batch, id) = reserved(&service, &track);
    service
        .publish_write_with(&id, &CancellationToken::default(), || {
            assert!(
                service.library.set_archived(&track, true).is_err(),
                "Archive must not invalidate the owner after its native exchange"
            );
            assert!(!service.library.track(&track)?.archived);
            Ok(())
        })
        .unwrap();
    assert_eq!(service.finish_batch(&batch).unwrap().state, "saved");
    assert_eq!(
        service.library.source_hash(&track).unwrap(),
        file_sha256(&path).unwrap()
    );
    service.library.set_archived(&track, true).unwrap();
    assert!(service.library.track(&track).unwrap().archived);
}

#[test]
fn archive_and_restore_retain_the_owner_of_an_interrupted_native_publication() {
    let (_temp, service, track, path) = fixture();
    let (batch, id) = reserved(&service, &track);
    let original_job: String = service
        .library
        .conn()
        .unwrap()
        .query_row(
            "SELECT current_job FROM tracks WHERE id=?1",
            [&track],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        service
            .publish_write_with(&id, &CancellationToken::default(), || {
                anyhow::bail!("synthetic interruption after native exchange")
            })
            .is_err()
    );
    let after = file_sha256(&path).unwrap();
    let frozen_hash = service.library.source_hash(&track).unwrap();
    assert_ne!(after, frozen_hash);
    assert!(
        service.library.set_archived(&track, true).is_err(),
        "Recover the retained publication before changing its archive identity"
    );
    assert!(!service.library.track(&track).unwrap().archived);

    // An older application could already have archived this interrupted row.
    service
        .library
        .conn()
        .unwrap()
        .execute("UPDATE tracks SET archived=1 WHERE id=?1", [&track])
        .unwrap();
    assert!(service.library.set_archived(&track, false).is_err());
    assert!(service.library.track(&track).unwrap().archived);
    assert_eq!(service.library.source_hash(&track).unwrap(), frozen_hash);
    let retained_job: String = service
        .library
        .conn()
        .unwrap()
        .query_row(
            "SELECT current_job FROM tracks WHERE id=?1",
            [&track],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(retained_job, original_job);

    service
        .library
        .conn()
        .unwrap()
        .execute("UPDATE tracks SET archived=0 WHERE id=?1", [&track])
        .unwrap();
    service.library.recover_metadata_edits().unwrap();
    assert_eq!(
        service
            .recover(&batch, &CancellationToken::default())
            .unwrap()
            .state,
        "saved"
    );
    assert_eq!(service.library.source_hash(&track).unwrap(), after);
    service.library.set_archived(&track, true).unwrap();
}

#[test]
fn unknown_replacement_after_publication_remains_recoverable_and_is_never_adopted() {
    let (_t, service, track, path) = fixture();
    let (batch, id) = reserved(&service, &track);
    assert!(
        service
            .publish_write_with(&id, &CancellationToken::default(), || anyhow::bail!(
                "crash"
            ))
            .is_err()
    );
    let bytes = std::fs::read(&path).unwrap();
    let replacement = path.with_file_name("external.wav");
    std::fs::write(&replacement, &bytes).unwrap();
    std::fs::rename(&replacement, &path).unwrap();
    service.library.recover_metadata_edits().unwrap();
    assert_eq!(
        service
            .library
            .metadata_edit_source_owner(&path, &file_sha256(&path).unwrap())
            .unwrap(),
        None
    );
    let result = service
        .recover(&batch, &CancellationToken::default())
        .unwrap();
    assert_eq!(result.state, "recovery_required", "{result:?}");
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(service.load_write(&id).unwrap().0, "recovery_required");
}
#[test]
fn pending_reservation_blocks_other_writers() {
    let (_t, service, track, path) = fixture();
    let (_batch, _id) = reserved(&service, &track);
    assert!(
        ensure_unreserved(&service.library.conn().unwrap(), &path_bytes(&path), None)
            .unwrap_err()
            .to_string()
            .contains("Another metadata edit owns")
    );
}

#[test]
fn saved_cleanup_interruption_retries_without_republishing_and_clears_reopened_error() {
    let (_temp, service, track, path) = fixture();
    let (batch, id) = reserved(&service, &track);
    let before = file_sha256(&path).unwrap();
    service
        .publish_write_with_cleanup(
            &id,
            &CancellationToken::default(),
            || Ok(()),
            |phase| {
                if phase == crate::artifact_cleanup::CleanupBoundary::SourceMoved {
                    anyhow::bail!("synthetic interruption after owned quarantine move")
                }
                Ok(())
            },
        )
        .unwrap();
    let saved = service.finish_batch(&batch).unwrap();
    assert_eq!(saved.state, "saved");
    assert!(saved.can_recover && saved.error.is_some());
    let output = crate::organization::capture_regular_source(&path)
        .unwrap()
        .1;
    assert_ne!(output.sha256, before);
    let recovered = service
        .recover(&batch, &CancellationToken::default())
        .unwrap();
    assert_eq!(recovered.state, "saved");
    assert!(!recovered.can_recover);
    assert_eq!(recovered.error, None);
    let current = crate::organization::capture_regular_source(&path)
        .unwrap()
        .1;
    assert_eq!(
        current.identity, output.identity,
        "cleanup retry must not publish audio again"
    );
    assert_eq!(current.sha256, output.sha256);
    let database = service.library.data_dir().join("library.sqlite");
    let runtime = service.runtime.clone();
    drop(service);
    let reopened = MetadataEditService::new(Library::open(&database).unwrap(), runtime);
    let saved = reopened.batch(&batch).unwrap();
    assert_eq!(saved.error, None);
    assert!(!saved.can_recover);
    assert!(saved.can_undo);
}

#[test]
fn interrupted_unacknowledged_copy_recovery_uses_a_fresh_leaf_and_keeps_unproven_bytes() {
    let (_t, service, track, path) = fixture();
    let (batch, id) = reserved(&service, &track);
    let (_, write) = service.load_write(&id).unwrap();
    let artifact = write.artifact().unwrap();
    std::fs::copy(&artifact.staged, &artifact.publish).unwrap();
    let old = crate::organization::capture_regular_source(&artifact.publish)
        .unwrap()
        .1;
    let before = file_sha256(&path).unwrap();
    service.library.recover_metadata_edits().unwrap();
    let result = service
        .recover(&batch, &CancellationToken::default())
        .unwrap();
    assert_eq!(
        result.state, "saved",
        "a crash after staging before its receipt must not trap recovery forever: {result:?}"
    );
    let new = crate::organization::capture_regular_source(&path)
        .unwrap()
        .1;
    assert_ne!(
        new.identity, old.identity,
        "equal unproven staging must never be adopted"
    );
    assert_eq!(file_sha256(&artifact.publish).unwrap(), old.sha256);
    assert_ne!(file_sha256(&path).unwrap(), before);
}

#[test]
fn committed_cleanup_retains_a_same_byte_replaced_temporary_and_keeps_undo() {
    let (_t, service, track, path) = fixture();
    let (batch, id) = reserved(&service, &track);
    let (_, write) = service.load_write(&id).unwrap();
    let artifact = write.artifact().unwrap();
    let bytes = std::fs::read(&artifact.staged).unwrap();
    let original = file_sha256(&path).unwrap();
    let unknown = std::cell::RefCell::new(None);
    service
        .publish_write_with(&id, &CancellationToken::default(), || {
            let replacement = artifact.staged.with_extension("replacement");
            std::fs::write(&replacement, &bytes)?;
            std::fs::rename(replacement, &artifact.staged)?;
            *unknown.borrow_mut() =
                Some(crate::organization::capture_regular_source(&artifact.staged)?.1);
            Ok(())
        })
        .unwrap();
    let unknown = unknown.into_inner().unwrap();
    let error: Option<String> = service
        .library
        .conn()
        .unwrap()
        .query_row(
            "SELECT error FROM metadata_edit_writes WHERE id=?1",
            [&id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        error.as_deref().is_some_and(|e| e.contains("retained")),
        "unknown same-byte temporary needs visible pending cleanup: {error:?}"
    );
    assert!(
        !artifact.displaced.exists(),
        "an unknown temporary must not stop retirement of other proven redundant copies"
    );
    let saved = service.finish_batch(&batch).unwrap();
    assert_eq!(saved.state, "saved");
    assert!(
        saved.can_recover && saved.error.is_some(),
        "successful metadata with pending cleanup needs a visible retry action: {saved:?}"
    );
    let summary = service.history_page(0, 10).unwrap().batches.remove(0);
    assert!(summary.can_recover && summary.error.is_some());
    let result = service.undo(&batch, &CancellationToken::default()).unwrap();
    assert_eq!(
        result.state, "undone",
        "unknown stage must not invalidate the proven durable original: {result:?}"
    );
    assert_eq!(file_sha256(&path).unwrap(), original);
    let quarantine:Vec<u8>=service.library.conn().unwrap().query_row("SELECT quarantine_path FROM artifact_cleanup_moves WHERE owner_kind='metadata_edit' AND owner_id=?1 AND source_path=?2 AND state='retained'",params![id,path_bytes(&artifact.staged)],|r|r.get(0)).unwrap();
    let actual = crate::organization::capture_regular_source(&bytes_path(&quarantine).unwrap())
        .unwrap()
        .1;
    assert_eq!(actual.identity, unknown.identity);
    assert_eq!(actual.sha256, unknown.sha256);
}

#[test]
fn undo_journals_every_row_before_restoring_any_file_and_recovery_finishes_them() {
    let (temp, service, first, first_path) = fixture();
    let second_path = temp.path().canonicalize().unwrap().join("second.wav");
    let second = add_ready_track(&service.library, &second_path, 0.2);
    let original = [
        file_sha256(&first_path).unwrap(),
        file_sha256(&second_path).unwrap(),
    ];
    let original_tags: Vec<_> = service
        .snapshot(&[first.clone(), second.clone()])
        .unwrap()
        .tracks
        .into_iter()
        .map(|t| t.native)
        .collect();
    let draft = service
        .create_draft(&[first.clone(), second.clone()])
        .unwrap();
    let mut patch = MetadataEditorPatch::default();
    patch.fields.text.insert(
        MetadataTextField::Title,
        TextEdit::Set {
            values: vec!["Edited".into()],
        },
    );
    let draft = service
        .update_draft(&draft.id, draft.revision, patch)
        .unwrap();
    let preview = service
        .preview(&draft.id, draft.revision, &CancellationToken::default())
        .unwrap();
    let saved = service
        .apply(&preview.id, &CancellationToken::default())
        .unwrap();
    assert_eq!(saved.state, "saved", "{saved:?}");
    let edited = [
        file_sha256(&first_path).unwrap(),
        file_sha256(&second_path).unwrap(),
    ];
    assert_ne!(edited, original);

    let undo = service
        .journal_undo(&saved.id, &CancellationToken::default())
        .unwrap();
    let states: Vec<String> = {
        let conn = service.library.conn().unwrap();
        let mut q = conn
            .prepare("SELECT state FROM metadata_edit_rows WHERE batch_id=?1 ORDER BY ordinal")
            .unwrap();
        q.query_map([&undo], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    };
    assert_eq!(
        states,
        ["ready", "ready"],
        "every row is journaled up front"
    );
    let writes: i64 = service
        .library
        .conn()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM metadata_edit_writes WHERE batch_id=?1",
            [&undo],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(writes, 0);
    assert_eq!(file_sha256(&first_path).unwrap(), edited[0]);
    assert_eq!(file_sha256(&second_path).unwrap(), edited[1]);

    let root = service.library.data_dir().to_path_buf();
    let runtime = service.runtime.clone();
    drop(service);
    let lib = Library::open(&root.join("library.sqlite")).unwrap();
    lib.recover_metadata_edits().unwrap();
    let service = MetadataEditService::new(lib, runtime);
    let batch = service
        .recover(&undo, &CancellationToken::default())
        .unwrap();
    assert_eq!(batch.state, "undone", "{batch:?}");
    // Undo wrote the previous values back, so the files are not the original bytes.
    let restored_tags: Vec<_> = service
        .snapshot(&[first.clone(), second.clone()])
        .unwrap()
        .tracks
        .into_iter()
        .map(|t| t.native)
        .collect();
    assert_eq!(restored_tags, original_tags);
    let conn = service.library.conn().unwrap();
    let linked: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM metadata_edit_writes w JOIN metadata_edit_writes o ON o.id=w.undo_of WHERE w.batch_id=?1 AND o.batch_id=?2 AND o.state='undone'",
            [&undo, &saved.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        linked, 2,
        "resumed rows are recorded as undoing the original writes"
    );
    drop(conn);
    assert!(
        !service
            .history()
            .unwrap()
            .into_iter()
            .find(|b| b.id == saved.id)
            .unwrap()
            .can_undo
    );
}

/// A save from an earlier version still keeps its original for Undo, and the library backup
/// binds that file until the save is undone or leaves the Undo window.
#[test]
fn a_save_from_an_earlier_version_keeps_its_original_bound_to_the_library_backup() {
    let (temp, service, track, path) = fixture();
    let (batch, id) = reserved(&service, &track);
    service
        .publish_write(&id, &CancellationToken::default())
        .unwrap();
    assert_eq!(service.finish_batch(&batch).unwrap().state, "saved");
    // The fixture's preparation job never runs; a backup waits for preparation to finish.
    service
        .library
        .conn()
        .unwrap()
        .execute("UPDATE jobs SET state='completed'", [])
        .unwrap();
    let destination = temp.path().join("backups");
    let backup = service.library.create_library_backup(&destination).unwrap();
    let original = service
        .load_write(&id)
        .unwrap()
        .1
        .artifact()
        .unwrap()
        .backup
        .unwrap();
    assert!(
        original.exists(),
        "an earlier-version save keeps its original"
    );
    let bytes = std::fs::read(&original).unwrap();
    std::fs::write(&original, b"later external original edit").unwrap();
    assert!(
        service
            .library
            .inspect_library_backup(&backup.path)
            .is_err()
    );
    assert!(service.library.create_library_backup(&destination).is_err());
    std::fs::write(&original, bytes).unwrap();
    // Undone, the save no longer needs it: the original and its folder go.
    let undone = service.undo(&batch, &CancellationToken::default()).unwrap();
    assert_eq!(undone.state, "undone", "{undone:?}");
    assert!(!original.exists());
    assert!(!original.parent().unwrap().exists());
    assert!(service.library.create_library_backup(&destination).is_ok());
    assert!(path.exists());
}

/// A save interrupted after it was journaled but before its write was created is recovered
/// with the change it was asked to make, so it is undone from values and does not keep the
/// original audio.
#[test]
fn a_save_interrupted_before_its_write_exists_recovers_with_its_recorded_change() {
    let (_temp, service, track, path) = fixture();
    let draft = service.create_draft(std::slice::from_ref(&track)).unwrap();
    let mut patch = MetadataEditorPatch::default();
    patch.fields.text.insert(
        MetadataTextField::Title,
        TextEdit::Set {
            values: vec!["Recovered title".into()],
        },
    );
    let draft = service
        .update_draft(&draft.id, draft.revision, patch)
        .unwrap();
    let preview = service
        .preview(&draft.id, draft.revision, &CancellationToken::default())
        .unwrap();
    let original = file_sha256(&path).unwrap();
    let error = service.apply_linked_with(&preview.id, &CancellationToken::default(), None, || {
        anyhow::bail!("synthetic crash after the save was journaled")
    });
    assert!(error.is_err());
    let conn = service.library.conn().unwrap();
    let batch: String = conn
        .query_row("SELECT id FROM metadata_edit_batches", [], |r| r.get(0))
        .unwrap();
    let writes: i64 = conn
        .query_row("SELECT COUNT(*) FROM metadata_edit_writes", [], |r| {
            r.get(0)
        })
        .unwrap();
    drop(conn);
    assert_eq!(writes, 0, "the interruption came before any write");
    let journaled: Option<String> = service
        .library
        .conn()
        .unwrap()
        .query_row(
            "SELECT json_type(payload_json,'$.patch') FROM metadata_edit_rows",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(journaled.as_deref(), Some("object"));
    assert_eq!(file_sha256(&path).unwrap(), original);
    let root = service.library.data_dir().to_path_buf();
    let runtime = service.runtime.clone();
    drop(service);
    let lib = Library::open(&root.join("library.sqlite")).unwrap();
    lib.recover_metadata_edits().unwrap();
    let service = MetadataEditService::new(lib, runtime);
    let recovered = service
        .recover(&batch, &CancellationToken::default())
        .unwrap();
    assert_eq!(recovered.state, "saved", "{recovered:?}");
    let id: String = service
        .library
        .conn()
        .unwrap()
        .query_row("SELECT id FROM metadata_edit_writes", [], |r| r.get(0))
        .unwrap();
    let (_, write) = service.load_write(&id).unwrap();
    let recorded = write
        .editor_patch
        .as_ref()
        .expect("the recovered save records its change");
    assert!(recorded.fields.text.contains_key(&MetadataTextField::Title));
    assert!(write.backup_retired, "the original was not retired");
    let backup = write.artifact().unwrap().backup;
    assert!(
        backup.as_ref().is_none_or(|backup| !backup.exists()),
        "the original audio was kept"
    );
    assert_ne!(file_sha256(&path).unwrap(), original);
    // The patch kept for the unstarted row is gone once the write holds it.
    let kept: Option<String> = service
        .library
        .conn()
        .unwrap()
        .query_row(
            "SELECT json_type(payload_json,'$.patch') FROM metadata_edit_rows",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(kept, None);
    // And the save can be undone from the recorded values.
    let undone = service.undo(&batch, &CancellationToken::default()).unwrap();
    assert_eq!(undone.state, "undone", "{undone:?}");
}
