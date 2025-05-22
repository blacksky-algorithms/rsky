use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde_json::json;

use crate::util::{env, http_client};

/// Execute the request-crawl command
pub fn execute(relay_hosts: &str) -> Result<()> {
    // Load environment variables
    let env_path = env::load_env().context("Failed to load environment variables")?;

    // Get relay hosts from arguments or environment
    let hosts = if relay_hosts.is_empty() {
        // If not provided, try to get from environment
        match env::get_optional_env_var("PDS_CRAWLERS") {
            Some(crawlers) if !crawlers.is_empty() => crawlers,
            _ => {
                return Err(anyhow::anyhow!(
                    "No relay hosts provided and PDS_CRAWLERS environment variable not set in {}",
                    env_path
                ));
            }
        }
    } else {
        relay_hosts.to_string()
    };

    // Get the PDS hostname
    let pds_hostname = http_client::get_pds_hostname()?;

    // Split comma-separated hosts
    let host_list: Vec<&str> = hosts.split(',').collect();

    let client = Client::new();

    // Request crawl from each host
    for host in host_list {
        let host = host.trim();
        if host.is_empty() {
            continue;
        }

        println!("Requesting crawl from {}", host);

        // Add https:// prefix if not present
        let url = if host.starts_with("http://") || host.starts_with("https://") {
            format!("{}/xrpc/com.atproto.sync.requestCrawl", host)
        } else {
            format!("https://{}/xrpc/com.atproto.sync.requestCrawl", host)
        };

        // Make the request
        client
            .post(url)
            .json(&json!({
                "hostname": pds_hostname
            }))
            .send()
            .context(format!("Failed to send request to {}", host))?
            .error_for_status()
            .context(format!("Server {} returned an error", host))?;
    }

    println!("done");

    Ok(())
}
