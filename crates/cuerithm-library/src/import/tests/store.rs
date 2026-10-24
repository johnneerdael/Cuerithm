use crate::*;
use cuerithm_dj_sync::import::*;
use cuerithm_media::CancellationToken;
use serde_json::json;
use std::{collections::BTreeMap, fs, path::Path};

struct SyntheticReader {
    tracks: usize,
}
impl SourceAdapter for SyntheticReader {
    fn provider(&self) -> SourceProvider {
        SourceProvider::Mixxx
    }
    fn inspect(&self, _: &ImportSnapshot) -> anyhow::Result<SourceProfile> {
        Ok(SourceProfile {
            provider: self.provider(),
            profile_id: "synthetic-test-profile".into(),
            schema_fingerprint: "a".repeat(64),
            origin: BTreeMap::new(),
            fields: BTreeMap::new(),
            issues: Vec::new(),
        })
    }
    fn read(
        &self,
        snapshot: &ImportSnapshot,
        _: &ReadOptions,
        sink: &mut dyn ObservationSink,
        cancel: &CancellationToken,
    ) -> anyhow::Result<ObservationHeader> {
        let blob = sink.put_blob(b"invented-raw-field", Some("application/octet-stream"))?;
        for n in 0..self.tracks {
            cancel.check()?;
            sink.push(ObservationRecord::Track(Box::new(TrackObservation {
                external_id: n.to_string(),
                metadata: BTreeMap::from([(
                    "title".into(),
                    Presence::Value(json!(format!("Track {n}"))),
                )]),
                file_tags: Some(blob.clone()),
                ..Default::default()
            })))?;
        }
        Ok(ObservationHeader {
            contract_version: 1,
            source: SourceKey {
                provider: SourceProvider::Mixxx,
                library_instance_id: "synthetic-native-uuid".into(),
            },
            product_version: "source-schema40".into(),
            schema_fingerprint: "a".repeat(64),
            snapshot_hash: snapshot.snapshot_hash.clone(),
            adapter_version: "synthetic-test".into(),
            completeness: Completeness::WholeLibrary,
            scope: json!({"kind":"all"}),
            observed_at: "2026-10-23T12:00:00Z".into(),
            accounting: RecordAccounting {
                tracks_seen: Some(self.tracks as u64),
                ..Default::default()
            },
            extensions: BTreeMap::new(),
        })
    }
}
fn request(root: &Path) -> ImportInspectRequest {
    fs::write(root.join("library.bin"), b"invented snapshot").unwrap();
    ImportInspectRequest {
        provider: SourceProvider::Mixxx,
        selection: ImportSourceSelection {
            folder_inventory: false,
            root: root.to_path_buf(),
            artifacts: vec![ArtifactRequest::stable("library.bin")],
        },
        binding: SourceBindingChoice::Automatic,
        origin: None,
        path_mappings: Vec::new(),
        capture_file_tags: false,
        include_history: false,
        include_queue: false,
    }
}
#[test]
fn inspection_is_paged_and_private() {
    let dir = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let library = Library::open(&dir.path().join("library.db")).unwrap();
    let inspection = library
        .inspect_import_with_reader(
            request(source.path()),
            &SyntheticReader { tracks: 1001 },
            &CancellationToken::default(),
        )
        .unwrap();
    assert_eq!(inspection.counts.tracks, 1001);
    let page = library
        .import_page(
            &inspection.inspection_id,
            ImportPageQuery {
                limit: 1,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(page.records.len(), 1);
    assert_eq!(page.total, 1001);
    assert!(page.next_cursor.is_some());
    let second = library
        .import_page(
            &inspection.inspection_id,
            ImportPageQuery {
                limit: 1,
                cursor: page.next_cursor,
                ..Default::default()
            },
        )
        .unwrap();
    assert_ne!(
        serde_json::to_value(&page.records[0]).unwrap(),
        serde_json::to_value(&second.records[0]).unwrap()
    );
    assert!(
        library
            .import_page(
                &inspection.inspection_id,
                ImportPageQuery {
                    limit: 1001,
                    ..Default::default()
                }
            )
            .is_err()
    );
    let conn = library.conn().unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM jobs", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row("SELECT data FROM import_blobs", [], |r| r
            .get::<_, Vec<u8>>(0))
            .unwrap(),
        b"invented-raw-field"
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM import_applied_baselines", [], |r| r
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        0
    );
    drop(conn);
    drop(library);
    let reopened = Library::open(&dir.path().join("library.db")).unwrap();
    assert_eq!(
        reopened
            .import_page(&inspection.inspection_id, ImportPageQuery::default())
            .unwrap()
            .total,
        1001
    );
}
#[test]
fn source_replica_requires_explicit_binding() {
    let dir = tempfile::tempdir().unwrap();
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let library = Library::open(&dir.path().join("library.db")).unwrap();
    let reader = SyntheticReader { tracks: 1 };
    let first = library
        .inspect_import_with_reader(request(a.path()), &reader, &CancellationToken::default())
        .unwrap();
    assert!(
        library
            .inspect_import_with_reader(request(b.path()), &reader, &CancellationToken::default())
            .unwrap_err()
            .to_string()
            .contains("replica")
    );
    let mut copied = request(b.path());
    copied.binding = SourceBindingChoice::Replica {
        source_id: first.source_id.clone(),
    };
    let replica = library
        .inspect_import_with_reader(copied, &reader, &CancellationToken::default())
        .unwrap();
    assert_eq!(replica.source_id, first.source_id);
    let mut fork = request(a.path());
    fork.binding = SourceBindingChoice::NewLibrary;
    let fork = library
        .inspect_import_with_reader(fork, &reader, &CancellationToken::default())
        .unwrap();
    assert_ne!(fork.source_id, first.source_id);
}

fn source_reader_request(
    root: &Path,
    provider: SourceProvider,
    name: &str,
) -> ImportInspectRequest {
    ImportInspectRequest {
        provider,
        selection: ImportSourceSelection {
            folder_inventory: false,
            root: root.into(),
            artifacts: vec![ArtifactRequest::stable(name)],
        },
        binding: SourceBindingChoice::Automatic,
        origin: None,
        path_mappings: Vec::new(),
        capture_file_tags: false,
        include_history: false,
        include_queue: false,
    }
}
fn apple_identity_request(root: &Path, native_id: &str) -> ImportInspectRequest {
    fs::write(root.join("Music.xml"), format!("<plist><dict><key>Library Persistent ID</key><string>{native_id}</string><key>Tracks</key><dict><key>1</key><dict><key>Track ID</key><integer>1</integer><key>Name</key><string>Fixture track</string></dict></dict></dict></plist>")).unwrap();
    source_reader_request(root, SourceProvider::AppleMusicXml, "Music.xml")
}
fn source_count(library: &Library) -> u64 {
    library
        .conn()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM import_sources", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn native_source_identity_requires_a_choice_for_copies_and_retains_independent_sources() {
    let dir = tempfile::tempdir().unwrap();
    let original = tempfile::tempdir().unwrap();
    let copied = tempfile::tempdir().unwrap();
    let independent = tempfile::tempdir().unwrap();
    let library = Library::open(&dir.path().join("library.db")).unwrap();
    let cancel = CancellationToken::default();
    let first = library
        .inspect_import(
            apple_identity_request(original.path(), "APPLE-ORIGINAL"),
            &cancel,
        )
        .unwrap();
    assert_eq!(
        first.header.extensions["native_library_instance_id"],
        "APPLE-ORIGINAL"
    );
    let copied_request = apple_identity_request(copied.path(), "APPLE-ORIGINAL");
    let error = library
        .inspect_import(copied_request.clone(), &cancel)
        .unwrap_err();
    assert!(
        error.to_string().contains("replica or new library"),
        "{error:#}"
    );
    assert_eq!(source_count(&library), 1);
    let mut replica_request = copied_request;
    replica_request.binding = SourceBindingChoice::Replica {
        source_id: first.source_id.clone(),
    };
    let replica = library.inspect_import(replica_request, &cancel).unwrap();
    assert_eq!(replica.source_id, first.source_id);
    assert_eq!(
        replica.header.extensions["native_library_instance_id"],
        "APPLE-ORIGINAL"
    );
    let mut independent_request = apple_identity_request(independent.path(), "APPLE-ORIGINAL");
    independent_request.binding = SourceBindingChoice::NewLibrary;
    let separate = library
        .inspect_import(independent_request.clone(), &cancel)
        .unwrap();
    assert_ne!(separate.source_id, first.source_id);
    assert_eq!(
        separate.header.extensions["native_library_instance_id"],
        "APPLE-ORIGINAL"
    );
    independent_request.binding = SourceBindingChoice::Automatic;
    assert_eq!(
        library
            .inspect_import(independent_request, &cancel)
            .unwrap()
            .source_id,
        separate.source_id
    );
    assert_eq!(source_count(&library), 2);
}

#[test]
fn native_source_identity_rejects_unrelated_replicas_and_detects_same_root_replacement() {
    let dir = tempfile::tempdir().unwrap();
    let original = tempfile::tempdir().unwrap();
    let unrelated = tempfile::tempdir().unwrap();
    let library = Library::open(&dir.path().join("library.db")).unwrap();
    let cancel = CancellationToken::default();
    let first = library
        .inspect_import(
            apple_identity_request(original.path(), "APPLE-ORIGINAL"),
            &cancel,
        )
        .unwrap();
    let mut invalid_replica = apple_identity_request(unrelated.path(), "APPLE-UNRELATED");
    invalid_replica.binding = SourceBindingChoice::Replica {
        source_id: first.source_id.clone(),
    };
    let error = library
        .inspect_import(invalid_replica, &cancel)
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("Replica source identity differs"),
        "{error:#}"
    );
    assert_eq!(source_count(&library), 1);
    let replacement = library
        .inspect_import(
            apple_identity_request(original.path(), "APPLE-REPLACEMENT"),
            &cancel,
        )
        .unwrap();
    assert_ne!(replacement.source_id, first.source_id);
    assert_eq!(
        replacement.header.extensions["native_library_instance_id"],
        "APPLE-REPLACEMENT"
    );
    assert_eq!(source_count(&library), 2);
}

#[test]
fn native_source_identity_m3u_registered_fallback_survives_reopen_and_scope_changes() {
    let dir = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("first.m3u8"), "#EXTM3U\nmissing-a.flac\n").unwrap();
    fs::write(root.path().join("second.m3u8"), "#EXTM3U\nmissing-b.flac\n").unwrap();
    let db = dir.path().join("library.db");
    let library = Library::open(&db).unwrap();
    let cancel = CancellationToken::default();
    let first = library
        .inspect_import(
            source_reader_request(root.path(), SourceProvider::M3u, "first.m3u8"),
            &cancel,
        )
        .unwrap();
    let native = first.header.extensions["native_library_instance_id"].clone();
    assert!(!native.as_str().unwrap().is_empty());
    drop(library);
    let library = Library::open(&db).unwrap();
    let second = library
        .inspect_import(
            source_reader_request(root.path(), SourceProvider::M3u, "second.m3u8"),
            &cancel,
        )
        .unwrap();
    assert_eq!(second.source_id, first.source_id);
    assert_eq!(
        second.header.extensions["native_library_instance_id"],
        native
    );
    assert_eq!(source_count(&library), 1);
}

#[test]
fn same_path_is_not_global_identity() {
    let local = IdentityCandidate {
        item_id: "local-a".into(),
        source_link: None,
        canonical_path: Some("/Music/song.flac".into()),
        volume_id: None,
        content_sha256: Some("a".repeat(64)),
    };
    let incoming = IdentityEvidence {
        established_item_id: Some("linked-b".into()),
        canonical_path: local.canonical_path.clone(),
        volume_id: Some("volume-1".into()),
        content_sha256: local.content_sha256.clone(),
        allow_content_matching: false,
    };
    assert_eq!(
        resolve_import_identity(&incoming, std::slice::from_ref(&local)).unwrap(),
        IdentityResolution::Established("linked-b".into())
    );
    let mut incoming = incoming;
    incoming.established_item_id = None;
    assert_eq!(
        resolve_import_identity(&incoming, std::slice::from_ref(&local)).unwrap(),
        IdentityResolution::Unmatched
    );
    let mut verified = local;
    verified.volume_id = Some("volume-1".into());
    assert_eq!(
        resolve_import_identity(&incoming, &[verified]).unwrap(),
        IdentityResolution::ExactPath("local-a".into())
    );
}
#[test]
fn import_store_migrates_without_touching_rekordbox_state() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.db");
    let library = Library::open(&path).unwrap();
    {
        let conn = library.conn().unwrap();
        conn.execute("INSERT INTO native_targets VALUES ('old','rekordbox','db','device','{}','schema','observation','[]',1,1,1)",[]).unwrap();
        conn.execute("INSERT INTO native_records VALUES ('old','djmdContent','1','{\"Title\":\"local\"}','hash')",[]).unwrap();
        conn.execute("INSERT INTO tracks(id,path,title,source_hash,created,updated) VALUES('existing',?1,'Local','hash',1,1)", [b"/Synthetic/old.flac".as_slice()]).unwrap();
        conn.execute("INSERT INTO native_track_links VALUES('old','1','existing','{}',NULL,NULL,?1,'observed',17)", [r#"{"title":{"local":"Local","remote":"Remote","owner":"local","acknowledged":false}}"#]).unwrap();
        conn.execute("INSERT INTO native_sync_track_ownership(target_id,local_track_id,identity_json,cue_ids_json,cue_proofs_json) VALUES('old','existing','{}','[1]','{\"proof\":true}')",[]).unwrap();
        conn.execute("INSERT INTO native_reconciliation_plans VALUES('plan','old','1','{}','{\"title\":\"keep_local\"}',4,1,1)",[]).unwrap();
        conn.execute("INSERT INTO rekordbox_sync_operations VALUES('operation','old','/Synthetic/master.db','acknowledged','{\"old_journal\":true}',1,1)",[]).unwrap();
        for table in [
            "import_preview_effects",
            "import_processing_receipts",
            "import_operations",
            "import_previews",
            "import_policies",
            "import_memberships",
            "import_collections",
            "import_applied_baselines",
            "import_active_fields",
            "import_bindings",
            "import_items",
            "import_observations",
            "import_snapshots",
            "import_blobs",
            "import_sources",
        ] {
            conn.execute(&format!("DROP TABLE {table}"), []).unwrap();
        }
        conn.pragma_update(None, "user_version", 29).unwrap();
    }
    drop(library);
    let reopened = Library::open(&path).unwrap();
    let conn = reopened.conn().unwrap();
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        31
    );
    // The import migration touches no rekordbox row. The lean-library migration that follows it
    // keeps a record's key and hash and drops the content of a table native playlist sync does
    // not read back.
    assert_eq!(
        conn.query_row(
            "SELECT json,hash FROM native_records WHERE target_id='old'",
            [],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        )
        .unwrap(),
        (String::new(), "hash".to_owned())
    );
    assert_eq!(
        conn.query_row(
            "SELECT baseline_json FROM native_track_links WHERE target_id='old'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        r#"{"title":{"local":"Local","remote":"Remote","owner":"local","acknowledged":false}}"#
    );
    assert_eq!(
        conn.query_row(
            "SELECT cue_proofs_json FROM native_sync_track_ownership WHERE target_id='old'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        r#"{"proof":true}"#
    );
    assert_eq!(
        conn.query_row(
            "SELECT decisions_json FROM native_reconciliation_plans WHERE id='plan'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        r#"{"title":"keep_local"}"#
    );
    assert_eq!(
        conn.query_row(
            "SELECT journal_json FROM rekordbox_sync_operations WHERE id='operation'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        r#"{"old_journal":true}"#
    );
}

#[test]
fn source_artifact_selection_order_does_not_fork_library_identity() {
    let dir = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let library = Library::open(&dir.path().join("library.db")).unwrap();
    let mut first = request(source.path());
    fs::write(source.path().join("sidecar.bin"), b"sidecar").unwrap();
    first
        .selection
        .artifacts
        .push(ArtifactRequest::stable("sidecar.bin"));
    let original = library
        .inspect_import_with_reader(
            first.clone(),
            &SyntheticReader { tracks: 1 },
            &CancellationToken::default(),
        )
        .unwrap();
    first.selection.artifacts.reverse();
    let again = library
        .inspect_import_with_reader(
            first,
            &SyntheticReader { tracks: 1 },
            &CancellationToken::default(),
        )
        .unwrap();
    assert_eq!(again.source_id, original.source_id);
}
#[test]
fn imported_observations_and_blobs_remain_part_of_a_valid_library_backup() {
    let dir = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let backups = tempfile::tempdir().unwrap();
    let library = Library::open(&dir.path().join("library.db")).unwrap();
    library
        .inspect_import_with_reader(
            request(source.path()),
            &SyntheticReader { tracks: 1 },
            &CancellationToken::default(),
        )
        .unwrap();
    let backup = library
        .create_library_backup(&backups.path().join("bundle"))
        .unwrap();
    assert_eq!(backup.schema, 31);
    let database = rusqlite::Connection::open(backup.path.join("library.sqlite")).unwrap();
    assert_eq!(
        database
            .query_row("SELECT COUNT(*) FROM import_snapshots", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        database
            .query_row("SELECT data FROM import_blobs", [], |r| r
                .get::<_, Vec<u8>>(0))
            .unwrap(),
        b"invented-raw-field"
    );
}

struct BoundaryReader {
    invalid: Option<&'static str>,
    unrepresented: u64,
    issue: bool,
    cancel_after_read: bool,
}
impl SourceAdapter for BoundaryReader {
    fn provider(&self) -> SourceProvider {
        SourceProvider::Mixxx
    }
    fn inspect(&self, snapshot: &ImportSnapshot) -> anyhow::Result<SourceProfile> {
        SyntheticReader { tracks: 1 }.inspect(snapshot)
    }
    fn read(
        &self,
        snapshot: &ImportSnapshot,
        options: &ReadOptions,
        sink: &mut dyn ObservationSink,
        cancel: &CancellationToken,
    ) -> anyhow::Result<ObservationHeader> {
        let mut header = SyntheticReader { tracks: 0 }.read(snapshot, options, sink, cancel)?;
        let mut track = TrackObservation {
            external_id: "boundary".into(),
            ..Default::default()
        };
        if self.invalid == Some("bpm") {
            track.bpm = Some(ScalarBpm {
                presence: FieldPresence::Value,
                value: Some(f64::NAN),
                ..Default::default()
            });
        }
        if self.invalid == Some("warning") {
            track.issues.push(ImportIssue {
                code: "field_unavailable".into(),
                record_id: Some("boundary".into()),
                field: Some("bpm".into()),
                ..Default::default()
            });
        }
        if self.invalid == Some("cue") {
            track.cues.push(CueObservation {
                external_id: "nan-cue".into(),
                family: CueFamily::Marker,
                native_start_seconds: Some(f64::NAN),
                ..Default::default()
            });
        }
        sink.push(ObservationRecord::Track(Box::new(track)))?;
        header.accounting.tracks_seen = Some(1);
        header.accounting.unrepresented_records = self.unrepresented;
        if self.unrepresented > 0 {
            header.completeness = Completeness::PartialWithErrors;
        }
        if self.issue {
            sink.push(ObservationRecord::Issue(ImportIssue {
                code: "invalid_source_row".into(),
                record_id: Some("row:bad".into()),
                ..Default::default()
            }))?;
        }
        if self.cancel_after_read {
            cancel.cancel();
            cancel.check()?;
        }
        Ok(header)
    }
}
#[test]
fn partial_inspection_requires_a_typed_issue_for_unrepresented_source_rows() {
    let dir = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let library = Library::open(&dir.path().join("library.db")).unwrap();
    let incomplete = BoundaryReader {
        invalid: None,
        unrepresented: 1,
        issue: false,
        cancel_after_read: false,
    };
    assert!(
        library
            .inspect_import_with_reader(
                request(source.path()),
                &incomplete,
                &CancellationToken::default()
            )
            .is_err(),
        "unaccounted rows must not be accepted as a partial success"
    );
    let accounted = BoundaryReader {
        issue: true,
        ..incomplete
    };
    let inspection = library
        .inspect_import_with_reader(
            request(source.path()),
            &accounted,
            &CancellationToken::default(),
        )
        .unwrap();
    assert_eq!(inspection.counts.tracks, 1);
    assert_eq!(inspection.counts.issues, 1);
    assert_eq!(
        inspection.header.completeness,
        Completeness::PartialWithErrors
    );
}
#[test]
fn nonfinite_source_numbers_cannot_silently_turn_into_absent_values_in_the_spool() {
    let dir = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let library = Library::open(&dir.path().join("library.db")).unwrap();
    for field in ["bpm", "cue"] {
        let reader = BoundaryReader {
            invalid: Some(field),
            unrepresented: 0,
            issue: false,
            cancel_after_read: false,
        };
        assert!(
            library
                .inspect_import_with_reader(
                    request(source.path()),
                    &reader,
                    &CancellationToken::default()
                )
                .is_err(),
            "nonfinite {field} must fail before JSON replaces it with null"
        );
    }
    assert_eq!(
        library
            .conn()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM import_snapshots", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}
#[test]
fn cancelled_inspection_persists_no_partial_sources_observations_or_blobs() {
    let dir = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let library = Library::open(&dir.path().join("library.db")).unwrap();
    let reader = BoundaryReader {
        invalid: None,
        unrepresented: 0,
        issue: false,
        cancel_after_read: true,
    };
    assert!(
        library
            .inspect_import_with_reader(
                request(source.path()),
                &reader,
                &CancellationToken::default()
            )
            .is_err()
    );
    let conn = library.conn().unwrap();
    for table in [
        "import_sources",
        "import_snapshots",
        "import_observations",
        "import_blobs",
    ] {
        assert_eq!(
            conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    assert_eq!(
        fs::read_dir(library.data_dir().join("import-work"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn field_warnings_are_visible_without_accounting_for_unrepresented_rows() {
    let dir = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let library = Library::open(&dir.path().join("library.db")).unwrap();
    let reader = BoundaryReader {
        invalid: Some("warning"),
        unrepresented: 0,
        issue: false,
        cancel_after_read: false,
    };
    let inspection = library
        .inspect_import_with_reader(
            request(source.path()),
            &reader,
            &CancellationToken::default(),
        )
        .unwrap();
    assert_eq!(inspection.counts.issues, 1);
    let missing = BoundaryReader {
        unrepresented: 1,
        ..reader
    };
    assert!(
        library
            .inspect_import_with_reader(
                request(source.path()),
                &missing,
                &CancellationToken::default()
            )
            .is_err(),
        "a valid row's warning cannot account for a different missing source record"
    );
}
