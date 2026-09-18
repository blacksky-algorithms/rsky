//! Deactivating, reactivating and deleting from the account pages, and
//! creating an account from the pages or inside an authorization request.

use rocket::http::{ContentType, Status};
use rocket::local::asynchronous::{Client, LocalResponse};
use rusqlite::Connection;
use serde_json::json;

mod common;
use common::oauth::*;

const TEST_DID: &str = "did:plc:khvyd3oiw46vif5gm7hijslk";
const MANAGE: &str = "/account/u/foo.rsky.com/manage";

async fn get<'c>(client: &'c Client, path: &str, cookie: &str) -> LocalResponse<'c> {
    client
        .get(path.to_string())
        .cookie(("device-id", cookie.to_string()))
        .dispatch()
        .await
}

async fn post<'c>(
    client: &'c Client,
    path: &str,
    cookie: &str,
    pairs: &[(&str, &str)],
) -> LocalResponse<'c> {
    client
        .post(path.to_string())
        .header(ContentType::Form)
        .cookie(("device-id", cookie.to_string()))
        .body(form_encode(pairs))
        .dispatch()
        .await
}

fn location(response: &LocalResponse<'_>) -> String {
    response
        .headers()
        .get_one("Location")
        .expect("redirect")
        .to_string()
}

fn cookie_of(response: &LocalResponse<'_>) -> String {
    response
        .cookies()
        .get("device-id")
        .expect("device cookie")
        .value()
        .to_string()
}

fn mailed_token(dir: &std::path::Path, purpose: &str) -> String {
    Connection::open(dir.join("account.sqlite"))
        .unwrap()
        .query_row(
            "SELECT token FROM email_token WHERE did = ?1 AND purpose = ?2",
            rusqlite::params![TEST_DID, purpose],
            |row| row.get(0),
        )
        .expect("a token was mailed")
}

fn hidden_value(html: &str, name: &str) -> String {
    let marker = format!("name=\"{name}\" value=\"");
    let start = html.find(&marker).expect("hidden field") + marker.len();
    html[start..start + html[start..].find('"').unwrap()].to_string()
}

struct Manager {
    cookie: String,
    csrf: String,
}

async fn sign_in(client: &Client) -> Manager {
    let response = client.get("/account/sign-in").dispatch().await;
    let cookie = cookie_of(&response);
    let csrf = extract_csrf(&response.into_string().await.unwrap());
    let response = post(
        client,
        "/account/sign-in",
        &cookie,
        &[
            ("csrf", &csrf),
            ("identifier", "foo@example.com"),
            ("password", "password"),
        ],
    )
    .await;
    assert_eq!(response.status(), Status::SeeOther);
    let cookie = cookie_of(&response);
    let response = get(client, MANAGE, &cookie).await;
    assert_eq!(response.status(), Status::Ok);
    let html = response.into_string().await.unwrap();
    Manager {
        cookie,
        csrf: extract_csrf(&html),
    }
}

fn publish_signing_key(client: &Client) {
    let actor_store = client
        .rocket()
        .state::<rsky_pds::actor_store::ActorStore>()
        .unwrap();
    let keypair = futures::executor::block_on(actor_store.keypair(TEST_DID)).unwrap();
    common::set_published_signing_key(Some(rsky_crypto::utils::encode_did_key(
        &keypair.public_key(),
    )));
}

#[tokio::test]
async fn deactivate_then_reactivate_from_the_pages() {
    std::env::set_var("PDS_INVITE_REQUIRED", "false");
    let (_dir, client) = get_oauth_client().await;
    create_active_account(&client).await;
    let key = dpop_key();
    let (request_uri, nonce) = run_par(&client, &key).await;
    let mut session = open_authorize_page(&client, &request_uri).await;
    let code = sign_in_and_accept(&client, &request_uri, &mut session).await;
    let tokens = exchange_code(&client, &key, &code, &nonce).await;
    let refresh_token = tokens["refresh_token"].as_str().unwrap().to_string();

    let manager = sign_in(&client).await;
    let response = get(&client, &format!("{MANAGE}/deactivate"), &manager.cookie).await;
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Deactivate account"), "{html}");
    assert!(html.contains(">Yes, Deactivate</button>"));
    let response = get(&client, &format!("{MANAGE}/reactivate"), &manager.cookie).await;
    assert_eq!(response.status(), Status::SeeOther);

    // the wrong password keeps the account active
    let response = post(
        &client,
        &format!("{MANAGE}/deactivate"),
        &manager.cookie,
        &[("csrf", &manager.csrf), ("password", "wrong")],
    )
    .await;
    assert_eq!(response.status(), Status::Ok);
    assert!(response
        .into_string()
        .await
        .unwrap()
        .contains("Invalid password"));

    let response = post(
        &client,
        &format!("{MANAGE}/deactivate"),
        &manager.cookie,
        &[("csrf", &manager.csrf), ("password", "password")],
    )
    .await;
    assert_eq!(response.status(), Status::SeeOther);
    assert_eq!(location(&response), MANAGE);

    // the restricted manage page, and the other sections closed
    let response = get(&client, MANAGE, &manager.cookie).await;
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Your account is deactivated."), "{html}");
    assert!(html.contains("Reactivate account"));
    assert!(html.contains("Delete account"));
    assert!(!html.contains("Email address"));
    let response = get(&client, &format!("{MANAGE}/email"), &manager.cookie).await;
    assert_eq!(response.status(), Status::SeeOther);
    let response = get(&client, &format!("{MANAGE}/handle"), &manager.cookie).await;
    assert_eq!(response.status(), Status::SeeOther);
    let response = get(&client, &format!("{MANAGE}/password"), &manager.cookie).await;
    assert_eq!(response.status(), Status::SeeOther);
    let response = get(&client, &format!("{MANAGE}/deactivate"), &manager.cookie).await;
    assert_eq!(response.status(), Status::SeeOther);

    // the OAuth session did not survive
    let token_htu = format!("{}/oauth/token", public_url(&client));
    let response = client
        .post("/oauth/token")
        .header(ContentType::Form)
        .header(rocket::http::Header::new(
            "DPoP",
            dpop_proof(&key, "POST", &token_htu, Some(&nonce), None),
        ))
        .body(form_encode(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", &refresh_token),
            ("client_id", LOOPBACK_CLIENT_ID),
        ]))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::BadRequest);

    // reactivation brings the sections back
    publish_signing_key(&client);
    let response = get(&client, &format!("{MANAGE}/reactivate"), &manager.cookie).await;
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Reactivate account"), "{html}");
    assert!(html.contains(">Reactivate</button>"));
    let response = post(
        &client,
        &format!("{MANAGE}/reactivate"),
        &manager.cookie,
        &[("csrf", &manager.csrf)],
    )
    .await;
    assert_eq!(response.status(), Status::SeeOther);
    assert_eq!(location(&response), format!("{MANAGE}?notice=reactivated"));
    let response = get(
        &client,
        &format!("{MANAGE}?notice=reactivated"),
        &manager.cookie,
    )
    .await;
    let html = response.into_string().await.unwrap();
    assert!(
        html.contains("Your account has been reactivated."),
        "{html}"
    );
    assert!(html.contains("Email address"));
    let response = post(
        &client,
        &format!("{MANAGE}/reactivate"),
        &manager.cookie,
        &[("csrf", &manager.csrf)],
    )
    .await;
    assert_eq!(response.status(), Status::SeeOther);
    common::set_published_signing_key(None);
}

#[tokio::test]
async fn delete_account_in_three_steps() {
    std::env::set_var("PDS_INVITE_REQUIRED", "false");
    let (dir, client) = get_oauth_client().await;
    create_active_account(&client).await;
    let manager = sign_in(&client).await;

    let response = get(&client, &format!("{MANAGE}/delete"), &manager.cookie).await;
    let html = response.into_string().await.unwrap();
    assert!(
        html.contains("Delete account <b>@foo.rsky.com</b>"),
        "{html}"
    );
    assert!(html.contains("confirmation code to your email address <b>foo@example.com</b>."));
    assert!(html.contains(">Send email</button>"));

    let response = post(
        &client,
        &format!("{MANAGE}/delete/request"),
        &manager.cookie,
        &[("csrf", &manager.csrf)],
    )
    .await;
    assert_eq!(response.status(), Status::Ok);
    let html = response.into_string().await.unwrap();
    assert!(
        html.contains("Check <b>foo@example.com</b> for an email"),
        "{html}"
    );
    assert!(html.contains("name=\"password\""));
    let code = mailed_token(dir.path(), "delete_account");

    // the wrong password or code stays on the form
    let response = post(
        &client,
        &format!("{MANAGE}/delete/verify"),
        &manager.cookie,
        &[
            ("csrf", &manager.csrf),
            ("code", &code),
            ("password", "wrong"),
        ],
    )
    .await;
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Invalid did or password"), "{html}");
    let response = post(
        &client,
        &format!("{MANAGE}/delete/verify"),
        &manager.cookie,
        &[
            ("csrf", &manager.csrf),
            ("code", "WRONG"),
            ("password", "password"),
        ],
    )
    .await;
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Token is invalid"), "{html}");

    // the right ones reach the last word, with an attestation and no password
    let response = post(
        &client,
        &format!("{MANAGE}/delete/verify"),
        &manager.cookie,
        &[
            ("csrf", &manager.csrf),
            ("code", &code),
            ("password", "password"),
        ],
    )
    .await;
    assert_eq!(response.status(), Status::Ok);
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Are you really, really sure?"), "{html}");
    assert!(!html.contains("password"));
    let intent = hidden_value(&html, "delete_intent");
    assert_eq!(hidden_value(&html, "code"), code);

    // a tampered or expired attestation sends the person back a step
    let response = post(
        &client,
        &format!("{MANAGE}/delete/confirm"),
        &manager.cookie,
        &[
            ("csrf", &manager.csrf),
            ("code", &code),
            ("delete_intent", "forged.9999999999"),
        ],
    )
    .await;
    assert_eq!(response.status(), Status::Ok);
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Please confirm again"), "{html}");
    assert!(html.contains("name=\"password\""));
    let expired = format!("{}.1", intent.split_once('.').unwrap().0);
    let response = post(
        &client,
        &format!("{MANAGE}/delete/confirm"),
        &manager.cookie,
        &[
            ("csrf", &manager.csrf),
            ("code", &code),
            ("delete_intent", &expired),
        ],
    )
    .await;
    assert!(response
        .into_string()
        .await
        .unwrap()
        .contains("Please confirm again"));

    // the real one deletes the account and ends every session of it
    let response = post(
        &client,
        &format!("{MANAGE}/delete/confirm"),
        &manager.cookie,
        &[
            ("csrf", &manager.csrf),
            ("code", &code),
            ("delete_intent", &intent),
        ],
    )
    .await;
    assert_eq!(response.status(), Status::SeeOther);
    assert_eq!(location(&response), "/account");
    let rotated = cookie_of(&response);
    assert_ne!(rotated, manager.cookie);
    let response = client
        .post("/xrpc/com.atproto.server.createSession")
        .header(ContentType::JSON)
        .body(json!({ "identifier": "foo.rsky.com", "password": "password" }).to_string())
        .dispatch()
        .await;
    assert_ne!(response.status(), Status::Ok);
    let response = get(&client, MANAGE, &rotated).await;
    assert_eq!(response.status(), Status::SeeOther);

    // replaying the final form after deletion does nothing
    let response = post(
        &client,
        &format!("{MANAGE}/delete/confirm"),
        &rotated,
        &[
            ("csrf", &manager.csrf),
            ("code", &code),
            ("delete_intent", &intent),
        ],
    )
    .await;
    assert_eq!(response.status(), Status::SeeOther);
}

#[tokio::test]
async fn sign_up_from_the_account_pages() {
    std::env::set_var("PDS_INVITE_REQUIRED", "false");
    let (_dir, client) = get_oauth_client().await;

    let response = client.get("/account/sign-up").dispatch().await;
    assert_eq!(response.status(), Status::Ok);
    let cookie = cookie_of(&response);
    let html = response.into_string().await.unwrap();
    assert!(html.contains("<title>Sign up</title>"), "{html}");
    assert!(html.contains("Step 1 of 2"));
    assert!(html.contains("name=\"domain\" value=\".rsky.com\""));
    let csrf = extract_csrf(&html);

    // step one carries the handle into step two
    let response = post(
        &client,
        "/account/sign-up",
        &cookie,
        &[
            ("csrf", &csrf),
            ("step", "handle"),
            ("handle", "Newbie"),
            ("domain", ".rsky.com"),
        ],
    )
    .await;
    assert_eq!(response.status(), Status::Ok);
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Step 2 of 2"), "{html}");
    assert!(html.contains("type=\"hidden\" name=\"handle\" value=\"newbie\""));
    assert!(!html.contains("name=\"invite_code\""));

    // a short password stays on step two
    let response = post(
        &client,
        "/account/sign-up",
        &cookie,
        &[
            ("csrf", &csrf),
            ("step", "credentials"),
            ("handle", "newbie"),
            ("domain", ".rsky.com"),
            ("email", "newbie@example.com"),
            ("password", "short"),
        ],
    )
    .await;
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Invalid password length."), "{html}");
    assert!(html.contains("name=\"email\" value=\"newbie@example.com\""));

    let response = post(
        &client,
        "/account/sign-up",
        &cookie,
        &[
            ("csrf", &csrf),
            ("step", "credentials"),
            ("handle", "newbie"),
            ("domain", ".rsky.com"),
            ("email", "newbie@example.com"),
            ("password", "a-long-enough-password"),
        ],
    )
    .await;
    assert_eq!(
        response.status(),
        Status::SeeOther,
        "{}",
        location(&response)
    );
    assert_eq!(location(&response), "/account/u/newbie.rsky.com");
    let signed_in = cookie_of(&response);
    assert_ne!(signed_in, cookie);
    let response = get(&client, "/account/u/newbie.rsky.com", &signed_in).await;
    assert_eq!(response.status(), Status::Ok);
    assert!(response
        .into_string()
        .await
        .unwrap()
        .contains("@newbie.rsky.com"));

    // the same handle again is refused with the reason
    let response = client
        .get("/account/sign-up?handle=newbie")
        .dispatch()
        .await;
    let cookie = cookie_of(&response);
    let html = response.into_string().await.unwrap();
    assert!(html.contains("name=\"handle\" value=\"newbie\""));
    let csrf = extract_csrf(&html);
    let response = post(
        &client,
        "/account/sign-up",
        &cookie,
        &[
            ("csrf", &csrf),
            ("step", "credentials"),
            ("handle", "newbie"),
            ("domain", ".rsky.com"),
            ("email", "other@example.com"),
            ("password", "a-long-enough-password"),
        ],
    )
    .await;
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Handle already taken"), "{html}");

    // a stale token re-renders step one
    let response = post(
        &client,
        "/account/sign-up",
        &cookie,
        &[("csrf", "stale"), ("step", "handle"), ("handle", "x")],
    )
    .await;
    assert!(response
        .into_string()
        .await
        .unwrap()
        .contains("Your session changed in another tab"));
}

#[tokio::test]
async fn sign_up_inside_an_authorization_request() {
    std::env::set_var("PDS_INVITE_REQUIRED", "false");
    let (_dir, client) = get_oauth_client().await;
    let key = dpop_key();
    let (request_uri, nonce) = run_par(&client, &key).await;

    // the welcome view offers sign-up inside the request
    let response = client
        .get(authorize_path(LOOPBACK_CLIENT_ID, &request_uri))
        .dispatch()
        .await;
    let cookie = cookie_of(&response);
    let html = response.into_string().await.unwrap();
    assert!(html.contains(">Welcome</h1>"), "{html}");
    assert!(html.contains("&amp;view=sign-up\">Create a new account</a>"));
    let csrf = extract_csrf(&html);

    let response = get(
        &client,
        &format!(
            "{}&view=sign-up",
            authorize_path(LOOPBACK_CLIENT_ID, &request_uri)
        ),
        &cookie,
    )
    .await;
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Choose a username"), "{html}");
    assert!(html.contains("name=\"request_uri\""));
    assert!(html.contains("&amp;view=welcome\">Back</a>"));

    let response = post(
        &client,
        "/oauth/authorize/sign-up",
        &cookie,
        &[
            ("csrf", &csrf),
            ("step", "handle"),
            ("handle", "flow"),
            ("domain", ".rsky.com"),
            ("client_id", LOOPBACK_CLIENT_ID),
            ("request_uri", &request_uri),
        ],
    )
    .await;
    assert_eq!(response.status(), Status::Ok);
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Step 2 of 2"), "{html}");
    assert!(html.contains("&amp;view=sign-up&amp;handle=flow\">Back</a>"));

    let response = post(
        &client,
        "/oauth/authorize/sign-up",
        &cookie,
        &[
            ("csrf", &csrf),
            ("step", "credentials"),
            ("handle", "flow"),
            ("domain", ".rsky.com"),
            ("email", "flow@example.com"),
            ("password", "a-long-enough-password"),
            ("client_id", LOOPBACK_CLIENT_ID),
            ("request_uri", &request_uri),
        ],
    )
    .await;
    assert_eq!(
        response.status(),
        Status::SeeOther,
        "{}",
        location(&response)
    );
    assert_eq!(
        location(&response),
        authorize_path(LOOPBACK_CLIENT_ID, &request_uri)
    );
    let signed_in = cookie_of(&response);

    // back on the request, the new account is offered consent right away
    let response = get(&client, &location(&response), &signed_in).await;
    assert_eq!(response.status(), Status::Ok);
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Sign in as..."), "{html}");
    assert!(html.contains("@flow.rsky.com"));
    let csrf = extract_csrf(&html);
    let response = post(
        &client,
        "/oauth/authorize/select",
        &signed_in,
        &[
            ("request_uri", &request_uri),
            ("client_id", LOOPBACK_CLIENT_ID),
            ("csrf", &csrf),
            ("did", &hidden_value(&html, "did")),
        ],
    )
    .await;
    assert_eq!(response.status(), Status::Ok);
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Authorize</button>"), "{html}");
    let response = post(
        &client,
        "/oauth/authorize/accept",
        &signed_in,
        &[
            ("request_uri", &request_uri),
            ("client_id", LOOPBACK_CLIENT_ID),
            ("csrf", &csrf),
            ("did", &hidden_value(&html, "did")),
        ],
    )
    .await;
    assert_eq!(response.status(), Status::SeeOther);
    let location = location(&response);
    let code = url::Url::parse(&location)
        .unwrap()
        .query_pairs()
        .find(|(key, _)| key == "code")
        .map(|(_, value)| value.into_owned())
        .expect("code");
    let tokens = exchange_code(&client, &key, &code, &nonce).await;
    assert!(tokens["sub"].as_str().unwrap().starts_with("did:plc:"));

    // a form from another site, or without a live request, is refused
    let response = client
        .post("/oauth/authorize/sign-up")
        .header(ContentType::Form)
        .header(rocket::http::Header::new("Origin", "https://evil.test"))
        .cookie(("device-id", signed_in.clone()))
        .body(form_encode(&[
            ("csrf", &csrf),
            ("step", "handle"),
            ("handle", "x"),
            ("client_id", LOOPBACK_CLIENT_ID),
            ("request_uri", &request_uri),
        ]))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Forbidden);
    let response = post(
        &client,
        "/oauth/authorize/sign-up",
        &signed_in,
        &[
            ("csrf", &csrf),
            ("step", "handle"),
            ("handle", "x"),
            ("client_id", LOOPBACK_CLIENT_ID),
            ("request_uri", "urn:ietf:params:oauth:request_uri:req-gone"),
        ],
    )
    .await;
    assert_eq!(response.status(), Status::BadRequest);
}
