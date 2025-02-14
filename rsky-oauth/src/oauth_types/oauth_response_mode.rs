use std::fmt;
use std::str::FromStr;

/// The response mode for an OAuth authorization request.
/// 
/// Specifies how the authorization response parameters should be returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OAuthResponseMode {
    /// Return parameters in the query string
    Query,
    /// Return parameters in the fragment
    Fragment,
    /// Return parameters in a form POST request
    FormPost,
}

impl OAuthResponseMode {
    /// Get a slice of all possible response modes
    pub fn variants() -> &'static [OAuthResponseMode] {
        &[
            OAuthResponseMode::Query,
            OAuthResponseMode::Fragment,
            OAuthResponseMode::FormPost,
        ]
    }
}

impl fmt::Display for OAuthResponseMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OAuthResponseMode::Query => write!(f, "query"),
            OAuthResponseMode::Fragment => write!(f, "fragment"),
            OAuthResponseMode::FormPost => write!(f, "form_post"),
        }
    }
}

/// Error returned when parsing a string into an OAuthResponseMode fails.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("Invalid response mode: {0}")]
pub struct ParseResponseModeError(String);

impl FromStr for OAuthResponseMode {
    type Err = ParseResponseModeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "query" => Ok(OAuthResponseMode::Query),
            "fragment" => Ok(OAuthResponseMode::Fragment),
            "form_post" => Ok(OAuthResponseMode::FormPost),
            _ => Err(ParseResponseModeError(s.to_string())),
        }
    }
}

impl AsRef<str> for OAuthResponseMode {
    fn as_ref(&self) -> &str {
        match self {
            OAuthResponseMode::Query => "query",
            OAuthResponseMode::Fragment => "fragment",
            OAuthResponseMode::FormPost => "form_post",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::AsRef;

    #[test]
    fn test_variants() {
        let variants = OAuthResponseMode::variants();
        assert_eq!(variants.len(), 3);
        assert!(variants.contains(&OAuthResponseMode::Query));
        assert!(variants.contains(&OAuthResponseMode::Fragment));
        assert!(variants.contains(&OAuthResponseMode::FormPost));
    }

    #[test]
    fn test_display() {
        assert_eq!(OAuthResponseMode::Query.to_string(), "query");
        assert_eq!(OAuthResponseMode::Fragment.to_string(), "fragment");
        assert_eq!(OAuthResponseMode::FormPost.to_string(), "form_post");
    }

    #[test]
    fn test_from_str() {
        assert_eq!("query".parse::<OAuthResponseMode>().unwrap(), OAuthResponseMode::Query);
        assert_eq!("QUERY".parse::<OAuthResponseMode>().unwrap(), OAuthResponseMode::Query);
        assert_eq!("fragment".parse::<OAuthResponseMode>().unwrap(), OAuthResponseMode::Fragment);
        assert_eq!("form_post".parse::<OAuthResponseMode>().unwrap(), OAuthResponseMode::FormPost);
        
        assert!("invalid".parse::<OAuthResponseMode>().is_err());
    }

    #[test]
    fn test_as_ref() {
        assert_eq!(OAuthResponseMode::Query.as_ref(), "query");
        assert_eq!(OAuthResponseMode::Fragment.as_ref(), "fragment");
        assert_eq!(OAuthResponseMode::FormPost.as_ref(), "form_post");
    }

    #[test]
    fn test_clone_and_copy() {
        let mode = OAuthResponseMode::Query;
        let cloned = mode.clone();
        assert_eq!(mode, cloned);
        
        let copied = mode;
        assert_eq!(mode, copied);
    }
}