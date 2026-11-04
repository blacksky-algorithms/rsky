//! The "Account" section of the account manager: email update and
//! verification, username, password, and the password reset a signed-out
//! person reaches from the sign-in page.

use rocket::http::{ContentType, Status};
use rocket::local::asynchronous::{Client, LocalResponse};
use rusqlite::Connection;

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

/// The token the server mailed for `purpose`, read from the database
/// since tests never send mail.
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

fn mark_email_confirmed(dir: &std::path::Path) {
    Connection::open(dir.join("account.sqlite"))
        .unwrap()
        .execute(
            "UPDATE account SET \"emailConfirmedAt\" = ?1 WHERE did = ?2",
            rusqlite::params!["2026-09-01T00:00:00.000Z", TEST_DID],
        )
        .unwrap();
}

struct Manager {
    cookie: String,
    csrf: String,
}

async fn sign_in(client: &Client) -> Manager {
    let response = client.get("/account/sign-in").dispatch().await;
    let cookie = response
        .cookies()
        .get("device-id")
        .unwrap()
        .value()
        .to_string();
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
    let cookie = response
        .cookies()
        .get("device-id")
        .unwrap()
        .value()
        .to_string();
    let response = get(client, MANAGE, &cookie).await;
    assert_eq!(response.status(), Status::Ok);
    let html = response.into_string().await.unwrap();
    Manager {
        cookie,
        csrf: extract_csrf(&html),
    }
}

#[tokio::test]
async fn manage_page_and_email_flows() {
    let (dir, client) = get_oauth_client().await;
    create_active_account(&client).await;
    let manager = sign_in(&client).await;

    let response = get(&client, MANAGE, &manager.cookie).await;
    let html = response.into_string().await.unwrap();
    assert!(html.contains("<h2>Account</h2>"), "{html}");
    assert!(html.contains("Your email address needs to be verified."));
    assert!(html.contains("foo@example.com"));
    assert!(html.contains("Deactivate account"));
    assert!(html.contains("Delete account"));

    // verifying the current address: request a code, enter it
    let response = get(&client, &format!("{MANAGE}/email/verify"), &manager.cookie).await;
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Verify your email"), "{html}");
    assert!(html.contains("Send verification code"));
    let response = post(
        &client,
        &format!("{MANAGE}/email/verify/request"),
        &manager.cookie,
        &[("csrf", &manager.csrf)],
    )
    .await;
    assert_eq!(response.status(), Status::Ok);
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Verification code"), "{html}");
    let response = post(
        &client,
        &format!("{MANAGE}/email/verify"),
        &manager.cookie,
        &[("csrf", &manager.csrf), ("code", "WRONG-CODE")],
    )
    .await;
    assert_eq!(response.status(), Status::Ok);
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Token is invalid"), "{html}");
    let code = mailed_token(dir.path(), "confirm_email");
    let response = post(
        &client,
        &format!("{MANAGE}/email/verify"),
        &manager.cookie,
        &[("csrf", &manager.csrf), ("code", &code)],
    )
    .await;
    assert_eq!(response.status(), Status::SeeOther);
    assert_eq!(location(&response), format!("{MANAGE}?notice=verified"));
    let response = get(
        &client,
        &format!("{MANAGE}?notice=verified"),
        &manager.cookie,
    )
    .await;
    let html = response.into_string().await.unwrap();
    assert!(
        html.contains("Your email address has been verified."),
        "{html}"
    );
    assert!(!html.contains("needs to be verified"));

    // changing a confirmed address: a code to the old one first
    let response = get(&client, &format!("{MANAGE}/email"), &manager.cookie).await;
    let html = response.into_string().await.unwrap();
    assert!(
        html.contains("Your account currently uses <b>foo@example.com</b>"),
        "{html}"
    );
    let response = post(
        &client,
        &format!("{MANAGE}/email/request"),
        &manager.cookie,
        &[("csrf", &manager.csrf), ("new_email", "not an email")],
    )
    .await;
    let html = response.into_string().await.unwrap();
    assert!(html.contains("not supported"), "{html}");
    let response = post(
        &client,
        &format!("{MANAGE}/email/request"),
        &manager.cookie,
        &[("csrf", &manager.csrf), ("new_email", "New@Example.org")],
    )
    .await;
    assert_eq!(response.status(), Status::Ok);
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Security step required"), "{html}");
    assert!(html.contains("name=\"new_email\" value=\"new@example.org\""));
    let response = post(
        &client,
        &format!("{MANAGE}/email/confirm"),
        &manager.cookie,
        &[
            ("csrf", &manager.csrf),
            ("new_email", "new@example.org"),
            ("code", "WRONG"),
        ],
    )
    .await;
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Token is invalid"), "{html}");
    assert!(html.contains("Security step required"));
    let code = mailed_token(dir.path(), "update_email");
    let response = post(
        &client,
        &format!("{MANAGE}/email/confirm"),
        &manager.cookie,
        &[
            ("csrf", &manager.csrf),
            ("new_email", "new@example.org"),
            ("code", &code),
        ],
    )
    .await;
    assert_eq!(response.status(), Status::Ok);
    let html = response.into_string().await.unwrap();
    assert!(
        html.contains("security code sent to <b>new@example.org</b>"),
        "{html}"
    );
    let response = get(&client, MANAGE, &manager.cookie).await;
    let html = response.into_string().await.unwrap();
    assert!(html.contains("new@example.org"));
    assert!(html.contains("needs to be verified"));

    // an unconfirmed address is replaced without a code
    let response = post(
        &client,
        &format!("{MANAGE}/email/request"),
        &manager.cookie,
        &[("csrf", &manager.csrf), ("new_email", "third@example.org")],
    )
    .await;
    assert_eq!(response.status(), Status::Ok);
    let html = response.into_string().await.unwrap();
    assert!(
        html.contains("security code sent to <b>third@example.org</b>"),
        "{html}"
    );
    assert!(html.contains("href=\"/account/u/foo.rsky.com/manage/email/verify?step=code\""));
    let response = get(
        &client,
        &format!("{MANAGE}/email/verify?step=code"),
        &manager.cookie,
    )
    .await;
    let html = response.into_string().await.unwrap();
    assert!(html.contains("name=\"code\""), "{html}");

    // a stale token on any of these forms is refused
    let response = post(
        &client,
        &format!("{MANAGE}/email/request"),
        &manager.cookie,
        &[("csrf", "stale"), ("new_email", "x@example.org")],
    )
    .await;
    assert_eq!(response.status(), Status::BadRequest);
    assert!(response
        .into_string()
        .await
        .unwrap()
        .contains("Your session changed in another tab"));

    // a code that cannot be recorded leaves the request form with the reason
    drop_table(dir.path(), "email_token");
    let response = post(
        &client,
        &format!("{MANAGE}/email/verify/request"),
        &manager.cookie,
        &[("csrf", &manager.csrf)],
    )
    .await;
    assert_eq!(response.status(), Status::Ok);
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Something went wrong"), "{html}");
}

#[tokio::test]
async fn a_custom_handle_prefills_the_custom_form() {
    let (_dir, client) = get_oauth_client().await;
    create_active_account(&client).await;
    let manager = sign_in(&client).await;
    client
        .rocket()
        .state::<rsky_pds::account_manager::AccountManager>()
        .unwrap()
        .update_handle(TEST_DID, "custom.example")
        .await
        .unwrap();

    let response = get(
        &client,
        &format!("/account/u/{TEST_DID}/manage/handle?mode=custom"),
        &manager.cookie,
    )
    .await;
    assert_eq!(response.status(), Status::Ok);
    let html = response.into_string().await.unwrap();
    assert!(
        html.contains("name=\"domain\" value=\"custom.example\""),
        "{html}"
    );
    assert!(html.contains("_atproto.custom.example"));
    let response = get(
        &client,
        &format!("/account/u/{TEST_DID}/manage/handle?mode=default"),
        &manager.cookie,
    )
    .await;
    let html = response.into_string().await.unwrap();
    assert!(html.contains("name=\"handle\" value=\"\""), "{html}");
}

#[tokio::test]
async fn handle_and_password_flows() {
    let (dir, client) = get_oauth_client().await;
    create_active_account(&client).await;
    let manager = sign_in(&client).await;

    // the chooser, then the default form seeded with the current handle
    let response = get(&client, &format!("{MANAGE}/handle"), &manager.cookie).await;
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Use a default username"), "{html}");
    assert!(html.contains("<em>alice.rsky.com</em>"));
    let response = get(
        &client,
        &format!("{MANAGE}/handle?mode=default"),
        &manager.cookie,
    )
    .await;
    let html = response.into_string().await.unwrap();
    assert!(html.contains("name=\"handle\" value=\"foo\""), "{html}");
    assert!(html.contains("name=\"domain\" value=\".rsky.com\""));
    let response = get(
        &client,
        &format!("{MANAGE}/handle?mode=custom"),
        &manager.cookie,
    )
    .await;
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Enter the domain you want to use"), "{html}");
    assert!(html.contains(&format!("did={TEST_DID}")));

    // an invalid handle stays on the form with the reason
    let response = post(
        &client,
        &format!("{MANAGE}/handle"),
        &manager.cookie,
        &[
            ("csrf", &manager.csrf),
            ("mode", "default"),
            ("handle", "a"),
            ("domain", ".rsky.com"),
        ],
    )
    .await;
    assert_eq!(response.status(), Status::Ok);
    let html = response.into_string().await.unwrap();
    assert!(html.contains("class=\"error\">"), "{html}");
    assert!(html.contains("name=\"handle\" value=\"a\""));

    // a domain this server does not offer falls back to its first one
    let response = post(
        &client,
        &format!("{MANAGE}/handle"),
        &manager.cookie,
        &[
            ("csrf", &manager.csrf),
            ("mode", "default"),
            ("handle", "Fallback"),
            ("domain", ".other.com"),
        ],
    )
    .await;
    assert_eq!(
        response.status(),
        Status::SeeOther,
        "{}",
        location(&response)
    );
    let response = get(&client, &location(&response), &manager.cookie).await;
    let html = response.into_string().await.unwrap();
    assert!(html.contains("@fallback.rsky.com"), "{html}");

    // a new default handle is recorded and the page follows the DID
    let response = post(
        &client,
        &format!("/account/u/{TEST_DID}/manage/handle"),
        &manager.cookie,
        &[
            ("csrf", &manager.csrf),
            ("mode", "default"),
            ("handle", "Renamed"),
            ("domain", ".rsky.com"),
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
        format!("/account/u/{TEST_DID}/manage?notice=handle")
    );
    let response = get(&client, &location(&response), &manager.cookie).await;
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Your username has been updated."), "{html}");
    assert!(html.contains("@renamed.rsky.com"));
    let manage = "/account/u/renamed.rsky.com/manage".to_string();

    // a custom domain this server cannot verify is refused with the reason
    let response = post(
        &client,
        &manage,
        &manager.cookie,
        &[
            ("csrf", &manager.csrf),
            ("mode", "custom"),
            ("domain", "nobody.invalid"),
        ],
    )
    .await;
    assert_eq!(response.status(), Status::NotFound);
    let response = post(
        &client,
        &format!("{manage}/handle"),
        &manager.cookie,
        &[
            ("csrf", &manager.csrf),
            ("mode", "custom"),
            ("domain", "nobody.invalid"),
        ],
    )
    .await;
    assert_eq!(response.status(), Status::Ok);
    let html = response.into_string().await.unwrap();
    assert!(html.contains("class=\"error\">"), "{html}");
    assert!(html.contains("_atproto.nobody.invalid"));
    assert!(html.contains("Email these instructions"));

    // the password change: a code, the current password, and the new one
    let response = get(&client, &format!("{manage}/password"), &manager.cookie).await;
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Change your password"), "{html}");
    let response = post(
        &client,
        &format!("{manage}/password/request"),
        &manager.cookie,
        &[("csrf", &manager.csrf)],
    )
    .await;
    assert_eq!(response.status(), Status::Ok);
    let html = response.into_string().await.unwrap();
    assert!(html.contains("name=\"current_password\""), "{html}");
    let code = mailed_token(dir.path(), "reset_password");
    let attempt = |code: &str, current: &str, new: &str| {
        let pairs = [
            ("csrf", manager.csrf.as_str()),
            ("code", code),
            ("current_password", current),
            ("password", new),
        ];
        let body = form_encode(&pairs);
        client
            .post(format!("{manage}/password"))
            .header(ContentType::Form)
            .cookie(("device-id", manager.cookie.clone()))
            .body(body)
            .dispatch()
    };
    let html = attempt(&code, "password", "short")
        .await
        .into_string()
        .await
        .unwrap();
    assert!(html.contains("Invalid password length."), "{html}");
    let html = attempt(&code, "wrong", "new-password-1")
        .await
        .into_string()
        .await
        .unwrap();
    assert!(html.contains("Invalid password"), "{html}");
    let html = attempt("WRONG", "password", "new-password-1")
        .await
        .into_string()
        .await
        .unwrap();
    assert!(html.contains("Token is invalid"), "{html}");
    let response = attempt(&code, "password", "new-password-1").await;
    assert_eq!(response.status(), Status::SeeOther);
    assert_eq!(location(&response), format!("{manage}?notice=password"));

    // the new password signs in; the old one does not
    let response = client
        .post("/xrpc/com.atproto.server.createSession")
        .header(ContentType::JSON)
        .body(r#"{"identifier":"renamed.rsky.com","password":"password"}"#)
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Unauthorized);
    let response = client
        .post("/xrpc/com.atproto.server.createSession")
        .header(ContentType::JSON)
        .body(r#"{"identifier":"renamed.rsky.com","password":"new-password-1"}"#)
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    mark_email_confirmed(dir.path());
}

#[tokio::test]
async fn password_reset_from_the_sign_in_page() {
    let (dir, client) = get_oauth_client().await;
    create_active_account(&client).await;

    let response = client.get("/.well-known/change-password").dispatch().await;
    assert_eq!(response.status(), Status::Found);
    assert_eq!(location(&response), "/account/reset-password");

    let response = client.get("/account/sign-in").dispatch().await;
    let html = response.into_string().await.unwrap();
    assert!(html.contains("href=\"/account/reset-password\""), "{html}");

    let response = client
        .get("/account/reset-password?email=foo%40example.com")
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let cookie = response
        .cookies()
        .get("device-id")
        .unwrap()
        .value()
        .to_string();
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Forgot Password"), "{html}");
    assert!(html.contains("value=\"foo@example.com\""));
    let csrf = extract_csrf(&html);

    // an unknown address gets the same answer as a known one
    let response = post(
        &client,
        "/account/reset-password/request",
        &cookie,
        &[("csrf", &csrf), ("email", "nobody@example.com")],
    )
    .await;
    assert_eq!(response.status(), Status::Ok);
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Reset Password"), "{html}");
    let response = post(
        &client,
        "/account/reset-password/request",
        &cookie,
        &[("csrf", &csrf), ("email", "foo@example.com")],
    )
    .await;
    assert_eq!(response.status(), Status::Ok);
    let html = response.into_string().await.unwrap();
    assert!(
        html.contains("name=\"username\" value=\"foo@example.com\""),
        "{html}"
    );
    let code = mailed_token(dir.path(), "reset_password");

    let response = post(
        &client,
        "/account/reset-password/confirm",
        &cookie,
        &[
            ("csrf", &csrf),
            ("code", "WRONG"),
            ("password", "another-password"),
            ("username", "foo@example.com"),
        ],
    )
    .await;
    assert_eq!(response.status(), Status::Ok);
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Token is invalid"), "{html}");
    let response = post(
        &client,
        "/account/reset-password/confirm",
        &cookie,
        &[("csrf", &csrf), ("code", &code), ("password", "short")],
    )
    .await;
    assert!(response
        .into_string()
        .await
        .unwrap()
        .contains("Invalid password length."));
    let response = post(
        &client,
        "/account/reset-password/confirm",
        &cookie,
        &[
            ("csrf", "stale"),
            ("code", &code),
            ("password", "another-password"),
        ],
    )
    .await;
    assert!(response
        .into_string()
        .await
        .unwrap()
        .contains("Your session changed in another tab"));
    let response = post(
        &client,
        "/account/reset-password/confirm",
        &cookie,
        &[
            ("csrf", &csrf),
            ("code", &code),
            ("password", "another-password"),
        ],
    )
    .await;
    assert_eq!(response.status(), Status::Ok);
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Password Updated"), "{html}");
    assert!(html.contains("href=\"/account/sign-in\">Okay</a>"));

    let response = client
        .post("/xrpc/com.atproto.server.createSession")
        .header(ContentType::JSON)
        .body(r#"{"identifier":"foo.rsky.com","password":"another-password"}"#)
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);

    // the other views are reachable directly
    let response = client
        .get("/account/reset-password?view=confirm")
        .dispatch()
        .await;
    assert!(response.into_string().await.unwrap().contains("Reset code"));
    let response = client
        .get("/account/reset-password?view=updated")
        .dispatch()
        .await;
    assert!(response
        .into_string()
        .await
        .unwrap()
        .contains("Password Updated"));
}
