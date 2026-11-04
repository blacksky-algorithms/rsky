//! The routes the production image adds beside the reference PDS, answered
//! the way its edge and clients expect.

mod common;

use common::{create_account, get_client};
use rocket::http::{Header, Status};
use rocket::local::asynchronous::Client;
use serde_json::Value;

async fn get(client: &Client, path: &str, host: Option<&str>) -> (Status, String) {
    let mut request = client.get(path);
    if let Some(host) = host {
        request = request.header(Header::new("Host", host.to_string()));
    }
    let response = request.dispatch().await;
    let status = response.status();
    (status, response.into_string().await.unwrap_or_default())
}

fn error_message(body: &str) -> String {
    let json: Value = serde_json::from_str(body).unwrap_or_else(|_| panic!("non-json: {body}"));
    json["message"].as_str().unwrap_or_default().to_string()
}

#[tokio::test]
async fn image_routes_answer_like_the_production_image() {
    std::env::set_var("PDS_EXTRA_HANDLE_DOMAINS", ".extra.test");
    let (dir, client) = get_client().await;
    create_account(&client).await;
    let did = "did:plc:khvyd3oiw46vif5gm7hijslk";
    // the harness creates accounts deactivated; a deactivated account has
    // no resolvable handle on either implementation
    rusqlite::Connection::open(dir.path().join("account.sqlite"))
        .unwrap()
        .execute(
            "UPDATE actor SET \"deactivatedAt\" = NULL, \"deleteAfter\" = NULL WHERE did = ?1",
            [did],
        )
        .unwrap();

    // tls-check: the host itself and every hosted handle domain, whether or
    // not the account exists yet
    let (status, body) = get(&client, "/tls-check", None).await;
    assert_eq!(status, Status::BadRequest);
    assert_eq!(error_message(&body), "bad or missing domain query param");
    let (status, _) = get(&client, "/tls-check?domain=", None).await;
    assert_eq!(status, Status::BadRequest);
    for domain in [
        "rsky.com",
        "foo.rsky.com",
        "nobody.rsky.com",
        "someone.extra.test",
    ] {
        let (status, body) = get(&client, &format!("/tls-check?domain={domain}"), None).await;
        assert_eq!(status, Status::Ok, "{domain}: {body}");
        assert_eq!(body, r#"{"success":true}"#);
    }
    let (status, body) = get(&client, "/tls-check?domain=evil.example", None).await;
    assert_eq!(status, Status::BadRequest);
    assert_eq!(
        error_message(&body),
        "handles are not provided on this domain"
    );

    // well-known DID: query first, then the request host, plain text
    let (status, body) = get(&client, "/custom-well-known-atproto-did", None).await;
    assert_eq!(status, Status::NotFound);
    assert_eq!(body, "User not found");
    let (status, body) = get(
        &client,
        "/custom-well-known-atproto-did?handle=foo.rsky.com",
        None,
    )
    .await;
    assert_eq!((status, body.as_str()), (Status::Ok, did));
    let (status, body) = get(
        &client,
        "/custom-well-known-atproto-did?handle=",
        Some("foo.rsky.com"),
    )
    .await;
    assert_eq!((status, body.as_str()), (Status::Ok, did));
    for handle in ["nobody.rsky.com", "foo.other.example"] {
        let (status, body) = get(
            &client,
            &format!("/custom-well-known-atproto-did?handle={handle}"),
            None,
        )
        .await;
        assert_eq!(
            (status, body.as_str()),
            (Status::NotFound, "User not found")
        );
    }

    // resolve-handle: local accounts, then the network for foreign handles
    let (status, body) = get(&client, "/custom-resolve-handle", None).await;
    assert_eq!(status, Status::BadRequest);
    assert_eq!(error_message(&body), "bad or missing handle query param");
    let (status, body) = get(&client, "/custom-resolve-handle?handle=FOO.rsky.com", None).await;
    assert_eq!(status, Status::Ok, "{body}");
    assert_eq!(body, format!(r#"{{"did":"{did}"}}"#));
    for handle in ["nobody.rsky.com", "nobody.invalid"] {
        let (status, body) = get(
            &client,
            &format!("/custom-resolve-handle?handle={handle}"),
            None,
        )
        .await;
        assert_eq!(status, Status::BadRequest, "{handle}: {body}");
        assert_eq!(error_message(&body), "Unable to resolve handle");
    }
}
