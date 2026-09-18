//! What every page shares: the deployment's name, logo, links, stylesheet,
//! and the copy variables that name the client app and operator.

use super::assets::stylesheet_href;
use super::branding::{BrandLink, Branding};
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq)]
pub struct PageShell {
    pub service_name: String,
    pub logo_url: Option<String>,
    pub links: Vec<BrandLink>,
    pub stylesheet_href: String,
    pub branding_css: String,
    /// The CSP hash of the inline branding style block
    pub branding_css_sha256: String,
    pub app_name: Option<String>,
    pub app_url: Option<String>,
    pub org_name: Option<String>,
    pub home_url: Option<String>,
    pub public_url: String,
    pub hostname: String,
    pub hsts: bool,
}

impl PageShell {
    pub fn new(branding: &Branding, public_url: &str, hostname: &str) -> Self {
        let branding_css = branding.css_vars();
        let branding_css_sha256 = STANDARD.encode(Sha256::digest(branding_css.as_bytes()));
        PageShell {
            service_name: branding.service_name.clone(),
            logo_url: branding.logo_url.clone(),
            links: branding.links(),
            stylesheet_href: stylesheet_href(),
            branding_css,
            branding_css_sha256,
            app_name: branding.app_name.clone(),
            app_url: branding.app_url.clone(),
            org_name: branding.org_name.clone(),
            home_url: branding.home_url.clone(),
            public_url: public_url.trim_end_matches('/').to_string(),
            hostname: hostname.to_string(),
            hsts: public_url.starts_with("https://"),
        }
    }

    /// The app name for copy that would otherwise have to name one.
    pub fn app_name_or(&self, fallback: &str) -> String {
        self.app_name
            .clone()
            .unwrap_or_else(|| fallback.to_string())
    }

    /// The origin the CSP `base-uri` is pinned to.
    pub fn origin(&self) -> String {
        self.public_url.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::branding::RgbColor;

    #[test]
    fn carries_branding_and_security_facts() {
        let branding = Branding {
            service_name: "Example".into(),
            primary: Some(RgbColor::parse("#6060E9").unwrap()),
            home_url: Some("https://home.test".into()),
            app_name: Some("App".into()),
            ..Branding::default()
        };
        let shell = PageShell::new(&branding, "https://pds.test/", "pds.test");
        assert_eq!(shell.service_name, "Example");
        assert_eq!(shell.links.len(), 1);
        assert!(shell.stylesheet_href.starts_with("/oauth/assets/ui-"));
        assert!(shell
            .branding_css
            .contains("--branding-color-primary: 96 96 233"));
        assert_eq!(shell.branding_css_sha256.len(), 44);
        assert_eq!(shell.public_url, "https://pds.test");
        assert_eq!(shell.origin(), "https://pds.test");
        assert!(shell.hsts);
        assert_eq!(shell.app_name_or("Social app"), "App");

        let plain = PageShell::new(&Branding::default(), "http://localhost:2583", "localhost");
        assert!(!plain.hsts);
        assert_eq!(plain.app_name_or("Social app"), "Social app");
        assert_eq!(plain.branding_css, "");
    }
}
