use crate::oauth::SharedOAuthProvider;
use rocket::get;
use rocket::http::Status;
use rocket::request::FromRequest;
use rocket::{Request, State};
use rsky_oauth::oauth_provider::lib::http::request::validate_fetch_site;

pub struct AuthorizeAccept {
    // pub device_id: DeviceId,
    // pub request_uri: RequestUri,
    // pub client_id:  OAuthClientId,
    // pub account_sub: Sub
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for AuthorizeAccept {
    type Error = ();

    async fn from_request(req: &'r Request<'_>) -> rocket::request::Outcome<Self, Self::Error> {
        match validate_fetch_site(req, vec!["same-origin"]) {
            Ok(_) => rocket::request::Outcome::Success(Self {}),
            Err(e) => rocket::request::Outcome::Error((Status::new(400), ())),
        }
        //
        // let request_uri = req.query_value("request_uri");
        // let client_id = req.query_value("client_id");
        // let sub = req.query_value("account_sub");
        // // let csrf_cookie =
    }
}

// Though this is a "no-cors" request, meaning that the browser will allow
// any cross-origin request, with credentials, to be sent, the handler will
// 1) validate the request origin,
// 2) validate the CSRF token,
// 3) validate the referer,
// 4) validate the sec-fetch-site header,
// 4) validate the sec-fetch-mode header,
// 5) validate the sec-fetch-dest header (see navigationHandler).
// And will error if any of these checks fail.
#[get("/oauth/authorize/accept")]
pub async fn oauth_authorize_accept(
    shared_oauth_provider: &State<SharedOAuthProvider>,
    authorize_accept: AuthorizeAccept,
) {
    unimplemented!()
}
