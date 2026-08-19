use hmac::{Hmac, Mac};
use secp256k1::SecretKey;
use sha2::{Digest, Sha256};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use zeroize::{Zeroize, Zeroizing};

use crate::commits::CommitSigner;
use crate::error::{HostError, Result};
use crate::service_jwt::{self, ServiceJwtIssuer};
use crate::signing::Signer;

const SECRET_KEY_BYTES: usize = 32;
pub const COMMIT_SIGN_AUDIT_EVENT: &str = "space_host_commit_signed";

#[derive(Clone)]
pub struct VerifyOnlyHs256Secret(Arc<Zeroizing<Vec<u8>>>);

impl VerifyOnlyHs256Secret {
    pub fn new(secret: impl Into<Vec<u8>>) -> Option<Self> {
        let secret = secret.into();
        (!secret.is_empty()).then(|| Self(Arc::new(Zeroizing::new(secret))))
    }

    pub(crate) fn verify(&self, signing_input: &[u8], signature: &[u8]) -> bool {
        let Ok(mut mac) = <Hmac<Sha256> as Mac>::new_from_slice(self.0.as_slice()) else {
            return false;
        };
        mac.update(signing_input);
        mac.verify_slice(signature).is_ok()
    }
}

impl fmt::Debug for VerifyOnlyHs256Secret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("VerifyOnlyHs256Secret([REDACTED])")
    }
}

pub trait SigningAudit: Send + Sync {
    fn commit_signed(&self, did: &str, space: &str, rev: &str);
}

struct TracingSigningAudit;

impl SigningAudit for TracingSigningAudit {
    fn commit_signed(&self, did: &str, space: &str, rev: &str) {
        tracing::info!(
            event = COMMIT_SIGN_AUDIT_EVENT,
            did,
            space,
            rev,
            "space repo commit signed"
        );
    }
}

pub struct PdsSeam {
    directory: PathBuf,
    audit: Arc<dyn SigningAudit>,
}

pub struct PdsServiceJwtIssuer {
    seam: Arc<PdsSeam>,
    issuer: String,
}

impl PdsServiceJwtIssuer {
    pub fn new(seam: Arc<PdsSeam>, issuer: impl Into<String>) -> Self {
        Self {
            seam,
            issuer: issuer.into(),
        }
    }
}

impl ServiceJwtIssuer for PdsServiceJwtIssuer {
    fn mint(&self, aud: &str, lxm: &str, now: u64, jti: String) -> Result<String> {
        let signer = self.seam.require_signer(&self.issuer)?;
        service_jwt::mint(&signer, &self.issuer, aud, lxm, now, jti)
    }
}

impl PdsSeam {
    pub fn open(directory: impl Into<PathBuf>) -> Result<Self> {
        let directory = directory.into();
        validate_actor_store_layout(&directory)?;
        Ok(Self {
            directory,
            audit: Arc::new(TracingSigningAudit),
        })
    }

    #[cfg(test)]
    pub(crate) fn with_audit(
        directory: impl Into<PathBuf>,
        audit: Arc<dyn SigningAudit>,
    ) -> Result<Self> {
        let directory = directory.into();
        validate_actor_store_layout(&directory)?;
        Ok(Self { directory, audit })
    }

    pub fn key_path(&self, author_did: &str) -> Option<PathBuf> {
        if author_did.is_empty()
            || !author_did.starts_with("did:")
            || author_did.contains('/')
            || author_did.contains('\\')
            || author_did.contains("..")
        {
            return None;
        }
        let digest = hex::encode(Sha256::digest(author_did.as_bytes()));
        Some(
            self.directory
                .join(&digest[..2])
                .join(author_did)
                .join("key"),
        )
    }

    pub fn signer(&self, author_did: &str) -> Result<Signer> {
        self.require_signer(author_did)
    }

    fn require_signer(&self, author_did: &str) -> Result<Signer> {
        let Some(path) = self.key_path(author_did) else {
            return Err(HostError::InvalidRequest(format!(
                "unusable did: {author_did}"
            )));
        };
        let mut bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(HostError::AccountNotHosted(author_did.to_string()))
            }
            Err(error) => return Err(HostError::Key(error.to_string())),
        };
        signer_from_bytes(&mut bytes)
    }
}

fn signer_from_bytes(bytes: &mut [u8]) -> Result<Signer> {
    let parsed = if bytes.len() == SECRET_KEY_BYTES {
        SecretKey::from_slice(bytes).map_err(|error| HostError::Key(error.to_string()))
    } else {
        Err(HostError::Key(format!(
            "actor key is {} bytes, expected {SECRET_KEY_BYTES}",
            bytes.len()
        )))
    };
    bytes.zeroize();
    parsed.map(Signer::from_secret)
}

impl CommitSigner for PdsSeam {
    fn did_key(&self, author_did: &str) -> Result<String> {
        Ok(self.require_signer(author_did)?.did_key().to_string())
    }

    fn sign(
        &self,
        author_did: &str,
        space_uri: &str,
        rev: &str,
        message: &[u8],
    ) -> Result<Vec<u8>> {
        let signature = self
            .require_signer(author_did)?
            .sign(message)
            .map_err(HostError::Key)?;
        self.audit.commit_signed(author_did, space_uri, rev);
        Ok(signature)
    }
}

fn validate_actor_store_layout(root: &Path) -> Result<()> {
    if !root.is_dir() {
        return Err(HostError::Store(format!(
            "actor store is not a directory: {}",
            root.display()
        )));
    }
    for prefix in std::fs::read_dir(root).map_err(|error| HostError::Store(error.to_string()))? {
        let prefix = prefix.map_err(|error| HostError::Store(error.to_string()))?;
        let name = prefix.file_name();
        let name = name.to_string_lossy();
        if name == "reserved_keys" {
            continue;
        }
        if name.len() != 2 || !name.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(HostError::Store(format!(
                "unrecognized actor-store layout entry: {name}"
            )));
        }
        if !prefix.path().is_dir() {
            return Err(HostError::Store(format!(
                "actor-store shard is not a directory: {name}"
            )));
        }
        for actor in
            std::fs::read_dir(prefix.path()).map_err(|error| HostError::Store(error.to_string()))?
        {
            let actor = actor.map_err(|error| HostError::Store(error.to_string()))?;
            let actor_name = actor.file_name().to_string_lossy().to_string();
            if !actor_name.starts_with("did:")
                || !actor.path().join("store.sqlite").is_file()
                || !actor.path().join("key").is_file()
            {
                return Err(HostError::Store(format!(
                    "unrecognized actor-store account layout: {actor_name}"
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commits::mint_commit;
    use std::sync::Mutex;

    const DID: &str = "did:plc:member";
    const SPACE: &str = "at://did:plc:authority/space/community.blacksky.feed/main";

    fn actor_store(secret: [u8; 32]) -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        let digest = hex::encode(Sha256::digest(DID.as_bytes()));
        let actor = directory.path().join(&digest[..2]).join(DID);
        std::fs::create_dir_all(&actor).unwrap();
        std::fs::write(actor.join("key"), secret).unwrap();
        std::fs::write(actor.join("store.sqlite"), []).unwrap();
        directory
    }

    #[test]
    fn actor_key_buffer_is_zeroized_after_parsing() {
        let mut bytes = [7u8; SECRET_KEY_BYTES];
        signer_from_bytes(&mut bytes).unwrap();
        assert_eq!(bytes, [0u8; SECRET_KEY_BYTES]);
    }

    #[test]
    fn service_auth_uses_the_authority_account_key() {
        let directory = actor_store([7u8; SECRET_KEY_BYTES]);
        let seam = Arc::new(PdsSeam::open(directory.path()).unwrap());
        let issuer = PdsServiceJwtIssuer::new(seam, DID);
        let token = issuer
            .mint("did:web:feeds.test", "test.method", 1_000, "jti-1".into())
            .unwrap();
        let account_key = Signer::from_secret(SecretKey::from_slice(&[7u8; 32]).unwrap());
        let claims = service_jwt::verify(
            &token,
            &["did:web:feeds.test"],
            "test.method",
            account_key.did_key(),
            1_001,
        )
        .unwrap();
        assert_eq!(claims.iss, DID);
    }

    #[test]
    fn layout_drift_refuses_startup() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join("unexpected-layout")).unwrap();
        assert!(matches!(
            PdsSeam::open(directory.path()),
            Err(HostError::Store(message)) if message.contains("layout")
        ));
    }

    #[derive(Default)]
    struct RecordingAudit(Mutex<Vec<(String, String, String)>>);

    impl SigningAudit for RecordingAudit {
        fn commit_signed(&self, did: &str, space: &str, rev: &str) {
            self.0
                .lock()
                .unwrap()
                .push((did.to_string(), space.to_string(), rev.to_string()));
        }
    }

    #[test]
    fn signing_emits_the_stable_audit_event_payload() {
        let directory = actor_store([9u8; 32]);
        let audit = Arc::new(RecordingAudit::default());
        let seam = PdsSeam::with_audit(directory.path(), audit.clone()).unwrap();
        mint_commit(&seam, SPACE, DID, "3rev1", &[1u8; 32], [2u8; 32]).unwrap();
        assert_eq!(
            *audit.0.lock().unwrap(),
            vec![(DID.to_string(), SPACE.to_string(), "3rev1".to_string())]
        );
        assert_eq!(COMMIT_SIGN_AUDIT_EVENT, "space_host_commit_signed");
    }

    #[test]
    fn hs256_secret_exposes_verification_without_exposing_key_material() {
        let secret = VerifyOnlyHs256Secret::new(b"verify-only".to_vec()).unwrap();
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(b"verify-only").unwrap();
        mac.update(b"header.payload");
        let signature = mac.finalize().into_bytes();
        assert!(secret.verify(b"header.payload", signature.as_slice()));
        assert!(!secret.verify(b"other", signature.as_slice()));
        assert_eq!(format!("{secret:?}"), "VerifyOnlyHs256Secret([REDACTED])");
    }
}
