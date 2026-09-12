use crate::account_manager::helpers::auth::ServiceJwtParams;
use crate::xrpc_server::auth::create_service_auth_headers;
use anyhow::{bail, Result};
use reqwest::header::HeaderMap;
use rsky_common::env::{env_bool, env_int, env_list, env_str};
use rsky_common::time::{DAY, HOUR, SECOND};
use secp256k1::Keypair;

#[derive(Debug, Clone, PartialEq)]
pub struct ServerConfig {
    pub service: CoreConfig,
    pub mod_service: Option<ServiceConfig>,
    pub report_service: Option<ServiceConfig>,
    pub bsky_app_view: Option<ServiceConfig>,
    pub subscription: SubscriptionConfig,
    pub invites: InvitesConfig,
    pub identity: IdentityConfig,
    pub crawlers: Vec<String>,
    pub actor_store: ActorStoreConfig,
    pub service_db: ServiceDbConfig,
    pub blobstore: BlobstoreConfig,
}

/// S3-compatible object storage, configured the way the reference PDS is.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct S3Config {
    /// One bucket holding every actor under DID-prefixed keys; legacy
    /// deployments without one keep a bucket per actor named after the DID.
    pub bucket: Option<String>,
    pub region: Option<String>,
    pub endpoint: Option<String>,
    pub force_path_style: bool,
    pub access_key_id: Option<String>,
    pub secret_access_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BlobstoreConfig {
    Disk {
        location: String,
        tmp_location: Option<String>,
    },
    S3(S3Config),
}

/// The `PDS_BLOBSTORE_*` variables as read from the environment.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BlobstoreEnv {
    pub disk_location: Option<String>,
    pub disk_tmp_location: Option<String>,
    pub s3: S3Config,
}

pub fn blobstore_cfg_from(env: BlobstoreEnv) -> Result<BlobstoreConfig> {
    if env.disk_location.is_some() && env.s3.bucket.is_some() {
        bail!("Cannot set both S3 and disk blobstore env vars");
    }
    if env.s3.access_key_id.is_some() != env.s3.secret_access_key.is_some() {
        bail!("Must specify both S3 access key id and secret access key blobstore env vars");
    }
    match env.disk_location {
        Some(location) => Ok(BlobstoreConfig::Disk {
            location,
            tmp_location: env.disk_tmp_location,
        }),
        None => Ok(BlobstoreConfig::S3(env.s3)),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActorStoreConfig {
    pub directory: String,
    pub cache_size: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ServiceDbConfig {
    pub account_db_location: String,
    pub sequencer_db_location: String,
    pub did_cache_db_location: String,
    /// The deletion and purge journal, kept outside every actor store.
    pub lifecycle_db_location: String,
    /// Per-actor advisory locks shared with maintenance tooling.
    pub lock_dir: String,
    /// The journal of every physical object-storage write.
    pub blob_attempts_db_location: String,
    /// The repair and quarantine journal.
    pub repair_db_location: String,
}

/// Per-location overrides of the layout under the data directory.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StorageOverrides {
    pub actor_store_directory: Option<String>,
    pub actor_store_cache_size: Option<usize>,
    pub account_db_location: Option<String>,
    pub sequencer_db_location: Option<String>,
    pub did_cache_db_location: Option<String>,
    pub lifecycle_db_location: Option<String>,
    pub lock_dir: Option<String>,
    pub blob_attempts_db_location: Option<String>,
    pub repair_db_location: Option<String>,
}

pub fn storage_cfg_from(
    data_directory: Option<String>,
    overrides: StorageOverrides,
) -> (ActorStoreConfig, ServiceDbConfig) {
    let db_loc = |name: &str| match &data_directory {
        Some(data_directory) => format!("{data_directory}/{name}"),
        None => name.to_string(),
    };
    let actor_store = ActorStoreConfig {
        directory: overrides
            .actor_store_directory
            .unwrap_or_else(|| db_loc("actors")),
        cache_size: overrides.actor_store_cache_size.unwrap_or(100),
    };
    let service_db = ServiceDbConfig {
        account_db_location: overrides
            .account_db_location
            .unwrap_or_else(|| db_loc("account.sqlite")),
        sequencer_db_location: overrides
            .sequencer_db_location
            .unwrap_or_else(|| db_loc("sequencer.sqlite")),
        did_cache_db_location: overrides
            .did_cache_db_location
            .unwrap_or_else(|| db_loc("did_cache.sqlite")),
        lifecycle_db_location: overrides
            .lifecycle_db_location
            .unwrap_or_else(|| db_loc("rsky/lifecycle.sqlite")),
        lock_dir: overrides.lock_dir.unwrap_or_else(|| db_loc("rsky/locks")),
        blob_attempts_db_location: overrides
            .blob_attempts_db_location
            .unwrap_or_else(|| db_loc("rsky/blob-attempts.sqlite")),
        repair_db_location: overrides
            .repair_db_location
            .unwrap_or_else(|| db_loc("rsky/repair.sqlite")),
    };
    (actor_store, service_db)
}

/// BksyAppViewConfig, ModServiceConfig, ReportServiceConfig, etc.
#[derive(Debug, Clone, PartialEq)]
pub struct ServiceConfig {
    pub url: String,
    pub did: String,
    pub cdn_url_pattern: Option<String>, // for BksyAppViewConfig, otherwise None
}

#[derive(Debug, Clone, PartialEq)]
pub struct SubscriptionConfig {
    pub max_buffer: u64,
    pub repo_backfill_limit_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IdentityConfig {
    pub plc_url: String,
    pub resolver_timeout: u64,
    pub cache_state_ttl: u64,
    pub cache_max_ttl: u64,
    pub recovery_did_key: Option<String>,
    pub service_handle_domains: Vec<String>,
    /// Handle domains served here but not offered at signup.
    pub extra_handle_domains: Vec<String>,
    pub handle_backup_name_servers: Option<Vec<String>>,
    pub enable_did_doc_with_session: bool,
}

impl IdentityConfig {
    /// Whether `handle` is one this server serves, on an offered or an
    /// extra domain, with the reference's rule for bare domains.
    pub fn is_hosted_handle(&self, handle: &str) -> bool {
        self.service_handle_domains
            .iter()
            .chain(self.extra_handle_domains.iter())
            .any(|available| match available.strip_prefix('.') {
                Some(bare) => handle == bare || handle.ends_with(available.as_str()),
                None => handle == available || handle.ends_with(&format!(".{available}")),
            })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct InvitesConfig {
    pub required: bool,
    pub interval: Option<usize>,
    pub epoch: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoreConfig {
    pub port: usize,
    pub hostname: String,
    pub public_url: String,
    pub did: String,
    pub version: Option<String>,
    pub privacy_policy_url: Option<String>,
    pub terms_of_service_url: Option<String>,
    pub accepting_imports: bool,
    pub blob_upload_limit: usize,
    pub contact_email_address: Option<String>,
    pub dev_mode: bool,
    /// Another implementation shares this data directory and may still
    /// serve its objects, so nothing in blob storage is deleted here.
    pub coexistence: bool,
    /// The write allowlist naming the actors this process may write; every
    /// actor is admitted when unset.
    pub write_allowlist_file: Option<String>,
    /// Serve reads only: every database opens read-only without migrating,
    /// no worker runs, and every mutating request is refused.
    pub read_only: bool,
    /// How long in-flight requests may finish after a stop signal.
    pub shutdown_grace_secs: u32,
    /// Where uploads are spooled while they are hashed and stored.
    pub upload_spool_dir: String,
}

pub fn env_to_cfg() -> ServerConfig {
    let port = env_int("PDS_PORT").unwrap_or(2583);
    let hostname = env_str("PDS_HOSTNAME").unwrap_or("localhost".to_string());
    let public_url = if hostname == "localhost" {
        format!("http://localhost:{port}")
    } else {
        format!("https://{hostname}")
    };
    let did = env_str("PDS_SERVICE_DID").unwrap_or(format!("did:web:{hostname}"));
    let service_cfg = CoreConfig {
        port,
        hostname: hostname.clone(),
        public_url,
        did,
        version: env_str("PDS_VERSION"),
        privacy_policy_url: env_str("PDS_PRIVACY_POLICY_URL"),
        terms_of_service_url: env_str("PDS_TERMS_OF_SERVICE_URL"),
        accepting_imports: env_bool("PDS_ACCEPTING_REPO_IMPORTS").unwrap_or(true),
        blob_upload_limit: env_int("PDS_BLOB_UPLOAD_LIMIT").unwrap_or(5 * 1024 * 1024), // 5mb
        contact_email_address: env_str("PDS_CONTACT_EMAIL_ADDRESS"),
        dev_mode: env_bool("PDS_DEV_MODE").unwrap_or(false),
        coexistence: env_bool("PDS_COEXISTENCE").unwrap_or(false),
        write_allowlist_file: env_str("PDS_WRITE_ALLOWLIST_FILE"),
        read_only: env_bool("PDS_READ_ONLY").unwrap_or(false),
        shutdown_grace_secs: env_int("PDS_SHUTDOWN_GRACE_SECS").unwrap_or(100) as u32,
        upload_spool_dir: env_str("PDS_UPLOAD_SPOOL_DIR").unwrap_or_else(|| {
            std::env::temp_dir()
                .join("rsky-pds-spool")
                .to_string_lossy()
                .to_string()
        }),
    };
    let service_handle_domains: Vec<String>;
    if !env_list("PDS_SERVICE_HANDLE_DOMAINS").is_empty() {
        service_handle_domains = env_list("PDS_SERVICE_HANDLE_DOMAINS");
    } else if hostname == "localhost" {
        service_handle_domains = vec![".test".to_string()];
    } else {
        service_handle_domains = vec![format!(".{hostname}")];
    }
    let identity_cfg: IdentityConfig = IdentityConfig {
        plc_url: env_str("PDS_DID_PLC_URL").unwrap_or("https://plc.directory".to_string()),
        resolver_timeout: env_int("PDS_ID_RESOLVER_TIMEOUT").unwrap_or_else(|| 3 * SECOND as usize)
            as u64,
        cache_state_ttl: env_int("PDS_DID_CACHE_STALE_TTL").unwrap_or(HOUR as usize) as u64,
        cache_max_ttl: env_int("PDS_DID_CACHE_MAX_TTL").unwrap_or(DAY as usize) as u64,
        recovery_did_key: env_str("PDS_RECOVERY_DID_KEY"),
        service_handle_domains,
        extra_handle_domains: env_list("PDS_EXTRA_HANDLE_DOMAINS"),
        handle_backup_name_servers: Some(env_list("PDS_HANDLE_BACKUP_NAMESERVERS")),
        enable_did_doc_with_session: env_bool("PDS_ENABLE_DID_DOC_WITH_SESSION").unwrap_or(false),
    };
    let bsky_app_view_cfg: Option<ServiceConfig> =
        env_str("PDS_BSKY_APP_VIEW_URL").map(|mod_service_url| ServiceConfig {
            url: mod_service_url,
            did: env_str("PDS_BSKY_APP_VIEW_DID").expect(
                "if bsky appview service url is configured, must configure its did as well.",
            ),
            cdn_url_pattern: env_str("PDS_BSKY_APP_VIEW_CDN_URL_PATTERN"),
        });
    let mod_service_cfg: Option<ServiceConfig> =
        env_str("PDS_MOD_SERVICE_URL").map(|mod_service_url| ServiceConfig {
            url: mod_service_url,
            did: env_str("PDS_MOD_SERVICE_DID")
                .expect("if mod service url is configured, must configure its did as well."),
            cdn_url_pattern: None,
        });
    let mut report_service_cfg: Option<ServiceConfig> =
        env_str("PDS_REPORT_SERVICE_URL").map(|mod_service_url| ServiceConfig {
            url: mod_service_url,
            did: env_str("PDS_REPORT_SERVICE_DID")
                .expect("if mod service url is configured, must configure its did as well."),
            cdn_url_pattern: None,
        });

    // if there's a mod service, default report service into it
    if mod_service_cfg.is_some() && report_service_cfg.is_none() {
        report_service_cfg = mod_service_cfg.clone();
    }
    let subscription_cfg = SubscriptionConfig {
        max_buffer: env_int("PDS_MAX_SUBSCRIPTION_BUFFER").unwrap_or(500) as u64,
        repo_backfill_limit_ms: env_int("PDS_REPO_BACKFILL_LIMIT_MS").unwrap_or(DAY as usize)
            as u64,
    };
    // default to being required if left undefined
    let invites_cfg = match env_bool("PDS_INVITE_REQUIRED").unwrap_or(true) {
        false => InvitesConfig {
            required: false,
            interval: None,
            epoch: None,
        },
        true => InvitesConfig {
            required: true,
            interval: env_int("PDS_INVITE_INTERVAL"),
            epoch: Some(env_int("PDS_INVITE_EPOCH").unwrap_or(0)),
        },
    };
    let crawlers_cfg = env_list("PDS_CRAWLERS");
    let (actor_store_cfg, service_db_cfg) = storage_cfg_from(
        env_str("PDS_DATA_DIRECTORY"),
        StorageOverrides {
            actor_store_directory: env_str("PDS_ACTOR_STORE_DIRECTORY"),
            actor_store_cache_size: env_int("PDS_ACTOR_STORE_CACHE_SIZE"),
            account_db_location: env_str("PDS_ACCOUNT_DB_LOCATION"),
            sequencer_db_location: env_str("PDS_SEQUENCER_DB_LOCATION"),
            did_cache_db_location: env_str("PDS_DID_CACHE_DB_LOCATION"),
            lifecycle_db_location: env_str("PDS_LIFECYCLE_DB"),
            lock_dir: env_str("PDS_LOCK_DIR"),
            blob_attempts_db_location: env_str("PDS_BLOB_ATTEMPTS_DB"),
            repair_db_location: env_str("PDS_REPAIR_DB"),
        },
    );
    let blobstore_cfg = blobstore_cfg_from(BlobstoreEnv {
        disk_location: env_str("PDS_BLOBSTORE_DISK_LOCATION"),
        disk_tmp_location: env_str("PDS_BLOBSTORE_DISK_TMP_LOCATION"),
        s3: S3Config {
            bucket: env_str("PDS_BLOBSTORE_S3_BUCKET"),
            region: env_str("PDS_BLOBSTORE_S3_REGION"),
            endpoint: env_str("PDS_BLOBSTORE_S3_ENDPOINT").or_else(|| env_str("AWS_ENDPOINT")),
            force_path_style: env_bool("PDS_BLOBSTORE_S3_FORCE_PATH_STYLE").unwrap_or(false),
            access_key_id: env_str("PDS_BLOBSTORE_S3_ACCESS_KEY_ID"),
            secret_access_key: env_str("PDS_BLOBSTORE_S3_SECRET_ACCESS_KEY"),
        },
    })
    .expect("invalid blobstore configuration");

    ServerConfig {
        service: service_cfg,
        mod_service: mod_service_cfg,
        report_service: report_service_cfg,
        bsky_app_view: bsky_app_view_cfg,
        subscription: subscription_cfg,
        invites: invites_cfg,
        crawlers: crawlers_cfg,
        identity: identity_cfg,
        actor_store: actor_store_cfg,
        service_db: service_db_cfg,
        blobstore: blobstore_cfg,
    }
}

impl ServerConfig {
    /// `keypair` must be the signing key of `did`, the account the token is
    /// issued on behalf of.
    pub async fn appview_auth_headers(
        &self,
        did: &str,
        lxm: &str,
        keypair: &Keypair,
    ) -> Result<HeaderMap> {
        match &self.bsky_app_view {
            None => bail!("No appview configured."),
            Some(bsky_app_view) => {
                create_service_auth_headers(
                    ServiceJwtParams {
                        iss: did.to_owned(),
                        aud: bsky_app_view.did.clone(),
                        exp: None,
                        lxm: Some(lxm.to_owned()),
                        jti: None,
                    },
                    keypair,
                )
                .await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_to_cfg_builds_default_and_configured_config() {
        let cfg = env_to_cfg();
        assert!(cfg.service.port > 0);
        assert!(!cfg.actor_store.directory.is_empty());
        assert!(cfg
            .service_db
            .account_db_location
            .ends_with("account.sqlite"));
        assert!(cfg.invites.required);

        // configured variant: exercises hostname/appview/mod-service/invite branches
        let vars = [
            ("PDS_HOSTNAME", "pds.example.com"),
            ("PDS_SERVICE_HANDLE_DOMAINS", ".pds.example.com"),
            ("PDS_BSKY_APP_VIEW_URL", "https://appview.example.com"),
            ("PDS_BSKY_APP_VIEW_DID", "did:web:appview.example.com"),
            ("PDS_MOD_SERVICE_URL", "https://mod.example.com"),
            ("PDS_MOD_SERVICE_DID", "did:web:mod.example.com"),
            ("PDS_REPORT_SERVICE_URL", "https://report.example.com"),
            ("PDS_REPORT_SERVICE_DID", "did:web:report.example.com"),
            ("PDS_INVITE_REQUIRED", "false"),
        ];
        for (key, value) in vars {
            std::env::set_var(key, value);
        }
        let cfg = env_to_cfg();
        for (key, _) in vars {
            std::env::remove_var(key);
        }
        assert_eq!(cfg.service.public_url, "https://pds.example.com");
        assert_eq!(
            cfg.identity.service_handle_domains,
            vec![".pds.example.com".to_string()]
        );
        assert_eq!(
            cfg.bsky_app_view.unwrap().did,
            "did:web:appview.example.com"
        );
        assert_eq!(cfg.mod_service.unwrap().did, "did:web:mod.example.com");
        assert_eq!(
            cfg.report_service.unwrap().did,
            "did:web:report.example.com"
        );
        assert!(!cfg.invites.required);

        // a mod service without an explicit report service is used for reports,
        // and a non-localhost hostname derives its own handle domain
        std::env::set_var("PDS_HOSTNAME", "pds2.example.com");
        std::env::set_var("PDS_MOD_SERVICE_URL", "https://mod.example.com");
        std::env::set_var("PDS_MOD_SERVICE_DID", "did:web:mod.example.com");
        let cfg = env_to_cfg();
        std::env::remove_var("PDS_HOSTNAME");
        std::env::remove_var("PDS_MOD_SERVICE_URL");
        std::env::remove_var("PDS_MOD_SERVICE_DID");
        assert_eq!(cfg.report_service.unwrap().did, "did:web:mod.example.com");
        assert_eq!(
            cfg.identity.service_handle_domains,
            vec![".pds2.example.com".to_string()]
        );

        let cfg = env_to_cfg();
        assert_eq!(
            cfg.service.public_url,
            format!("http://localhost:{}", cfg.service.port)
        );

        // no appview configured means no auth headers
        let mut no_appview = cfg.clone();
        no_appview.bsky_app_view = None;
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let secret = secp256k1::SecretKey::from_slice(&[0x29u8; 32]).unwrap();
        let keypair = Keypair::from_secret_key(&secp256k1::Secp256k1::new(), &secret);
        assert!(rt
            .block_on(no_appview.appview_auth_headers(
                "did:example:alice",
                "app.bsky.feed.getTimeline",
                &keypair
            ))
            .is_err());

        // with one configured, the header is signed by the key it was handed
        let mut with_appview = cfg;
        with_appview.bsky_app_view = Some(ServiceConfig {
            url: "https://appview.example.com".to_owned(),
            did: "did:web:appview.example.com".to_owned(),
            cdn_url_pattern: None,
        });
        let headers = rt
            .block_on(with_appview.appview_auth_headers(
                "did:example:alice",
                "app.bsky.feed.getTimeline",
                &keypair,
            ))
            .unwrap();
        let jwt = headers
            .get(reqwest::header::AUTHORIZATION)
            .unwrap()
            .to_str()
            .unwrap()
            .strip_prefix("Bearer ")
            .unwrap()
            .to_owned();
        let did_key = rsky_crypto::utils::encode_did_key(&keypair.public_key());
        let payload = rt
            .block_on(crate::xrpc_server::auth::verify_jwt(
                jwt,
                Some("did:web:appview.example.com".to_owned()),
                Some("app.bsky.feed.getTimeline"),
                move |_iss, _refresh| {
                    let did_key = did_key.clone();
                    async move { Ok(did_key) }
                },
            ))
            .unwrap();
        assert_eq!(payload.iss, "did:example:alice");
    }

    #[test]
    fn hosted_handles_cover_offered_and_extra_domains() {
        let identity = IdentityConfig {
            plc_url: String::new(),
            resolver_timeout: 0,
            cache_state_ttl: 0,
            cache_max_ttl: 0,
            recovery_did_key: None,
            service_handle_domains: vec![".rsky.com".to_owned()],
            extra_handle_domains: vec!["extra.test".to_owned()],
            handle_backup_name_servers: None,
            enable_did_doc_with_session: false,
        };
        assert!(identity.is_hosted_handle("alice.rsky.com"));
        assert!(identity.is_hosted_handle("rsky.com"));
        assert!(identity.is_hosted_handle("bob.extra.test"));
        assert!(identity.is_hosted_handle("extra.test"));
        assert!(!identity.is_hosted_handle("alice.elsewhere.test"));
        assert!(!identity.is_hosted_handle("notrsky.com"));
    }

    #[test]
    fn blobstore_cfg_prefers_disk_when_disk_location_set() {
        let cfg = blobstore_cfg_from(BlobstoreEnv {
            disk_location: Some("/data/blobs".to_owned()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(
            cfg,
            BlobstoreConfig::Disk {
                location: "/data/blobs".to_owned(),
                tmp_location: None,
            }
        );
        let cfg = blobstore_cfg_from(BlobstoreEnv {
            disk_location: Some("/data/blobs".to_owned()),
            disk_tmp_location: Some("/tmp/blobs".to_owned()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(
            cfg,
            BlobstoreConfig::Disk {
                location: "/data/blobs".to_owned(),
                tmp_location: Some("/tmp/blobs".to_owned()),
            }
        );
    }

    #[test]
    fn blobstore_cfg_reads_the_reference_s3_settings() {
        let s3 = S3Config {
            bucket: Some("my-bucket".to_owned()),
            region: Some("nyc3".to_owned()),
            endpoint: Some("https://nyc3.digitaloceanspaces.com".to_owned()),
            force_path_style: true,
            access_key_id: Some("key".to_owned()),
            secret_access_key: Some("secret".to_owned()),
        };
        let cfg = blobstore_cfg_from(BlobstoreEnv {
            s3: s3.clone(),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(cfg, BlobstoreConfig::S3(s3));
        // no bucket at all is the legacy per-actor layout
        assert_eq!(
            blobstore_cfg_from(BlobstoreEnv::default()).unwrap(),
            BlobstoreConfig::S3(S3Config::default())
        );
        assert!(blobstore_cfg_from(BlobstoreEnv {
            disk_location: Some("/data/blobs".to_owned()),
            s3: S3Config {
                bucket: Some("my-bucket".to_owned()),
                ..Default::default()
            },
            ..Default::default()
        })
        .is_err());
        assert!(blobstore_cfg_from(BlobstoreEnv {
            s3: S3Config {
                access_key_id: Some("key".to_owned()),
                ..Default::default()
            },
            ..Default::default()
        })
        .is_err());
    }

    #[test]
    fn storage_cfg_defaults_without_data_directory() {
        let (actor_store, service_db) = storage_cfg_from(None, StorageOverrides::default());
        assert_eq!(actor_store.directory, "actors");
        assert_eq!(actor_store.cache_size, 100);
        assert_eq!(service_db.account_db_location, "account.sqlite");
        assert_eq!(service_db.sequencer_db_location, "sequencer.sqlite");
        assert_eq!(service_db.did_cache_db_location, "did_cache.sqlite");
        assert_eq!(service_db.lifecycle_db_location, "rsky/lifecycle.sqlite");
        assert_eq!(service_db.lock_dir, "rsky/locks");
        assert_eq!(
            service_db.blob_attempts_db_location,
            "rsky/blob-attempts.sqlite"
        );
        assert_eq!(service_db.repair_db_location, "rsky/repair.sqlite");
    }

    #[test]
    fn storage_cfg_defaults_under_data_directory() {
        let (actor_store, service_db) =
            storage_cfg_from(Some("/data".to_owned()), StorageOverrides::default());
        assert_eq!(actor_store.directory, "/data/actors");
        assert_eq!(service_db.account_db_location, "/data/account.sqlite");
        assert_eq!(service_db.sequencer_db_location, "/data/sequencer.sqlite");
        assert_eq!(service_db.did_cache_db_location, "/data/did_cache.sqlite");
        assert_eq!(
            service_db.lifecycle_db_location,
            "/data/rsky/lifecycle.sqlite"
        );
        assert_eq!(service_db.lock_dir, "/data/rsky/locks");
        assert_eq!(
            service_db.blob_attempts_db_location,
            "/data/rsky/blob-attempts.sqlite"
        );
        assert_eq!(service_db.repair_db_location, "/data/rsky/repair.sqlite");
    }

    #[test]
    fn storage_cfg_explicit_values_win() {
        let (actor_store, service_db) = storage_cfg_from(
            Some("/data".to_owned()),
            StorageOverrides {
                actor_store_directory: Some("/elsewhere/actors".to_owned()),
                actor_store_cache_size: Some(5),
                account_db_location: Some("/dbs/account.sqlite".to_owned()),
                sequencer_db_location: Some("/dbs/sequencer.sqlite".to_owned()),
                did_cache_db_location: Some("/dbs/did_cache.sqlite".to_owned()),
                lifecycle_db_location: Some("/dbs/lifecycle.sqlite".to_owned()),
                lock_dir: Some("/dbs/locks".to_owned()),
                blob_attempts_db_location: Some("/dbs/attempts.sqlite".to_owned()),
                repair_db_location: Some("/dbs/repair.sqlite".to_owned()),
            },
        );
        assert_eq!(actor_store.directory, "/elsewhere/actors");
        assert_eq!(actor_store.cache_size, 5);
        assert_eq!(service_db.account_db_location, "/dbs/account.sqlite");
        assert_eq!(service_db.sequencer_db_location, "/dbs/sequencer.sqlite");
        assert_eq!(service_db.did_cache_db_location, "/dbs/did_cache.sqlite");
        assert_eq!(service_db.lifecycle_db_location, "/dbs/lifecycle.sqlite");
        assert_eq!(service_db.lock_dir, "/dbs/locks");
        assert_eq!(service_db.blob_attempts_db_location, "/dbs/attempts.sqlite");
        assert_eq!(service_db.repair_db_location, "/dbs/repair.sqlite");
    }
}
