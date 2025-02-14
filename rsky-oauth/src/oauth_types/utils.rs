use http::Uri;
use std::net::IpAddr;

pub type LoopbackHost = String;

pub fn is_hostname_ip(hostname: &str) -> bool {
    // Try parsing as IPv4 or IPv6
    if let Ok(_ip) = hostname.parse::<IpAddr>() {
        return true;
    }

    // Check for bracketed IPv6
    if hostname.starts_with('[') && hostname.ends_with(']') {
        if let Ok(_ip) = hostname[1..hostname.len()-1].parse::<IpAddr>() {
            return true;
        }
    }

    false
}

pub fn is_loopback_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "[::1]")
}

pub fn is_loopback_url(input: &Uri) -> bool {
    // Uri::authority() returns Option<&Authority>
    input.authority()
        .map(|auth| is_loopback_host(auth.host()))
        .unwrap_or(false)
}

pub fn safe_url(input: &str) -> Option<Uri> {
    input.parse::<Uri>().ok()
}

pub fn extract_url_path(url: &str) -> Result<String, &'static str> {
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err("URL must use the 'https:' or 'http:' protocol");
    }

    let end_of_protocol = if url.starts_with("https://") { 8 } else { 7 };

    let hash_idx = url[end_of_protocol..].find('#').map(|i| i + end_of_protocol);
    let question_idx = url[end_of_protocol..].find('?').map(|i| i + end_of_protocol);

    let query_str_idx = match (question_idx, hash_idx) {
        (Some(q), Some(h)) if q < h => Some(q),
        (Some(q), None) => Some(q),
        _ => None,
    };

    let path_end = match (hash_idx, query_str_idx) {
        (None, None) => url.len(),
        (None, Some(q)) => q,
        (Some(h), None) => h,
        (Some(h), Some(q)) => h.min(q),
    };

    let slash_idx = url[end_of_protocol..].find('/').map(|i| i + end_of_protocol);
    
    let path_start = match slash_idx {
        Some(idx) if idx <= path_end => idx,
        _ => path_end,
    };

    if end_of_protocol == path_start {
        return Err("URL must contain a host");
    }

    Ok(url[path_start..path_end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_hostname_ip() {
        assert!(is_hostname_ip("127.0.0.1"));
        assert!(is_hostname_ip("[::1]"));
        assert!(!is_hostname_ip("localhost"));
    }

    #[test]
    fn test_is_loopback_host() {
        assert!(is_loopback_host("localhost"));
        assert!(is_loopback_host("127.0.0.1"));
        assert!(is_loopback_host("[::1]"));
        assert!(!is_loopback_host("example.com"));
    }

    #[test]
    fn test_extract_url_path() {
        assert_eq!(extract_url_path("https://example.com/path").unwrap(), "/path");
        assert_eq!(extract_url_path("https://example.com/path?query").unwrap(), "/path");
        assert_eq!(extract_url_path("https://example.com/path#hash").unwrap(), "/path");
        assert!(extract_url_path("ftp://example.com").is_err());
    }
}