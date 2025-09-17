# rsky-relay

An ATproto-compliant relay server implementation written in Rust.

## Overview

This project implements an ATproto relay that allows you to participate in the AT Protocol network. It handles subscription to repository updates via the `com.atproto.sync.subscribeRepos` endpoint and provides relay functionality to other services.

## Prerequisites

- Rust and Cargo
- Python 3 (for running the crawler script)
- SSL certificates (optional, for HTTPS support)
- `websocat` (for testing WebSocket connections)

## Setup and Installation

### Clone the Repository

```bash
git clone https://github.com/yourusername/rsky-relay.git
cd rsky-relay
```

### Building the Project

```bash
cargo build --release
```

## Usage

### 1. Run the Crawler (Optional)

The crawler script collects necessary data from the network:

```bash
cd rsky-relay
uv init .
uv add requests
uv run python3 crawler.py
```

Note: You can stop the crawler after a few requests and then run the relay with the `--no-plc-export` flag.

### 2. Generate SSL Certificates (Optional for HTTPS)

If you want to use HTTPS, generate SSL certificates using the provided script:

```bash
./ssl.sh <local ip>
```

To find your local IP address, you can use:

```bash
ip a
```

Note: You can skip this step if you don't need HTTPS. In that case, don't specify `-c` and `-p` options when running the relay.

### 3. Run the Relay Server

Start the relay server with debug logging:

```bash
RUST_LOG='rsky_relay=debug' cargo run -rp rsky-relay -- -c <local ip>.crt -p <local ip>.key
```

For non-HTTPS mode:

```bash
RUST_LOG='rsky_relay=debug' cargo run -rp rsky-relay
```

### 4. Test the Connection

You can test the WebSocket connection using `websocat`:

```bash
websocat -k wss://localhost:9000/xrpc/com.atproto.sync.subscribeRepos?cursor=0
```

You can test the HTTP endpoints using `curl`:

```bash
curl https://localhost:9000/xrpc/com.atproto.sync.listHosts?limit=10
```

## Command-Line Options

- `-c, --cert <FILE>`: Path to SSL certificate file
- `-p, --key <FILE>`: Path to SSL private key file
- `--no-plc-export`: Run the relay without requiring PLC export data (useful after running the crawler for only a short time)

## Logging

rsky-relay uses the `RUST_LOG` environment variable to control log levels. Example:

```bash
RUST_LOG='rsky_relay=debug' cargo run -rp rsky-relay
```
