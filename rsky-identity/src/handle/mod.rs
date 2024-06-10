use crate::types::HandleResolverOpts;
use anyhow::Result;
use hickory_resolver::config::*;
use hickory_resolver::error::ResolveResult;
use hickory_resolver::lookup_ip::LookupIp;
use hickory_resolver::Resolver;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;
use url::Url;

pub const SUBDOMAIN: &str = "_atproto";
pub const PREFIX: &str = "did=";

#[derive(Clone, Debug)]
pub struct HandleResolver {
    pub timeout: Duration,
    backup_nameservers: Option<Vec<String>>,
    backup_nameserver_ips: Option<Vec<IpAddr>>,
}

impl HandleResolver {
    pub fn new(opts: HandleResolverOpts) -> Self {
        Self {
            timeout: opts.timeout.unwrap_or(Duration::from_millis(3000)),
            backup_nameservers: opts.backup_nameservers,
            backup_nameserver_ips: None,
        }
    }

    pub async fn resolve(&mut self, handle: &String) -> Result<Option<String>> {
        let dns_future = self.resolve_dns(handle);
        let http_future = self.resolve_http(handle);

        match dns_future.await {
            Ok(dns_res) => Ok(dns_res),
            Err(_) => match http_future.await {
                Ok(http_res) => Ok(http_res),
                Err(_) => self.resolve_backup_dns(handle).await,
            },
        }
    }

    pub async fn resolve_dns(&self, handle: &String) -> Result<Option<String>> {
        let resolver = Resolver::new(ResolverConfig::default(), ResolverOpts::default())?;
        let results = match resolver.txt_lookup(format!("{SUBDOMAIN}.{handle}")) {
            Ok(res) => res,
            Err(_) => return Ok(None),
        };

        let results = results
            .iter()
            .map(|item| item.to_string())
            .collect::<Vec<String>>();

        self.parse_dns_result(results)
    }

    pub async fn resolve_http(&self, handle: &String) -> Result<Option<String>> {
        let url = Url::parse(format!("https://{handle}/.well-known/atproto-did").as_str())?;
        let client = reqwest::Client::new();

        let res = client
            .get(url.as_str())
            .header("Connection", "Keep-Alive")
            .header("Keep-Alive", "timeout=5, max=1000")
            .send()
            .await?;

        let res = res.text().await?;

        let did = match res.split("\n").collect::<Vec<&str>>().first() {
            None => return Ok(None),
            Some(first) => first.trim(),
        };

        match did.starts_with("did:") {
            true => Ok(Some(did.to_string())),
            false => Ok(None),
        }
    }

    pub async fn resolve_backup_dns(&mut self, handle: &String) -> Result<Option<String>> {
        let backup_ips = self.get_backup_nameserver_ips().await?;
        match backup_ips {
            Some(backup_ips) if backup_ips.len() >= 1 => {
                let mut config = ResolverConfig::default();
                let _ = backup_ips
                    .iter()
                    .map(|ip| {
                        config.add_name_server(NameServerConfig {
                            socket_addr: SocketAddr::new(*ip, 8080),
                            protocol: Default::default(),
                            tls_dns_name: None,
                            trust_negative_responses: false,
                            bind_addr: None,
                        })
                    })
                    .collect::<Vec<()>>();

                let resolver = Resolver::new(config, ResolverOpts::default())?;

                let results = match resolver.txt_lookup(format!("{SUBDOMAIN}.{handle}")) {
                    Ok(res) => res,
                    Err(_) => return Ok(None),
                };

                let results = results
                    .iter()
                    .map(|item| item.to_string())
                    .collect::<Vec<String>>();

                self.parse_dns_result(results)
            }
            _ => Ok(None),
        }
    }

    pub fn parse_dns_result(&self, results: Vec<String>) -> Result<Option<String>> {
        let found = results
            .iter()
            .filter(|i| i.starts_with(PREFIX))
            .collect::<Vec<&String>>();

        match found.len() != 1 {
            true => Ok(None),
            false => Ok(Some(found[0][PREFIX.len()..].to_string())),
        }
    }

    async fn get_backup_nameserver_ips(&mut self) -> Result<Option<Vec<IpAddr>>> {
        match &self.backup_nameservers {
            None => return Ok(None),
            Some(backup_nameservers) => {
                if self.backup_nameserver_ips.is_none() {
                    let resolver =
                        Resolver::new(ResolverConfig::default(), ResolverOpts::default())?;
                    let responses: Vec<LookupIp> = backup_nameservers
                        .iter()
                        .map(|h| resolver.lookup_ip(h))
                        .collect::<ResolveResult<Vec<LookupIp>>>()?;

                    for response in responses {
                        let mut backup_nameserver_ips = match &self.backup_nameserver_ips {
                            None => vec![],
                            Some(backup_nameserver_ips) => backup_nameserver_ips.clone(),
                        };
                        backup_nameserver_ips
                            .append(&mut response.iter().map(|ip| ip).collect::<Vec<IpAddr>>());
                        self.backup_nameserver_ips = Some(backup_nameserver_ips);
                    }
                }
            }
        }
        Ok(self.backup_nameserver_ips.clone())
    }
}
