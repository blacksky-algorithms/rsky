//! A stand-in projection destination: it accepts the daemon's
//! `projectRecords` batches and keeps them, so the gate can say exactly which
//! operations were projected, in which order, and how many times.

use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

/// One projected operation, reduced to what the gate compares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Projected {
    /// The batch this operation arrived in, counted from 1 per sink.
    pub batch: usize,
    pub uri: String,
    pub operation: String,
    pub revision: String,
    pub cid: Option<String>,
}

impl Projected {
    /// `collection/rkey`, the part of the URI that identifies the record.
    pub fn path(&self) -> String {
        let mut segments = self.uri.rsplitn(3, '/');
        let rkey = segments.next().unwrap_or_default();
        let collection = segments.next().unwrap_or_default();
        format!("{collection}/{rkey}")
    }

    /// `collection/rkey:operation`, the label the gate asserts on.
    pub fn label(&self) -> String {
        format!("{}:{}", self.path(), self.operation)
    }
}

#[derive(Default)]
struct State {
    ops: Vec<Projected>,
    batches: usize,
    acknowledgements: usize,
}

pub struct Sink {
    port: u16,
    state: Arc<Mutex<State>>,
}

impl Sink {
    /// Bind an ephemeral port and serve until the process exits.
    pub fn start() -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let state = Arc::new(Mutex::new(State::default()));
        let served = state.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let (path, body) = match read_request(&mut stream) {
                    Some(request) => request,
                    None => continue,
                };
                record(&served, &path, &body);
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                      Content-Length: 2\r\nConnection: close\r\n\r\n{}",
                );
            }
        });
        Ok(Self { port, state })
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// Everything received so far, without removing it.
    pub fn seen(&self) -> Vec<Projected> {
        self.state.lock().expect("sink state").ops.clone()
    }

    /// Take everything received so far, leaving the sink empty, so the next
    /// phase's projections are read in isolation.
    pub fn drain(&self) -> Vec<Projected> {
        std::mem::take(&mut self.state.lock().expect("sink state").ops)
    }

    pub fn batches(&self) -> usize {
        self.state.lock().expect("sink state").batches
    }

    pub fn acknowledgements(&self) -> usize {
        self.state.lock().expect("sink state").acknowledgements
    }
}

fn record(state: &Arc<Mutex<State>>, path: &str, body: &str) {
    let mut state = state.lock().expect("sink state");
    if path.contains("ackSyncersObserved") {
        state.acknowledgements += 1;
        return;
    }
    if !path.contains("projectRecords") {
        return;
    }
    let Ok(parsed) = serde_json::from_str::<Value>(body) else {
        return;
    };
    state.batches += 1;
    let batch = state.batches;
    let ops = parsed["ops"].as_array().cloned().unwrap_or_default();
    for op in ops {
        state.ops.push(Projected {
            batch,
            uri: op["uri"].as_str().unwrap_or_default().to_string(),
            operation: op["operation"].as_str().unwrap_or_default().to_string(),
            revision: op["revision"].as_str().unwrap_or_default().to_string(),
            cid: op["cid"].as_str().map(str::to_string),
        });
    }
}

/// Read one request, returning its path and body. Only the request line and
/// `content-length` are interpreted; the daemon sends no chunked bodies.
fn read_request(stream: &mut std::net::TcpStream) -> Option<(String, String)> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).ok()? == 0 {
        return None;
    }
    let path = request_line.split_whitespace().nth(1)?.to_string();
    let mut length = 0usize;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).ok()? == 0 {
            break;
        }
        if header.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                length = value.trim().parse().unwrap_or(0);
            }
        }
    }
    let mut body = vec![0u8; length];
    if length > 0 {
        reader.read_exact(&mut body).ok()?;
    }
    Some((path, String::from_utf8_lossy(&body).to_string()))
}
