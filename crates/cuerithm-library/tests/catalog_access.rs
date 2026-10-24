#![cfg(feature = "autotag")]
use cuerithm_autotag::ChallengeStore;
use cuerithm_library::Library;

#[test]
fn detected_challenge_is_persisted_by_the_real_tagger_and_prevents_direct_access_after_reopen() {
    use cuerithm_autotag::{AutotagConfig, CatalogEndpoints, TagInput, Tagger};
    use cuerithm_media::MediaInfo;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::Arc,
        time::Duration,
    };
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("library.sqlite");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut request = [0; 8192];
        assert!(socket.read(&mut request).unwrap() > 0);
        socket.write_all(b"HTTP/1.1 403 Forbidden\r\ncf-mitigated: challenge\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
    });
    let input = TagInput {
        path: "public-fixture.flac".into(),
        source_sha256: "0".repeat(64),
        duration_seconds: 300.,
        media: MediaInfo {
            codec: "flac".into(),
            sample_rate: 44100,
            channels: 2,
            bits_per_sample: Some(16),
            properties: Default::default(),
            tags: [
                ("artist".into(), "Fixture Artist".into()),
                ("title".into(), "Fixture Track".into()),
            ]
            .into(),
        },
    };
    let mut config = AutotagConfig::default();
    config.options.platforms = vec!["traxsource".into()];
    for attempt in 0..2 {
        let library = Library::open(&path).unwrap();
        let tagger = Tagger::with_endpoints_and_store(
            CatalogEndpoints::for_local_server(&url, Duration::from_secs(1)).unwrap(),
            Arc::new(library.clone()),
        )
        .unwrap();
        let error = tagger.match_track(&input, &config).unwrap_err();
        assert!(
            error.to_string().contains("solver"),
            "attempt {attempt}: {error:#}"
        );
        assert!(library.requires_solver("traxsource").unwrap());
    }
    server.join().unwrap();
}

#[test]
fn detected_source_requirement_survives_reopen_and_can_be_reset() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("library.sqlite");
    {
        let lib = Library::open(&path).unwrap();
        assert!(!lib.requires_solver("traxsource").unwrap());
        lib.mark_requires_solver("traxsource").unwrap();
        lib.mark_requires_solver("traxsource").unwrap();
    }
    let lib = Library::open(&path).unwrap();
    assert!(lib.requires_solver("traxsource").unwrap());
    assert!(!lib.requires_solver("beatport").unwrap());
    let catalog = lib.tagging_catalog().unwrap();
    assert!(
        catalog
            .access
            .iter()
            .any(|a| a.id == "traxsource" && a.requires_solver)
    );
    lib.reset_catalog_access().unwrap();
    assert!(!lib.requires_solver("traxsource").unwrap());
}

#[test]
fn main_schema_thirteen_upgrades_without_losing_detected_restrictions() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("library.sqlite");
    {
        let connection = rusqlite::Connection::open(&path).unwrap();
        for migration in [
            include_str!("../migrations/001_initial.sql"),
            include_str!("../migrations/002_tag_writes.sql"),
            include_str!("../migrations/003_preparation_cache.sql"),
            include_str!("../migrations/004_cleanup.sql"),
            include_str!("../migrations/005_cleanup_preparation.sql"),
            include_str!("../migrations/006_playlists.sql"),
            include_str!("../migrations/007_maintenance.sql"),
            include_str!("../migrations/008_maintenance_writes.sql"),
            include_str!("../migrations/009_unused_removal.sql"),
            include_str!("../migrations/010_unused_retention.sql"),
            include_str!("../migrations/011_native_import.sql"),
            include_str!("../migrations/012_automatic_playlists.sql"),
            include_str!("../migrations/013_catalog_access.sql"),
        ] {
            connection.execute_batch(migration).unwrap();
        }
        connection
            .execute(
                "INSERT INTO catalog_access(source,requires_solver) VALUES('traxsource',1)",
                [],
            )
            .unwrap();
    }
    let library = Library::open(&path).unwrap();
    assert!(library.requires_solver("traxsource").unwrap());
    let connection = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        connection
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        31
    );
    for table in [
        "organization_operations",
        "preparation_match_snapshots",
        "import_sources",
    ] {
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
    }
}
