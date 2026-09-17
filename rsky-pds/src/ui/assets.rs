//! The stylesheet behind every page, served under a content-addressed name
//! so a new build can never be paired with a cached old stylesheet.

use rocket::http::{ContentType, Header, Status};
use rocket::request::Request;
use rocket::response::{Responder, Response};
use sha2::{Digest, Sha256};
use std::io::Cursor;
use std::sync::LazyLock;

pub const UI_CSS: &str = include_str!("../../assets/ui.css");

pub static UI_CSS_HASH: LazyLock<String> = LazyLock::new(|| {
    let digest = Sha256::digest(UI_CSS.as_bytes());
    hex::encode(&digest[..8])
});

pub fn stylesheet_name() -> String {
    format!("ui-{}.css", *UI_CSS_HASH)
}

pub fn stylesheet_href() -> String {
    format!("/oauth/assets/{}", stylesheet_name())
}

pub struct CssAsset;

impl<'r> Responder<'r, 'static> for CssAsset {
    fn respond_to(self, _: &'r Request<'_>) -> rocket::response::Result<'static> {
        Response::build()
            .status(Status::Ok)
            .header(ContentType::new("text", "css").with_params(("charset", "utf-8")))
            .header(Header::new(
                "Cache-Control",
                "public, max-age=31536000, immutable",
            ))
            .header(Header::new("ETag", format!("\"{}\"", *UI_CSS_HASH)))
            .sized_body(UI_CSS.len(), Cursor::new(UI_CSS))
            .ok()
    }
}

/// Only the current build's name resolves; anything else is a 404 so there
/// is exactly one valid stylesheet URL per binary.
#[rocket::get("/oauth/assets/<file>")]
pub fn ui_asset(file: &str) -> Option<CssAsset> {
    (file == stylesheet_name()).then_some(CssAsset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rocket::local::blocking::Client;
    use rocket::routes;

    #[test]
    fn hash_is_stable_and_named_consistently() {
        assert_eq!(UI_CSS_HASH.len(), 16);
        assert!(UI_CSS_HASH.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(
            stylesheet_href(),
            format!("/oauth/assets/ui-{}.css", *UI_CSS_HASH)
        );
    }

    #[test]
    fn serves_only_the_current_stylesheet() {
        let client =
            Client::tracked(rocket::build().mount("/", routes![ui_asset])).expect("rocket");
        let response = client.get(stylesheet_href()).dispatch();
        assert_eq!(response.status(), Status::Ok);
        assert_eq!(
            response.headers().get_one("Cache-Control"),
            Some("public, max-age=31536000, immutable")
        );
        assert_eq!(
            response.headers().get_one("ETag").unwrap(),
            format!("\"{}\"", *UI_CSS_HASH)
        );
        assert!(response
            .headers()
            .get_one("Content-Type")
            .unwrap()
            .starts_with("text/css"));
        assert!(response
            .into_string()
            .unwrap()
            .contains("--branding-color-primary"));

        let response = client
            .get("/oauth/assets/ui-0000000000000000.css")
            .dispatch();
        assert_eq!(response.status(), Status::NotFound);
        let response = client.get("/oauth/assets/other.css").dispatch();
        assert_eq!(response.status(), Status::NotFound);
    }
}
