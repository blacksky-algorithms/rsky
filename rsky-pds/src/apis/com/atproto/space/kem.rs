//! The account's `#atproto_kem` key (proposal §KEM key).
//!
//! Each account with space repos publishes a P-256 key in its DID document as
//! `{did}#atproto_kem`. A syncer encapsulates against it to derive the secret
//! a repo host MACs its answers under, so a host whose accounts publish none
//! can be read by nobody who checks those MACs.
//!
//! The key is generated on first use and kept in the actor store beside the
//! signing key. It is published the first time an account touches spaces --
//! creating one, or writing into one -- rather than for every account up
//! front, because the proposal asks it of accounts with space repos and a
//! directory write per account is not free. Publication is best-effort: a
//! directory that cannot be reached leaves the write it rode in on to
//! succeed, and the next space write tries again.

use crate::actor_store::ActorStore;
use crate::plc;
use crate::space_mac::{KemKeyProvider, MacError};
use anyhow::Result;
use rsky_crypto::constants::P256_JWT_ALG;
use rsky_crypto::did::format_did_key;
use secp256k1::SecretKey;

/// The `did:key` for a KEM private key's public half, as the DID document
/// carries it.
pub fn kem_did_key(key: &p256::SecretKey) -> Result<String> {
    let sec1 = key.public_key().to_sec1_bytes().to_vec();
    format_did_key(P256_JWT_ALG.to_string(), sec1)
}

/// KEM private keys from the actor store, for MACing space answers.
pub struct ActorKemKeys<'a>(pub &'a ActorStore);

#[async_trait::async_trait]
impl KemKeyProvider for ActorKemKeys<'_> {
    async fn get_kem_key(&self, author_did: &str) -> Result<[u8; 32], MacError> {
        match self.0.kem_key(author_did).await {
            Ok(Some(key)) => Ok(key.to_bytes().into()),
            Ok(None) => Err(MacError::KemKeyNotAvailable),
            Err(error) => {
                tracing::warn!(%error, did = author_did, "could not read a KEM key");
                Err(MacError::KemKeyNotAvailable)
            }
        }
    }
}

/// Make sure `did`'s DID document carries its `#atproto_kem` key, generating
/// the key if it has none. Best-effort: failures are logged, not returned.
///
/// Only a `did:plc` is published, through a PLC operation signed by this
/// server's rotation key, as a handle update is; a `did:web` document is
/// edited by whoever hosts it. The directory is asked once per key: the key
/// last confirmed there is recorded, and a matching record skips the trip.
pub async fn ensure_kem_key_published(
    actor_store: &ActorStore,
    plc_url: &str,
    rotation_key: &SecretKey,
    did: &str,
) {
    if !did.starts_with("did:plc:") {
        return;
    }
    if let Err(error) = publish(actor_store, plc_url, rotation_key, did).await {
        tracing::warn!(%error, did, "could not publish the account's #atproto_kem key");
    }
}

async fn publish(
    actor_store: &ActorStore,
    plc_url: &str,
    rotation_key: &SecretKey,
    did: &str,
) -> Result<()> {
    let kem_key = kem_did_key(&actor_store.ensure_kem_key(did).await?)?;
    if actor_store.kem_published(did).await?.as_deref() == Some(kem_key.as_str()) {
        return Ok(());
    }
    let client = plc::Client::new(plc_url.to_string());
    if client
        .update_kem_key(&did.to_string(), rotation_key, &kem_key)
        .await?
    {
        tracing::info!(did, %kem_key, "published the account's #atproto_kem key");
    }
    actor_store.set_kem_published(did, &kem_key).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plc::test_support::MockPlc;
    use crate::plc::types::Operation;
    use rsky_crypto::utils::encode_did_key;
    use secp256k1::{Keypair, Secp256k1};
    use std::collections::BTreeMap;

    fn keypair(byte: u8) -> Keypair {
        let secret = secp256k1::SecretKey::from_slice(&[byte; 32]).unwrap();
        Keypair::from_secret_key(&Secp256k1::new(), &secret)
    }

    async fn store_with_actor(did: &str) -> (tempfile::TempDir, ActorStore) {
        let dir = tempfile::tempdir().unwrap();
        let lifecycle =
            crate::lifecycle::LifecycleStore::open(dir.path().join("rsky/lifecycle.sqlite"))
                .await
                .unwrap();
        let store = ActorStore::new(
            &crate::config::ActorStoreConfig {
                directory: dir.path().join("actors").to_str().unwrap().to_string(),
                cache_size: 10,
            },
            crate::background::BackgroundQueue::default(),
            lifecycle,
        );
        store.create(did, &keypair(0x44)).await.unwrap();
        (dir, store)
    }

    #[test]
    fn kem_did_key_is_a_p256_did_key() {
        let key = p256::SecretKey::from_slice(&[0x24u8; 32]).unwrap();
        let did_key = kem_did_key(&key).unwrap();
        let parsed = rsky_crypto::did::parse_did_key(&did_key).unwrap();
        assert_eq!(parsed.jwt_alg, P256_JWT_ALG);
    }

    #[tokio::test]
    async fn the_kem_key_is_generated_once_and_kept() {
        let did = "did:plc:kemkeyaaaaaaaaaaaaaaaaaa";
        let (_dir, store) = store_with_actor(did).await;
        assert!(store.kem_key(did).await.unwrap().is_none());
        let first = store.ensure_kem_key(did).await.unwrap();
        let second = store.ensure_kem_key(did).await.unwrap();
        assert_eq!(first.to_bytes(), second.to_bytes());
        assert_eq!(
            store.kem_key(did).await.unwrap().unwrap().to_bytes(),
            first.to_bytes()
        );
        let provided = ActorKemKeys(&store).get_kem_key(did).await.unwrap();
        assert_eq!(provided, <[u8; 32]>::from(first.to_bytes()));
    }

    #[tokio::test]
    async fn a_missing_kem_key_is_not_available_for_macs() {
        let did = "did:plc:kemkeybbbbbbbbbbbbbbbbbb";
        let (_dir, store) = store_with_actor(did).await;
        assert!(matches!(
            ActorKemKeys(&store).get_kem_key(did).await,
            Err(MacError::KemKeyNotAvailable)
        ));
    }

    #[tokio::test]
    async fn publication_adds_atproto_kem_once_and_keeps_atproto() {
        let did = "did:plc:kemkeycccccccccccccccccc";
        let (_dir, store) = store_with_actor(did).await;
        let rotation = keypair(0x11);
        let signing_key = encode_did_key(&keypair(0x44).public_key());
        let plc = MockPlc::start(
            &encode_did_key(&rotation.public_key()),
            BTreeMap::from([(did.to_string(), signing_key.clone())]),
        );

        ensure_kem_key_published(&store, &plc.url, &rotation.secret_key(), did).await;

        let posted = plc.posted();
        assert_eq!(posted.len(), 1);
        let op: Operation = serde_json::from_value(posted[0].clone()).unwrap();
        let kem_key = kem_did_key(&store.kem_key(did).await.unwrap().unwrap()).unwrap();
        assert_eq!(op.verification_methods.get("atproto_kem"), Some(&kem_key));
        assert_eq!(op.verification_methods.get("atproto"), Some(&signing_key));
        assert_eq!(store.kem_published(did).await.unwrap(), Some(kem_key));

        // Recorded as published: the directory is not asked again.
        ensure_kem_key_published(&store, &plc.url, &rotation.secret_key(), did).await;
        assert_eq!(plc.posted().len(), 1);
    }

    #[tokio::test]
    async fn an_unreachable_directory_is_retried_on_the_next_write() {
        let did = "did:plc:kemkeyddddddddddddddddddd";
        let (_dir, store) = store_with_actor(did).await;
        let rotation = keypair(0x11);
        let plc = MockPlc::start(&encode_did_key(&rotation.public_key()), BTreeMap::new());

        // Best-effort: the directory knows nothing of this DID, so publication
        // fails, nothing is recorded, and the call still returns.
        ensure_kem_key_published(
            &store,
            &format!("{}/missing", plc.url),
            &rotation.secret_key(),
            did,
        )
        .await;
        assert!(store.kem_published(did).await.unwrap().is_none());
        assert!(store.kem_key(did).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn a_did_web_is_left_to_whoever_hosts_it() {
        let did = "did:web:kem.example.com";
        let (_dir, store) = store_with_actor(did).await;
        let rotation = keypair(0x11);
        ensure_kem_key_published(&store, "http://127.0.0.1:1", &rotation.secret_key(), did).await;
        assert!(store.kem_key(did).await.unwrap().is_none());
    }
}
