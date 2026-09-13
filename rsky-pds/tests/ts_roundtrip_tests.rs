//! The reference TypeScript PDS and rsky over one data directory: sessions
//! minted on either side are honoured by the other, a record rsky writes
//! is read back by TypeScript, and the firehose frame TypeScript emits for
//! that write is byte-identical to the one rsky would emit.
//!
//! Needs Docker and `TS_PDS_IMAGE` (for example `blacksky/pds:0.5.27-bsky.3`);
//! without them the test reports that it skipped and passes.

mod common;

use common::get_client_in;
use rocket::http::{ContentType, Header, Status};
use rocket::local::asynchronous::Client;
use rsky_common::time::from_str_to_utc;
use rsky_lexicon::com::atproto::sync::{SubscribeReposCommit, SubscribeReposCommitOperation};
use rsky_pds::sequencer::events::{CommitEvt, SeqEvt, TypedCommitEvt};
use rsky_pds::sequencer::RequestSeqRangeOpts;
use rsky_pds::xrpc_server::stream::frames::{Frame, MessageFrame, MessageFrameOpts};
use rsky_pds::SharedSequencer;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const JWT_SECRET: &str = "roundtrip-jwt-secret-0123456789abcdef";
const DPOP_SECRET: &str = "b2b1e4c9d7a3f1e0c5d4b3a2918273645546372819aabbccddeeff0011223344";
const ADMIN_PASSWORD: &str = "3ed1c7b568d3328c44430add531a099f";
const HOSTNAME: &str = "rsky.com";

/// A PLC directory both processes can reach: the container through
/// host.docker.internal, the rocket through loopback. Stores the genesis
/// operation the TypeScript PDS submits and answers the document, audit
/// log, and data forms from it.
fn start_shared_plc() -> u16 {
    let listener = TcpListener::bind("0.0.0.0:0").expect("bind shared plc");
    let port = listener.local_addr().unwrap().port();
    let ops: Arc<Mutex<HashMap<String, Value>>> = Arc::new(Mutex::new(HashMap::new()));
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = Vec::new();
            let mut chunk = [0u8; 8192];
            loop {
                let n = stream.read(&mut chunk).unwrap_or(0);
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
                if let Some(head_end) = find(&buf, b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
                    let length = head
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                        })
                        .unwrap_or(0);
                    if buf.len() >= head_end + 4 + length {
                        break;
                    }
                }
            }
            let text = String::from_utf8_lossy(&buf).to_string();
            let mut lines = text.lines();
            let request_line = lines.next().unwrap_or("").to_string();
            let mut parts = request_line.split_whitespace();
            let method = parts.next().unwrap_or("GET").to_string();
            let path = parts
                .next()
                .unwrap_or("/")
                .replace("%3A", ":")
                .replace("%3a", ":");
            let body_start = find(buf.as_slice(), b"\r\n\r\n")
                .map(|i| i + 4)
                .unwrap_or(buf.len());
            let body = String::from_utf8_lossy(&buf[body_start..]).to_string();
            let did = path
                .trim_start_matches('/')
                .split('/')
                .next()
                .unwrap_or("")
                .to_string();
            let (status, response) = if method == "POST" {
                if let Ok(op) = serde_json::from_str::<Value>(&body) {
                    ops.lock().unwrap().insert(did.clone(), op);
                }
                ("200 OK", String::new())
            } else {
                let op = ops.lock().unwrap().get(&did).cloned();
                match op {
                    None => (
                        "404 Not Found",
                        "{\"message\":\"DID not registered\"}".to_string(),
                    ),
                    Some(op) => {
                        let endpoint = op["services"]["atproto_pds"]["endpoint"]
                            .as_str()
                            .unwrap_or("")
                            .to_string();
                        if path.ends_with("/log/audit") {
                            ("200 OK", json!([{"did": did, "cid": "op0", "nullified": false, "createdAt": "2026-01-01T00:00:00.000Z", "operation": op}]).to_string())
                        } else if path.ends_with("/data") {
                            ("200 OK", json!({"did": did, "alsoKnownAs": op["alsoKnownAs"], "verificationMethods": op["verificationMethods"], "rotationKeys": op["rotationKeys"], "services": op["services"]}).to_string())
                        } else {
                            let key = op["verificationMethods"]["atproto"]
                                .as_str()
                                .unwrap_or("")
                                .trim_start_matches("did:key:")
                                .to_string();
                            ("200 OK", json!({
                                "@context": ["https://www.w3.org/ns/did/v1", "https://w3id.org/security/multikey/v1", "https://w3id.org/security/suites/secp256k1-2019/v1"],
                                "id": did,
                                "alsoKnownAs": op["alsoKnownAs"],
                                "verificationMethod": [{"id": format!("{did}#atproto"), "type": "Multikey", "controller": did, "publicKeyMultibase": key}],
                                "service": [{"id": "#atproto_pds", "type": "AtprotoPersonalDataServer", "serviceEndpoint": endpoint}],
                            }).to_string())
                        }
                    }
                }
            };
            let _ = stream.write_all(
                format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).as_bytes(),
            );
        }
    });
    port
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

struct TsContainer {
    name: String,
    port: u16,
}

impl TsContainer {
    async fn start(image: &str, data: &std::path::Path, plc_port: u16) -> Self {
        let port = free_port();
        let name = format!("rsky-roundtrip-{}", std::process::id());
        let status = Command::new("docker")
            .args(["run", "-d", "--name", &name, "-p", &format!("127.0.0.1:{port}:{port}")])
            .args(["-v", &format!("{}:/pds", data.display())])
            .args(["--add-host", "host.docker.internal:host-gateway"])
            .args(["-e", &format!("PDS_PORT={port}"), "-e", &format!("PDS_HOSTNAME={HOSTNAME}")])
            .args(["-e", "PDS_DATA_DIRECTORY=/pds", "-e", "PDS_BLOBSTORE_DISK_LOCATION=/pds/blocks"])
            .args(["-e", &format!("PDS_DID_PLC_URL=http://host.docker.internal:{plc_port}")])
            .args(["-e", &format!("PDS_JWT_SECRET={JWT_SECRET}"), "-e", &format!("PDS_DPOP_SECRET={DPOP_SECRET}")])
            .args(["-e", &format!("PDS_ADMIN_PASSWORD={ADMIN_PASSWORD}")])
            .args(["-e", "PDS_PLC_ROTATION_KEY_K256_PRIVATE_KEY_HEX=fb478b39dd2ddf84bef135dd60f90381903eefadbb9df4b18a2b9b174ae72582"])
            .args(["-e", &format!("PDS_SERVICE_HANDLE_DOMAINS=.{HOSTNAME}"), "-e", "PDS_SERVICE_NAME=Roundtrip"])
            .args(["-e", "PDS_INVITE_REQUIRED=false", "-e", "PDS_DEV_MODE=true"])
            .args(["-e", "PDS_BSKY_APP_VIEW_URL=http://host.docker.internal:1", "-e", "PDS_BSKY_APP_VIEW_DID=did:web:appview.invalid"])
            .args(["-e", "PDS_CRAWLERS=", "-e", "LOG_ENABLED=true", "-e", "LOG_LEVEL=warn"])
            .arg(image)
            .status()
            .expect("docker run");
        assert!(status.success(), "docker run failed");
        let container = Self { name, port };
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(90) {
            if let Ok(response) = reqwest::get(container.url("/xrpc/_health")).await {
                if response.status().is_success() {
                    return container;
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        let logs = Command::new("docker")
            .args(["logs", "--tail", "50", &container.name])
            .output()
            .unwrap();
        panic!(
            "the TypeScript PDS never became healthy:\n{}",
            String::from_utf8_lossy(&logs.stderr)
        );
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{}", self.port, path)
    }

    async fn post(&self, path: &str, body: Value, token: Option<&str>) -> (u16, Value) {
        let client = reqwest::Client::new();
        let mut request = client.post(self.url(path)).json(&body);
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        let response = request.send().await.expect("request to the TypeScript PDS");
        let status = response.status().as_u16();
        (status, response.json().await.unwrap_or(Value::Null))
    }

    async fn get(&self, path: &str, token: Option<&str>) -> (u16, Value) {
        let client = reqwest::Client::new();
        let mut request = client.get(self.url(path));
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        let response = request.send().await.expect("request to the TypeScript PDS");
        let status = response.status().as_u16();
        (status, response.json().await.unwrap_or(Value::Null))
    }

    /// Binary websocket messages from the firehose, from `cursor`, until
    /// `stop` accepts one or the deadline passes.
    fn firehose_until(
        port: u16,
        cursor: i64,
        stop: impl Fn(&[u8]) -> bool,
        deadline: Duration,
    ) -> Option<Vec<u8>> {
        use base64::Engine;
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.set_read_timeout(Some(deadline)).unwrap();
        let key = base64::engine::general_purpose::STANDARD
            .encode(std::process::id().to_le_bytes().repeat(4));
        stream
            .write_all(
                format!(
                    "GET /xrpc/com.atproto.sync.subscribeRepos?cursor={cursor} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
                )
                .as_bytes(),
            )
            .unwrap();
        let mut buf = Vec::new();
        let mut chunk = [0u8; 65536];
        let head_end = loop {
            let n = stream.read(&mut chunk).unwrap_or(0);
            if n == 0 {
                return None;
            }
            buf.extend_from_slice(&chunk[..n]);
            if let Some(i) = find(&buf, b"\r\n\r\n") {
                break i + 4;
            }
        };
        assert!(
            String::from_utf8_lossy(&buf[..head_end]).starts_with("HTTP/1.1 101"),
            "websocket handshake refused"
        );
        buf.drain(..head_end);
        let started = Instant::now();
        let mut message = Vec::new();
        loop {
            while buf.len() < 2 {
                let n = stream.read(&mut chunk).unwrap_or(0);
                if n == 0 || started.elapsed() > deadline {
                    return None;
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            let fin = buf[0] & 0x80 != 0;
            let opcode = buf[0] & 0x0F;
            let mut length = (buf[1] & 0x7F) as usize;
            let mut offset = 2;
            let extra = match length {
                126 => 2,
                127 => 8,
                _ => 0,
            };
            while buf.len() < offset + extra {
                let n = stream.read(&mut chunk).unwrap_or(0);
                if n == 0 {
                    return None;
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            if extra == 2 {
                length = u16::from_be_bytes([buf[2], buf[3]]) as usize;
            } else if extra == 8 {
                length = u64::from_be_bytes(buf[2..10].try_into().unwrap()) as usize;
            }
            offset += extra;
            while buf.len() < offset + length {
                let n = stream.read(&mut chunk).unwrap_or(0);
                if n == 0 || started.elapsed() > deadline {
                    return None;
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            let payload = buf[offset..offset + length].to_vec();
            buf.drain(..offset + length);
            match opcode {
                0x8 => return None,
                0x9 | 0xA => continue,
                _ => {}
            }
            message.extend_from_slice(&payload);
            if fin {
                let complete = std::mem::take(&mut message);
                if stop(&complete) {
                    return Some(complete);
                }
            }
        }
    }
}

impl Drop for TsContainer {
    fn drop(&mut self) {
        let _ = Command::new("docker")
            .args(["rm", "-f", &self.name])
            .output();
    }
}

async fn rocket_json(
    client: &Client,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (Status, Value) {
    let mut request = match method {
        "POST" => client.post(path),
        _ => client.get(path),
    };
    if let Some(token) = token {
        request = request.header(Header::new("Authorization", format!("Bearer {token}")));
    }
    if let Some(body) = body {
        request = request.header(ContentType::JSON).body(body.to_string());
    }
    let response = request.dispatch().await;
    let status = response.status();
    (status, response.into_json().await.unwrap_or(Value::Null))
}

/// The frame rsky emits for a commit row, built the way its firehose does.
fn rsky_frame(evt: &TypedCommitEvt) -> Vec<u8> {
    let TypedCommitEvt {
        r#type,
        seq,
        time,
        evt,
    } = evt.clone();
    let CommitEvt {
        rebase,
        too_big,
        repo,
        commit,
        prev,
        rev,
        since,
        blocks,
        ops,
        blobs,
        prev_data,
    } = evt;
    let body = SubscribeReposCommit {
        seq,
        time: from_str_to_utc(&time).unwrap(),
        rebase,
        too_big,
        repo,
        commit,
        prev,
        rev,
        since,
        blocks,
        ops: ops
            .into_iter()
            .map(|op| SubscribeReposCommitOperation {
                path: op.path,
                cid: op.cid,
                prev: op.prev,
                action: op.action.to_string(),
            })
            .collect(),
        blobs,
        prev_data,
    };
    MessageFrame::new(
        body,
        Some(MessageFrameOpts {
            r#type: Some(format!("#{type}")),
        }),
    )
    .to_bytes()
    .unwrap()
}

#[tokio::test]
async fn sessions_records_and_frames_round_trip_between_the_implementations() {
    let Ok(image) = std::env::var("TS_PDS_IMAGE") else {
        eprintln!("TS_PDS_IMAGE is not set; skipping the round-trip test");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    std::fs::create_dir_all(data.join("blocks")).unwrap();
    let plc_port = start_shared_plc();

    // both processes share the secrets, the hostname, and the directory
    std::env::set_var("PDS_JWT_SECRET", JWT_SECRET);
    std::env::set_var("PDS_DPOP_SECRET", DPOP_SECRET);
    std::env::set_var("PDS_ADMIN_PASS", ADMIN_PASSWORD);
    std::env::set_var("PDS_HOSTNAME", HOSTNAME);
    std::env::set_var("PDS_SERVICE_DID", format!("did:web:{HOSTNAME}"));
    std::env::set_var("PDS_SERVICE_HANDLE_DOMAINS", format!(".{HOSTNAME}"));
    std::env::set_var("PDS_DEV_MODE", "true");
    std::env::set_var("PDS_COEXISTENCE", "true");
    std::env::set_var("PDS_DID_PLC_URL", format!("http://127.0.0.1:{plc_port}"));
    std::env::set_var("PDS_BLOBSTORE_DISK_LOCATION", data.join("blocks"));
    let allowlist = dir.path().join("write-allowlist.toml");
    std::fs::write(&allowlist, "version = 1\ndefault = \"absent\"\n[entries]\n").unwrap();
    std::env::set_var("PDS_WRITE_ALLOWLIST_FILE", &allowlist);
    std::env::set_var("PDS_LOCK_DIR", dir.path().join("rsky/locks"));

    let ts = TsContainer::start(&image, &data, plc_port).await;

    // the account is born on TypeScript, as every real account was
    let (status, created) = ts
        .post(
            "/xrpc/com.atproto.server.createAccount",
            json!({"email": "alice@example.test", "handle": format!("alice.{HOSTNAME}"), "password": "alice-password"}),
            None,
        )
        .await;
    assert_eq!(status, 200, "{created}");
    let did = created["did"].as_str().unwrap().to_string();
    let ts_access = created["accessJwt"].as_str().unwrap().to_string();
    let ts_refresh = created["refreshJwt"].as_str().unwrap().to_string();

    // rsky is admitted as this account's writer before it boots
    std::fs::write(
        &allowlist,
        format!("version = 1\ndefault = \"absent\"\n[entries]\n\"{did}\" = \"active\"\n"),
    )
    .unwrap();
    let rsky = get_client_in(&data).await;

    // a TypeScript session is honoured by rsky
    let (status, session) = rocket_json(
        &rsky,
        "GET",
        "/xrpc/com.atproto.server.getSession",
        Some(&ts_access),
        None,
    )
    .await;
    assert_eq!(status, Status::Ok, "{session}");
    assert_eq!(session["did"], did);
    assert_eq!(session["active"], true);
    assert_eq!(session["handle"], format!("alice.{HOSTNAME}"));

    // a refresh on rsky yields tokens TypeScript honours
    let (status, refreshed) = rocket_json(
        &rsky,
        "POST",
        "/xrpc/com.atproto.server.refreshSession",
        Some(&ts_refresh),
        None,
    )
    .await;
    assert_eq!(status, Status::Ok, "{refreshed}");
    let rsky_access = refreshed["accessJwt"].as_str().unwrap().to_string();
    let (status, seen) = ts
        .get("/xrpc/com.atproto.server.getSession", Some(&rsky_access))
        .await;
    assert_eq!(status, 200, "{seen}");
    assert_eq!(seen["did"], did);
    let (status, ts_seen) = ts
        .get("/xrpc/com.atproto.server.getSession", Some(&ts_access))
        .await;
    assert_eq!(status, 200);
    let stable = |v: &Value| {
        let mut v = v.clone();
        if let Some(map) = v.as_object_mut() {
            map.remove("accessJwt");
            map.remove("refreshJwt");
        }
        v
    };
    assert_eq!(
        stable(&session),
        stable(&ts_seen),
        "getSession shapes differ"
    );

    // rsky writes; TypeScript reads the record and the same latest commit
    let (status, written) = rocket_json(
        &rsky,
        "POST",
        "/xrpc/com.atproto.repo.createRecord",
        Some(&rsky_access),
        Some(json!({"repo": did, "collection": "app.bsky.feed.post", "rkey": "roundtrip", "record": {"$type": "app.bsky.feed.post", "text": "written by rsky", "createdAt": "2026-09-12T00:00:00.000Z"}})),
    )
    .await;
    assert_eq!(status, Status::Ok, "{written}");
    let (status, read_back) = ts.get(&format!("/xrpc/com.atproto.repo.getRecord?repo={did}&collection=app.bsky.feed.post&rkey=roundtrip"), None).await;
    assert_eq!(status, 200, "{read_back}");
    assert_eq!(read_back["cid"], written["cid"]);
    assert_eq!(read_back["value"]["text"], "written by rsky");
    let (_, ts_latest) = ts
        .get(
            &format!("/xrpc/com.atproto.sync.getLatestCommit?did={did}"),
            None,
        )
        .await;
    let (_, rsky_latest) = rocket_json(
        &rsky,
        "GET",
        &format!("/xrpc/com.atproto.sync.getLatestCommit?did={did}"),
        None,
        None,
    )
    .await;
    assert_eq!(
        ts_latest, rsky_latest,
        "latest commit differs between the readers"
    );

    // the frame TypeScript streams for rsky's write is the frame rsky would stream
    let sequencer = rsky.rocket().state::<SharedSequencer>().unwrap();
    let events = sequencer
        .sequencer
        .read()
        .await
        .request_seq_range(RequestSeqRangeOpts {
            earliest_seq: Some(0),
            latest_seq: None,
            earliest_time: None,
            limit: Some(1000),
        })
        .await
        .unwrap();
    let commit = events
        .iter()
        .filter_map(|evt| match evt {
            SeqEvt::TypedCommitEvt(commit)
                if commit.evt.repo == did
                    && commit.evt.rev == rsky_latest["rev"].as_str().unwrap() =>
            {
                Some(commit.as_ref().clone())
            }
            _ => None,
        })
        .next_back()
        .expect("rsky's commit is in the shared sequencer");
    let expected = rsky_frame(&commit);
    let seq = commit.seq;
    let port = ts.port;
    let wanted = expected.clone();
    let received = tokio::task::spawn_blocking(move || {
        TsContainer::firehose_until(
            port,
            seq - 1,
            |frame| frame == wanted.as_slice() || frame_seq(frame) == Some(seq),
            Duration::from_secs(30),
        )
    })
    .await
    .unwrap()
    .expect("TypeScript streamed the commit");
    assert_eq!(
        received,
        expected,
        "frame bytes differ:\n ts   {}\n rsky {}",
        hex(&received),
        hex(&expected)
    );
    drop(ts);
}

fn frame_seq(frame: &[u8]) -> Option<i64> {
    let mut cursor = std::io::Cursor::new(frame);
    let _header: Value = serde_ipld_dagcbor::from_reader(&mut cursor).ok()?;
    let body: Value = serde_ipld_dagcbor::from_reader(&mut cursor).ok()?;
    body.get("seq")?.as_i64()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
