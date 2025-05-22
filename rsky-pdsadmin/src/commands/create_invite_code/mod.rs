use anyhow::Result;
use serde_json::{Value, json};
use std::env;

use crate::commands::is_verbose;
use crate::util::{env as env_util, http_client};

/// Execute the create-invite-code command
pub fn execute() -> Result<()> {
    // Load environment variables with detailed error handling
    let env_file = match env_util::load_env() {
        Ok(file) => file,
        Err(e) => {
            eprintln!("Error loading environment variables: {}", e);
            eprintln!(
                "Please ensure pds.env exists with required variables (PDS_ADMIN_PASSWORD, PDS_HOSTNAME)"
            );
            return Err(anyhow::anyhow!("Failed to load environment variables"));
        }
    };

    println!("Using environment from: {}", env_file);

    // Verify critical environment variables are present
    if env::var("PDS_ADMIN_PASSWORD").is_err() {
        eprintln!("ERROR: PDS_ADMIN_PASSWORD is not set in {}", env_file);
        eprintln!("Please set PDS_ADMIN_PASSWORD in your environment file");
        return Err(anyhow::anyhow!("PDS_ADMIN_PASSWORD is not set"));
    }

    if env::var("PDS_HOSTNAME").is_err() {
        eprintln!("ERROR: PDS_HOSTNAME is not set in {}", env_file);
        eprintln!("Please set PDS_HOSTNAME in your environment file");
        return Err(anyhow::anyhow!("PDS_HOSTNAME is not set"));
    }

    println!("Creating invite code with use count of 1...");

    // Display verbose debugging information if enabled
    if is_verbose() {
        println!("[DEBUG] Preparing to create invite code");
        println!(
            "[DEBUG] Using PDS hostname: {}",
            env::var("PDS_HOSTNAME").unwrap_or_else(|_| "unknown host".to_string())
        );
        println!("[DEBUG] Using endpoint: com.atproto.server.createInviteCode");
    }

    // Create an invite code with improved error handling
    let invite_code_result: Value = match http_client::admin_post(
        "com.atproto.server.createInviteCode",
        json!({
            "useCount": 1
        }),
    ) {
        Ok(result) => {
            if is_verbose() {
                println!("[DEBUG] Successfully received response from server");
            }
            result
        }
        Err(e) => {
            eprintln!("ERROR: Failed to create invite code: {}", e);
            eprintln!(
                "Please check that the PDS server is running and accessible at {}",
                env::var("PDS_HOSTNAME").unwrap_or_else(|_| "unknown host".to_string())
            );
            return Err(anyhow::anyhow!("Failed to create invite code: {}", e));
        }
    };

    // Extract the invite code with better error handling
    let invite_code = match invite_code_result.get("code").and_then(|c| c.as_str()) {
        Some(code) => code,
        None => {
            eprintln!("ERROR: Server response did not contain an invite code");
            eprintln!("Server response: {}", invite_code_result);
            return Err(anyhow::anyhow!("Failed to parse invite code from response"));
        }
    };

    // Print the invite code
    println!("Successfully created invite code: {}", invite_code);

    Ok(())
}
