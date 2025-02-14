use rocket::http::{ContentType, Status};
use testcontainers::runners::AsyncRunner;
use rsky_lexicon::com::atproto::server::{CreateSessionInput, CreateSessionOutput};
use testcontainers_modules::{postgres};
mod common;

#[tokio::test]
async fn test_create_session() {
    let client = common::setup().await;
    let input = CreateSessionInput {
        identifier: "dw12a1d321.rsky.ripperoni.com".to_string(),
        password: "ngAUvO6BGYipTfDjjdT9ozgJ".to_string(),
    };
    let response = client
        .post("/xrpc/com.atproto.server.createSession")
        .header(ContentType::JSON)
        .body(serde_json::to_string(&input).unwrap().into_bytes())
        .dispatch()
        .await;
    let status = response.status();

    assert_eq!(status, Status::Ok);
    response.into_json::<CreateSessionOutput>().await.unwrap();
}

#[tokio::test]
async fn test_index() {
    let client = common::setup().await;
    let response = client.get("/").dispatch().await;
    assert_eq!(response.status(), Status::Ok);
}

#[tokio::test]
async fn test_robots_txt() {
    let client = common::setup().await;
    let response = client.get("/robots.txt").dispatch().await;
    let response_status = response.status();
    let response_body = response.into_string().await.unwrap();
    assert_eq!(response_status, Status::Ok);
    assert_eq!(
        response_body,
        "# Hello!\n\n# Crawling the public API is allowed\nUser-agent: *\nAllow: /"
    );
}