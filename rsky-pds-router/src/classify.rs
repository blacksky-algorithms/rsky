//! What a request is and whose account it concerns, from its method,
//! path, query, headers, and (for mutations) its JSON body.

use crate::inventory::{self, Target};
use base64::Engine;
use http::{HeaderMap, Method};
use serde_json::Value;

/// The upstream pool a read belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadClass {
    /// `com.atproto.sync.*`
    Sync,
    /// `app.bsky.*` and `chat.bsky.*`
    Bsky,
    /// Everything else served by the main process: repo reads, sessions,
    /// well-known, custom routes.
    Main,
}

/// The kinds of requests the router tells apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// The router's own health.
    RouterHealth,
    /// Authorization-server protocol traffic and assets: pass-through.
    OauthAs,
    /// A token or revocation request on the authorization server, which
    /// carries a credential the router can resolve to the account it
    /// belongs to.
    OauthGrant,
    /// A read; `identity` is the account it concerns when one is named.
    Read {
        class: ReadClass,
        identity: Option<String>,
    },
    /// An XRPC procedure by NSID.
    Procedure {
        nsid: String,
        target: Option<Target>,
    },
    /// An OAuth UI API endpoint under `~api`.
    Api {
        endpoint: String,
        target: Option<Target>,
    },
    /// A mutation the inventory does not know.
    UnknownMutation,
}

pub const API_PREFIX: &str = "/@atproto/oauth-provider/~api/";

const OAUTH_AS_PREFIXES: &[&str] = &[
    "/oauth/",
    "/.well-known/oauth-",
    "/@atproto/oauth-provider/~assets/",
];

/// UI API endpoints that are authorization-server traffic or read-only,
/// which Caddy sends straight to the authorization server.
const API_AS_ENDPOINTS: &[&str] = &[
    "consent",
    "reject",
    "verify-handle-availability",
    "device-sessions",
    "oauth-sessions",
    "account-sessions",
];

fn is_read_method(method: &Method) -> bool {
    matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

fn query_param<'a>(query: Option<&'a str>, name: &str) -> Option<&'a str> {
    query?.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == name).then_some(value)
    })
}

/// The account a bearer token acts for, read without verification: the
/// `sub` of a session token, or the `iss` of a service token, which a
/// service (the video processor, for one) presents when it acts for the
/// account and which carries no `sub`. It only decides which backend
/// answers; the backend authenticates.
pub fn bearer_subject(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(http::header::AUTHORIZATION)?.to_str().ok()?;
    let token = value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("DPoP "))?;
    jwt_claim(token, "sub").or_else(|| jwt_claim(token, "iss"))
}

/// A string claim of a JWT's payload, read without verification.
pub fn jwt_claim(token: &str, claim: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let claims: Value = serde_json::from_slice(&bytes).ok()?;
    claims.get(claim)?.as_str().map(str::to_owned)
}

/// Decodes a percent-encoded query value.
fn decoded(value: &str) -> String {
    let mut out = Vec::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3])
                    .ok()
                    .and_then(|hex| u8::from_str_radix(hex, 16).ok());
                match hex {
                    Some(byte) => {
                        out.push(byte);
                        i += 3;
                        continue;
                    }
                    None => out.push(b'%'),
                }
            }
            b'+' => out.push(b' '),
            other => out.push(other),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// The account a read concerns, from the repository it names or the
/// bearer subject, or the request host for well-known lookups.
pub fn read_identity(path: &str, query: Option<&str>, headers: &HeaderMap) -> Option<String> {
    for name in ["did", "repo", "actor"] {
        if let Some(value) = query_param(query, name) {
            return Some(decoded(value));
        }
    }
    if path == "/.well-known/atproto-did" || path == "/custom-well-known-atproto-did" {
        if let Some(handle) = query_param(query, "handle") {
            return Some(decoded(handle));
        }
        return headers
            .get(http::header::HOST)
            .and_then(|host| host.to_str().ok())
            .map(|host| host.split(':').next().unwrap_or(host).to_owned());
    }
    bearer_subject(headers)
}

pub fn classify(method: &Method, path: &str, query: Option<&str>, headers: &HeaderMap) -> Kind {
    if path == "/xrpc/_health" {
        return Kind::RouterHealth;
    }
    if *method == Method::POST && (path == "/oauth/token" || path == "/oauth/revoke") {
        return Kind::OauthGrant;
    }
    if OAUTH_AS_PREFIXES
        .iter()
        .any(|prefix| path.starts_with(prefix))
    {
        return Kind::OauthAs;
    }
    if let Some(endpoint) = path.strip_prefix(API_PREFIX) {
        let endpoint = endpoint.split('?').next().unwrap_or(endpoint);
        if API_AS_ENDPOINTS.contains(&endpoint) {
            return Kind::OauthAs;
        }
        if is_read_method(method) {
            return Kind::Read {
                class: ReadClass::Main,
                identity: read_identity(path, query, headers),
            };
        }
        return Kind::Api {
            endpoint: endpoint.to_owned(),
            target: inventory::api_endpoint(endpoint),
        };
    }
    if is_read_method(method) {
        let class = match path.strip_prefix("/xrpc/") {
            Some(nsid) if nsid.starts_with("com.atproto.sync.") => ReadClass::Sync,
            Some(nsid) if nsid.starts_with("app.bsky.") || nsid.starts_with("chat.bsky.") => {
                ReadClass::Bsky
            }
            _ => ReadClass::Main,
        };
        return Kind::Read {
            class,
            identity: read_identity(path, query, headers),
        };
    }
    match path.strip_prefix("/xrpc/") {
        Some(nsid) if !nsid.is_empty() => Kind::Procedure {
            nsid: nsid.to_owned(),
            target: inventory::procedure(nsid),
        },
        _ => Kind::UnknownMutation,
    }
}

/// How a mutation target was found, or why it could not be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Attribution {
    /// The account the request mutates: a DID, a handle, or an email,
    /// resolved by the caller through the account tables.
    Identifier(String),
    /// Every account named, for a request that mutates several.
    Identifiers(Vec<String>),
    /// An email token to resolve through the account tables.
    EmailToken(String),
    /// The request mutates no account.
    None,
    /// The request must name an account and did not: a schema violation
    /// the reference answers 400 to.
    Malformed(&'static str),
    /// The request cannot be attributed while any canary exists.
    Unattributable(&'static str),
}

/// Applies a target rule to a parsed request.
pub fn attribute(target: Target, body: Option<&Value>, headers: &HeaderMap) -> Attribution {
    let field = |name: &str| body.and_then(|body| inventory::string_field(body, name));
    match target {
        Target::BodyRepo => field("repo")
            .map_or(Attribution::Malformed("repo is required"), |repo| {
                Attribution::Identifier(repo.to_owned())
            }),
        Target::AuthSubject => bearer_subject(headers).map_or(
            Attribution::Malformed("authentication is required"),
            Attribution::Identifier,
        ),
        Target::BodyDid => field("did").map_or(Attribution::Malformed("did is required"), |did| {
            Attribution::Identifier(did.to_owned())
        }),
        Target::BodyAccount => field("account")
            .map_or(Attribution::Malformed("account is required"), |account| {
                Attribution::Identifier(account.to_owned())
            }),
        Target::BodyRecipientDid => field("recipientDid")
            .map_or(Attribution::Malformed("recipientDid is required"), |did| {
                Attribution::Identifier(did.to_owned())
            }),
        Target::TokenOrEmailOrSubject => {
            if let Some(token) = field("token") {
                Attribution::EmailToken(token.to_owned())
            } else if let Some(email) = field("email") {
                Attribution::Identifier(email.to_owned())
            } else if let Some(subject) = bearer_subject(headers) {
                Attribution::Identifier(subject)
            } else {
                Attribution::Malformed("token, email, or authentication is required")
            }
        }
        Target::EmailOrToken => {
            if let Some(email) = field("email") {
                Attribution::Identifier(email.to_owned())
            } else if let Some(token) = field("token") {
                Attribution::EmailToken(token.to_owned())
            } else {
                Attribution::Malformed("email or token is required")
            }
        }
        Target::SubjectStatus => {
            let subject = body.and_then(|body| body.get("subject"));
            if let Some(did) = subject.and_then(|subject| inventory::string_field(subject, "did")) {
                Attribution::Identifier(did.to_owned())
            } else if let Some(uri) =
                subject.and_then(|subject| inventory::string_field(subject, "uri"))
            {
                inventory::at_uri_authority(uri).map_or(
                    Attribution::Malformed("subject.uri is not an at-uri"),
                    |did| Attribution::Identifier(did.to_owned()),
                )
            } else if subject
                .and_then(|subject| inventory::string_field(subject, "cid"))
                .is_some()
            {
                Attribution::Unattributable("a bare blob subject names no account")
            } else {
                Attribution::Malformed("subject is required")
            }
        }
        Target::Identifier => {
            if let Some(did) = headers
                .get("x-atproto-did")
                .and_then(|value| value.to_str().ok())
            {
                Attribution::Identifier(did.to_owned())
            } else if let Some(identifier) = field("identifier").or_else(|| field("username")) {
                Attribution::Identifier(identifier.to_owned())
            } else {
                Attribution::Malformed("identifier is required")
            }
        }
        Target::NewDid => field("did").map_or(Attribution::None, |did| {
            Attribution::Identifier(did.to_owned())
        }),
        Target::BodyDidOrList => match body.and_then(|body| body.get("did")) {
            Some(Value::String(did)) => Attribution::Identifier(did.clone()),
            Some(Value::Array(dids)) => {
                let dids: Vec<String> = dids
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect();
                if dids.is_empty() {
                    Attribution::Malformed("did is required")
                } else {
                    Attribution::Identifiers(dids)
                }
            }
            _ => Attribution::Malformed("did is required"),
        },
        Target::None => Attribution::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::header::{AUTHORIZATION, HOST};

    fn jwt_for(sub: &str) -> String {
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::json!({ "sub": sub, "scope": "com.atproto.access" }).to_string());
        format!("eyJhbGciOiJIUzI1NiJ9.{payload}.sig")
    }

    fn headers_with(name: http::header::HeaderName, value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(name, value.parse().unwrap());
        headers
    }

    #[test]
    fn requests_are_classified_by_method_and_path() {
        let none = HeaderMap::new();
        assert_eq!(
            classify(&Method::GET, "/xrpc/_health", None, &none),
            Kind::RouterHealth
        );
        assert_eq!(
            classify(&Method::POST, "/oauth/token", None, &none),
            Kind::OauthGrant
        );
        assert_eq!(
            classify(&Method::POST, "/oauth/revoke", None, &none),
            Kind::OauthGrant
        );
        assert_eq!(
            classify(&Method::POST, "/oauth/par", None, &none),
            Kind::OauthAs
        );
        assert_eq!(
            classify(
                &Method::GET,
                "/.well-known/oauth-authorization-server",
                None,
                &none
            ),
            Kind::OauthAs
        );
        assert_eq!(
            classify(
                &Method::POST,
                "/@atproto/oauth-provider/~api/consent",
                None,
                &none
            ),
            Kind::OauthAs
        );
        assert_eq!(
            classify(
                &Method::GET,
                "/@atproto/oauth-provider/~api/device-sessions",
                None,
                &none
            ),
            Kind::OauthAs
        );
        assert_eq!(
            classify(
                &Method::POST,
                "/@atproto/oauth-provider/~api/update-handle",
                None,
                &none
            ),
            Kind::Api {
                endpoint: "update-handle".into(),
                target: Some(Target::BodyDid)
            }
        );
        assert_eq!(
            classify(
                &Method::POST,
                "/@atproto/oauth-provider/~api/new-thing",
                None,
                &none
            ),
            Kind::Api {
                endpoint: "new-thing".into(),
                target: None
            }
        );
        assert_eq!(
            classify(
                &Method::GET,
                "/xrpc/com.atproto.sync.getRepo",
                Some("did=did%3Aplc%3Aa"),
                &none
            ),
            Kind::Read {
                class: ReadClass::Sync,
                identity: Some("did:plc:a".into())
            }
        );
        assert_eq!(
            classify(
                &Method::GET,
                "/xrpc/app.bsky.feed.getAuthorFeed",
                Some("actor=alice.test&limit=5"),
                &none
            ),
            Kind::Read {
                class: ReadClass::Bsky,
                identity: Some("alice.test".into())
            }
        );
        assert_eq!(
            classify(
                &Method::GET,
                "/xrpc/com.atproto.repo.getRecord",
                Some("repo=did:plc:b&collection=x"),
                &none
            ),
            Kind::Read {
                class: ReadClass::Main,
                identity: Some("did:plc:b".into())
            }
        );
        let bearer = headers_with(AUTHORIZATION, &format!("Bearer {}", jwt_for("did:plc:me")));
        assert_eq!(
            classify(
                &Method::GET,
                "/xrpc/com.atproto.server.getSession",
                None,
                &bearer
            ),
            Kind::Read {
                class: ReadClass::Main,
                identity: Some("did:plc:me".into())
            }
        );
        let dpop = headers_with(AUTHORIZATION, &format!("DPoP {}", jwt_for("did:plc:dpop")));
        assert_eq!(bearer_subject(&dpop).as_deref(), Some("did:plc:dpop"));
        assert!(bearer_subject(&headers_with(AUTHORIZATION, "Basic abc")).is_none());
        assert!(bearer_subject(&headers_with(AUTHORIZATION, "Bearer not.a.jwt")).is_none());
        let host = headers_with(HOST, "alice.test:443");
        assert_eq!(
            classify(&Method::GET, "/.well-known/atproto-did", None, &host),
            Kind::Read {
                class: ReadClass::Main,
                identity: Some("alice.test".into())
            }
        );
        assert_eq!(
            classify(
                &Method::GET,
                "/custom-well-known-atproto-did",
                Some("handle=bob.test"),
                &none
            ),
            Kind::Read {
                class: ReadClass::Main,
                identity: Some("bob.test".into())
            }
        );
        assert_eq!(
            classify(&Method::GET, "/tls-check", Some("domain=x"), &none),
            Kind::Read {
                class: ReadClass::Main,
                identity: None
            }
        );
        assert_eq!(
            classify(
                &Method::HEAD,
                "/@atproto/oauth-provider/~api/update-handle",
                None,
                &none
            ),
            Kind::Read {
                class: ReadClass::Main,
                identity: None
            }
        );
        assert_eq!(
            classify(
                &Method::POST,
                "/xrpc/com.atproto.repo.createRecord",
                None,
                &none
            ),
            Kind::Procedure {
                nsid: "com.atproto.repo.createRecord".into(),
                target: Some(Target::BodyRepo)
            }
        );
        assert_eq!(
            classify(&Method::POST, "/xrpc/com.example.newThing", None, &none),
            Kind::Procedure {
                nsid: "com.example.newThing".into(),
                target: None
            }
        );
        assert_eq!(
            classify(&Method::POST, "/xrpc/", None, &none),
            Kind::UnknownMutation
        );
        assert_eq!(
            classify(&Method::DELETE, "/anything", None, &none),
            Kind::UnknownMutation
        );
        assert_eq!(decoded("a%2Fb+c%zz"), "a/b c%zz");
        assert_eq!(decoded("%\u{e9}x%4"), "%\u{e9}x%4");
    }

    #[test]
    fn mutation_targets_follow_the_inventory_rules() {
        let none = HeaderMap::new();
        let bearer = headers_with(AUTHORIZATION, &format!("Bearer {}", jwt_for("did:plc:me")));
        let body = serde_json::json!({
            "repo": "alice.test", "did": "did:plc:d", "account": "did:plc:acct",
            "recipientDid": "did:plc:r", "token": "TOKEN", "email": "a@b.c",
            "identifier": "alice.test", "username": "ignored",
        });
        assert_eq!(
            attribute(Target::BodyRepo, Some(&body), &none),
            Attribution::Identifier("alice.test".into())
        );
        assert_eq!(
            attribute(Target::BodyRepo, None, &none),
            Attribution::Malformed("repo is required")
        );
        assert_eq!(
            attribute(Target::AuthSubject, None, &bearer),
            Attribution::Identifier("did:plc:me".into())
        );
        let service_payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
            serde_json::json!({ "iss": "did:plc:me", "aud": "did:web:pds.test", "lxm": "com.atproto.repo.uploadBlob" })
                .to_string(),
        );
        let service = headers_with(
            AUTHORIZATION,
            &format!("Bearer eyJhbGciOiJFUzI1NksifQ.{service_payload}.sig"),
        );
        assert_eq!(
            attribute(Target::AuthSubject, None, &service),
            Attribution::Identifier("did:plc:me".into())
        );
        assert!(matches!(
            attribute(Target::AuthSubject, None, &none),
            Attribution::Malformed(_)
        ));
        assert_eq!(
            attribute(Target::BodyDid, Some(&body), &none),
            Attribution::Identifier("did:plc:d".into())
        );
        assert!(matches!(
            attribute(Target::BodyDid, None, &none),
            Attribution::Malformed(_)
        ));
        assert_eq!(
            attribute(Target::BodyAccount, Some(&body), &none),
            Attribution::Identifier("did:plc:acct".into())
        );
        assert!(matches!(
            attribute(Target::BodyAccount, None, &none),
            Attribution::Malformed(_)
        ));
        assert_eq!(
            attribute(Target::BodyRecipientDid, Some(&body), &none),
            Attribution::Identifier("did:plc:r".into())
        );
        assert!(matches!(
            attribute(Target::BodyRecipientDid, None, &none),
            Attribution::Malformed(_)
        ));
        assert_eq!(
            attribute(Target::TokenOrEmailOrSubject, Some(&body), &none),
            Attribution::EmailToken("TOKEN".into())
        );
        let email_only = serde_json::json!({"email": "a@b.c"});
        assert_eq!(
            attribute(Target::TokenOrEmailOrSubject, Some(&email_only), &none),
            Attribution::Identifier("a@b.c".into())
        );
        assert_eq!(
            attribute(Target::TokenOrEmailOrSubject, None, &bearer),
            Attribution::Identifier("did:plc:me".into())
        );
        assert!(matches!(
            attribute(Target::TokenOrEmailOrSubject, None, &none),
            Attribution::Malformed(_)
        ));
        assert_eq!(
            attribute(Target::EmailOrToken, Some(&body), &none),
            Attribution::Identifier("a@b.c".into())
        );
        let token_only = serde_json::json!({"token": "T"});
        assert_eq!(
            attribute(Target::EmailOrToken, Some(&token_only), &none),
            Attribution::EmailToken("T".into())
        );
        assert!(matches!(
            attribute(Target::EmailOrToken, None, &none),
            Attribution::Malformed(_)
        ));
        let by_did = serde_json::json!({"subject": {"did": "did:plc:s"}});
        assert_eq!(
            attribute(Target::SubjectStatus, Some(&by_did), &none),
            Attribution::Identifier("did:plc:s".into())
        );
        let by_uri = serde_json::json!({"subject": {"uri": "at://did:plc:u/app.bsky.feed.post/1"}});
        assert_eq!(
            attribute(Target::SubjectStatus, Some(&by_uri), &none),
            Attribution::Identifier("did:plc:u".into())
        );
        let bad_uri = serde_json::json!({"subject": {"uri": "nope"}});
        assert!(matches!(
            attribute(Target::SubjectStatus, Some(&bad_uri), &none),
            Attribution::Malformed(_)
        ));
        let by_cid = serde_json::json!({"subject": {"cid": "bafy"}});
        assert!(matches!(
            attribute(Target::SubjectStatus, Some(&by_cid), &none),
            Attribution::Unattributable(_)
        ));
        assert!(matches!(
            attribute(Target::SubjectStatus, None, &none),
            Attribution::Malformed(_)
        ));
        assert_eq!(
            attribute(Target::Identifier, Some(&body), &none),
            Attribution::Identifier("alice.test".into())
        );
        let username = serde_json::json!({"username": "bob.test"});
        assert_eq!(
            attribute(Target::Identifier, Some(&username), &none),
            Attribution::Identifier("bob.test".into())
        );
        let gatekeeper = headers_with("x-atproto-did".parse().unwrap(), "did:plc:gk");
        assert_eq!(
            attribute(Target::Identifier, Some(&body), &gatekeeper),
            Attribution::Identifier("did:plc:gk".into())
        );
        assert!(matches!(
            attribute(Target::Identifier, None, &none),
            Attribution::Malformed(_)
        ));
        assert_eq!(
            attribute(Target::NewDid, Some(&body), &none),
            Attribution::Identifier("did:plc:d".into())
        );
        assert_eq!(attribute(Target::NewDid, None, &none), Attribution::None);
        assert_eq!(
            attribute(Target::BodyDidOrList, Some(&body), &none),
            Attribution::Identifier("did:plc:d".into())
        );
        let list = serde_json::json!({"did": ["did:plc:1", "did:plc:2", 3]});
        assert_eq!(
            attribute(Target::BodyDidOrList, Some(&list), &none),
            Attribution::Identifiers(vec!["did:plc:1".into(), "did:plc:2".into()])
        );
        let empty = serde_json::json!({"did": []});
        assert!(matches!(
            attribute(Target::BodyDidOrList, Some(&empty), &none),
            Attribution::Malformed(_)
        ));
        assert!(matches!(
            attribute(Target::BodyDidOrList, None, &none),
            Attribution::Malformed(_)
        ));
        assert_eq!(attribute(Target::None, None, &none), Attribution::None);
    }
}
