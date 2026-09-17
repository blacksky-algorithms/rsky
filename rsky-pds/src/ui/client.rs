//! How a client is named and pictured, following the reference UI: a
//! trusted client is shown by its registered name and logo; anyone else by
//! what its client id proves, so a page can never be dressed up by metadata
//! the server has no reason to believe.

use rsky_oauth::AuthorizePageData;
use url::Url;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientDisplay {
    /// The registered name of a trusted client
    Name(String),
    /// A loopback client: something running on the user's own machine
    LocalApp,
    /// A conventional client id: just its host
    Host(String),
    /// Any other https id, split so the host can be emphasised
    Url {
        proto: String,
        host: String,
        rest: String,
    },
    Raw(String),
}

impl ClientDisplay {
    pub fn is_local(&self) -> bool {
        matches!(self, ClientDisplay::LocalApp)
    }

    /// The plain-text form, for `<title>` and aria labels.
    pub fn text(&self) -> String {
        match self {
            ClientDisplay::Name(name) => name.clone(),
            ClientDisplay::LocalApp => "An application on your device".to_string(),
            ClientDisplay::Host(host) => host.clone(),
            ClientDisplay::Url { proto, host, rest } => format!("{proto}{host}{rest}"),
            ClientDisplay::Raw(raw) => raw.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientView {
    pub id: String,
    pub display: ClientDisplay,
    /// Only a trusted client's logo is shown
    pub logo_uri: Option<String>,
    pub trusted: bool,
    pub tos_uri: Option<String>,
    pub policy_uri: Option<String>,
}

pub fn is_loopback_client_id(id: &str) -> bool {
    id.starts_with("http://")
}

/// `https://host/oauth-client-metadata.json` with no port and no query.
pub fn is_conventional_client_id(id: &str) -> bool {
    match Url::parse(id) {
        Ok(url) => {
            url.scheme() == "https"
                && url.path() == "/oauth-client-metadata.json"
                && url.port().is_none()
                && url.query().is_none()
                && url.host_str().is_some()
        }
        Err(_) => false,
    }
}

pub fn client_display(page: &AuthorizePageData) -> ClientDisplay {
    display_for(
        page.client_id.as_str(),
        page.client_name.as_deref(),
        page.client_trusted,
    )
}

pub fn display_for(client_id: &str, client_name: Option<&str>, trusted: bool) -> ClientDisplay {
    if trusted {
        if let Some(name) = client_name.map(str::trim).filter(|n| !n.is_empty()) {
            return ClientDisplay::Name(name.to_string());
        }
    }
    if is_loopback_client_id(client_id) {
        return ClientDisplay::LocalApp;
    }
    match Url::parse(client_id) {
        Ok(url) if url.scheme() == "https" && url.host_str().is_some() => {
            let host = url.host_str().unwrap_or_default().to_string();
            if is_conventional_client_id(client_id) {
                return ClientDisplay::Host(host);
            }
            let port = url.port().map(|p| format!(":{p}")).unwrap_or_default();
            let query = url.query().map(|q| format!("?{q}")).unwrap_or_default();
            ClientDisplay::Url {
                proto: "https://".to_string(),
                host: format!("{host}{port}"),
                rest: format!("{}{query}", url.path()),
            }
        }
        _ => ClientDisplay::Raw(client_id.to_string()),
    }
}

pub fn client_view(page: &AuthorizePageData) -> ClientView {
    ClientView {
        id: page.client_id.clone(),
        display: client_display(page),
        logo_uri: if page.client_trusted {
            page.logo_uri.clone().filter(|l| !l.is_empty())
        } else {
            None
        },
        trusted: page.client_trusted,
        tos_uri: page.tos_uri.clone().filter(|u| !u.is_empty()),
        policy_uri: page.policy_uri.clone().filter(|u| !u.is_empty()),
    }
}

/// The "App" column of the apps list: the registered name when the client
/// published one (trust is not needed to label a session the user already
/// granted), otherwise the same shortening rules as the consent page.
pub fn client_app_name(client_id: &str, client_name: Option<&str>) -> String {
    if is_loopback_client_id(client_id) {
        return "A local app".to_string();
    }
    if let Some(name) = client_name.map(str::trim).filter(|n| !n.is_empty()) {
        return name.to_string();
    }
    if is_conventional_client_id(client_id) {
        return Url::parse(client_id)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
            .unwrap_or_else(|| client_id.to_string());
    }
    client_id.to_string()
}

/// The "Client" column of the apps list.
pub fn client_identifier(client_id: &str) -> String {
    if is_loopback_client_id(client_id) {
        return "loopback".to_string();
    }
    if is_conventional_client_id(client_id) {
        return Url::parse(client_id)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
            .unwrap_or_else(|| client_id.to_string());
    }
    client_id.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(client_id: &str, name: Option<&str>, trusted: bool) -> AuthorizePageData {
        AuthorizePageData {
            client_id: client_id.to_string(),
            client_name: name.map(str::to_string),
            client_trusted: trusted,
            logo_uri: Some("https://app.example/logo.png".to_string()),
            ..AuthorizePageData::default()
        }
    }

    #[test]
    fn names_trusted_clients_and_describes_the_rest() {
        assert_eq!(
            display_for(
                "https://app.example/oauth-client-metadata.json",
                Some("Example App"),
                true
            ),
            ClientDisplay::Name("Example App".into())
        );
        assert_eq!(
            display_for(
                "https://app.example/oauth-client-metadata.json",
                Some(" "),
                true
            ),
            ClientDisplay::Host("app.example".into())
        );
        assert_eq!(
            display_for(
                "http://localhost?redirect_uri=http%3A%2F%2F127.0.0.1%3A19006%2Fauth%2Fweb%2Fcallback&scope=atproto",
                None,
                false
            ),
            ClientDisplay::LocalApp
        );
        assert_eq!(
            display_for(
                "https://app.example/oauth-client-metadata.json",
                Some("Untrusted"),
                false
            ),
            ClientDisplay::Host("app.example".into())
        );
        assert_eq!(
            display_for(
                "https://app.example:8443/clients/meta.json?v=2",
                None,
                false
            ),
            ClientDisplay::Url {
                proto: "https://".into(),
                host: "app.example:8443".into(),
                rest: "/clients/meta.json?v=2".into(),
            }
        );
        assert_eq!(
            display_for("urn:something", None, false),
            ClientDisplay::Raw("urn:something".into())
        );
        assert_eq!(
            ClientDisplay::LocalApp.text(),
            "An application on your device"
        );
        assert!(ClientDisplay::LocalApp.is_local());
        assert_eq!(
            ClientDisplay::Url {
                proto: "https://".into(),
                host: "h".into(),
                rest: "/p".into()
            }
            .text(),
            "https://h/p"
        );
        assert_eq!(ClientDisplay::Raw("r".into()).text(), "r");
        assert_eq!(ClientDisplay::Host("h".into()).text(), "h");
        assert_eq!(ClientDisplay::Name("n".into()).text(), "n");
    }

    #[test]
    fn conventional_ids_are_strict() {
        assert!(is_conventional_client_id(
            "https://app.example/oauth-client-metadata.json"
        ));
        assert!(is_conventional_client_id(
            "https://app.example:443/oauth-client-metadata.json"
        ));
        assert!(!is_conventional_client_id(
            "https://app.example:8443/oauth-client-metadata.json"
        ));
        assert!(!is_conventional_client_id(
            "https://app.example/oauth-client-metadata.json?x=1"
        ));
        assert!(!is_conventional_client_id("https://app.example/meta.json"));
        assert!(!is_conventional_client_id(
            "http://app.example/oauth-client-metadata.json"
        ));
        assert!(!is_conventional_client_id("not a url"));
    }

    #[test]
    fn the_view_hides_untrusted_logos_and_keeps_policy_links() {
        let mut p = page(
            "https://app.example/oauth-client-metadata.json",
            Some("Example"),
            true,
        );
        p.tos_uri = Some("https://app.example/tos".into());
        p.policy_uri = Some(String::new());
        let view = client_view(&p);
        assert_eq!(view.display, ClientDisplay::Name("Example".into()));
        assert_eq!(
            view.logo_uri.as_deref(),
            Some("https://app.example/logo.png")
        );
        assert_eq!(view.tos_uri.as_deref(), Some("https://app.example/tos"));
        assert_eq!(view.policy_uri, None);
        assert!(view.trusted);

        let view = client_view(&page(
            "https://app.example/oauth-client-metadata.json",
            Some("Example"),
            false,
        ));
        assert_eq!(view.logo_uri, None);
        assert_eq!(view.display, ClientDisplay::Host("app.example".into()));
    }

    #[test]
    fn apps_list_labels() {
        assert_eq!(
            client_app_name("http://localhost?x=1", Some("Local")),
            "A local app"
        );
        assert_eq!(
            client_app_name(
                "https://app.example/oauth-client-metadata.json",
                Some("Named")
            ),
            "Named"
        );
        assert_eq!(
            client_app_name("https://app.example/oauth-client-metadata.json", None),
            "app.example"
        );
        assert_eq!(
            client_app_name("https://app.example/other.json", None),
            "https://app.example/other.json"
        );
        assert_eq!(client_identifier("http://localhost"), "loopback");
        assert_eq!(
            client_identifier("https://app.example/oauth-client-metadata.json"),
            "app.example"
        );
        assert_eq!(
            client_identifier("https://app.example/other.json"),
            "https://app.example/other.json"
        );
    }
}
