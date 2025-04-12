use crate::oauth_provider::lib::util::url::UrlReference;
use rocket::Request;

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
    validate_header_value(req, "sec-fetch-mode", allowed_values)
}

pub fn validate_fetch_dest(req: &Request, allowed_values: Vec<&str>) -> Result<(), ()> {
    validate_header_value(req, "sec-fetch-dest", allowed_values)
}

pub fn validate_fetch_site(req: &Request, allowed_values: Vec<&str>) -> Result<(), ()> {
    validate_header_value(req, "sec-fetch-site", allowed_values)
}

// CORS ensure not cross origin
pub fn validate_same_origin(req: &Request, origin: &str) -> Result<(), ()> {
    match req.headers().get_one("origin") {
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
    let cookies = req.cookies();
    if cookies.get(cookie_name).is_none() {
        // No Cookie
    }

    if cookies.get(cookie_name).unwrap().value() != csrf_token {
        //Invalid Cookie
    }

    if clear_cookie {
        unimplemented!()
    }
    unimplemented!()
}

pub fn validate_referer(req: &Request, url_reference: UrlReference) -> Result<(), ()> {
    let headers = req.headers().clone();
    let referer = headers.get("referer").next();
    match referer {
        None => {
            unimplemented!()
        }
        Some(referer) => {
            unimplemented!()
        }
    }
}
