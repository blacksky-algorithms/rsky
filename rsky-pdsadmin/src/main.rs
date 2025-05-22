use anyhow::Result;
use std::process;

fn main() {
    if let Err(err) = run() {
        // Print the main error message
        eprintln!("ERROR: {}", err);

        // If available, print the error chain to provide more context
        let mut source = err.source();
        if source.is_some() {
            eprintln!("\nError details:");
            while let Some(err) = source {
                eprintln!("  - {}", err);
                source = err.source();
            }

            // Provide general troubleshooting tips
            eprintln!("\nTroubleshooting tips:");
            eprintln!("  - Ensure that the PDS server is running and accessible");
            eprintln!("  - Check that environment variables are properly set in pds.env");
            eprintln!("  - Verify your network connection and firewall settings");
        }

        process::exit(1);
    }
}

fn run() -> Result<()> {
    // Check if running as root for most commands
    if !cfg!(windows) && !is_running_as_root() && !is_running_in_container() {
        let args: Vec<String> = std::env::args().collect();
        if args.len() > 1
            && args[1] != "help"
            && args[1] != "--help"
            && args[1] != "-h"
            && args[1] != "--version"
            && args[1] != "-V"
        {
            eprintln!("ERROR: This command must be run as root");
            process::exit(1);
        }
    }

    rsky_pdsadmin::run()
}

fn is_running_as_root() -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match std::fs::metadata("/") {
            Ok(metadata) => metadata.uid() == 0,
            Err(_) => false,
        }
    }
    #[cfg(not(unix))]
    {
        false
    }
}

fn is_running_in_container() -> bool {
    std::env::var("RSKY_PDS_CONTAINER")
        .map(|val| val == "true")
        .unwrap_or(false)
}
