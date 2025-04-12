use std::net::{IpAddr, Ipv4Addr};
use url::Url;

/// Check if a hostname string represents an IP address.
pub fn is_hostname_ip(hostname: &str) -> bool {
    // Remove IPv6 brackets if present
    let hostname = hostname.trim_start_matches('[').trim_end_matches(']');
    hostname.parse::<IpAddr>().is_ok()
}

/// List of known loopback hosts
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoopbackHost {
    Localhost(String),
    Ipv4Loopback(String), // Any address in 127.0.0.0/8
    Ipv6Loopback(String), // [::1]
}

impl LoopbackHost {
    pub fn new_ipv4_loopback(addr: &str) -> Self {
        Self::Ipv4Loopback(addr.to_string())
    }

    pub fn new_ipv6_loopback(addr: &str) -> Self {
        Self::Ipv6Loopback(addr.to_string())
    }

    pub fn new_localhost(name: &str) -> Self {
        Self::Localhost(name.to_string())
    }
}

impl AsRef<str> for LoopbackHost {
    fn as_ref(&self) -> &str {
        match self {
            LoopbackHost::Localhost(s) => s,
            LoopbackHost::Ipv4Loopback(s) => s,
            LoopbackHost::Ipv6Loopback(s) => s,
        }
    }
}

/// Check if an IPv4 address is in the loopback range (127.0.0.0/8)
pub fn is_ipv4_loopback(addr: &str) -> bool {
    if let Ok(ip) = addr.parse::<Ipv4Addr>() {
        return ip.octets()[0] == 127;
    }
    false
}

/// Check if a host string is a loopback host.
pub fn is_loopback_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }

    // Check for IPv6 loopback
    if host == "[::1]" {
        return true;
    }

    // Check for any IPv4 address in 127.0.0.0/8 block
    is_ipv4_loopback(host)
}

/// Check if a URL points to a loopback address.
pub fn is_loopback_url(url: &Url) -> bool {
    if let Some(host) = url.host_str() {
        is_loopback_host(host)
    } else {
        false
    }
}

/// Safely parse a URL string, returning None if parsing fails.
pub fn safe_url(input: &str) -> Option<Url> {
    Url::parse(input).ok()
}

/// Extract the path component from a URL string without normalizing it.
pub fn extract_url_path(url: &str) -> Result<String, &'static str> {
    // Check protocol
    let (path_start, protocol) = if url.starts_with("https://") {
        (8, "https://")
    } else if url.starts_with("http://") {
        (7, "http://")
    } else {
        return Err("URL must use the 'https:' or 'http:' protocol");
    };

    // Find end markers
    let remainder = &url[path_start..];
    let hash_idx = remainder.find('#');
    let query_idx = remainder.find('?');

    // Find path end
    let path_end = match (query_idx, hash_idx) {
        (Some(q), Some(h)) => Some(q.min(h)),
        (Some(q), None) => Some(q),
        (None, Some(h)) => Some(h),
        (None, None) => None,
    };

    // Find first slash after protocol
    let slash_idx = remainder.find('/');

    // Get path based on found indices
    let path = match (slash_idx, path_end) {
        (Some(s), Some(e)) if s < e => &remainder[s..e],
        (Some(s), None) => &remainder[s..],
        _ => "/",
    };

    // Validate host exists
    if protocol.len() == remainder.len() {
        return Err("URL must contain a host");
    }

    Ok(path.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_hostname_ip() {
        assert!(is_hostname_ip("127.0.0.1"));
        assert!(is_hostname_ip("127.1.2.3"));
        assert!(is_hostname_ip("[::1]"));
        assert!(!is_hostname_ip("localhost"));
        assert!(!is_hostname_ip("example.com"));
    }

    #[test]
    fn test_is_ipv4_loopback() {
        assert!(is_ipv4_loopback("127.0.0.1"));
        assert!(is_ipv4_loopback("127.1.2.3"));
        assert!(is_ipv4_loopback("127.255.255.255"));
        assert!(!is_ipv4_loopback("128.0.0.1"));
        assert!(!is_ipv4_loopback("126.255.255.255"));
    }

    #[test]
    fn test_is_loopback_host() {
        assert!(is_loopback_host("localhost"));
        assert!(is_loopback_host("LOCALHOST"));
        assert!(is_loopback_host("127.0.0.1"));
        assert!(is_loopback_host("127.42.13.37"));
        assert!(is_loopback_host("[::1]"));
        assert!(!is_loopback_host("example.com"));
        assert!(!is_loopback_host("192.168.0.1"));
    }

    #[test]
    fn test_is_loopback_url() {
        assert!(is_loopback_url(&Url::parse("http://localhost").unwrap()));
        assert!(is_loopback_url(&Url::parse("http://127.0.0.1").unwrap()));
        assert!(is_loopback_url(&Url::parse("http://127.42.13.37").unwrap()));
        assert!(is_loopback_url(&Url::parse("http://[::1]").unwrap()));
        assert!(!is_loopback_url(&Url::parse("http://example.com").unwrap()));
    }

    #[test]
    fn test_loopback_host_enum() {
        let localhost = LoopbackHost::new_localhost("localhost");
        let ipv4 = LoopbackHost::new_ipv4_loopback("127.0.0.1");
        let ipv4_alt = LoopbackHost::new_ipv4_loopback("127.42.13.37");
        let ipv6 = LoopbackHost::new_ipv6_loopback("[::1]");

        assert_eq!(localhost.as_ref(), "localhost");
        assert_eq!(ipv4.as_ref(), "127.0.0.1");
        assert_eq!(ipv4_alt.as_ref(), "127.42.13.37");
        assert_eq!(ipv6.as_ref(), "[::1]");
    }

    #[test]
    fn test_extract_url_path() {
        assert_eq!(
            extract_url_path("https://example.com/path").unwrap(),
            "/path"
        );
        assert_eq!(extract_url_path("https://example.com/").unwrap(), "/");
        assert_eq!(extract_url_path("https://example.com").unwrap(), "/");
        assert_eq!(
            extract_url_path("https://example.com/path?query").unwrap(),
            "/path"
        );
        assert_eq!(
            extract_url_path("https://example.com/path#fragment").unwrap(),
            "/path"
        );
        assert!(extract_url_path("invalid").is_err());
    }
}
