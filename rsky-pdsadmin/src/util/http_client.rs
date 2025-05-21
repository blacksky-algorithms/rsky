use anyhow::{Context, Result};
use reqwest::{blocking, header};
use serde::{Serialize, de::DeserializeOwned};

use crate::commands::is_verbose;
use crate::util::env::get_env_var;

/// Create a new HTTP client with admin authentication
pub fn create_admin_client() -> Result<blocking::Client> {
    let pds_admin_password = get_env_var("PDS_ADMIN_PASSWORD")
        .context("PDS_ADMIN_PASSWORD environment variable not set")?;

    let mut headers = header::HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/json"),
    );

    let mut auth_value = header::HeaderValue::from_str(&format!(
        "Basic {}",
        base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            format!("admin:{}", pds_admin_password)
        )
    ))?;
    auth_value.set_sensitive(true);
    headers.insert(header::AUTHORIZATION, auth_value);

    let client = blocking::Client::builder()
        .default_headers(headers)
        .build()
        .context("Failed to build HTTP client")?;

    Ok(client)
}

/// Create a new HTTP client without authentication
pub fn create_client() -> Result<blocking::Client> {
    let mut headers = header::HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/json"),
    );

    let client = blocking::Client::builder()
        .default_headers(headers)
        .build()
        .context("Failed to build HTTP client")?;

    Ok(client)
}

/// Get the PDS hostname from environment variables
pub fn get_pds_hostname() -> Result<String> {
    get_env_var("PDS_HOSTNAME")
        .context("PDS_HOSTNAME environment variable not set. Please set it in your pds.env file")
}

/// Get the PDS hostname from environment variables
pub fn get_pds_protocol() -> Result<String> {
    get_env_var("PDS_PROTOCOL").context("PDS_PROTOCOL environment variable not set. Please set it in your pds.env file. Should be either http or https")
}

/// Log verbose information if verbose mode is enabled
pub fn log_verbose(message: &str) {
    if is_verbose() {
        println!("[DEBUG] {}", message);
    }
}

/// Build a PDS API URL
pub fn build_pds_url(endpoint: &str) -> Result<String> {
    let hostname = get_pds_hostname()?;
    let protocol = get_pds_protocol()?;
    Ok(format!("{}://{}/xrpc/{}", protocol, hostname, endpoint))
}

/// Make a GET request to the PDS with admin authentication
pub fn admin_get<T: DeserializeOwned>(endpoint: &str) -> Result<T> {
    let client = create_admin_client()?;
    let url = build_pds_url(endpoint)?;

    // Log the request for debugging
    println!("Making GET request to {}", url);

    let response = client
        .get(&url)
        .send()
        .context(format!("Failed to send request to {}", url))?;

    if !response.status().is_success() {
        let status = response.status();
        let err_text = response
            .text()
            .unwrap_or_else(|_| "Could not read error response".to_string());

        return Err(anyhow::anyhow!(
            "Server returned error status {}: {}",
            status,
            err_text
        ));
    }

    response.json().context(format!(
        "Failed to parse response from {} as JSON",
        endpoint
    ))
}

/// Make a POST request to the PDS with admin authentication
pub fn admin_post<T: DeserializeOwned, D: Serialize>(endpoint: &str, data: D) -> Result<T> {
    let client = create_admin_client()?;
    let url = build_pds_url(endpoint)?;

    // Log the request for debugging
    println!("Making POST request to {}", url);

    // Print verbose information if enabled
    if is_verbose() {
        println!("Request details:");
        println!("  Endpoint: {}", endpoint);
        if let Ok(json_str) = serde_json::to_string_pretty(&data) {
            println!("  Request data: {}", json_str);
        }
    }

    let response = client
        .post(&url)
        .json(&data)
        .send()
        .context(format!("Failed to send request to {}", url))?;

    // Print verbose response info if enabled
    if is_verbose() {
        println!("Response status: {}", response.status());
        println!("Response headers:");
        for (key, value) in response.headers().iter() {
            println!("  {}: {}", key, value.to_str().unwrap_or("<binary>"));
        }
    }

    if !response.status().is_success() {
        let status = response.status();
        let err_text = response
            .text()
            .unwrap_or_else(|_| "Could not read error response".to_string());

        return Err(anyhow::anyhow!(
            "Server returned error status {}: {}",
            status,
            err_text
        ));
    }

    response.json().context(format!(
        "Failed to parse response from {} as JSON",
        endpoint
    ))
}

/// Make a POST request to the PDS without requiring success
pub fn post_no_fail<D: Serialize>(endpoint: &str, data: D) -> Result<serde_json::Value> {
    let client = create_client()?;
    let url = build_pds_url(endpoint)?;

    let response = client
        .post(url)
        .json(&data)
        .send()
        .context("Failed to send request")?;

    response.json().context("Failed to parse response as JSON")
}
