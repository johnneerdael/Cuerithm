#![cfg(feature = "autotag")]
use cuerithm_autotag::{MetadataTextField, TextEdit};
use cuerithm_library::*;
use cuerithm_media::{CancellationToken, MediaRuntime};
use std::path::Path;
fn fixture() -> (
    tempfile::TempDir,
    Library,
    MetadataEditService,
    String,
    std::path::PathBuf,
) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let path = root.join("source.wav");
    let mut writer = hound::WavWriter::create(
        &path,
        hound::WavSpec {
            channels: 2,
            sample_rate: 44100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    for n in 0..44100 {
        let value = (((n as f64 * 440. * std::f64::consts::TAU / 44100.).sin()) * 8000.) as i16;
        writer.write_sample(value).unwrap();
        writer.write_sample(value).unwrap();
    }
    writer.finalize().unwrap();
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
        .find(|j| j.id == job)
        .unwrap()
        .track_id;
    let conn = rusqlite::Connection::open(root.join("data/library.sqlite")).unwrap();
    conn.execute("UPDATE tracks SET state='ready',analysis_json='{}',musical_key='8A',bpm=128,energy=7 WHERE id=?1",[&track]).unwrap();
    let runtime = MediaRuntime::from_bundle(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/media-runtime/bin"),
    )
    .unwrap();
    let service = MetadataEditService::new(lib.clone(), runtime);
    (temp, lib, service, track, path)
}
#[test]
fn durable_metadata_draft_previews_saves_and_guardedly_undoes_without_reanalyzing() {
    let (_temp, lib, service, track, path) = fixture();
    assert_eq!(lib.tagging_storage().unwrap().original_files, 0);
    let conn = rusqlite::Connection::open(lib.data_dir().join("library.sqlite")).unwrap();
    conn.execute("UPDATE tracks SET artist='Display artist',album='Display album',genre='Display genre' WHERE id=?1",[&track]).unwrap();
    let original_display: (String, String, String, String) = conn
        .query_row(
            "SELECT title,artist,album,genre FROM tracks WHERE id=?1",
            [&track],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    drop(conn);
    let original_title = service
        .snapshot(std::slice::from_ref(&track))
        .unwrap()
        .tracks[0]
        .native
        .text
        .get(&MetadataTextField::Title)
        .cloned();
    let draft = service.create_draft(std::slice::from_ref(&track)).unwrap();
    let mut patch = MetadataEditorPatch::default();
    patch.fields.text.insert(
        MetadataTextField::Title,
        TextEdit::Set {
            values: vec!["Reviewed title".into()],
        },
    );
    let draft = service
        .update_draft(&draft.id, draft.revision, patch)
        .unwrap();
    assert_eq!(service.draft(&draft.id).unwrap().revision, draft.revision);
    let preview = service
        .preview(&draft.id, draft.revision, &CancellationToken::default())
        .unwrap();
    assert!(preview.can_apply);
    let saved = service
        .apply(&preview.id, &CancellationToken::default())
        .unwrap();
    assert!(
        saved
            .rows
            .iter()
            .any(|r| matches!(r.outcome.as_str(), "written" | "queued")),
        "{saved:?}"
    );
    assert!(path.exists());
    assert_eq!(saved.state, "saved");
    assert!(saved.can_undo);
    // A save keeps no copy of the original audio once it has settled.
    let retained = lib.tagging_storage().unwrap();
    assert_eq!((retained.original_files, retained.original_bytes), (0, 0));
    assert!(
        walkdir::WalkDir::new(lib.data_dir().join("metadata-edits"))
            .into_iter()
            .filter_map(|e| e.ok())
            .all(|e| !e.file_type().is_file()),
        "no file of the audio is left in the metadata folder"
    );
    let audio_before = pcm(&service, &path);
    assert_eq!(service.draft(&draft.id).unwrap().state, "saved");
    let snapshot = service.snapshot(std::slice::from_ref(&track)).unwrap();
    assert_eq!(
        snapshot.tracks[0].native.text[&MetadataTextField::Title],
        vec!["Reviewed title"]
    );
    assert_eq!(snapshot.tracks[0].measured["key"], "8A");
    assert_eq!(snapshot.tracks[0].measured["bpm"], 128.);
    assert_eq!(snapshot.tracks[0].measured["energy"], 7);
    assert_eq!(snapshot.tracks[0].state, "ready");
    let undone = service
        .undo(&saved.id, &CancellationToken::default())
        .unwrap();
    assert_eq!(undone.state, "undone", "{undone:?}");
    // Undo wrote the previous values back: the title and the audio are what they were.
    assert_eq!(
        service
            .snapshot(std::slice::from_ref(&track))
            .unwrap()
            .tracks[0]
            .native
            .text
            .get(&MetadataTextField::Title),
        original_title.as_ref()
    );
    assert_eq!(pcm(&service, &path), audio_before);
    let retained = lib.tagging_storage().unwrap();
    assert_eq!((retained.original_files, retained.original_bytes), (0, 0));
    assert!(
        !service
            .history()
            .unwrap()
            .into_iter()
            .find(|b| b.id == saved.id)
            .unwrap()
            .can_undo
    );
    let conn = rusqlite::Connection::open(lib.data_dir().join("library.sqlite")).unwrap();
    let display: (String, String, String, String) = conn
        .query_row(
            "SELECT title,artist,album,genre FROM tracks WHERE id=?1",
            [&track],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(display, original_display);
}

/// The decoded audio of a file, whatever its tags.
fn pcm(service: &MetadataEditService, path: &Path) -> String {
    let _ = service;
    let runtime = MediaRuntime::from_bundle(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/media-runtime/bin"),
    )
    .unwrap();
    let cache = tempfile::tempdir().unwrap();
    runtime
        .decode(path, cache.path())
        .unwrap()
        .pcm_sha256()
        .unwrap()
}

fn title_preview(service: &MetadataEditService, track: &str) -> MetadataEditPreview {
    let d = service.create_draft(&[track.into()]).unwrap();
    let mut p = MetadataEditorPatch::default();
    p.fields.text.insert(
        MetadataTextField::Title,
        TextEdit::Set {
            values: vec!["Edited".into()],
        },
    );
    let d = service.update_draft(&d.id, d.revision, p).unwrap();
    service
        .preview(&d.id, d.revision, &CancellationToken::default())
        .unwrap()
}
#[test]
fn stale_preview_and_external_edits_never_overwrite_new_bytes() {
    use std::io::Write;
    let (_t, _lib, service, track, path) = fixture();
    let preview = title_preview(&service, &track);
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    file.write_all(b"external revision").unwrap();
    drop(file);
    let changed = std::fs::read(&path).unwrap();
    assert!(
        service
            .apply(&preview.id, &CancellationToken::default())
            .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), changed);
}
#[test]
fn guarded_undo_rejects_later_source_change_and_retains_it() {
    use std::io::Write;
    let (_t, _lib, service, track, path) = fixture();
    let preview = title_preview(&service, &track);
    let saved = service
        .apply(&preview.id, &CancellationToken::default())
        .unwrap();
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"new external bytes")
        .unwrap();
    let changed = std::fs::read(&path).unwrap();
    let undo = service
        .undo(&saved.id, &CancellationToken::default())
        .unwrap();
    assert_eq!(undo.state, "partial");
    assert_eq!(undo.rows[0].outcome, "failed");
    assert_eq!(std::fs::read(&path).unwrap(), changed);
}
#[test]
fn optimistic_drafts_persist_invalid_input_and_invalidate_old_preview() {
    let (_t, lib, service, track, _path) = fixture();
    let preview = title_preview(&service, &track);
    let draft = service.draft(&preview.draft_id).unwrap();
    let mut patch = draft.patch.clone();
    patch
        .form_values
        .insert("release_date".into(), "2026-".into());
    let updated = service
        .update_draft(&draft.id, draft.revision, patch)
        .unwrap();
    assert!(
        service
            .update_draft(&draft.id, draft.revision, updated.patch.clone())
            .is_err()
    );
    assert!(
        service
            .preview(&draft.id, updated.revision, &CancellationToken::default())
            .is_err()
    );
    assert!(
        service
            .apply(&preview.id, &CancellationToken::default())
            .is_err()
    );
    let database = lib.data_dir().join("library.sqlite");
    drop(service);
    drop(lib);
    let reopened = Library::open(&database).unwrap();
    let runtime = MediaRuntime::from_bundle(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/media-runtime/bin"),
    )
    .unwrap();
    assert_eq!(
        MetadataEditService::new(reopened, runtime)
            .draft(&draft.id)
            .unwrap()
            .patch
            .form_values["release_date"],
        "2026-"
    );
}
#[test]
fn incoming_edit_queues_same_logical_job_without_writing_source() {
    let (_t, lib, service, track, path) = fixture();
    let conn = rusqlite::Connection::open(lib.data_dir().join("library.sqlite")).unwrap();
    conn.execute("UPDATE tracks SET state='incoming' WHERE id=?1", [&track])
        .unwrap();
    let before = std::fs::read(&path).unwrap();
    let job: String = conn
        .query_row(
            "SELECT current_job FROM tracks WHERE id=?1",
            [&track],
            |r| r.get(0),
        )
        .unwrap();
    let preview = title_preview(&service, &track);
    let result = service
        .apply(&preview.id, &CancellationToken::default())
        .unwrap();
    assert_eq!(result.state, "queued", "{result:?}");
    assert_eq!(result.rows[0].outcome, "queued");
    let directory = staging_dirs(&lib).pop().unwrap();
    let files = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    assert_eq!(
        files.len(),
        1,
        "accepted Incoming keeps its Undo original but retires the redundant preview output"
    );
    assert_eq!(std::fs::read(&files[0]).unwrap(), before);
    let storage = lib.tagging_storage().unwrap();
    assert_eq!(
        storage.original_files, 1,
        "queued Incoming original is accounted before any worker/tag journal"
    );
    assert_eq!(storage.original_bytes, before.len() as u64);
    assert_eq!(std::fs::read(path).unwrap(), before);
    let queued: String = conn
        .query_row(
            "SELECT current_job FROM tracks WHERE id=?1",
            [&track],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(queued, job);
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM metadata_edit_overrides WHERE job_id=?1 AND state='queued'",
            [job],
            |r| r.get::<_, u64>(0)
        )
        .unwrap(),
        1
    );
}

#[test]
fn a_newer_incoming_edit_retires_the_original_kept_for_the_one_it_replaces() {
    let (_t, lib, service, track, path) = fixture();
    let conn = rusqlite::Connection::open(lib.data_dir().join("library.sqlite")).unwrap();
    conn.execute("UPDATE tracks SET state='incoming' WHERE id=?1", [&track])
        .unwrap();
    let before = std::fs::read(&path).unwrap();
    let first = service
        .apply(
            &title_preview(&service, &track).id,
            &CancellationToken::default(),
        )
        .unwrap();
    assert_eq!(first.state, "queued", "{first:?}");
    let old_dirs = staging_dirs(&lib);
    assert_eq!(old_dirs.len(), 1);
    let second = service
        .apply(
            &title_preview(&service, &track).id,
            &CancellationToken::default(),
        )
        .unwrap();
    assert_eq!(second.state, "queued", "{second:?}");
    let replaced: String = conn
        .query_row(
            "SELECT state FROM metadata_edit_rows WHERE batch_id=?1",
            [&first.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(replaced, "superseded");
    assert!(
        !old_dirs[0].exists(),
        "the replaced edit's retained original and workspace are removed"
    );
    assert_eq!(
        staging_dirs(&lib).len(),
        1,
        "only the newest edit keeps one"
    );
    let storage = lib.tagging_storage().unwrap();
    assert_eq!(storage.original_files, 1);
    assert_eq!(storage.original_bytes, before.len() as u64);
}
#[test]
fn a_saved_preview_is_not_kept_and_cannot_be_saved_twice() {
    let (_t, lib, service, track, _path) = fixture();
    let preview = title_preview(&service, &track);
    let saved = service
        .apply(&preview.id, &CancellationToken::default())
        .unwrap();
    assert_eq!(saved.state, "saved", "{saved:?}");
    let conn = rusqlite::Connection::open(lib.data_dir().join("library.sqlite")).unwrap();
    let kept: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM metadata_edit_previews WHERE id=?1",
            [&preview.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        kept, 0,
        "the saved batch holds the plan; the preview copy is dropped"
    );
    let again = service
        .apply(&preview.id, &CancellationToken::default())
        .unwrap_err()
        .to_string();
    assert!(again.contains("already applied"), "{again}");
    let undone = service
        .undo(&saved.id, &CancellationToken::default())
        .unwrap();
    assert_eq!(undone.state, "undone", "{undone:?}");
}
fn staging_dirs(lib: &Library) -> Vec<std::path::PathBuf> {
    let root = lib.data_dir().join("metadata-edits/staging");
    if !root.exists() {
        return vec![];
    }
    std::fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir())
        .collect()
}
#[test]
fn unpublished_preview_audio_is_retired_on_replace_edit_discard_and_restart() {
    let (_t, lib, service, track, _path) = fixture();
    let first = title_preview(&service, &track);
    let dirs = staging_dirs(&lib);
    assert_eq!(dirs.len(), 1, "{dirs:?}");
    let second = service
        .preview(
            &first.draft_id,
            first.draft_revision,
            &CancellationToken::default(),
        )
        .unwrap();
    assert_ne!(first.id, second.id);
    assert!(
        !dirs[0].exists(),
        "unretired preview entries: {:?}; cleanup receipts: {:?}",
        std::fs::read_dir(&dirs[0]).map(|entries| entries
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>()),
        {
            let conn = rusqlite::Connection::open(lib.data_dir().join("library.sqlite")).unwrap();
            let mut query = conn
                .prepare("SELECT state,error FROM artifact_cleanup_moves")
                .unwrap();
            query
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
                })
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap()
        }
    );
    assert_eq!(staging_dirs(&lib).len(), 1);
    let d = service.draft(&second.draft_id).unwrap();
    let d = service.update_draft(&d.id, d.revision, d.patch).unwrap();
    assert!(staging_dirs(&lib).is_empty());
    service
        .preview(&d.id, d.revision, &CancellationToken::default())
        .unwrap();
    service.discard_draft(&d.id, d.revision).unwrap();
    assert!(staging_dirs(&lib).is_empty());
    let _last = title_preview(&service, &track);
    assert_eq!(staging_dirs(&lib).len(), 1);
    let database = lib.data_dir().join("library.sqlite");
    drop(service);
    drop(lib);
    let reopened = Library::open(&database).unwrap();
    assert!(staging_dirs(&reopened).is_empty());
}
#[test]
fn history_summaries_are_sql_paged_without_decoding_full_rows() {
    let (_t, lib, service, track, _path) = fixture();
    let mut ids = vec![];
    for n in 0..3 {
        let draft = service.create_draft(std::slice::from_ref(&track)).unwrap();
        let mut patch = MetadataEditorPatch::default();
        patch.fields.text.insert(
            MetadataTextField::Title,
            TextEdit::Set {
                values: vec![format!("Revision{n}")],
            },
        );
        let draft = service
            .update_draft(&draft.id, draft.revision, patch)
            .unwrap();
        let preview = service
            .preview(&draft.id, draft.revision, &CancellationToken::default())
            .unwrap();
        ids.push(
            service
                .apply(&preview.id, &CancellationToken::default())
                .unwrap()
                .id,
        );
    }
    let conn = rusqlite::Connection::open(lib.data_dir().join("library.sqlite")).unwrap();
    conn.execute("UPDATE metadata_edit_rows SET payload_json='malformed off-page evidence' WHERE batch_id=?1",[&ids[0]]).unwrap();
    let page = service.history_page(0, 1).unwrap();
    assert_eq!(
        (page.total, page.offset, page.limit, page.batches.len()),
        (3, 0, 1, 1)
    );
    assert_eq!(page.batches[0].id, ids[2]);
    assert_eq!(page.batches[0].counts["written"], 1);
    assert!(page.batches[0].can_undo);
    let older = service.history_page(2, 1).unwrap();
    assert_eq!(older.batches[0].id, ids[0]);
    assert!(!older.batches[0].can_undo);
    assert!(service.batch(&ids[0]).is_err());
    assert_eq!(service.batch(&ids[2]).unwrap().rows.len(), 1);
}

#[test]
fn drafts_keep_pending_work_discoverable_beyond_completed_history_budget() {
    let (_temp, lib, service, track, _path) = fixture();
    let preview = title_preview(&service, &track);
    let saved = service
        .apply(&preview.id, &CancellationToken::default())
        .unwrap();
    let template = service.draft(&saved.draft_id).unwrap();
    assert_eq!(template.state, "saved");
    let mut large = template.clone();
    large.patch.fields.text.insert(
        MetadataTextField::Title,
        TextEdit::Set {
            values: vec!["x".repeat(1024 * 1024)],
        },
    );
    large.patch.fields.validate().unwrap();
    let mut conn = rusqlite::Connection::open(lib.data_dir().join("library.sqlite")).unwrap();
    let transaction = conn.transaction().unwrap();
    let mut bytes = 0;
    for _ in 0..65 {
        let mut draft = large.clone();
        draft.id = uuid::Uuid::new_v4().to_string();
        let json = serde_json::to_string(&draft).unwrap();
        assert!(json.len() < 2 * 1024 * 1024);
        bytes += json.len();
        transaction
            .execute(
                "INSERT INTO metadata_edit_drafts VALUES(?1,?2,?3,?4,?5,?6)",
                rusqlite::params![
                    draft.id,
                    draft.revision,
                    draft.state,
                    json,
                    draft.created,
                    draft.updated
                ],
            )
            .unwrap();
    }
    transaction.commit().unwrap();
    assert!(bytes > 64 * 1024 * 1024);
    let pending = service.create_draft(std::slice::from_ref(&track)).unwrap();
    let mut patch = MetadataEditorPatch::default();
    patch
        .form_values
        .insert("release_date".into(), "2026-".into());
    let pending = service
        .update_draft(&pending.id, pending.revision, patch)
        .unwrap();
    let drafts = service
        .drafts()
        .expect("completed history must not exhaust the recovery-list budget");
    assert_eq!(drafts.len(), 1);
    assert_eq!(drafts[0].id, pending.id);
    assert_eq!(drafts[0].patch.form_values["release_date"], "2026-");
    assert_eq!(service.draft(&template.id).unwrap().state, "saved");
    assert_eq!(service.history_page(0, 10).unwrap().batches[0].id, saved.id);
}

#[test]
fn incoming_edit_move_restart_and_guarded_undo_preserve_analysis_cues_and_later_edits() {
    use cuerithm_autotag::{
        AnalysisTagSource, AutotagConfig, CatalogMatch, CatalogTrack, ProviderBackend, ProviderId,
        RatingEdit, RatingIdentity, SupportedTag, TagInput, Tagger, ValueEdit,
    };
    use std::{
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::{Duration, Instant},
    };
    struct Catalog(AtomicUsize);
    impl ProviderBackend for Catalog {
        fn id(&self) -> ProviderId {
            ProviderId::Beatport
        }
        fn candidates(&self, _: &TagInput, _: &AutotagConfig) -> anyhow::Result<Vec<CatalogMatch>> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(vec![CatalogMatch::new(
                1.,
                CatalogTrack {
                    platform: "beatport".into(),
                    title: "Catalog title".into(),
                    artists: vec!["Catalog artist".into()],
                    album: Some("Catalog album".into()),
                    genres: vec!["House".into()],
                    key: Some("F#m".into()),
                    bpm: Some(199),
                    ..Default::default()
                },
            )])
        }
    }
    for later_edit in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let folder = temp.path().canonicalize().unwrap();
        let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let source = folder.join("source.flac");
        std::fs::copy(
            project.join("crates/camelot-source-analysis/tests/fixtures/pulses64-stereo16.flac"),
            &source,
        )
        .unwrap();
        {
            let mut wrapper = onetagger_tag::Tag::load_file(&source, true).unwrap();
            let tag = wrapper.tag_mut();
            tag.set_native("TITLE", vec!["Original title".into()]);
            tag.set_native("RATING", vec!["47".into()]);
            tag.set_native("OPAQUE_DJ_RECORD", vec!["preserve native record".into()]);
            tag.set_art(
                onetagger_tag::CoverType::CoverFront,
                "image/png",
                None,
                icon("32x32.png"),
            );
            tag.save_file(&source).unwrap();
        }
        let db = folder.join("data/library.sqlite");
        let lib = Library::open(&db).unwrap();
        let managed = folder.join("managed");
        std::fs::create_dir(&managed).unwrap();
        let grant = lib.register_organization_root(managed.clone()).unwrap();
        let model = lib.data_dir().join("test-energy.json");
        std::fs::write(&model,serde_json::to_vec(&serde_json::json!({"gamma":0.000001,"classes":[3,8],"n_support":[1,1],"support_vectors":[vec![0.;183],vec![1.;183]],"dual_coef":[[1.,-1.]],"intercept":[0.1]})).unwrap()).unwrap();
        let mut config = AutotagConfig::default();
        config.options.platforms = vec!["beatport".into()];
        config.options.tags = vec![
            SupportedTag::Title,
            SupportedTag::Artist,
            SupportedTag::Album,
            SupportedTag::Genre,
            SupportedTag::BPM,
            SupportedTag::Key,
        ];
        config.analysis_tag_source = AnalysisTagSource::Catalog;
        let mut settings = PreparationSettings {
            autotag_enabled: true,
            stability_millis: 10,
            tagging: Some(config),
            organization: OrganizationSettings {
                mode: OrganizationSettingMode::Move,
                destination_root_id: Some(grant.id),
                ..Default::default()
            },
            ..Default::default()
        };
        settings.engine.energy_model = Some(model);
        let revision = lib.save_settings(settings).unwrap();
        let job = lib
            .enqueue(std::slice::from_ref(&source), revision.clone())
            .unwrap()
            .remove(0);
        let track = lib
            .jobs()
            .unwrap()
            .into_iter()
            .find(|j| j.id == job)
            .unwrap()
            .track_id;
        let runtime = MediaRuntime::from_bundle(&project.join("tools/media-runtime/bin")).unwrap();
        let editor = MetadataEditService::new(lib.clone(), runtime.clone());
        let original_native = editor
            .snapshot(std::slice::from_ref(&track))
            .unwrap()
            .tracks
            .remove(0)
            .native;
        let draft = editor.create_draft(std::slice::from_ref(&track)).unwrap();
        let mut patch = MetadataEditorPatch::default();
        // The edit also replaces the picture in the file; Undo has to put the old one back.
        assert_eq!(original_native.artwork.len(), 1);
        let replacement = editor.import_artwork(icon("128x128.png")).unwrap();
        patch.artwork.push(MetadataArtworkChoice {
            target: Some(original_native.artwork[0].identity.clone()),
            asset_id: Some(replacement.id),
            kind: Some(3),
            description: None,
        });
        patch.fields.text.insert(
            MetadataTextField::Title,
            TextEdit::Set {
                values: vec!["User title".into()],
            },
        );
        patch.fields.ratings.push(RatingEdit {
            identity: RatingIdentity {
                owner: "native".into(),
            },
            value: ValueEdit::Clear,
        });
        let draft = editor
            .update_draft(&draft.id, draft.revision, patch)
            .unwrap();
        let preview = editor
            .preview(&draft.id, draft.revision, &CancellationToken::default())
            .unwrap();
        let before = std::fs::read(&source).unwrap();
        let accepted = editor
            .apply(&preview.id, &CancellationToken::default())
            .unwrap();
        assert_eq!(accepted.state, "queued");
        assert_eq!(std::fs::read(&source).unwrap(), before);
        let catalog = Arc::new(Catalog(AtomicUsize::new(0)));
        let worker = PreparationService::start_with_tagger(
            lib.clone(),
            runtime.clone(),
            1,
            Arc::new(Tagger::with_backends(vec![catalog.clone()]).unwrap()),
        )
        .unwrap();
        let start = Instant::now();
        let finished = loop {
            let current = lib
                .jobs()
                .unwrap()
                .into_iter()
                .find(|j| j.id == job)
                .unwrap();
            if current.state == "completed"
                || current.state == "attention"
                || start.elapsed() > Duration::from_secs(120)
            {
                break current;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        worker.shutdown();
        drop(worker);
        assert_eq!(finished.state, "completed", "{finished:?}");
        assert_eq!(catalog.0.load(Ordering::SeqCst), 1);
        // Once the edit has been written its original audio is deleted; the replaced picture stays.
        assert_eq!(lib.tagging_storage().unwrap().original_files, 0);
        assert!(staging_dirs(&lib).is_empty());
        let final_path = lib.track_path(&track).unwrap();
        assert_eq!(final_path, managed.join("Catalog artist - User title.flac"));
        assert!(!source.exists());
        assert_eq!(lib.track(&track).unwrap().state, "ready");
        let prepared = editor
            .snapshot(std::slice::from_ref(&track))
            .unwrap()
            .tracks
            .remove(0)
            .native;
        assert_eq!(prepared.text[&MetadataTextField::Title], vec!["User title"]);
        assert_eq!(
            prepared.text[&MetadataTextField::Album],
            vec!["Catalog album"]
        );
        assert!(prepared.ratings.is_empty());
        assert_eq!(lib.cue_revision_count(&track).unwrap(), 1);
        let sql = rusqlite::Connection::open(&db).unwrap();
        let measurements:String=sql.query_row("SELECT json_array(analysis_json,musical_key,bpm,energy,duration) FROM tracks WHERE id=?1",[&track],|r|r.get(0)).unwrap();
        let cues:String=sql.query_row("SELECT json_group_array(json_array(id,job_id,json)) FROM cue_revisions WHERE track_id=?1",[&track],|r|r.get(0)).unwrap();
        drop(sql);
        let pcm = runtime
            .decode(&final_path, lib.data_dir())
            .unwrap()
            .pcm_sha256()
            .unwrap();
        drop(editor);
        drop(lib);
        let lib = Library::open(&db).unwrap();
        let editor = MetadataEditService::new(lib.clone(), runtime.clone());
        let saved = editor.batch(&accepted.id).unwrap();
        assert!(
            saved.can_undo,
            "Incoming final proof must follow its own automatic Move: {saved:?}"
        );
        if later_edit {
            let draft = editor.create_draft(std::slice::from_ref(&track)).unwrap();
            let mut patch = MetadataEditorPatch::default();
            patch.fields.text.insert(
                MetadataTextField::Composer,
                TextEdit::Set {
                    values: vec!["Later composer".into()],
                },
            );
            let draft = editor
                .update_draft(&draft.id, draft.revision, patch)
                .unwrap();
            let preview = editor
                .preview(&draft.id, draft.revision, &CancellationToken::default())
                .unwrap();
            editor
                .apply(&preview.id, &CancellationToken::default())
                .unwrap();
            let before = std::fs::read(&final_path).unwrap();
            assert!(
                editor
                    .undo(&saved.id, &CancellationToken::default())
                    .is_err()
            );
            assert_eq!(std::fs::read(&final_path).unwrap(), before);
            assert_eq!(
                editor
                    .snapshot(std::slice::from_ref(&track))
                    .unwrap()
                    .tracks[0]
                    .native
                    .text[&MetadataTextField::Composer],
                vec!["Later composer"]
            );
        } else {
            let undo = editor
                .undo(&saved.id, &CancellationToken::default())
                .unwrap();
            assert_eq!(undo.state, "undone", "{undo:?}");
            let restored = editor
                .snapshot(std::slice::from_ref(&track))
                .unwrap()
                .tracks
                .remove(0)
                .native;
            assert_eq!(
                restored.text[&MetadataTextField::Title],
                original_native.text[&MetadataTextField::Title]
            );
            assert_eq!(
                restored.ratings, original_native.ratings,
                "Undo must restore exact native47, not normalized40"
            );
            assert_eq!(
                restored.text[&MetadataTextField::Album],
                vec!["Catalog album"]
            );
            assert_eq!(restored.artwork.len(), 1);
            assert_eq!(
                restored.artwork[0].identity.sha256, original_native.artwork[0].identity.sha256,
                "the picture the edit replaced is back, without the original audio"
            );
            // The Undo is the end of it: its own copy of the audio goes with the write, and the
            // picture kept for it is released (only the imported replacement is left).
            assert!(
                staging_dirs(&lib).is_empty(),
                "an Undo of an Incoming edit keeps no audio"
            );
            assert_eq!(lib.tagging_storage().unwrap().original_files, 0);
            assert_eq!(artwork_assets(&lib), 1);
            assert_eq!(restored.bpm, prepared.bpm);
            assert_eq!(
                restored.text[&MetadataTextField::Key],
                prepared.text[&MetadataTextField::Key]
            );
        }
        let sql = rusqlite::Connection::open(&db).unwrap();
        assert_eq!(sql.query_row("SELECT json_array(analysis_json,musical_key,bpm,energy,duration) FROM tracks WHERE id=?1",[&track],|r|r.get::<_,String>(0)).unwrap(),measurements);
        assert_eq!(sql.query_row("SELECT json_group_array(json_array(id,job_id,json)) FROM cue_revisions WHERE track_id=?1",[&track],|r|r.get::<_,String>(0)).unwrap(),cues);
        assert_eq!(
            runtime
                .decode(&final_path, lib.data_dir())
                .unwrap()
                .pcm_sha256()
                .unwrap(),
            pcm
        );
        assert_eq!(lib.track_path(&track).unwrap(), final_path);
        assert!(!source.exists());
        let wrapper = onetagger_tag::Tag::load_file(&final_path, true).unwrap();
        assert_eq!(
            wrapper.tag().get_native("OPAQUE_DJ_RECORD"),
            Some(vec!["preserve native record".into()])
        );
    }
}

fn icon(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../apps/desktop/src-tauri/icons")
            .join(name),
    )
    .unwrap()
}
fn artwork_assets(lib: &Library) -> i64 {
    rusqlite::Connection::open(lib.data_dir().join("library.sqlite"))
        .unwrap()
        .query_row("SELECT COUNT(*) FROM metadata_artwork_assets", [], |r| {
            r.get(0)
        })
        .unwrap()
}
fn save(
    service: &MetadataEditService,
    track: &str,
    patch: MetadataEditorPatch,
) -> MetadataEditBatch {
    let draft = service.create_draft(&[track.into()]).unwrap();
    let draft = service
        .update_draft(&draft.id, draft.revision, patch)
        .unwrap();
    let preview = service
        .preview(&draft.id, draft.revision, &CancellationToken::default())
        .unwrap();
    service
        .apply(&preview.id, &CancellationToken::default())
        .unwrap()
}
fn pictures(service: &MetadataEditService, track: &str) -> Vec<cuerithm_autotag::ArtworkIdentity> {
    service.snapshot(&[track.into()]).unwrap().tracks[0]
        .native
        .artwork
        .iter()
        .map(|a| a.identity.clone())
        .collect()
}

/// Replacing a picture keeps the original picture (artwork belongs in the library), not the audio
/// file it came from, and Undo puts it back.
#[test]
fn undoing_an_artwork_replacement_restores_the_picture_without_keeping_the_audio() {
    let (_t, lib, service, track, path) = fixture();
    let first = service.import_artwork(icon("32x32.png")).unwrap();
    let mut add = MetadataEditorPatch::default();
    add.artwork.push(MetadataArtworkChoice {
        target: None,
        asset_id: Some(first.id),
        kind: Some(3),
        description: None,
    });
    save(&service, &track, add);
    let original = pictures(&service, &track);
    assert_eq!(original.len(), 1);
    let assets_before = artwork_assets(&lib);

    let second = service.import_artwork(icon("128x128.png")).unwrap();
    let mut replace = MetadataEditorPatch::default();
    replace.artwork.push(MetadataArtworkChoice {
        target: Some(original[0].clone()),
        asset_id: Some(second.id),
        kind: Some(3),
        description: None,
    });
    let saved = save(&service, &track, replace);
    assert_eq!(saved.state, "saved", "{saved:?}");
    let replaced = pictures(&service, &track);
    assert_ne!(replaced[0].sha256, original[0].sha256);
    // The audio file is not kept; the picture it held is.
    assert_eq!(lib.tagging_storage().unwrap().original_files, 0);
    assert_eq!(
        artwork_assets(&lib),
        assets_before + 2,
        "the new picture and the kept one"
    );

    let audio = pcm(&service, &path);
    let undone = service
        .undo(&saved.id, &CancellationToken::default())
        .unwrap();
    assert_eq!(undone.state, "undone", "{undone:?}");
    let restored = pictures(&service, &track);
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].sha256, original[0].sha256);
    assert_eq!(pcm(&service, &path), audio);
    assert_eq!(lib.tagging_storage().unwrap().original_files, 0);
    assert_eq!(
        artwork_assets(&lib),
        assets_before + 1,
        "the kept picture goes once the save is undone"
    );
}

/// A save can leave the pictures exactly as they were while it changes other fields: it replaces
/// a picture with itself, or removes one and adds the same one back. Undo then has no picture to
/// restore, and must still undo the other changes without the original audio.
#[test]
fn undo_after_a_save_that_left_the_pictures_as_they_were_restores_its_other_changes() {
    for remove_and_add_back in [false, true] {
        let (_t, lib, service, track, path) = fixture();
        let asset = service.import_artwork(icon("32x32.png")).unwrap();
        add_picture(&service, &track, &asset.id);
        let original = pictures(&service, &track);
        assert_eq!(original.len(), 1);
        let before = service
            .snapshot(std::slice::from_ref(&track))
            .unwrap()
            .tracks[0]
            .native
            .clone();
        let mut patch = MetadataEditorPatch::default();
        patch.fields.text.insert(
            MetadataTextField::Title,
            TextEdit::Set {
                values: vec!["Changed title".into()],
            },
        );
        if remove_and_add_back {
            patch.artwork.push(MetadataArtworkChoice {
                target: Some(original[0].clone()),
                asset_id: None,
                kind: None,
                description: None,
            });
            patch.artwork.push(MetadataArtworkChoice {
                target: None,
                asset_id: Some(asset.id.clone()),
                kind: original[0].kind,
                description: original[0].description.clone(),
            });
        } else {
            patch.artwork.push(MetadataArtworkChoice {
                target: Some(original[0].clone()),
                asset_id: Some(asset.id.clone()),
                kind: original[0].kind,
                description: original[0].description.clone(),
            });
        }
        let saved = save(&service, &track, patch);
        assert_eq!(saved.state, "saved", "{saved:?}");
        assert_eq!(
            pictures(&service, &track),
            original,
            "the save left the picture as it was"
        );
        assert_eq!(lib.tagging_storage().unwrap().original_files, 0);
        let audio = pcm(&service, &path);
        let undone = service
            .undo(&saved.id, &CancellationToken::default())
            .unwrap();
        assert_eq!(
            undone.state, "undone",
            "remove_and_add_back={remove_and_add_back}: {undone:?}"
        );
        let restored = service
            .snapshot(std::slice::from_ref(&track))
            .unwrap()
            .tracks[0]
            .native
            .clone();
        assert_eq!(restored.text, before.text);
        assert_eq!(restored.artwork, before.artwork);
        assert_eq!(pcm(&service, &path), audio);
        assert_eq!(lib.tagging_storage().unwrap().original_files, 0);
    }
}

/// The neighbours of the case above change the pictures, so the save keeps the originals and
/// Undo puts them back: a picture whose description alone changed, and a removed picture.
#[test]
fn undo_restores_a_picture_whose_description_changed_or_that_was_removed() {
    for removed in [false, true] {
        let (_t, lib, service, track, _path) = fixture();
        let asset = service.import_artwork(icon("32x32.png")).unwrap();
        add_picture(&service, &track, &asset.id);
        let original = pictures(&service, &track);
        let mut patch = MetadataEditorPatch::default();
        patch.fields.text.insert(
            MetadataTextField::Title,
            TextEdit::Set {
                values: vec!["Changed title".into()],
            },
        );
        patch.artwork.push(MetadataArtworkChoice {
            target: Some(original[0].clone()),
            asset_id: (!removed).then(|| asset.id.clone()),
            kind: original[0].kind,
            description: (!removed).then(|| "Back cover".to_string()),
        });
        let saved = save(&service, &track, patch);
        assert_eq!(saved.state, "saved", "{saved:?}");
        assert_ne!(pictures(&service, &track), original);
        assert_eq!(lib.tagging_storage().unwrap().original_files, 0);
        let undone = service
            .undo(&saved.id, &CancellationToken::default())
            .unwrap();
        assert_eq!(undone.state, "undone", "removed={removed}: {undone:?}");
        assert_eq!(pictures(&service, &track), original, "removed={removed}");
    }
}

/// The same for an edit accepted while the track was in Incoming: written by the worker with its
/// title changed and its picture replaced by itself, its Undo has no picture to restore.
#[test]
fn undo_of_an_incoming_save_that_left_the_pictures_as_they_were_restores_its_other_changes() {
    use std::time::{Duration, Instant};
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().canonicalize().unwrap();
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let lib = Library::open(&folder.join("data/library.sqlite")).unwrap();
    let model = lib.data_dir().join("test-energy.json");
    std::fs::write(&model,serde_json::to_vec(&serde_json::json!({"gamma":0.000001,"classes":[3,8],"n_support":[1,1],"support_vectors":[vec![0.;183],vec![1.;183]],"dual_coef":[[1.,-1.]],"intercept":[0.1]})).unwrap()).unwrap();
    let mut settings = PreparationSettings {
        stability_millis: 10,
        ..Default::default()
    };
    settings.engine.energy_model = Some(model);
    let revision = lib.save_settings(settings).unwrap();
    let source = folder.join("source.flac");
    std::fs::copy(
        project.join("crates/camelot-source-analysis/tests/fixtures/pulses64-stereo16.flac"),
        &source,
    )
    .unwrap();
    {
        let mut wrapper = onetagger_tag::Tag::load_file(&source, true).unwrap();
        let tag = wrapper.tag_mut();
        tag.set_native("TITLE", vec!["Original title".into()]);
        tag.set_art(
            onetagger_tag::CoverType::CoverFront,
            "image/png",
            None,
            icon("32x32.png"),
        );
        tag.save_file(&source).unwrap();
    }
    let job = lib
        .enqueue(std::slice::from_ref(&source), revision)
        .unwrap()
        .remove(0);
    let track = lib
        .jobs()
        .unwrap()
        .into_iter()
        .find(|j| j.id == job)
        .unwrap()
        .track_id;
    let runtime = MediaRuntime::from_bundle(&project.join("tools/media-runtime/bin")).unwrap();
    let editor = MetadataEditService::new(lib.clone(), runtime.clone());
    let original = pictures(&editor, &track);
    assert_eq!(original.len(), 1);
    let same = editor.import_artwork(icon("32x32.png")).unwrap();
    let mut patch = MetadataEditorPatch::default();
    patch.fields.text.insert(
        MetadataTextField::Title,
        TextEdit::Set {
            values: vec!["Saved title".into()],
        },
    );
    patch.artwork.push(MetadataArtworkChoice {
        target: Some(original[0].clone()),
        asset_id: Some(same.id),
        kind: original[0].kind,
        description: original[0].description.clone(),
    });
    let queued = save(&editor, &track, patch);
    assert_eq!(queued.state, "queued", "{queued:?}");
    let worker = PreparationService::start_with_workers(lib.clone(), runtime.clone(), 1).unwrap();
    let start = Instant::now();
    loop {
        let jobs = lib.jobs().unwrap();
        if jobs.iter().all(|j| j.state == "completed")
            || jobs.iter().any(|j| j.state == "attention")
            || start.elapsed() > Duration::from_secs(120)
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    worker.shutdown();
    drop(worker);
    assert!(
        lib.jobs().unwrap().iter().all(|j| j.state == "completed"),
        "{:?}",
        lib.jobs().unwrap()
    );
    assert_eq!(pictures(&editor, &track).len(), 1);
    assert_eq!(pictures(&editor, &track)[0].sha256, original[0].sha256);
    // The worker wrote it and the original audio is gone.
    assert_eq!(lib.tagging_storage().unwrap().original_files, 0);
    assert!(staging_dirs(&lib).is_empty());
    let undone = editor
        .undo(&queued.id, &CancellationToken::default())
        .unwrap();
    assert_eq!(undone.state, "undone", "{undone:?}");
    let restored = editor
        .snapshot(std::slice::from_ref(&track))
        .unwrap()
        .tracks
        .remove(0)
        .native;
    assert_eq!(
        restored.text[&MetadataTextField::Title],
        vec!["Original title"]
    );
    assert_eq!(restored.artwork.len(), 1);
    assert_eq!(restored.artwork[0].identity.sha256, original[0].sha256);
    assert!(staging_dirs(&lib).is_empty());
}

/// An edit accepted while the track was still in Incoming keeps the original audio only until it
/// has been written: then the pictures it replaced become artwork assets and the audio is deleted.
#[test]
fn a_written_incoming_edit_keeps_its_replaced_picture_and_not_the_audio() {
    let (_t, lib, service, track, path) = fixture();
    let first = service.import_artwork(icon("32x32.png")).unwrap();
    let mut add = MetadataEditorPatch::default();
    add.artwork.push(MetadataArtworkChoice {
        target: None,
        asset_id: Some(first.id),
        kind: Some(3),
        description: None,
    });
    save(&service, &track, add);
    let original = pictures(&service, &track);
    assert_eq!(original.len(), 1);
    let db = lib.data_dir().join("library.sqlite");
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute("UPDATE tracks SET state='incoming' WHERE id=?1", [&track])
        .unwrap();
    let assets_before = artwork_assets(&lib);

    let second = service.import_artwork(icon("128x128.png")).unwrap();
    let mut replace = MetadataEditorPatch::default();
    replace.artwork.push(MetadataArtworkChoice {
        target: Some(original[0].clone()),
        asset_id: Some(second.id),
        kind: Some(3),
        description: None,
    });
    let queued = save(&service, &track, replace);
    assert_eq!(queued.state, "queued", "{queued:?}");
    // Waiting for its turn, the edit still holds the original: nothing was written yet.
    assert_eq!(lib.tagging_storage().unwrap().original_files, 1);
    assert_eq!(artwork_assets(&lib), assets_before + 1);

    // The worker writes it (done here by hand, the way its commit records it).
    conn.execute_batch(
        "UPDATE metadata_edit_overrides SET state='committed',final_json='{}';
         UPDATE metadata_edit_rows SET state='written' WHERE state='queued';
         UPDATE metadata_edit_batches SET state='saved' WHERE state='queued';
         UPDATE jobs SET state='completed';",
    )
    .unwrap();
    drop(service);
    drop(conn);
    drop(lib);
    let lib = Library::open(&db).unwrap();
    assert_eq!(
        lib.tagging_storage().unwrap().original_files,
        0,
        "the original audio of a written Incoming edit is deleted"
    );
    assert!(
        staging_dirs(&lib).is_empty(),
        "and so is its private folder, not only its accounting"
    );
    assert_eq!(
        artwork_assets(&lib),
        assets_before + 2,
        "the new picture and the one it replaced"
    );
    let sql = rusqlite::Connection::open(&db).unwrap();
    let (retired, kept): (bool, i64) = sql
        .query_row(
            "SELECT json_extract(payload_json,'$.original_retired'),json_array_length(payload_json,'$.kept_pictures') FROM metadata_edit_rows WHERE state='written' AND json_extract(payload_json,'$.capture.public.state')='incoming'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!(retired && kept == 1, "retired={retired} kept={kept}");
    assert!(path.exists());
    // A library backup does not wait for an original that was deleted on purpose.
    lib.create_library_backup(&_t.path().join("backups"))
        .expect("a library backup without the deleted original");
    // Nothing is left to retire at the next start.
    drop(lib);
    let lib = Library::open(&db).unwrap();
    assert_eq!(lib.tagging_storage().unwrap().original_files, 0);
    assert_eq!(artwork_assets(&lib), assets_before + 2);
}

/// A track added to the library for the retention tests.
fn extra_track(lib: &Library, root: &Path, n: usize) -> String {
    let path = root.join(format!("extra{n}.wav"));
    let mut writer = hound::WavWriter::create(
        &path,
        hound::WavSpec {
            channels: 2,
            sample_rate: 44100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    for i in 0..4410 {
        let value = (((i as f64 * (200. + n as f64) * std::f64::consts::TAU / 44100.).sin())
            * 8000.) as i16;
        writer.write_sample(value).unwrap();
        writer.write_sample(value).unwrap();
    }
    writer.finalize().unwrap();
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
        .find(|j| j.id == job)
        .unwrap()
        .track_id;
    rusqlite::Connection::open(lib.data_dir().join("library.sqlite"))
        .unwrap()
        .execute("UPDATE tracks SET state='ready',analysis_json='{}',musical_key='8A',bpm=128,energy=7 WHERE id=?1", [&track])
        .unwrap();
    track
}
/// Saves from tracks that were still in Incoming count in the same window of ten as every other
/// save: an older one stops being undoable and releases the pictures kept for it.
#[test]
fn only_the_newest_ten_incoming_saves_can_be_undone_and_older_ones_release_their_pictures() {
    let (temp, lib, service, first_track, _path) = fixture();
    let root = temp.path().canonicalize().unwrap();
    let db = lib.data_dir().join("library.sqlite");
    let first = service.import_artwork(icon("32x32.png")).unwrap();
    let second = service.import_artwork(icon("128x128.png")).unwrap();
    let imported = artwork_assets(&lib);
    assert_eq!(imported, 2);
    // Every track already has a picture.
    let mut tracks = vec![first_track];
    for n in 1..12 {
        tracks.push(extra_track(&lib, &root, n));
    }
    for track in &tracks {
        let mut add = MetadataEditorPatch::default();
        add.artwork.push(MetadataArtworkChoice {
            target: None,
            asset_id: Some(first.id.clone()),
            kind: Some(3),
            description: None,
        });
        save(&service, track, add);
    }
    // One saved Incoming edit: the picture is replaced while the track is waiting.
    let incoming_save = |lib: &Library, service: &MetadataEditService, track: &str| {
        let original = pictures(service, track);
        rusqlite::Connection::open(lib.data_dir().join("library.sqlite"))
            .unwrap()
            .execute("UPDATE tracks SET state='incoming' WHERE id=?1", [track])
            .unwrap();
        let mut replace = MetadataEditorPatch::default();
        replace.artwork.push(MetadataArtworkChoice {
            target: Some(original[0].clone()),
            asset_id: Some(second.id.clone()),
            kind: Some(3),
            description: None,
        });
        let queued = save(service, track, replace);
        assert_eq!(queued.state, "queued", "{queued:?}");
        queued.id
    };
    // The worker writes them (done here by hand, the way its commit records it).
    let committed = |lib: &Library| {
        rusqlite::Connection::open(lib.data_dir().join("library.sqlite"))
            .unwrap()
            .execute_batch(
                "UPDATE metadata_edit_overrides SET state='committed',final_json='{}' WHERE state='queued';
                 UPDATE metadata_edit_rows SET state='written' WHERE state='queued';
                 UPDATE metadata_edit_batches SET state='saved' WHERE state='queued';
                 UPDATE jobs SET state='completed' WHERE id IN (SELECT job_id FROM metadata_edit_overrides WHERE state='committed');",
            )
            .unwrap();
    };
    let mut batches: Vec<String> = tracks[..10]
        .iter()
        .map(|track| incoming_save(&lib, &service, track))
        .collect();
    committed(&lib);
    drop(service);
    drop(lib);
    // Ten saves: each keeps the picture it replaced, and the audio goes.
    let lib = Library::open(&db).unwrap();
    assert_eq!(artwork_assets(&lib), imported + 10);
    assert!(staging_dirs(&lib).is_empty());
    let runtime = MediaRuntime::from_bundle(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/media-runtime/bin"),
    )
    .unwrap();
    let service = MetadataEditService::new(lib.clone(), runtime);
    for track in &tracks[10..] {
        batches.push(incoming_save(&lib, &service, track));
    }
    committed(&lib);
    drop(service);
    drop(lib);
    let lib = Library::open(&db).unwrap();
    let state = |batch: &str| -> String {
        rusqlite::Connection::open(&db)
            .unwrap()
            .query_row(
                "SELECT state FROM metadata_edit_overrides WHERE batch_id=?1",
                [batch],
                |r| r.get(0),
            )
            .unwrap()
    };
    for (index, batch) in batches.iter().enumerate() {
        assert_eq!(
            state(batch),
            if index < 2 { "expired" } else { "committed" },
            "save {index}"
        );
    }
    // The ten newest hold a picture each; the two oldest gave theirs up and no audio is left.
    assert_eq!(artwork_assets(&lib), imported + 10);
    assert!(staging_dirs(&lib).is_empty());
    assert_eq!(lib.tagging_storage().unwrap().original_files, 0);
}

/// Gives a track the picture its Incoming save will replace.
fn add_picture(service: &MetadataEditService, track: &str, asset: &str) {
    let mut add = MetadataEditorPatch::default();
    add.artwork.push(MetadataArtworkChoice {
        target: None,
        asset_id: Some(asset.into()),
        kind: Some(3),
        description: None,
    });
    save(service, track, add);
}
/// An Incoming edit as the worker leaves it between two steps: written, with its proof of the
/// final file not recorded yet (an automatic Move may still be to come).
fn queue_incoming_replacement(
    lib: &Library,
    service: &MetadataEditService,
    track: &str,
    second: &str,
) -> String {
    let original = pictures(service, track);
    rusqlite::Connection::open(lib.data_dir().join("library.sqlite"))
        .unwrap()
        .execute("UPDATE tracks SET state='incoming' WHERE id=?1", [track])
        .unwrap();
    let mut replace = MetadataEditorPatch::default();
    replace.artwork.push(MetadataArtworkChoice {
        target: Some(original[0].clone()),
        asset_id: Some(second.into()),
        kind: Some(3),
        description: None,
    });
    let queued = save(service, track, replace);
    assert_eq!(queued.state, "queued", "{queued:?}");
    queued.id
}
fn override_state(db: &Path, batch: &str) -> String {
    rusqlite::Connection::open(db)
        .unwrap()
        .query_row(
            "SELECT state FROM metadata_edit_overrides WHERE batch_id=?1",
            [batch],
            |r| r.get(0),
        )
        .unwrap()
}

/// An Incoming save the worker has written but not yet given its final proof is not an old save
/// that left the window: finishing it needs its row to stay as it is.
#[test]
fn a_written_incoming_save_that_is_not_finalised_yet_is_never_expired() {
    let (temp, lib, service, first_track, _path) = fixture();
    let root = temp.path().canonicalize().unwrap();
    let db = lib.data_dir().join("library.sqlite");
    let first = service.import_artwork(icon("32x32.png")).unwrap();
    let second = service.import_artwork(icon("128x128.png")).unwrap();
    let imported = artwork_assets(&lib);
    let mut tracks = vec![first_track];
    for n in 1..12 {
        tracks.push(extra_track(&lib, &root, n));
    }
    for track in &tracks {
        add_picture(&service, track, &first.id);
    }
    // Twelve Incoming saves; the oldest has been written but not finalised, the others are done.
    let batches: Vec<String> = tracks
        .iter()
        .map(|track| queue_incoming_replacement(&lib, &service, track, &second.id))
        .collect();
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute_batch(&format!(
            "UPDATE metadata_edit_overrides SET state='committed',final_json='{{}}' WHERE state='queued';
             UPDATE metadata_edit_overrides SET final_json=NULL WHERE batch_id='{}';
             UPDATE metadata_edit_rows SET state='written' WHERE state='queued';
             UPDATE metadata_edit_batches SET state='saved' WHERE state='queued';
             UPDATE jobs SET state='completed' WHERE id IN (SELECT job_id FROM metadata_edit_overrides WHERE state='committed');",
            batches[0]
        ))
        .unwrap();
    drop(service);
    drop(lib);
    let lib = Library::open(&db).unwrap();
    // It is still waiting for its proof, and its row is where the finishing step looks for it.
    assert_eq!(override_state(&db, &batches[0]), "committed");
    let waiting: i64 = rusqlite::Connection::open(&db)
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM metadata_edit_overrides WHERE batch_id=?1 AND state='committed' AND final_json IS NULL",
            [&batches[0]],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(waiting, 1);
    // The window of ten holds the finished ones: the oldest of those is the one that left.
    assert_eq!(override_state(&db, &batches[1]), "expired");
    for batch in &batches[2..] {
        assert_eq!(override_state(&db, batch), "committed");
    }
    assert_eq!(artwork_assets(&lib), imported + 11);
    // Once it is finalised it counts like any other save, and as the oldest it leaves.
    let finalised = rusqlite::Connection::open(&db)
        .unwrap()
        .execute(
            "UPDATE metadata_edit_overrides SET final_json='{}' WHERE batch_id=?1 AND state='committed' AND final_json IS NULL",
            [&batches[0]],
        )
        .unwrap();
    assert_eq!(finalised, 1);
    drop(lib);
    let lib = Library::open(&db).unwrap();
    assert_eq!(override_state(&db, &batches[0]), "expired");
    assert_eq!(artwork_assets(&lib), imported + 10);
}

/// More than ten Incoming saves are accepted before the worker has written any of them. The
/// window of ten is enforced when the worker commits them, not at the next save or restart, both
/// when the commit is also the end of the job and when an automatic Move follows it.
#[test]
fn incoming_saves_the_worker_commits_beyond_the_tenth_expire_without_another_save() {
    use std::time::{Duration, Instant};
    for moved in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let folder = temp.path().canonicalize().unwrap();
        let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let db = folder.join("data/library.sqlite");
        let lib = Library::open(&db).unwrap();
        let managed = folder.join("managed");
        std::fs::create_dir(&managed).unwrap();
        let grant = lib.register_organization_root(managed.clone()).unwrap();
        let model = lib.data_dir().join("test-energy.json");
        std::fs::write(&model,serde_json::to_vec(&serde_json::json!({"gamma":0.000001,"classes":[3,8],"n_support":[1,1],"support_vectors":[vec![0.;183],vec![1.;183]],"dual_coef":[[1.,-1.]],"intercept":[0.1]})).unwrap()).unwrap();
        let mut settings = PreparationSettings {
            stability_millis: 10,
            organization: if moved {
                OrganizationSettings {
                    mode: OrganizationSettingMode::Move,
                    destination_root_id: Some(grant.id),
                    ..Default::default()
                }
            } else {
                OrganizationSettings::default()
            },
            ..Default::default()
        };
        settings.engine.energy_model = Some(model);
        let revision = lib.save_settings(settings).unwrap();
        let mut tracks = vec![];
        for n in 0..12 {
            let source = folder.join(format!("source{n}.flac"));
            std::fs::copy(
                project
                    .join("crates/camelot-source-analysis/tests/fixtures/pulses64-stereo16.flac"),
                &source,
            )
            .unwrap();
            let mut wrapper = onetagger_tag::Tag::load_file(&source, true).unwrap();
            let tag = wrapper.tag_mut();
            tag.set_native("TITLE", vec![format!("Original title {n}")]);
            tag.set_native("ARTIST", vec![format!("Artist {n}")]);
            tag.set_art(
                onetagger_tag::CoverType::CoverFront,
                "image/png",
                None,
                icon("32x32.png"),
            );
            tag.save_file(&source).unwrap();
            let job = lib
                .enqueue(std::slice::from_ref(&source), revision.clone())
                .unwrap()
                .remove(0);
            tracks.push(
                lib.jobs()
                    .unwrap()
                    .into_iter()
                    .find(|j| j.id == job)
                    .unwrap()
                    .track_id,
            );
        }
        let runtime = MediaRuntime::from_bundle(&project.join("tools/media-runtime/bin")).unwrap();
        let editor = MetadataEditService::new(lib.clone(), runtime.clone());
        let replacement = editor.import_artwork(icon("128x128.png")).unwrap();
        let imported = artwork_assets(&lib);
        // Twelve edits are accepted while every track is still waiting in Incoming.
        let batches: Vec<String> = tracks
            .iter()
            .enumerate()
            .map(|(n, track)| {
                let mut patch = MetadataEditorPatch::default();
                patch.artwork.push(MetadataArtworkChoice {
                    target: Some(pictures(&editor, track)[0].clone()),
                    asset_id: Some(replacement.id.clone()),
                    kind: Some(3),
                    description: None,
                });
                patch.fields.text.insert(
                    MetadataTextField::Title,
                    TextEdit::Set {
                        values: vec![format!("Saved title {n}")],
                    },
                );
                let queued = save(&editor, track, patch);
                assert_eq!(queued.state, "queued", "{queued:?}");
                queued.id
            })
            .collect();
        let state = |batch: &str| override_state(&db, batch);
        assert!(batches.iter().all(|batch| state(batch) == "queued"));
        let worker =
            PreparationService::start_with_workers(lib.clone(), runtime.clone(), 1).unwrap();
        let start = Instant::now();
        loop {
            let jobs = lib.jobs().unwrap();
            if jobs.iter().all(|j| j.state == "completed")
                || jobs.iter().any(|j| j.state == "attention")
                || start.elapsed() > Duration::from_secs(240)
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        worker.shutdown();
        drop(worker);
        let jobs = lib.jobs().unwrap();
        assert!(
            jobs.iter().all(|j| j.state == "completed"),
            "moved={moved}: {jobs:?}"
        );
        // Nothing else was saved and the library was not reopened: the worker's commit enforced it.
        for (index, batch) in batches.iter().enumerate() {
            assert_eq!(
                state(batch),
                if index < 2 { "expired" } else { "committed" },
                "moved={moved}: save {index}"
            );
        }
        assert_eq!(
            artwork_assets(&lib),
            imported + 10,
            "moved={moved}: the ten newest hold the picture they replaced, the others released it"
        );
        assert_eq!(lib.tagging_storage().unwrap().original_files, 0);
        assert!(staging_dirs(&lib).is_empty());
    }
}

/// A save whose original could not be deleted yet is still bound by a library backup.
#[test]
fn a_backup_binds_the_original_of_an_expired_incoming_save_that_is_still_on_disk() {
    let (temp, lib, service, track, _path) = fixture();
    let db = lib.data_dir().join("library.sqlite");
    let first = service.import_artwork(icon("32x32.png")).unwrap();
    let second = service.import_artwork(icon("128x128.png")).unwrap();
    add_picture(&service, &track, &first.id);
    let batch = queue_incoming_replacement(&lib, &service, &track, &second.id);
    // Written, then pushed out of the window; deleting the original failed and is retried later.
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute_batch(
            "UPDATE metadata_edit_overrides SET state='expired',final_json='{}';
             UPDATE metadata_edit_rows SET state='written' WHERE state='queued';
             UPDATE metadata_edit_batches SET state='saved' WHERE state='queued';
             UPDATE jobs SET state='completed';",
        )
        .unwrap();
    assert_eq!(override_state(&db, &batch), "expired");
    let original = std::fs::read_dir(&staging_dirs(&lib)[0])
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("original"))
        })
        .unwrap();
    let destination = temp.path().join("backups");
    // Untouched, the original is bound like that of any written save.
    lib.create_library_backup(&destination).unwrap();
    // A changed original is not blessed by a new backup while the save still owns it.
    std::fs::write(&original, b"later external edit of the original").unwrap();
    assert!(
        lib.create_library_backup(&destination).is_err(),
        "a backup accepted the changed original of a save that still owns it"
    );
}

/// Only the newest ten saved batches can be undone; the older ones keep their history line.
#[test]
fn only_the_newest_ten_saves_can_be_undone() {
    let (_t, lib, service, track, _path) = fixture();
    let mut ids = vec![];
    for n in 0..12 {
        std::thread::sleep(std::time::Duration::from_millis(3));
        let mut patch = MetadataEditorPatch::default();
        patch.fields.text.insert(
            MetadataTextField::Title,
            TextEdit::Set {
                values: vec![format!("Title {n}")],
            },
        );
        ids.push(save(&service, &track, patch).id);
    }
    let history = service.history().unwrap();
    assert_eq!(history.len(), 12, "history lines are never dropped");
    let undoable = |id: &str| history.iter().find(|b| b.id == id).unwrap().can_undo;
    // Only the newest save is undoable at a time (each later one changed the track), and an
    // older one is out of the window for good.
    assert!(undoable(&ids[11]));
    let expired: i64 = rusqlite::Connection::open(lib.data_dir().join("library.sqlite"))
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM metadata_edit_writes WHERE json_extract(payload_json,'$.undo_expired')=1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(expired, 2);
    assert!(
        !service
            .undo(&ids[0], &CancellationToken::default())
            .map(|b| b.state == "undone")
            .unwrap_or(false)
    );
}

/// A restart in the middle of retiring the original completes it and keeps Undo working.
#[test]
fn a_restart_after_the_original_was_deleted_completes_the_save_and_keeps_undo() {
    let (temp, lib, service, track, path) = fixture();
    let first = service.import_artwork(icon("32x32.png")).unwrap();
    let mut add = MetadataEditorPatch::default();
    add.artwork.push(MetadataArtworkChoice {
        target: None,
        asset_id: Some(first.id),
        kind: Some(3),
        description: None,
    });
    save(&service, &track, add);
    let original = pictures(&service, &track);
    let second = service.import_artwork(icon("128x128.png")).unwrap();
    let mut replace = MetadataEditorPatch::default();
    replace.artwork.push(MetadataArtworkChoice {
        target: Some(original[0].clone()),
        asset_id: Some(second.id),
        kind: Some(3),
        description: None,
    });
    let saved = save(&service, &track, replace);
    let audio = pcm(&service, &path);
    let db = lib.data_dir().join("library.sqlite");
    drop(service);
    drop(lib);
    // The crash: the original is gone, but the journal does not say so yet.
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute(
        "UPDATE metadata_edit_writes SET error='Metadata temporary cleanup pending',payload_json=json_set(payload_json,'$.backup_retired',json('false')) WHERE json_extract(payload_json,'$.backup_retired')=1 AND batch_id=?1",
        [&saved.id],
    )
    .unwrap();
    drop(conn);
    let lib = Library::open(&db).unwrap();
    let runtime = MediaRuntime::from_bundle(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/media-runtime/bin"),
    )
    .unwrap();
    let service = MetadataEditService::new(lib.clone(), runtime);
    let pending: i64 = rusqlite::Connection::open(&db)
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM metadata_edit_writes WHERE error IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(pending, 0, "the start completed the cleanup");
    let undone = service
        .undo(&saved.id, &CancellationToken::default())
        .unwrap();
    assert_eq!(undone.state, "undone", "{undone:?}");
    assert_eq!(pictures(&service, &track)[0].sha256, original[0].sha256);
    assert_eq!(pcm(&service, &path), audio);
    drop(temp);
}
