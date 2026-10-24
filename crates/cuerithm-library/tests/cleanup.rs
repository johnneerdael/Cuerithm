#![cfg(feature = "autotag")]
use cuerithm_library::{CleanupScope, CleanupService, Library};
use cuerithm_media::file_sha256;
#[path = "support/cleanup_fixture.rs"]
mod fixture;
use fixture::{Fixture, config};
/// The decoded audio of a file, whatever its tags.
fn pcm_of(f: &Fixture, path: &std::path::Path) -> String {
    f.rt.decode(path, &f.lib.data_dir().join("pcm"))
        .unwrap()
        .pcm_sha256()
        .unwrap()
}
#[test]
fn preview_is_read_only_and_selected_apply_and_undo_preserve_analysis_and_cues() {
    let f = Fixture::new(2);
    let service = f.service();
    let before: Vec<_> = f.paths.iter().map(|p| std::fs::read(p).unwrap()).collect();
    let before_tags =
        cuerithm_autotag::read_metadata(&f.paths[0], &f.rt.probe(&f.paths[0]).unwrap()).unwrap();
    let before_pcm = pcm_of(&f, &f.paths[0]);
    let analyses: Vec<_> = f
        .ids
        .iter()
        .map(|id| serde_json::to_value(f.lib.analysis(id).unwrap()).unwrap())
        .collect();
    let preview = service
        .preview_cleanup(CleanupScope::AllReady, config())
        .unwrap();
    assert_eq!(preview.proposals.len(), 2);
    for (i, path) in f.paths.iter().enumerate() {
        assert_eq!(std::fs::read(path).unwrap(), before[i]);
        assert_eq!(f.lib.cue_revision_count(&f.ids[i]).unwrap(), 1);
    }
    let proposal = preview
        .proposals
        .iter()
        .find(|p| p.track_id == f.ids[0])
        .unwrap();
    let applied = service
        .apply_cleanup(&preview.id, std::slice::from_ref(&proposal.id))
        .unwrap();
    assert_eq!(applied.applied, vec![f.ids[0].clone()]);
    assert!(applied.failed.is_empty());
    assert_eq!(f.lib.track(&f.ids[0]).unwrap().title, "signal track 0");
    assert_eq!(std::fs::read(&f.paths[1]).unwrap(), before[1]);
    assert!(
        serde_json::to_value(service.cleanup_history().unwrap()).unwrap()[0]["can_undo"]
            .as_bool()
            .unwrap_or(false)
    );
    for (i, id) in f.ids.iter().enumerate() {
        assert_eq!(f.lib.cue_revision_count(id).unwrap(), 1);
        assert_eq!(
            serde_json::to_value(f.lib.analysis(id).unwrap()).unwrap(),
            analyses[i]
        );
    }
    let undone = service.undo_cleanup(&applied.id).unwrap();
    assert_eq!(undone.applied, vec![f.ids[0].clone()]);
    // Undo writes the recorded tag values back: the audio and every tag are what they were.
    assert_eq!(
        cuerithm_autotag::read_metadata(&f.paths[0], &f.rt.probe(&f.paths[0]).unwrap()).unwrap(),
        before_tags
    );
    assert_eq!(pcm_of(&f, &f.paths[0]), before_pcm);
    assert_eq!(f.lib.track(&f.ids[0]).unwrap().title, "signal__track 0");
    assert_eq!(
        f.lib.track(&f.ids[0]).unwrap().source_sha256,
        file_sha256(&f.paths[0]).unwrap(),
        "the library knows the file it now has"
    );
    assert_eq!(f.lib.cue_revision_count(&f.ids[0]).unwrap(), 1);
    let history = service.cleanup_history().unwrap();
    assert_eq!(
        serde_json::to_value(history.iter().find(|h| h.id == applied.id).unwrap()).unwrap()["can_undo"],
        false
    );
}
#[test]
fn stale_archive_preparation_and_file_changes_are_reported_per_track() {
    let f = Fixture::new(4);
    let service = f.service();
    let preview = service
        .preview_cleanup(CleanupScope::AllReady, config())
        .unwrap();
    f.lib.set_archived(&f.ids[0], true).unwrap();
    let job = f
        .lib
        .jobs()
        .unwrap()
        .into_iter()
        .find(|j| j.track_id == f.ids[1])
        .unwrap();
    f.lib.retry(&job.id, None).unwrap();
    std::fs::write(&f.paths[2], b"newer user bytes").unwrap();
    let selection: Vec<_> = preview.proposals.iter().map(|p| p.id.clone()).collect();
    let result = service.apply_cleanup(&preview.id, &selection).unwrap();
    assert_eq!(result.applied, vec![f.ids[3].clone()]);
    assert_eq!(result.conflicted.len(), 3);
    assert_eq!(std::fs::read(&f.paths[2]).unwrap(), b"newer user bytes");
}
#[test]
fn metadata_races_and_sql_failure_preserve_files_and_later_values() {
    let f = Fixture::new(2);
    let service = f.service();
    let preview = service
        .preview_cleanup(CleanupScope::AllReady, config())
        .unwrap();
    let original = std::fs::read(&f.paths[1]).unwrap();
    let conn = rusqlite::Connection::open(&f.db).unwrap();
    conn.execute(
        "UPDATE tracks SET title='Later title' WHERE id=?1",
        [&f.ids[0]],
    )
    .unwrap();
    conn.execute_batch("CREATE TRIGGER reject_cleanup BEFORE UPDATE OF metadata_json ON tracks BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").unwrap();
    let selection: Vec<_> = preview.proposals.iter().map(|p| p.id.clone()).collect();
    let result = service.apply_cleanup(&preview.id, &selection).unwrap();
    assert_eq!(result.conflicted.len(), 1);
    assert_eq!(result.failed.len(), 1);
    assert!(result.applied.is_empty());
    assert_eq!(f.lib.track(&f.ids[0]).unwrap().title, "Later title");
    assert_eq!(std::fs::read(&f.paths[1]).unwrap(), original);
    assert_eq!(f.lib.cue_revision_count(&f.ids[1]).unwrap(), 1);
}
#[test]
fn persisted_history_supports_undo_after_restart_and_preserves_later_file_edits() {
    let Fixture {
        dir,
        db,
        lib,
        rt,
        ids,
        paths,
    } = Fixture::new(2);
    let service = CleanupService::new(lib.clone(), rt.clone());
    let preview = service
        .preview_cleanup(CleanupScope::AllReady, config())
        .unwrap();
    let selection: Vec<_> = preview.proposals.iter().map(|p| p.id.clone()).collect();
    let applied = service.apply_cleanup(&preview.id, &selection).unwrap();
    assert_eq!(applied.applied.len(), 2);
    let hash = file_sha256(&paths[0]).unwrap();
    drop(service);
    drop(lib);
    let lib = Library::open(&db).unwrap();
    let service = CleanupService::new(lib.clone(), rt);
    assert!(
        service
            .cleanup_history()
            .unwrap()
            .iter()
            .any(|h| h.id == applied.id)
    );
    std::fs::write(&paths[1], b"later edit").unwrap();
    let undone = service.undo_cleanup(&applied.id).unwrap();
    assert_eq!(undone.applied, vec![ids[0].clone()]);
    assert_eq!(undone.conflicted.len(), 1);
    assert_ne!(file_sha256(&paths[0]).unwrap(), hash);
    assert_eq!(std::fs::read(&paths[1]).unwrap(), b"later edit");
    assert_eq!(lib.cue_revision_count(&ids[0]).unwrap(), 1);
    drop(dir);
}

#[test]
fn cleanup_and_undo_cover_all_native_audio_formats_and_historical_cues() {
    use cuerithm_autotag::{
        AnalysisTags, AutotagConfig, FieldChange, MatchStatus, MetadataField, TagInput,
        TagMatchOutcome, TagWritePlan, read_metadata,
    };
    let archive = tempfile::tempdir().unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    tar::Archive::new(flate2::read::GzDecoder::new(
        std::fs::File::open(root.join("tools/fixtures/media.tar.gz")).unwrap(),
    ))
    .unpack(archive.path())
    .unwrap();
    let cases: serde_json::Value =
        serde_json::from_slice(&std::fs::read(archive.path().join("cases.json")).unwrap()).unwrap();
    for row in cases.as_array().unwrap() {
        let f = Fixture::new(1);
        let path = &f.paths[0];
        std::fs::copy(archive.path().join(row["file"].as_str().unwrap()), path).unwrap();
        let cache = f.lib.data_dir().join("seed");
        let audio = f.rt.decode(path, &cache).unwrap();
        let before = read_metadata(path, &audio.info).unwrap();
        let input = TagInput {
            path: path.clone(),
            source_sha256: audio.source_sha256.clone(),
            media: audio.info.clone(),
            duration_seconds: audio.clock.duration_seconds(),
        };
        let mut cfg = AutotagConfig::default();
        cfg.options.tags.clear();
        cfg.write_energy = false;
        let outcome = TagMatchOutcome {
            status: MatchStatus::NoMatch,
            matches: vec![],
            attempted: vec![],
            warnings: vec![],
        };
        let staged = TagWritePlan::build(input, &outcome, &cfg, &AnalysisTags::default())
            .unwrap()
            .with_metadata_edits(vec![FieldChange {
                field: MetadataField::Title,
                before: before
                    .fields
                    .get(&MetadataField::Title)
                    .cloned()
                    .unwrap_or_default(),
                after: vec!["signal__track 0".into()],
            }])
            .unwrap()
            .stage_prepared(&f.rt, &audio, &cache)
            .unwrap();
        std::fs::copy(&staged.path, path).unwrap();
        f.lib
            .enqueue(
                std::slice::from_ref(path),
                f.lib.settings().unwrap().revision,
            )
            .unwrap();
        let run = f.lib.claim_next().unwrap().unwrap();
        f.lib
            .apply_analysis_for_run(&run, &fixture::result(), &f.rt.probe(path).unwrap())
            .unwrap();
        let original_tags = read_metadata(path, &f.rt.probe(path).unwrap()).unwrap();
        let original_pcm = pcm_of(&f, path);
        let cues = f.lib.cue_revision_count(&f.ids[0]).unwrap();
        let analysis = serde_json::to_value(f.lib.analysis(&f.ids[0]).unwrap()).unwrap();
        assert_eq!(cues, 2);
        let service = f.service();
        let preview = service
            .preview_cleanup(CleanupScope::AllReady, config())
            .unwrap();
        assert_eq!(preview.proposals.len(), 1, "{}", row["file"]);
        let applied = service
            .apply_cleanup(&preview.id, &[preview.proposals[0].id.clone()])
            .unwrap();
        assert_eq!(
            applied.applied.len(),
            1,
            "{}: {:?}",
            row["file"],
            applied.failed
        );
        assert_eq!(f.lib.track(&f.ids[0]).unwrap().title, "signal track 0");
        assert_eq!(f.lib.cue_revision_count(&f.ids[0]).unwrap(), cues);
        assert_eq!(
            serde_json::to_value(f.lib.analysis(&f.ids[0]).unwrap()).unwrap(),
            analysis
        );
        let undone = service.undo_cleanup(&applied.id).unwrap();
        assert_eq!(
            undone.applied.len(),
            1,
            "{}: {:?}",
            row["file"],
            undone.failed
        );
        // Every tag and every decoded sample are what they were, in every format.
        assert_eq!(
            read_metadata(path, &f.rt.probe(path).unwrap()).unwrap(),
            original_tags,
            "{}",
            row["file"]
        );
        assert_eq!(pcm_of(&f, path), original_pcm, "{}", row["file"]);
        assert_eq!(f.lib.cue_revision_count(&f.ids[0]).unwrap(), cues);
    }
}
