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

/// A row of the manage page.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SettingRow {
    pub href: String,
    pub title: &'static str,
    /// The current value, when the row has one
    pub value: String,
    pub icon: &'static str,
    pub destructive: bool,
}

#[derive(Template)]
#[template(path = "account/manage.html")]
pub struct ManagePage {
    pub shell: PageShell,
    pub nav: AccountNav,
    /// The address awaiting verification, when there is one
    pub unverified_email: Option<String>,
    pub verify_href: String,
    pub deactivated: bool,
    pub rows: Vec<SettingRow>,
    pub notice: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmailStep {
    /// Ask for the new address
    Choose,
    /// The current address is confirmed: a code went there first
    Token,
    /// The address changed; offer to send a code to the new one
    VerifyRequest,
    /// Enter the code sent to the new address
    Verify,
}

#[derive(Template)]
#[template(path = "account/email.html")]
pub struct EmailPage {
    pub shell: PageShell,
    pub nav: AccountNav,
    pub step: EmailStep,
    pub current_email: Option<String>,
    pub new_email: String,
    pub error: Option<String>,
    pub request_action: String,
    pub confirm_action: String,
    pub verify_request_action: String,
    pub verify_action: String,
    pub verify_code_href: String,
    pub cancel_href: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandleMode {
    Choose,
    Default,
    Custom,
}

#[derive(Template)]
#[template(path = "account/handle.html")]
pub struct HandlePage {
    pub shell: PageShell,
    pub nav: AccountNav,
    pub mode: HandleMode,
    pub did: String,
    /// The handle without its domain, when it is on one of this server's
    pub segment: String,
    pub domains: Vec<String>,
    pub selected_domain: String,
    /// The typed domain of a custom handle
    pub custom: String,
    pub error: Option<String>,
    pub default_href: String,
    pub custom_href: String,
    pub submit_action: String,
    pub cancel_href: String,
    pub mailto_href: String,
}

impl HandlePage {
    /// The instructions as a mail body, for "Email these instructions".
    pub fn instructions_mailto(handle: &str, did: &str) -> String {
        let body = format!(
            "Hello,\n\nTo associate the domain \"{handle}\" with my AT Protocol identity ({did}), one of the following configuration changes is required. Either method is sufficient, only one needs to be applied.\n\nDNS: Add the following record to your domain's DNS configuration.\nHost: _atproto.{handle}\nType: TXT\nValue: did={did}\n\nHTTP: Make a text file with the contents below available at the following URL.\nURL: https://{handle}/.well-known/atproto-did\nFile contents: {did}\n\nThank you."
        );
        format!(
            "mailto:?body={}",
            url::form_urlencoded::byte_serialize(body.as_bytes())
                .collect::<String>()
                .replace('+', "%20")
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasswordStep {
    Request,
    Confirm,
}

#[derive(Template)]
#[template(path = "account/password.html")]
pub struct PasswordPage {
    pub shell: PageShell,
    pub nav: AccountNav,
    pub step: PasswordStep,
    pub error: Option<String>,
    pub request_action: String,
    pub confirm_action: String,
    pub code_href: String,
    pub cancel_href: String,
}

#[derive(Template)]
#[template(path = "account/deactivate.html")]
pub struct DeactivatePage {
    pub shell: PageShell,
    pub nav: AccountNav,
    pub error: Option<String>,
    pub submit_action: String,
    pub cancel_href: String,
}

#[derive(Template)]
#[template(path = "account/reactivate.html")]
pub struct ReactivateAccountPage {
    pub shell: PageShell,
    pub nav: AccountNav,
    pub error: Option<String>,
    pub submit_action: String,
    pub cancel_href: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeleteStep {
    /// Offer to mail the confirmation code
    Request,
    /// The code and the password
    Confirm,
    /// The last word
    FinalConfirm,
}

#[derive(Template)]
#[template(path = "account/delete.html")]
pub struct DeletePage {
    pub shell: PageShell,
    pub nav: AccountNav,
    pub step: DeleteStep,
    pub email: Option<String>,
    pub error: Option<String>,
    /// The mailed code, carried by the final form
    pub code: String,
    /// The signed attestation that the code and password were checked
    pub intent: String,
    pub request_action: String,
    pub verify_action: String,
    pub confirm_action: String,
    pub cancel_href: String,
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
    fn manage_page_rows_and_notices() {
        let rows = vec![
            SettingRow {
                href: "/m/email".into(),
                title: "Email address",
                value: "alice@example.com".into(),
                icon: "mail",
                destructive: false,
            },
            SettingRow {
                href: "/m/delete".into(),
                title: "Delete account",
                value: String::new(),
                icon: "trash",
                destructive: true,
            },
        ];
        let mut page = ManagePage {
            shell: shell(),
            nav: nav(Section::Manage),
            unverified_email: Some("alice@example.com".into()),
            verify_href: "/m/email/verify".into(),
            deactivated: false,
            rows,
            notice: Some("Your password has been updated.".into()),
        };
        let html = page.render().unwrap();
        assert!(html.contains("<h2>Account</h2>"));
        assert!(html.contains("Your email address needs to be verified."));
        assert!(html.contains("href=\"/m/email/verify\">Verify now</a>"));
        assert!(html.contains("Your password has been updated."));
        assert!(html.contains("alice@example.com"));
        assert!(html.contains("class=\"item destructive\""));
        page.unverified_email = None;
        page.notice = None;
        page.deactivated = true;
        let html = page.render().unwrap();
        assert!(!html.contains("Verify now"));
        assert!(html.contains("Your account is deactivated."));
    }

    #[test]
    fn email_page_steps() {
        let mut page = EmailPage {
            shell: shell(),
            nav: nav(Section::Manage),
            step: EmailStep::Choose,
            current_email: Some("alice@example.com".into()),
            new_email: String::new(),
            error: None,
            request_action: "/m/email/request".into(),
            confirm_action: "/m/email/confirm".into(),
            verify_request_action: "/m/email/verify/request".into(),
            verify_action: "/m/email/verify".into(),
            verify_code_href: "/m/email/verify?step=code".into(),
            cancel_href: "/m".into(),
        };
        let html = page.render().unwrap();
        assert!(html.contains("Update your email"));
        assert!(html.contains("Your account currently uses <b>alice@example.com</b>. Choose a new email address to associate with it."));
        assert!(html.contains("name=\"new_email\""));
        assert!(html.contains("action=\"/m/email/request\""));
        page.current_email = None;
        assert!(page
            .render()
            .unwrap()
            .contains("Choose a new email address to associate with your account."));

        page.step = EmailStep::Token;
        page.new_email = "new@example.com".into();
        page.error = Some("Token is invalid".into());
        let html = page.render().unwrap();
        assert!(html.contains("Security step required"));
        assert!(
            html.contains("Please enter the security code that was sent to your email address.")
        );
        assert!(html.contains("name=\"new_email\" value=\"new@example.com\""));
        assert!(html.contains("name=\"code\""));
        assert!(html.contains("action=\"/m/email/confirm\""));
        assert!(html.contains("class=\"error\">Token is invalid"));

        page.step = EmailStep::VerifyRequest;
        let html = page.render().unwrap();
        assert!(html.contains("Verify your email"));
        assert!(html.contains("security code sent to <b>new@example.com</b>"));
        assert!(html.contains("Send verification code"));
        assert!(html.contains("href=\"/m/email/verify?step=code\">Already have a code?</a>"));

        page.step = EmailStep::Verify;
        let html = page.render().unwrap();
        assert!(html.contains("Verification code"));
        assert!(html.contains("action=\"/m/email/verify\""));
    }

    #[test]
    fn handle_page_modes() {
        let mut page = HandlePage {
            shell: shell(),
            nav: nav(Section::Manage),
            mode: HandleMode::Choose,
            did: "did:plc:alice".into(),
            segment: "alice".into(),
            domains: vec![".pds.test".into(), ".other.test".into()],
            selected_domain: ".pds.test".into(),
            custom: String::new(),
            error: None,
            default_href: "/m/handle?mode=default".into(),
            custom_href: "/m/handle?mode=custom".into(),
            submit_action: "/m/handle".into(),
            cancel_href: "/m".into(),
            mailto_href: HandlePage::instructions_mailto("alice.com", "did:plc:alice"),
        };
        let html = page.render().unwrap();
        assert!(html.contains("Update your username"));
        assert!(html.contains("Use a default username"));
        assert!(html.contains("<em>alice.pds.test</em>"));
        assert!(html.contains("Use a domain name I own"));

        page.mode = HandleMode::Default;
        let html = page.render().unwrap();
        assert!(html.contains("Choose a new default username."));
        assert!(html.contains("name=\"handle\" value=\"alice\""));
        assert!(html.contains("name=\"domain\" value=\".pds.test\" checked"));
        assert!(html.contains("name=\"domain\" value=\".other.test\""));
        assert!(html.contains("Use 3–18 letters, numbers or hyphens"));
        page.domains = vec![".pds.test".into()];
        let html = page.render().unwrap();
        assert!(html.contains("type=\"hidden\" name=\"domain\" value=\".pds.test\""));

        page.mode = HandleMode::Custom;
        page.custom = "alice.com".into();
        page.error = Some("Handle already taken".into());
        let html = page.render().unwrap();
        assert!(html.contains(
            "Update your username to a domain name you own to self-verify your identity."
        ));
        assert!(html.contains("_atproto.alice.com"));
        assert!(html.contains("did=did:plc:alice"));
        assert!(html.contains("https://alice.com/.well-known/atproto-did"));
        assert!(html.contains("Email these instructions"));
        assert!(html.contains("mailto:?body=Hello"));
        assert!(html.contains("Verify and Save"));
        assert!(html.contains("class=\"error\">Handle already taken"));
        page.custom = String::new();
        let html = page.render().unwrap();
        assert!(html.contains("_atproto.&lt;your-domain&gt;"));
        assert!(!html.contains("Email these instructions"));
    }

    #[test]
    fn password_page_steps() {
        let mut page = PasswordPage {
            shell: shell(),
            nav: nav(Section::Manage),
            step: PasswordStep::Request,
            error: None,
            request_action: "/m/password/request".into(),
            confirm_action: "/m/password".into(),
            code_href: "/m/password?step=code".into(),
            cancel_href: "/m".into(),
        };
        let html = page.render().unwrap();
        assert!(html.contains("Change your password"));
        assert!(html.contains(
            "To change your password, you'll need to enter a security code sent to your email."
        ));
        assert!(html.contains("Send verification code"));
        page.step = PasswordStep::Confirm;
        page.error = Some("Token is expired".into());
        let html = page.render().unwrap();
        assert!(html.contains("name=\"code\""));
        assert!(html.contains("name=\"current_password\""));
        assert!(html.contains("name=\"password\""));
        assert!(html.contains("minlength=\"8\""));
        assert!(html.contains("class=\"error\">Token is expired"));
    }

    #[test]
    fn deactivate_reactivate_and_delete_pages() {
        let mut page = DeactivatePage {
            shell: shell(),
            nav: nav(Section::Manage),
            error: None,
            submit_action: "/m/deactivate".into(),
            cancel_href: "/m".into(),
        };
        let html = page.render().unwrap();
        assert!(html.contains("hidden from the Blacksky app and across the Atmosphere network."));
        assert!(
            html.contains("There is no time limit for account deactivation, come back any time.")
        );
        assert!(html.contains("app passwords"));
        assert!(html.contains(
            "If you're trying to change your handle or email, do so before you deactivate."
        ));
        assert!(html.contains("name=\"password\""));
        assert!(html.contains(">Yes, Deactivate</button>"));
        page.error = Some("Invalid password".into());
        page.shell = PageShell::new(&Branding::default(), "https://pds.test", "pds.test");
        let html = page.render().unwrap();
        assert!(html.contains("hidden across the Atmosphere network."));
        assert!(html.contains("class=\"error\">Invalid password"));

        let mut page = ReactivateAccountPage {
            shell: shell(),
            nav: nav(Section::Manage),
            error: Some("Something went wrong".into()),
            submit_action: "/m/reactivate".into(),
            cancel_href: "/m".into(),
        };
        let html = page.render().unwrap();
        assert!(html.contains("that includes the Blacksky app and any other Atmosphere app"));
        assert!(html.contains("You can deactivate your account again at any time from this page."));
        assert!(html.contains(">Reactivate</button>"));
        assert!(html.contains("class=\"error\">Something went wrong"));
        page.shell = PageShell::new(&Branding::default(), "https://pds.test", "pds.test");
        assert!(page
            .render()
            .unwrap()
            .contains("visible again across the Atmosphere network."));

        let mut page = DeletePage {
            shell: shell(),
            nav: nav(Section::Manage),
            step: DeleteStep::Request,
            email: Some("alice@example.com".into()),
            error: None,
            code: String::new(),
            intent: String::new(),
            request_action: "/m/delete/request".into(),
            verify_action: "/m/delete/verify".into(),
            confirm_action: "/m/delete/confirm".into(),
            cancel_href: "/m".into(),
        };
        let html = page.render().unwrap();
        assert!(html.contains("Delete account <b>@alice.test</b>"));
        assert!(html
            .contains("send a confirmation code to your email address <b>alice@example.com</b>."));
        assert!(html.contains("no longer be visible to other Blacksky users."));
        assert!(html.contains(">Send email</button>"));
        page.email = None;
        page.shell = PageShell::new(&Branding::default(), "https://pds.test", "pds.test");
        let html = page.render().unwrap();
        assert!(html.contains("send a confirmation code to your email address."));
        assert!(html.contains("no longer be visible to other users."));
        page.shell = shell();
        page.email = Some("alice@example.com".into());

        page.step = DeleteStep::Confirm;
        page.error = Some("Invalid did or password".into());
        let html = page.render().unwrap();
        assert!(html.contains("Check <b>alice@example.com</b> for an email with the confirmation code to enter below:"));
        assert!(html.contains("name=\"code\""));
        assert!(html.contains("name=\"password\""));
        assert!(html.contains(">Delete my account</button>"));
        assert!(html.contains("class=\"error\">Invalid did or password"));
        page.email = None;
        assert!(page
            .render()
            .unwrap()
            .contains("Check your email for the confirmation code to enter below:"));

        page.step = DeleteStep::FinalConfirm;
        page.code = "CODE-1".into();
        page.intent = "intent.exp".into();
        let html = page.render().unwrap();
        assert!(html.contains("Are you really, really sure?"));
        assert!(html.contains(
            "irreversibly delete your Blacksky account <b>@alice.test</b> and all associated data."
        ));
        assert!(html.contains("name=\"code\" value=\"CODE-1\""));
        assert!(html.contains("name=\"delete_intent\" value=\"intent.exp\""));
        assert!(html.contains(">Yes, delete my account</button>"));
        assert!(html.contains("autocomplete=\"off\""));
        page.shell = PageShell::new(&Branding::default(), "https://pds.test", "pds.test");
        assert!(page
            .render()
            .unwrap()
            .contains("irreversibly delete your account <b>@alice.test</b>"));
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
