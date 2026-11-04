//! `importRepo` honours the reference deployment settings: the size limit
//! is read from `PDS_MAX_REPO_IMPORT_SIZE`, and a server that is not
//! accepting imports says so.

mod common;

use common::get_client_with_fixture_copy;
use rocket::http::{ContentType, Header, Status};
use serde_json::Value;

#[tokio::test]
async fn import_size_and_acceptance_follow_the_settings() {
    std::env::set_var("PDS_ACCEPTING_REPO_IMPORTS", "false");
    std::env::set_var("PDS_MAX_REPO_IMPORT_SIZE", "16");
    let (fixture, _dir, client) = get_client_with_fixture_copy().await;
    let alice = fixture.did("alice");
    let response = client
        .get(format!("/xrpc/com.atproto.sync.getRepo?did={alice}"))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let car = response.into_bytes().await.unwrap();
    assert!(car.len() > 16);

    let import = |car: Vec<u8>| {
        client
            .post("/xrpc/com.atproto.repo.importRepo")
            .header(ContentType::new("application", "vnd.ipld.car"))
            .header(Header::new("content-length", car.len().to_string()))
            .header(Header::new(
                "Authorization",
                format!("Bearer {}", fixture.token("alice_access")),
            ))
            .body(car)
            .dispatch()
    };

    let response = import(car.clone()).await;
    assert_eq!(response.status(), Status::BadRequest);
    let body: Value = response.into_json().await.unwrap();
    assert_eq!(body["error"], "InvalidRequest");
    assert!(
        body["message"]
            .as_str()
            .unwrap()
            .starts_with("Content-Length is greater than maximum of"),
        "{body}"
    );

    std::env::remove_var("PDS_MAX_REPO_IMPORT_SIZE");
    let response = import(car).await;
    assert_eq!(response.status(), Status::BadRequest);
    let body: Value = response.into_json().await.unwrap();
    assert_eq!(body["error"], "InvalidRequest");
    assert_eq!(body["message"], "Service is not accepting repo imports");
}
