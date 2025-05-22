# RSKY-PDS Admin CLI

A Rust command-line utility for administering RSKY-PDS (Personal Data Server) instances. This tool is the Rust equivalent of the `pdsadmin.sh` script from the Bluesky PDS, built specifically for RSKY.

## Features

- **Account Management**: Create, list, delete, and reset passwords for accounts
- **Invite Code Management**: Generate invite codes for new users
- **PDS Administration**: Request crawls from relays, update the PDS to the latest version
- **Database Management**: Initialize and manage the RSKY-PDS database
- **Extensibility**: Easily add new commands through a modular architecture

## Installation

### From Source

```bash
# Clone the repository (if you haven't already)
git clone https://github.com/blacksky-algorithms/rsky.git
cd rsky

# Build the pdsadmin tool
cargo build --package rsky-pdsadmin

# Optionally, install it for system-wide access
cargo install --path rsky-pdsadmin
```

### Features

If you want to include the embedded database CLI:

```bash
cargo build --package rsky-pdsadmin --features db_cli
```

## Configuration

The CLI looks for a PDS environment file in the following locations (in order):

1. The path specified in the `PDS_ENV_FILE` environment variable
2. `./pds.env` (current working directory)
3. `/pds/pds.env`
4. `/usr/src/rsky/pds.env`
5. `$HOME/.config/rsky/rsky-pds/pds.env`

The environment file should contain the necessary configuration for connecting to your RSKY-PDS instance, including:

```
PDS_HOSTNAME=your-pds-hostname.com
PDS_ADMIN_PASSWORD=your-admin-password
```

## Usage

### General Help

```bash
pdsadmin help
```

### Account Management

List all accounts:
```bash
pdsadmin account list
```

Create a new account:
```bash
pdsadmin account create <EMAIL> <HANDLE>
```

Reset an account password:
```bash
pdsadmin account reset-password <DID>
```

Delete an account:
```bash
pdsadmin account delete <DID>
```

Takedown an account:
```bash
pdsadmin account takedown <DID>
```

Remove a takedown:
```bash
pdsadmin account untakedown <DID>
```

### Invite Codes

Create a new invite code:
```bash
pdsadmin create-invite-code
```

### Relay Interaction

Request a crawl from a relay:
```bash
pdsadmin request-crawl [RELAY_HOST]
```

### Updates

Update to the latest PDS version:
```bash
pdsadmin update
```

### RSKY-PDS Specific Commands

Initialize the database:
```bash
pdsadmin rsky-pds init-db
```

## Extending the CLI

The RSKY-PDS Admin CLI is designed to be easily extensible. You can add new commands by:

1. Creating a new module in `src/commands/`
2. Implementing the command logic
3. Adding the command to the `Commands` enum in `src/commands/mod.rs`
4. Adding the command execution to the `execute` function in `src/commands/mod.rs`

### External Commands

RSKY-PDS Admin also supports external commands through the PATH. Create executables named `rsky-pdsadmin-<command>` and place them in your PATH. The CLI will automatically discover and use them.

## Running in a Container

RSKY-PDS Admin detects if it's running inside a container by checking for the `RSKY_PDS_CONTAINER` environment variable. When running in a container, it adjusts its behavior accordingly, particularly for database connections.

## Development

### Prerequisites

- Rust (latest stable)
- PostgreSQL client libraries (for diesel)

### Building

```bash
cargo build
```

### Testing

```bash
cargo test
```

### Linting

```bash
cargo clippy
cargo fmt
```

## License

MIT License