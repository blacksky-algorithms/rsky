use askama::Template;
use rsky_oauth::AuthorizePageData;

pub fn scope_description(scope: &str) -> &'static str {
    match scope {
        "atproto" => "Uniquely identify your account",
        "transition:generic" => "Full access to your account data (except chats and email)",
        "transition:chat.bsky" => "Access your direct messages",
        "transition:email" => "Read your account's email address",
        _ => "Additional access requested by the app",
    }
}

/// A signed-in account shown in the device account picker.
pub struct SessionOption {
    pub did: String,
    pub label: String,
}

#[derive(Template)]
#[template(path = "oauth_signin.html")]
pub struct SignInPage {
    pub client_display: String,
    pub client_id: String,
    pub request_uri: String,
    pub csrf: String,
    pub login_hint: String,
    pub error: Option<String>,
    pub signup_url: Option<String>,
    pub sessions: Vec<SessionOption>,
}

pub struct ScopeItem {
    pub scope: String,
    pub description: &'static str,
}

#[derive(Template)]
#[template(path = "oauth_consent.html")]
pub struct ConsentPage {
    pub client_display: String,
    pub client_id: String,
    pub client_trusted: bool,
    pub request_uri: String,
    pub csrf: String,
    pub did: String,
    pub account_label: String,
    pub scopes: Vec<ScopeItem>,
}

#[derive(Template)]
#[template(path = "oauth_error.html")]
pub struct ErrorPage {
    pub message: String,
}

pub fn client_display(page: &AuthorizePageData) -> String {
    match (&page.client_name, page.client_trusted) {
        (Some(name), true) => name.clone(),
        _ => page.client_id.clone(),
    }
}

pub fn scope_items(scopes: &[String]) -> Vec<ScopeItem> {
    scopes
        .iter()
        .map(|scope| ScopeItem {
            scope: scope.clone(),
            description: scope_description(scope),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_sign_in_page() {
        let page = SignInPage {
            client_display: "Example App".to_string(),
            client_id: "https://app.example.com/client".to_string(),
            request_uri: "urn:ietf:params:oauth:request_uri:req-x".to_string(),
            csrf: "csrf-token".to_string(),
            login_hint: "alice.example.com".to_string(),
            error: Some("Invalid identifier or password".to_string()),
            signup_url: Some("https://example.com/signup".to_string()),
            sessions: vec![SessionOption {
                did: "did:plc:alice".to_string(),
                label: "alice.example.com".to_string(),
            }],
        };
        let html = page.render().unwrap();
        assert!(html.contains("Example App"));
        assert!(html.contains("urn:ietf:params:oauth:request_uri:req-x"));
        assert!(html.contains("csrf-token"));
        assert!(html.contains("alice.example.com"));
        assert!(html.contains("Invalid identifier or password"));
        assert!(html.contains("https://example.com/signup"));
        assert!(html.contains("did:plc:alice"));
        assert!(html.contains("name=\"password\""));
    }

    #[test]
    fn renders_sign_in_page_without_optionals() {
        let page = SignInPage {
            client_display: "https://app.example.com/client".to_string(),
            client_id: "https://app.example.com/client".to_string(),
            request_uri: "urn:x".to_string(),
            csrf: "csrf".to_string(),
            login_hint: String::new(),
            error: None,
            signup_url: None,
            sessions: vec![],
        };
        let html = page.render().unwrap();
        assert!(html.contains("Sign in"));
        assert!(!html.contains("Create an account"));
    }

    #[test]
    fn renders_consent_page() {
        let page = ConsentPage {
            client_display: "Example App".to_string(),
            client_id: "https://app.example.com/client".to_string(),
            client_trusted: true,
            request_uri: "urn:x".to_string(),
            csrf: "csrf".to_string(),
            did: "did:plc:alice".to_string(),
            account_label: "alice.example.com".to_string(),
            scopes: scope_items(&[
                "atproto".to_string(),
                "transition:generic".to_string(),
                "transition:chat.bsky".to_string(),
                "transition:email".to_string(),
                "unknown:scope".to_string(),
            ]),
        };
        let html = page.render().unwrap();
        assert!(html.contains("Example App"));
        assert!(html.contains("alice.example.com"));
        assert!(html.contains("Uniquely identify your account"));
        assert!(html.contains("Access your direct messages"));
        assert!(html.contains("Additional access requested by the app"));
        assert!(html.contains("Authorize"));
        assert!(html.contains("Deny"));
    }

    #[test]
    fn renders_error_page() {
        let page = ErrorPage {
            message: "this request has expired".to_string(),
        };
        let html = page.render().unwrap();
        assert!(html.contains("this request has expired"));
    }

    #[test]
    fn client_display_only_trusts_named_trusted_clients() {
        let mut data = AuthorizePageData {
            request_uri: "urn:x".to_string(),
            client_id: "https://app.example.com/client".to_string(),
            client_name: Some("Example App".to_string()),
            client_uri: None,
            logo_uri: None,
            client_trusted: false,
            scopes: vec![],
            login_hint: None,
            prompt: None,
            sessions: vec![],
        };
        assert_eq!(client_display(&data), "https://app.example.com/client");
        data.client_trusted = true;
        assert_eq!(client_display(&data), "Example App");
        data.client_name = None;
        assert_eq!(client_display(&data), "https://app.example.com/client");
    }
}
