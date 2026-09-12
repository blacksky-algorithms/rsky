//! The reference PDS's request limits, kept in this process.

mod common;

use common::{create_account, get_client};
use rocket::http::{ContentType, Header, Status};
use rocket::local::asynchronous::Client;
use rsky_pds::rate_limits::{RateLimits, CREATE_SESSION, GLOBAL_IP};
use serde_json::{json, Value};

const DID: &str = "did:plc:khvyd3oiw46vif5gm7hijslk";

async fn login(
    client: &Client,
    password: &str,
    bypass: bool,
) -> (Status, Value, Vec<(String, String)>) {
    let mut request = client
        .post("/xrpc/com.atproto.server.createSession")
        .header(ContentType::JSON)
        .body(json!({"identifier": "foo@example.com", "password": password}).to_string());
    if bypass {
        request = request.header(Header::new("x-ratelimit-bypass", "bypass-me"));
    }
    let response = request.dispatch().await;
    let status = response.status();
    let headers: Vec<(String, String)> = response
        .headers()
        .iter()
        .filter(|h| h.name.as_str().starts_with("RateLimit") || h.name == "Retry-After")
        .map(|h| (h.name.to_string(), h.value.to_string()))
        .collect();
    let body: Value = response.into_json().await.unwrap_or(Value::Null);
    (status, body, headers)
}

#[tokio::test]
async fn limits_follow_the_reference_table() {
    std::env::set_var("PDS_RATE_LIMITS_ENABLED", "true");
    std::env::set_var("PDS_RATE_LIMIT_BYPASS_KEY", "bypass-me");
    let (dir, client) = get_client().await;
    let (identifier, password) = create_account(&client).await;
    rusqlite::Connection::open(dir.path().join("account.sqlite"))
        .unwrap()
        .execute(
            "UPDATE actor SET \"deactivatedAt\" = NULL, \"deleteAfter\" = NULL WHERE did = ?1",
            [DID],
        )
        .unwrap();
    assert_eq!(identifier, "foo@example.com");

    // createSession: 30 attempts per identifier and address in five
    // minutes; most of the budget is spent directly, since a password
    // check is slow in a debug build
    let limits = client.rocket().state::<RateLimits>().unwrap();
    limits
        .consume(&CREATE_SESSION[1], "foo@example.com-", 27)
        .unwrap();
    for attempt in 0..3 {
        let (status, body, _) = login(&client, "wrong", false).await;
        assert_ne!(status, Status::TooManyRequests, "attempt {attempt}: {body}");
    }
    let (status, body, headers) = login(&client, "wrong", false).await;
    assert_eq!(status, Status::TooManyRequests, "{body}");
    assert_eq!(body["error"], "RateLimitExceeded");
    assert_eq!(body["message"], "Rate Limit Exceeded");
    let header = |name: &str| {
        headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| panic!("{name} missing from {headers:?}"))
    };
    assert_eq!(header("RateLimit-Limit"), "30");
    assert_eq!(header("RateLimit-Remaining"), "0");
    assert_eq!(header("RateLimit-Policy"), "30;w=300");
    assert!(header("RateLimit-Reset").parse::<u64>().unwrap() > 1_600_000_000);
    assert!(header("Retry-After").parse::<u64>().unwrap() <= 300);
    // the bypass key skips every limit
    let (status, body, _) = login(&client, &password, true).await;
    assert_eq!(status, Status::Ok, "{body}");
    let token = format!("Bearer {}", body["accessJwt"].as_str().unwrap());

    // repository writes are charged by kind
    let post = |rkey: &str| {
        json!({
            "repo": DID,
            "collection": "app.bsky.feed.post",
            "rkey": rkey,
            "record": {
                "$type": "app.bsky.feed.post",
                "text": "limited",
                "createdAt": "2024-01-01T00:00:00.000Z",
            },
        })
    };
    let created = client
        .post("/xrpc/com.atproto.repo.createRecord")
        .header(ContentType::JSON)
        .header(Header::new("Authorization", token.clone()))
        .body(post("3lratelimitaa").to_string())
        .dispatch()
        .await;
    assert_eq!(created.status(), Status::Ok);
    let applied = client
        .post("/xrpc/com.atproto.repo.applyWrites")
        .header(ContentType::JSON)
        .header(Header::new("Authorization", token.clone()))
        .body(
            json!({
                "repo": DID,
                "writes": [
                    {"$type": "com.atproto.repo.applyWrites#create", "collection": "app.bsky.feed.post", "rkey": "3lratelimitab", "value": post("x")["record"]},
                    {"$type": "com.atproto.repo.applyWrites#update", "collection": "app.bsky.feed.post", "rkey": "3lratelimitaa", "value": post("x")["record"]},
                    {"$type": "com.atproto.repo.applyWrites#delete", "collection": "app.bsky.feed.post", "rkey": "3lratelimitab"},
                ]
            })
            .to_string(),
        )
        .dispatch()
        .await;
    assert_eq!(applied.status(), Status::Ok);
    let put = client
        .post("/xrpc/com.atproto.repo.putRecord")
        .header(ContentType::JSON)
        .header(Header::new("Authorization", token.clone()))
        .body(post("3lratelimitaa").to_string())
        .dispatch()
        .await;
    assert_eq!(put.status(), Status::Ok);
    let deleted = client
        .post("/xrpc/com.atproto.repo.deleteRecord")
        .header(ContentType::JSON)
        .header(Header::new("Authorization", token.clone()))
        .body(
            json!({"repo": DID, "collection": "app.bsky.feed.post", "rkey": "3lratelimitaa"})
                .to_string(),
        )
        .dispatch()
        .await;
    assert_eq!(deleted.status(), Status::Ok);

    // the account routes charge their own budgets before doing anything
    for (path, body) in [
        (
            "/xrpc/com.atproto.server.requestPasswordReset",
            json!({"email": "foo@example.com"}),
        ),
        ("/xrpc/com.atproto.server.requestEmailUpdate", json!({})),
        (
            "/xrpc/com.atproto.server.requestEmailConfirmation",
            json!({}),
        ),
        (
            "/xrpc/com.atproto.identity.updateHandle",
            json!({"handle": "bar.rsky.com"}),
        ),
    ] {
        let response = client
            .post(path)
            .header(ContentType::JSON)
            .header(Header::new("Authorization", token.clone()))
            .body(body.to_string())
            .dispatch()
            .await;
        assert_ne!(response.status(), Status::TooManyRequests, "{path}");
        assert_ne!(response.status(), Status::NotFound, "{path}");
    }

    // the global budget: 3000 XRPC requests per address in five minutes,
    // keyed by the forwarded address; the budget is spent down directly
    // rather than with three thousand requests
    let _ = limits.consume(&GLOBAL_IP, "", 2_990);
    let mut refused = None;
    for _ in 0..11 {
        let response = client.get("/xrpc/_health/live").dispatch().await;
        if response.status() == Status::TooManyRequests {
            refused = Some(response);
            break;
        }
    }
    let refused = refused.expect("the global limit never triggered");
    assert_eq!(refused.headers().get_one("RateLimit-Limit"), Some("3000"));
    let body: Value = refused.into_json().await.unwrap();
    assert_eq!(body["error"], "RateLimitExceeded");
    let other_address = client
        .get("/xrpc/_health/live")
        .header(Header::new("X-Forwarded-For", "203.0.113.9, 10.0.0.1"))
        .dispatch()
        .await;
    assert_eq!(other_address.status(), Status::Ok);
    let export = client
        .get(format!("/xrpc/com.atproto.sync.getRepo?did={DID}"))
        .dispatch()
        .await;
    assert_eq!(export.status(), Status::Ok);
    let bypassed = client
        .get("/xrpc/_health/live")
        .header(Header::new("x-ratelimit-bypass", "bypass-me"))
        .dispatch()
        .await;
    assert_eq!(bypassed.status(), Status::Ok);
}
