//! An in-process stand-in for a PLC directory, so the rotation paths can be
//! exercised without reaching the network.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

#[derive(Debug, Default)]
pub(crate) struct MockPlcState {
    /// did -> the `atproto` verification method the directory currently serves
    pub keys: BTreeMap<String, String>,
    /// every operation the directory accepted, in order
    pub posted: Vec<serde_json::Value>,
}

pub(crate) struct MockPlc {
    pub url: String,
    pub state: Arc<Mutex<MockPlcState>>,
}

impl MockPlc {
    pub fn start(rotation_key: &str, keys: BTreeMap<String, String>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock plc directory");
        let url = format!("http://{}", listener.local_addr().unwrap());
        let state = Arc::new(Mutex::new(MockPlcState {
            keys,
            posted: Vec::new(),
        }));
        let thread_state = state.clone();
        let rotation_key = rotation_key.to_owned();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                handle(&mut stream, &thread_state, &rotation_key);
            }
        });
        MockPlc { url, state }
    }

    pub fn published_key(&self, did: &str) -> Option<String> {
        self.state.lock().unwrap().keys.get(did).cloned()
    }

    pub fn posted(&self) -> Vec<serde_json::Value> {
        self.state.lock().unwrap().posted.clone()
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn handle(stream: &mut TcpStream, state: &Arc<Mutex<MockPlcState>>, rotation_key: &str) {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 1024];
    let head_end = loop {
        let n = match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => n,
        };
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = find_subslice(&buf, b"\r\n\r\n") {
            break pos + 4;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut request_line = head.split_whitespace();
    let method = request_line.next().unwrap_or("").to_owned();
    let path = request_line.next().unwrap_or("/").to_owned();
    let content_length = head
        .lines()
        .find_map(|line| {
            let lower = line.to_ascii_lowercase();
            let value = lower.strip_prefix("content-length:")?;
            value.trim().parse::<usize>().ok()
        })
        .unwrap_or(0);
    while buf.len() < head_end + content_length {
        let n = match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        buf.extend_from_slice(&chunk[..n]);
    }
    let body = buf[head_end..].to_vec();

    let path = path.replace("%3A", ":").replace("%3a", ":");
    let segments: Vec<&str> = path.trim_matches('/').split('/').collect();
    let did = segments.first().copied().unwrap_or_default().to_owned();
    let current = state
        .lock()
        .unwrap()
        .keys
        .get(&did)
        .cloned()
        .unwrap_or_default();

    let response_body = match (method.as_str(), segments.len()) {
        ("GET", 2) if segments[1] == "data" => serde_json::json!({
            "did": did,
            "rotationKeys": [rotation_key],
            "verificationMethods": { "atproto": current },
            "alsoKnownAs": [format!("at://{did}.test")],
            "services": {
                "atproto_pds": {
                    "type": "AtprotoPersonalDataServer",
                    "endpoint": "https://pds.test"
                }
            }
        })
        .to_string(),
        ("GET", 3) if segments[1] == "log" && segments[2] == "last" => serde_json::json!({
            "type": "plc_operation",
            "rotationKeys": [rotation_key],
            "verificationMethods": { "atproto": current },
            "alsoKnownAs": [format!("at://{did}.test")],
            "services": {
                "atproto_pds": {
                    "type": "AtprotoPersonalDataServer",
                    "endpoint": "https://pds.test"
                }
            },
            "prev": null,
            "sig": "c2ln"
        })
        .to_string(),
        ("POST", 1) => {
            let op: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
            let mut guard = state.lock().unwrap();
            if let Some(key) = op
                .get("verificationMethods")
                .and_then(|methods| methods.get("atproto"))
                .and_then(|key| key.as_str())
            {
                guard.keys.insert(did, key.to_owned());
            }
            guard.posted.push(op);
            "{}".to_owned()
        }
        _ => {
            let _ = stream.write_all(
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
            return;
        }
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        response_body.len(),
        response_body
    );
    let _ = stream.write_all(response.as_bytes());
}
