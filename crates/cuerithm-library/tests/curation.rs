use cuerithm_library::*;
use cuerithm_media::CancellationToken;
use cuerithm_smartlists::{Operator as SmartOperator, RuleValue as SmartRuleValue, SmartRule};
fn fixture() -> (
    tempfile::TempDir,
    Library,
    CurationService,
    String,
    std::path::PathBuf,
) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let path = root.join("source.flac");
    std::fs::write(
        &path,
        b"synthetic opaque music bytes; no decoder should read these for local curation",
    )
    .unwrap();
    let library = Library::open(&root.join("data/library.sqlite")).unwrap();
    let job = library
        .enqueue(
            std::slice::from_ref(&path),
            library.settings().unwrap().revision,
        )
        .unwrap()
        .remove(0);
    let track = library
        .jobs()
        .unwrap()
        .into_iter()
        .find(|jobrow| jobrow.id == job)
        .unwrap()
        .track_id;
    rusqlite::Connection::open(library.data_dir().join("library.sqlite")).unwrap().execute("UPDATE tracks SET state='ready',musical_key='8A',bpm=128,energy=7,analysis_json='{}' WHERE id=?1",[&track]).unwrap();
    let service = CurationService::new(library.clone(), None);
    (temp, library, service, track, path)
}
fn taxonomy(service: &CurationService, change: CurationTaxonomyChange) -> CurationTaxonomyReceipt {
    service
        .edit_taxonomy(CurationTaxonomyRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            expected_revision: service.catalog().unwrap().revision,
            change,
        })
        .unwrap()
}
#[test]
fn local_curate_is_durable_idempotent_and_never_opens_unavailable_audio() {
    let (_temp, library, service, track, path) = fixture();
    let group = taxonomy(
        &service,
        CurationTaxonomyChange::CreateGroup {
            name: "Mood".into(),
            mode: CurationGroupMode::Multiple,
        },
    )
    .created_id
    .unwrap();
    let value = taxonomy(
        &service,
        CurationTaxonomyChange::CreateValue {
            group_id: group.clone(),
            name: "Warm".into(),
        },
    )
    .created_id
    .unwrap();
    std::fs::remove_file(&path).unwrap();
    let before = service.snapshot(std::slice::from_ref(&track)).unwrap();
    assert_eq!(before.tracks[0].desired_revision, 0);
    let request = CurationUpdateRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        taxonomy_revision: service.catalog().unwrap().revision,
        tracks: vec![CurationExpectedRevision {
            track_id: track.clone(),
            revision: 0,
        }],
        patch: CurationPatch {
            groups: vec![CurationGroupEdit::Add {
                group_id: group.clone(),
                value_ids: vec![value.clone()],
            }],
            user_rating: CurationValueEdit::Set { value: 4 },
            notes: CurationValueEdit::Set {
                value: "warm opener".into(),
            },
        },
    };
    let receipt = service.update(request.clone()).unwrap();
    assert_eq!(receipt.accepted[0].revision, 1);
    assert_eq!(
        service.update(request.clone()).unwrap().accepted[0].revision,
        1
    );
    let mut stale = request;
    stale.request_id = uuid::Uuid::new_v4().to_string();
    assert!(service.update(stale).is_err());
    let pending = service.snapshot(std::slice::from_ref(&track)).unwrap();
    assert_eq!(pending.tracks[0].state, "pending");
    assert_eq!(pending.tracks[0].acknowledged_revision, 0);
    assert_eq!(
        pending.tracks[0].desired.categories[&group],
        vec![value.clone()]
    );
    assert_eq!(
        service
            .flush_pending(20, &CancellationToken::default())
            .unwrap()
            .processed,
        1
    );
    let saved = service.snapshot(std::slice::from_ref(&track)).unwrap();
    assert_eq!(saved.tracks[0].state, "saved");
    assert_eq!(saved.tracks[0].acknowledged_revision, 1);
    assert_eq!(saved.tracks[0].committed.user_rating, Some(4));
    assert_eq!(saved.tracks[0].committed.notes, "warm opener");
    assert_eq!(saved.tracks[0].measured, before.tracks[0].measured);
    assert!(!path.exists());
    assert_eq!(library.track(&track).unwrap().state, "ready");
    let database = library.data_dir().join("library.sqlite");
    drop(service);
    drop(library);
    let reopened = CurationService::new(Library::open(&database).unwrap(), None);
    assert_eq!(
        reopened.snapshot(&[track]).unwrap().tracks[0]
            .committed
            .categories[&group],
        vec![value]
    );
}

fn change(service: &CurationService, track: &str, patch: CurationPatch) -> CurationReceipt {
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
        .unwrap()
}
#[test]
fn queued_revisions_coalesce_and_single_groups_preserve_unrelated_values() {
    let (_temp, _lib, service, track, _path) = fixture();
    let mood = taxonomy(
        &service,
        CurationTaxonomyChange::CreateGroup {
            name: "Mood".into(),
            mode: CurationGroupMode::Multiple,
        },
    )
    .created_id
    .unwrap();
    let warm = taxonomy(
        &service,
        CurationTaxonomyChange::CreateValue {
            group_id: mood.clone(),
            name: "Warm".into(),
        },
    )
    .created_id
    .unwrap();
    let dark = taxonomy(
        &service,
        CurationTaxonomyChange::CreateValue {
            group_id: mood.clone(),
            name: "Dark".into(),
        },
    )
    .created_id
    .unwrap();
    for value in [&warm, &dark] {
        change(
            &service,
            &track,
            CurationPatch {
                groups: vec![CurationGroupEdit::Add {
                    group_id: mood.clone(),
                    value_ids: vec![value.clone()],
                }],
                ..Default::default()
            },
        );
    }
    service
        .flush_pending(20, &CancellationToken::default())
        .unwrap();
    assert_eq!(service.history_page(0, 10).unwrap().total, 1);
    assert_eq!(
        service
            .snapshot(std::slice::from_ref(&track))
            .unwrap()
            .tracks[0]
            .acknowledged_revision,
        2
    );
    assert!(
        service
            .edit_taxonomy(CurationTaxonomyRequest {
                request_id: uuid::Uuid::new_v4().to_string(),
                expected_revision: service.catalog().unwrap().revision,
                change: CurationTaxonomyChange::SetGroupMode {
                    id: mood.clone(),
                    mode: CurationGroupMode::Single
                }
            })
            .is_err()
    );
    change(
        &service,
        &track,
        CurationPatch {
            groups: vec![CurationGroupEdit::Set {
                group_id: mood.clone(),
                value_ids: vec![warm.clone()],
            }],
            ..Default::default()
        },
    );
    service
        .flush_pending(20, &CancellationToken::default())
        .unwrap();
    taxonomy(
        &service,
        CurationTaxonomyChange::SetGroupMode {
            id: mood.clone(),
            mode: CurationGroupMode::Single,
        },
    );
    change(
        &service,
        &track,
        CurationPatch {
            groups: vec![CurationGroupEdit::Add {
                group_id: mood.clone(),
                value_ids: vec![dark.clone()],
            }],
            notes: CurationValueEdit::Set {
                value: "retained note".into(),
            },
            ..Default::default()
        },
    );
    service
        .flush_pending(20, &CancellationToken::default())
        .unwrap();
    let row = service.snapshot(&[track]).unwrap().tracks.remove(0);
    assert_eq!(row.committed.categories[&mood], vec![dark]);
    assert_eq!(row.committed.notes, "retained note");
}
#[test]
fn discard_is_not_undo_and_later_edits_block_old_publication() {
    let (_temp, _lib, service, track, _path) = fixture();
    change(
        &service,
        &track,
        CurationPatch {
            notes: CurationValueEdit::Set {
                value: "saved note".into(),
            },
            ..Default::default()
        },
    );
    service
        .flush_pending(20, &CancellationToken::default())
        .unwrap();
    let original = service.history_page(0, 1).unwrap().rows.remove(0);
    change(
        &service,
        &track,
        CurationPatch {
            notes: CurationValueEdit::Set {
                value: "unpublished note".into(),
            },
            ..Default::default()
        },
    );
    let row = service
        .snapshot(std::slice::from_ref(&track))
        .unwrap()
        .tracks
        .remove(0);
    service.discard(&track, row.desired_revision).unwrap();
    assert_eq!(
        service
            .snapshot(std::slice::from_ref(&track))
            .unwrap()
            .tracks[0]
            .committed
            .notes,
        "saved note"
    );
    assert!(service.publication(&original.id).unwrap().summary.can_undo);
    assert_eq!(
        service
            .undo(&original.id, &CancellationToken::default())
            .unwrap()
            .summary
            .state,
        "undone"
    );
    assert_eq!(
        service
            .snapshot(std::slice::from_ref(&track))
            .unwrap()
            .tracks[0]
            .committed
            .notes,
        ""
    );
    change(
        &service,
        &track,
        CurationPatch {
            notes: CurationValueEdit::Set {
                value: "later".into(),
            },
            ..Default::default()
        },
    );
    service
        .flush_pending(20, &CancellationToken::default())
        .unwrap();
    assert!(
        service
            .undo(&original.id, &CancellationToken::default())
            .is_err()
    );
    assert_eq!(
        service.snapshot(&[track]).unwrap().tracks[0]
            .committed
            .notes,
        "later"
    );
}
#[test]
fn legacy_category_creation_invalidates_catalog_cas() {
    let (_temp, library, service, track, _path) = fixture();
    let catalog = service.catalog().unwrap();
    let conn = rusqlite::Connection::open(library.data_dir().join("library.sqlite")).unwrap();
    conn.execute(
        "INSERT INTO custom_tag_categories(id,name)VALUES('legacy','Existing category')",
        [],
    )
    .unwrap();
    conn.execute("INSERT INTO custom_tags(id,category_id,name)VALUES('legacy-value','legacy','Existing value')",[]).unwrap();
    assert!(service.catalog().unwrap().revision > catalog.revision);
    assert!(
        service
            .update(CurationUpdateRequest {
                request_id: uuid::Uuid::new_v4().to_string(),
                taxonomy_revision: catalog.revision,
                tracks: vec![CurationExpectedRevision {
                    track_id: track,
                    revision: 0
                }],
                patch: Default::default()
            })
            .is_err()
    );
    let current = service.catalog().unwrap();
    assert_eq!(current.groups[0].id, "legacy");
    assert_eq!(current.groups[0].values[0].id, "legacy-value");
}

#[test]
fn curate_filters_pages_select_all_and_smart_fields_share_committed_ids() {
    let (_temp, library, service, track, path) = fixture();
    let other = path.with_file_name("second.flac");
    std::fs::write(&other, b"different synthetic music bytes").unwrap();
    let job = library
        .enqueue(&[other], library.settings().unwrap().revision)
        .unwrap()
        .remove(0);
    let second = library
        .jobs()
        .unwrap()
        .into_iter()
        .find(|row| row.id == job)
        .unwrap()
        .track_id;
    let conn = rusqlite::Connection::open(library.data_dir().join("library.sqlite")).unwrap();
    conn.execute("UPDATE tracks SET state='ready' WHERE id=?1", [&second])
        .unwrap();
    let group = taxonomy(
        &service,
        CurationTaxonomyChange::CreateGroup {
            name: "Mood".into(),
            mode: CurationGroupMode::Multiple,
        },
    )
    .created_id
    .unwrap();
    let warm = taxonomy(
        &service,
        CurationTaxonomyChange::CreateValue {
            group_id: group.clone(),
            name: "Warm".into(),
        },
    )
    .created_id
    .unwrap();
    for id in [&track, &second] {
        change(
            &service,
            id,
            CurationPatch {
                groups: vec![CurationGroupEdit::Add {
                    group_id: group.clone(),
                    value_ids: vec![warm.clone()],
                }],
                user_rating: CurationValueEdit::Set { value: 4 },
                notes: CurationValueEdit::Set {
                    value: "日本語 midnight".into(),
                },
            },
        );
    }
    service
        .flush_pending(20, &CancellationToken::default())
        .unwrap();
    let query = TrackQuery {
        limit: 1,
        curation: Some(CurationFilter {
            all_tags: vec![warm.clone()],
            user_rating_min: Some(4),
            notes: "日本語".into(),
            ..Default::default()
        }),
        ..Default::default()
    };
    let first = library.tracks(query.clone()).unwrap();
    assert_eq!(first.total, 2);
    assert_eq!(first.rows[0].metadata["user_rating"], 4);
    assert_eq!(first.rows[0].metadata["curation_notes"], "日本語 midnight");
    assert_eq!(
        library
            .track_ids(TrackQuery {
                expected_revision: Some(first.revision),
                ..query.clone()
            })
            .unwrap()
            .len(),
        2
    );
    let second_page = library
        .tracks(TrackQuery {
            cursor: first.next_cursor.clone(),
            ..query.clone()
        })
        .unwrap();
    assert_eq!(second_page.rows.len(), 1);
    assert_ne!(first.rows[0].id, second_page.rows[0].id);
    change(
        &service,
        &track,
        CurationPatch {
            groups: vec![CurationGroupEdit::Remove {
                group_id: group.clone(),
                value_ids: vec![warm.clone()],
            }],
            ..Default::default()
        },
    );
    service
        .flush_pending(20, &CancellationToken::default())
        .unwrap();
    assert!(
        library
            .tracks(TrackQuery {
                cursor: first.next_cursor,
                ..query.clone()
            })
            .is_err()
    );
    assert!(
        library
            .track_ids(TrackQuery {
                expected_revision: Some(first.revision),
                ..query.clone()
            })
            .is_err()
    );
    assert_eq!(library.track_ids(query).unwrap(), vec![second]);
    assert_eq!(
        service
            .snapshot(std::slice::from_ref(&track))
            .unwrap()
            .tracks[0]
            .track_id,
        track,
        "filtered-out track remains addressable by stable selection ID"
    );
    let catalog = library.smart_field_catalog().unwrap();
    assert!(catalog.catalog.fields.contains_key("rating"));
    assert!(
        catalog.catalog.fields["user_rating"]
            .unavailable_reason
            .is_none()
    );
    assert!(
        catalog.catalog.fields["curation_notes"]
            .unavailable_reason
            .is_none()
    );
    let preview = library
        .preview_smart_list(
            SmartDefinition {
                match_all: true,
                rules: vec![SmartRule {
                    id: "curated".into(),
                    field: "user_rating".into(),
                    operator: SmartOperator::NumberEquals,
                    value: SmartRuleValue::Number(4.),
                    or: false,
                }],
            },
            SmartPreviewScope::default(),
            1_700_000_000_000,
        )
        .unwrap();
    assert_eq!(preview.track_ids.len(), 2);
}

#[test]
fn pending_and_history_pages_do_not_decode_unselected_payloads() {
    let (_temp, library, service, track, _) = fixture();
    change(
        &service,
        &track,
        CurationPatch {
            notes: CurationValueEdit::Set {
                value: "pending".into(),
            },
            ..Default::default()
        },
    );
    let conn = rusqlite::Connection::open(library.data_dir().join("library.sqlite")).unwrap();
    conn.execute(
        "UPDATE curation_tracks SET desired_json='malformed retained evidence' WHERE track_id=?1",
        [&track],
    )
    .unwrap();
    let page = service.pending_page(0, 1).unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.rows[0].track_id, track);
    assert!(service.snapshot(std::slice::from_ref(&track)).is_err());
    assert_eq!(service.discard(&track, 1).unwrap().state, "saved");
    assert_eq!(service.pending_page(0, 1).unwrap().total, 0);
    assert!(service.pending_page(0, 0).is_err());
}

#[cfg(feature = "autotag")]
#[test]
fn mapped_curate_reuses_editor_publication_and_guards_native_undo() {
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
    let library = Library::open(&root.join("data/library.sqlite")).unwrap();
    let job = library
        .enqueue(
            std::slice::from_ref(&path),
            library.settings().unwrap().revision,
        )
        .unwrap()
        .remove(0);
    let track = library
        .jobs()
        .unwrap()
        .into_iter()
        .find(|row| row.id == job)
        .unwrap()
        .track_id;
    let conn = rusqlite::Connection::open(library.data_dir().join("library.sqlite")).unwrap();
    conn.execute("UPDATE tracks SET state='ready',analysis_json='{}',musical_key='8A',bpm=128,energy=7 WHERE id=?1",[&track]).unwrap();
    let runtime = cuerithm_media::MediaRuntime::from_bundle(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/media-runtime/bin"),
    )
    .unwrap();
    let pcm = runtime
        .decode(&path, library.data_dir())
        .unwrap()
        .pcm_sha256()
        .unwrap();
    let service = CurationService::new(library.clone(), Some(runtime.clone()));
    let group = taxonomy(
        &service,
        CurationTaxonomyChange::CreateGroup {
            name: "Mood".into(),
            mode: CurationGroupMode::Multiple,
        },
    )
    .created_id
    .unwrap();
    let warm = taxonomy(
        &service,
        CurationTaxonomyChange::CreateValue {
            group_id: group.clone(),
            name: "Warm".into(),
        },
    )
    .created_id
    .unwrap();
    taxonomy(
        &service,
        CurationTaxonomyChange::PutMapping {
            mapping: CurationMapping {
                id: uuid::Uuid::new_v4().to_string(),
                source: CurationMappingSource::Group {
                    group_id: group.clone(),
                },
                target: CurationMappingTarget::Text {
                    field: CurationTextField::Grouping,
                    encoding: CurationEncoding::Values,
                },
                containers: vec![CurationContainer::Id3],
                enabled: true,
                allow_id3_upgrade: false,
            },
        },
    );
    change(
        &service,
        &track,
        CurationPatch {
            groups: vec![CurationGroupEdit::Add {
                group_id: group.clone(),
                value_ids: vec![warm.clone()],
            }],
            ..Default::default()
        },
    );
    let flushed = service
        .flush_pending(20, &CancellationToken::default())
        .unwrap();
    assert_eq!(
        flushed.failed,
        0,
        "{:?}",
        service.snapshot(std::slice::from_ref(&track)).unwrap()
    );
    assert_eq!(flushed.processed, 1);
    let saved = service
        .snapshot(std::slice::from_ref(&track))
        .unwrap()
        .tracks
        .remove(0);
    assert_eq!(saved.state, "saved");
    let native =
        cuerithm_autotag::read_metadata_snapshot(&path, &runtime.probe(&path).unwrap()).unwrap();
    assert_eq!(
        native.text[&cuerithm_autotag::MetadataTextField::Grouping],
        vec!["Warm"]
    );
    assert_eq!(
        runtime
            .decode(&path, library.data_dir())
            .unwrap()
            .pcm_sha256()
            .unwrap(),
        pcm
    );
    let publication = service.history_page(0, 1).unwrap().rows.remove(0);
    assert!(publication.native_required && publication.can_undo);
    let detail = service.publication(&publication.id).unwrap();
    assert!(detail.child_batch_id.is_some());
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM metadata_edit_batches", [], |row| row
            .get::<_, u64>(
            0
        ))
        .unwrap(),
        1
    );
    service
        .undo(&publication.id, &CancellationToken::default())
        .unwrap();
    // Undo writes the recorded tag values back; no copy of the original file is kept, so the
    // bytes of the container may differ while the tags and the audio are what they were.
    let restored =
        cuerithm_autotag::read_metadata_snapshot(&path, &runtime.probe(&path).unwrap()).unwrap();
    assert!(
        restored
            .text
            .get(&cuerithm_autotag::MetadataTextField::Grouping)
            .is_none_or(|values| values.is_empty())
    );
    assert_eq!(
        runtime
            .decode(&path, library.data_dir())
            .unwrap()
            .pcm_sha256()
            .unwrap(),
        pcm
    );
    assert!(
        service.snapshot(&[track]).unwrap().tracks[0]
            .committed
            .categories
            .is_empty()
    );
}

#[test]
fn later_legacy_assignments_survive_pending_notes_and_guarded_undo() {
    let (_temp, library, service, track, _) = fixture();
    change(
        &service,
        &track,
        CurationPatch {
            notes: CurationValueEdit::Set {
                value: "pending notes".into(),
            },
            ..Default::default()
        },
    );
    let group = taxonomy(
        &service,
        CurationTaxonomyChange::CreateGroup {
            name: "Legacy".into(),
            mode: CurationGroupMode::Multiple,
        },
    )
    .created_id
    .unwrap();
    let value = taxonomy(
        &service,
        CurationTaxonomyChange::CreateValue {
            group_id: group.clone(),
            name: "Preserved".into(),
        },
    )
    .created_id
    .unwrap();
    let sql = rusqlite::Connection::open(library.data_dir().join("library.sqlite")).unwrap();
    sql.execute(
        "INSERT INTO track_custom_tags VALUES(?1,?2)",
        rusqlite::params![track, value],
    )
    .unwrap();
    assert_eq!(
        service
            .snapshot(std::slice::from_ref(&track))
            .unwrap()
            .tracks[0]
            .desired
            .categories[&group],
        vec![value.clone()]
    );
    service
        .flush_pending(20, &CancellationToken::default())
        .unwrap();
    let publication = service.history_page(0, 10).unwrap().rows.remove(0);
    let detail = service.publication(&publication.id).unwrap();
    assert_eq!(detail.before.categories[&group], vec![value.clone()]);
    assert_eq!(detail.after.categories[&group], vec![value.clone()]);
    service
        .undo(&publication.id, &CancellationToken::default())
        .unwrap();
    let row = service
        .snapshot(std::slice::from_ref(&track))
        .unwrap()
        .tracks
        .remove(0);
    assert!(row.committed.notes.is_empty());
    assert_eq!(row.committed.categories[&group], vec![value]);
    assert_eq!(
        library.track(&track).unwrap().metadata["categories"][&group],
        serde_json::json!(row.committed.categories[&group])
    );
}

#[test]
fn pending_first_assignment_prevents_taxonomy_retirement() {
    let (_temp, _library, service, track, _) = fixture();
    let group = taxonomy(
        &service,
        CurationTaxonomyChange::CreateGroup {
            name: "Mood".into(),
            mode: CurationGroupMode::Single,
        },
    )
    .created_id
    .unwrap();
    let value = taxonomy(
        &service,
        CurationTaxonomyChange::CreateValue {
            group_id: group.clone(),
            name: "Warm".into(),
        },
    )
    .created_id
    .unwrap();
    change(
        &service,
        &track,
        CurationPatch {
            groups: vec![CurationGroupEdit::Set {
                group_id: group,
                value_ids: vec![value.clone()],
            }],
            ..Default::default()
        },
    );
    assert!(
        service
            .edit_taxonomy(CurationTaxonomyRequest {
                request_id: uuid::Uuid::new_v4().to_string(),
                expected_revision: service.catalog().unwrap().revision,
                change: CurationTaxonomyChange::DeleteValue {
                    id: value,
                    replacement_id: None
                }
            })
            .is_err()
    );
}

#[cfg(feature = "autotag")]
#[test]
fn incoming_mapped_curate_move_restart_undo_preserves_measured_identity() {
    use std::time::{Duration, Instant};
    for later_change in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let project = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let source = root.join("source.flac");
        std::fs::copy(
            project.join("crates/camelot-source-analysis/tests/fixtures/pulses64-stereo16.flac"),
            &source,
        )
        .unwrap();
        {
            let mut wrapper = onetagger_tag::Tag::load_file(&source, true).unwrap();
            let tag = wrapper.tag_mut();
            tag.set_native("TITLE", vec!["Original title".into()]);
            tag.set_native("ARTIST", vec!["Fixture artist".into()]);
            tag.set_native("OPAQUE_DJ_RECORD", vec!["keep me".into()]);
            tag.save_file(&source).unwrap();
        }
        let db = root.join("data/library.sqlite");
        let library = Library::open(&db).unwrap();
        let managed = root.join("managed");
        std::fs::create_dir(&managed).unwrap();
        let grant = library.register_organization_root(managed.clone()).unwrap();
        let model = library.data_dir().join("test-energy.json");
        std::fs::write(&model,serde_json::to_vec(&serde_json::json!({"gamma":0.000001,"classes":[3,8],"n_support":[1,1],"support_vectors":[vec![0.;183],vec![1.;183]],"dual_coef":[[1.,-1.]],"intercept":[0.1]})).unwrap()).unwrap();
        let mut settings = PreparationSettings {
            autotag_enabled: false,
            stability_millis: 10,
            organization: OrganizationSettings {
                mode: OrganizationSettingMode::Move,
                destination_root_id: Some(grant.id),
                ..Default::default()
            },
            ..Default::default()
        };
        settings.engine.energy_model = Some(model);
        let settings = library.save_settings(settings).unwrap();
        let job = library
            .enqueue(std::slice::from_ref(&source), settings)
            .unwrap()
            .remove(0);
        let track = library
            .jobs()
            .unwrap()
            .into_iter()
            .find(|j| j.id == job)
            .unwrap()
            .track_id;
        let runtime =
            cuerithm_media::MediaRuntime::from_bundle(&project.join("tools/media-runtime/bin"))
                .unwrap();
        let service = CurationService::new(library.clone(), Some(runtime.clone()));
        let group = taxonomy(
            &service,
            CurationTaxonomyChange::CreateGroup {
                name: "Mood".into(),
                mode: CurationGroupMode::Multiple,
            },
        )
        .created_id
        .unwrap();
        let value = taxonomy(
            &service,
            CurationTaxonomyChange::CreateValue {
                group_id: group.clone(),
                name: "Warm".into(),
            },
        )
        .created_id
        .unwrap();
        taxonomy(
            &service,
            CurationTaxonomyChange::PutMapping {
                mapping: CurationMapping {
                    id: uuid::Uuid::new_v4().to_string(),
                    source: CurationMappingSource::Group {
                        group_id: group.clone(),
                    },
                    target: CurationMappingTarget::Text {
                        field: CurationTextField::Grouping,
                        encoding: CurationEncoding::Values,
                    },
                    containers: vec![CurationContainer::Vorbis],
                    enabled: true,
                    allow_id3_upgrade: false,
                },
            },
        );
        change(
            &service,
            &track,
            CurationPatch {
                groups: vec![CurationGroupEdit::Set {
                    group_id: group.clone(),
                    value_ids: vec![value],
                }],
                user_rating: CurationValueEdit::Set { value: 4 },
                notes: CurationValueEdit::Set {
                    value: "my notes".into(),
                },
            },
        );
        let bytes = std::fs::read(&source).unwrap();
        let outcome = service
            .flush_pending(20, &CancellationToken::default())
            .unwrap();
        assert_eq!(outcome.failed, 0);
        assert_eq!(
            service
                .snapshot(std::slice::from_ref(&track))
                .unwrap()
                .tracks[0]
                .state,
            "waiting_incoming"
        );
        assert_eq!(std::fs::read(&source).unwrap(), bytes);
        let worker = PreparationService::start(library.clone(), runtime.clone()).unwrap();
        let start = Instant::now();
        let finished = loop {
            let current = library
                .jobs()
                .unwrap()
                .into_iter()
                .find(|j| j.id == job)
                .unwrap();
            if current.state == "completed"
                || current.state == "attention"
                || start.elapsed() > Duration::from_secs(90)
            {
                break current;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        worker.shutdown();
        drop(worker);
        assert_eq!(finished.state, "completed", "{finished:?}");
        let path = library.track_path(&track).unwrap();
        assert_eq!(path, managed.join("Fixture artist - Original title.flac"));
        assert!(!source.exists());
        assert_eq!(library.track(&track).unwrap().state, "ready");
        let sql = rusqlite::Connection::open(&db).unwrap();
        let measured:String=sql.query_row("SELECT json_array(analysis_json,musical_key,bpm,energy,duration) FROM tracks WHERE id=?1",[&track],|r|r.get(0)).unwrap();
        let cues:String=sql.query_row("SELECT json_group_array(json_array(id,job_id,json)) FROM cue_revisions WHERE track_id=?1",[&track],|r|r.get(0)).unwrap();
        assert_eq!(library.cue_revision_count(&track).unwrap(), 1);
        let pcm = runtime
            .decode(&path, library.data_dir())
            .unwrap()
            .pcm_sha256()
            .unwrap();
        let publication = service.history_page(0, 10).unwrap().rows.remove(0);
        drop(sql);
        drop(service);
        drop(library);
        let library = Library::open(&db).unwrap();
        let service = CurationService::new(library.clone(), Some(runtime.clone()));
        assert!(
            service
                .publication(&publication.id)
                .unwrap()
                .summary
                .can_undo
        );
        if later_change {
            let mut wrapper = onetagger_tag::Tag::load_file(&path, true).unwrap();
            wrapper
                .tag_mut()
                .set_native("COMPOSER", vec!["later external edit".into()]);
            wrapper.tag_mut().save_file(&path).unwrap();
            let changed = std::fs::read(&path).unwrap();
            assert!(
                service
                    .undo(&publication.id, &CancellationToken::default())
                    .is_err()
            );
            assert_eq!(std::fs::read(&path).unwrap(), changed);
        } else {
            service
                .undo(&publication.id, &CancellationToken::default())
                .unwrap();
            let row = service
                .snapshot(std::slice::from_ref(&track))
                .unwrap()
                .tracks
                .remove(0);
            assert!(row.committed.categories.is_empty());
            assert!(row.committed.user_rating.is_none());
            assert!(row.committed.notes.is_empty());
            let native =
                cuerithm_autotag::read_metadata_snapshot(&path, &runtime.probe(&path).unwrap())
                    .unwrap();
            assert!(
                native
                    .text
                    .get(&cuerithm_autotag::MetadataTextField::Grouping)
                    .is_none_or(Vec::is_empty)
            );
            assert_eq!(
                native.text[&cuerithm_autotag::MetadataTextField::Title],
                ["Original title"]
            );
            assert_eq!(
                runtime
                    .decode(&path, library.data_dir())
                    .unwrap()
                    .pcm_sha256()
                    .unwrap(),
                pcm
            );
            let sql = rusqlite::Connection::open(&db).unwrap();
            assert_eq!(sql.query_row("SELECT json_array(analysis_json,musical_key,bpm,energy,duration) FROM tracks WHERE id=?1",[&track],|r|r.get::<_,String>(0)).unwrap(),measured);
            assert_eq!(sql.query_row("SELECT json_group_array(json_array(id,job_id,json)) FROM cue_revisions WHERE track_id=?1",[&track],|r|r.get::<_,String>(0)).unwrap(),cues);
            assert_eq!(library.track_path(&track).unwrap(), path);
            assert_eq!(library.track(&track).unwrap().state, "ready");
            let wrapper = onetagger_tag::Tag::load_file(&path, true).unwrap();
            assert_eq!(
                wrapper.tag().get_raw("OPAQUE_DJ_RECORD").unwrap(),
                ["keep me"]
            );
        }
    }
}

#[test]
fn legacy_assignment_mapping_requires_discoverable_explicit_reexport_without_audio_reads() {
    let (_temp, library, service, track, path) = fixture();
    let sql = rusqlite::Connection::open(library.data_dir().join("library.sqlite")).unwrap();
    sql.execute(
        "INSERT INTO custom_tag_categories(id,name)VALUES('legacy-group','Legacy group')",
        [],
    )
    .unwrap();
    sql.execute(
        "INSERT INTO custom_tags(id,category_id,name)VALUES('legacy-value','legacy-group','Warm')",
        [],
    )
    .unwrap();
    sql.execute(
        "INSERT INTO track_custom_tags VALUES(?1,'legacy-value')",
        [&track],
    )
    .unwrap();
    std::fs::remove_file(path).unwrap();
    taxonomy(
        &service,
        CurationTaxonomyChange::PutMapping {
            mapping: CurationMapping {
                id: "legacy-map".into(),
                source: CurationMappingSource::Group {
                    group_id: "legacy-group".into(),
                },
                target: CurationMappingTarget::Text {
                    field: CurationTextField::Grouping,
                    encoding: CurationEncoding::Values,
                },
                containers: vec![CurationContainer::Id3],
                enabled: true,
                allow_id3_upgrade: false,
            },
        },
    );
    assert_eq!(
        sql.query_row("SELECT COUNT(*) FROM curation_tracks", [], |r| r
            .get::<_, u64>(0))
            .unwrap(),
        0
    );
    let snapshot = service.snapshot(std::slice::from_ref(&track)).unwrap();
    assert!(snapshot.tracks[0].native_stale);
    assert_eq!(
        snapshot.tracks[0].committed.categories["legacy-group"],
        ["legacy-value"]
    );
    let pending = service.pending_page(0, 10).unwrap();
    assert_eq!(pending.total, 1);
    assert_eq!(pending.rows[0].track_id, track);
    assert!(pending.rows[0].native_stale);
    assert_eq!(
        sql.query_row("SELECT COUNT(*) FROM metadata_edit_batches", [], |r| r
            .get::<_, u64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn pending_multiple_values_block_single_mode_and_mapping_collisions_are_rejected() {
    let (_temp, _library, service, track, _) = fixture();
    let group = taxonomy(
        &service,
        CurationTaxonomyChange::CreateGroup {
            name: "Mood".into(),
            mode: CurationGroupMode::Multiple,
        },
    )
    .created_id
    .unwrap();
    let mut values = Vec::new();
    for name in ["Warm", "Dark"] {
        values.push(
            taxonomy(
                &service,
                CurationTaxonomyChange::CreateValue {
                    group_id: group.clone(),
                    name: name.into(),
                },
            )
            .created_id
            .unwrap(),
        );
    }
    change(
        &service,
        &track,
        CurationPatch {
            groups: vec![CurationGroupEdit::Set {
                group_id: group.clone(),
                value_ids: values,
            }],
            ..Default::default()
        },
    );
    assert!(
        service
            .edit_taxonomy(CurationTaxonomyRequest {
                request_id: uuid::Uuid::new_v4().to_string(),
                expected_revision: service.catalog().unwrap().revision,
                change: CurationTaxonomyChange::SetGroupMode {
                    id: group.clone(),
                    mode: CurationGroupMode::Single
                }
            })
            .is_err()
    );
    let mapping = CurationMapping {
        id: "group-map".into(),
        source: CurationMappingSource::Group { group_id: group },
        target: CurationMappingTarget::Text {
            field: CurationTextField::Grouping,
            encoding: CurationEncoding::Json,
        },
        containers: vec![CurationContainer::Id3],
        enabled: true,
        allow_id3_upgrade: false,
    };
    taxonomy(
        &service,
        CurationTaxonomyChange::PutMapping {
            mapping: mapping.clone(),
        },
    );
    let mut collision = mapping;
    collision.id = "notes-map".into();
    collision.source = CurationMappingSource::Notes;
    assert!(
        service
            .edit_taxonomy(CurationTaxonomyRequest {
                request_id: uuid::Uuid::new_v4().to_string(),
                expected_revision: service.catalog().unwrap().revision,
                change: CurationTaxonomyChange::PutMapping { mapping: collision }
            })
            .is_err()
    );
}
