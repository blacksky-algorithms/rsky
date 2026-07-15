use crate::context;
use anyhow::{bail, Result};
use reqwest::header::HeaderMap;
use rsky_common::env::{env_bool, env_int, env_list, env_str};
use rsky_common::time::{DAY, HOUR, SECOND};

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

#[derive(Debug, Clone, PartialEq)]
pub enum BlobstoreConfig {
    Disk {
        location: String,
        tmp_location: Option<String>,
    },
    S3 {
        bucket: Option<String>,
    },
}

pub fn blobstore_cfg_from(
    disk_location: Option<String>,
    disk_tmp_location: Option<String>,
    s3_bucket: Option<String>,
) -> Result<BlobstoreConfig> {
    match (disk_location, s3_bucket) {
        (Some(_), Some(_)) => bail!("Cannot set both S3 and disk blobstore env vars"),
        (Some(location), None) => Ok(BlobstoreConfig::Disk {
            location,
            tmp_location: disk_tmp_location,
        }),
        (None, Some(bucket)) => Ok(BlobstoreConfig::S3 {
            bucket: Some(bucket),
        }),
        // legacy deployments derive a per-actor bucket from the DID
        (None, None) => Ok(BlobstoreConfig::S3 { bucket: None }),
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
}

pub fn storage_cfg_from(
    data_directory: Option<String>,
    actor_store_directory: Option<String>,
    actor_store_cache_size: Option<usize>,
    account_db_location: Option<String>,
    sequencer_db_location: Option<String>,
    did_cache_db_location: Option<String>,
) -> (ActorStoreConfig, ServiceDbConfig) {
    let db_loc = |name: &str| match &data_directory {
        Some(data_directory) => format!("{data_directory}/{name}"),
        None => name.to_string(),
    };
    let actor_store = ActorStoreConfig {
        directory: actor_store_directory.unwrap_or_else(|| db_loc("actors")),
        cache_size: actor_store_cache_size.unwrap_or(100),
    };
    let service_db = ServiceDbConfig {
        account_db_location: account_db_location.unwrap_or_else(|| db_loc("account.sqlite")),
        sequencer_db_location: sequencer_db_location.unwrap_or_else(|| db_loc("sequencer.sqlite")),
        did_cache_db_location: did_cache_db_location.unwrap_or_else(|| db_loc("did_cache.sqlite")),
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
    pub handle_backup_name_servers: Option<Vec<String>>,
    pub enable_did_doc_with_session: bool,
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
        env_str("PDS_ACTOR_STORE_DIRECTORY"),
        env_int("PDS_ACTOR_STORE_CACHE_SIZE"),
        env_str("PDS_ACCOUNT_DB_LOCATION"),
        env_str("PDS_SEQUENCER_DB_LOCATION"),
        env_str("PDS_DID_CACHE_DB_LOCATION"),
    );
    let blobstore_cfg = blobstore_cfg_from(
        env_str("PDS_BLOBSTORE_DISK_LOCATION"),
        env_str("PDS_BLOBSTORE_DISK_TMP_LOCATION"),
        env_str("PDS_BLOBSTORE_S3_BUCKET"),
    )
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
    pub async fn appview_auth_headers(&self, did: &str, lxm: &str) -> Result<HeaderMap> {
        match &self.bsky_app_view {
            None => bail!("No appview configured."),
            Some(bsky_app_view) => {
                context::service_auth_headers(did, &bsky_app_view.did, lxm).await
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
        let mut no_appview = cfg;
        no_appview.bsky_app_view = None;
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        assert!(rt
            .block_on(
                no_appview.appview_auth_headers("did:example:alice", "app.bsky.feed.getTimeline")
            )
            .is_err());
    }

    #[test]
    fn blobstore_cfg_prefers_disk_when_disk_location_set() {
        let cfg = blobstore_cfg_from(Some("/data/blobs".to_owned()), None, None).unwrap();
        assert_eq!(
            cfg,
            BlobstoreConfig::Disk {
                location: "/data/blobs".to_owned(),
                tmp_location: None,
            }
        );

        let cfg = blobstore_cfg_from(
            Some("/data/blobs".to_owned()),
            Some("/data/tmp".to_owned()),
            None,
        )
        .unwrap();
        assert_eq!(
            cfg,
            BlobstoreConfig::Disk {
                location: "/data/blobs".to_owned(),
                tmp_location: Some("/data/tmp".to_owned()),
            }
        );
    }

    #[test]
    fn blobstore_cfg_uses_s3_bucket_when_set() {
        let cfg = blobstore_cfg_from(None, None, Some("my-bucket".to_owned())).unwrap();
        assert_eq!(
            cfg,
            BlobstoreConfig::S3 {
                bucket: Some("my-bucket".to_owned()),
            }
        );
    }

    #[test]
    fn blobstore_cfg_falls_back_to_legacy_s3() {
        let cfg = blobstore_cfg_from(None, None, None).unwrap();
        assert_eq!(cfg, BlobstoreConfig::S3 { bucket: None });
    }

    #[test]
    fn blobstore_cfg_rejects_both_disk_and_s3() {
        assert!(blobstore_cfg_from(
            Some("/data/blobs".to_owned()),
            None,
            Some("my-bucket".to_owned()),
        )
        .is_err());
    }

    #[test]
    fn storage_cfg_defaults_without_data_directory() {
        let (actor_store, service_db) = storage_cfg_from(None, None, None, None, None, None);
        assert_eq!(actor_store.directory, "actors");
        assert_eq!(actor_store.cache_size, 100);
        assert_eq!(service_db.account_db_location, "account.sqlite");
        assert_eq!(service_db.sequencer_db_location, "sequencer.sqlite");
        assert_eq!(service_db.did_cache_db_location, "did_cache.sqlite");
    }

    #[test]
    fn storage_cfg_defaults_under_data_directory() {
        let (actor_store, service_db) =
            storage_cfg_from(Some("/data".to_owned()), None, None, None, None, None);
        assert_eq!(actor_store.directory, "/data/actors");
        assert_eq!(service_db.account_db_location, "/data/account.sqlite");
        assert_eq!(service_db.sequencer_db_location, "/data/sequencer.sqlite");
        assert_eq!(service_db.did_cache_db_location, "/data/did_cache.sqlite");
    }

    #[test]
    fn storage_cfg_explicit_values_win() {
        let (actor_store, service_db) = storage_cfg_from(
            Some("/data".to_owned()),
            Some("/elsewhere/actors".to_owned()),
            Some(5),
            Some("/dbs/account.sqlite".to_owned()),
            Some("/dbs/sequencer.sqlite".to_owned()),
            Some("/dbs/did_cache.sqlite".to_owned()),
        );
        assert_eq!(actor_store.directory, "/elsewhere/actors");
        assert_eq!(actor_store.cache_size, 5);
        assert_eq!(service_db.account_db_location, "/dbs/account.sqlite");
        assert_eq!(service_db.sequencer_db_location, "/dbs/sequencer.sqlite");
        assert_eq!(service_db.did_cache_db_location, "/dbs/did_cache.sqlite");
    }
}
