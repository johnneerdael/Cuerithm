use super::reconcile::{Fixture, track};
use crate::*;
use cuerithm_dj_sync::import::*;
use cuerithm_media::{CancellationToken, MediaRuntime};
use serde_json::json;
/// Undo writes the tags it recorded back; no copy of the file is kept, so the container's bytes
/// may differ from the original. What has to come back is the audio and the tags.
#[cfg(feature = "autotag")]
fn assert_restored(dir: &std::path::Path, original: &[u8], now: &std::path::Path, label: &str) {
    let extension = now.extension().unwrap().to_string_lossy().into_owned();
    let copy = dir.join(format!("original-copy.{extension}"));
    std::fs::write(&copy, original).unwrap();
    let pcm = |path: &std::path::Path| {
        runtime()
            .decode(path, &dir.join("restored-cache"))
            .unwrap()
            .pcm_sha256()
            .unwrap()
    };
    assert_eq!(pcm(now), pcm(&copy), "{label}: audio");
    let tags = |path: &std::path::Path| {
        cuerithm_autotag::read_metadata_snapshot(path, &runtime().probe(path).unwrap()).unwrap()
    };
    assert_eq!(tags(now), tags(&copy), "{label}: tags");
}
fn runtime() -> MediaRuntime {
    let bundle = std::env::var_os("CUERITHM_MEDIA_RUNTIME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/media-runtime/bin")
        });
    MediaRuntime::from_bundle(&bundle).unwrap()
}
fn setup(policy: ImportPolicy) -> (Fixture, String, String, std::path::PathBuf) {
    setup_format(policy, "wav", "pcm_s16le")
}
fn setup_format(
    policy: ImportPolicy,
    extension: &str,
    codec: &str,
) -> (Fixture, String, String, std::path::PathBuf) {
    setup_configured(policy, extension, codec, |_| {})
}
fn setup_configured(
    policy: ImportPolicy,
    extension: &str,
    codec: &str,
    configure: impl FnOnce(&Fixture),
) -> (Fixture, String, String, std::path::PathBuf) {
    setup_observation(policy, extension, codec, configure, |_| {})
}
fn setup_observation(
    policy: ImportPolicy,
    extension: &str,
    codec: &str,
    configure: impl FnOnce(&Fixture),
    observation: impl FnOnce(&mut TrackObservation),
) -> (Fixture, String, String, std::path::PathBuf) {
    let f = Fixture::new();
    configure(&f);
    let path = f.root.path().join("original.wav");
    let mut w = hound::WavWriter::create(
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
        w.write_sample(
            ((n as f64 * 440.0 * std::f64::consts::TAU / 44100.0).sin() * 10000.0) as i16,
        )
        .unwrap();
    }
    w.finalize().unwrap();
    let path = if extension != "wav" || codec != "pcm_s16le" {
        let target = f.root.path().join(format!("original.{extension}"));
        let fixture_root = f.root.path().join("format-fixtures");
        let archive = std::fs::File::open(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tools/fixtures/import-publication-media.tar.gz"),
        )
        .unwrap();
        tar::Archive::new(flate2::read::GzDecoder::new(archive))
            .unpack(&fixture_root)
            .unwrap();
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(fixture_root.join("manifest.json")).unwrap())
                .unwrap();
        let case = manifest["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["extension"] == extension && case["codec"] == codec)
            .unwrap();
        let source = fixture_root.join(case["file"].as_str().unwrap());
        assert_eq!(
            cuerithm_media::file_sha256(&source).unwrap(),
            case["sha256"].as_str().unwrap()
        );
        std::fs::copy(source, &target).unwrap();
        target
    } else {
        path
    };
    let mut t = track("Accepted title");
    t.locations.native_path = Some(path.clone());
    t.locations.platform = Some(std::env::consts::OS.into());
    observation(&mut t);
    let i = f.inspect(vec![t], Completeness::WholeLibrary);
    let mut r = f.request(&i);
    r.policy = policy;
    let op = f.apply(&f.preview(r)).operation_id;
    let id = f.lib.tracks(TrackQuery::default()).unwrap().rows[0]
        .id
        .clone();
    (f, op, id, path)
}

#[cfg(feature = "autotag")]
fn archived_import_file_preview_requires_restore(copy: bool) {
    let destination = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let (f, _, id, path) = setup(ImportPolicy::default());
    let mut policy = ImportPolicy::default();
    if copy {
        let root = f
            .lib
            .register_organization_root(destination.path().to_owned())
            .unwrap();
        policy.relocation = RelocationPolicy::Copy {
            root_id: root.id,
            rule: "{title}".into(),
        };
    } else {
        policy.write_audio_tags = ["title".into()].into();
    }
    let track = f.lib.imported_item(&id).unwrap().observations[0]
        .track
        .clone();
    let inspection = f.inspect(vec![track], Completeness::WholeLibrary);
    let mut request = f.request(&inspection);
    request.policy = policy;
    let op = f.apply(&f.preview(request)).operation_id;
    let service = ImportProcessingService::start(f.lib.clone(), runtime()).unwrap();
    service.run_pending(&CancellationToken::default()).unwrap();
    f.lib.set_archived(&id, true).unwrap();
    let before = std::fs::read(&path).unwrap();
    let counts = f.lib.counts().unwrap();
    assert_eq!((counts.tracks, counts.archive, counts.incoming), (0, 1, 0));
    let preview = f
        .lib
        .preview_import_effects(&op, &CancellationToken::default())
        .unwrap();
    assert_eq!((preview.ready, preview.blocked), (0, 1));
    let page = f
        .lib
        .import_effects_page(&preview.preview_id, None, 100)
        .unwrap();
    let reason = page.records[0].reason.as_deref().unwrap();
    assert!(
        reason.contains("Restore") && reason.contains("Archive"),
        "{reason}"
    );
    f.lib.discard_import_preview(&preview.preview_id).unwrap();
    assert!(f.lib.track(&id).unwrap().archived);
    let counts = f.lib.counts().unwrap();
    assert_eq!((counts.tracks, counts.archive, counts.incoming), (0, 1, 0));
    assert_eq!(
        f.lib
            .tracks(TrackQuery {
                view: TrackView::Archive,
                ..Default::default()
            })
            .unwrap()
            .total,
        1
    );
    assert_eq!(
        f.lib
            .conn()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM tracks WHERE id=?1", [&id], |r| r
                .get::<_, u64>(0))
            .unwrap(),
        0
    );
    assert!(
        f.lib
            .conn()
            .unwrap()
            .query_row(
                "SELECT local_track_id FROM import_items WHERE id=?1",
                [&id],
                |r| r.get::<_, Option<String>>(0)
            )
            .unwrap()
            .is_none()
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(std::fs::read_dir(destination.path()).unwrap().count(), 0);
    f.lib.set_archived(&id, false).unwrap();
    let preview = f
        .lib
        .preview_import_effects(&op, &CancellationToken::default())
        .unwrap();
    assert_eq!(
        (preview.ready, preview.blocked),
        (1, 0),
        "{:?}",
        f.lib
            .import_effects_page(&preview.preview_id, None, 100)
            .unwrap()
    );
    assert!(!f.lib.track(&id).unwrap().archived);
    let counts = f.lib.counts().unwrap();
    assert_eq!((counts.tracks, counts.archive, counts.incoming), (1, 0, 0));
    f.lib.discard_import_preview(&preview.preview_id).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(std::fs::read_dir(destination.path()).unwrap().count(), 0);
    assert!(f.lib.jobs().unwrap().is_empty());
}

#[cfg(feature = "autotag")]
#[test]
fn archived_import_tag_preview_requires_restore_without_promoting_or_modifying_media() {
    archived_import_file_preview_requires_restore(false);
}

#[cfg(feature = "autotag")]
#[test]
fn archived_import_copy_preview_requires_restore_without_promoting_or_modifying_media() {
    archived_import_file_preview_requires_restore(true);
}

#[cfg(feature = "autotag")]
#[test]
fn promotion_preview_preserves_dates_and_empty_title_without_adopting_placeholder_tags() {
    for missing_title in [false, true] {
        let (f, op, id, path) = setup_observation(
            ImportPolicy {
                write_audio_tags: ["album".into()].into(),
                ..Default::default()
            },
            "wav",
            "pcm_s16le",
            |_| {},
            |track| {
                if missing_title {
                    track.metadata.remove("title");
                } else {
                    track
                        .metadata
                        .insert("title".into(), Presence::Empty(json!("")));
                }
                track
                    .metadata
                    .insert("album".into(), Presence::Value(json!("Album from source")));
                track
                    .metadata
                    .insert("duration".into(), Presence::Value(json!(321.0)));
            },
        );
        let service = ImportProcessingService::start(f.lib.clone(), runtime()).unwrap();
        service.run_pending(&CancellationToken::default()).unwrap();
        f.lib
            .conn()
            .unwrap()
            .execute(
                "UPDATE import_items SET created=123,updated=456 WHERE id=?1",
                [&id],
            )
            .unwrap();
        let fields = f.lib.imported_item(&id).unwrap().active_fields;
        let before = std::fs::read(&path).unwrap();
        let summary = f.lib.track(&id).unwrap();
        assert_eq!(summary.title, "Imported item");
        assert_eq!(
            (
                summary.metadata["library.date_added"].clone(),
                summary.metadata["library.date_modified"].clone()
            ),
            (json!(123), json!(456))
        );
        assert_eq!(
            f.lib
                .tracks(TrackQuery {
                    search: "album from source".into(),
                    ..Default::default()
                })
                .unwrap()
                .total,
            1
        );
        let preview = f
            .lib
            .preview_import_effects(&op, &CancellationToken::default())
            .unwrap();
        assert_eq!(
            preview.ready,
            1,
            "{:?}",
            f.lib
                .import_effects_page(&preview.preview_id, None, 100)
                .unwrap()
        );
        let summary = f.lib.track(&id).unwrap();
        assert_eq!(
            (
                summary.title.as_str(),
                summary.metadata["library.date_added"].clone(),
                summary.metadata["library.date_modified"].clone()
            ),
            ("Imported item", json!(123), json!(456))
        );
        assert_eq!(
            f.lib
                .conn()
                .unwrap()
                .query_row("SELECT title FROM tracks WHERE id=?1", [&id], |r| r
                    .get::<_, String>(0))
                .unwrap(),
            ""
        );
        assert_eq!(f.lib.imported_item(&id).unwrap().active_fields, fields);
        assert_eq!(
            f.lib
                .tracks(TrackQuery {
                    search: "album from source".into(),
                    ..Default::default()
                })
                .unwrap()
                .total,
            1
        );
        f.lib.discard_import_preview(&preview.preview_id).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(
            f.lib.track(&id).unwrap().metadata["library.date_modified"],
            456
        );
        let preview = f
            .lib
            .preview_import_effects(&op, &CancellationToken::default())
            .unwrap();
        f.lib
            .apply_import_effects(&preview.preview_id, &CancellationToken::default())
            .unwrap();
        let native =
            cuerithm_autotag::read_metadata_snapshot(&path, &runtime().probe(&path).unwrap())
                .unwrap();
        assert!(
            native
                .text
                .get(&cuerithm_autotag::MetadataTextField::Title)
                .is_none_or(Vec::is_empty)
        );
        assert_eq!(
            native.text.get(&cuerithm_autotag::MetadataTextField::Album),
            Some(&vec!["Album from source".into()])
        );
        let summary = f.lib.track(&id).unwrap();
        assert_eq!(summary.title, "Imported item");
        assert_eq!(summary.metadata["library.date_added"], 123);
        assert!(summary.metadata["library.date_modified"].as_i64().unwrap() > 456);
        assert_eq!(f.lib.imported_item(&id).unwrap().active_fields, fields);
    }
}

#[cfg(feature = "autotag")]
#[test]
fn import_duration_enrichment_matches_audio_without_adopting_its_duration() {
    use cuerithm_autotag::*;
    struct Catalog {
        seconds: u64,
        seen: std::sync::Arc<std::sync::Mutex<Vec<f64>>>,
    }
    impl ProviderBackend for Catalog {
        fn id(&self) -> ProviderId {
            ProviderId::Beatport
        }
        fn candidates(
            &self,
            input: &TagInput,
            _: &AutotagConfig,
        ) -> anyhow::Result<Vec<CatalogMatch>> {
            self.seen.lock().unwrap().push(input.duration_seconds);
            Ok(vec![CatalogMatch::new(
                1.0,
                CatalogTrack {
                    platform: "beatport".into(),
                    title: "Catalog title".into(),
                    duration: std::time::Duration::from_secs(self.seconds),
                    ..Default::default()
                },
            )])
        }
    }
    for seconds in [321, 1] {
        let policy = ImportPolicy {
            metadata_enrichment: EnrichmentPolicy {
                enabled: true,
                providers: vec!["beatport".into()],
                fields: ["title".into()].into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let (f, op, id, _) = setup_observation(
            policy,
            "wav",
            "pcm_s16le",
            |f| {
                let mut settings = f.lib.settings().unwrap().settings;
                let mut config = AutotagConfig::default();
                config.options.match_duration = true;
                config.options.max_duration_difference = 2;
                settings.tagging = Some(config);
                f.lib.save_settings(settings).unwrap();
            },
            |track| {
                track
                    .metadata
                    .insert("duration".into(), Presence::Value(json!(321.0)));
            },
        );
        let before = f.lib.imported_item(&id).unwrap().active_fields;
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let service = ImportProcessingService::start_with_tagger(
            f.lib.clone(),
            runtime(),
            std::sync::Arc::new(
                Tagger::with_backends(vec![std::sync::Arc::new(Catalog {
                    seconds,
                    seen: seen.clone(),
                })])
                .unwrap(),
            ),
        )
        .unwrap();
        let result = service.run_pending(&CancellationToken::default()).unwrap();
        assert_eq!(
            (result.processed, result.blocked, result.failed),
            if seconds == 1 { (1, 0, 0) } else { (0, 1, 0) },
            "candidate duration {seconds}"
        );
        assert_eq!(*seen.lock().unwrap(), vec![1.0]);
        assert_eq!(f.lib.imported_item(&id).unwrap().active_fields, before);
        if seconds == 1 {
            let preview = f
                .lib
                .preview_import_effects(&op, &CancellationToken::default())
                .unwrap();
            assert_eq!(preview.ready, 1);
            f.lib
                .apply_import_effects(&preview.preview_id, &CancellationToken::default())
                .unwrap();
            assert_eq!(f.lib.track(&id).unwrap().title, "Catalog title");
        }
        assert_eq!(f.lib.track(&id).unwrap().duration, Some(321.0));
        assert_eq!(
            f.lib.imported_item(&id).unwrap().active_fields["metadata.duration"],
            before["metadata.duration"]
        );
        assert!(f.lib.analysis(&id).unwrap().is_none());
        assert!(f.lib.jobs().unwrap().is_empty());
        assert_eq!(f.lib.cue_revision_count(&id).unwrap(), 0);
    }
}

#[cfg(feature = "autotag")]
#[test]
fn import_duration_enrichment_keeps_its_frozen_decoder_identity() {
    use cuerithm_autotag::*;
    struct Catalog(std::sync::Arc<std::sync::atomic::AtomicUsize>);
    impl ProviderBackend for Catalog {
        fn id(&self) -> ProviderId {
            ProviderId::Beatport
        }
        fn candidates(&self, _: &TagInput, _: &AutotagConfig) -> anyhow::Result<Vec<CatalogMatch>> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(vec![])
        }
    }
    let (f, op, id, _) = setup_observation(
        ImportPolicy {
            metadata_enrichment: EnrichmentPolicy {
                enabled: true,
                providers: vec!["beatport".into()],
                fields: ["title".into()].into(),
                ..Default::default()
            },
            ..Default::default()
        },
        "wav",
        "pcm_s16le",
        |f| {
            drop(ImportProcessingService::start(f.lib.clone(), runtime()).unwrap());
        },
        |_| {},
    );
    let frozen: Option<String> = f.lib.conn().unwrap().query_row("SELECT json_extract(p.json,'$.processing.decoder_identity') FROM import_previews p JOIN import_operations o ON o.preview_id=p.id WHERE o.id=?1", [&op], |r| r.get(0)).unwrap();
    assert_eq!(frozen.as_deref(), Some(runtime().identity.as_str()));
    let before = f.lib.imported_item(&id).unwrap().active_fields;
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut changed = runtime();
    changed.identity.push_str(":different");
    let service = ImportProcessingService::start_with_tagger(
        f.lib.clone(),
        changed,
        std::sync::Arc::new(
            Tagger::with_backends(vec![std::sync::Arc::new(Catalog(calls.clone()))]).unwrap(),
        ),
    )
    .unwrap();
    let result = service.run_pending(&CancellationToken::default()).unwrap();
    assert_eq!((result.processed, result.blocked, result.failed), (0, 1, 0));
    let stages = f.lib.import_processing_stages(&op, None, 100).unwrap();
    assert!(
        stages.records[0]
            .error
            .as_deref()
            .unwrap()
            .contains("Decoder changed after preview")
    );
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(f.lib.imported_item(&id).unwrap().active_fields, before);
    assert!(f.lib.analysis(&id).unwrap().is_none());
    assert!(f.lib.jobs().unwrap().is_empty());
}

#[cfg(feature = "autotag")]
#[test]
fn import_duration_survives_file_preview_discard_apply_and_undo() {
    for edit_local in [false, true] {
        let (f, op, id, _) = setup_observation(
            ImportPolicy {
                write_audio_tags: ["title".into()].into(),
                ..Default::default()
            },
            "wav",
            "pcm_s16le",
            |_| {},
            |track| {
                track
                    .metadata
                    .insert("duration".into(), Presence::Value(json!(321.0)));
            },
        );
        assert_eq!(f.lib.track(&id).unwrap().duration, Some(321.0));
        let service = ImportProcessingService::start(f.lib.clone(), runtime()).unwrap();
        service.run_pending(&CancellationToken::default()).unwrap();
        let preview = f
            .lib
            .preview_import_effects(&op, &CancellationToken::default())
            .unwrap();
        assert_eq!(preview.ready, 1);
        assert_eq!(f.lib.track(&id).unwrap().duration, Some(321.0));
        f.lib.discard_import_preview(&preview.preview_id).unwrap();
        assert_eq!(f.lib.track(&id).unwrap().duration, Some(321.0));
        if edit_local {
            f.lib
                .conn()
                .unwrap()
                .execute("UPDATE tracks SET duration=444 WHERE id=?1", [&id])
                .unwrap();
        }
        let preview = f
            .lib
            .preview_import_effects(&op, &CancellationToken::default())
            .unwrap();
        assert_eq!(preview.ready, 1);
        f.lib
            .apply_import_effects(&preview.preview_id, &CancellationToken::default())
            .unwrap();
        assert_eq!(
            f.lib.track(&id).unwrap().duration,
            Some(if edit_local { 444.0 } else { 321.0 })
        );
        if edit_local {
            f.lib
                .conn()
                .unwrap()
                .execute("UPDATE tracks SET duration=555 WHERE id=?1", [&id])
                .unwrap();
        }
        let undo = f
            .lib
            .undo_import_operation(&op, &CancellationToken::default())
            .unwrap();
        assert_eq!(undo.blocked, u64::from(edit_local), "{:?}", undo.errors);
        assert_eq!(
            f.lib.track(&id).unwrap().duration,
            Some(if edit_local { 555.0 } else { 321.0 })
        );
        assert!(f.lib.analysis(&id).unwrap().is_none());
        assert!(f.lib.jobs().unwrap().is_empty());
        assert_eq!(f.lib.cue_revision_count(&id).unwrap(), 0);
    }
}

#[test]
fn review_fix_unrelated_native_edits_survive_effect_publication_and_undo() {
    let policy = ImportPolicy {
        analysis_fields: [(AnalysisField::Key, AnalysisAdoption::ReplaceSelected)].into(),
        ..Default::default()
    };
    let (f, op, id, path) = setup(policy);
    f.lib.conn().unwrap().execute("INSERT INTO tracks(id,path,title,source_hash,state,metadata_json,created,updated) SELECT id,?1,'Edited locally',source_hash,'ready','{}',created,updated FROM import_items WHERE id=?2",rusqlite::params![crate::model::path_bytes(&path),id]).unwrap();
    f.lib
        .conn()
        .unwrap()
        .execute(
            "UPDATE import_items SET local_track_id=id WHERE id=?1",
            [&id],
        )
        .unwrap();
    let service = ImportProcessingService::start(f.lib.clone(), runtime()).unwrap();
    service.run_pending(&CancellationToken::default()).unwrap();
    let preview = f
        .lib
        .preview_import_effects(&op, &CancellationToken::default())
        .unwrap();
    assert_eq!(preview.ready, 1);
    f.lib
        .apply_import_effects(&preview.preview_id, &CancellationToken::default())
        .unwrap();
    let native = || {
        f.lib
            .conn()
            .unwrap()
            .query_row(
                "SELECT title,bpm,energy FROM tracks WHERE id=?1",
                [&id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, Option<f64>>(1)?,
                        r.get::<_, Option<u8>>(2)?,
                    ))
                },
            )
            .unwrap()
    };
    assert_eq!(native().0, "Edited locally");
    f.lib
        .conn()
        .unwrap()
        .execute(
            "UPDATE tracks SET title='Edited after publication',bpm=143,energy=8 WHERE id=?1",
            [&id],
        )
        .unwrap();
    let undo = f
        .lib
        .undo_import_operation(&op, &CancellationToken::default())
        .unwrap();
    assert_eq!(undo.blocked, 0, "{:?}", undo.errors);
    assert_eq!(
        native(),
        ("Edited after publication".into(), Some(143.), Some(8))
    );
}
#[test]
fn review_fix_refresh_cleanup_respects_descriptive_field_mask() {
    let (f, _, id, _) = setup_observation(
        ImportPolicy::default(),
        "wav",
        "pcm_s16le",
        |f| {
            f.lib
                .save_settings(PreparationSettings {
                    cleanup: Some(cuerithm_cleanup::CleanupConfig {
                        fixes: vec![cuerithm_cleanup::ConfiguredFix {
                            id: "decode".into(),
                            enabled: true,
                            ignored_values: Default::default(),
                            rule: cuerithm_cleanup::SmartFix::DecodeEntities,
                        }],
                        ..Default::default()
                    }),
                    ..Default::default()
                })
                .unwrap();
        },
        |t| {
            t.metadata
                .insert("title".into(), Presence::Value(json!("A &amp; B")));
        },
    );
    let t = f.lib.imported_item(&id).unwrap().observations[0]
        .track
        .clone();
    let inspection = f.inspect(vec![t], Completeness::WholeLibrary);
    let mut request = f.request(&inspection);
    request.policy.descriptive_fields = Some(["artist".into()].into());
    request.policy.cleanup = CleanupPolicy {
        enabled: true,
        rules: vec!["decode".into()],
    };
    let op = f.apply(&f.preview(request)).operation_id;
    let service = ImportProcessingService::start(f.lib.clone(), runtime()).unwrap();
    service.run_pending(&CancellationToken::default()).unwrap();
    let preview = f
        .lib
        .preview_import_effects(&op, &CancellationToken::default())
        .unwrap();
    f.lib
        .apply_import_effects(&preview.preview_id, &CancellationToken::default())
        .unwrap();
    assert_eq!(
        f.lib.imported_item(&id).unwrap().active_fields["metadata.title"]["value"],
        "A &amp; B"
    );
}
#[cfg(feature = "autotag")]
#[test]
fn review_fix_retry_restores_dependencies_and_orders_multiple_items() {
    use cuerithm_autotag::*;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    struct Catalog(Arc<AtomicBool>);
    impl ProviderBackend for Catalog {
        fn id(&self) -> ProviderId {
            ProviderId::Beatport
        }
        fn candidates(&self, _: &TagInput, _: &AutotagConfig) -> anyhow::Result<Vec<CatalogMatch>> {
            Ok(if self.0.load(Ordering::SeqCst) {
                vec![CatalogMatch::new(
                    1.,
                    CatalogTrack {
                        platform: "beatport".into(),
                        title: "A &amp; B".into(),
                        ..Default::default()
                    },
                )]
            } else {
                vec![]
            })
        }
    }
    for cancelled in [false, true] {
        let (f, _, id, path) = setup_configured(ImportPolicy::default(), "wav", "pcm_s16le", |f| {
            f.lib
                .save_settings(PreparationSettings {
                    cleanup: Some(cuerithm_cleanup::CleanupConfig {
                        fixes: vec![cuerithm_cleanup::ConfiguredFix {
                            id: "decode".into(),
                            enabled: true,
                            ignored_values: Default::default(),
                            rule: cuerithm_cleanup::SmartFix::DecodeEntities,
                        }],
                        ..Default::default()
                    }),
                    ..Default::default()
                })
                .unwrap();
        });
        let first = f.lib.imported_item(&id).unwrap().observations[0]
            .track
            .clone();
        let mut second = first.clone();
        second.external_id = "b".into();
        let second_path = path.with_file_name("second.wav");
        std::fs::copy(&path, &second_path).unwrap();
        second.locations.native_path = Some(second_path);
        let inspection = f.inspect(vec![first, second], Completeness::WholeLibrary);
        let mut request = f.request(&inspection);
        request.policy.metadata_enrichment = EnrichmentPolicy {
            enabled: true,
            providers: vec!["beatport".into()],
            fields: ["title".into()].into(),
            ..Default::default()
        };
        request.policy.cleanup = CleanupPolicy {
            enabled: true,
            rules: vec!["decode".into()],
        };
        let op = f.apply(&f.preview(request)).operation_id;
        let available = Arc::new(AtomicBool::new(cancelled));
        let service = ImportProcessingService::start_with_tagger(
            f.lib.clone(),
            runtime(),
            Arc::new(Tagger::with_backends(vec![Arc::new(Catalog(available.clone()))]).unwrap()),
        )
        .unwrap();
        let token = CancellationToken::default();
        if cancelled {
            let claimed = Arc::new(AtomicUsize::new(0));
            let cancel = token.clone();
            service.set_claim_hook(Box::new(move || {
                if claimed.fetch_add(1, Ordering::SeqCst) == 1 {
                    cancel.cancel();
                }
            }));
        }
        let first = service.run_pending(&token).unwrap();
        assert!(if cancelled {
            first.cancelled > 0
        } else {
            first.blocked > 0
        });
        available.store(true, Ordering::SeqCst);
        service.set_claim_hook(Box::new(|| {}));
        service
            .retry_pending(&op, &CancellationToken::default())
            .unwrap();
        let preview = f
            .lib
            .preview_import_effects(&op, &CancellationToken::default())
            .unwrap();
        assert_eq!(
            preview.blocked,
            0,
            "{:?}",
            f.lib
                .import_effects_page(&preview.preview_id, None, 100)
                .unwrap()
        );
        assert_eq!(preview.ready, 4);
        f.lib
            .apply_import_effects(&preview.preview_id, &CancellationToken::default())
            .unwrap();
        for row in f.lib.tracks(TrackQuery::default()).unwrap().rows {
            assert_eq!(row.title, "A & B");
        }
    }
}
#[cfg(feature = "autotag")]
#[test]
fn review_fix_first_tag_and_copy_handle_both_artist_and_genre_representations() {
    for plural in [false, true] {
        for copy in [false, true] {
            let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
            let (f, _, id, path) = setup_observation(
                ImportPolicy::default(),
                "wav",
                "pcm_s16le",
                |_| {},
                |t| {
                    for (singular, plural_name, value) in [
                        ("artist", "artists", "Artist"),
                        ("genre", "genres", "House"),
                    ] {
                        t.metadata.insert(
                            if plural { plural_name } else { singular }.into(),
                            Presence::Value(if plural { json!([value]) } else { json!(value) }),
                        );
                    }
                },
            );
            let mut policy = ImportPolicy::default();
            if copy {
                let destination = f
                    .lib
                    .register_organization_root(root.path().to_owned())
                    .unwrap();
                policy.relocation = RelocationPolicy::Copy {
                    root_id: destination.id,
                    rule: "{artist} - {title}".into(),
                };
            } else {
                policy.write_audio_tags = ["title".into(), "artist".into(), "genre".into()].into();
            }
            let t = f.lib.imported_item(&id).unwrap().observations[0]
                .track
                .clone();
            let inspection = f.inspect(vec![t], Completeness::WholeLibrary);
            let mut request = f.request(&inspection);
            request.policy = policy;
            let op = f.apply(&f.preview(request)).operation_id;
            let service = ImportProcessingService::start(f.lib.clone(), runtime()).unwrap();
            service.run_pending(&CancellationToken::default()).unwrap();
            let preview = f
                .lib
                .preview_import_effects(&op, &CancellationToken::default())
                .unwrap();
            let page = f
                .lib
                .import_effects_page(&preview.preview_id, None, 100)
                .unwrap();
            assert_eq!(preview.ready, 1, "{page:?}");
            let row = &page.records[0];
            f.lib
                .apply_import_effects(&preview.preview_id, &CancellationToken::default())
                .unwrap();
            if copy {
                assert!(
                    row.changes["files"][0]["before_path"]
                        .as_str()
                        .unwrap()
                        .ends_with("original.wav")
                );
                assert!(
                    row.changes["files"][0]["after_path"]
                        .as_str()
                        .unwrap()
                        .ends_with("Artist - Accepted title.wav")
                );
            } else {
                assert!(row.changes["audio_tags"][0]["before"].is_object());
                assert!(
                    row.changes["audio_tags"][0]["after"]
                        .to_string()
                        .contains("Accepted title")
                );
            }
            assert!(path.exists());
        }
    }
}
#[cfg(feature = "autotag")]
#[test]
fn review_fix_file_effects_expose_actual_tag_changes_before_apply() {
    let (f, op, _, path) = setup(ImportPolicy {
        write_audio_tags: ["title".into()].into(),
        ..Default::default()
    });
    let original = std::fs::read(path).unwrap();
    let service = ImportProcessingService::start(f.lib.clone(), runtime()).unwrap();
    service.run_pending(&CancellationToken::default()).unwrap();
    let preview = f
        .lib
        .preview_import_effects(&op, &CancellationToken::default())
        .unwrap();
    let page = f
        .lib
        .import_effects_page(&preview.preview_id, None, 100)
        .unwrap();
    let row = &page.records[0];
    assert!(row.changes["audio_tags"][0]["before"].is_object());
    assert!(
        row.changes["audio_tags"][0]["after"]
            .to_string()
            .contains("Accepted title")
    );
    assert_eq!(
        std::fs::read(row.changes["audio_tags"][0]["path"].as_str().unwrap()).unwrap(),
        original
    );
}
#[cfg(feature = "autotag")]
#[test]
fn review_fix_file_tag_projection_preserves_import_shapes_and_later_unrelated_edits() {
    let (f, op, id, _) = setup_observation(
        ImportPolicy {
            write_audio_tags: ["title".into(), "label".into()].into(),
            ..Default::default()
        },
        "wav",
        "pcm_s16le",
        |_| {},
        |track| {
            track
                .metadata
                .insert("artists".into(), Presence::Value(json!(["Artist"])));
            track
                .metadata
                .insert("genres".into(), Presence::Value(json!(["House"])));
            track
                .metadata
                .insert("label".into(), Presence::Value(json!(["Label"])));
        },
    );
    let before = f.lib.imported_item(&id).unwrap();
    let service = ImportProcessingService::start(f.lib.clone(), runtime()).unwrap();
    service.run_pending(&CancellationToken::default()).unwrap();
    let preview = f
        .lib
        .preview_import_effects(&op, &CancellationToken::default())
        .unwrap();
    f.lib
        .apply_import_effects(&preview.preview_id, &CancellationToken::default())
        .unwrap();
    let after = f.lib.imported_item(&id).unwrap();
    assert_eq!(
        after.active_fields["metadata.label"],
        before.active_fields["metadata.label"]
    );
    assert_eq!(
        after.provenance["metadata.label"],
        before.provenance["metadata.label"]
    );
    let rows = f.lib.tracks(TrackQuery::default()).unwrap();
    assert_eq!(
        (&rows.rows[0].artist, &rows.rows[0].genre),
        (&"Artist".to_owned(), &"House".to_owned())
    );
    f.lib
        .conn()
        .unwrap()
        .execute("UPDATE tracks SET bpm=143,energy=8 WHERE id=?1", [&id])
        .unwrap();
    let undo = f
        .lib
        .undo_import_operation(&op, &CancellationToken::default())
        .unwrap();
    assert_eq!(undo.blocked, 1, "{:?}", undo.errors);
    let after = f.lib.tracks(TrackQuery::default()).unwrap();
    assert_eq!(
        (after.rows[0].bpm, after.rows[0].energy),
        (Some(143.), Some(8))
    );
    let detail = f.lib.imported_item(&id).unwrap();
    assert_eq!(
        detail.active_fields["metadata.label"],
        before.active_fields["metadata.label"]
    );
}
#[test]
fn effects_require_review_and_preserve_later_local_edits() {
    let p = ImportPolicy {
        analysis_fields: [(AnalysisField::Key, AnalysisAdoption::ReplaceSelected)].into(),
        ..Default::default()
    };
    let (f, op, id, _) = setup(p);
    let s = ImportProcessingService::start(f.lib.clone(), runtime()).unwrap();
    s.run_pending(&CancellationToken::default()).unwrap();
    let before = f.lib.imported_item(&id).unwrap().active_fields;
    let preview = f
        .lib
        .preview_import_effects(&op, &CancellationToken::default())
        .unwrap();
    assert_eq!(f.lib.imported_item(&id).unwrap().active_fields, before);
    f.lib.conn().unwrap().execute("INSERT INTO import_active_fields(item_id,field,value_json,provenance_json) VALUES(?1,'key',?2,'{}') ON CONFLICT(item_id,field) DO UPDATE SET value_json=excluded.value_json",rusqlite::params![id,json!({"presence":"value","raw":"Local edit"}).to_string()]).unwrap();
    assert!(
        f.lib
            .apply_import_effects(&preview.preview_id, &CancellationToken::default())
            .is_err()
    );
    assert_eq!(
        f.lib.imported_item(&id).unwrap().active_fields["key"]["raw"],
        "Local edit"
    );
}
#[test]
fn measured_effect_apply_and_undo_are_idempotent() {
    let p = ImportPolicy {
        analysis_fields: [(AnalysisField::Key, AnalysisAdoption::ReplaceSelected)].into(),
        ..Default::default()
    };
    let (f, op, id, _) = setup(p);
    let s = ImportProcessingService::start(f.lib.clone(), runtime()).unwrap();
    s.run_pending(&CancellationToken::default()).unwrap();
    let before = f.lib.imported_item(&id).unwrap().active_fields;
    let p = f
        .lib
        .preview_import_effects(&op, &CancellationToken::default())
        .unwrap();
    assert_eq!(p.ready, 1);
    let a = f
        .lib
        .apply_import_effects(&p.preview_id, &CancellationToken::default())
        .unwrap();
    assert_eq!(a.processed, 1);
    assert_eq!(
        f.lib
            .apply_import_effects(&p.preview_id, &CancellationToken::default())
            .unwrap()
            .processed,
        1
    );
    assert_eq!(
        f.lib.imported_item(&id).unwrap().active_fields["key"]["provenance"],
        "measured"
    );
    f.lib
        .undo_import_operation(&op, &CancellationToken::default())
        .unwrap();
    assert_eq!(f.lib.imported_item(&id).unwrap().active_fields, before);
    f.lib
        .undo_import_operation(&op, &CancellationToken::default())
        .unwrap();
    assert!(f.lib.jobs().unwrap().is_empty());
}
#[cfg(feature = "autotag")]
#[test]
fn missing_credential_blocks_only_requested_stage() {
    let p = ImportPolicy {
        metadata_enrichment: EnrichmentPolicy {
            enabled: true,
            providers: vec!["discogs".into()],
            fields: ["title".into()].into(),
            credential_refs: [("discogs".into(), "absent".into())].into(),
        },
        write_audio_tags: ["title".into()].into(),
        ..Default::default()
    };
    let (f, op, id, path) = setup(p);
    let s = ImportProcessingService::start(f.lib.clone(), runtime()).unwrap();
    let result = s.run_pending(&CancellationToken::default()).unwrap();
    assert_eq!(result.blocked, 1);
    let preview = f
        .lib
        .preview_import_effects(&op, &CancellationToken::default())
        .unwrap();
    assert_eq!(preview.ready, 1);
    assert_eq!(preview.blocked, 1);
    let result = f
        .lib
        .apply_import_effects(&preview.preview_id, &CancellationToken::default())
        .unwrap();
    assert_eq!(result.processed, 1);
    assert_eq!(
        runtime()
            .probe(&path)
            .unwrap()
            .tags
            .get("title")
            .map(String::as_str),
        Some("Accepted title")
    );
    assert!(f.lib.jobs().unwrap().is_empty());
    assert_eq!(f.lib.tracks(TrackQuery::default()).unwrap().rows[0].id, id);
}
#[cfg(feature = "autotag")]
#[test]
fn accepted_imported_tags_write_without_provider_lookup() {
    let p = ImportPolicy {
        write_audio_tags: ["title".into()].into(),
        ..Default::default()
    };
    let (f, op, id, path) = setup(p);
    let before = std::fs::read(&path).unwrap();
    let original_pcm = runtime()
        .decode(&path, &f.root.path().join("before-cache"))
        .unwrap()
        .pcm_sha256()
        .unwrap();
    let s = ImportProcessingService::start(f.lib.clone(), runtime()).unwrap();
    s.run_pending(&CancellationToken::default()).unwrap();
    let p = f
        .lib
        .preview_import_effects(&op, &CancellationToken::default())
        .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(p.ready, 1);
    let r = f
        .lib
        .apply_import_effects(&p.preview_id, &CancellationToken::default())
        .unwrap();
    assert_eq!(r.processed, 1);
    assert_eq!(
        runtime()
            .probe(&path)
            .unwrap()
            .tags
            .get("title")
            .map(String::as_str),
        Some("Accepted title")
    );
    assert_eq!(
        runtime()
            .decode(&path, &f.root.path().join("after-cache"))
            .unwrap()
            .pcm_sha256()
            .unwrap(),
        original_pcm
    );
    assert_eq!(path.file_name().unwrap(), "original.wav");
    assert!(f.lib.jobs().unwrap().is_empty());
    assert!(f.lib.analysis(&id).unwrap().is_none());
    f.lib
        .undo_import_operation(&op, &CancellationToken::default())
        .unwrap();
    assert_restored(f.root.path(), &before, &path, "undo");
}
#[cfg(feature = "autotag")]
#[test]
fn raw_imported_key_is_written_and_used_for_file_naming() {
    let policy = ImportPolicy {
        write_audio_tags: ["key".into()].into(),
        relocation: RelocationPolicy::RenameInPlace {
            rule: "{key}".into(),
        },
        ..Default::default()
    };
    let (f, op, id, path) = setup_observation(
        policy,
        "flac",
        "flac",
        |_| {},
        |track| {
            track.key = Some(MusicalKey {
                presence: FieldPresence::Value,
                raw: Some("Am".into()),
                ..Default::default()
            });
        },
    );
    let before = std::fs::read(&path).unwrap();
    f.lib
        .conn()
        .unwrap()
        .execute("DELETE FROM import_bindings WHERE item_id=?1", [&id])
        .unwrap();
    let service = ImportProcessingService::start(f.lib.clone(), runtime()).unwrap();
    service.run_pending(&CancellationToken::default()).unwrap();
    let preview = f
        .lib
        .preview_import_effects(&op, &CancellationToken::default())
        .unwrap();
    assert_eq!(
        preview.blocked,
        0,
        "{:?}",
        f.lib
            .import_effects_page(&preview.preview_id, None, 100)
            .unwrap()
    );
    let result = f
        .lib
        .apply_import_effects(&preview.preview_id, &CancellationToken::default())
        .unwrap();
    assert_eq!(result.processed, 2);
    let final_path = path.with_file_name("Am.flac");
    assert_eq!(f.lib.track_path(&id).unwrap(), final_path);
    let media = runtime().probe(&final_path).unwrap();
    let tags = cuerithm_autotag::read_metadata_snapshot(&final_path, &media).unwrap();
    assert_eq!(
        tags.text.get(&cuerithm_autotag::MetadataTextField::Key),
        Some(&vec!["Am".into()])
    );
    assert_eq!(f.lib.track(&id).unwrap().key.as_deref(), Some("Am"));
    let undo = f
        .lib
        .undo_import_operation(&op, &CancellationToken::default())
        .unwrap();
    assert_eq!(undo.blocked, 0, "{:?}", undo.errors);
    assert_restored(f.root.path(), &before, &path, "undo");
}

#[cfg(feature = "autotag")]
#[test]
fn original_media_tag_preview_and_apply_require_the_source_app_closed() {
    let (f, op, _, path) = setup(ImportPolicy {
        write_audio_tags: ["title".into()].into(),
        ..Default::default()
    });
    let processor = ImportProcessingService::start(f.lib.clone(), runtime()).unwrap();
    processor
        .run_pending(&CancellationToken::default())
        .unwrap();
    cuerithm_dj_sync::discovery::pretend_target_running(true);
    let blocked = f
        .lib
        .preview_import_effects(&op, &CancellationToken::default())
        .unwrap();
    cuerithm_dj_sync::discovery::pretend_target_running(false);
    assert_eq!(blocked.ready, 0);
    assert_eq!(blocked.blocked, 1);
    let preview = f
        .lib
        .preview_import_effects(&op, &CancellationToken::default())
        .unwrap();
    assert_eq!(preview.ready, 1);
    let before = std::fs::read(&path).unwrap();
    cuerithm_dj_sync::discovery::pretend_target_running(true);
    let result = f
        .lib
        .apply_import_effects(&preview.preview_id, &CancellationToken::default());
    cuerithm_dj_sync::discovery::pretend_target_running(false);
    assert!(result.is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    f.lib
        .apply_import_effects(&preview.preview_id, &CancellationToken::default())
        .unwrap();
    let published = std::fs::read(&path).unwrap();
    cuerithm_dj_sync::discovery::pretend_target_running(true);
    let undo = f
        .lib
        .undo_import_operation(&op, &CancellationToken::default())
        .unwrap();
    cuerithm_dj_sync::discovery::pretend_target_running(false);
    assert_eq!(undo.blocked, 1);
    assert_eq!(std::fs::read(&path).unwrap(), published);
    assert_eq!(
        f.lib
            .undo_import_operation(&op, &CancellationToken::default())
            .unwrap()
            .undone,
        1
    );
    assert_restored(f.root.path(), &before, &path, "undo");
}
#[cfg(feature = "autotag")]
#[test]
fn accepted_catalog_number_writes_through_the_qualified_catalog_tag() {
    let (f, op, id, path) = setup(ImportPolicy {
        write_audio_tags: ["catalog_number".into()].into(),
        ..Default::default()
    });
    f.lib.conn().unwrap().execute("INSERT INTO import_active_fields(item_id,field,value_json,provenance_json) VALUES(?1,'metadata.catalog_number',?2,'{}')",rusqlite::params![id,serde_json::json!({"presence":"value","value":"AUTHORED001"}).to_string()]).unwrap();
    let processor = ImportProcessingService::start(f.lib.clone(), runtime()).unwrap();
    processor
        .run_pending(&CancellationToken::default())
        .unwrap();
    let preview = f
        .lib
        .preview_import_effects(&op, &CancellationToken::default())
        .unwrap();
    assert_eq!(
        preview.ready,
        1,
        "{:?}",
        f.lib
            .import_effects_page(&preview.preview_id, None, 100)
            .unwrap()
    );
    f.lib
        .apply_import_effects(&preview.preview_id, &CancellationToken::default())
        .unwrap();
    let observed =
        cuerithm_autotag::read_metadata_snapshot(&path, &runtime().probe(&path).unwrap()).unwrap();
    assert_eq!(
        observed
            .text
            .get(&cuerithm_autotag::MetadataTextField::CatalogNumber),
        Some(&vec!["AUTHORED001".to_owned()])
    );
}
#[test]
fn reference_repair_failure_blocks_rename() {
    let p = ImportPolicy {
        relocation: RelocationPolicy::RenameInPlace {
            rule: "{title}".into(),
        },
        ..Default::default()
    };
    let (f, op, _, path) = setup(p);
    let bytes = std::fs::read(&path).unwrap();
    let s = ImportProcessingService::start(f.lib.clone(), runtime()).unwrap();
    s.run_pending(&CancellationToken::default()).unwrap();
    let p = f
        .lib
        .preview_import_effects(&op, &CancellationToken::default())
        .unwrap();
    assert_eq!(p.ready, 0);
    assert_eq!(p.blocked, 1);
    assert!(
        f.lib
            .import_effects_page(&p.preview_id, None, 100)
            .unwrap()
            .records[0]
            .reason
            .as_deref()
            .unwrap()
            .contains("reference")
    );
    f.lib
        .apply_import_effects(&p.preview_id, &CancellationToken::default())
        .unwrap();
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}
#[cfg(feature = "autotag")]
#[test]
fn enrichment_is_library_only_without_tag_authorization() {
    use cuerithm_autotag::*;
    struct Catalog;
    impl ProviderBackend for Catalog {
        fn id(&self) -> ProviderId {
            ProviderId::Beatport
        }
        fn candidates(
            &self,
            input: &TagInput,
            _: &AutotagConfig,
        ) -> anyhow::Result<Vec<CatalogMatch>> {
            assert_eq!(
                input.media.tags.get("title").map(String::as_str),
                Some("Accepted title")
            );
            Ok(vec![CatalogMatch::new(
                1.0,
                CatalogTrack {
                    platform: "beatport".into(),
                    title: "Catalog title".into(),
                    ..Default::default()
                },
            )])
        }
    }
    let p = ImportPolicy {
        metadata_enrichment: EnrichmentPolicy {
            enabled: true,
            providers: vec!["beatport".into()],
            fields: ["title".into()].into(),
            ..Default::default()
        },
        ..Default::default()
    };
    let (f, op, id, path) = setup(p);
    let before = std::fs::read(&path).unwrap();
    let s = ImportProcessingService::start_with_tagger(
        f.lib.clone(),
        runtime(),
        std::sync::Arc::new(Tagger::with_backends(vec![std::sync::Arc::new(Catalog)]).unwrap()),
    )
    .unwrap();
    assert_eq!(
        s.run_pending(&CancellationToken::default())
            .unwrap()
            .processed,
        1
    );
    assert_eq!(
        f.lib.imported_item(&id).unwrap().active_fields["metadata.title"]["value"],
        "Accepted title"
    );
    let preview = f
        .lib
        .preview_import_effects(&op, &CancellationToken::default())
        .unwrap();
    assert_eq!(preview.ready, 1);
    f.lib
        .apply_import_effects(&preview.preview_id, &CancellationToken::default())
        .unwrap();
    assert_eq!(
        f.lib.imported_item(&id).unwrap().active_fields["metadata.title"]["value"],
        "Catalog title"
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(f.lib.jobs().unwrap().is_empty());
    assert_eq!(f.lib.tracks(TrackQuery::default()).unwrap().rows[0].id, id);
    assert_eq!(
        f.lib.imported_item(&id).unwrap().provenance["metadata.title"]["identity"]["metadata_evidence"]
            ["candidate"]["track"]["platform"],
        "beatport"
    );
}
#[test]
fn cleanup_is_frozen_library_only_and_undo_respects_later_edit() {
    let f = Fixture::new();
    let settings = PreparationSettings {
        cleanup: Some(cuerithm_cleanup::CleanupConfig {
            fixes: vec![cuerithm_cleanup::ConfiguredFix {
                id: "decode".into(),
                enabled: true,
                ignored_values: Default::default(),
                rule: cuerithm_cleanup::SmartFix::DecodeEntities,
            }],
            ..Default::default()
        }),
        ..Default::default()
    };
    f.lib.save_settings(settings).unwrap();
    let i = f.inspect(vec![track("A &amp; B")], Completeness::WholeLibrary);
    let mut r = f.request(&i);
    r.policy.cleanup = CleanupPolicy {
        enabled: true,
        rules: vec!["decode".into()],
    };
    let op = f.apply(&f.preview(r)).operation_id;
    let id = f.lib.tracks(TrackQuery::default()).unwrap().rows[0]
        .id
        .clone();
    f.lib.save_settings(PreparationSettings::default()).unwrap();
    let s = ImportProcessingService::start(f.lib.clone(), runtime()).unwrap();
    assert_eq!(
        s.run_pending(&CancellationToken::default())
            .unwrap()
            .processed,
        1
    );
    let p = f
        .lib
        .preview_import_effects(&op, &CancellationToken::default())
        .unwrap();
    f.lib
        .apply_import_effects(&p.preview_id, &CancellationToken::default())
        .unwrap();
    assert_eq!(
        f.lib.imported_item(&id).unwrap().active_fields["metadata.title"]["value"],
        json!(["A & B"])
    );
    f.lib.conn().unwrap().execute("UPDATE import_active_fields SET value_json=?1 WHERE item_id=?2 AND field='metadata.title'",rusqlite::params![json!({"presence":"value","value":"Later"}).to_string(),id]).unwrap();
    let undo = f
        .lib
        .undo_import_operation(&op, &CancellationToken::default())
        .unwrap();
    assert_eq!(undo.blocked, 1);
    let journal:String=f.lib.conn().unwrap().query_row("SELECT receipt_json FROM import_processing_receipts WHERE operation_id=?1 AND stage='publication.effects'",[&op],|r|r.get(0)).unwrap();
    let journal: serde_json::Value = serde_json::from_str(&journal).unwrap();
    let history = f
        .lib
        .import_operation_page(ImportOperationQuery::default())
        .unwrap();
    assert!(
        history.records[0]
            .issues
            .iter()
            .any(|i| i.stage == "publication.effects" && i.message.contains("Later"))
    );
    assert!(
        journal["last_error"]
            .as_str()
            .is_some_and(|s| s.contains("Later") && s.len() <= 1024)
    );
    assert_eq!(
        f.lib.imported_item(&id).unwrap().active_fields["metadata.title"]["value"],
        "Later"
    );
}
#[cfg(feature = "autotag")]
#[test]
fn rename_in_place_needs_no_tag_write() {
    let p = ImportPolicy {
        relocation: RelocationPolicy::RenameInPlace {
            rule: "{title}".into(),
        },
        ..Default::default()
    };
    let (f, op, id, path) = setup(p);
    let before = std::fs::read(&path).unwrap();
    // A detached local item has no original source reference to repair. Bound
    // imported items use the separate unqualified-repair blocker test.
    f.lib
        .conn()
        .unwrap()
        .execute("DELETE FROM import_bindings WHERE item_id=?1", [&id])
        .unwrap();
    let s = ImportProcessingService::start(f.lib.clone(), runtime()).unwrap();
    s.run_pending(&CancellationToken::default()).unwrap();
    let p = f
        .lib
        .preview_import_effects(&op, &CancellationToken::default())
        .unwrap();
    assert_eq!(
        p.ready,
        1,
        "{:?}",
        f.lib.import_effects_page(&p.preview_id, None, 100).unwrap()
    );
    f.lib
        .apply_import_effects(&p.preview_id, &CancellationToken::default())
        .unwrap();
    let destination = path.with_file_name("Accepted title.wav");
    assert!(!path.exists());
    assert_eq!(std::fs::read(&destination).unwrap(), before);
    assert_eq!(f.lib.track_path(&id).unwrap(), destination);
    assert!(f.lib.analysis(&id).unwrap().is_none());
    assert!(f.lib.jobs().unwrap().is_empty());
    let undo = f
        .lib
        .undo_import_operation(&op, &CancellationToken::default())
        .unwrap();
    assert_eq!(undo.blocked, 0, "{:?}", undo.errors);
    assert_eq!(std::fs::read(path).unwrap(), before);
}
#[cfg(feature = "autotag")]
#[test]
fn selected_tag_write_preserves_pcm_and_undo_bytes_in_all_ten_formats() {
    for (extension, codec) in [
        ("mp3", "libmp3lame"),
        ("aac", "aac"),
        ("m4a", "aac"),
        ("m4a", "alac"),
        ("flac", "flac"),
        ("wav", "pcm_s16le"),
        ("aiff", "pcm_s16be"),
        ("ogg", "libvorbis"),
        ("opus", "libopus"),
        ("wma", "wmav2"),
    ] {
        let policy = ImportPolicy {
            write_audio_tags: ["title".into()].into(),
            ..Default::default()
        };
        let (f, op, id, path) = setup_format(policy, extension, codec);
        let original = std::fs::read(&path).unwrap();
        let pcm = runtime()
            .decode(&path, &f.root.path().join("pcm-before"))
            .unwrap()
            .pcm_sha256()
            .unwrap();
        let service = ImportProcessingService::start(f.lib.clone(), runtime()).unwrap();
        service.run_pending(&CancellationToken::default()).unwrap();
        let preview = f
            .lib
            .preview_import_effects(&op, &CancellationToken::default())
            .unwrap();
        let effects = f
            .lib
            .import_effects_page(&preview.preview_id, None, 100)
            .unwrap();
        assert_eq!(preview.ready, 1, "{extension}/{codec}: {effects:?}");
        f.lib
            .apply_import_effects(&preview.preview_id, &CancellationToken::default())
            .unwrap();
        assert_eq!(
            runtime()
                .probe(&path)
                .unwrap()
                .tags
                .get("title")
                .map(String::as_str),
            Some("Accepted title"),
            "{extension}/{codec}"
        );
        assert_eq!(
            runtime()
                .decode(&path, &f.root.path().join("pcm-after"))
                .unwrap()
                .pcm_sha256()
                .unwrap(),
            pcm,
            "{extension}/{codec}"
        );
        let fields = f.lib.imported_item(&id).unwrap().active_fields;
        assert!(!fields.contains_key("metadata.codec"));
        assert!(!fields.contains_key("metadata.tags"));
        assert!(
            !fields.contains_key("metadata.comment"),
            "Unchecked file comment must remain only in the file layer"
        );
        let undo = f
            .lib
            .undo_import_operation(&op, &CancellationToken::default())
            .unwrap();
        assert_eq!(undo.blocked, 0, "{extension}/{codec}: {:?}", undo.errors);
        assert_restored(
            f.root.path(),
            &original,
            &path,
            &format!("{extension}/{codec}"),
        );
    }
}
#[test]
fn prepared_legacy_analysis_does_not_qualify_unresolved_imported_performance_data() {
    let f = Fixture::new();
    f.lib.conn().unwrap().execute("INSERT INTO tracks(id,path,title,source_hash,state,analysis_json,created,updated) VALUES('existing',?1,'Local','hash','ready','{}',1,1)",[crate::model::path_bytes(std::path::Path::new("/Synthetic/existing.flac"))]).unwrap();
    let mut t = track("Source");
    t.cues.push(CueObservation {
        external_id: "native".into(),
        family: CueFamily::Memory,
        native_start_seconds: Some(12.0),
        ..Default::default()
    });
    let i = f.inspect(vec![t], Completeness::WholeLibrary);
    let mut r = f.request(&i);
    r.target_items.insert("a".into(), "existing".into());
    f.apply(&f.preview(r));
    let readiness = f.lib.import_export_readiness("existing").unwrap();
    assert!(!readiness.ready);
    assert!(readiness.reasons.iter().any(|r| r.contains("clock")));
    assert!(
        f.lib
            .dj_sync_library(DjSyncScope::Full)
            .unwrap_err()
            .to_string()
            .contains("clock")
    );
}
#[cfg(feature = "autotag")]
#[test]
fn restart_recovers_a_linked_tag_journal_without_replaying_the_import() {
    use super::super::publication::{EffectJournal, EffectRow};
    let policy = ImportPolicy {
        write_audio_tags: ["title".into()].into(),
        ..Default::default()
    };
    let (f, op, id, path) = setup(policy);
    let original = std::fs::read(&path).unwrap();
    let processor = ImportProcessingService::start(f.lib.clone(), runtime()).unwrap();
    processor
        .run_pending(&CancellationToken::default())
        .unwrap();
    let preview = f
        .lib
        .preview_import_effects(&op, &CancellationToken::default())
        .unwrap();
    let raw:String=f.lib.conn().unwrap().query_row("SELECT proposal_json FROM import_preview_effects WHERE preview_id=?1 AND kind='processing'",[&preview.preview_id],|r|r.get(0)).unwrap();
    let row: EffectRow = serde_json::from_str(&raw).unwrap();
    let attempt = format!("{}:interruption", preview.preview_id);
    let journal = EffectJournal {
        local_applied: true,
        ..Default::default()
    };
    f.lib.conn().unwrap().execute("INSERT INTO import_processing_receipts VALUES(?1,?2,'publication.effects',?3,'applying',?4,?5,1)",rusqlite::params![op,id,attempt,raw,serde_json::to_string(&journal).unwrap()]).unwrap();
    f.lib
        .conn()
        .unwrap()
        .execute(
            "UPDATE import_previews SET state='effects_applying' WHERE id=?1",
            [&preview.preview_id],
        )
        .unwrap();
    let service = MetadataEditService::new(f.lib.clone(), runtime());
    let result = service.apply_import_linked(
        row.tag_preview.as_deref().unwrap(),
        &CancellationToken::default(),
        || anyhow::bail!("Injected interruption after durable tag journal"),
    );
    assert!(result.is_err());
    assert_eq!(std::fs::read(&path).unwrap(), original);
    drop(service);
    drop(processor);
    let f = f.reopen();
    let _processor = ImportProcessingService::start(f.lib.clone(), runtime()).unwrap();
    let recovery = f
        .lib
        .recover_import_operation(&op, &CancellationToken::default())
        .unwrap();
    assert_eq!(recovery.recovered, 1);
    assert_eq!(recovery.recovery_required, 0, "{:?}", recovery.errors);
    assert_eq!(
        runtime()
            .probe(&path)
            .unwrap()
            .tags
            .get("title")
            .map(String::as_str),
        Some("Accepted title")
    );
    assert_eq!(
        f.lib
            .recover_import_operation(&op, &CancellationToken::default())
            .unwrap()
            .recovered,
        0
    );
    assert!(f.lib.jobs().unwrap().is_empty());
    let undo = f
        .lib
        .undo_import_operation(&op, &CancellationToken::default())
        .unwrap();
    assert_eq!(undo.blocked, 0, "{:?}", undo.errors);
    // Recovered, the save recorded its change like any other, so its Undo writes the previous
    // tags back: the audio and tags return, the container's bytes need not.
    assert_restored(f.root.path(), &original, &path, "undo after recovery");
}
#[cfg(feature = "autotag")]
#[test]
fn stale_media_and_name_collision_never_publish() {
    for collision in [false, true] {
        let policy = if collision {
            ImportPolicy {
                relocation: RelocationPolicy::RenameInPlace {
                    rule: "{title}".into(),
                },
                ..Default::default()
            }
        } else {
            ImportPolicy {
                write_audio_tags: ["title".into()].into(),
                ..Default::default()
            }
        };
        let (f, op, id, path) = setup(policy);
        if collision {
            f.lib
                .conn()
                .unwrap()
                .execute("DELETE FROM import_bindings WHERE item_id=?1", [&id])
                .unwrap();
            std::fs::write(
                path.with_file_name("Accepted title.wav"),
                b"Unrelated existing file",
            )
            .unwrap();
        }
        let processor = ImportProcessingService::start(f.lib.clone(), runtime()).unwrap();
        processor
            .run_pending(&CancellationToken::default())
            .unwrap();
        let preview = f
            .lib
            .preview_import_effects(&op, &CancellationToken::default())
            .unwrap();
        if collision {
            assert_eq!(preview.blocked, 1);
            assert_eq!(preview.ready, 0);
        } else {
            std::fs::write(&path, b"Changed after review").unwrap();
            assert!(
                f.lib
                    .apply_import_effects(&preview.preview_id, &CancellationToken::default())
                    .is_err()
            );
            assert_eq!(std::fs::read(&path).unwrap(), b"Changed after review");
        }
        assert!(f.lib.jobs().unwrap().is_empty());
    }
}
#[cfg(feature = "autotag")]
#[test]
fn provider_cleanup_and_tag_write_review_the_final_values_together() {
    use cuerithm_autotag::*;
    struct Catalog;
    impl ProviderBackend for Catalog {
        fn id(&self) -> ProviderId {
            ProviderId::Beatport
        }
        fn candidates(&self, _: &TagInput, _: &AutotagConfig) -> anyhow::Result<Vec<CatalogMatch>> {
            Ok(vec![CatalogMatch::new(
                1.0,
                CatalogTrack {
                    platform: "beatport".into(),
                    title: "A &amp; B".into(),
                    ..Default::default()
                },
            )])
        }
    }
    let policy = ImportPolicy {
        metadata_enrichment: EnrichmentPolicy {
            enabled: true,
            providers: vec!["beatport".into()],
            fields: ["title".into()].into(),
            ..Default::default()
        },
        cleanup: CleanupPolicy {
            enabled: true,
            rules: vec!["decode".into()],
        },
        write_audio_tags: ["title".into()].into(),
        ..Default::default()
    };
    let (f, op, id, path) = setup_configured(policy, "wav", "pcm_s16le", |f| {
        let settings = PreparationSettings {
            cleanup: Some(cuerithm_cleanup::CleanupConfig {
                fixes: vec![cuerithm_cleanup::ConfiguredFix {
                    id: "decode".into(),
                    enabled: true,
                    ignored_values: Default::default(),
                    rule: cuerithm_cleanup::SmartFix::DecodeEntities,
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        f.lib.save_settings(settings).unwrap();
    });
    let s = ImportProcessingService::start_with_tagger(
        f.lib.clone(),
        runtime(),
        std::sync::Arc::new(Tagger::with_backends(vec![std::sync::Arc::new(Catalog)]).unwrap()),
    )
    .unwrap();
    assert_eq!(
        s.run_pending(&CancellationToken::default())
            .unwrap()
            .processed,
        3
    );
    let preview = f
        .lib
        .preview_import_effects(&op, &CancellationToken::default())
        .unwrap();
    assert_eq!(
        preview.ready,
        3,
        "{:?}",
        f.lib
            .import_effects_page(&preview.preview_id, None, 100)
            .unwrap()
    );
    f.lib
        .apply_import_effects(&preview.preview_id, &CancellationToken::default())
        .unwrap();
    assert_eq!(
        f.lib.imported_item(&id).unwrap().active_fields["metadata.title"]["value"],
        json!(["A & B"])
    );
    assert_eq!(
        runtime()
            .probe(&path)
            .unwrap()
            .tags
            .get("title")
            .map(String::as_str),
        Some("A & B")
    );
    let undo = f
        .lib
        .undo_import_operation(&op, &CancellationToken::default())
        .unwrap();
    assert_eq!(undo.blocked, 0, "{:?}", undo.errors);
    assert_eq!(
        f.lib.imported_item(&id).unwrap().active_fields["metadata.title"]["value"],
        "Accepted title"
    );
    assert_eq!(
        f.lib.imported_item(&id).unwrap().provenance["metadata.title"]["kind"],
        "source"
    );
}
#[cfg(feature = "autotag")]
#[test]
fn undo_restores_absent_native_key_and_source_provenance() {
    let p = ImportPolicy {
        analysis_fields: [(AnalysisField::Key, AnalysisAdoption::ReplaceSelected)].into(),
        write_audio_tags: ["title".into()].into(),
        ..Default::default()
    };
    let (f, op, id, _) = setup(p);
    let before = f.lib.imported_item(&id).unwrap();
    let s = ImportProcessingService::start(f.lib.clone(), runtime()).unwrap();
    s.run_pending(&CancellationToken::default()).unwrap();
    let preview = f
        .lib
        .preview_import_effects(&op, &CancellationToken::default())
        .unwrap();
    f.lib
        .apply_import_effects(&preview.preview_id, &CancellationToken::default())
        .unwrap();
    assert!(f.lib.track(&id).unwrap().key.is_some());
    let result = f
        .lib
        .undo_import_operation(&op, &CancellationToken::default())
        .unwrap();
    assert_eq!(result.blocked, 0, "{:?}", result.errors);
    let after = f.lib.imported_item(&id).unwrap();
    assert_eq!(after.active_fields, before.active_fields);
    assert_eq!(after.provenance, before.provenance);
}
#[cfg(feature = "autotag")]
#[test]
fn file_owner_promotion_preserves_unicode_search() {
    let p = ImportPolicy {
        write_audio_tags: ["title".into()].into(),
        ..Default::default()
    };
    let (f, op, id, _) = setup(p);
    f.lib.conn().unwrap().execute("UPDATE import_active_fields SET value_json=?1 WHERE item_id=?2 AND field='metadata.title'",rusqlite::params![json!({"presence":"value","value":"ÉCLAIR"}).to_string(),id]).unwrap();
    let s = ImportProcessingService::start(f.lib.clone(), runtime()).unwrap();
    s.run_pending(&CancellationToken::default()).unwrap();
    let preview = f
        .lib
        .preview_import_effects(&op, &CancellationToken::default())
        .unwrap();
    f.lib
        .apply_import_effects(&preview.preview_id, &CancellationToken::default())
        .unwrap();
    assert_eq!(
        f.lib
            .tracks(TrackQuery {
                search: "éclair".into(),
                ..Default::default()
            })
            .unwrap()
            .rows[0]
            .id,
        id
    );
}
#[cfg(feature = "autotag")]
#[test]
fn restart_recovers_an_interrupted_native_tag_undo() {
    use super::super::publication::EffectJournal;
    let p = ImportPolicy {
        write_audio_tags: ["title".into()].into(),
        ..Default::default()
    };
    let (f, op, id, path) = setup(p);
    let original = std::fs::read(&path).unwrap();
    let processor = ImportProcessingService::start(f.lib.clone(), runtime()).unwrap();
    processor
        .run_pending(&CancellationToken::default())
        .unwrap();
    let preview = f
        .lib
        .preview_import_effects(&op, &CancellationToken::default())
        .unwrap();
    f.lib
        .apply_import_effects(&preview.preview_id, &CancellationToken::default())
        .unwrap();
    let raw:String=f.lib.conn().unwrap().query_row("SELECT receipt_json FROM import_processing_receipts WHERE operation_id=?1 AND item_id=?2 AND stage='publication.effects'",rusqlite::params![op,id],|r|r.get(0)).unwrap();
    let journal: EffectJournal = serde_json::from_str(&raw).unwrap();
    let tag_batch = journal.tag_batch.unwrap();
    let service = MetadataEditService::new(f.lib.clone(), runtime());
    assert!(
        service
            .undo_linked_with(
                &tag_batch,
                &CancellationToken::default(),
                None,
                || anyhow::bail!("Injected interruption after durable Undo journal")
            )
            .is_err()
    );
    drop(service);
    drop(processor);
    let f = f.reopen();
    let _processor = ImportProcessingService::start(f.lib.clone(), runtime()).unwrap();
    let result = f
        .lib
        .undo_import_operation(&op, &CancellationToken::default())
        .unwrap();
    assert_eq!(result.blocked, 0, "{:?}", result.errors);
    assert_restored(f.root.path(), &original, &path, "interrupted undo");
    assert_eq!(
        f.lib
            .undo_import_operation(&op, &CancellationToken::default())
            .unwrap()
            .undone,
        0
    );
}
#[test]
fn changed_processing_implementation_blocks_reviewed_effects_before_adoption() {
    let p = ImportPolicy {
        analysis_fields: [(AnalysisField::Key, AnalysisAdoption::ReplaceSelected)].into(),
        ..Default::default()
    };
    let (f, op, id, _) = setup(p);
    let processor = ImportProcessingService::start(f.lib.clone(), runtime()).unwrap();
    processor
        .run_pending(&CancellationToken::default())
        .unwrap();
    let before = f.lib.imported_item(&id).unwrap().active_fields;
    let preview = f
        .lib
        .preview_import_effects(&op, &CancellationToken::default())
        .unwrap();
    f.lib.conn().unwrap().execute("UPDATE import_previews SET json=json_set(json,'$.processing.processing_sha256','obsolete-implementation') WHERE id=(SELECT preview_id FROM import_operations WHERE id=?1)",[&op]).unwrap();
    assert!(
        f.lib
            .apply_import_effects(&preview.preview_id, &CancellationToken::default())
            .is_err()
    );
    assert_eq!(f.lib.imported_item(&id).unwrap().active_fields, before);
}
#[cfg(feature = "autotag")]
#[test]
fn enrichment_names_the_final_track_with_or_without_a_tag_write() {
    use cuerithm_autotag::*;
    struct Catalog;
    impl ProviderBackend for Catalog {
        fn id(&self) -> ProviderId {
            ProviderId::Beatport
        }
        fn candidates(&self, _: &TagInput, _: &AutotagConfig) -> anyhow::Result<Vec<CatalogMatch>> {
            Ok(vec![CatalogMatch::new(
                1.0,
                CatalogTrack {
                    platform: "beatport".into(),
                    title: "Final catalog title".into(),
                    ..Default::default()
                },
            )])
        }
    }
    for tags in [false, true] {
        let p = ImportPolicy {
            metadata_enrichment: EnrichmentPolicy {
                enabled: true,
                providers: vec!["beatport".into()],
                fields: ["title".into()].into(),
                ..Default::default()
            },
            relocation: RelocationPolicy::RenameInPlace {
                rule: "{title}".into(),
            },
            write_audio_tags: if tags {
                ["title".into()].into()
            } else {
                Default::default()
            },
            ..Default::default()
        };
        let (f, op, id, path) = setup(p);
        let original = std::fs::read(&path).unwrap();
        f.lib
            .conn()
            .unwrap()
            .execute("DELETE FROM import_bindings WHERE item_id=?1", [&id])
            .unwrap();
        let processor = ImportProcessingService::start_with_tagger(
            f.lib.clone(),
            runtime(),
            std::sync::Arc::new(Tagger::with_backends(vec![std::sync::Arc::new(Catalog)]).unwrap()),
        )
        .unwrap();
        processor
            .run_pending(&CancellationToken::default())
            .unwrap();
        let preview = f
            .lib
            .preview_import_effects(&op, &CancellationToken::default())
            .unwrap();
        assert_eq!(
            preview.blocked,
            0,
            "{:?}",
            f.lib
                .import_effects_page(&preview.preview_id, None, 100)
                .unwrap()
        );
        f.lib
            .apply_import_effects(&preview.preview_id, &CancellationToken::default())
            .unwrap();
        let final_path = path.with_file_name("Final catalog title.wav");
        assert_eq!(f.lib.track_path(&id).unwrap(), final_path);
        if tags {
            assert_eq!(
                runtime()
                    .probe(&final_path)
                    .unwrap()
                    .tags
                    .get("title")
                    .map(String::as_str),
                Some("Final catalog title")
            );
        } else {
            assert_eq!(std::fs::read(final_path).unwrap(), original);
        }
        let undo = f
            .lib
            .undo_import_operation(&op, &CancellationToken::default())
            .unwrap();
        assert_eq!(undo.blocked, 0, "{:?}", undo.errors);
        assert_restored(f.root.path(), &original, &path, "undo");
    }
}
#[cfg(feature = "autotag")]
#[test]
fn source_reference_added_after_review_blocks_relocation() {
    let p = ImportPolicy {
        relocation: RelocationPolicy::RenameInPlace {
            rule: "{title}".into(),
        },
        ..Default::default()
    };
    let (f, op, id, path) = setup(p);
    f.lib
        .conn()
        .unwrap()
        .execute("DELETE FROM import_bindings WHERE item_id=?1", [&id])
        .unwrap();
    let processor = ImportProcessingService::start(f.lib.clone(), runtime()).unwrap();
    processor
        .run_pending(&CancellationToken::default())
        .unwrap();
    let preview = f
        .lib
        .preview_import_effects(&op, &CancellationToken::default())
        .unwrap();
    assert_eq!(preview.ready, 1);
    let mut t = track("Accepted title");
    t.external_id = "new-reference".into();
    t.locations.native_path = Some(path.clone());
    t.locations.platform = Some(std::env::consts::OS.into());
    let inspection = f.inspect(vec![t], Completeness::WholeLibrary);
    let mut request = f.request(&inspection);
    request
        .target_items
        .insert("new-reference".into(), id.clone());
    f.apply(&f.preview(request));
    assert_eq!(
        f.lib
            .conn()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM import_bindings WHERE item_id=?1",
                [&id],
                |r| r.get::<_, u64>(0)
            )
            .unwrap(),
        1
    );
    assert!(
        f.lib
            .apply_import_effects(&preview.preview_id, &CancellationToken::default())
            .is_err()
    );
    assert!(path.exists());
    assert!(!path.with_file_name("Accepted title.wav").exists());
}
#[test]
fn cleanup_genre_conversion_preserves_custom_tag_output() {
    let f = Fixture::new();
    let settings = PreparationSettings {
        cleanup: Some(cuerithm_cleanup::CleanupConfig {
            genres: cuerithm_cleanup::GenreRules {
                enabled: true,
                conversion: Some(cuerithm_cleanup::GenreConversion {
                    category: "Style".into(),
                    genres: ["House".into()].into(),
                    remove_genre: true,
                }),
                ..Default::default()
            },
            ..Default::default()
        }),
        ..Default::default()
    };
    f.lib.save_settings(settings).unwrap();
    let mut t = track("Original");
    t.metadata
        .insert("genres".into(), Presence::Value(json!(["House"])));
    let inspection = f.inspect(vec![t], Completeness::WholeLibrary);
    let mut request = f.request(&inspection);
    request.policy.cleanup = CleanupPolicy {
        enabled: true,
        rules: vec!["genres".into()],
    };
    let op = f.apply(&f.preview(request)).operation_id;
    let id = f.lib.tracks(TrackQuery::default()).unwrap().rows[0]
        .id
        .clone();
    let processor = ImportProcessingService::start(f.lib.clone(), runtime()).unwrap();
    processor
        .run_pending(&CancellationToken::default())
        .unwrap();
    let preview = f
        .lib
        .preview_import_effects(&op, &CancellationToken::default())
        .unwrap();
    f.lib
        .apply_import_effects(&preview.preview_id, &CancellationToken::default())
        .unwrap();
    let fields = f.lib.imported_item(&id).unwrap().active_fields;
    assert_eq!(
        fields["metadata.custom_tags"]["value"],
        json!([{"category":"Style","value":"House"}])
    );
    assert_eq!(fields["metadata.genres"]["value"], json!([]));
}
#[cfg(feature = "autotag")]
#[test]
fn ordinary_organization_preserves_original_import_references() {
    let p = ImportPolicy {
        write_audio_tags: ["title".into()].into(),
        ..Default::default()
    };
    let (f, op, id, path) = setup(p);
    let processor = ImportProcessingService::start(f.lib.clone(), runtime()).unwrap();
    processor
        .run_pending(&CancellationToken::default())
        .unwrap();
    let preview = f
        .lib
        .preview_import_effects(&op, &CancellationToken::default())
        .unwrap();
    f.lib
        .apply_import_effects(&preview.preview_id, &CancellationToken::default())
        .unwrap();
    let rename = f
        .lib
        .preview_organization(
            OrganizationPreviewRequest {
                scope: OrganizationScope::Tracks {
                    track_ids: vec![id.clone()],
                },
                settings: OrganizationSettings {
                    mode: OrganizationSettingMode::RenameInPlace,
                    pattern: "{title}".into(),
                    ..Default::default()
                },
            },
            &CancellationToken::default(),
        )
        .unwrap();
    assert_eq!(rename.totals.blocked, 1);
    assert!(!rename.can_apply);
    let destination = f.root.path().join("copy-destination");
    std::fs::create_dir(&destination).unwrap();
    let root = f.lib.register_organization_root(destination).unwrap();
    let copy = f
        .lib
        .preview_organization(
            OrganizationPreviewRequest {
                scope: OrganizationScope::Tracks {
                    track_ids: vec![id.clone()],
                },
                settings: OrganizationSettings {
                    mode: OrganizationSettingMode::Copy,
                    pattern: "{title}".into(),
                    destination_root_id: Some(root.id),
                    ..Default::default()
                },
            },
            &CancellationToken::default(),
        )
        .unwrap();
    assert!(
        copy.can_apply,
        "{:?}",
        f.lib.organization_preview_page(&copy.id, 0, 100).unwrap()
    );
    let batch = f
        .lib
        .apply_organization_preview(&copy.id, &copy.revision, &CancellationToken::default())
        .unwrap();
    assert_eq!(batch.totals.completed, 1);
    assert!(path.exists());
    let undo = f
        .lib
        .undo_organization_batch(&batch.id, &CancellationToken::default())
        .unwrap();
    assert_eq!(undo.totals.blocked, 0);
    assert_eq!(f.lib.track_path(&id).unwrap(), path);
}
#[test]
fn empty_import_links_preserve_existing_reference_catalog_identity() {
    use sha2::{Digest, Sha256};
    let (f, _, id, path) = setup(ImportPolicy::default());
    f.lib
        .conn()
        .unwrap()
        .execute("DELETE FROM import_bindings WHERE item_id=?1", [&id])
        .unwrap();
    f.lib.conn().unwrap().execute("INSERT INTO tracks(id,path,title,source_hash,state,created,updated) VALUES(?1,?2,'Legacy','hash','ready',1,1)",rusqlite::params![id,crate::model::path_bytes(&path)]).unwrap();
    let snapshot = f.lib.organization_reference_snapshot(&id).unwrap();
    let raw: String = f
        .lib
        .conn()
        .unwrap()
        .query_row(
            "SELECT payload_json FROM organization_reference_snapshots WHERE digest=?1",
            [snapshot.digest],
            |r| r.get(0),
        )
        .unwrap();
    let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        value["catalog_digest"],
        format!("{:x}", Sha256::digest(b"[[],[],[]]"))
    );
}
#[cfg(feature = "autotag")]
#[test]
fn ordinary_copy_undo_blocks_after_another_source_link_is_added() {
    let p = ImportPolicy {
        write_audio_tags: ["title".into()].into(),
        ..Default::default()
    };
    let (f, op, id, path) = setup(p);
    let processor = ImportProcessingService::start(f.lib.clone(), runtime()).unwrap();
    processor
        .run_pending(&CancellationToken::default())
        .unwrap();
    let preview = f
        .lib
        .preview_import_effects(&op, &CancellationToken::default())
        .unwrap();
    f.lib
        .apply_import_effects(&preview.preview_id, &CancellationToken::default())
        .unwrap();
    let destination = f.root.path().join("copies");
    std::fs::create_dir(&destination).unwrap();
    let root = f.lib.register_organization_root(destination).unwrap();
    let copy = f
        .lib
        .preview_organization(
            OrganizationPreviewRequest {
                scope: OrganizationScope::Tracks {
                    track_ids: vec![id.clone()],
                },
                settings: OrganizationSettings {
                    mode: OrganizationSettingMode::Copy,
                    pattern: "{title}".into(),
                    destination_root_id: Some(root.id),
                    ..Default::default()
                },
            },
            &CancellationToken::default(),
        )
        .unwrap();
    let batch = f
        .lib
        .apply_organization_preview(&copy.id, &copy.revision, &CancellationToken::default())
        .unwrap();
    let copied = f.lib.track_path(&id).unwrap();
    let mut t = track("Accepted title");
    t.external_id = "copy-reference".into();
    t.locations.native_path = Some(copied.clone());
    t.locations.platform = Some(std::env::consts::OS.into());
    let inspection = f.inspect(vec![t], Completeness::WholeLibrary);
    let mut request = f.request(&inspection);
    request
        .target_items
        .insert("copy-reference".into(), id.clone());
    request.policy.selected_fields.clear();
    f.apply(&f.preview(request));
    let undo = f
        .lib
        .undo_organization_batch(&batch.id, &CancellationToken::default())
        .unwrap();
    assert_eq!(undo.totals.blocked, 1);
    assert_eq!(f.lib.track_path(&id).unwrap(), copied);
    assert!(path.exists());
    assert!(copied.exists());
}
#[cfg(feature = "autotag")]
#[test]
fn import_copy_undo_blocks_after_another_source_link_is_added() {
    let (f, _, id, path) = setup(ImportPolicy::default());
    let destination = f.root.path().join("import-copies");
    std::fs::create_dir(&destination).unwrap();
    let root = f.lib.register_organization_root(destination).unwrap();
    let original = f.lib.imported_item(&id).unwrap().observations[0]
        .track
        .clone();
    let inspection = f.inspect(vec![original], Completeness::WholeLibrary);
    let mut request = f.request(&inspection);
    request.policy.relocation = RelocationPolicy::Copy {
        root_id: root.id,
        rule: "{title}".into(),
    };
    let operation = f.apply(&f.preview(request)).operation_id;
    let processor = ImportProcessingService::start(f.lib.clone(), runtime()).unwrap();
    processor
        .run_pending(&CancellationToken::default())
        .unwrap();
    let preview = f
        .lib
        .preview_import_effects(&operation, &CancellationToken::default())
        .unwrap();
    assert_eq!(
        preview.ready,
        1,
        "{:?}",
        f.lib
            .import_effects_page(&preview.preview_id, None, 100)
            .unwrap()
    );
    f.lib
        .apply_import_effects(&preview.preview_id, &CancellationToken::default())
        .unwrap();
    let copied = f.lib.track_path(&id).unwrap();
    assert_ne!(copied, path);
    assert!(path.exists() && copied.exists());
    let mut track = track("Copied binding");
    track.external_id = "new-copy-source-link".into();
    track.locations.original = Some(copied.to_string_lossy().into_owned());
    track.locations.native_path = Some(copied.clone());
    track.locations.platform = Some(std::env::consts::OS.into());
    let inspection = f.inspect(vec![track], Completeness::WholeLibrary);
    let mut request = f.request(&inspection);
    request.policy.selected_fields.clear();
    request
        .target_items
        .insert("new-copy-source-link".into(), id.clone());
    f.apply(&f.preview(request));
    let undo = f
        .lib
        .undo_import_operation(&operation, &CancellationToken::default())
        .unwrap();
    assert_eq!(undo.blocked, 1);
    assert_eq!(f.lib.track_path(&id).unwrap(), copied);
    assert!(path.exists() && copied.exists());
}
#[cfg(feature = "autotag")]
#[test]
fn no_catalog_match_is_a_blocked_choice_without_erasing_imported_values() {
    use cuerithm_autotag::*;
    struct Empty;
    impl ProviderBackend for Empty {
        fn id(&self) -> ProviderId {
            ProviderId::Beatport
        }
        fn candidates(&self, _: &TagInput, _: &AutotagConfig) -> anyhow::Result<Vec<CatalogMatch>> {
            Ok(vec![])
        }
    }
    let p = ImportPolicy {
        metadata_enrichment: EnrichmentPolicy {
            enabled: true,
            providers: vec!["beatport".into()],
            fields: ["title".into()].into(),
            ..Default::default()
        },
        ..Default::default()
    };
    let (f, _, id, path) = setup(p);
    let before = f.lib.imported_item(&id).unwrap().active_fields;
    let bytes = std::fs::read(&path).unwrap();
    let processor = ImportProcessingService::start_with_tagger(
        f.lib.clone(),
        runtime(),
        std::sync::Arc::new(Tagger::with_backends(vec![std::sync::Arc::new(Empty)]).unwrap()),
    )
    .unwrap();
    let result = processor
        .run_pending(&CancellationToken::default())
        .unwrap();
    assert_eq!(result.blocked, 1);
    assert_eq!(result.failed, 0);
    assert_eq!(f.lib.imported_item(&id).unwrap().active_fields, before);
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}
