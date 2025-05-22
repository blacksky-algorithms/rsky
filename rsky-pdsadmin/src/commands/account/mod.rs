use anyhow::{Context, Result};
use clap::Subcommand;
use dialoguer::Confirm;
use rand::{Rng, distributions::Alphanumeric};
use serde_json::{Value, json};
use std::io;

use crate::util::{env, http_client};

#[derive(Subcommand, Debug)]
pub enum AccountCommands {
    /// List all accounts
    List,

    /// Create a new account
    Create {
        /// Email address for the account
        #[arg(default_value = "")]
        email: String,

        /// Handle for the account
        #[arg(default_value = "")]
        handle: String,
    },

    /// Delete an account
    Delete {
        /// DID of the account to delete
        did: String,
    },

    /// Takedown an account
    Takedown {
        /// DID of the account to takedown
        did: String,
    },

    /// Remove a takedown from an account
    Untakedown {
        /// DID of the account to untakedown
        did: String,
    },

    /// Reset the password for an account
    #[command(name = "reset-password")]
    ResetPassword {
        /// DID of the account to reset the password for
        did: String,
    },
}

pub fn execute(command: &AccountCommands) -> Result<()> {
    // Load environment variables
    env::load_env().context("Failed to load environment variables")?;

    match command {
        AccountCommands::List => list_accounts(),
        AccountCommands::Create { email, handle } => create_account(email, handle),
        AccountCommands::Delete { did } => delete_account(did),
        AccountCommands::Takedown { did } => takedown_account(did),
        AccountCommands::Untakedown { did } => untakedown_account(did),
        AccountCommands::ResetPassword { did } => reset_password(did),
    }
}

/// List all accounts on the PDS
fn list_accounts() -> Result<()> {
    println!("Fetching account list...");

    // Get the PDS hostname with better error messages
    let pds_hostname = match http_client::get_pds_hostname() {
        Ok(hostname) => hostname,
        Err(e) => {
            eprintln!("ERROR: Unable to get PDS hostname: {}", e);
            eprintln!("Please check that PDS_HOSTNAME is set in your environment file");
            return Err(anyhow::anyhow!("Failed to get PDS hostname"));
        }
    };

    println!("Connecting to PDS server at {}", pds_hostname);

    // Get list of DIDs with improved error handling
    let client = match http_client::create_client() {
        Ok(client) => client,
        Err(e) => {
            eprintln!("ERROR: Failed to create HTTP client: {}", e);
            return Err(anyhow::anyhow!("Failed to create HTTP client"));
        }
    };

    let url = format!(
        "https://{}/xrpc/com.atproto.sync.listRepos?limit=100",
        pds_hostname
    );

    println!("Requesting repository list...");

    // Send the request with detailed error handling
    let response = match client.get(&url).send() {
        Ok(resp) => resp,
        Err(e) => {
            eprintln!("ERROR: Failed to connect to PDS server at {}", pds_hostname);
            eprintln!("Reason: {}", e);
            eprintln!("Please check that the PDS server is running and accessible");
            return Err(anyhow::anyhow!(
                "Failed to send request to list repositories"
            ));
        }
    };

    // Check server response status
    if !response.status().is_success() {
        let status = response.status();
        let error_text = response
            .text()
            .unwrap_or_else(|_| "Unable to read error response".to_string());
        eprintln!("ERROR: Server returned error status {}", status);
        eprintln!("Response: {}", error_text);
        return Err(anyhow::anyhow!("Server returned error status {}", status));
    }

    // Parse the JSON response
    let response: Value = match response.json() {
        Ok(json) => json,
        Err(e) => {
            eprintln!("ERROR: Failed to parse server response as JSON");
            eprintln!("Reason: {}", e);
            return Err(anyhow::anyhow!("Failed to parse response as JSON"));
        }
    };

    // Extract DIDs with better error handling
    let repos = match response.get("repos") {
        Some(repos) => repos,
        None => {
            eprintln!("ERROR: Server response doesn't contain 'repos' field");
            eprintln!("Response: {}", response);
            return Err(anyhow::anyhow!("Failed to find 'repos' in server response"));
        }
    };

    let repos_array = match repos.as_array() {
        Some(array) => array,
        None => {
            eprintln!("ERROR: 'repos' field is not an array");
            return Err(anyhow::anyhow!("Failed to parse repos as array"));
        }
    };

    let dids: Vec<_> = repos_array
        .iter()
        .filter_map(|repo| repo["did"].as_str())
        .collect();

    if dids.is_empty() {
        println!("No accounts found on this PDS server");
        return Ok(());
    }

    println!("Found {} accounts", dids.len());

    // Format header for output table
    let mut results = vec![json!({
        "handle": "Handle",
        "email": "Email",
        "did": "DID"
    })];

    // Get account info for each DID
    println!("Fetching account details for each DID...");

    for did in dids {
        println!("Fetching account info for {}", did);

        match http_client::admin_get::<Value>(&format!(
            "com.atproto.admin.getAccountInfo?did={}",
            did
        )) {
            Ok(account_info) => {
                results.push(account_info);
            }
            Err(e) => {
                eprintln!("WARNING: Failed to get account info for {}: {}", did, e);
                // Continue with other DIDs rather than failing completely
                results.push(json!({
                    "handle": "<unavailable>",
                    "email": "<unavailable>",
                    "did": did
                }));
            }
        }
    }

    // Print results header
    println!("\nAccounts:");
    println!("----------------------------------------");

    // Print results as a table with better formatting
    for result in &results {
        println!(
            "{:<20} {:<25} {}",
            result["handle"].as_str().unwrap_or("<unknown>"),
            result["email"].as_str().unwrap_or("<unknown>"),
            result["did"].as_str().unwrap_or("<unknown>")
        );
    }
    println!("----------------------------------------");

    Ok(())
}

/// Create a new account
fn create_account(email: &str, handle: &str) -> Result<()> {
    let mut email_val = email.to_string();
    let mut handle_val = handle.to_string();

    // If email is not provided, prompt for it
    if email_val.is_empty() {
        let pds_hostname = http_client::get_pds_hostname()?;
        print!("Enter an email address (e.g. alice@{}): ", pds_hostname);
        io::stdin().read_line(&mut email_val)?;
        email_val = email_val.trim().to_string();
    }

    // If handle is not provided, prompt for it
    if handle_val.is_empty() {
        let pds_hostname = http_client::get_pds_hostname()?;
        print!("Enter a handle (e.g. alice.{}): ", pds_hostname);
        io::stdin().read_line(&mut handle_val)?;
        handle_val = handle_val.trim().to_string();
    }

    // Validate inputs
    if email_val.is_empty() || handle_val.is_empty() {
        return Err(anyhow::anyhow!("Email and handle are required"));
    }

    // Generate a random password
    let password: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(24)
        .map(char::from)
        .collect();

    // Create an invite code
    let invite_code_result: Value = http_client::admin_post(
        "com.atproto.server.createInviteCode",
        json!({
            "useCount": 1
        }),
    )?;

    let invite_code = invite_code_result["code"]
        .as_str()
        .context("Failed to parse invite code from response")?;

    // Create the account
    let create_account_data = json!({
        "email": email_val,
        "handle": handle_val,
        "password": password,
        "inviteCode": invite_code
    });

    let result =
        http_client::post_no_fail("com.atproto.server.createAccount", create_account_data)?;

    let did = result["did"].as_str();
    if did.is_none() || !did.unwrap_or("").starts_with("did:") {
        if let Some(error_message) = result["message"].as_str() {
            return Err(anyhow::anyhow!(
                "Failed to create account: {}",
                error_message
            ));
        } else {
            return Err(anyhow::anyhow!("Failed to create account: unknown error"));
        }
    }

    println!();
    println!("Account created successfully!");
    println!("-----------------------------");
    println!("Handle   : {}", handle_val);
    println!("DID      : {}", did.unwrap());
    println!("Password : {}", password);
    println!("-----------------------------");
    println!("Save this password, it will not be displayed again.");
    println!();

    Ok(())
}

/// Delete an account
fn delete_account(did: &str) -> Result<()> {
    // Validate DID
    if !did.starts_with("did:") {
        return Err(anyhow::anyhow!("DID parameter must start with \"did:\""));
    }

    // Confirm deletion
    println!("This action is permanent.");
    let confirmed = Confirm::new()
        .with_prompt(format!("Are you sure you'd like to delete {}?", did))
        .default(false)
        .interact()?;

    if !confirmed {
        return Ok(());
    }

    // Delete the account
    http_client::admin_post::<Value, _>(
        "com.atproto.admin.deleteAccount",
        json!({
            "did": did
        }),
    )?;

    println!("{} deleted", did);

    Ok(())
}

/// Takedown an account
fn takedown_account(did: &str) -> Result<()> {
    // Validate DID
    if !did.starts_with("did:") {
        return Err(anyhow::anyhow!("DID parameter must start with \"did:\""));
    }

    // Generate takedown reference (timestamp)
    let takedown_ref = chrono::Utc::now().timestamp().to_string();

    // Takedown the account
    http_client::admin_post::<Value, _>(
        "com.atproto.admin.updateSubjectStatus",
        json!({
            "subject": {
                "$type": "com.atproto.admin.defs#repoRef",
                "did": did
            },
            "takedown": {
                "applied": true,
                "ref": takedown_ref
            }
        }),
    )?;

    println!("{} taken down", did);

    Ok(())
}

/// Remove a takedown from an account
fn untakedown_account(did: &str) -> Result<()> {
    // Validate DID
    if !did.starts_with("did:") {
        return Err(anyhow::anyhow!("DID parameter must start with \"did:\""));
    }

    // Untakedown the account
    http_client::admin_post::<Value, _>(
        "com.atproto.admin.updateSubjectStatus",
        json!({
            "subject": {
                "$type": "com.atproto.admin.defs#repoRef",
                "did": did
            },
            "takedown": {
                "applied": false
            }
        }),
    )?;

    println!("{} untaken down", did);

    Ok(())
}

/// Reset the password for an account
fn reset_password(did: &str) -> Result<()> {
    // Validate DID
    if !did.starts_with("did:") {
        return Err(anyhow::anyhow!("DID parameter must start with \"did:\""));
    }

    // Generate a random password
    let password: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(24)
        .map(char::from)
        .collect();

    // Reset the password
    http_client::admin_post::<Value, _>(
        "com.atproto.admin.updateAccountPassword",
        json!({
            "did": did,
            "password": password
        }),
    )?;

    println!();
    println!("Password reset for {}", did);
    println!("New password: {}", password);
    println!();

    Ok(())
}
