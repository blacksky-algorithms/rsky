//! The HTML responder for every browser page: the headers the reference
//! authorization UI sends, applied only to pages so JSON and XRPC routes
//! are untouched.

use super::shell::PageShell;
use askama::Template;
use rocket::http::{ContentType, Header, Status};
use rocket::request::Request;
use rocket::response::{Responder, Response};
use std::io::Cursor;

pub struct UiHtml {
    pub status: Status,
    pub html: String,
    pub csp: String,
    pub hsts: bool,
}

/// hCaptcha needs its own origins when a page embeds it.
pub const HCAPTCHA_ORIGINS: &str = "https://hcaptcha.com https://*.hcaptcha.com";

pub fn content_security_policy(shell: &PageShell, hcaptcha: bool) -> String {
    let mut directives = vec![
        "default-src 'none'".to_string(),
        format!("base-uri {}", shell.origin()),
        "img-src data: https:".to_string(),
        "frame-ancestors 'none'".to_string(),
    ];
    if hcaptcha {
        directives.push(format!("connect-src 'self' {HCAPTCHA_ORIGINS}"));
        directives.push(format!(
            "style-src 'self' 'sha256-{}' {HCAPTCHA_ORIGINS}",
            shell.branding_css_sha256
        ));
        directives.push(format!("script-src {HCAPTCHA_ORIGINS}"));
        directives.push(format!("frame-src {HCAPTCHA_ORIGINS}"));
    } else {
        directives.push("connect-src 'self'".to_string());
        directives.push(format!(
            "style-src 'self' 'sha256-{}'",
            shell.branding_css_sha256
        ));
    }
    if shell.hsts {
        directives.push("upgrade-insecure-requests".to_string());
    }
    directives.join("; ")
}

pub fn render_page<T: Template>(status: Status, shell: &PageShell, template: &T) -> UiHtml {
    render_page_with(status, shell, template, false)
}

pub fn render_page_with<T: Template>(
    status: Status,
    shell: &PageShell,
    template: &T,
    hcaptcha: bool,
) -> UiHtml {
    let html = template.render().unwrap_or_else(|error| {
        tracing::error!(%error, "page template failed to render");
        "<!doctype html><title>Error</title><p>Something went wrong.</p>".to_string()
    });
    UiHtml {
        status,
        html,
        csp: content_security_policy(shell, hcaptcha),
        hsts: shell.hsts,
    }
}

impl<'r> Responder<'r, 'static> for UiHtml {
    fn respond_to(self, _: &'r Request<'_>) -> rocket::response::Result<'static> {
        let mut response = Response::build();
        response
            .status(self.status)
            .header(ContentType::HTML)
            .header(Header::new("Cache-Control", "no-store"))
            .header(Header::new("Pragma", "no-cache"))
            .header(Header::new("Content-Security-Policy", self.csp))
            .header(Header::new("Referrer-Policy", "same-origin"))
            .header(Header::new("X-Frame-Options", "DENY"))
            .header(Header::new("Cross-Origin-Opener-Policy", "same-origin"))
            .header(Header::new("Cross-Origin-Resource-Policy", "same-origin"))
            .header(Header::new(
                "Permissions-Policy",
                "otp-credentials=*, document-domain=()",
            ))
            .header(Header::new("X-XSS-Protection", "0"))
            .header(Header::new("Vary", "Cookie"));
        if self.hsts {
            response.header(Header::new("Strict-Transport-Security", "max-age=63072000"));
        }
        // the device cookie moved from /oauth to the whole site; the copy a
        // browser still holds at the old path is retired page by page
        response.header_adjoin(Header::new(
            "Set-Cookie",
            crate::oauth::legacy_device_cookie_removal(),
        ));
        response
            .sized_body(self.html.len(), Cursor::new(self.html))
            .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::branding::Branding;
    use rocket::local::blocking::Client;
    use rocket::routes;

    struct Broken;

    impl std::fmt::Display for Broken {
        fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            Err(std::fmt::Error)
        }
    }

    impl Template for Broken {
        fn render_into(&self, _: &mut (impl std::fmt::Write + ?Sized)) -> askama::Result<()> {
            Err(askama::Error::Fmt(std::fmt::Error))
        }
        const EXTENSION: Option<&'static str> = Some("html");
        const SIZE_HINT: usize = 0;
        const MIME_TYPE: &'static str = "text/html";
    }

    #[test]
    fn a_template_that_fails_to_render_still_answers_a_page() {
        let page = render_page(Status::Ok, &shell("https://pds.test"), &Broken);
        assert!(page.html.contains("Something went wrong"));
        assert_eq!(page.status, Status::Ok);
    }

    #[derive(Template)]
    #[template(source = "<p>{{ text }}</p>", ext = "html")]
    struct Tiny {
        text: String,
    }

    fn shell(public_url: &str) -> PageShell {
        PageShell::new(&Branding::default(), public_url, "pds.test")
    }

    #[rocket::get("/page")]
    fn page() -> UiHtml {
        render_page(
            Status::Ok,
            &shell("https://pds.test"),
            &Tiny {
                text: "hello".into(),
            },
        )
    }

    #[rocket::get("/plain")]
    fn plain() -> UiHtml {
        render_page_with(
            Status::BadRequest,
            &shell("http://localhost:2583"),
            &Tiny { text: "x".into() },
            true,
        )
    }

    #[test]
    fn pages_carry_the_reference_headers() {
        let client =
            Client::tracked(rocket::build().mount("/", routes![page, plain])).expect("rocket");
        let response = client.get("/page").dispatch();
        assert_eq!(response.status(), Status::Ok);
        let h = response.headers();
        assert_eq!(h.get_one("Cache-Control"), Some("no-store"));
        assert!(h
            .get("Set-Cookie")
            .any(|c| c.starts_with("device-id=; Path=/oauth; Max-Age=0")));
        assert_eq!(h.get_one("Pragma"), Some("no-cache"));
        assert_eq!(h.get_one("X-Frame-Options"), Some("DENY"));
        assert_eq!(h.get_one("Referrer-Policy"), Some("same-origin"));
        assert_eq!(h.get_one("Vary"), Some("Cookie"));
        assert_eq!(
            h.get_one("Strict-Transport-Security"),
            Some("max-age=63072000")
        );
        let csp = h.get_one("Content-Security-Policy").unwrap();
        assert!(csp.starts_with("default-src 'none'; base-uri https://pds.test; "));
        assert!(csp.contains("connect-src 'self'"));
        assert!(csp.contains("style-src 'self' 'sha256-"));
        assert!(csp.contains("frame-ancestors 'none'"));
        assert!(csp.ends_with("upgrade-insecure-requests"));
        assert!(!csp.contains("hcaptcha"));
        assert!(h.get_one("Content-Type").unwrap().starts_with("text/html"));
        assert_eq!(response.into_string().unwrap(), "<p>hello</p>");

        let response = client.get("/plain").dispatch();
        assert_eq!(response.status(), Status::BadRequest);
        let h = response.headers();
        assert_eq!(h.get_one("Strict-Transport-Security"), None);
        let csp = h.get_one("Content-Security-Policy").unwrap();
        assert!(csp.contains("script-src https://hcaptcha.com https://*.hcaptcha.com"));
        assert!(csp.contains("frame-src https://hcaptcha.com"));
        assert!(!csp.contains("upgrade-insecure-requests"));
    }
}
