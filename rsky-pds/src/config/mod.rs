use crate::common::env::{env_bool, env_int, env_str};

#[derive(Debug, Clone, PartialEq)]
pub struct ServerConfig {
    pub service: CoreConfig,
    pub mod_service: Option<ServiceConfig>,
    pub report_service: Option<ServiceConfig>,
    pub bsky_app_view: Option<ServiceConfig>,
}

/// BksyAppViewConfig, ModServiceConfig, ReportServiceConfig, etc.
#[derive(Debug, Clone, PartialEq)]
pub struct ServiceConfig {
    pub url: String,
    pub did: String,
    pub cdn_url_pattern: Option<String>, // for BksyAppViewConfig, otherwise None
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
        hostname,
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
    ServerConfig {
        service: service_cfg,
        mod_service: mod_service_cfg,
        report_service: report_service_cfg,
        bsky_app_view: bsky_app_view_cfg,
    }
}
