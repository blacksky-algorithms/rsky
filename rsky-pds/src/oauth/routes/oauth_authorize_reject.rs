use crate::oauth::SharedOAuthProvider;
use rocket::http::Status;
use rocket::request::FromRequest;
use rocket::{get, Request, State};
use rsky_oauth::oauth_provider::device::device_manager::DeviceManager;
use rsky_oauth::oauth_provider::lib::http::request::{
    validate_csrf_token, validate_fetch_site, validate_referer,
};
use rsky_oauth::oauth_provider::request::request_uri::RequestUri;
use rsky_oauth::oauth_types::OAuthClientId;

pub struct AuthorizeReject {
    pub request_uri: RequestUri,
    pub client_id: OAuthClientId,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for AuthorizeReject {
    type Error = ();

    async fn from_request(req: &'r Request<'_>) -> rocket::request::Outcome<Self, Self::Error> {
        match validate_fetch_site(req, vec!["same-origin"]) {
            Ok(_) => {}
            Err(e) => return rocket::request::Outcome::Error((Status::new(400), ())),
        }

        let query = req.query_fields();
        let csrf_token = "".to_string();
        let request_uri = RequestUri::new("").unwrap();
        let client_id = OAuthClientId::new("").unwrap();

        validate_referer(req);

        validate_csrf_token(req, csrf_token.as_str(), "", true);

        let device_manager = req.guard::<&State<DeviceManager>>().await.unwrap();
        rocket::request::Outcome::Success(Self {
            request_uri,
            client_id,
        })
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
#[get("/oauth/authorize/reject")]
pub async fn oauth_authorize_reject(
    shared_oauth_provider: &State<SharedOAuthProvider>,
    authorize_reject: AuthorizeReject,
) {
    unimplemented!()
}
