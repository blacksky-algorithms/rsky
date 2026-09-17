//! The account manager screens.

use super::AccountCardView;
use crate::ui::scopes::PermissionGroup;
use crate::ui::shell::PageShell;
use askama::Template;

/// One entry of the account manager's navigation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavItem {
    pub href: String,
    pub title: &'static str,
    pub description: &'static str,
    /// The name of an inline icon partial
    pub icon: &'static str,
    pub current: bool,
}

/// The parts of the frame every account page renders into.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountNav {
    /// The account home, and the target of the mobile back button
    pub base_href: String,
    pub items: Vec<NavItem>,
    /// The heading of the current page; empty on the home page
    pub page_title: String,
    pub at_base: bool,
    pub account: AccountCardView,
    /// Another account can be picked on this device
    pub can_switch: bool,
    pub switch_href: String,
    pub sign_out_action: String,
    pub csrf: String,
}

impl AccountNav {
    /// The reference's five entries, in its order, for the account named
    /// by `account_id` in URLs.
    pub fn items_for(account_id: &str, current: Section) -> Vec<NavItem> {
        let base = format!("/account/u/{account_id}");
        [
            (Section::Home, base.clone(), "Home", "", "house"),
            (
                Section::Manage,
                format!("{base}/manage"),
                "Account",
                "Manage your account",
                "user",
            ),
            (
                Section::Devices,
                format!("{base}/devices"),
                "Devices",
                "Manage your active sessions",
                "monitor-smartphone",
            ),
            (
                Section::Apps,
                format!("{base}/apps"),
                "Apps",
                "Manage applications that have access to your account",
                "globe",
            ),
            (
                Section::About,
                format!("{base}/about"),
                "About",
                "What is an Atmosphere Account?",
                "circle-question-mark",
            ),
        ]
        .into_iter()
        .map(|(section, href, title, description, icon)| NavItem {
            href,
            title,
            description,
            icon,
            current: section == current,
        })
        .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Home,
    Manage,
    Devices,
    Apps,
    About,
}

impl Section {
    pub fn title(self) -> &'static str {
        match self {
            Section::Home => "",
            Section::Manage => "Account",
            Section::Devices => "Devices",
            Section::Apps => "Apps",
            Section::About => "About",
        }
    }
}

#[derive(Template)]
#[template(path = "account/home.html")]
pub struct HomePage {
    pub shell: PageShell,
    pub nav: AccountNav,
    pub about_href: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeviceRow {
    pub device_id: String,
    /// Empty when the user agent is unknown
    pub name: String,
    pub ip_address: String,
    pub last_seen: String,
    pub current: bool,
}

#[derive(Template)]
#[template(path = "account/devices.html")]
pub struct DevicesPage {
    pub shell: PageShell,
    pub nav: AccountNav,
    pub apps_href: String,
    pub filter: String,
    /// Every device, before filtering
    pub total: usize,
    pub devices: Vec<DeviceRow>,
    pub sign_out_action: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AppRow {
    pub token_id: String,
    pub name: String,
    pub identifier: String,
    pub authorized: String,
    pub last_accessed: String,
    pub details_href: String,
}

#[derive(Template)]
#[template(path = "account/apps.html")]
pub struct AppsPage {
    pub shell: PageShell,
    pub nav: AccountNav,
    pub filter: String,
    pub total: usize,
    pub apps: Vec<AppRow>,
}

#[derive(Template)]
#[template(path = "account/app_details.html")]
pub struct AppDetailsPage {
    pub shell: PageShell,
    pub nav: AccountNav,
    pub token_id: String,
    pub name: String,
    pub identifier: String,
    pub only_atproto: bool,
    pub groups: Vec<PermissionGroup>,
    pub revoke_action: String,
    pub back_href: String,
}

#[derive(Template)]
#[template(path = "account/about.html")]
pub struct AboutPage {
    pub shell: PageShell,
    pub nav: AccountNav,
    pub handle: String,
    /// The app profile link, when the deployment names an app
    pub profile_href: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::branding::Branding;
    use crate::ui::scopes::permission_groups;
    use std::collections::BTreeMap;

    fn shell() -> PageShell {
        let branding = Branding {
            service_name: "Example PDS".into(),
            logo_url: Some("https://x.test/logo.svg".into()),
            home_url: Some("https://home.test".into()),
            app_name: Some("Blacksky".into()),
            app_url: Some("https://blacksky.community".into()),
            org_name: Some("Blacksky Algorithms".into()),
            ..Branding::default()
        };
        PageShell::new(&branding, "https://pds.test", "pds.test")
    }

    fn nav(section: Section) -> AccountNav {
        AccountNav {
            base_href: "/account/u/alice.test".into(),
            items: AccountNav::items_for("alice.test", section),
            page_title: section.title().to_string(),
            at_base: section == Section::Home,
            account: AccountCardView {
                did: "did:plc:alice".into(),
                handle: "@alice.test".into(),
                ..AccountCardView::default()
            },
            can_switch: true,
            switch_href: "/account/sign-in".into(),
            sign_out_action: "/account/u/alice.test/sign-out".into(),
            csrf: "csrf-token".into(),
        }
    }

    #[test]
    fn navigation_lists_the_reference_sections() {
        let items = AccountNav::items_for("did:plc:alice", Section::Apps);
        let titles: Vec<&str> = items.iter().map(|i| i.title).collect();
        assert_eq!(titles, ["Home", "Account", "Devices", "Apps", "About"]);
        assert_eq!(items[3].href, "/account/u/did:plc:alice/apps");
        assert!(items[3].current);
        assert!(!items[0].current);
        assert_eq!(Section::Home.title(), "");
        assert_eq!(Section::Manage.title(), "Account");
    }

    #[test]
    fn home_page_lists_the_other_sections() {
        let html = HomePage {
            shell: shell(),
            nav: nav(Section::Home),
            about_href: "/account/u/alice.test/about".into(),
        }
        .render()
        .unwrap();
        assert!(html.contains("<title>My Atmosphere Account</title>"));
        assert!(html.contains("Your Atmosphere account is hosted by <b>Example PDS</b>."));
        assert!(html.contains(">What does this mean?</a>"));
        assert!(html.contains("Manage your active sessions"));
        assert!(html.contains("What is an Atmosphere Account?"));
        assert!(!html.contains("item-title\">Home"));
        assert!(html.contains("aria-current=\"page\""));
        assert!(html.contains("Select another account"));
        assert!(html.contains("<span>Sign out</span></button>"));
        assert!(html.contains("name=\"csrf\" value=\"csrf-token\""));
        assert!(!html.contains("back-link show"));
        assert!(html.contains(">Home</a>"));
        assert!(!html.contains("Bluesky"));
    }

    #[test]
    fn devices_page_variants() {
        let mut page = DevicesPage {
            shell: shell(),
            nav: nav(Section::Devices),
            apps_href: "/account/u/alice.test/apps".into(),
            filter: String::new(),
            total: 2,
            devices: vec![
                DeviceRow {
                    device_id: "dev-1".into(),
                    name: "macOS \u{2022} Safari".into(),
                    ip_address: "10.0.0.1".into(),
                    last_seen: "just now".into(),
                    current: true,
                },
                DeviceRow {
                    device_id: "dev-2".into(),
                    name: String::new(),
                    ip_address: "10.0.0.2".into(),
                    last_seen: "3 days ago".into(),
                    current: false,
                },
            ],
            sign_out_action: "/account/u/alice.test/devices/sign-out".into(),
        };
        let html = page.render().unwrap();
        assert!(html.contains("<h2>Devices</h2>"));
        assert!(html.contains("back-link show"));
        assert!(html.contains("sign out all devices"));
        assert!(html.contains("placeholder=\"Filter devices\""));
        assert!(html.contains("This device"));
        assert!(html.contains("Unknown user agent"));
        assert!(html.contains("title=\"Cannot remove current device\" disabled"));
        assert!(html.contains("name=\"device_id\" value=\"dev-2\""));
        assert!(!html.contains("No matches"));

        page.filter = "nothing".into();
        page.devices.clear();
        let html = page.render().unwrap();
        assert!(html.contains("No matches"));
        assert!(html.contains("value=\"nothing\""));

        page.total = 0;
        let html = page.render().unwrap();
        assert!(html.contains("Looks like you aren't logged in on any other devices."));
        assert!(!html.contains("Filter devices"));
    }

    #[test]
    fn apps_pages_variants() {
        let mut page = AppsPage {
            shell: shell(),
            nav: nav(Section::Apps),
            filter: String::new(),
            total: 1,
            apps: vec![AppRow {
                token_id: "tok-1".into(),
                name: "Example App".into(),
                identifier: "app.example".into(),
                authorized: "Sep 9, 2026".into(),
                last_accessed: "2 hours ago".into(),
                details_href: "/account/u/alice.test/apps/tok-1".into(),
            }],
        };
        let html = page.render().unwrap();
        assert!(html.contains("These apps have access to your account."));
        assert!(html.contains("placeholder=\"Filter apps\""));
        assert!(html.contains("Why is this time so recent?"));
        assert!(html.contains("href=\"/account/u/alice.test/apps/tok-1\">Details</a>"));
        page.apps.clear();
        page.filter = "x".into();
        assert!(page.render().unwrap().contains("No matches"));
        page.total = 0;
        assert!(page
            .render()
            .unwrap()
            .contains("It appears that you haven't used this account to sign in to any apps yet."));

        let grouping = permission_groups(
            &["atproto".to_string(), "transition:generic".to_string()],
            &BTreeMap::new(),
            false,
            Some("Blacksky"),
        );
        let mut details = AppDetailsPage {
            shell: shell(),
            nav: nav(Section::Apps),
            token_id: "tok-1".into(),
            name: "Example App".into(),
            identifier: "app.example".into(),
            only_atproto: false,
            groups: grouping.groups,
            revoke_action: "/account/u/alice.test/apps/revoke".into(),
            back_href: "/account/u/alice.test/apps".into(),
        };
        let html = details.render().unwrap();
        assert!(
            html.contains("This app has access to your account with the following permissions:")
        );
        assert!(html.contains("Blacksky"));
        assert!(html.contains("Repository"));
        assert!(html.contains("name=\"token_id\" value=\"tok-1\""));
        assert!(html.contains(">Revoke access</button>"));
        assert!(html.contains(">Close</a>"));
        details.only_atproto = true;
        details.groups.clear();
        let html = details.render().unwrap();
        assert!(html.contains("This app can uniquely identify you through your account."));
    }

    #[test]
    fn about_page_names_the_deployment_and_its_app() {
        let mut page = AboutPage {
            shell: shell(),
            nav: nav(Section::About),
            handle: "@alice.test".into(),
            profile_href: Some("https://blacksky.community/profile/alice.test".into()),
        };
        let html = page.render().unwrap();
        assert!(html.contains("What is an Atmosphere account?"));
        assert!(html.contains("href=\"https://blacksky.community\">Blacksky</a>"));
        assert!(html.contains("https://blacksky.community/profile/alice.test\">Blacksky app</a>"));
        assert!(html.contains("hosted by <b>Example PDS</b>"));
        assert!(html.contains("href=\"https://home.test\">Blacksky Algorithms</a>"));
        assert!(html.contains("href=\"https://atproto.com\">AT Protocol</a>"));
        assert!(html.contains("@alice.test"));
        assert!(!html.contains("Bluesky"));

        page.profile_href = None;
        let branding = Branding {
            service_name: "pds.test PDS".into(),
            ..Branding::default()
        };
        page.shell = PageShell::new(&branding, "https://pds.test", "pds.test");
        let html = page.render().unwrap();
        assert!(html.contains("any social app built on the same network"));
        assert!(!html.contains("Learn more about the network"));
        assert!(html.contains("AT Protocol"));
    }
}
