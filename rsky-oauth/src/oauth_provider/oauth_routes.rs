use rocket::{get, post, routes, Route};

pub fn get_routes() -> Vec<Route> {
    routes![
        oauth_well_known,
        oauth_jwks,
        oauth_par,
        oauth_token,
        post_oauth_revoke,
        oauth_introspect,
        oauth_authorize,
        oauth_authorize_signin,
        oauth_authorize_accept,
        oauth_authorize_reject
    ]
}

#[get("/.well-known/oauth-authorization-server")]
pub async fn oauth_well_known() {
    unimplemented!()
}

#[get("/oauth/jwks")]
pub async fn oauth_jwks() {
    unimplemented!()
}

#[post("/oauth/par")]
pub async fn oauth_par() {
    unimplemented!()
}

#[post("/oauth/token")]
pub async fn oauth_token() {
    unimplemented!()
}

#[post("/oauth/revoke")]
pub async fn post_oauth_revoke() {
    unimplemented!()
}

#[post("/oauth/introspect")]
pub async fn oauth_introspect() {
    unimplemented!()
}

#[get("/oauth/authorize")]
pub async fn oauth_authorize() {
    unimplemented!()
}

#[post("/oauth/authorize/sign-in")]
pub async fn oauth_authorize_signin() {
    unimplemented!()
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
pub async fn oauth_authorize_accept() {
    unimplemented!()
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
pub async fn oauth_authorize_reject() {
    unimplemented!()
}
