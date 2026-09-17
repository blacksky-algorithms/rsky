//! The authorization flow screens.

use super::AccountCardView;
use crate::ui::client::{ClientDisplay, ClientView};
use crate::ui::scopes::PermissionGroup;
use crate::ui::shell::PageShell;
use crate::ui::technical::TechnicalItem;
use askama::Template;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignInView {
    /// Existing sessions on this device to pick from
    Picker,
    /// The plain credentials form
    Form,
    /// The form with the identifier fixed by a login hint
    ForcedIdentifier,
    /// The form for a picked session that must confirm its password
    ConfirmSelected,
}

#[derive(Template)]
#[template(path = "oauth/sign_in.html")]
pub struct SignInPage {
    pub shell: PageShell,
    pub view: SignInView,
    pub subtitle: String,
    pub client: ClientView,
    pub client_id: String,
    pub request_uri: String,
    pub csrf: String,
    pub identifier: String,
    pub identifier_readonly: bool,
    pub error: Option<String>,
    pub otp_hint: Option<String>,
    pub otp_error: bool,
    pub show_remember: bool,
    pub remember_checked: bool,
    pub submit_label: String,
    pub sessions: Vec<AccountCardView>,
    pub show_picker: bool,
    pub sign_in_action: String,
    pub select_action: String,
    pub another_account_href: String,
    pub signup_href: Option<String>,
    pub forgot_href: Option<String>,
    pub back_href: String,
    pub back_label: String,
}

impl SignInPage {
    pub fn subtitle_for(view: SignInView) -> &'static str {
        match view {
            SignInView::Picker => "Select from an existing account",
            SignInView::Form => "Enter your username and password",
            SignInView::ForcedIdentifier => "Enter your password",
            SignInView::ConfirmSelected => "Confirm your password to continue",
        }
    }
}

#[derive(Template)]
#[template(path = "oauth/welcome.html")]
pub struct WelcomePage {
    pub shell: PageShell,
    pub csrf: String,
    pub client_id: String,
    pub request_uri: String,
    pub signup_href: String,
    pub sign_in_href: String,
    pub cancel_action: String,
}

#[derive(Template)]
#[template(path = "oauth/consent.html")]
pub struct ConsentPage {
    pub shell: PageShell,
    pub client: ClientView,
    pub client_id: String,
    pub request_uri: String,
    pub csrf: String,
    pub account: AccountCardView,
    pub scope_raw: String,
    pub only_atproto: bool,
    /// The page offers to decline the email grant
    pub email_optional: bool,
    pub groups: Vec<PermissionGroup>,
    pub identity_warning: bool,
    pub technical: Vec<TechnicalItem>,
    pub session_token: Option<String>,
    pub accept_action: String,
    pub reject_action: String,
    pub back_href: String,
}

#[derive(Template)]
#[template(path = "oauth/reactivate.html")]
pub struct ReactivatePage {
    pub shell: PageShell,
    pub csrf: String,
    pub client_id: String,
    pub request_uri: String,
    pub account: AccountCardView,
    pub session_token: Option<String>,
    pub error: Option<String>,
    pub reactivate_action: String,
    pub cancel_action: String,
}

#[derive(Template)]
#[template(path = "oauth/cookie_error.html")]
pub struct CookieErrorPage {
    pub shell: PageShell,
    pub cookie_message: String,
    pub continue_action: String,
    pub continue_params: Vec<(String, String)>,
}

impl CookieErrorPage {
    pub fn message(hostname: &str) -> String {
        format!(
            "It seems that your browser is not accepting cookies. Press \"Continue\" to try again. If the error persists, please ensure that your privacy settings allow cookies for the \"{hostname}\" website."
        )
    }
}

#[derive(Template)]
#[template(path = "oauth/error.html")]
pub struct ErrorPage {
    pub shell: PageShell,
    pub title: String,
    pub message: String,
    pub back_href: String,
}

impl ErrorPage {
    pub fn new(shell: PageShell, message: impl Into<String>) -> Self {
        ErrorPage {
            shell,
            title: "An error occurred".to_string(),
            message: message.into(),
            back_href: String::new(),
        }
    }

    pub fn not_found(shell: PageShell) -> Self {
        ErrorPage {
            shell,
            title: "Page not found".to_string(),
            message: "The page you asked for does not exist.".to_string(),
            back_href: "/account".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::branding::Branding;
    use crate::ui::client::display_for;
    use std::collections::BTreeMap;

    fn shell() -> PageShell {
        let branding = Branding {
            service_name: "Example PDS".into(),
            logo_url: Some("https://x.test/logo.svg".into()),
            home_url: Some("https://home.test".into()),
            app_name: Some("Blacksky".into()),
            ..Branding::default()
        };
        PageShell::new(&branding, "https://pds.test", "pds.test")
    }

    fn client(trusted: bool) -> ClientView {
        ClientView {
            id: "https://app.example/oauth-client-metadata.json".into(),
            display: display_for(
                "https://app.example/oauth-client-metadata.json",
                Some("Example App"),
                trusted,
            ),
            logo_uri: trusted.then(|| "https://app.example/logo.png".to_string()),
            trusted,
            tos_uri: Some("https://app.example/tos".into()),
            policy_uri: None,
        }
    }

    fn alice() -> AccountCardView {
        AccountCardView {
            did: "did:plc:alice".into(),
            handle: "@alice.test".into(),
            ..AccountCardView::default()
        }
    }

    fn sign_in(view: SignInView) -> SignInPage {
        SignInPage {
            shell: shell(),
            view,
            subtitle: SignInPage::subtitle_for(view).to_string(),
            client: client(true),
            client_id: "https://app.example/oauth-client-metadata.json".into(),
            request_uri: "urn:ietf:params:oauth:request_uri:req-1".into(),
            csrf: "csrf-token".into(),
            identifier: String::new(),
            identifier_readonly: false,
            error: None,
            otp_hint: None,
            otp_error: false,
            show_remember: true,
            remember_checked: false,
            submit_label: "Sign in".into(),
            sessions: vec![alice()],
            show_picker: matches!(view, SignInView::Picker),
            sign_in_action: "/oauth/authorize/sign-in".into(),
            select_action: "/oauth/authorize/select".into(),
            another_account_href: "/oauth/authorize?x&view=sign-in".into(),
            signup_href: Some("https://signup.test".into()),
            forgot_href: Some("/account/reset-password".into()),
            back_href: String::new(),
            back_label: "Back".into(),
        }
    }

    #[test]
    fn sign_in_form_carries_the_contract_and_copy() {
        let html = sign_in(SignInView::Form).render().unwrap();
        assert!(html.contains("<title>Sign in</title>"));
        assert!(html.contains("Enter your username and password"));
        assert!(html.contains("name=\"csrf\" value=\"csrf-token\""));
        assert!(html.contains("name=\"client_id\""));
        assert!(html.contains("name=\"request_uri\""));
        assert!(html.contains("name=\"identifier\""));
        assert!(html.contains("name=\"password\""));
        assert!(html.contains("name=\"remember\""));
        assert!(html.contains("Remember this account on this device"));
        assert!(html.contains("Verify the website address before entering your password"));
        assert!(html.contains("Forgot?"));
        assert!(!html.contains("name=\"scope\""));
        assert!(html.contains("Create a new account"));
        assert!(!html.contains("name=\"email_otp\""));
        assert!(html.contains("Example PDS"));
        assert!(html.contains("https://x.test/logo.svg"));
        assert!(html.contains("rel=\"canonical\" href=\"https://home.test\""));
        assert!(html.contains(">Home</a>"));
        assert!(html.contains("<meta name=\"robots\" content=\"noindex\">"));
        assert!(!html.contains("Bluesky"));
    }

    #[test]
    fn sign_in_variants() {
        let mut page = sign_in(SignInView::ForcedIdentifier);
        page.identifier = "alice.test".into();
        page.identifier_readonly = true;
        page.otp_hint = Some("a***@example.test".into());
        page.submit_label = "Confirm".into();
        page.error = Some("The sign-in code was not accepted".into());
        page.remember_checked = true;
        let html = page.render().unwrap();
        assert!(html.contains("Enter your password"));
        assert!(html.contains("value=\"alice.test\""));
        assert!(html.contains("readonly"));
        assert!(html.contains("name=\"email_otp\""));
        assert!(html.contains("Check your a***@example.test email for a login code"));
        assert!(html.contains(">Confirm</button>"));
        assert!(html.contains("class=\"error\">The sign-in code was not accepted"));
        assert!(html.contains("value=\"on\" checked"));

        let mut page = sign_in(SignInView::Form);
        page.otp_error = true;
        page.show_remember = false;
        page.signup_href = None;
        page.forgot_href = None;
        let html = page.render().unwrap();
        assert!(!html.contains("Forgot?"));
        assert!(html.contains("name=\"email_otp\""));
        assert!(html.contains("Enter the code from your email."));
        assert!(!html.contains("name=\"remember\""));
        assert!(!html.contains("Create a new account"));

        let html = sign_in(SignInView::ConfirmSelected).render().unwrap();
        assert!(html.contains("Confirm your password to continue"));
    }

    #[test]
    fn picker_lists_sessions_and_the_other_account_entry() {
        let mut page = sign_in(SignInView::Picker);
        page.sessions[0].login_required = true;
        page.back_href = "/back".into();
        page.back_label = "Cancel".into();
        let html = page.render().unwrap();
        assert!(html.contains("Select from an existing account"));
        assert!(html.contains("Sign in as..."));
        assert!(html.contains("aria-label=\"Sign in as @alice.test\""));
        assert!(html.contains("action=\"/oauth/authorize/select\""));
        assert!(html.contains("name=\"did\" value=\"did:plc:alice\""));
        assert!(html.contains("Login required"));
        assert!(html.contains("Another account"));
        assert!(html.contains(">Sign up</a>"));
        assert!(html.contains(">Cancel</a>"));
        assert!(!html.contains("name=\"password\""));
    }

    fn consent(trusted: bool) -> ConsentPage {
        let grouping = crate::ui::scopes::permission_groups(
            &[
                "atproto".to_string(),
                "transition:generic".to_string(),
                "identity:*".to_string(),
            ],
            &BTreeMap::new(),
            false,
            Some("Blacksky"),
        );
        ConsentPage {
            shell: shell(),
            client: client(trusted),
            client_id: "https://app.example/oauth-client-metadata.json".into(),
            request_uri: "urn:ietf:params:oauth:request_uri:req-1".into(),
            csrf: "csrf-token".into(),
            account: alice(),
            scope_raw: "atproto transition:generic identity:*".into(),
            only_atproto: false,
            email_optional: grouping.can_drop_email,
            identity_warning: grouping.identity_warning,
            groups: grouping.groups,
            technical: vec![TechnicalItem {
                scope: "atproto".into(),
                title: "Confirm your identity".into(),
                detail: Some("Lets the app know who you are.".into()),
            }],
            session_token: Some("ephemeral".into()),
            accept_action: "/oauth/authorize/accept".into(),
            reject_action: "/oauth/authorize/reject".into(),
            back_href: "/oauth/authorize?x".into(),
        }
    }

    #[test]
    fn consent_shows_grouped_permissions_and_the_reference_copy() {
        let html = consent(true).render().unwrap();
        assert!(html.contains("<title>Authorize</title>"));
        assert!(html.contains("Grant access to your <b>@alice.test</b> account"));
        assert!(html.contains("Example App"));
        assert!(html.contains("https://app.example/logo.png"));
        assert!(html.contains("wants to access your <b>@alice.test</b> account"));
        assert!(html.contains("Technical details"));
        assert!(html.contains("<pre class=\"text-xs\">atproto transition:generic identity:*</pre>"));
        assert!(html.contains("Confirm your identity"));
        assert!(html.contains("<b>Blacksky</b>") || html.contains("Blacksky</div>"));
        assert!(html.contains("Manage your profile, posts, likes and follows"));
        assert!(html.contains("permanently break"));
        assert!(html.contains("By clicking <b>Authorize</b>"));
        assert!(html.contains("href=\"https://app.example/tos\""));
        assert!(html.contains(">Authorize</button>"));
        assert!(html.contains(">Deny access</button>"));
        assert!(html.contains(">Back</a>"));
        assert!(html.contains("name=\"session_token\" value=\"ephemeral\""));
        assert!(html.contains("name=\"scope\" value=\"atproto transition:generic identity:*\""));
        assert!(!html.contains("name=\"email_optional\""));
        assert!(html.contains("action=\"/oauth/authorize/reject\""));
        assert!(!html.contains("Bluesky"));

        let mut page = consent(false);
        page.only_atproto = true;
        page.groups.clear();
        page.identity_warning = false;
        page.session_token = None;
        page.back_href = String::new();
        let html = page.render().unwrap();
        assert!(
            html.contains("wants to uniquely identify you through your <b>@alice.test</b> account")
        );
        assert!(html.contains("avatar-brand"));
        assert!(!html.contains("app.example/logo.png"));
        assert!(html.contains("app.example</div>"));
        assert!(!html.contains("permanently break"));
        assert!(!html.contains("session_token"));
        assert!(!html.contains(">Back</a>"));
    }

    #[test]
    fn consent_renders_every_client_display_shape() {
        for display in [
            ClientDisplay::LocalApp,
            ClientDisplay::Url {
                proto: "https://".into(),
                host: "h.test:8443".into(),
                rest: "/meta.json?v=1".into(),
            },
            ClientDisplay::Raw("urn:x".into()),
        ] {
            let mut page = consent(false);
            page.client.display = display.clone();
            let html = page.render().unwrap();
            match display {
                ClientDisplay::LocalApp => assert!(html.contains("An application on your device")),
                ClientDisplay::Url { .. } => {
                    assert!(html.contains("url-host\">h.test:8443</span>"))
                }
                _ => assert!(html.contains("urn:x")),
            }
        }
    }

    #[test]
    fn welcome_reactivate_cookie_and_error_pages() {
        let html = WelcomePage {
            shell: shell(),
            csrf: "c".into(),
            client_id: "cid".into(),
            request_uri: "req".into(),
            signup_href: "/oauth/authorize?view=sign-up".into(),
            sign_in_href: "/oauth/authorize?view=sign-in".into(),
            cancel_action: "/oauth/authorize/reject".into(),
        }
        .render()
        .unwrap();
        assert!(html.contains("<title>Authenticate</title>"));
        assert!(html.contains(">Welcome</h1>"));
        assert!(html.contains("Please authenticate to continue"));
        assert!(html.contains("Create a new account"));
        assert!(html.contains(">Cancel</button>"));

        let html = ReactivatePage {
            shell: shell(),
            csrf: "c".into(),
            client_id: "cid".into(),
            request_uri: "req".into(),
            account: alice(),
            session_token: Some("t".into()),
            error: Some("nope".into()),
            reactivate_action: "/oauth/authorize/reactivate".into(),
            cancel_action: "/oauth/authorize/reject".into(),
        }
        .render()
        .unwrap();
        assert!(html.contains("Welcome back!"));
        assert!(html.contains("Your account is currently deactivated."));
        assert!(html.contains("You previously deactivated <b>@alice.test</b>"));
        assert!(html.contains("Yes, reactivate my account"));
        assert!(html.contains("name=\"session_token\" value=\"t\""));
        assert!(html.contains("class=\"error\">nope"));

        let html = CookieErrorPage {
            shell: shell(),
            cookie_message: CookieErrorPage::message("pds.test"),
            continue_action: "/oauth/authorize".into(),
            continue_params: vec![
                ("client_id".into(), "cid".into()),
                ("redirect-test".into(), "1".into()),
            ],
        }
        .render()
        .unwrap();
        assert!(html.contains("Cookie Error"));
        assert!(
            html.contains("allow cookies for the &quot;pds.test&quot; website")
                || html.contains("allow cookies for the \"pds.test\" website")
        );
        assert!(html.contains("name=\"redirect-test\" value=\"1\""));
        assert!(html.contains(">Continue</button>"));

        let html = ErrorPage::new(shell(), "client_id and request_uri are required")
            .render()
            .unwrap();
        assert!(html.contains("An error occurred"));
        assert!(html.contains("class=\"error\">client_id and request_uri are required"));
        assert!(!html.contains(">Back</a>"));
        let html = ErrorPage::not_found(shell()).render().unwrap();
        assert!(html.contains("Page not found"));
        assert!(html.contains("href=\"/account\">Back</a>"));
    }

    #[test]
    fn account_card_view_from_account_info() {
        let info = rsky_oauth::store::AccountInfo {
            did: "did:plc:x".into(),
            handle: Some("x.test".into()),
            email: None,
            email_verified: false,
            deactivated: true,
        };
        let view = AccountCardView::from_account(&info, true);
        assert_eq!(view.handle, "@x.test");
        assert!(view.login_required && view.deactivated);
        let info = rsky_oauth::store::AccountInfo {
            handle: None,
            ..info
        };
        assert_eq!(
            AccountCardView::from_account(&info, false).handle,
            "did:plc:x"
        );
    }
}
