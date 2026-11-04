//! `GET /metrics` and the request accounting behind it.

mod common;

use common::{create_account, get_client};
use rocket::http::Status;
use rsky_pds::lifecycle::LifecycleStore;

#[tokio::test]
async fn metrics_report_requests_and_outstanding_work() {
    let (_dir, client) = get_client().await;
    create_account(&client).await;
    let did = "did:plc:khvyd3oiw46vif5gm7hijslk";
    rusqlite::Connection::open(_dir.path().join("account.sqlite"))
        .unwrap()
        .execute(
            "UPDATE actor SET \"deactivatedAt\" = NULL, \"deleteAfter\" = NULL WHERE did = ?1",
            [did],
        )
        .unwrap();
    let live = client.get("/xrpc/_health/live").dispatch().await;
    assert_eq!(live.status(), Status::Ok);
    let missing = client.get("/no/such/route").dispatch().await;
    assert_eq!(missing.status(), Status::NotFound);
    let bad_token = client
        .get("/xrpc/com.atproto.server.getSession")
        .header(rocket::http::Header::new("Authorization", "Bearer nope"))
        .dispatch()
        .await;
    assert_eq!(bad_token.status(), Status::BadRequest);
    // an actor marked with undelivered work is reported by name
    client
        .rocket()
        .state::<LifecycleStore>()
        .unwrap()
        .mark_pending_work(did)
        .await
        .unwrap();

    let response = client.get("/metrics").dispatch().await;
    assert_eq!(response.status(), Status::Ok);
    let text = response.into_string().await.unwrap();
    for needle in [
        "pds_http_requests_total{method=\"GET\",route=\"/xrpc/_health/live\",status=\"200\"}",
        "pds_http_requests_total{method=\"GET\",route=\"unrouted\",status=\"404\"}",
        "pds_http_requests_total{method=\"POST\",route=\"/xrpc/com.atproto.server.createAccount\",status=\"200\"}",
        "pds_http_request_duration_seconds_bucket{route=\"/xrpc/_health/live\"",
        "pds_auth_failures_total{kind=\"BadJwt\"}",
        &format!("pds_publish_intents_pending{{did=\"{did}\"}} 0"),
        &format!("pds_blob_work_nonterminal{{did=\"{did}\"}} 0"),
        "pds_pending_work_accounts 1",
        "pds_lifecycle_pending 0",
        "pds_repair_pending 0",
        "pds_write_attempts_total",
        "pds_control_journal_writes_total{table=\"pending_work\"}",
        "pds_sequencer_last_seq",
        "pds_firehose_subscribers 0",
        "pds_shutting_down 0",
    ] {
        assert!(text.contains(needle), "{needle} missing from:\n{text}");
    }
}
