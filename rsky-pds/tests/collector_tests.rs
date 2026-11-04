//! The service runs the blob collector itself once the store is no longer
//! shared: a deleted account's namespace is visited on the first pass.

mod common;

use common::get_client_in;
use rocket::http::Status;
use rsky_pds::blob_generations::Generations;
use rsky_pds::lifecycle::{LifecycleStore, PurgeObligation};

const DID: &str = "did:plc:collected";

#[tokio::test]
async fn the_collector_runs_in_the_service_when_enabled() {
    let dir = tempfile::tempdir().unwrap();
    let path = |name: &str| dir.path().join(name);
    for (key, name) in [
        ("PDS_ACTOR_STORE_DIRECTORY", "actors"),
        ("PDS_ACCOUNT_DB_LOCATION", "account.sqlite"),
        ("PDS_SEQUENCER_DB_LOCATION", "sequencer.sqlite"),
        ("PDS_DID_CACHE_DB_LOCATION", "did_cache.sqlite"),
        ("PDS_LIFECYCLE_DB", "rsky/lifecycle.sqlite"),
        ("PDS_LOCK_DIR", "rsky/locks"),
        ("PDS_BLOB_ATTEMPTS_DB", "rsky/blob-attempts.sqlite"),
        ("PDS_BLOB_GENERATIONS_DB", "rsky/blob-generations.sqlite"),
        ("PDS_REPAIR_DB", "rsky/repair.sqlite"),
    ] {
        std::env::set_var(key, path(name));
    }
    std::env::set_var("PDS_BLOB_GC_ENABLED", "true");
    std::env::set_var("PDS_BLOB_GC_INTERVAL_SECS", "1");
    Generations::open(path("rsky/blob-generations.sqlite"))
        .await
        .unwrap();
    let lifecycle = LifecycleStore::open(path("rsky/lifecycle.sqlite"))
        .await
        .unwrap();
    lifecycle
        .record_purge_obligation(&PurgeObligation {
            did: DID.to_owned(),
            requested_at: rsky_common::now(),
            namespace_prefixes: vec![],
            manifest: serde_json::json!({}),
        })
        .await
        .unwrap();

    let client = get_client_in(dir.path()).await;
    let needle = "pds_blob_collector_outcomes_total{outcome=\"purge-observed-empty\"} 1";
    let started = std::time::Instant::now();
    loop {
        let response = client.get("/metrics").dispatch().await;
        assert_eq!(response.status(), Status::Ok);
        let text = response.into_string().await.unwrap();
        if text.contains(needle) {
            break;
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(30),
            "the collector never ran: {text}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let progress = lifecycle
        .purge_progress_of(DID)
        .await
        .unwrap()
        .expect("the obligation was visited");
    assert!(progress.observed_empty_at.is_some());
    assert!(progress.physically_purged_at.is_none());
    assert_eq!(lifecycle.open_purge_obligations().await.unwrap().len(), 1);
}
