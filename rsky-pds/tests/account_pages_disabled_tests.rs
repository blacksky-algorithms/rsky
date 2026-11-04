//! With `PDS_ACCOUNT_UI_ENABLED=false` nothing under `/account` is served,
//! while the OAuth screens stay. A separate binary, since the flag is read
//! when the server starts.

use rocket::http::Status;

mod common;
use common::oauth::*;

#[tokio::test]
async fn account_pages_can_be_switched_off() {
    std::env::set_var("PDS_ACCOUNT_UI_ENABLED", "false");
    let (_dir, client) = get_oauth_client().await;
    create_active_account(&client).await;
    for path in ["/account", "/account/sign-in", "/account/u/foo.rsky.com"] {
        let response = client.get(path).dispatch().await;
        assert_eq!(response.status(), Status::NotFound, "{path}");
    }
    let key = dpop_key();
    let (request_uri, _) = run_par(&client, &key).await;
    open_authorize_page(&client, &request_uri).await;
}
