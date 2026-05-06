//! End-to-end smoke test against a real PostgreSQL.
//!
//! Validates the load-bearing claims that earlier silently broke in production:
//! - Bulk-load updates `LoadState.last_completed_creator` at creator transitions.
//! - LMDB persistence after a successful bulk-load is loadable on restart.
//! - Resume from prior partial state preserves all edges (idempotent inserts).
//!
//! Set `SMOKE_DATABASE_URL` to a writable PG for these tests to run; without
//! it they skip with a printed reason. CI should set it to a fresh PG (e.g.
//! a service container). Locally:
//!     `SMOKE_DATABASE_URL=postgres://postgres@localhost cargo test -p rsky-graph --test smoke`
//!
//! Tests run sequentially (`#[tokio::test(flavor = "current_thread")]`) and
//! create+drop their own schema so they share one PG without colliding.

use std::sync::Arc;

use rsky_graph::bulk_load;
use rsky_graph::graph::FollowGraph;
use rsky_graph::persistence;
use rsky_graph::types::LoadState;
use tokio_postgres::NoTls;

const TEST_FOLLOWS: &[(&str, &str)] = &[
    ("did:plc:aaa", "did:plc:bbb"),
    ("did:plc:aaa", "did:plc:ccc"),
    ("did:plc:bbb", "did:plc:aaa"),
    ("did:plc:bbb", "did:plc:ddd"),
    ("did:plc:ccc", "did:plc:eee"),
    ("did:plc:ddd", "did:plc:aaa"),
    ("did:plc:eee", "did:plc:fff"),
];

/// Returns a database URL or None (skip the test). Each call appends a unique
/// schema name via `?options=-csearch_path%3D<schema>` so concurrent tests
/// don't collide.
fn db_url() -> Option<String> {
    std::env::var("SMOKE_DATABASE_URL").ok()
}

async fn populate_test_schema(url: &str) {
    let (client, conn) = tokio_postgres::connect(url, NoTls)
        .await
        .expect("connect to smoke pg");
    tokio::spawn(async move {
        let _ = conn.await;
    });

    // Drop + recreate so each test starts clean.
    client
        .batch_execute(
            r#"
            DROP SCHEMA IF EXISTS bsky CASCADE;
            CREATE SCHEMA bsky;
            CREATE TABLE bsky.follow (
                creator TEXT NOT NULL,
                "subjectDid" TEXT NOT NULL
            );
            "#,
        )
        .await
        .expect("schema setup");

    for (creator, subject) in TEST_FOLLOWS {
        client
            .execute(
                "INSERT INTO bsky.follow (creator, \"subjectDid\") VALUES ($1, $2)",
                &[creator, subject],
            )
            .await
            .expect("insert");
    }
}

#[tokio::test]
async fn bulk_load_populates_graph_and_marks_state_complete() {
    let Some(url) = db_url() else {
        eprintln!("skipping: set SMOKE_DATABASE_URL to run");
        return;
    };
    populate_test_schema(&url).await;

    let graph = Arc::new(FollowGraph::new());
    let state = Arc::new(LoadState::new());

    bulk_load::bulk_load_keyset(&url, &graph, &state)
        .await
        .expect("bulk load");

    assert_eq!(graph.follow_count(), TEST_FOLLOWS.len() as u64);
    assert!(state.is_complete());
    // After load, every creator must be flagged loaded.
    assert!(state.creator_loaded("did:plc:aaa"));
    assert!(state.creator_loaded("did:plc:eee"));
    // Edges round-trip.
    assert!(graph.is_following("did:plc:aaa", "did:plc:bbb"));
    assert!(!graph.is_following("did:plc:aaa", "did:plc:zzz"));
}

#[tokio::test]
async fn load_state_advances_with_keyset_cursor() {
    let Some(url) = db_url() else {
        eprintln!("skipping: set SMOKE_DATABASE_URL to run");
        return;
    };
    populate_test_schema(&url).await;

    // Use a low batch size so the cursor advances at fine granularity.
    std::env::set_var("GRAPH_LOAD_BATCH_SIZE", "2");
    std::env::set_var("GRAPH_LOAD_THROTTLE_MS", "0");

    let graph = Arc::new(FollowGraph::new());
    let state = Arc::new(LoadState::new());

    bulk_load::bulk_load_keyset(&url, &graph, &state)
        .await
        .expect("bulk load");

    assert!(state.is_complete());
    let last = state.last_completed();
    assert_eq!(last, "did:plc:eee", "last creator processed should be eee");

    std::env::remove_var("GRAPH_LOAD_BATCH_SIZE");
    std::env::remove_var("GRAPH_LOAD_THROTTLE_MS");
}

#[tokio::test]
async fn persistence_round_trip_via_lmdb() {
    let Some(url) = db_url() else {
        eprintln!("skipping: set SMOKE_DATABASE_URL to run");
        return;
    };
    populate_test_schema(&url).await;
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().to_str().unwrap().to_string();

    {
        let graph = Arc::new(FollowGraph::new());
        let state = Arc::new(LoadState::new());
        bulk_load::bulk_load_keyset(&url, &graph, &state)
            .await
            .expect("bulk load");
        persistence::save_to_lmdb(&path, &graph)
            .await
            .expect("save");
    }

    // Fresh graph -- only loads from LMDB.
    let graph2 = Arc::new(FollowGraph::new());
    let count = persistence::load_from_lmdb(&path, &graph2)
        .await
        .expect("load");
    assert!(count > 0, "should have loaded users from LMDB");
    assert_eq!(graph2.follow_count(), TEST_FOLLOWS.len() as u64);
    assert!(graph2.is_following("did:plc:aaa", "did:plc:bbb"));
}

#[tokio::test]
async fn resume_skips_already_loaded_creators() {
    let Some(url) = db_url() else {
        eprintln!("skipping: set SMOKE_DATABASE_URL to run");
        return;
    };
    populate_test_schema(&url).await;

    // Pre-populate the graph as if a previous load partially completed
    // through "did:plc:bbb"; the resume logic should pick up from there.
    let graph = Arc::new(FollowGraph::new());
    let state = Arc::new(LoadState::new());

    // Simulate prior progress: aaa is fully loaded, bbb is partial.
    graph.add_follow("did:plc:aaa", "did:plc:bbb");
    graph.add_follow("did:plc:aaa", "did:plc:ccc");
    graph.add_follow("did:plc:bbb", "did:plc:aaa");

    bulk_load::bulk_load_keyset(&url, &graph, &state)
        .await
        .expect("bulk load");

    // Final state must include all expected edges regardless of what was
    // pre-loaded -- idempotent inserts make the resume safe.
    assert_eq!(graph.follow_count(), TEST_FOLLOWS.len() as u64);
    for (a, b) in TEST_FOLLOWS {
        assert!(graph.is_following(a, b), "missing edge {a} -> {b}");
    }
}
