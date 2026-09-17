//! Writes every browser page, in each of its variants, as static HTML with
//! sample branding so the screens can be compared with the reference ones.
//!
//! Run with: `cargo run -p rsky-pds --example render_ui -- <out_dir>`

use askama::Template;
use rsky_pds::ui::branding::{Branding, RgbColor};
use rsky_pds::ui::client::{display_for, ClientView};
use rsky_pds::ui::pages::oauth::{
    ConsentPage, CookieErrorPage, ErrorPage, ReactivatePage, SignInPage, SignInView, WelcomePage,
};
use rsky_pds::ui::pages::AccountCardView;
use rsky_pds::ui::scopes::{permission_groups, IncludeSetView};
use rsky_pds::ui::shell::PageShell;
use rsky_pds::ui::technical::technical_items;
use std::collections::BTreeMap;

const CLIENT_ID: &str = "https://blacksky.community/oauth-client-metadata.json";
const REQUEST_URI: &str = "urn:ietf:params:oauth:request_uri:req-2c9f1a";

fn shell() -> PageShell {
    let branding = Branding {
        service_name: "Blacksky".to_string(),
        logo_url: Some("https://blacksky.community/static/logo.svg".to_string()),
        primary: RgbColor::parse("#6060E9").ok(),
        home_url: Some("https://www.blackskyweb.xyz/".to_string()),
        terms_of_service_url: Some("https://www.blackskyweb.xyz/terms".to_string()),
        privacy_policy_url: Some("https://www.blackskyweb.xyz/privacy".to_string()),
        support_url: Some("https://go.blacksky.app/support".to_string()),
        app_name: Some("Blacksky".to_string()),
        app_url: Some("https://blacksky.community".to_string()),
        org_name: Some("Blacksky Algorithms".to_string()),
        ..Branding::default()
    };
    PageShell::new(&branding, "https://blacksky.app", "blacksky.app")
}

fn client(trusted: bool) -> ClientView {
    ClientView {
        id: CLIENT_ID.to_string(),
        display: display_for(CLIENT_ID, Some("Blacksky"), trusted),
        logo_uri: trusted.then(|| "https://blacksky.community/static/logo.svg".to_string()),
        trusted,
        tos_uri: Some("https://blacksky.community/terms".to_string()),
        policy_uri: Some("https://blacksky.community/privacy".to_string()),
    }
}

fn alice() -> AccountCardView {
    AccountCardView {
        did: "did:plc:qz3x7k2j9m4n8p1r5s6t7u8v".to_string(),
        handle: "@alice.blacksky.app".to_string(),
        ..AccountCardView::default()
    }
}

fn sign_in(view: SignInView, client: ClientView) -> SignInPage {
    SignInPage {
        shell: shell(),
        view,
        subtitle: SignInPage::subtitle_for(view).to_string(),
        client,
        client_id: CLIENT_ID.to_string(),
        request_uri: REQUEST_URI.to_string(),
        csrf: "csrf-demo-token".to_string(),
        identifier: String::new(),
        identifier_readonly: false,
        error: None,
        otp_hint: None,
        otp_error: false,
        show_remember: true,
        remember_checked: false,
        submit_label: "Sign in".to_string(),
        sessions: vec![
            alice(),
            AccountCardView {
                did: "did:plc:bob".to_string(),
                handle: "@bob.blacksky.app".to_string(),
                login_required: true,
                ..AccountCardView::default()
            },
        ],
        show_picker: matches!(view, SignInView::Picker),
        sign_in_action: "/oauth/authorize/sign-in".to_string(),
        select_action: "/oauth/authorize/select".to_string(),
        another_account_href: "/oauth/authorize?view=sign-in".to_string(),
        signup_href: Some("https://blacksky.app/gate/signup".to_string()),
        forgot_href: Some("/account/reset-password".to_string()),
        back_href: "/oauth/authorize".to_string(),
        back_label: "Back".to_string(),
    }
}

fn consent(trusted: bool, scopes: &[&str]) -> ConsentPage {
    let scopes: Vec<String> = scopes.iter().map(|s| s.to_string()).collect();
    let mut sets = BTreeMap::new();
    sets.insert(
        "app.bsky.authFull".to_string(),
        IncludeSetView {
            title: Some("Full access to the social app".to_string()),
            detail: Some("Everything the app needs".to_string()),
            scopes: vec!["repo:app.bsky.feed.post".to_string()],
        },
    );
    let grouping = permission_groups(&scopes, &sets, trusted, Some("Blacksky"));
    ConsentPage {
        shell: shell(),
        client: client(trusted),
        client_id: CLIENT_ID.to_string(),
        request_uri: REQUEST_URI.to_string(),
        csrf: "csrf-demo-token".to_string(),
        account: alice(),
        scope_raw: scopes.join(" "),
        only_atproto: grouping.only_atproto,
        email_optional: grouping.can_drop_email,
        identity_warning: grouping.identity_warning,
        groups: grouping.groups,
        technical: technical_items(&scopes),
        session_token: None,
        accept_action: "/oauth/authorize/accept".to_string(),
        reject_action: "/oauth/authorize/reject".to_string(),
        back_href: "/oauth/authorize".to_string(),
    }
}

fn main() {
    let out_dir = std::env::args().nth(1).unwrap_or_else(|| ".".to_string());
    let write = |name: &str, html: askama::Result<String>| {
        std::fs::write(
            format!("{out_dir}/{name}.html"),
            html.expect("page renders"),
        )
        .expect("write page");
    };

    write(
        "sign_in_form",
        sign_in(SignInView::Form, client(true)).render(),
    );
    let mut page = sign_in(SignInView::Form, client(false));
    page.error = Some("Invalid identifier or password".to_string());
    write("sign_in_form_error", page.render());
    let mut page = sign_in(SignInView::ForcedIdentifier, client(true));
    page.identifier = "alice.blacksky.app".to_string();
    page.identifier_readonly = true;
    page.otp_hint = Some("a***@example.test".to_string());
    page.submit_label = "Confirm".to_string();
    write("sign_in_otp", page.render());
    write(
        "sign_in_picker",
        sign_in(SignInView::Picker, client(true)).render(),
    );
    let mut page = sign_in(SignInView::ConfirmSelected, client(true));
    page.identifier = "alice.blacksky.app".to_string();
    page.identifier_readonly = true;
    page.submit_label = "Confirm".to_string();
    write("sign_in_confirm", page.render());

    write(
        "consent_trusted",
        consent(true, &["atproto", "transition:generic", "transition:email"]).render(),
    );
    write(
        "consent_untrusted",
        consent(
            false,
            &[
                "atproto",
                "identity:*",
                "account:email?action=manage",
                "repo:app.bsky.feed.post?action=create&action=update",
                "repo:app.bsky.graph.follow",
                "blob:?accept=image/*&accept=video/*",
                "rpc:app.bsky.notification.registerPush?aud=did:web:push.example.com",
                "include:app.bsky.authFull",
                "space:app.bulleted.space?authority=*&action=read&action=create&collection=app.bulleted.note",
            ],
        )
        .render(),
    );
    write("consent_identify", consent(false, &["atproto"]).render());

    write(
        "welcome",
        WelcomePage {
            shell: shell(),
            csrf: "csrf-demo-token".to_string(),
            client_id: CLIENT_ID.to_string(),
            request_uri: REQUEST_URI.to_string(),
            signup_href: "https://blacksky.app/gate/signup".to_string(),
            sign_in_href: "/oauth/authorize?view=sign-in".to_string(),
            cancel_action: "/oauth/authorize/reject".to_string(),
        }
        .render(),
    );
    write(
        "reactivate",
        ReactivatePage {
            shell: shell(),
            csrf: "csrf-demo-token".to_string(),
            client_id: CLIENT_ID.to_string(),
            request_uri: REQUEST_URI.to_string(),
            account: alice(),
            session_token: None,
            error: None,
            reactivate_action: "/oauth/authorize/reactivate".to_string(),
            cancel_action: "/oauth/authorize/reject".to_string(),
        }
        .render(),
    );
    write(
        "cookie_error",
        CookieErrorPage {
            shell: shell(),
            cookie_message: CookieErrorPage::message("blacksky.app"),
            continue_action: "/oauth/authorize".to_string(),
            continue_params: vec![
                ("client_id".to_string(), CLIENT_ID.to_string()),
                ("request_uri".to_string(), REQUEST_URI.to_string()),
                ("redirect-test".to_string(), "1".to_string()),
            ],
        }
        .render(),
    );
    write(
        "error",
        ErrorPage::new(shell(), "This authorization request has expired.").render(),
    );
    write("not_found", ErrorPage::not_found(shell()).render());
}
