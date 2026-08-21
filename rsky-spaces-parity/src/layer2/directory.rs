//! A stand-in DID directory, so neither server reaches the network.
//!
//! It answers every path with a DID document for the requested DID: the
//! `#atproto` signing key the gate controls, and a PDS service endpoint
//! pointing back at itself, which absorbs best-effort write notifications.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;

pub struct Directory {
    pub port: u16,
}

impl Directory {
    /// Bind an ephemeral port and serve until the process exits.
    pub fn start(keys: BTreeMap<String, String>, handle: String) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut buf = [0u8; 8192];
                let read = stream.read(&mut buf).unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..read]).to_string();
                let path = request.split_whitespace().nth(1).unwrap_or("/").to_string();
                let did = path
                    .trim_start_matches('/')
                    .split('?')
                    .next()
                    .unwrap_or_default()
                    .replace("%3A", ":")
                    .replace("%3a", ":");
                let multibase = keys
                    .get(&did)
                    .or_else(|| keys.values().next())
                    .cloned()
                    .unwrap_or_default();
                let body = serde_json::json!({
                    "id": did,
                    "alsoKnownAs": [format!("at://{handle}")],
                    "verificationMethod": [{
                        "id": format!("{did}#atproto"),
                        "type": "Multikey",
                        "controller": did,
                        "publicKeyMultibase": multibase,
                    }],
                    "service": [{
                        "id": "#atproto_pds",
                        "type": "AtprotoPersonalDataServer",
                        "serviceEndpoint": format!("http://127.0.0.1:{port}"),
                    }],
                })
                .to_string();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        Ok(Self { port })
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}
