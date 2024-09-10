use crate::common::env::{env_bool, env_int, env_list, env_str};
use crate::common::time::{DAY, HOUR, SECOND};
use crate::context;
use anyhow::{bail, Result};
use reqwest::header::HeaderMap;

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
        blob_upload_limit: env_int("PDS_BLOB_UPLOAD_LIMIT").unwrap_or_else(|| 5 * 1024 * 1024), // 5mb
        contact_email_address: env_str("PDS_CONTACT_EMAIL_ADDRESS"),
        dev_mode: env_bool("PDS_DEV_MODE").unwrap_or(false),
    };
    let service_handle_domains: Vec<String>;
    if env_list("PDS_SERVICE_HANDLE_DOMAINS").len() > 0 {
        service_handle_domains = env_list("PDS_SERVICE_HANDLE_DOMAINS");
    } else {
        if hostname == "localhost" {
            service_handle_domains = vec![".test".to_string()];
        } else {
            service_handle_domains = vec![format!(".{hostname}")];
        }
    }
    let identity_cfg: IdentityConfig = IdentityConfig {
        plc_url: env_str("PDS_DID_PLC_URL").unwrap_or("https://plc.directory".to_string()),
        resolver_timeout: env_int("PDS_ID_RESOLVER_TIMEOUT").unwrap_or_else(|| 3 * SECOND as usize)
            as u64,
        cache_state_ttl: env_int("PDS_DID_CACHE_STALE_TTL").unwrap_or_else(|| HOUR as usize) as u64,
        cache_max_ttl: env_int("PDS_DID_CACHE_MAX_TTL").unwrap_or_else(|| DAY as usize) as u64,
        recovery_did_key: env_str("PDS_RECOVERY_DID_KEY"),
        service_handle_domains,
        handle_backup_name_servers: Some(env_list("PDS_HANDLE_BACKUP_NAMESERVERS")),
        enable_did_doc_with_session: env_bool("PDS_ENABLE_DID_DOC_WITH_SESSION").unwrap_or(false),
    };
    let bsky_app_view_cfg: Option<ServiceConfig> = match env_str("PDS_BSKY_APP_VIEW_URL") {
        None => None,
        Some(mod_service_url) => Some(ServiceConfig {
            url: mod_service_url,
            did: env_str("PDS_BSKY_APP_VIEW_DID").expect(
                "if bsky appview service url is configured, must configure its did as well.",
            ),
            cdn_url_pattern: env_str("PDS_BSKY_APP_VIEW_CDN_URL_PATTERN"),
        }),
    };
    let mod_service_cfg: Option<ServiceConfig> = match env_str("PDS_MOD_SERVICE_URL") {
        None => None,
        Some(mod_service_url) => Some(ServiceConfig {
            url: mod_service_url,
            did: env_str("PDS_MOD_SERVICE_DID")
                .expect("if mod service url is configured, must configure its did as well."),
            cdn_url_pattern: None,
        }),
    };
    let mut report_service_cfg: Option<ServiceConfig> = match env_str("PDS_REPORT_SERVICE_URL") {
        None => None,
        Some(mod_service_url) => Some(ServiceConfig {
            url: mod_service_url,
            did: env_str("PDS_REPORT_SERVICE_DID")
                .expect("if mod service url is configured, must configure its did as well."),
            cdn_url_pattern: None,
        }),
    };

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

    ServerConfig {
        service: service_cfg,
        mod_service: mod_service_cfg,
        report_service: report_service_cfg,
        bsky_app_view: bsky_app_view_cfg,
        subscription: subscription_cfg,
        invites: invites_cfg,
        crawlers: crawlers_cfg,
        identity: identity_cfg,
    }
}

impl ServerConfig {
    pub async fn appview_auth_headers(&self, did: &String, lxm: &String) -> Result<HeaderMap> {
        match &self.bsky_app_view {
            None => bail!("No appview configured."),
            Some(bsky_app_view) => {
                context::service_auth_headers(did, &bsky_app_view.did, lxm).await
            }
        }
    }
}
