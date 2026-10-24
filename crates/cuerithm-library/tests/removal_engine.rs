//! Duplicate merges that delete files and Missing & broken removals take track records out of
//! the same library, so they must save the same records, purge the same derived data and keep
//! one bounded Undo history between them. A file that moved inside a watch folder is followed
//! and so is neither a duplicate nor missing.
#![cfg(feature = "autotag")]
#[path = "support/duplicates_fixture.rs"]
mod fixture;

use cuerithm_dj_sync::import::{ArtifactRequest, PathMapping, SourceProvider};
use cuerithm_library::{
    BrokenScanRequest, DUPLICATE_UNDO_LIMIT, DuplicateMergeService,
    DuplicateTolerance as Tolerance, ImportInspectRequest, ImportPreviewRequest,
    ImportProcessingService, ImportSourceSelection, MISSING_UNDO_WINDOW, MaintenanceFileState,
    MergeDisposition, REMOVAL_UNDO_LIMIT, SourceBindingChoice, TrackQuery, TrackView,
};
use cuerithm_media::CancellationToken;
use fixture::*;
use std::{sync::Arc, time::Duration};

fn token() -> CancellationToken {
    CancellationToken::default()
}
fn service(f: &Fixture, trash: &Arc<FakeTrash>) -> DuplicateMergeService {
    DuplicateMergeService::with_trash(f.lib.clone(), f.runtime.clone(), trash.clone())
}
fn trash(f: &Fixture) -> Arc<FakeTrash> {
    Arc::new(FakeTrash::new(f.dir.path().join("fake-trash")))
}
fn sql(f: &Fixture, query: &str) {
    rusqlite::Connection::open(&f.db)
        .unwrap()
        .execute_batch(query)
        .unwrap();
}
fn count(f: &Fixture, query: &str) -> i64 {
    rusqlite::Connection::open(&f.db)
        .unwrap()
        .query_row(query, [], |r| r.get(0))
        .unwrap()
}

fn attach_import_source(
    f: &Fixture,
    media: &std::path::Path,
    link: Option<&str>,
    name: &str,
) -> (String, String, String) {
    let file = format!("{name}.xml");
    let root = f.dir.path().canonicalize().unwrap();
    #[cfg(windows)]
    let source_prefix = url::Url::from_directory_path(&root)
        .unwrap()
        .to_file_path()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    #[cfg(not(windows))]
    let source_prefix = root.to_str().unwrap().into();
    let media = media
        .parent()
        .unwrap()
        .canonicalize()
        .unwrap()
        .join(media.file_name().unwrap());
    #[cfg(windows)]
    let media_uri = url::Url::from_file_path(&media).unwrap().to_string();
    #[cfg(not(windows))]
    let media_uri = format!("file://{}", media.to_string_lossy());
    std::fs::write(f.dir.path().join(&file), format!("<plist><dict><key>Library Persistent ID</key><string>{name}</string><key>Tracks</key><dict><key>1</key><dict><key>Track ID</key><integer>1</integer><key>Name</key><string>Signal</string><key>Artist</key><string>North</string><key>Grouping</key><string>Retained source audit</string><key>Total Time</key><integer>20000</integer><key>Location</key><string>{media_uri}</string></dict></dict><key>Playlists</key><array><dict><key>Playlist Persistent ID</key><string>source-list</string><key>Name</key><string>Source list</string><key>Playlist Items</key><array><dict><key>Track ID</key><integer>1</integer></dict></array></dict></array></dict></plist>")).unwrap();
    let inspection = f
        .lib
        .inspect_import(
            ImportInspectRequest {
                provider: SourceProvider::AppleMusicXml,
                selection: ImportSourceSelection {
                    root: f.dir.path().into(),
                    artifacts: vec![ArtifactRequest::stable(&file)],
                    folder_inventory: false,
                },
                binding: SourceBindingChoice::Automatic,
                origin: None,
                path_mappings: vec![PathMapping {
                    source_prefix,
                    local_root: root,
                    root_id: "fixture-root".into(),
                }],
                capture_file_tags: false,
                include_history: false,
                include_queue: false,
            },
            &token(),
        )
        .unwrap();
    let mut request = ImportPreviewRequest {
        inspection_id: inspection.inspection_id,
        ..Default::default()
    };
    if let Some(id) = link {
        request.target_items.insert("1".into(), id.into());
    }
    let preview = f.lib.preview_import(request, &token()).unwrap();
    f.lib.apply_import(&preview.preview_id, &token()).unwrap();
    let conn = rusqlite::Connection::open(&f.db).unwrap();
    let item: String = conn
        .query_row(
            "SELECT item_id FROM import_bindings WHERE source_id=?1 AND external_id='1'",
            [&inspection.source_id],
            |r| r.get(0),
        )
        .unwrap();
    let collection: String = conn
        .query_row(
            "SELECT id FROM import_collections WHERE source_id=?1 AND external_id='source-list'",
            [&inspection.source_id],
            |r| r.get(0),
        )
        .unwrap();
    (inspection.source_id, collection, item)
}

fn assert_removed_import_is_audit_only(f: &Fixture, id: &str, collection: &str) {
    assert!(
        f.lib.track(id).is_err(),
        "removed native track resurfaced as imported-only"
    );
    assert!(
        !f.lib
            .track_ids(TrackQuery::default())
            .unwrap()
            .contains(&id.to_owned())
    );
    assert!(
        !f.lib
            .track_ids(TrackQuery {
                view: TrackView::Archive,
                ..Default::default()
            })
            .unwrap()
            .contains(&id.to_owned())
    );
    assert!(f.lib.track_path(id).is_err());
    assert!(f.lib.track_file_missing(id).unwrap());
    assert!(f.lib.source_hash_optional(id).is_err());
    assert!(f.lib.source_locator(id).is_err());
    assert!(f.lib.accepted_musical_data(id).is_err());
    let detail = f.lib.imported_item(id).unwrap();
    assert!(!detail.playback_allowed);
    assert!(!detail.observations.is_empty());
    assert!(!detail.provenance.is_empty());
    let page = f
        .lib
        .imported_collection_page(collection, None, 100)
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.rows[0].item_id.as_deref(), Some(id));
    assert!(page.rows[0].track.is_none());
}

fn imported_audit(f: &Fixture, id: &str) -> Vec<(String, String, String, u64)> {
    let conn = rusqlite::Connection::open(&f.db).unwrap();
    conn.prepare("SELECT field,value_json,provenance_json,revision FROM import_active_fields WHERE item_id=?1 AND field NOT LIKE '\\_%' ESCAPE '\\' ORDER BY field").unwrap().query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap().collect::<Result<_,_>>().unwrap()
}

#[test]
fn imported_removal_missing_track_stays_hidden_through_source_refresh_and_returns_on_undo() {
    let mut f = Fixture::empty();
    let at = f.add_track("missing-linked.wav", 331);
    let id = f.ids[at].clone();
    let (source, collection, imported) =
        attach_import_source(&f, &f.paths[at], Some(&id), "SOURCE-MISSING");
    assert_eq!(imported, id);
    let before = imported_audit(&f, &id);
    f.playlist("user-set", vec![id.clone()]);
    std::fs::remove_file(&f.paths[at]).unwrap();
    let report = broken_report(&f);
    let preview = f
        .lib
        .preview_missing_removal(&report.id, std::slice::from_ref(&id), &token())
        .unwrap();
    let removal = f.lib.apply_missing_removal(&preview.id, &token()).unwrap();
    assert_eq!(f.lib.counts().unwrap().tracks, 0);
    assert_removed_import_is_audit_only(&f, &id, &collection);
    assert_eq!(imported_audit(&f, &id), before);
    let xml = f.dir.path().join("SOURCE-MISSING.xml");
    let current = std::fs::read_to_string(&xml).unwrap().replace("</dict><key>Playlists</key>", "<key>2</key><dict><key>Track ID</key><integer>2</integer><key>Name</key><string>Unrelated new track</string><key>Location</key><string>file:///unavailable/unrelated.wav</string></dict></dict><key>Playlists</key>");
    std::fs::write(xml, current).unwrap();
    let inspection = f.lib.inspect_import_refresh(&source, &token()).unwrap();
    let preview = f
        .lib
        .preview_import(
            ImportPreviewRequest {
                inspection_id: inspection.inspection_id,
                ..Default::default()
            },
            &token(),
        )
        .unwrap();
    assert!(preview.blocked + preview.conflicts > 0);
    assert_eq!(preview.tracks_created, 1);
    f.lib.apply_import(&preview.preview_id, &token()).unwrap();
    assert_removed_import_is_audit_only(&f, &id, &collection);
    f.lib.undo_missing_repair(&removal.id, &token()).unwrap();
    assert_eq!(f.lib.counts().unwrap().tracks, 2);
    assert_eq!(f.lib.track(&id).unwrap().id, id);
    assert_eq!(
        f.lib.playlist_members("user-set").unwrap(),
        vec![id.clone()]
    );
    assert_eq!(
        f.lib
            .imported_collection_page(&collection, None, 100)
            .unwrap()
            .rows[0]
            .track
            .as_ref()
            .unwrap()
            .id,
        id
    );
    let conn = rusqlite::Connection::open(&f.db).unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT state,local_track_id FROM import_items WHERE id=?1",
            [&id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        )
        .unwrap(),
        ("imported".into(), Some(id))
    );
}

#[test]
fn imported_removal_duplicate_delete_stays_hidden_and_undo_restores_source_link() {
    let f = Fixture::new();
    let id = f.ids[1].clone();
    let (_, collection, _) = attach_import_source(&f, &f.paths[1], Some(&id), "SOURCE-DUPLICATE");
    let expected = std::fs::read(&f.paths[1]).unwrap();
    let before = f.lib.imported_item(&id).unwrap();
    let audit = imported_audit(&f, &id);
    let review = f.review(Tolerance::None);
    let preview = f
        .lib
        .preview_duplicate_merge_choice_with(
            &review.id,
            review.revision,
            vec![review.groups[0].id.clone()],
            Some(MergeDisposition::Delete),
            &Capable,
            &token(),
        )
        .unwrap();
    let stand_in = trash(&f);
    let service = service(&f, &stand_in);
    let merged = service
        .apply_confirmed(&preview.id, true, &token())
        .unwrap();
    assert!(merged.removed_ids.contains(&id));
    assert_eq!(f.lib.counts().unwrap().tracks, 2);
    assert_removed_import_is_audit_only(&f, &id, &collection);
    assert_eq!(imported_audit(&f, &id), audit);
    service.undo(&merged.id, &token()).unwrap();
    assert!(
        !f.paths[1].exists(),
        "Record Undo leaves files in the Trash"
    );
    assert_eq!(
        std::fs::read(stand_in.folder.join(f.paths[1].file_name().unwrap())).unwrap(),
        expected
    );
    assert_eq!(f.lib.counts().unwrap().tracks, 3);
    assert_eq!(
        f.lib
            .imported_collection_page(&collection, None, 100)
            .unwrap()
            .rows[0]
            .track
            .as_ref()
            .unwrap()
            .id,
        id
    );
    assert_eq!(
        f.lib.imported_item(&id).unwrap().provenance,
        before.provenance
    );
}

#[test]
fn imported_removal_undo_refuses_a_new_visible_import_only_occupant() {
    let mut f = Fixture::empty();
    let at = f.add_track("missing-replaced.wav", 337);
    let id = f.ids[at].clone();
    attach_import_source(&f, &f.paths[at], Some(&id), "SOURCE-OLD");
    std::fs::remove_file(&f.paths[at]).unwrap();
    let report = broken_report(&f);
    let preview = f
        .lib
        .preview_missing_removal(&report.id, std::slice::from_ref(&id), &token())
        .unwrap();
    let removed = f.lib.apply_missing_removal(&preview.id, &token()).unwrap();
    let (_, _, new_id) = attach_import_source(&f, &f.paths[at], None, "SOURCE-NEW");
    assert_ne!(new_id, id);
    assert_eq!(f.lib.counts().unwrap().tracks, 1);
    let error = f
        .lib
        .undo_missing_repair(&removed.id, &token())
        .unwrap_err();
    assert!(error.to_string().contains("already used"), "{error:#}");
    assert!(f.lib.track(&id).is_err());
    assert_eq!(f.lib.track(&new_id).unwrap().id, new_id);
}

#[test]
fn imported_removal_old_processing_and_file_previews_cannot_promote_retained_audit() {
    let mut f = Fixture::empty();
    let at = f.add_track("retained-source.wav", 347);
    let id = f.ids[at].clone();
    let (source, collection, _) =
        attach_import_source(&f, &f.paths[at], Some(&id), "SOURCE-PENDING");
    let queue = |field: &str| {
        let inspection = f.lib.inspect_import_refresh(&source, &token()).unwrap();
        let mut request = ImportPreviewRequest {
            inspection_id: inspection.inspection_id,
            ..Default::default()
        };
        request.policy.write_audio_tags = [field.into()].into();
        let preview = f.lib.preview_import(request, &token()).unwrap();
        assert!(preview.processing_requested);
        f.lib
            .apply_import(&preview.preview_id, &token())
            .unwrap()
            .operation_id
    };
    let ready = queue("title");
    let processor = ImportProcessingService::start(f.lib.clone(), f.runtime.clone()).unwrap();
    assert_eq!(processor.run_pending(&token()).unwrap().processed, 1);
    let pending = queue("artist");
    let bytes = std::fs::read(&f.paths[at]).unwrap();
    let retained = f.paths[at].with_extension("held");
    std::fs::rename(&f.paths[at], &retained).unwrap();
    let report = broken_report(&f);
    let preview = f
        .lib
        .preview_missing_removal(&report.id, std::slice::from_ref(&id), &token())
        .unwrap();
    f.lib.apply_missing_removal(&preview.id, &token()).unwrap();
    // Restoring the same physical file alone does not restore the removed library record.
    std::fs::rename(retained, &f.paths[at]).unwrap();
    let result = processor.run_pending(&token()).unwrap();
    assert_eq!((result.processed, result.blocked), (0, 1));
    let stages = f.lib.import_processing_stages(&pending, None, 100).unwrap();
    assert!(
        stages.records[0]
            .error
            .as_deref()
            .unwrap()
            .contains("removed from the library")
    );
    let effects = f.lib.preview_import_effects(&ready, &token()).unwrap();
    assert_eq!((effects.ready, effects.blocked), (0, 1));
    let page = f
        .lib
        .import_effects_page(&effects.preview_id, None, 100)
        .unwrap();
    assert!(
        page.records[0]
            .reason
            .as_deref()
            .unwrap()
            .contains("removed from the library")
    );
    f.lib.discard_import_preview(&effects.preview_id).unwrap();
    assert_removed_import_is_audit_only(&f, &id, &collection);
    assert_eq!(std::fs::read(&f.paths[at]).unwrap(), bytes);
    assert_eq!(f.lib.counts().unwrap().tracks, 0);
}

#[test]
fn imported_removal_interrupted_file_recovery_preserves_detached_audit_for_undo() {
    let mut f = Fixture::empty();
    let at = f.add_track("interrupted-source.wav", 349);
    let id = f.ids[at].clone();
    let (source, _, _) = attach_import_source(&f, &f.paths[at], Some(&id), "SOURCE-RECOVERY");
    let inspection = f.lib.inspect_import_refresh(&source, &token()).unwrap();
    let mut request = ImportPreviewRequest {
        inspection_id: inspection.inspection_id,
        ..Default::default()
    };
    request.policy.write_audio_tags = ["title".into()].into();
    let preview = f.lib.preview_import(request, &token()).unwrap();
    let operation = f
        .lib
        .apply_import(&preview.preview_id, &token())
        .unwrap()
        .operation_id;
    let processor = ImportProcessingService::start(f.lib.clone(), f.runtime.clone()).unwrap();
    processor.run_pending(&token()).unwrap();
    let effects = f.lib.preview_import_effects(&operation, &token()).unwrap();
    assert_eq!(effects.ready, 1);
    f.lib
        .apply_import_effects(&effects.preview_id, &token())
        .unwrap();
    std::fs::remove_file(&f.paths[at]).unwrap();
    let report = broken_report(&f);
    let preview = f
        .lib
        .preview_missing_removal(&report.id, std::slice::from_ref(&id), &token())
        .unwrap();
    let removal = f.lib.apply_missing_removal(&preview.id, &token()).unwrap();
    let conn = rusqlite::Connection::open(&f.db).unwrap();
    assert_eq!(conn.execute("UPDATE import_processing_receipts SET state='applying' WHERE operation_id=?1 AND stage='publication.effects' AND json_extract(receipt_json,'$.local_applied')=1", [&operation]).unwrap(), 1);
    let held = || {
        conn.query_row(
            "SELECT state,local_track_id,revision,updated FROM import_items WHERE id=?1",
            [&id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, u64>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            },
        )
        .unwrap()
    };
    let before = held();
    let audit = imported_audit(&f, &id);
    let result = f
        .lib
        .recover_import_operation(&operation, &token())
        .unwrap();
    assert_eq!((result.recovered, result.recovery_required), (0, 1));
    assert!(result.errors[0].contains("removed from the library"));
    assert_eq!(held(), before);
    assert_eq!(imported_audit(&f, &id), audit);
    f.lib.undo_missing_repair(&removal.id, &token()).unwrap();
    assert_eq!(held().0, "imported");
    assert_eq!(held().1.as_deref(), Some(id.as_str()));
    assert_eq!(f.lib.track(&id).unwrap().id, id);
}

/// The cached cover of one file content, as the app would have written it.
fn plant_cover(f: &Fixture, hash: &str) -> std::path::PathBuf {
    let folder = f.lib.data_dir().join("cache/artwork");
    std::fs::create_dir_all(&folder).unwrap();
    let cover = folder.join(hash);
    std::fs::write(&cover, b"cover").unwrap();
    cover
}

/// Gives a track one of everything that can hang off it, so a removal has to deal with all of
/// it: DJ-program links, a custom tag, cached analysis, review artwork and a sync baseline.
fn give_everything(f: &Fixture, id: &str, hash: &str) {
    sql(
        f,
        &format!(
            "INSERT OR IGNORE INTO native_targets VALUES('desk','rekordbox','db','device','{{}}','fingerprint','observation','[]',1,1,1);
             INSERT OR IGNORE INTO custom_tag_categories VALUES('mood','Mood');
             INSERT OR IGNORE INTO custom_tags VALUES('warm','mood','Warm');
             INSERT INTO track_custom_tags VALUES('{id}','warm');
             INSERT INTO cue_revisions VALUES('cue-{id}','{id}',NULL,'{{}}',1);
             INSERT INTO sync_links VALUES('{id}','target','external-{id}','{{}}');
             INSERT INTO native_track_links VALUES('desk','content-1','{id}','{{}}',NULL,NULL,'{{}}','linked',1);
             INSERT INTO native_sync_track_ownership VALUES('desk','{id}','{{}}','[]','{{}}',NULL);
             INSERT OR IGNORE INTO preparation_analysis VALUES('{hash}','settings','engine','decoder','{{}}');
             INSERT INTO preparation_match_generations(track_id,job_id,generation,attempt,source_hash,settings_revision,payload_json,created)
               VALUES('{id}',NULL,'gen-{id}',1,'{hash}',(SELECT revision FROM selected_settings),'{{}}',0);
             INSERT INTO preparation_review_artwork(generation,candidate_id,url_digest,asset_json) VALUES('gen-{id}','c','d','{{}}');
             INSERT INTO preparation_review_artwork(generation,candidate_id,url_digest,asset_json) VALUES('gen-old-{id}','c','d','{{}}');
             INSERT OR IGNORE INTO playlists(id,kind,name,position,last_edit_id) VALUES('smart','smartlist','Smart',0,'edit');
             INSERT INTO smart_playlist_members VALUES('smart','{id}',0);
             INSERT INTO curation_native_stale VALUES('{id}');
             INSERT INTO preparation_reuse SELECT id,source_hash,'{hash}',(SELECT revision FROM selected_settings) FROM jobs WHERE track_id='{id}' LIMIT 1;
             INSERT INTO preparation_match_previews VALUES('preview-{id}','{id}','gen-{id}','1','{{}}','preview',0);
             INSERT INTO maintenance_fingerprints VALUES('{id}','engine-from-an-earlier-release','{hash}','{{}}',0);
             INSERT INTO maintenance_scans VALUES('unused-{id}','unused','{{\"references\":[\"{id}\"]}}',0);"
        ),
    );
    // The test is only meaningful if every kind of derived row exists before the removal.
    for table in DERIVED {
        assert!(
            count(f, &format!("SELECT COUNT(*) FROM {table}")) > 0,
            "the test must create a {table} row"
        );
    }
}

/// The per-track tables a removal deletes without saving them for Undo, because they are caches
/// or flags that are rebuilt on demand (`maintenance/purge.rs`).
const DERIVED: [&str; 5] = [
    "maintenance_fingerprints",
    "smart_playlist_members",
    "curation_native_stale",
    "preparation_reuse",
    "preparation_match_previews",
];

/// Every place in the library that still names a track, its file or its content, other than
/// the history lines that are meant to say what happened.
fn traces(f: &Fixture, needles: &[String]) -> Vec<String> {
    // The merge journal keeps the image Undo needs for the newest ten removals; the repair history
    // keeps one summary line per repair. Every other saved report or preview is searched too.
    const HISTORY: [&str; 1] = ["maintenance_writes"];
    let conn = rusqlite::Connection::open(&f.db).unwrap();
    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let mut found = vec![];
    for table in tables.iter().filter(|t| !HISTORY.contains(&t.as_str())) {
        let columns: Vec<String> = conn
            .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let scope = if table == "maintenance_scans" {
            " AND kind!='missing_repair'"
        } else {
            ""
        };
        for column in columns {
            for needle in needles {
                let hits: i64 = conn
                    .query_row(
                        &format!(
                            "SELECT COUNT(*) FROM \"{table}\" WHERE instr(CAST(\"{column}\" AS TEXT), ?1)>0{scope}"
                        ),
                        [needle],
                        |r| r.get(0),
                    )
                    .unwrap();
                if hits > 0 {
                    found.push(format!("{table}.{column} names {needle}"));
                }
            }
        }
    }
    found
}

fn broken_report(f: &Fixture) -> cuerithm_library::BrokenScanReport {
    f.lib
        .scan_broken_tracks(BrokenScanRequest::default(), &f.runtime, &token())
        .unwrap()
}

/// A pair of identical files with a title of its own; the copy leaves through "Delete from disk".
fn merge_removal(
    f: &mut Fixture,
    service: &DuplicateMergeService,
    n: usize,
) -> (String, std::path::PathBuf) {
    let seed = 301 + 2 * n as i32;
    let keep = f.add_track(&format!("pair-{n}-a.wav"), seed);
    // The copy is the same audio with bytes of its own, so its cover is its own too.
    let copy_path = f.dir.path().join(format!("pair-{n}-b.wav"));
    wav_with_extra_chunk(&copy_path, seed);
    let copy = f.import(copy_path);
    let cover = plant_cover(f, &hash(&f.paths[copy]));
    let title = format!(
        "Merge pair {n} {}",
        [
            "alpha", "bravo", "delta", "echo", "golf", "kilo", "lima", "oscar", "papa", "romeo",
            "sierra", "tango", "uniform", "victor"
        ][n]
    );
    sql(
        f,
        &format!(
            "UPDATE tracks SET title='{title}' WHERE id IN ('{}','{}')",
            f.ids[keep], f.ids[copy]
        ),
    );
    let review = f.review(Tolerance::None);
    assert_eq!(review.groups.len(), 1, "round {n}");
    let preview = f
        .lib
        .preview_duplicate_merge_choice_with(
            &review.id,
            review.revision,
            vec![review.groups[0].id.clone()],
            Some(MergeDisposition::Delete),
            &Capable,
            &token(),
        )
        .unwrap();
    let result = service
        .apply_confirmed(&preview.id, true, &token())
        .unwrap();
    assert_eq!(result.removed_ids.len(), 1, "round {n}");
    (result.id, cover)
}

/// A track whose file is gone is removed from the library through Missing & broken.
fn missing_removal(f: &mut Fixture, n: usize) -> (String, std::path::PathBuf) {
    let gone = f.add_track(&format!("gone-{n}.wav"), 601 + n as i32);
    let cover = plant_cover(f, &hash(&f.paths[gone]));
    std::fs::remove_file(&f.paths[gone]).unwrap();
    let report = broken_report(f);
    let preview = f
        .lib
        .preview_missing_removal(&report.id, std::slice::from_ref(&f.ids[gone]), &token())
        .unwrap();
    assert_eq!(preview.ready, 1, "round {n}");
    let id = f
        .lib
        .apply_missing_removal(&preview.id, &token())
        .unwrap()
        .id;
    (id, cover)
}

fn merge_is_final(f: &Fixture, id: &str) -> bool {
    f.lib
        .duplicate_merge_history()
        .unwrap()
        .into_iter()
        .find(|h| h.result.id == id)
        .unwrap()
        .undo_expired
}
fn missing_is_final(f: &Fixture, id: &str) -> bool {
    f.lib
        .missing_repair_history()
        .unwrap()
        .into_iter()
        .find(|h| h.entry.id == id)
        .unwrap()
        .undo_discarded
}

#[test]
fn every_kind_of_removal_shares_one_undo_limit_of_ten() {
    assert_eq!(REMOVAL_UNDO_LIMIT, 10);
    assert_eq!(DUPLICATE_UNDO_LIMIT, REMOVAL_UNDO_LIMIT);
    assert_eq!(MISSING_UNDO_WINDOW, REMOVAL_UNDO_LIMIT);

    let mut f = Fixture::empty();
    let stand_in = trash(&f);
    let service = service(&f, &stand_in);
    // Fourteen removals, alternating between the two kinds. Counted together, the newest ten
    // keep their records; the four oldest are final, whichever kind they are.
    struct Removal {
        merge: bool,
        id: String,
        cover: std::path::PathBuf,
    }
    let mut removals: Vec<Removal> = vec![];
    for n in 0..14 {
        std::thread::sleep(Duration::from_millis(3));
        let merge = n % 2 == 0;
        let (id, cover) = if merge {
            merge_removal(&mut f, &service, n)
        } else {
            missing_removal(&mut f, n)
        };
        removals.push(Removal { merge, id, cover });
    }
    let is_final = |f: &Fixture, removal: &Removal| {
        if removal.merge {
            merge_is_final(f, &removal.id)
        } else {
            missing_is_final(f, &removal.id)
        }
    };
    for (n, removal) in removals.iter().enumerate() {
        assert_eq!(
            is_final(&f, removal),
            n < 4,
            "removal {n} ({})",
            if removal.merge { "merge" } else { "missing" }
        );
        // The cover of the removed music is kept for Undo exactly as long as its saved records.
        assert_eq!(
            removal.cover.exists(),
            n >= 4,
            "the cover of removal {n} follows its saved records"
        );
    }
    // A final removal refuses Undo, in either place.
    for removal in &removals[..4] {
        let error = if removal.merge {
            service.undo(&removal.id, &token()).unwrap_err().to_string()
        } else {
            f.lib
                .undo_missing_repair(&removal.id, &token())
                .unwrap_err()
                .to_string()
        };
        assert!(
            error.contains("no longer available") || error.contains("discarded"),
            "{error}"
        );
    }
    // What a final removal keeps is its summary only: the records are not stored any more.
    let stored = count(
        &f,
        &format!(
            "SELECT COUNT(*) FROM maintenance_writes WHERE id='{}' AND json_extract(payload_json,'$.removed') IS NOT NULL",
            removals[0].id
        ),
    );
    assert_eq!(stored, 0);

    // Undoing one frees its place in the window: the next removal does not push another out.
    // The undone track is back, so its cover stays with it.
    let newest_merge = removals.iter().rev().find(|r| r.merge).unwrap();
    service.undo(&newest_merge.id, &token()).unwrap();
    assert!(newest_merge.cover.exists());
    std::thread::sleep(Duration::from_millis(3));
    let (extra, extra_cover) = merge_removal(&mut f, &service, 13);
    for removal in &removals[4..] {
        if removal.id != newest_merge.id {
            assert!(!is_final(&f, removal), "{}", removal.id);
            assert!(removal.cover.exists());
        }
    }
    assert!(!merge_is_final(&f, &extra) && extra_cover.exists());
    // One more now exceeds the limit again, and the oldest still-undoable one becomes final.
    std::thread::sleep(Duration::from_millis(3));
    let (last, _) = missing_removal(&mut f, 99);
    assert!(!missing_is_final(&f, &last));
    assert!(
        is_final(&f, &removals[4]),
        "the oldest removal left in the window is pushed out by the eleventh"
    );
    assert!(
        !removals[4].cover.exists(),
        "and its cover goes with its records"
    );
    assert!(!is_final(&f, &removals[5]) && removals[5].cover.exists());
}

#[test]
fn a_removal_leaves_nothing_derived_from_the_track_behind_whichever_way_it_happens() {
    // Through "Delete from disk": the copy has the same audio but not the same bytes.
    let f = Fixture::new();
    let (copy, hash_copy) = (f.ids[1].clone(), hash(&f.paths[1]));
    let copy_path = f.paths[1].to_string_lossy().into_owned();
    assert_ne!(hash_copy, hash(&f.paths[0]));
    give_everything(&f, &copy, &hash_copy);
    let art = f.lib.data_dir().join("cache/artwork");
    std::fs::create_dir_all(&art).unwrap();
    std::fs::write(art.join(&hash_copy), b"cover").unwrap();
    let review = f.review(Tolerance::None);
    let preview = f
        .lib
        .preview_duplicate_merge_choice_with(
            &review.id,
            review.revision,
            vec![review.groups[0].id.clone()],
            Some(MergeDisposition::Delete),
            &Capable,
            &token(),
        )
        .unwrap();
    let stand_in = trash(&f);
    let result = service(&f, &stand_in)
        .apply_confirmed(&preview.id, true, &token())
        .unwrap();
    assert_eq!(result.removed_ids, vec![copy.clone()]);
    let left = traces(&f, &[copy.clone(), hash_copy.clone(), copy_path.clone()]);
    assert!(left.is_empty(), "left behind by the merge: {left:#?}");
    assert_nothing_derived_remains(&f, &copy);
    assert!(
        art.join(&hash_copy).exists(),
        "its cover stays for Undo until the saved records are dropped"
    );

    // Through Missing & broken: the same call to the same engine, so the same result.
    let mut f = Fixture::new();
    let gone = f.add_track("gone.wav", 211);
    let (hash_gone, gone_path) = (
        hash(&f.paths[gone]),
        f.paths[gone].to_string_lossy().into_owned(),
    );
    let id = f.ids[gone].clone();
    give_everything(&f, &id, &hash_gone);
    let art = f.lib.data_dir().join("cache/artwork");
    std::fs::create_dir_all(&art).unwrap();
    std::fs::write(art.join(&hash_gone), b"cover").unwrap();
    std::fs::remove_file(&f.paths[gone]).unwrap();
    let audio_before = audio_files(&f);
    assert!(!audio_before.is_empty());
    let report = broken_report(&f);
    let preview = f
        .lib
        .preview_missing_removal(&report.id, std::slice::from_ref(&id), &token())
        .unwrap();
    f.lib.apply_missing_removal(&preview.id, &token()).unwrap();
    let left = traces(&f, &[id.clone(), hash_gone.clone(), gone_path]);
    assert!(left.is_empty(), "left behind by the removal: {left:#?}");
    assert_nothing_derived_remains(&f, &id);
    assert!(art.join(&hash_gone).exists(), "the cover stays for Undo");
    assert_eq!(
        audio_files(&f),
        audio_before,
        "removing records never touches an audio file"
    );
}

/// Nothing a removal does not need for Undo is left, in any of the derived tables, in the saved
/// unused-file scan (it names every path the library refers to) or in review artwork of a
/// generation no track holds any more.
fn assert_nothing_derived_remains(f: &Fixture, track: &str) {
    for table in DERIVED {
        let rows = if table == "preparation_reuse" {
            // Keyed by job: no row may outlive the job of the removed track.
            "SELECT COUNT(*) FROM preparation_reuse WHERE job_id NOT IN (SELECT id FROM jobs)"
                .to_owned()
        } else {
            format!("SELECT COUNT(*) FROM {table} WHERE track_id='{track}'")
        };
        assert_eq!(count(f, &rows), 0, "{table} still has rows of the track");
    }
    assert_eq!(
        count(
            f,
            "SELECT COUNT(*) FROM maintenance_scans WHERE kind='unused'"
        ),
        0,
        "the unused-file scan lists every path in the library"
    );
    assert_eq!(
        count(
            f,
            "SELECT COUNT(*) FROM preparation_review_artwork WHERE generation NOT IN (SELECT generation FROM preparation_match_generations)"
        ),
        0,
        "review artwork of a generation nobody holds"
    );
}

/// Every audio file next to the library, by name and content.
fn audio_files(f: &Fixture) -> Vec<(String, String)> {
    let mut files: Vec<(String, String)> = std::fs::read_dir(f.dir.path())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_file())
        .map(|entry| {
            (
                entry.file_name().to_string_lossy().into_owned(),
                hash(&entry.path()),
            )
        })
        .collect();
    files.sort();
    files
}

#[test]
fn both_kinds_of_removal_save_the_same_rows_and_unlink_the_rekordbox_track() {
    // The same rows are saved whichever way a track record leaves: compare what each stored.
    let f = Fixture::new();
    let copy = f.ids[1].clone();
    give_everything(&f, &copy, &hash(&f.paths[1]));
    let review = f.review(Tolerance::None);
    let preview = f
        .lib
        .preview_duplicate_merge_choice_with(
            &review.id,
            review.revision,
            vec![review.groups[0].id.clone()],
            Some(MergeDisposition::Delete),
            &Capable,
            &token(),
        )
        .unwrap();
    let stand_in = trash(&f);
    let merged = service(&f, &stand_in)
        .apply_confirmed(&preview.id, true, &token())
        .unwrap();
    let merge_tables = saved_tables(
        &f,
        &format!(
            "SELECT json_extract(payload_json,'$.removed.tables') FROM maintenance_writes WHERE id='{}'",
            merged.id
        ),
    );

    let mut g = Fixture::new();
    let gone = g.add_track("gone.wav", 211);
    let id = g.ids[gone].clone();
    give_everything(&g, &id, &hash(&g.paths[gone]));
    std::fs::remove_file(&g.paths[gone]).unwrap();
    let report = broken_report(&g);
    let preview = g
        .lib
        .preview_missing_removal(&report.id, std::slice::from_ref(&id), &token())
        .unwrap();
    let removed = g.lib.apply_missing_removal(&preview.id, &token()).unwrap();
    let missing_tables = saved_tables(
        &g,
        &format!(
            "SELECT json_extract(payload_json,'$.snapshot.image.tables') FROM maintenance_scans WHERE id='{}'",
            removed.id
        ),
    );
    assert_eq!(
        merge_tables, missing_tables,
        "one engine saves the same rows for the same kind of track"
    );
    // The rekordbox link stays in place, unlinked; every other row leaves and comes back; the
    // caches that are rebuilt on demand are not saved.
    for table in [
        "sync_links",
        "native_sync_track_ownership",
        "track_custom_tags",
        "cue_revisions",
        "jobs",
        "tracks",
    ] {
        assert!(
            merge_tables.contains(&(table.to_owned(), false)),
            "{table} is saved: {merge_tables:?}"
        );
    }
    assert!(merge_tables.contains(&("native_track_links".to_owned(), true)));
    for cache in ["maintenance_fingerprints", "smart_playlist_members"] {
        assert!(!merge_tables.iter().any(|(t, _)| t == cache), "{cache}");
    }
    for fx in [&f, &g] {
        assert_eq!(
            count(
                fx,
                "SELECT COUNT(*) FROM native_track_links WHERE status='not_in_local_library' AND local_track_id IS NULL"
            ),
            1
        );
    }
}

/// The tables a removal saved for Undo, and whether each row stays in place with its
/// reference cleared (detached) or leaves and comes back.
fn saved_tables(f: &Fixture, query: &str) -> Vec<(String, bool)> {
    let text: String = rusqlite::Connection::open(&f.db)
        .unwrap()
        .query_row(query, [], |r| r.get(0))
        .unwrap();
    let mut tables: Vec<(String, bool)> = serde_json::from_str::<Vec<serde_json::Value>>(&text)
        .unwrap()
        .iter()
        .map(|t| {
            (
                t["table"].as_str().unwrap().to_owned(),
                t["detached"].as_bool().unwrap(),
            )
        })
        .collect();
    tables.sort();
    tables
}

#[test]
fn a_file_moved_inside_a_watch_folder_is_followed_so_it_is_neither_missing_nor_a_duplicate() {
    let mut f = Fixture::empty();
    let album = f.dir.path().join("album");
    std::fs::create_dir(&album).unwrap();
    let old = album.join("song.wav");
    wav(&old, 101);
    let moved = f.import(old.clone());
    let other = f.add_track("other.wav", 43);
    // Distinct titles: only the audio could make these two a pair.
    sql(
        &f,
        &format!(
            "UPDATE tracks SET title='Amber Skyline' WHERE id='{}'; UPDATE tracks SET title='Quiet Harbour' WHERE id='{}'",
            f.ids[moved], f.ids[other]
        ),
    );
    // The file is moved up one folder, outside CueRithm.
    let new = f.dir.path().join("song.wav");
    std::fs::rename(&old, &new).unwrap();
    let before = f.scan(Tolerance::None);
    assert_eq!(
        before
            .skipped_unavailable
            .iter()
            .map(|t| (t.id.clone(), t.state))
            .collect::<Vec<_>>(),
        vec![(f.ids[moved].clone(), MaintenanceFileState::Missing)],
        "until it is followed, the track is missing and Duplicates leaves it out"
    );
    // The watch folder sees the file and follows it instead of admitting a second track.
    let outcome = f
        .lib
        .relink_moved_tracks(
            std::slice::from_ref(&new),
            &[f.dir.path().to_path_buf()],
            |_| true,
        )
        .unwrap();
    assert_eq!(outcome.relinked.len(), 1);
    assert_eq!(outcome.consumed, vec![new.clone()], "not admitted as new");
    // Afterwards the track is neither a duplicate nor missing, and nothing was added.
    let report = f.scan(Tolerance::None);
    assert!(report.groups.is_empty(), "{:?}", report.groups);
    assert!(report.skipped_unavailable.is_empty());
    let files = broken_report(&f);
    assert!(
        files
            .tracks
            .iter()
            .all(|t| t.state == MaintenanceFileState::Present),
        "{:?}",
        files.tracks
    );
    assert_eq!(count(&f, "SELECT COUNT(*) FROM tracks"), 2);
    assert_eq!(
        f.lib.track_path(&f.ids[moved]).unwrap(),
        new.canonicalize().unwrap()
    );
}

/// A future table that names a track must be handled by the removal on purpose. A table that has
/// a column pointing at a track is either reached by a required foreign key (the engine saves it
/// for Undo and deletes it with the track), a derived table `purge.rs` deletes, a link the
/// removal detaches, or the playlist order the caller rewrites. Anything else fails here until
/// someone decides what removal does with it.
#[test]
fn every_table_that_names_a_track_is_accounted_for() {
    use std::collections::{BTreeMap, BTreeSet};
    // A DJ-library link or an imported item loses its library track and stays, as a record of
    // what the DJ library holds.
    const DETACHED: [&str; 2] = ["native_track_links", "import_items"];
    const CALLER: [&str; 1] = ["playlist_members"];
    let f = Fixture::empty();
    let conn = rusqlite::Connection::open(&f.db).unwrap();
    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    // table -> tables it requires (a NOT NULL column that is a foreign key to them)
    let mut requires: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut names_a_track = BTreeSet::new();
    for table in &tables {
        let columns: Vec<(String, bool)> = conn
            .prepare(&format!(
                "SELECT name,\"notnull\" OR pk>0 FROM pragma_table_info('{table}')"
            ))
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? == 1)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        if columns
            .iter()
            .any(|(name, _)| name == "track_id" || name == "local_track_id")
        {
            names_a_track.insert(table.clone());
        }
        let keys: Vec<(String, String)> = conn
            .prepare(&format!(
                "SELECT \"table\",\"from\" FROM pragma_foreign_key_list('{table}')"
            ))
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        for (parent, from) in keys {
            if columns
                .iter()
                .any(|(name, required)| *name == from && *required)
            {
                requires.entry(table.clone()).or_default().insert(parent);
            }
        }
    }
    // Everything the engine reaches from the track by required keys.
    let mut reached = BTreeSet::from(["tracks".to_owned()]);
    loop {
        let more: Vec<String> = requires
            .iter()
            .filter(|(table, parents)| {
                !reached.contains(*table) && parents.iter().any(|p| reached.contains(p))
            })
            .map(|(table, _)| table.clone())
            .collect();
        if more.is_empty() {
            break;
        }
        reached.extend(more);
    }
    for table in DERIVED.iter().chain(&DETACHED).chain(&CALLER) {
        assert!(
            tables.iter().any(|t| t == table),
            "{table} is listed as handled but no longer exists"
        );
    }
    let unaccounted: Vec<&String> = names_a_track
        .iter()
        .filter(|t| {
            *t != "tracks"
                && !reached.contains(*t)
                && !DERIVED.contains(&t.as_str())
                && !DETACHED.contains(&t.as_str())
                && !CALLER.contains(&t.as_str())
        })
        .collect();
    assert!(
        unaccounted.is_empty(),
        "tables that name a track but that a removal would leave behind: {unaccounted:?}"
    );
    // The derived tables that hang off a track are reached by the keys too, so listing them as
    // derived is what keeps them out of the Undo image.
    for table in DERIVED {
        assert!(
            reached.contains(table) || names_a_track.contains(table),
            "{table} is not attached to a track"
        );
    }
}

fn saved(f: &Fixture, kind: &str) -> i64 {
    count(
        f,
        &format!("SELECT COUNT(*) FROM maintenance_scans WHERE kind='{kind}'"),
    )
}

/// Scanning again and again, without ever starting a review, must not pile up either.
#[test]
fn scanning_over_and_over_keeps_only_the_newest_scans() {
    let f = Fixture::new();
    // Each kind is bounded by its own save, not only by whatever is saved next.
    for _ in 0..6 {
        std::thread::sleep(Duration::from_millis(3));
        f.scan(Tolerance::None);
    }
    assert_eq!(saved(&f, "duplicates"), 3);
    for _ in 0..6 {
        std::thread::sleep(Duration::from_millis(3));
        broken_report(&f);
    }
    assert_eq!(saved(&f, "broken"), 3);
}

/// Every scan listed the whole library again, and a review and a merge plan each embedded
/// another copy. Only the newest few of each stay, and an older review that a plan still
/// depends on stays with it.
#[test]
fn saved_scans_and_reviews_are_bounded_as_they_are_saved() {
    let f = Fixture::new();
    let first = f.review(Tolerance::None);
    for _ in 0..6 {
        std::thread::sleep(Duration::from_millis(3));
        f.review(Tolerance::None);
    }
    assert_eq!(saved(&f, "duplicates"), 3);
    assert_eq!(saved(&f, "duplicate_review"), 3);
    assert!(
        f.lib.duplicate_review(&first.id, &token()).is_err(),
        "the oldest review is gone: scan again"
    );
    // A review that a saved plan was made from stays while the plan is one of the newest.
    let newest = f.review(Tolerance::None);
    f.lib
        .preview_duplicate_merge_choice_with(
            &newest.id,
            newest.revision,
            vec![newest.groups[0].id.clone()],
            Some(MergeDisposition::Archive),
            &Capable,
            &token(),
        )
        .unwrap();
    for _ in 0..4 {
        std::thread::sleep(Duration::from_millis(3));
        f.review(Tolerance::None);
    }
    assert!(f.lib.duplicate_review(&newest.id, &token()).is_ok());
    assert_eq!(saved(&f, "duplicate_merge_plan"), 1);
}

/// Libraries made by earlier versions carry every scan they ever saved. Opening one trims them
/// without touching the history lines or the reports a view still needs.
#[test]
fn a_library_with_many_old_saved_reports_is_trimmed_when_it_opens() {
    let f = Fixture::empty();
    let db = f.db.clone();
    let (dir, runtime) = (f.dir, f.runtime);
    drop(f.lib);
    let conn = rusqlite::Connection::open(&db).unwrap();
    let padding = "x".repeat(300_000);
    for kind in [
        "duplicates",
        "duplicate_review",
        "duplicate_merge_plan",
        "broken",
        "unused",
        "unused_removal_plan",
        "missing_repair",
    ] {
        for n in 0..9 {
            conn.execute(
                "INSERT INTO maintenance_scans VALUES(?1,?2,?3,?4)",
                rusqlite::params![format!("{kind}-{n}"), kind, padding, n],
            )
            .unwrap();
        }
    }
    drop(conn);
    let before = std::fs::metadata(&db).unwrap().len();
    let lib = cuerithm_library::Library::open(&db).unwrap();
    drop(lib);
    let conn = rusqlite::Connection::open(&db).unwrap();
    let rows = |kind: &str| -> Vec<String> {
        conn.prepare("SELECT id FROM maintenance_scans WHERE kind=?1 ORDER BY created")
            .unwrap()
            .query_map([kind], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    for kind in [
        "duplicates",
        "duplicate_review",
        "duplicate_merge_plan",
        "broken",
        "unused",
        "unused_removal_plan",
    ] {
        assert_eq!(
            rows(kind),
            [6, 7, 8].map(|n| format!("{kind}-{n}")),
            "{kind}"
        );
    }
    assert_eq!(rows("missing_repair").len(), 9, "history is never trimmed");
    // The space comes back too: the library file shrinks on the same start.
    let bytes = std::fs::metadata(&db).unwrap().len();
    assert!(
        bytes * 2 < before + 1_000_000,
        "{before} bytes became {bytes}"
    );
    drop((dir, runtime));
}

/// Archive-only merges used to stay undoable for ever, with the library state they changed. They
/// share the window of ten with the removals.
#[test]
fn an_archive_only_merge_leaves_the_undo_window_like_any_other() {
    let mut f = Fixture::empty();
    let stand_in = trash(&f);
    let service = service(&f, &stand_in);
    let mut merges = vec![];
    for round in 0..12 {
        std::thread::sleep(Duration::from_millis(3));
        let keep = f.add_track(&format!("keep-{round}.wav"), 501 + 2 * round);
        let copy_path = f.dir.path().join(format!("copy-{round}.wav"));
        wav_with_extra_chunk(&copy_path, 501 + 2 * round);
        let copy = f.import(copy_path);
        let title = format!(
            "Archived pair {round} {}",
            [
                "alpha", "bravo", "delta", "echo", "golf", "kilo", "lima", "oscar", "papa",
                "romeo", "sierra", "tango"
            ][round as usize]
        );
        sql(
            &f,
            &format!(
                "UPDATE tracks SET title='{title}' WHERE id IN ('{}','{}')",
                f.ids[keep], f.ids[copy]
            ),
        );
        let review = f.review(Tolerance::None);
        assert_eq!(review.groups.len(), 1, "round {round}");
        let preview = f
            .lib
            .preview_duplicate_merge_choice_with(
                &review.id,
                review.revision,
                vec![review.groups[0].id.clone()],
                Some(MergeDisposition::Archive),
                &Capable,
                &token(),
            )
            .unwrap();
        merges.push(
            service
                .apply_confirmed(&preview.id, false, &token())
                .unwrap()
                .id,
        );
    }
    for (n, id) in merges.iter().enumerate() {
        assert_eq!(merge_is_final(&f, id), n < 2, "merge {n}");
    }
    let error = service.undo(&merges[0], &token()).unwrap_err().to_string();
    assert!(error.contains("no longer available"), "{error}");
    assert!(service.undo(&merges[11], &token()).is_ok());
}
