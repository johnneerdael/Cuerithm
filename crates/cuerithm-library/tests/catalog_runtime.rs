use cuerithm_library::Library;

#[test]
fn catalog_runtime_schema_is_explicit_and_survives_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("library.sqlite");
    let library = Library::open(&path).unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
            .unwrap(),
        31,
        "New provider and typed-evidence settings must not be silently read by an older binary"
    );
    for table in [
        "catalog_rate_state",
        "catalog_request_starts",
        "catalog_response_cache",
    ] {
        assert!(
            conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                [table],
                |row| row.get::<_, bool>(0)
            )
            .unwrap()
        );
    }
    drop(conn);
    drop(library);
    Library::open(&path).unwrap();
}

#[cfg(feature = "autotag")]
#[test]
fn catalog_start_spacing_window_and_server_cooldown_survive_restart() {
    use cuerithm_autotag::{
        CatalogService, CatalogState, RateClaim, RateObservation, StartDecision,
    };
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("library.sqlite");
    let library = Library::open(&path).unwrap();
    let mut claim = RateClaim {
        service: CatalogService::MusicBrainz,
        now_ms: 1_000_000,
        min_interval_ms: 1_000,
        window_ms: 60_000,
        max_starts: 2,
    };
    assert_eq!(library.claim_start(&claim).unwrap(), StartDecision::Granted);
    assert_eq!(
        library.claim_start(&claim).unwrap(),
        StartDecision::WaitUntil(1_001_000)
    );
    claim.now_ms = 1_001_000;
    assert_eq!(library.claim_start(&claim).unwrap(), StartDecision::Granted);
    claim.now_ms = 1_002_000;
    assert_eq!(
        library.claim_start(&claim).unwrap(),
        StartDecision::WaitUntil(1_060_000)
    );
    library
        .observe(&RateObservation {
            service: claim.service,
            not_before_ms: Some(1_120_000),
            limit: Some(1),
            remaining: Some(0),
            observed_ms: 1_002_000,
            manual_hold: None,
        })
        .unwrap();
    drop(library);
    let library = Library::open(&path).unwrap();
    assert_eq!(
        library.claim_start(&claim).unwrap(),
        StartDecision::WaitUntil(1_120_000)
    );
    claim.now_ms = 1_120_000;
    assert_eq!(library.claim_start(&claim).unwrap(), StartDecision::Granted);
}

#[cfg(feature = "autotag")]
#[test]
fn concurrent_consumers_share_one_atomic_start_and_bounded_expiring_cache() {
    use cuerithm_autotag::{
        CachedResponse, CatalogService, CatalogState, MAX_CATALOG_JSON_BYTES, RateClaim,
        StartDecision,
    };
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("library.sqlite");
    let library = Library::open(&path).unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let threads = (0..8)
        .map(|_| {
            let library = library.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                library
                    .claim_start(&RateClaim {
                        service: CatalogService::Discogs,
                        now_ms: 10_000,
                        min_interval_ms: 0,
                        window_ms: 60_000,
                        max_starts: 1,
                    })
                    .unwrap()
            })
        })
        .collect::<Vec<_>>();
    let decisions = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        decisions
            .iter()
            .filter(|value| **value == StartDecision::Granted)
            .count(),
        1
    );
    assert!(decisions.iter().all(|value| matches!(
        value,
        StartDecision::Granted | StartDecision::WaitUntil(70_000)
    )));
    let key = "a".repeat(64);
    library
        .cache_put(
            &key,
            &CachedResponse {
                body: b"public metadata".to_vec(),
                expires_ms: 50_000,
            },
        )
        .unwrap();
    assert_eq!(
        library.cache_get(&key, 49_999).unwrap().unwrap().body,
        b"public metadata"
    );
    assert!(library.cache_get(&key, 50_000).unwrap().is_none());
    assert!(
        library
            .cache_put(
                "https://query.invalid/?private-input",
                &CachedResponse {
                    body: vec![],
                    expires_ms: 50_000
                }
            )
            .is_err()
    );
    assert!(
        library
            .cache_put(
                &key,
                &CachedResponse {
                    body: vec![0; MAX_CATALOG_JSON_BYTES + 1],
                    expires_ms: 50_000
                }
            )
            .is_err()
    );
}

#[cfg(feature = "autotag")]
#[test]
fn an_unknown_quota_hold_survives_restart_and_explicit_clear_keeps_consumed_starts() {
    use cuerithm_autotag::{
        CatalogService, CatalogState, RateClaim, RateObservation, StartDecision,
    };
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("library.sqlite");
    let library = Library::open(&path).unwrap();
    let mut claim = RateClaim {
        service: CatalogService::SonoVault,
        now_ms: 1000,
        min_interval_ms: 0,
        window_ms: 60000,
        max_starts: 1,
    };
    assert_eq!(library.claim_start(&claim).unwrap(), StartDecision::Granted);
    library
        .observe(&RateObservation {
            service: claim.service,
            not_before_ms: None,
            limit: None,
            remaining: None,
            observed_ms: 2000,
            manual_hold: Some(true),
        })
        .unwrap();
    drop(library);
    let library = Library::open(&path).unwrap();
    claim.now_ms = 3000;
    assert_eq!(
        library.claim_start(&claim).unwrap(),
        StartDecision::WaitUntil(i64::MAX)
    );
    library
        .observe(&RateObservation {
            service: claim.service,
            not_before_ms: None,
            limit: None,
            remaining: None,
            observed_ms: 3000,
            manual_hold: Some(false),
        })
        .unwrap();
    assert_eq!(
        library.claim_start(&claim).unwrap(),
        StartDecision::WaitUntil(61000)
    );
    claim.now_ms = 61000;
    assert_eq!(library.claim_start(&claim).unwrap(), StartDecision::Granted);
}
