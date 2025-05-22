use anyhow::{Context, Result};
use std::fs;
use std::path::Path;
use std::process::Command;

use crate::util::env;

/// URL for downloading the compose.yaml file
const COMPOSE_URL: &str =
    "https://raw.githubusercontent.com/blacksky-algorithms/rsky/main/compose.yaml";

/// Execute the update command
pub fn execute(_target_version: &str) -> Result<()> {
    // Find the compose.yaml file
    let compose_file = env::find_compose_file()?;
    let compose_path = compose_file.to_string_lossy().to_string();

    // Temporary file for downloading the new compose.yaml
    let temp_file = format!("{}.tmp", compose_path);

    println!("* Downloading RSKY compose file");

    // Download the new compose.yaml
    let client = reqwest::blocking::Client::new();
    let response = client
        .get(COMPOSE_URL)
        .send()
        .context("Failed to download compose.yaml")?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Failed to download compose.yaml: {}",
            response.status()
        ));
    }

    let content = response
        .text()
        .context("Failed to read compose.yaml content")?;

    // Write the new content to a temporary file
    fs::write(&temp_file, &content).context("Failed to write temporary compose.yaml")?;

    // Check if the file has changed
    if compare_files(&compose_path, &temp_file)? {
        println!("RSKY-PDS is already up to date");
        fs::remove_file(&temp_file).context("Failed to remove temporary file")?;
        return Ok(());
    }

    println!("* Updating RSKY-PDS");

    // Backup the old compose.yaml
    let backup_file = format!("{}.bak", compose_path);
    fs::copy(&compose_path, &backup_file).context("Failed to backup compose.yaml")?;

    // Replace the old compose.yaml with the new one
    fs::rename(&temp_file, &compose_path).context("Failed to update compose.yaml")?;

    println!("* Restarting RSKY-PDS");

    // Check if docker is available
    let docker_exists = Command::new("which")
        .arg("docker")
        .status()
        .map(|status| status.success())
        .unwrap_or(false);

    if docker_exists {
        // Try to restart using docker-compose
        let compose_dir = Path::new(&compose_path).parent().unwrap_or(Path::new("."));

        println!("* Stopping containers");
        Command::new("docker-compose")
            .args(["-f", &compose_path, "down"])
            .current_dir(compose_dir)
            .status()
            .context("Failed to stop containers")?;

        println!("* Starting containers");
        Command::new("docker-compose")
            .args(["-f", &compose_path, "up", "-d"])
            .current_dir(compose_dir)
            .status()
            .context("Failed to start containers")?;
    } else {
        println!("WARNING: Docker is not available");
        println!("Please restart the service manually");
    }

    println!("RSKY-PDS has been updated");
    println!("---------------------");
    println!("Check logs to ensure the service started correctly");
    println!("Check container logs: docker logs rsky-pds");
    println!();

    Ok(())
}

/// Compare two files to see if they are identical
fn compare_files(file1: &str, file2: &str) -> Result<bool> {
    let content1 = fs::read_to_string(file1).context(format!("Failed to read {}", file1))?;
    let content2 = fs::read_to_string(file2).context(format!("Failed to read {}", file2))?;

    Ok(content1 == content2)
}
