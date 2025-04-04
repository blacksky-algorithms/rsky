use crate::oauth::SharedOAuthProvider;
use rocket::http::Status;
use rocket::request::FromRequest;
use rocket::{get, Request, State};
use rsky_oauth::oauth_provider::device::device_manager::DeviceManager;
use rsky_oauth::oauth_provider::lib::http::request::{
    validate_csrf_token, validate_fetch_site, validate_referer,
};
use rsky_oauth::oauth_provider::request::request_uri::RequestUri;
use rsky_oauth::oauth_provider::AuthorizeReject;
use rsky_oauth::oauth_types::OAuthClientId;

// Though this is a "no-cors" request, meaning that the browser will allow
// any cross-origin request, with credentials, to be sent, the handler will
// 1) validate the request origin,
// 2) validate the CSRF token,
// 3) validate the referer,
// 4) validate the sec-fetch-site header,
// 4) validate the sec-fetch-mode header,
// 5) validate the sec-fetch-dest header (see navigationHandler).
// And will error if any of these checks fail.
#[get("/oauth/authorize/reject")]
pub async fn oauth_authorize_reject(
    shared_oauth_provider: &State<SharedOAuthProvider>,
    authorize_reject: AuthorizeReject,
) {
    unimplemented!()
}
