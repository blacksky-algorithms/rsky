use crate::oauth_provider::lib::util::url::UrlReference;
use rand::distr::Alphanumeric;
use rand::Rng;
use rocket::http::{Cookie, SameSite};
use rocket::time::Duration;
use rocket::Request;
use url::Url;

pub fn validate_header_value(
    req: &Request,
    name: &str,
    allowed_values: Vec<&str>,
) -> Result<(), ()> {
    let headers = req.headers().clone();
    let mut value = headers.get(name);
    let header = value.next();
    if header.is_none() || value.next().is_some() {
        //HTTP Error for invalid header
        return Err(());
    }

    if !allowed_values.contains(&header.unwrap()) {
        //HTTP Error
        return Err(());
    }
    Ok(())
}

pub fn validate_fetch_mode(req: &Request, allowed_values: Vec<&str>) -> Result<(), ()> {
    validate_header_value(req, "Sec-Fetch-Mode", allowed_values)
}

pub fn validate_fetch_dest(req: &Request, allowed_values: Vec<&str>) -> Result<(), ()> {
    validate_header_value(req, "Sec-Fetch-Dest", allowed_values)
}

pub fn validate_fetch_site(req: &Request, allowed_values: Vec<&str>) -> Result<(), ()> {
    validate_header_value(req, "Sec-Fetch-Site", allowed_values)
}

// CORS ensure not cross origin
pub fn validate_same_origin(req: &Request, origin: &str) -> Result<(), ()> {
    match req.headers().get_one("Origin") {
        None => Err(()),
        Some(header_origin) => {
            if header_origin != origin {
                Err(())
            } else {
                Ok(())
            }
        }
    }
}

pub fn validate_csrf_token(
    req: &Request,
    csrf_token: &str,
    cookie_name: &str,
    clear_cookie: bool,
) -> Result<(), ()> {
    let mut cookies = req.cookies();
    let csrf_cookie: &Cookie = match cookies.get(cookie_name) {
        None => return Err(()),
        Some(cookie) => cookie,
    };
    if csrf_cookie.value() != csrf_token {
        return Err(());
    }

    if clear_cookie {
        cookies.remove(csrf_cookie.clone());
        let new_cookie = Cookie::build((csrf_token.to_string(), ""))
            .secure(true)
            .http_only(false)
            .same_site(SameSite::Lax)
            .max_age(Duration::hours(0));
        cookies.add(new_cookie);
    }
    Ok(())
}

pub fn validate_referer(req: &Request, url_reference: UrlReference) -> Result<(), ()> {
    let headers = req.headers().clone();
    let referer = headers.get("referer").next();
    match referer {
        None => Err(()),
        Some(referer) => {
            let referer = Url::parse(referer).unwrap();
            if referer.origin().unicode_serialization() == url_reference.origin.unwrap() {
                Ok(())
            } else {
                Err(())
            }
        }
    }
}

pub fn setup_csrf_token(req: &Request, cookie_name: String) {
    let csrf_token: String = rand::rng()
        .sample_iter(&Alphanumeric)
        .take(16)
        .map(char::from)
        .collect();
    let path = req.clone().uri().path().as_str().to_string();
    let cookie = Cookie::build((cookie_name, csrf_token))
        .secure(true)
        .http_only(false)
        .same_site(SameSite::Lax)
        .path(path);
    req.cookies().add(cookie);
}
