use super::*;
use crate::Library;
fn fixture() -> (
    tempfile::TempDir,
    CurationService,
    String,
    std::path::PathBuf,
    String,
) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let path = root.join("source.wav");
    let mut wav = hound::WavWriter::create(
        &path,
        hound::WavSpec {
            channels: 1,
            sample_rate: 44100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    for n in 0..44100 {
        wav.write_sample(((n as f64 * 0.1).sin() * 6000.) as i16)
            .unwrap();
    }
    wav.finalize().unwrap();
    let lib = Library::open(&root.join("data/library.sqlite")).unwrap();
    let job = lib
        .enqueue(
            std::slice::from_ref(&path),
            lib.settings().unwrap().revision,
        )
        .unwrap()
        .remove(0);
    let track = lib
        .jobs()
        .unwrap()
        .into_iter()
        .find(|row| row.id == job)
        .unwrap()
        .track_id;
    lib.conn().unwrap().execute("UPDATE tracks SET state='ready',analysis_json='{}',musical_key='8A',bpm=128,energy=7 WHERE id=?1",[&track]).unwrap();
    let runtime = cuerithm_media::MediaRuntime::from_bundle(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/media-runtime/bin"),
    )
    .unwrap();
    let service = CurationService::new(lib, Some(runtime));
    let group = edit(
        &service,
        CurationTaxonomyChange::CreateGroup {
            name: "Mood".into(),
            mode: CurationGroupMode::Multiple,
        },
    )
    .created_id
    .unwrap();
    let value = edit(
        &service,
        CurationTaxonomyChange::CreateValue {
            group_id: group.clone(),
            name: "Warm".into(),
        },
    )
    .created_id
    .unwrap();
    for (source, target) in [
        (
            CurationMappingSource::Group {
                group_id: group.clone(),
            },
            CurationMappingTarget::Text {
                field: CurationTextField::Grouping,
                encoding: CurationEncoding::Values,
            },
        ),
        (
            CurationMappingSource::Notes,
            CurationMappingTarget::Comment {
                language: "eng".into(),
                description: "CueRithm notes".into(),
                encoding: CurationEncoding::Values,
            },
        ),
    ] {
        edit(
            &service,
            CurationTaxonomyChange::PutMapping {
                mapping: CurationMapping {
                    id: uuid::Uuid::new_v4().to_string(),
                    source,
                    target,
                    containers: vec![CurationContainer::Id3],
                    enabled: true,
                    allow_id3_upgrade: false,
                },
            },
        );
    }
    update(
        &service,
        &track,
        CurationPatch {
            groups: vec![CurationGroupEdit::Add {
                group_id: group,
                value_ids: vec![value.clone()],
            }],
            notes: CurationValueEdit::Set {
                value: "first".into(),
            },
            ..Default::default()
        },
    );
    (temp, service, track, path, value)
}
fn edit(service: &CurationService, change: CurationTaxonomyChange) -> CurationTaxonomyReceipt {
    service
        .edit_taxonomy(CurationTaxonomyRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            expected_revision: service.catalog().unwrap().revision,
            change,
        })
        .unwrap()
}
fn update(service: &CurationService, track: &str, patch: CurationPatch) {
    let snapshot = service.snapshot(&[track.into()]).unwrap();
    service
        .update(CurationUpdateRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            taxonomy_revision: snapshot.taxonomy_revision,
            tracks: vec![CurationExpectedRevision {
                track_id: track.into(),
                revision: snapshot.tracks[0].desired_revision,
            }],
            patch,
        })
        .unwrap();
}
#[test]
fn older_native_ack_does_not_acknowledge_newer_durable_notes() {
    let (_temp, service, track, path, _) = fixture();
    let old = super::super::publication::claim(&service, &track, true)
        .unwrap()
        .unwrap();
    publish_with(&service, &old, &CancellationToken::default(), |boundary| {
        if boundary == NativeBoundary::ChildLinked {
            update(
                &service,
                &track,
                CurationPatch {
                    notes: CurationValueEdit::Set {
                        value: "second".into(),
                    },
                    ..Default::default()
                },
            );
        }
        Ok(())
    })
    .unwrap();
    let row = service
        .snapshot(std::slice::from_ref(&track))
        .unwrap()
        .tracks
        .remove(0);
    assert_eq!(row.desired_revision, 2);
    assert_eq!(row.acknowledged_revision, 1);
    assert_eq!(row.state, "pending");
    assert_eq!(row.desired.notes, "second");
    assert_eq!(row.committed.notes, "first");
    let result = service
        .flush_pending(20, &CancellationToken::default())
        .unwrap();
    assert_eq!(
        result.failed,
        0,
        "{:?}",
        service.snapshot(std::slice::from_ref(&track)).unwrap()
    );
    let row = service
        .snapshot(std::slice::from_ref(&track))
        .unwrap()
        .tracks
        .remove(0);
    assert_eq!(row.acknowledged_revision, 2);
    assert_eq!(row.state, "saved");
    let runtime = service.runtime.as_ref().unwrap();
    let native =
        cuerithm_autotag::read_metadata_snapshot(&path, &runtime.probe(&path).unwrap()).unwrap();
    assert_eq!(
        native
            .comments
            .iter()
            .find(|comment| comment.identity.description == "CueRithm notes")
            .unwrap()
            .values,
        vec!["second"]
    );
    assert_eq!(
        service
            .library
            .conn()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM metadata_edit_batches", [], |row| row
                .get::<_, u64>(
                0
            ))
            .unwrap(),
        2
    );
}
#[test]
fn crash_before_and_after_atomic_child_link_recovers_one_child() {
    for boundary in [NativeBoundary::PreviewSaved, NativeBoundary::ChildLinked] {
        let (_temp, service, track, path, _) = fixture();
        let original = std::fs::read(&path).unwrap();
        let old = super::super::publication::claim(&service, &track, true)
            .unwrap()
            .unwrap();
        assert!(
            publish_with(&service, &old, &CancellationToken::default(), |at| {
                if at == boundary {
                    anyhow::bail!("fault at durable Curate child boundary")
                }
                Ok(())
            })
            .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), original);
        let database = service.library.data_dir().join("library.sqlite");
        let runtime = service.runtime.clone();
        drop(service);
        let service = CurationService::new(Library::open(&database).unwrap(), runtime);
        let row = service
            .retry(&track, 1, &CancellationToken::default())
            .unwrap();
        assert_eq!(row.state, "saved", "{row:?}");
        assert_eq!(row.acknowledged_revision, 1);
        assert_eq!(
            service
                .library
                .conn()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM metadata_edit_batches", [], |row| row
                    .get::<_, u64>(
                    0
                ))
                .unwrap(),
            1
        );
        assert_eq!(service.history_page(0, 10).unwrap().total, 1);
    }
}
#[test]
fn stale_historical_recovery_routes_through_current_pending_revision() {
    let (_temp, service, track, _path, _) = fixture();
    let old = super::super::publication::claim(&service, &track, true)
        .unwrap()
        .unwrap();
    assert!(
        publish_with(&service, &old, &CancellationToken::default(), |at| {
            if at == NativeBoundary::ChildLinked {
                anyhow::bail!("stopped before native publication")
            }
            Ok(())
        })
        .is_err()
    );
    service.library.recover_curation().unwrap();
    update(
        &service,
        &track,
        CurationPatch {
            notes: CurationValueEdit::Set {
                value: "newer".into(),
            },
            ..Default::default()
        },
    );
    assert!(
        !service
            .publication(&old.public.summary.id)
            .unwrap()
            .summary
            .can_recover
    );
    let pending = service.pending_page(0, 10).unwrap().rows.remove(0);
    assert_eq!(pending.desired_revision, 2);
    assert!(pending.can_retry);
    let recovered = service
        .retry(&track, 2, &CancellationToken::default())
        .unwrap();
    assert_eq!(recovered.acknowledged_revision, 1);
    assert_eq!(recovered.desired_revision, 2);
    assert_eq!(recovered.state, "pending");
    service
        .flush_pending(20, &CancellationToken::default())
        .unwrap();
    assert_eq!(
        service.snapshot(&[track]).unwrap().tracks[0]
            .committed
            .notes,
        "newer"
    );
}
#[test]
fn rename_keeps_frozen_file_projection_and_requires_explicit_reexport() {
    let (_temp, service, track, path, value) = fixture();
    let old = super::super::publication::claim(&service, &track, true)
        .unwrap()
        .unwrap();
    edit(
        &service,
        CurationTaxonomyChange::RenameValue {
            id: value,
            name: "Bright".into(),
        },
    );
    publish(&service, &old, &CancellationToken::default()).unwrap();
    let row = service
        .snapshot(std::slice::from_ref(&track))
        .unwrap()
        .tracks
        .remove(0);
    assert!(row.native_stale);
    let runtime = service.runtime.as_ref().unwrap();
    assert_eq!(
        cuerithm_autotag::read_metadata_snapshot(&path, &runtime.probe(&path).unwrap())
            .unwrap()
            .text[&MetadataTextField::Grouping],
        vec!["Warm"]
    );
    update(
        &service,
        &track,
        CurationPatch {
            notes: CurationValueEdit::Set {
                value: "notes after rename".into(),
            },
            ..Default::default()
        },
    );
    service
        .flush_pending(20, &CancellationToken::default())
        .unwrap();
    assert_eq!(
        cuerithm_autotag::read_metadata_snapshot(&path, &runtime.probe(&path).unwrap())
            .unwrap()
            .text[&MetadataTextField::Grouping],
        vec!["Warm"],
        "editing notes must not implicitly export a renamed category"
    );
    let row = service
        .snapshot(std::slice::from_ref(&track))
        .unwrap()
        .tracks
        .remove(0);
    assert!(
        row.native_stale,
        "differential notes publication cannot acknowledge stale category mappings"
    );
    service
        .reexport(CurationReexportRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            taxonomy_revision: service.catalog().unwrap().revision,
            tracks: vec![CurationExpectedRevision {
                track_id: track.clone(),
                revision: row.desired_revision,
            }],
        })
        .unwrap();
    let result = service
        .flush_pending(20, &CancellationToken::default())
        .unwrap();
    assert_eq!(
        result.failed,
        0,
        "{:?}",
        service.snapshot(std::slice::from_ref(&track)).unwrap()
    );
    assert!(!service.snapshot(&[track]).unwrap().tracks[0].native_stale);
    assert_eq!(
        cuerithm_autotag::read_metadata_snapshot(&path, &runtime.probe(&path).unwrap())
            .unwrap()
            .text[&MetadataTextField::Grouping],
        vec!["Bright"]
    );
}

#[test]
fn newer_clear_reverses_the_inflight_native_projection() {
    let (_temp, service, track, path, value) = fixture();
    let group = service
        .catalog()
        .unwrap()
        .groups
        .into_iter()
        .find(|g| g.values.iter().any(|v| v.id == value))
        .unwrap()
        .id;
    let old = super::super::publication::claim(&service, &track, true)
        .unwrap()
        .unwrap();
    publish_with(&service, &old, &CancellationToken::default(), |at| {
        if at == NativeBoundary::ChildLinked {
            update(
                &service,
                &track,
                CurationPatch {
                    groups: vec![CurationGroupEdit::Clear {
                        group_id: group.clone(),
                    }],
                    ..Default::default()
                },
            );
        }
        Ok(())
    })
    .unwrap();
    let result = service
        .flush_pending(20, &CancellationToken::default())
        .unwrap();
    assert_eq!(result.failed, 0);
    let runtime = service.runtime.as_ref().unwrap();
    let native =
        cuerithm_autotag::read_metadata_snapshot(&path, &runtime.probe(&path).unwrap()).unwrap();
    assert!(
        native
            .text
            .get(&MetadataTextField::Grouping)
            .is_none_or(Vec::is_empty),
        "cleared library category must clear the in-flight file value: {native:?}"
    );
}

#[test]
fn interrupted_inverse_restores_native_values_from_one_durable_child() {
    use super::super::publication::{UndoBoundary, undo_with};
    for boundary in [UndoBoundary::IntentSaved, UndoBoundary::ChildLinked] {
        let (_temp, service, track, path, _) = fixture();
        service
            .flush_pending(20, &CancellationToken::default())
            .unwrap();
        let publication = service.history_page(0, 10).unwrap().rows.remove(0);
        assert!(publication.can_undo);
        assert!(
            undo_with(
                &service,
                &publication.id,
                &CancellationToken::default(),
                |at| {
                    if at == boundary {
                        anyhow::bail!("interrupted inverse boundary")
                    }
                    Ok(())
                }
            )
            .is_err()
        );
        let database = service.library.data_dir().join("library.sqlite");
        let runtime = service.runtime.clone();
        drop(service);
        let service = CurationService::new(Library::open(&database).unwrap(), runtime);
        let row = service
            .retry(&track, 2, &CancellationToken::default())
            .unwrap();
        assert_eq!(row.acknowledged_revision, 2, "{row:?}");
        assert!(row.committed.categories.is_empty());
        let runtime = service.runtime.as_ref().unwrap();
        let native =
            cuerithm_autotag::read_metadata_snapshot(&path, &runtime.probe(&path).unwrap())
                .unwrap();
        assert!(
            native
                .text
                .get(&MetadataTextField::Grouping)
                .is_none_or(Vec::is_empty),
            "Undo must restore file and library consistently: {native:?}"
        );
        assert_eq!(
            service
                .library
                .conn()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM metadata_edit_batches", [], |r| r
                    .get::<_, u64>(0))
                .unwrap(),
            2
        );
        assert_eq!(service.library.conn().unwrap().query_row("SELECT COUNT(*) FROM metadata_edit_rows r JOIN metadata_edit_batches b ON b.id=r.batch_id WHERE b.kind='undo'",[],|r|r.get::<_,u64>(0)).unwrap(),1);
    }
}

#[test]
fn archived_pending_track_does_not_starve_another_mapped_publication() {
    let (_temp, service, archived, path, _) = fixture();
    let second = path.with_file_name("second.wav");
    std::fs::copy(&path, &second).unwrap();
    let job = service
        .library
        .enqueue(
            std::slice::from_ref(&second),
            service.library.settings().unwrap().revision,
        )
        .unwrap()
        .remove(0);
    let track = service
        .library
        .jobs()
        .unwrap()
        .into_iter()
        .find(|j| j.id == job)
        .unwrap()
        .track_id;
    service
        .library
        .conn()
        .unwrap()
        .execute("UPDATE tracks SET state='ready' WHERE id=?1", [&track])
        .unwrap();
    update(
        &service,
        &track,
        CurationPatch {
            notes: CurationValueEdit::Set {
                value: "publish B".into(),
            },
            ..Default::default()
        },
    );
    service.library.set_archived(&archived, true).unwrap();
    service
        .library
        .conn()
        .unwrap()
        .execute(
            "UPDATE curation_tracks SET updated=0 WHERE track_id=?1",
            [&archived],
        )
        .unwrap();
    let outcome = service
        .flush_pending(20, &CancellationToken::default())
        .unwrap();
    assert_eq!((outcome.processed, outcome.failed), (1, 1));
    assert_eq!(
        service
            .snapshot(std::slice::from_ref(&track))
            .unwrap()
            .tracks[0]
            .state,
        "saved"
    );
    let pending = service.pending_page(0, 10).unwrap();
    assert_eq!(pending.total, 1);
    assert_eq!(pending.rows[0].track_id, archived);
    assert_eq!(pending.rows[0].state, "error");
    assert!(!pending.rows[0].can_retry);
    assert!(!pending.rows[0].can_discard);
    assert!(
        pending.rows[0]
            .error
            .as_deref()
            .unwrap()
            .contains("active Ready or Incoming")
    );
    let runtime = service.runtime.as_ref().unwrap();
    let native =
        cuerithm_autotag::read_metadata_snapshot(&second, &runtime.probe(&second).unwrap())
            .unwrap();
    assert!(native.comments.iter().any(|c| c.values == ["publish B"]));
    service.library.set_archived(&archived, false).unwrap();
    let restored = service
        .retry(&archived, 1, &CancellationToken::default())
        .unwrap();
    // Restore enqueues a fresh Incoming owner. The earlier native intent may
    // be discarded, but must not silently adopt that new preparation revision.
    assert_eq!(restored.state, "error", "{restored:?}");
    assert!(restored.can_discard);
    assert_eq!(service.discard(&archived, 1).unwrap().state, "saved");
}

#[test]
fn archived_active_child_keeps_evidence_but_hides_history_recovery() {
    let (_temp, service, track, _, _) = fixture();
    let publication = super::super::publication::claim(&service, &track, true)
        .unwrap()
        .unwrap();
    assert!(
        publish_with(
            &service,
            &publication,
            &CancellationToken::default(),
            |at| {
                if at == NativeBoundary::ChildLinked {
                    anyhow::bail!("retained child")
                }
                Ok(())
            }
        )
        .is_err()
    );
    service.library.recover_curation().unwrap();
    assert!(
        service
            .publication(&publication.public.summary.id)
            .unwrap()
            .summary
            .can_recover
    );
    service.library.set_archived(&track, true).unwrap();
    let history = service.publication(&publication.public.summary.id).unwrap();
    assert!(!history.summary.can_recover);
    assert!(history.child_batch_id.is_some());
    assert!(!service.pending_page(0, 10).unwrap().rows[0].can_retry);
}

#[test]
fn discard_stopped_unpublished_child_releases_only_owned_staging() {
    for boundary in [NativeBoundary::PreviewSaved, NativeBoundary::ChildLinked] {
        let (_temp, service, track, path, _) = fixture();
        let bytes = std::fs::read(&path).unwrap();
        let publication = super::super::publication::claim(&service, &track, true)
            .unwrap()
            .unwrap();
        assert!(
            publish_with(
                &service,
                &publication,
                &CancellationToken::default(),
                |at| {
                    if at == boundary {
                        anyhow::bail!("stopped before physical publication")
                    }
                    Ok(())
                }
            )
            .is_err()
        );
        service.library.recover_curation().unwrap();
        assert!(service.pending_page(0, 10).unwrap().rows[0].can_discard);
        let discarded = service.discard(&track, 1).unwrap();
        assert_eq!(discarded.state, "saved");
        assert_eq!(discarded.desired, discarded.committed);
        assert_eq!(service.pending_page(0, 10).unwrap().total, 0);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(
            service
                .publication(&publication.public.summary.id)
                .unwrap()
                .summary
                .state,
            "discarded"
        );
        assert_eq!(
            service
                .library
                .conn()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM metadata_edit_writes", [], |r| r
                    .get::<_, u64>(0))
                .unwrap(),
            0
        );
        let staging = service.library.data_dir().join("metadata-edits/staging");
        assert_eq!(
            std::fs::read_dir(staging).unwrap().count(),
            0,
            "unpublished owned preview audio must not accumulate"
        );
    }
}

#[test]
fn mapped_json_values_rating_owner_and_named_notes_preserve_other_native_records() {
    use cuerithm_autotag::{
        CommentEdit, CommentIdentity, RatingEdit, RatingIdentity, TextEdit, ValueEdit,
    };
    let (_temp, service, track, path, value) = fixture();
    service.discard(&track, 1).unwrap();
    let editor =
        crate::MetadataEditService::new(service.library.clone(), service.runtime.clone().unwrap());
    let draft = editor.create_draft(std::slice::from_ref(&track)).unwrap();
    let mut fields = MetadataPatch::default();
    fields.ratings.push(RatingEdit {
        identity: RatingIdentity {
            owner: "other DJ".into(),
        },
        value: ValueEdit::Set { value: 3 },
    });
    fields.comments.push(CommentEdit {
        identity: CommentIdentity {
            language: "eng".into(),
            description: "Other notes".into(),
            sha256: String::new(),
        },
        value: TextEdit::Set {
            values: vec!["keep other notes".into()],
        },
    });
    let draft = editor
        .update_draft(
            &draft.id,
            draft.revision,
            crate::MetadataEditorPatch {
                fields,
                ..Default::default()
            },
        )
        .unwrap();
    let preview = editor
        .preview(&draft.id, draft.revision, &CancellationToken::default())
        .unwrap();
    editor
        .apply(&preview.id, &CancellationToken::default())
        .unwrap();
    let before = editor
        .snapshot(std::slice::from_ref(&track))
        .unwrap()
        .tracks
        .remove(0)
        .native;
    let mut mapping = service
        .catalog()
        .unwrap()
        .mappings
        .into_iter()
        .find(|m| matches!(m.source, CurationMappingSource::Group { .. }))
        .unwrap();
    let CurationMappingSource::Group { group_id } = mapping.source.clone() else {
        unreachable!()
    };
    mapping.target = CurationMappingTarget::Text {
        field: CurationTextField::Grouping,
        encoding: CurationEncoding::Json,
    };
    edit(&service, CurationTaxonomyChange::PutMapping { mapping });
    let second = edit(
        &service,
        CurationTaxonomyChange::CreateValue {
            group_id: group_id.clone(),
            name: "Dark; / blue, 日本語".into(),
        },
    )
    .created_id
    .unwrap();
    edit(
        &service,
        CurationTaxonomyChange::PutMapping {
            mapping: CurationMapping {
                id: "user-rating".into(),
                source: CurationMappingSource::Rating,
                target: CurationMappingTarget::Rating {
                    owner: "Cuerithm".into(),
                },
                containers: vec![CurationContainer::Id3],
                enabled: true,
                allow_id3_upgrade: false,
            },
        },
    );
    update(
        &service,
        &track,
        CurationPatch {
            groups: vec![CurationGroupEdit::Set {
                group_id,
                value_ids: vec![value, second],
            }],
            user_rating: CurationValueEdit::Set { value: 5 },
            notes: CurationValueEdit::Set {
                value: "own notes".into(),
            },
        },
    );
    let outcome = service
        .flush_pending(20, &CancellationToken::default())
        .unwrap();
    assert_eq!(
        outcome.failed,
        0,
        "{:?}",
        service.pending_page(0, 10).unwrap()
    );
    let runtime = service.runtime.as_ref().unwrap();
    let after =
        cuerithm_autotag::read_metadata_snapshot(&path, &runtime.probe(&path).unwrap()).unwrap();
    let values: Vec<String> =
        serde_json::from_str(&after.text[&MetadataTextField::Grouping][0]).unwrap();
    assert_eq!(
        values
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>(),
        [String::from("Warm"), String::from("Dark; / blue, 日本語")]
            .into_iter()
            .collect::<std::collections::BTreeSet<String>>()
    );
    assert_eq!(
        after
            .ratings
            .iter()
            .find(|r| r.identity.owner == "other DJ"),
        before
            .ratings
            .iter()
            .find(|r| r.identity.owner == "other DJ")
    );
    assert_eq!(
        after
            .ratings
            .iter()
            .find(|r| r.identity.owner == "Cuerithm")
            .unwrap()
            .stars,
        Some(5)
    );
    assert_eq!(
        after
            .comments
            .iter()
            .find(|c| c.identity.description == "Other notes"),
        before
            .comments
            .iter()
            .find(|c| c.identity.description == "Other notes")
    );
    assert!(
        after
            .comments
            .iter()
            .any(|c| c.identity.description == "CueRithm notes" && c.values == ["own notes"])
    );
}

#[test]
fn inverse_blocked_before_any_file_write_can_be_discarded_without_adopting_changed_source() {
    use super::super::publication::{UndoBoundary, undo_with};
    let (_temp, service, track, path, _) = fixture();
    service
        .flush_pending(20, &CancellationToken::default())
        .unwrap();
    let original = service.history_page(0, 10).unwrap().rows.remove(0);
    let changed = b"external replacement after accepted Undo intent";
    assert!(
        undo_with(
            &service,
            &original.id,
            &CancellationToken::default(),
            |at| {
                if at == UndoBoundary::IntentSaved {
                    std::fs::write(&path, changed)?;
                }
                Ok(())
            }
        )
        .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), changed);
    assert_eq!(service.library.conn().unwrap().query_row("SELECT COUNT(*) FROM metadata_edit_writes w JOIN metadata_edit_batches b ON b.id=w.batch_id WHERE b.kind='undo'",[],|row|row.get::<_,u64>(0)).unwrap(),0);
    let pending = service.pending_page(0, 10).unwrap().rows.remove(0);
    assert!(
        pending.can_discard,
        "a stopped inverse with no physical write intent must be cancellable: {pending:?}"
    );
    let row = service.discard(&track, pending.desired_revision).unwrap();
    assert_eq!(row.state, "saved");
    assert_eq!(row.desired, row.committed);
    assert!(
        !row.committed.categories.is_empty(),
        "cancelled Undo retains the acknowledged Curate values"
    );
    assert_eq!(std::fs::read(&path).unwrap(), changed);
    assert_eq!(service.pending_page(0, 10).unwrap().total, 0);
    assert_eq!(
        service.publication(&original.id).unwrap().summary.state,
        "saved"
    );
}

#[test]
fn library_backup_preserves_mapped_curation_history_and_native_undo() {
    let (temp, service, track, path, _) = fixture();
    let before = std::fs::read(&path).unwrap();
    let tags = |service: &CurationService| {
        cuerithm_autotag::read_metadata(
            &path,
            &service.runtime.as_ref().unwrap().probe(&path).unwrap(),
        )
        .unwrap()
    };
    let tags_before = tags(&service);
    service
        .library
        .conn()
        .unwrap()
        .execute("UPDATE jobs SET state='completed'", [])
        .unwrap();
    let result = service
        .flush_pending(20, &CancellationToken::default())
        .unwrap();
    assert_eq!(result.failed, 0);
    let publication = service.history_page(0, 1).unwrap().rows.remove(0);
    assert!(publication.can_undo);
    let saved = std::fs::read(&path).unwrap();
    assert_ne!(saved, before);
    let database = service.library.data_dir().join("library.sqlite");
    let backup = service
        .library
        .create_library_backup(&temp.path().join("backups"))
        .unwrap();
    service
        .library
        .stage_library_restore(&backup.path, &backup.database_sha256)
        .unwrap();
    let runtime = service.runtime.clone();
    drop(service);
    assert!(matches!(
        crate::apply_pending_library_restore(&database).unwrap(),
        crate::LibraryRestoreStartup::Applied { .. }
    ));
    let service = CurationService::new(Library::open(&database).unwrap(), runtime);
    let restored = service.publication(&publication.id).unwrap();
    assert!(restored.summary.can_undo);
    assert_eq!(std::fs::read(&path).unwrap(), saved);
    service
        .undo(&publication.id, &CancellationToken::default())
        .unwrap();
    // Undo writes the previous tag values back; it has no copy of the original bytes.
    assert_eq!(tags(&service), tags_before);
    let state = service.snapshot(&[track]).unwrap().tracks.remove(0);
    assert!(state.committed.notes.is_empty());
}
