//! The authorization pages when sign-up is offered: the welcome view comes
//! first for a device without sessions, and every screen links to the
//! sign-up page. A separate binary, since the offer is read from the
//! environment when the server starts.

use rocket::http::{ContentType, Status};

mod common;
use common::oauth::*;

const SIGNUP_URL: &str = "https://signup.test/start";

#[tokio::test]
async fn welcome_view_leads_to_sign_up_or_sign_in() {
    std::env::set_var("PDS_OAUTH_SIGNUP_URL", SIGNUP_URL);
    let (_dir, client) = get_oauth_client().await;
    create_active_account(&client).await;
    let key = dpop_key();
    let (request_uri, nonce) = run_par(&client, &key).await;

    let response = client
        .get(authorize_path(LOOPBACK_CLIENT_ID, &request_uri))
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
    assert!(html.contains(">Welcome</h1>"), "{html}");
    assert!(html.contains("Please authenticate to continue"));
    assert!(html.contains(&format!("href=\"{SIGNUP_URL}\"")));
    assert!(html.contains("&amp;view=sign-in\">Sign in</a>"));
    assert!(html.contains(">Cancel</button>"));
    let csrf = extract_csrf(&html);

    // "Sign in" shows the form, whose Back returns to the welcome view
    let response = client
        .get(format!(
            "{}&view=sign-in",
            authorize_path(LOOPBACK_CLIENT_ID, &request_uri)
        ))
        .cookie(("device-id", cookie.clone()))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let html = response.into_string().await.unwrap();
    assert!(html.contains("name=\"password\""), "{html}");
    assert!(html.contains("Create a new account"));
    assert!(html.contains("&amp;view=welcome\">Back</a>"));

    // once a session exists the picker comes first and offers sign-up
    let mut session = AuthorizeSession { cookie, csrf };
    let code = sign_in_and_accept(&client, &request_uri, &mut session).await;
    exchange_code(&client, &key, &code, &nonce).await;
    let (request_uri, _) = run_par(&client, &key).await;
    let response = client
        .get(authorize_path(LOOPBACK_CLIENT_ID, &request_uri))
        .cookie(("device-id", session.cookie.clone()))
        .dispatch()
        .await;
    let html = response.into_string().await.unwrap();
    assert!(html.contains("Sign in as..."), "{html}");
    assert!(html.contains(">Sign up</a>"));
    assert!(html.contains("&amp;view=welcome\">Back</a>"));

    // the welcome view can still be asked for, and Cancel rejects
    let html = get_authorize_html(
        &client,
        format!(
            "{}&view=welcome",
            authorize_path(LOOPBACK_CLIENT_ID, &request_uri)
        ),
        &session,
    )
    .await;
    assert!(html.contains(">Welcome</h1>"), "{html}");
    let response = client
        .post("/oauth/authorize/reject")
        .header(ContentType::Form)
        .cookie(("device-id", session.cookie.clone()))
        .body(form_encode(&[
            ("request_uri", &request_uri),
            ("client_id", LOOPBACK_CLIENT_ID),
            ("csrf", &extract_csrf(&html)),
        ]))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::SeeOther);
    assert!(response
        .headers()
        .get_one("Location")
        .unwrap()
        .contains("error=access_denied"));
}
