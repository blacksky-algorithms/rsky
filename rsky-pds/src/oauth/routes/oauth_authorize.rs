use crate::oauth::SharedOAuthProvider;
use rocket::request::FromRequest;
use rocket::{get, Request, State};

pub struct OAuthAuthorizeRequestBody(Option<String>);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for OAuthAuthorizeRequestBody {
    type Error = ();

    async fn from_request(req: &'r Request<'_>) -> rocket::request::Outcome<Self, Self::Error> {
        match req.headers().get_one("dpop") {
            None => rocket::request::Outcome::Success(OAuthAuthorizeRequestBody(None)),
            Some(res) => {
                rocket::request::Outcome::Success(OAuthAuthorizeRequestBody(Some(res.to_string())))
            }
        }
    }
}

#[get("/oauth/authorize")]
pub async fn oauth_authorize(
    shared_oauth_provider: &State<SharedOAuthProvider>,
    body: OAuthAuthorizeRequestBody,
) {
    unimplemented!()
}
