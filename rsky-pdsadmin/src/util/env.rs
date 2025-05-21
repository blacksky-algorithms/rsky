use anyhow::{Context, Result};
use std::env;
use std::path::{Path, PathBuf};

/// Environment variable for PDS configuration
pub const PDS_ENV_FILE: &str = "PDS_ENV_FILE";

/// Default paths to check for PDS configuration
pub const DEFAULT_PATHS: &[&str] = &[
    "./pds.env",
    "/pds/pds.env",
    "/usr/src/rsky/pds.env",
    "$HOME/.config/rsky/rsky-pds/pds.env",
];

/// Load environment variables from the PDS configuration file
pub fn load_env() -> Result<String> {
    // Check if PDS_ENV_FILE is set
    if let Ok(env_file) = env::var(PDS_ENV_FILE) {
        let path = shellexpand::full(&env_file)
            .map(|s| s.into_owned())
            .unwrap_or(env_file);
        load_from_path(&path).context(format!("Failed to load environment from {}", path))?;
        return Ok(path);
    }

    // Try the default paths
    for path in DEFAULT_PATHS {
        let expanded_path = shellexpand::full(path)
            .map(|s| s.into_owned())
            .unwrap_or_else(|_| path.to_string());

        if Path::new(&expanded_path).exists() {
            load_from_path(&expanded_path)
                .context(format!("Failed to load environment from {}", expanded_path))?;
            return Ok(expanded_path);
        }
    }

    Err(anyhow::anyhow!(
        "Could not find PDS environment file. Set PDS_ENV_FILE or create a pds.env file in one of the default locations."
    ))
}

/// Load environment variables from a specific path
fn load_from_path(path: &str) -> Result<()> {
    dotenv::from_path(path).context(format!("Failed to load .env file from {}", path))?;
    Ok(())
}

/// Get a required environment variable
pub fn get_env_var(name: &str) -> Result<String> {
    env::var(name).context(format!("Environment variable {} is not set", name))
}

/// Get an optional environment variable
pub fn get_optional_env_var(name: &str) -> Option<String> {
    env::var(name).ok()
}

/// Find the compose.yaml file for the RSKY PDS
pub fn find_compose_file() -> Result<PathBuf> {
    // Check for compose.yaml in the RSKY repo path
    let compose_path = PathBuf::from("/home/parallels/src/blacksky-algorithms/rsky/compose.yaml");
    if compose_path.exists() {
        return Ok(compose_path);
    }

    // Check for compose.yaml in the /pds directory
    let pds_compose_path = PathBuf::from("/pds/compose.yaml");
    if pds_compose_path.exists() {
        return Ok(pds_compose_path);
    }

    Err(anyhow::anyhow!("Could not find compose.yaml file"))
}

/// Check if diesel CLI is installed
pub fn check_diesel_cli() -> bool {
    which::which("diesel").is_ok()
}

/// Check if we're running in a container
pub fn is_in_container() -> bool {
    env::var("RSKY_PDS_CONTAINER")
        .map(|val| val == "true")
        .unwrap_or(false)
}

/// Get the database URL
pub fn get_database_url() -> Result<String> {
    let default_db_url = if is_in_container() {
        "postgres://postgres:postgres@postgres:5432/postgres".to_string()
    } else {
        "postgres://postgres:postgres@localhost:5678/postgres".to_string()
    };

    Ok(env::var("DATABASE_URL").unwrap_or(default_db_url))
}
