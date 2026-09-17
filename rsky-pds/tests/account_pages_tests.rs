//! The account manager pages: entry, sign-in and picker, home, devices,
//! connected apps, sign-out, and what a session may not reach.

use rocket::http::{ContentType, Header, Status};
use rocket::local::asynchronous::{Client, LocalResponse};
use serde_json::Value;

mod common;
use common::oauth::*;

const TEST_DID: &str = "did:plc:khvyd3oiw46vif5gm7hijslk";

async fn get<'c>(client: &'c Client, path: &str, cookie: Option<&str>) -> LocalResponse<'c> {
    let mut request = client.get(path.to_string());
    if let Some(cookie) = cookie {
        request = request.cookie(("device-id", cookie.to_string()));
    }
    request.dispatch().await
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

fn cookie_of(response: &LocalResponse<'_>) -> String {
    response
        .cookies()
        .get("device-id")
        .expect("device cookie")
        .value()
        .to_string()
}

fn location(response: &LocalResponse<'_>) -> String {
    response
        .headers()
        .get_one("Location")
        .expect("redirect")
        .to_string()
}

struct Manager {
    cookie: String,
    csrf: String,
}

/// Signs the test account in on a fresh device and lands on its home.
async fn sign_in(client: &Client) -> Manager {
    let response = get(client, "/account/sign-in", None).await;
    assert_eq!(response.status(), Status::Ok);
    let cookie = cookie_of(&response);
    let html = response.into_string().await.unwrap();
    let csrf = extract_csrf(&html);
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
    assert_eq!(location(&response), "/account/u/foo.rsky.com");
    let rotated = cookie_of(&response);
    assert_ne!(rotated, cookie);
    let response = get(client, "/account/u/foo.rsky.com", Some(&rotated)).await;
    assert_eq!(response.status(), Status::Ok);
    let html = response.into_string().await.unwrap();
    Manager {
        cookie: rotated,
        csrf: extract_csrf(&html),
    }
}

#[tokio::test]
async fn entry_sign_in_and_home() {
    let (_dir, client) = get_oauth_client().await;
    create_active_account(&client).await;

    // a device without sessions gets the welcome view, since this server
    // creates accounts itself
    let response = get(&client, "/account", None).await;
    assert_eq!(response.status(), Status::Ok);
    let html = response.into_string().await.unwrap();
    assert!(html.contains(">Welcome</h1>"), "{html}");
    assert!(html.contains("href=\"/account/sign-up\">Create a new account</a>"));
    assert!(!html.contains(">Cancel</button>"));
    let response = get(&client, "/account/sign-in", None).await;
    let cookie = cookie_of(&response);
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Enter your username and password"), "{html}");
    assert!(html.contains("action=\"/account/sign-in\""));
    assert!(!html.contains("name=\"remember\""));
    assert!(!html.contains("name=\"client_id\""));
    let csrf = extract_csrf(&html);

    // wrong password: the form again
    let response = post(
        &client,
        "/account/sign-in",
        &cookie,
        &[
            ("csrf", &csrf),
            ("identifier", "foo@example.com"),
            ("password", "nope"),
        ],
    )
    .await;
    assert_eq!(response.status(), Status::Ok);
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Invalid identifier or password"), "{html}");
    assert!(html.contains("value=\"foo@example.com\""));

    // a stale token: the form again with the current token
    let response = post(
        &client,
        "/account/sign-in",
        &cookie,
        &[
            ("csrf", "stale"),
            ("identifier", "foo@example.com"),
            ("password", "password"),
        ],
    )
    .await;
    assert_eq!(response.status(), Status::Ok);
    let html = response.into_string().await.unwrap();
    assert!(
        html.contains("Your session changed in another tab"),
        "{html}"
    );
    assert_eq!(extract_csrf(&html), csrf);

    // from another site: refused
    let response = client
        .post("/account/sign-in")
        .header(ContentType::Form)
        .header(Header::new("Origin", "https://evil.test"))
        .cookie(("device-id", cookie.clone()))
        .body(form_encode(&[
            ("csrf", &csrf),
            ("identifier", "foo@example.com"),
            ("password", "password"),
        ]))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Forbidden);

    let manager = sign_in(&client).await;
    let response = get(&client, "/account/u/foo.rsky.com", Some(&manager.cookie)).await;
    let html = response.into_string().await.unwrap();
    assert!(html.contains("<title>My Atmosphere Account</title>"));
    assert!(html.contains("@foo.rsky.com"));
    assert!(html.contains("Manage your active sessions"));
    assert!(html.contains("href=\"/account/u/foo.rsky.com/about\">What does this mean?</a>"));
    assert!(html.contains("<span>Sign out</span>"));

    // the entry now goes straight to the one account; its DID works too
    let response = get(&client, "/account", Some(&manager.cookie)).await;
    assert_eq!(response.status(), Status::SeeOther);
    assert_eq!(location(&response), "/account/u/foo.rsky.com");
    let response = get(
        &client,
        &format!("/account/u/{TEST_DID}"),
        Some(&manager.cookie),
    )
    .await;
    assert_eq!(response.status(), Status::Ok);
    let response = get(&client, "/account/u/@Foo.rsky.com", Some(&manager.cookie)).await;
    assert_eq!(response.status(), Status::Ok);

    // the picker lists the account; picking it goes home
    let response = get(&client, "/account/sign-in", Some(&manager.cookie)).await;
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Sign in as..."), "{html}");
    assert!(html.contains("action=\"/account/select\""));
    assert!(html.contains("href=\"/account/sign-in?view=sign-in\""));
    let response = post(
        &client,
        "/account/select",
        &manager.cookie,
        &[("csrf", &manager.csrf), ("did", TEST_DID)],
    )
    .await;
    assert_eq!(response.status(), Status::SeeOther);
    assert_eq!(location(&response), "/account/u/foo.rsky.com");
    let response = post(
        &client,
        "/account/select",
        &manager.cookie,
        &[("csrf", &manager.csrf), ("did", "did:plc:someoneelse")],
    )
    .await;
    assert_eq!(response.status(), Status::BadRequest);
    let response = post(
        &client,
        "/account/select",
        &manager.cookie,
        &[("csrf", "stale"), ("did", TEST_DID)],
    )
    .await;
    assert_eq!(response.status(), Status::Ok);
    assert!(response
        .into_string()
        .await
        .unwrap()
        .contains("Your session changed in another tab"));

    // an account this device does not hold is sent to sign in for it
    let response = get(&client, "/account/u/someone.else", Some(&manager.cookie)).await;
    assert_eq!(response.status(), Status::SeeOther);
    assert_eq!(
        location(&response),
        "/account/sign-in?login_hint=someone.else"
    );
    let response = get(
        &client,
        "/account/sign-in?login_hint=someone.else",
        Some(&manager.cookie),
    )
    .await;
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Enter your password"), "{html}");
    assert!(html.contains("value=\"someone.else\""));

    // the gate's redirects render the code field and the error
    let response = get(
        &client,
        "/account/sign-in?otp_hint=f***%40example.com&otp_error=true",
        Some(&manager.cookie),
    )
    .await;
    let html = response.into_string().await.unwrap();
    assert!(html.contains("name=\"email_otp\""), "{html}");
    assert!(html.contains("The sign-in code was not accepted"));
    let response = get(
        &client,
        "/account/sign-in?auth_error=true",
        Some(&manager.cookie),
    )
    .await;
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Invalid identifier or password"), "{html}");

    // unknown pages under /account get the branded not-found page
    let response = get(&client, "/account/nothing/here", Some(&manager.cookie)).await;
    assert_eq!(response.status(), Status::NotFound);
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Page not found"), "{html}");
}

#[tokio::test]
async fn devices_page_and_device_sign_out() {
    let (_dir, client) = get_oauth_client().await;
    create_active_account(&client).await;
    let phone = sign_in(&client).await;
    let laptop = sign_in(&client).await;
    let phone_device = phone.cookie.split_once('.').unwrap().0.to_string();
    let laptop_device = laptop.cookie.split_once('.').unwrap().0.to_string();

    let response = get(
        &client,
        "/account/u/foo.rsky.com/devices",
        Some(&laptop.cookie),
    )
    .await;
    assert_eq!(response.status(), Status::Ok);
    let html = response.into_string().await.unwrap();
    assert!(html.contains("<h2>Devices</h2>"), "{html}");
    assert!(html.contains("This device"));
    assert!(html.contains("Unknown user agent"));
    assert!(html.contains("just now"));
    assert!(html.contains(&format!("name=\"device_id\" value=\"{phone_device}\"")));
    assert!(html.contains("title=\"Cannot remove current device\" disabled"));

    // the filter narrows by name and address
    let response = get(
        &client,
        "/account/u/foo.rsky.com/devices?q=nothing-matches",
        Some(&laptop.cookie),
    )
    .await;
    assert!(response.into_string().await.unwrap().contains("No matches"));

    // the current device cannot be signed out from the list
    let response = post(
        &client,
        "/account/u/foo.rsky.com/devices/sign-out",
        &laptop.cookie,
        &[("csrf", &laptop.csrf), ("device_id", &laptop_device)],
    )
    .await;
    assert_eq!(response.status(), Status::BadRequest);
    assert!(response
        .into_string()
        .await
        .unwrap()
        .contains("Cannot remove current device"));

    // the other one can
    let response = post(
        &client,
        "/account/u/foo.rsky.com/devices/sign-out",
        &laptop.cookie,
        &[("csrf", &laptop.csrf), ("device_id", &phone_device)],
    )
    .await;
    assert_eq!(response.status(), Status::SeeOther);
    assert_eq!(location(&response), "/account/u/foo.rsky.com/devices");
    let response = get(&client, "/account/u/foo.rsky.com", Some(&phone.cookie)).await;
    assert_eq!(response.status(), Status::SeeOther);
    assert_eq!(
        location(&response),
        "/account/sign-in?login_hint=foo.rsky.com"
    );

    // a stale csrf on a destructive form is refused with a way back
    let response = post(
        &client,
        "/account/u/foo.rsky.com/devices/sign-out",
        &laptop.cookie,
        &[("csrf", "stale"), ("device_id", &phone_device)],
    )
    .await;
    assert_eq!(response.status(), Status::BadRequest);
    let html = response.into_string().await.unwrap();
    assert!(
        html.contains("Your session changed in another tab"),
        "{html}"
    );
    assert!(html.contains("href=\"/account/u/foo.rsky.com/devices\">Back</a>"));

    // signing out of this device removes the session and rotates the cookie
    let response = post(
        &client,
        "/account/u/foo.rsky.com/sign-out",
        &laptop.cookie,
        &[("csrf", &laptop.csrf)],
    )
    .await;
    assert_eq!(response.status(), Status::SeeOther);
    assert_eq!(location(&response), "/account");
    let rotated = cookie_of(&response);
    assert_ne!(rotated, laptop.cookie);
    assert!(rotated.starts_with(&format!("{laptop_device}.")));
    let response = get(&client, "/account", Some(&rotated)).await;
    assert!(response
        .into_string()
        .await
        .unwrap()
        .contains(">Welcome</h1>"));
    let response = get(&client, "/account/u/foo.rsky.com/devices", Some(&rotated)).await;
    assert_eq!(response.status(), Status::SeeOther);
    assert!(location(&response).starts_with("/account/sign-in?login_hint="));

    // signing out when nothing is signed in is a no-op
    let html = get(&client, "/account/sign-in", Some(&rotated))
        .await
        .into_string()
        .await
        .unwrap();
    let csrf = extract_csrf(&html);
    let response = post(
        &client,
        "/account/u/foo.rsky.com/sign-out",
        &rotated,
        &[("csrf", &csrf)],
    )
    .await;
    assert_eq!(response.status(), Status::SeeOther);
    assert_eq!(location(&response), "/account");
}

#[tokio::test]
async fn apps_page_details_and_revocation() {
    let (_dir, client) = get_oauth_client().await;
    create_active_account(&client).await;
    let key = dpop_key();
    let (request_uri, nonce) = run_par(&client, &key).await;
    let mut session = open_authorize_page(&client, &request_uri).await;
    let code = sign_in_and_accept(&client, &request_uri, &mut session).await;
    let tokens = exchange_code(&client, &key, &code, &nonce).await;
    let refresh_token = tokens["refresh_token"].as_str().unwrap().to_string();

    // the OAuth sign-in also signed the device in to the account manager
    let response = get(&client, "/account/u/foo.rsky.com", Some(&session.cookie)).await;
    assert_eq!(response.status(), Status::Ok);
    let csrf = extract_csrf(&response.into_string().await.unwrap());

    let response = get(
        &client,
        "/account/u/foo.rsky.com/apps",
        Some(&session.cookie),
    )
    .await;
    assert_eq!(response.status(), Status::Ok);
    let html = response.into_string().await.unwrap();
    assert!(html.contains("<h2>Apps</h2>"), "{html}");
    assert!(html.contains("A local app"));
    assert!(html.contains("<code>loopback</code>"));
    assert!(html.contains("Why is this time so recent?"));
    let marker = "/account/u/foo.rsky.com/apps/";
    let start = html.find(marker).unwrap() + marker.len();
    let token_id = html[start..start + html[start..].find('"').unwrap()].to_string();
    assert!(token_id.starts_with("tok-"), "{token_id}");

    let response = get(
        &client,
        "/account/u/foo.rsky.com/apps?q=nothing-matches",
        Some(&session.cookie),
    )
    .await;
    assert!(response.into_string().await.unwrap().contains("No matches"));

    let response = get(
        &client,
        &format!("/account/u/foo.rsky.com/apps/{token_id}"),
        Some(&session.cookie),
    )
    .await;
    assert_eq!(response.status(), Status::Ok);
    let html = response.into_string().await.unwrap();
    assert!(
        html.contains("This app has access to your account with the following permissions:"),
        "{html}"
    );
    assert!(html.contains(">Revoke access</button>"));
    assert!(html.contains(&format!("name=\"token_id\" value=\"{token_id}\"")));

    let response = get(
        &client,
        "/account/u/foo.rsky.com/apps/tok-unknown",
        Some(&session.cookie),
    )
    .await;
    assert_eq!(response.status(), Status::NotFound);

    let response = post(
        &client,
        "/account/u/foo.rsky.com/apps/revoke",
        &session.cookie,
        &[("csrf", &csrf), ("token_id", &token_id)],
    )
    .await;
    assert_eq!(response.status(), Status::SeeOther);
    assert_eq!(location(&response), "/account/u/foo.rsky.com/apps");
    let response = get(
        &client,
        "/account/u/foo.rsky.com/apps",
        Some(&session.cookie),
    )
    .await;
    assert!(response
        .into_string()
        .await
        .unwrap()
        .contains("It appears that you haven't used this account to sign in to any apps yet."));

    // the revoked session can no longer refresh
    let token_htu = format!("{}/oauth/token", public_url(&client));
    let response = client
        .post("/oauth/token")
        .header(ContentType::Form)
        .header(Header::new(
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
    let body: Value = serde_json::from_str(&response.into_string().await.unwrap()).unwrap();
    assert_eq!(body["error"], "invalid_grant");

    // revoking something that is not this account's is refused
    let response = post(
        &client,
        "/account/u/foo.rsky.com/apps/revoke",
        &session.cookie,
        &[("csrf", &csrf), ("token_id", "tok-unknown")],
    )
    .await;
    assert_eq!(response.status(), Status::BadRequest);
}

#[tokio::test]
async fn about_page_stale_sessions_and_sign_out_all() {
    let (_dir, client) = get_oauth_client().await;
    create_active_account(&client).await;
    let manager = sign_in(&client).await;

    let response = get(
        &client,
        "/account/u/foo.rsky.com/about",
        Some(&manager.cookie),
    )
    .await;
    assert_eq!(response.status(), Status::Ok);
    let html = response.into_string().await.unwrap();
    assert!(html.contains("What is an Atmosphere account?"), "{html}");
    assert!(html.contains("<h2>About</h2>"));
    assert!(html.contains("@foo.rsky.com"));
    assert!(!html.contains("Bluesky"));

    // a stale authentication must sign in again, and the picker says so
    let device_id = manager.cookie.split_once('.').unwrap().0.to_string();
    let shared = client
        .rocket()
        .state::<rsky_pds::oauth::SharedOAuthProvider>()
        .unwrap();
    shared
        .provider
        .store()
        .upsert_device_account(
            &device_id,
            TEST_DID,
            now_secs() - rsky_oauth::store::AUTHENTICATION_MAX_AGE - 60,
        )
        .await
        .unwrap();
    let response = get(&client, "/account/u/foo.rsky.com", Some(&manager.cookie)).await;
    assert_eq!(response.status(), Status::SeeOther);
    assert_eq!(
        location(&response),
        "/account/sign-in?login_hint=foo.rsky.com"
    );
    // a stale session still exists, so the picker is offered, not the welcome
    let response = get(&client, "/account", Some(&manager.cookie)).await;
    assert_eq!(location(&response), "/account/sign-in");
    let response = get(&client, "/account/sign-in", Some(&manager.cookie)).await;
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Login required"), "{html}");
    let csrf = extract_csrf(&html);
    let response = post(
        &client,
        "/account/select",
        &manager.cookie,
        &[("csrf", &csrf), ("did", TEST_DID)],
    )
    .await;
    assert_eq!(response.status(), Status::Ok);
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Confirm your password to continue"), "{html}");
    assert!(html.contains("value=\"foo.rsky.com\""));

    // signing in again refreshes the authentication
    let response = post(
        &client,
        "/account/sign-in",
        &manager.cookie,
        &[
            ("csrf", &csrf),
            ("identifier", "foo.rsky.com"),
            ("password", "password"),
        ],
    )
    .await;
    assert_eq!(response.status(), Status::SeeOther);
    let cookie = cookie_of(&response);
    let response = get(&client, "/account/u/foo.rsky.com", Some(&cookie)).await;
    assert_eq!(response.status(), Status::Ok);
    let csrf = extract_csrf(&response.into_string().await.unwrap());

    // sign out of everything on this device
    let response = post(
        &client,
        "/account/sign-out-all",
        &cookie,
        &[("csrf", &csrf)],
    )
    .await;
    assert_eq!(response.status(), Status::SeeOther);
    assert_eq!(location(&response), "/account");
    let rotated = cookie_of(&response);
    assert_ne!(rotated, cookie);
    let response = get(&client, "/account/u/foo.rsky.com", Some(&rotated)).await;
    assert_eq!(response.status(), Status::SeeOther);

    // a deactivated account reaches its home but not the other pages
    let account_manager = client
        .rocket()
        .state::<rsky_pds::account_manager::AccountManager>()
        .unwrap();
    let manager = sign_in(&client).await;
    account_manager
        .deactivate_account(TEST_DID, None)
        .await
        .unwrap();
    let response = get(&client, "/account/u/foo.rsky.com", Some(&manager.cookie)).await;
    assert_eq!(response.status(), Status::Ok);
    let response = get(
        &client,
        "/account/u/foo.rsky.com/apps",
        Some(&manager.cookie),
    )
    .await;
    assert_eq!(response.status(), Status::SeeOther);
    assert_eq!(location(&response), "/account/u/foo.rsky.com");
}
