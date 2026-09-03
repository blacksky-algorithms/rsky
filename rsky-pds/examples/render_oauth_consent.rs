//! Standalone render harness for manual visual QA of the OAuth templates.
//!
//! Renders the real `ConsentPage`/`SignInPage`/`ErrorPage` Askama templates
//! (the exact production code path, including `scope_items`'s scope-grammar
//! parsing) with representative multi-scope sample data, and writes the
//! resulting HTML to disk so it can be opened in a browser -- there is no
//! running PDS server in this environment to click through the real flow.
//!
//! Run with: `cargo run -p rsky-pds --example render_oauth_consent -- <out_dir>`

use askama::Template;
use rsky_pds::oauth::templates::{scope_items, ConsentPage, ErrorPage, SessionOption, SignInPage};

fn main() {
    let out_dir = std::env::args().nth(1).unwrap_or_else(|| ".".to_string());

    let consent = ConsentPage {
        client_display: "Skylight Reader".to_string(),
        client_id: "https://skylight.example.com/oauth/client-metadata.json".to_string(),
        client_trusted: false,
        request_uri: "urn:ietf:params:oauth:request_uri:req-2c9f1a".to_string(),
        csrf: "csrf-demo-token".to_string(),
        did: "did:plc:qz3x7k2j9m4n8p1r5s6t7u8v".to_string(),
        account_label: "alice.example.social".to_string(),
        // A representative multi-scope grant covering every form this crate
        // can currently parse: the base scope, a legacy transition grant, a
        // collection- and action-narrowed repo grant, a wildcard repo grant,
        // a blob grant, a delegated rpc call, an unresolved include: set,
        // and a space: grant with mixed actions -- this is exactly the case
        // the raw-scope-string bug made illegible.
        scopes: scope_items(&[
            "atproto".to_string(),
            "transition:chat.bsky".to_string(),
            "repo:app.bsky.feed.post?action=create&action=update".to_string(),
            "repo:app.bsky.graph.follow".to_string(),
            "blob:image/*".to_string(),
            "rpc:app.bsky.notification.registerPush?aud=did:web:push.example.com".to_string(),
            "include:app.bsky.authFull".to_string(),
            "space:app.bulleted.space?authority=*&action=read&action=create\
             &collection=app.bulleted.note&collection=app.bulleted.outline"
                .to_string(),
        ]),
    };
    std::fs::write(
        format!("{out_dir}/oauth_consent_sample.html"),
        consent.render().expect("consent page renders"),
    )
    .expect("write consent sample");

    let signin = SignInPage {
        client_display: "Skylight Reader".to_string(),
        client_id: "https://skylight.example.com/oauth/client-metadata.json".to_string(),
        request_uri: "urn:ietf:params:oauth:request_uri:req-2c9f1a".to_string(),
        csrf: "csrf-demo-token".to_string(),
        login_hint: "alice.example.social".to_string(),
        error: Some("That handle or password didn't match.".to_string()),
        signup_url: Some("https://example.social/signup".to_string()),
        sessions: vec![SessionOption {
            did: "did:plc:qz3x7k2j9m4n8p1r5s6t7u8v".to_string(),
            label: "alice.example.social".to_string(),
        }],
    };
    std::fs::write(
        format!("{out_dir}/oauth_signin_sample.html"),
        signin.render().expect("signin page renders"),
    )
    .expect("write signin sample");

    let error = ErrorPage {
        message: "This authorization request has expired. Return to the app and try again."
            .to_string(),
    };
    std::fs::write(
        format!("{out_dir}/oauth_error_sample.html"),
        error.render().expect("error page renders"),
    )
    .expect("write error sample");

    println!("wrote oauth_{{consent,signin,error}}_sample.html to {out_dir}");
}
