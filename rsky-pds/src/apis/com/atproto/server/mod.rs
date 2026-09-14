use crate::actor_store::ActorStore;
use crate::{plc, SharedIdResolver};
use anyhow::{bail, Result};
use rand::{distributions::Alphanumeric, Rng};
use rocket::form::validate::Contains;
use rocket::State;
use rsky_common::env::{env_int, env_str};
use rsky_common::get_verification_material;
use rsky_crypto::utils::encode_did_key;
use rsky_identity::did::atproto_data::get_did_key_from_multibase;
use rsky_identity::types::DidDocument;
use secp256k1::{Keypair, Secp256k1, SecretKey};
use std::env;
use std::sync::LazyLock;

pub static PDS_PLC_ROTATION_KEYPAIR: LazyLock<Keypair> = LazyLock::new(|| {
    let secp = Secp256k1::new();
    let private_key = env::var("PDS_PLC_ROTATION_KEY_K256_PRIVATE_KEY_HEX").unwrap();
    let secret_key = SecretKey::from_slice(&hex::decode(private_key.as_bytes()).unwrap()).unwrap();
    Keypair::from_secret_key(&secp, &secret_key)
});

#[derive(Debug, Deserialize, Serialize)]
pub struct AssertionContents {
    pub signing_key: Option<String>,
    pub pds_endpoint: Option<String>,
    pub rotation_keys: Option<Vec<String>>,
}

/// Formatted xxxxx-xxxxx
/// The account's DID document for session outputs, when the server is
/// configured to include it; resolution failures leave it out rather than
/// failing the session call.
pub async fn did_doc_for_session(
    enabled: bool,
    id_resolver: &SharedIdResolver,
    did: &str,
) -> Option<serde_json::Value> {
    if !enabled {
        return None;
    }
    let lock = id_resolver.id_resolver.read().await;
    match lock.did.ensure_resolve(&did.to_string(), None).await {
        Ok(doc) => serde_json::to_value(doc).ok(),
        Err(error) => {
            tracing::warn!(%error, did, "could not resolve the DID document for a session");
            None
        }
    }
}

pub fn get_random_token() -> String {
    let token: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(50)
        .map(char::from)
        .collect();
    //Bluesky Client doesn't support 1,8,9,0 in the email verification tokens
    let allowed_token = token.replace(&['1', '8', '9', '0'][..], "");
    allowed_token[0..5].to_owned() + "-" + &allowed_token[5..10]
}

#[tracing::instrument(skip_all)]
pub async fn safe_resolve_did_doc(
    id_resolver: &State<SharedIdResolver>,
    did: &String,
    force_refresh: Option<bool>,
) -> Result<Option<DidDocument>> {
    let lock = id_resolver.id_resolver.read().await;
    match lock.did.resolve(did.clone(), force_refresh).await {
        Ok(did_doc) => Ok(did_doc),
        Err(err) => {
            tracing::error!(
                "@LOG: failed to resolve did doc for `{did}` with error: `{}`",
                err.to_string()
            );
            Ok(None)
        }
    }
}

/// generate an invite code preceded by the hostname
/// with '.'s replaced by '-'s, so it is not mistakable for a link
/// ex: blacksky-app-abc234-567xy
/// regex: blacksky-app-[a-z2-7]{5}-[a-z2-7]{5}
pub fn gen_invite_code() -> String {
    env::var("PDS_HOSTNAME")
        .unwrap_or("localhost".to_owned())
        .replace(".", "-")
        + "-"
        + &get_random_token().to_lowercase()
}

pub fn gen_invite_codes(count: i32) -> Vec<String> {
    let mut codes = Vec::new();
    for _i in 0..count {
        codes.push(gen_invite_code());
    }
    codes
}

pub fn validate_handle(handle: &str, service_handle_domains: &[String]) -> bool {
    service_handle_domains.iter().any(|domain| {
        let suffix = if domain.starts_with('.') {
            domain.clone()
        } else {
            format!(".{domain}")
        };
        handle
            .strip_suffix(suffix.as_str())
            .is_some_and(|front| !front.is_empty() && !front.contains('.'))
    })
}

pub async fn is_valid_did_doc_for_service(actor_store: &ActorStore, did: String) -> Result<bool> {
    match assert_valid_did_documents_for_service(actor_store, did).await {
        Ok(()) => Ok(true),
        Err(_) => Ok(false),
    }
}

pub async fn assert_valid_did_documents_for_service(
    actor_store: &ActorStore,
    did: String,
) -> Result<()> {
    let expected_signing_key = encode_did_key(&actor_store.keypair(&did).await?.public_key());
    if did.starts_with("did:plc") {
        let plc_url = env_str("PDS_DID_PLC_URL").unwrap_or("https://plc.directory".to_owned());
        let plc_client = plc::Client::new(plc_url);
        let resolved = plc_client.get_document_data(&did).await?;
        let pds_endpoint = resolved
            .services
            .get("atproto_pds")
            .map(|service| service.endpoint.clone());
        let signing_key = resolved.verification_methods.get("atproto").cloned();
        assert_valid_doc_contents(
            AssertionContents {
                pds_endpoint,
                signing_key,
                rotation_keys: Some(resolved.rotation_keys),
            },
            &expected_signing_key,
        )
        .await?;
    } else if let Some(host) = did.strip_prefix("did:web:") {
        // Bare-host did:web: the document lives at the well-known path. No
        // rotation keys to assert; control of the host is the rotation story.
        let host = host.replace("%3A", ":").replace("%3a", ":");
        if host.contains(':') || host.contains('/') {
            bail!("Unsupported did:web form for activation: {did}")
        }
        let url =
            crate::outbound::client().checked(&format!("https://{host}/.well-known/did.json"))?;
        let response = crate::outbound::client()
            .get(url, rsky_identity::safe_fetch::Redirects::Follow(3))
            .await?;
        let (status, body) =
            rsky_identity::safe_fetch::SafeClient::read_bounded(response, 64 * 1024).await?;
        if !status.is_success() {
            bail!("did:web document request answered {status}")
        }
        let doc: DidDocument = serde_json::from_slice(&body)?;
        let pds_endpoint = doc.service.as_deref().and_then(|services| {
            services
                .iter()
                .find(|s| s.id.ends_with("atproto_pds"))
                .map(|s| s.service_endpoint.clone())
        });
        let signing_key = get_verification_material(&doc, "atproto")
            .and_then(|material| get_did_key_from_multibase(material).ok().flatten());
        assert_valid_doc_contents(
            AssertionContents {
                pds_endpoint,
                signing_key,
                rotation_keys: None,
            },
            &expected_signing_key,
        )
        .await?;
    } else {
        bail!("Unsupported did method: {did}")
    }
    Ok(())
}

pub async fn assert_valid_doc_contents(
    contents: AssertionContents,
    expected_signing_key: &str,
) -> Result<()> {
    let AssertionContents {
        signing_key,
        pds_endpoint,
        rotation_keys,
    } = contents;
    let plc_rotation_key = encode_did_key(&PDS_PLC_ROTATION_KEYPAIR.public_key());

    if let Some(rotation_keys) = rotation_keys {
        if !rotation_keys.contains(plc_rotation_key) {
            bail!("Server rotation key not included in PLC DID data")
        }
    }
    // @TODO: Move next 3 lines to a shared config context
    let port = env_int("PDS_PORT").unwrap_or(2583);
    let hostname = env_str("PDS_HOSTNAME").unwrap_or("localhost".to_owned());
    let public_url = if hostname == "localhost" {
        format!("http://localhost:{port}")
    } else {
        format!("https://{hostname}")
    };

    if pds_endpoint.is_none() || pds_endpoint.unwrap() != public_url {
        bail!("DID document atproto_pds service endpoint does not match PDS public url")
    }

    if signing_key.is_none() || signing_key.unwrap() != expected_signing_key {
        bail!("DID document verification method does not match expected signing key")
    }
    Ok(())
}

/*
pub fn validate_existing_did(
    handle: &str,
    input_did: &str,
    signing_key: Keypair
) -> Result<String> {
    todo!()
}*/

pub mod activate_account;
pub mod check_account_status;
pub mod confirm_email;
pub mod create_account;
pub mod create_app_password;
pub mod create_invite_code;
pub mod create_invite_codes;
pub mod create_session;
pub mod deactivate_account;
pub mod delete_account;
pub mod delete_session;
pub mod describe_server;
pub mod get_account_invite_codes;
pub mod get_service_auth;
pub mod get_session;
pub mod list_app_passwords;
pub mod refresh_session;
pub mod request_account_delete;
pub mod request_email_confirmation;
pub mod request_email_update;
pub mod request_password_reset;
pub mod reserve_signing_key;
pub mod reset_password;
pub mod revoke_app_password;
pub mod update_email;

#[cfg(test)]
mod tests {
    use super::{did_doc_for_session, validate_handle};
    use crate::SharedIdResolver;
    use rsky_identity::types::IdentityResolverOpts;
    use rsky_identity::IdResolver;
    use std::io::{Read, Write};

    /// Serves one DID document for every request.
    fn serve_document(body: &'static str) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        format!("http://127.0.0.1:{port}")
    }

    fn resolver(plc_url: String) -> SharedIdResolver {
        SharedIdResolver {
            id_resolver: tokio::sync::RwLock::new(IdResolver::new(IdentityResolverOpts {
                timeout: Some(std::time::Duration::from_millis(500)),
                plc_url: Some(plc_url),
                did_cache: None,
                backup_nameservers: None,
            })),
        }
    }

    #[tokio::test]
    async fn session_did_doc_is_optional_and_never_fails_the_session() {
        let did = "did:plc:sessiondoc";
        let good = resolver(serve_document(
            r#"{"id":"did:plc:sessiondoc","alsoKnownAs":["at://doc.test"],"verificationMethod":[],"service":[]}"#,
        ));
        assert_eq!(did_doc_for_session(false, &good, did).await, None);
        let doc = did_doc_for_session(true, &good, did).await.unwrap();
        assert_eq!(doc["id"], did);
        let unreachable = resolver("http://127.0.0.1:1".to_owned());
        assert_eq!(did_doc_for_session(true, &unreachable, did).await, None);
    }

    fn domains() -> Vec<String> {
        vec![
            ".pds.example.com".to_string(),
            "alt.example.net".to_string(),
        ]
    }

    #[test]
    fn accepts_direct_child_of_service_domain() {
        assert!(validate_handle("alice.pds.example.com", &domains()));
    }

    #[test]
    fn accepts_direct_child_of_secondary_domain() {
        assert!(validate_handle("bob.alt.example.net", &domains()));
    }

    #[test]
    fn rejects_evil_suffix_domain() {
        assert!(!validate_handle("alice.evilpds.example.com", &domains()));
        assert!(!validate_handle("evilpds.example.com", &domains()));
    }

    #[test]
    fn rejects_multi_label_handles() {
        assert!(!validate_handle("a.b.pds.example.com", &domains()));
    }

    #[test]
    fn rejects_bare_service_domain() {
        assert!(!validate_handle("pds.example.com", &domains()));
        assert!(!validate_handle("alt.example.net", &domains()));
    }
}
