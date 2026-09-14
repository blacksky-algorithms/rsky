//! The router against mock upstreams: every routing decision, the
//! write-ahead journal, failover, and policy reload.

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use base64::Engine;
use rsky_pds_router::journal::{Journal, Sink};
use rsky_pds_router::lookup::AccountLookup;
use rsky_pds_router::policy::Routing;
use rsky_pds_router::server::{app, metrics_app, Deadlines, Router, Upstreams, JSON_BODY_LIMIT};
use serde_json::{json, Value};
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;

const ALICE: &str = "did:plc:alice";
const BOB: &str = "did:plc:bob";
const CANARY: &str = "did:plc:canary";

/// A mock PDS that echoes what it received and reports its name.
#[derive(Clone)]
struct Mock {
    name: &'static str,
    url: String,
    hits: Arc<AtomicUsize>,
    release: Arc<tokio::sync::Notify>,
}

async fn echo(
    State(mock): State<Mock>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, [(&'static str, &'static str); 1], String) {
    mock.hits.fetch_add(1, Ordering::SeqCst);
    if headers.contains_key("x-mock-hold") {
        mock.release.notified().await;
    }
    if uri.path().ends_with("slow") {
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
    let status = headers
        .get("x-mock-status")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
        .unwrap_or(200);
    let echoed = json!({
        "backend": mock.name,
        "method": method.as_str(),
        "path": uri.path_and_query().map(|pq| pq.as_str()).unwrap_or(""),
        "host": headers.get("host").and_then(|h| h.to_str().ok()),
        "connection": headers.get("connection").and_then(|h| h.to_str().ok()),
        "body": String::from_utf8_lossy(&body),
    });
    (
        StatusCode::from_u16(status).unwrap(),
        [("content-type", "application/json")],
        echoed.to_string(),
    )
}

impl Mock {
    async fn start(name: &'static str) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let mock = Self {
            name,
            url,
            hits: Arc::new(AtomicUsize::new(0)),
            release: Arc::new(tokio::sync::Notify::new()),
        };
        let router = axum::Router::new().fallback(echo).with_state(mock.clone());
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        mock
    }

    /// A URL nothing listens on.
    async fn closed(name: &'static str) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        Self {
            name,
            url,
            hits: Arc::new(AtomicUsize::new(0)),
            release: Arc::new(tokio::sync::Notify::new()),
        }
    }

    /// A listener that accepts and closes every connection without a
    /// response.
    async fn broken(name: &'static str) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let hits = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&hits);
        tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                counted.fetch_add(1, Ordering::SeqCst);
                drop(stream);
            }
        });
        Self {
            name,
            url,
            hits,
            release: Arc::new(tokio::sync::Notify::new()),
        }
    }

    fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }
}

/// A journal sink that fails after a number of successful lines.
struct FailAfter(usize);

impl Write for FailAfter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.0 == 0 {
            return Err(std::io::Error::other("sink failed"));
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Sink for FailAfter {
    fn sync(&mut self) -> std::io::Result<()> {
        self.0 -= 1;
        Ok(())
    }
}

struct Harness {
    dir: TempDir,
    routing: Arc<Routing>,
    url: String,
    ts_main: Mock,
    ts_sync: Mock,
    ts_read: [Mock; 2],
    rsky: Mock,
    oauth: Mock,
    client: reqwest::Client,
}

const DEFAULT_POLICY: &str = "version = 1\n[reads]\n[writes]\n";
const DEFAULT_ALLOWLIST: &str = "version = 1\n";

impl Harness {
    async fn start() -> Self {
        Self::start_with(None).await
    }

    async fn start_with(rsky: Option<Mock>) -> Self {
        Self::start_full(rsky, None).await
    }

    async fn start_full(rsky: Option<Mock>, journal: Option<Journal>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let policy_path = dir.path().join("policy.toml");
        let allowlist_path = dir.path().join("write-allowlist.toml");
        std::fs::write(&policy_path, DEFAULT_POLICY).unwrap();
        std::fs::write(&allowlist_path, DEFAULT_ALLOWLIST).unwrap();
        let account_db = dir.path().join("account.sqlite");
        let conn = rusqlite::Connection::open(&account_db).unwrap();
        conn.execute_batch(&format!(
            "CREATE TABLE actor (did TEXT PRIMARY KEY, handle TEXT);
             CREATE TABLE account (did TEXT PRIMARY KEY, email TEXT);
             CREATE TABLE email_token (purpose TEXT, did TEXT, token TEXT);
             INSERT INTO actor VALUES ('{ALICE}', 'alice.test'), ('{BOB}', 'bob.test'), ('{CANARY}', 'canary.test');
             INSERT INTO account VALUES ('{ALICE}', 'alice@example.com'), ('{CANARY}', 'canary@example.com');
             INSERT INTO email_token VALUES ('reset_password', '{CANARY}', 'CANARY-TOKEN'), ('reset_password', '{ALICE}', 'ALICE-TOKEN');"
        ))
        .unwrap();
        drop(conn);
        let routing = Arc::new(Routing::new(policy_path, allowlist_path));
        routing.load().unwrap();
        let journal = match journal {
            Some(journal) => journal,
            None => Journal::open(&dir.path().join("mutations.jsonl")).unwrap(),
        };
        let lookup = AccountLookup::open(&account_db).unwrap();
        let ts_main = Mock::start("ts-main").await;
        let ts_sync = Mock::start("ts-sync").await;
        let ts_read = [
            Mock::start("ts-read-1").await,
            Mock::start("ts-read-2").await,
        ];
        let rsky = match rsky {
            Some(rsky) => rsky,
            None => Mock::start("rsky").await,
        };
        let oauth = Mock::start("oauth").await;
        let router = Arc::new(Router::new(
            Arc::clone(&routing),
            Upstreams {
                ts_main: ts_main.url.clone(),
                ts_sync: ts_sync.url.clone(),
                ts_read: ts_read.iter().map(|m| m.url.clone()).collect(),
                rsky: rsky.url.clone(),
                oauth: oauth.url.clone(),
            },
            Deadlines {
                read: Duration::from_millis(500),
                sync: Duration::from_millis(500),
                write: Duration::from_millis(500),
            },
            journal,
            lookup,
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            axum::serve(listener, app(router)).await.unwrap();
        });
        Self {
            dir,
            routing,
            url,
            ts_main,
            ts_sync,
            ts_read,
            rsky,
            oauth,
            client: reqwest::Client::new(),
        }
    }

    fn set_policy(&self, text: &str) {
        std::thread::sleep(Duration::from_millis(10));
        std::fs::write(self.dir.path().join("policy.toml"), text).unwrap();
        self.routing.refresh();
    }

    fn set_allowlist(&self, text: &str) {
        std::thread::sleep(Duration::from_millis(10));
        std::fs::write(self.dir.path().join("write-allowlist.toml"), text).unwrap();
        self.routing.refresh();
    }

    async fn get(&self, path: &str) -> Answer {
        self.send(self.client.get(format!("{}{path}", self.url)))
            .await
    }

    async fn get_as(&self, path: &str, did: &str) -> Answer {
        self.send(
            self.client
                .get(format!("{}{path}", self.url))
                .bearer_auth(jwt_for(did)),
        )
        .await
    }

    async fn post(&self, path: &str, body: Value) -> Answer {
        self.send(self.client.post(format!("{}{path}", self.url)).json(&body))
            .await
    }

    async fn post_as(&self, path: &str, did: &str, body: Value) -> Answer {
        self.send(
            self.client
                .post(format!("{}{path}", self.url))
                .bearer_auth(jwt_for(did))
                .json(&body),
        )
        .await
    }

    async fn send(&self, request: reqwest::RequestBuilder) -> Answer {
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        let backend = header(&response, "x-pds-backend");
        let reason = header(&response, "x-router-reason");
        let retry_after = header(&response, "retry-after");
        let body: Value = response.json().await.unwrap_or(Value::Null);
        Answer {
            status,
            backend,
            reason,
            retry_after,
            body,
        }
    }

    fn journal(&self) -> Vec<Value> {
        std::fs::read_to_string(self.dir.path().join("mutations.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}

fn header(response: &reqwest::Response, name: &str) -> String {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned()
}

fn jwt_for(sub: &str) -> String {
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(json!({ "sub": sub, "scope": "com.atproto.access" }).to_string());
    format!("eyJhbGciOiJIUzI1NiJ9.{payload}.sig")
}

#[derive(Debug)]
struct Answer {
    status: u16,
    backend: String,
    reason: String,
    retry_after: String,
    body: Value,
}

impl Answer {
    fn served_by(&self) -> &str {
        self.body["backend"].as_str().unwrap_or("")
    }

    fn error(&self) -> &str {
        self.body["error"].as_str().unwrap_or("")
    }
}

fn canary_policy(extra: &str) -> String {
    format!("version = 1\n{extra}[reads]\n[writes]\ncanary_rsky = [\"{CANARY}\"]\n")
}

#[tokio::test]
async fn health_is_answered_by_the_router_itself() {
    let h = Harness::start().await;
    let answer = h.get("/xrpc/_health").await;
    assert_eq!(answer.status, 200);
    assert_eq!(answer.backend, "router");
    assert_eq!(answer.reason, "health");
    assert_eq!(h.ts_main.hits() + h.rsky.hits(), 0);
}

#[tokio::test]
async fn reads_go_to_the_pool_for_their_class() {
    let h = Harness::start().await;
    let sync = h
        .get(&format!("/xrpc/com.atproto.sync.getRepo?did={ALICE}"))
        .await;
    assert_eq!((sync.status, sync.served_by()), (200, "ts-sync"));
    assert_eq!(
        (sync.backend.as_str(), sync.reason.as_str()),
        ("ts", "default")
    );
    let first = h.get("/xrpc/app.bsky.feed.getTimeline").await;
    let second = h.get("/xrpc/app.bsky.feed.getTimeline").await;
    let mut pools = [first.served_by().to_owned(), second.served_by().to_owned()];
    pools.sort();
    assert_eq!(pools, ["ts-read-1", "ts-read-2"]);
    let main = h
        .get(&format!(
            "/xrpc/com.atproto.repo.getRecord?repo={ALICE}&collection=app.bsky.feed.post&rkey=1"
        ))
        .await;
    assert_eq!(main.served_by(), "ts-read-1");
    assert_eq!(
        main.body["path"],
        format!(
            "/xrpc/com.atproto.repo.getRecord?repo={ALICE}&collection=app.bsky.feed.post&rkey=1"
        )
    );
    let head = h
        .send(
            h.client
                .head(format!("{}/xrpc/com.atproto.server.describeServer", h.url)),
        )
        .await;
    assert_eq!(head.status, 200);
    assert_eq!(head.backend, "ts");
}

#[tokio::test]
async fn pinned_identities_read_from_rsky_by_did_handle_email_and_session() {
    let h = Harness::start().await;
    h.set_policy(&format!(
        "version = 1\n[reads]\npin_rsky = [\"{ALICE}\"]\n[writes]\n"
    ));
    let by_did = h
        .get(&format!(
            "/xrpc/com.atproto.sync.getLatestCommit?did={ALICE}"
        ))
        .await;
    assert_eq!(
        (by_did.served_by(), by_did.reason.as_str()),
        ("rsky", "pinned")
    );
    let by_handle = h
        .get("/xrpc/app.bsky.actor.getProfile?actor=alice.test")
        .await;
    assert_eq!(by_handle.served_by(), "rsky");
    let by_encoded_did = h
        .get("/xrpc/com.atproto.repo.describeRepo?repo=did%3Aplc%3Aalice")
        .await;
    assert_eq!(by_encoded_did.served_by(), "rsky");
    let by_session = h.get_as("/xrpc/com.atproto.server.getSession", ALICE).await;
    assert_eq!(by_session.served_by(), "rsky");
    let other = h
        .get(&format!("/xrpc/com.atproto.sync.getLatestCommit?did={BOB}"))
        .await;
    assert_eq!(other.served_by(), "ts-sync");
    let unknown_handle = h
        .get("/xrpc/app.bsky.actor.getProfile?actor=nobody.test")
        .await;
    assert_ne!(unknown_handle.served_by(), "rsky");
    let well_known = h
        .send(
            h.client
                .get(format!("{}/.well-known/atproto-did", h.url))
                .header("host", "alice.test"),
        )
        .await;
    assert_eq!(well_known.served_by(), "rsky");
    let custom = h
        .get("/custom-well-known-atproto-did?handle=alice.test")
        .await;
    assert_eq!(custom.served_by(), "rsky");
}

#[tokio::test]
async fn default_rsky_reads_honour_ts_holdbacks_and_the_kill_switch() {
    let h = Harness::start().await;
    h.set_policy(&format!(
        "version = 1\n[reads]\ndefault = \"rsky\"\npin_ts = [\"{BOB}\"]\n[writes]\n"
    ));
    let alice = h
        .get(&format!(
            "/xrpc/com.atproto.sync.getLatestCommit?did={ALICE}"
        ))
        .await;
    assert_eq!(alice.served_by(), "rsky");
    let anonymous = h.get("/xrpc/com.atproto.server.describeServer").await;
    assert_eq!(anonymous.served_by(), "rsky");
    let bob = h
        .get(&format!("/xrpc/com.atproto.sync.getLatestCommit?did={BOB}"))
        .await;
    assert_eq!(bob.served_by(), "ts-sync");
    h.set_policy(&format!(
        "version = 1\nkill_switch = true\n[reads]\ndefault = \"rsky\"\npin_rsky = [\"{ALICE}\"]\n[writes]\n"
    ));
    let alice = h
        .get(&format!(
            "/xrpc/com.atproto.sync.getLatestCommit?did={ALICE}"
        ))
        .await;
    assert_eq!(alice.served_by(), "ts-sync");
}

#[tokio::test]
async fn a_bad_policy_file_keeps_the_previous_policy() {
    let h = Harness::start().await;
    h.set_policy(&format!(
        "version = 1\n[reads]\npin_rsky = [\"{ALICE}\"]\n[writes]\n"
    ));
    h.set_policy("version = 1\n[reads]\ndefault = \"elsewhere\"\n");
    let alice = h
        .get(&format!(
            "/xrpc/com.atproto.sync.getLatestCommit?did={ALICE}"
        ))
        .await;
    assert_eq!(alice.served_by(), "rsky");
    h.set_policy("this is not toml");
    let alice = h
        .get(&format!(
            "/xrpc/com.atproto.sync.getLatestCommit?did={ALICE}"
        ))
        .await;
    assert_eq!(alice.served_by(), "rsky");
    h.set_policy(&canary_policy(""));
    h.set_allowlist(&format!(
        "version = 1\n[entries]\n\"{CANARY}\" = \"active\"\n"
    ));
    h.set_allowlist("version = 2\n");
    let canary = h
        .post_as(
            "/xrpc/com.atproto.repo.createRecord",
            CANARY,
            json!({ "repo": CANARY }),
        )
        .await;
    assert_eq!(canary.served_by(), "rsky");
}

#[tokio::test]
async fn an_upstream_that_closes_the_connection_does_not_fail_over() {
    let h = Harness::start_with(Some(Mock::broken("rsky").await)).await;
    h.set_policy(&format!(
        "version = 1\n[reads]\npin_rsky = [\"{ALICE}\"]\n[writes]\n"
    ));
    let read = h
        .get(&format!("/xrpc/com.atproto.sync.getRepo?did={ALICE}"))
        .await;
    assert_eq!((read.status, read.error()), (503, "UpstreamUnavailable"));
    assert_eq!(read.backend, "rsky");
    assert!(h.rsky.hits() >= 1);
    assert_eq!(h.ts_sync.hits(), 0);
}

#[tokio::test]
async fn a_truncated_mutation_body_is_refused_without_forwarding() {
    let h = Harness::start().await;
    let address = h.url.trim_start_matches("http://").to_owned();
    let mut stream = tokio::net::TcpStream::connect(&address).await.unwrap();
    let head = "POST /xrpc/com.atproto.repo.createRecord HTTP/1.1\r\nHost: router\r\nContent-Type: application/json\r\nContent-Length: 100\r\n\r\n{\"repo\":";
    tokio::io::AsyncWriteExt::write_all(&mut stream, head.as_bytes())
        .await
        .unwrap();
    tokio::io::AsyncWriteExt::shutdown(&mut stream)
        .await
        .unwrap();
    let mut response = Vec::new();
    tokio::io::AsyncReadExt::read_to_end(&mut stream, &mut response)
        .await
        .unwrap();
    let text = String::from_utf8_lossy(&response);
    assert!(text.starts_with("HTTP/1.1 400"), "{text}");
    assert!(text.contains("x-router-reason: body"), "{text}");
    assert_eq!(h.ts_main.hits(), 0);
    assert!(h.journal().is_empty());
}

/// With the account database gone, a read cannot be pinned and takes the
/// default backend; a mutation that needs the lookup is refused.
#[tokio::test]
async fn a_failing_account_database_pins_nothing_and_refuses_attributed_mutations() {
    let h = Harness::start().await;
    h.set_policy(&format!(
        "version = 1\n[reads]\npin_rsky = [\"{ALICE}\"]\n[writes]\n"
    ));
    rusqlite::Connection::open(h.dir.path().join("account.sqlite"))
        .unwrap()
        .execute_batch("DROP TABLE actor; DROP TABLE email_token;")
        .unwrap();
    let by_handle = h
        .get("/xrpc/app.bsky.actor.getProfile?actor=alice.test")
        .await;
    assert_ne!(by_handle.served_by(), "rsky");
    let by_did = h
        .get(&format!(
            "/xrpc/com.atproto.sync.getLatestCommit?did={ALICE}"
        ))
        .await;
    assert_eq!(by_did.served_by(), "rsky");
    let by_token = h
        .post(
            "/xrpc/com.atproto.server.resetPassword",
            json!({ "token": "alice-token", "password": "p" }),
        )
        .await;
    assert_eq!(
        (by_token.status, by_token.error()),
        (503, "RouterLookupUnavailable")
    );
}

#[tokio::test]
async fn a_journal_that_cannot_be_written_fails_mutations_closed() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Journal::over(&dir.path().join("j.jsonl"), FailAfter(1), 0);
    let h = Harness::start_full(None, Some(journal)).await;
    let before = rsky_pds_router::metrics::METRICS.journal_failures.get();
    // the start line is written; the end line fails and is counted
    let write = h
        .post(
            "/xrpc/com.atproto.server.createInviteCode",
            json!({ "useCount": 1 }),
        )
        .await;
    assert_eq!((write.status, write.served_by()), (200, "ts-main"));
    assert_eq!(
        rsky_pds_router::metrics::METRICS.journal_failures.get(),
        before + 1
    );
    // the next start line fails: refused before anything is forwarded
    let next = h
        .post(
            "/xrpc/com.atproto.server.createInviteCode",
            json!({ "useCount": 1 }),
        )
        .await;
    assert_eq!(next.reason, "journal");
    assert_eq!(
        rsky_pds_router::metrics::METRICS.journal_failures.get(),
        before + 2
    );
    let read = h.get("/xrpc/com.atproto.server.describeServer").await;
    assert_eq!(read.status, 200);
    assert_eq!(
        (next.status, next.error()),
        (503, "RouterJournalUnavailable")
    );
    assert_eq!(h.ts_main.hits(), 1);
}

#[tokio::test]
async fn hop_by_hop_headers_are_not_forwarded() {
    let h = Harness::start().await;
    let answer = h
        .send(
            h.client
                .get(format!("{}/xrpc/com.atproto.server.describeServer", h.url))
                .header("keep-alive", "timeout=5")
                .header("connection", "keep-alive"),
        )
        .await;
    assert_eq!(answer.status, 200);
    assert!(answer.body["connection"].is_null(), "{}", answer.body);
    assert_eq!(answer.served_by(), "ts-read-1", "{}", answer.body);
    assert_eq!(answer.body["host"], h.url.trim_start_matches("http://"));
}

#[tokio::test]
async fn a_request_without_a_host_header_gets_the_upstream_authority() {
    let h = Harness::start().await;
    let address = h.url.trim_start_matches("http://").to_owned();
    let mut stream = tokio::net::TcpStream::connect(&address).await.unwrap();
    tokio::io::AsyncWriteExt::write_all(
        &mut stream,
        b"GET /xrpc/com.atproto.server.describeServer HTTP/1.0\r\n\r\n",
    )
    .await
    .unwrap();
    let mut response = Vec::new();
    tokio::io::AsyncReadExt::read_to_end(&mut stream, &mut response)
        .await
        .unwrap();
    let text = String::from_utf8_lossy(&response);
    assert!(text.starts_with("HTTP/1.0 200"), "{text}");
    let expected = format!(
        "\"host\":\"{}\"",
        h.ts_read[0].url.trim_start_matches("http://")
    );
    assert!(text.contains(&expected), "{text}");
}

#[tokio::test]
async fn the_binary_serves_health_and_metrics() {
    let dir = tempfile::tempdir().unwrap();
    let policy = dir.path().join("policy.toml");
    let allowlist = dir.path().join("write-allowlist.toml");
    let account_db = dir.path().join("account.sqlite");
    std::fs::write(&policy, DEFAULT_POLICY).unwrap();
    std::fs::write(&allowlist, DEFAULT_ALLOWLIST).unwrap();
    rusqlite::Connection::open(&account_db)
        .unwrap()
        .execute_batch("CREATE TABLE actor (did TEXT PRIMARY KEY, handle TEXT);")
        .unwrap();
    let port = Mock::closed("port")
        .await
        .url
        .rsplit(':')
        .next()
        .unwrap()
        .to_owned();
    let metrics_port = Mock::closed("metrics")
        .await
        .url
        .rsplit(':')
        .next()
        .unwrap()
        .to_owned();
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_pds-router"))
        .env("ROUTER_PORT", &port)
        .env("ROUTER_METRICS_PORT", &metrics_port)
        .env("ROUTER_POLICY_FILE", &policy)
        .env("ROUTER_ALLOWLIST_FILE", &allowlist)
        .env("ROUTER_ACCOUNT_DB", &account_db)
        .env("RUST_LOG", "info")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let client = reqwest::Client::new();
    let mut health = None;
    for _ in 0..200 {
        if let Ok(response) = client
            .get(format!("http://127.0.0.1:{port}/xrpc/_health"))
            .send()
            .await
        {
            health = Some(response.status().as_u16());
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let metrics = client
        .get(format!("http://127.0.0.1:{metrics_port}/metrics"))
        .send()
        .await
        .map(|response| response.status().as_u16());
    std::process::Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    let mut exit = None;
    for _ in 0..200 {
        if let Some(status) = child.try_wait().unwrap() {
            exit = Some(status);
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let exit = exit.unwrap_or_else(|| {
        child.kill().unwrap();
        child.wait().unwrap()
    });
    assert!(exit.success(), "{exit}");
    assert_eq!(health, Some(200));
    assert_eq!(metrics.unwrap(), 200);
    assert!(dir.path().join(format!("mutations-{port}.jsonl")).exists());
}

#[tokio::test]
async fn reads_fail_over_to_the_ts_pool_only_on_connection_failure() {
    let h = Harness::start_with(Some(Mock::closed("rsky").await)).await;
    h.set_policy(&format!(
        "version = 1\n[reads]\npin_rsky = [\"{ALICE}\"]\n[writes]\n"
    ));
    let sync = h
        .get(&format!("/xrpc/com.atproto.sync.getRepo?did={ALICE}"))
        .await;
    assert_eq!((sync.status, sync.served_by()), (200, "ts-sync"));
    assert_eq!(
        (sync.backend.as_str(), sync.reason.as_str()),
        ("ts", "failover")
    );
    let bsky = h
        .get(&format!("/xrpc/app.bsky.actor.getProfile?actor={ALICE}"))
        .await;
    assert!(bsky.served_by().starts_with("ts-read-"));
    let main = h
        .get(&format!("/xrpc/com.atproto.repo.describeRepo?repo={ALICE}"))
        .await;
    assert_eq!(main.served_by(), "ts-read-1");
    // a mutation for a canary never fails over
    h.set_policy(&canary_policy(""));
    h.set_allowlist(&format!(
        "version = 1\n[entries]\n\"{CANARY}\" = \"active\"\n"
    ));
    let write = h
        .post_as(
            "/xrpc/com.atproto.repo.createRecord",
            CANARY,
            json!({ "repo": CANARY, "collection": "app.bsky.feed.post", "record": {} }),
        )
        .await;
    assert_eq!(write.status, 503);
    assert_eq!(write.error(), "UpstreamUnavailable");
    assert_eq!(write.retry_after, "1");
    assert_eq!(h.ts_main.hits(), 0);
    let lines = h.journal();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["backend"], "rsky");
    assert_eq!(lines[1]["status"], 503);
}

#[tokio::test]
async fn upstream_errors_are_answered_within_the_class_deadline() {
    let h = Harness::start().await;
    let slow = h.get("/xrpc/com.atproto.repo.slow").await;
    assert_eq!(slow.status, 504);
    assert_eq!(slow.error(), "UpstreamTimeout");
    let passed = h
        .send(
            h.client
                .get(format!("{}/xrpc/com.atproto.server.describeServer", h.url))
                .header("x-mock-status", "404"),
        )
        .await;
    assert_eq!(passed.status, 404);
    assert_eq!(passed.served_by(), "ts-read-1");
}

#[tokio::test]
async fn mutations_default_to_ts_main_with_raw_uploads_on_the_sync_worker() {
    let h = Harness::start().await;
    let create = h
        .post_as(
            "/xrpc/com.atproto.repo.createRecord",
            ALICE,
            json!({ "repo": "alice.test", "collection": "app.bsky.feed.post", "record": { "text": "hi" } }),
        )
        .await;
    assert_eq!((create.status, create.served_by()), (200, "ts-main"));
    assert_eq!(
        (create.backend.as_str(), create.reason.as_str()),
        ("ts", "target")
    );
    assert!(create.body["body"]
        .as_str()
        .unwrap()
        .contains("\"text\":\"hi\""));
    let upload = h
        .send(
            h.client
                .post(format!("{}/xrpc/com.atproto.repo.uploadBlob", h.url))
                .bearer_auth(jwt_for(ALICE))
                .header("content-type", "image/png")
                .body(vec![0u8; 4096]),
        )
        .await;
    assert_eq!((upload.status, upload.served_by()), (200, "ts-sync"));
    assert_eq!(upload.body["body"].as_str().unwrap().len(), 4096);
    let import = h
        .send(
            h.client
                .post(format!("{}/xrpc/com.atproto.repo.importRepo", h.url))
                .bearer_auth(jwt_for(ALICE))
                .header("content-type", "application/vnd.ipld.car")
                .body("car"),
        )
        .await;
    assert_eq!(import.served_by(), "ts-sync");
    let invite = h
        .post(
            "/xrpc/com.atproto.server.createInviteCode",
            json!({ "useCount": 1 }),
        )
        .await;
    assert_eq!(
        (invite.served_by(), invite.reason.as_str()),
        ("ts-main", "default")
    );
    let lines = h.journal();
    assert_eq!(lines.len(), 8);
    assert_eq!(lines[0]["nsid"], "com.atproto.repo.createRecord");
    assert_eq!(lines[0]["did"], ALICE);
    assert_eq!(lines[1]["id"], lines[0]["id"]);
    assert_eq!(lines[1]["phase"], "end");
    assert_eq!(lines[1]["status"], 200);
    assert_eq!(lines[2]["nsid"], "com.atproto.repo.uploadBlob");
    assert_eq!(lines[6]["nsid"], "com.atproto.server.createInviteCode");
    assert!(lines[6]["did"].is_null());
}

#[tokio::test]
async fn canary_mutations_go_to_rsky_only_while_active_and_never_to_ts() {
    let h = Harness::start().await;
    h.set_policy(&canary_policy(""));
    let record = json!({ "repo": CANARY, "collection": "app.bsky.feed.post", "record": {} });
    let absent = h
        .post_as(
            "/xrpc/com.atproto.repo.createRecord",
            CANARY,
            record.clone(),
        )
        .await;
    assert_eq!((absent.status, absent.error()), (503, "RouterNotAdmitted"));
    assert_eq!(absent.reason, "not-admitted");
    assert_eq!(absent.retry_after, "1");
    h.set_allowlist(&format!(
        "version = 1\n[entries]\n\"{CANARY}\" = \"draining\"\n"
    ));
    let draining = h
        .post_as(
            "/xrpc/com.atproto.repo.createRecord",
            CANARY,
            record.clone(),
        )
        .await;
    assert_eq!(draining.status, 503);
    h.set_allowlist(&format!(
        "version = 1\n[entries]\n\"{CANARY}\" = {{ state = \"maintenance\", workflow_id = \"w1\" }}\n"
    ));
    let maintenance = h
        .post_as(
            "/xrpc/com.atproto.repo.createRecord",
            CANARY,
            record.clone(),
        )
        .await;
    assert_eq!(maintenance.status, 503);
    h.set_allowlist(&format!(
        "version = 1\n[entries]\n\"{CANARY}\" = \"active\"\n"
    ));
    let active = h
        .post_as(
            "/xrpc/com.atproto.repo.createRecord",
            CANARY,
            record.clone(),
        )
        .await;
    assert_eq!((active.status, active.served_by()), (200, "rsky"));
    assert_eq!(
        (active.backend.as_str(), active.reason.as_str()),
        ("rsky", "canary")
    );
    // by handle, by session subject, by email, by token, by header
    let by_handle = h
        .post_as(
            "/xrpc/com.atproto.repo.putRecord",
            CANARY,
            json!({ "repo": "canary.test" }),
        )
        .await;
    assert_eq!(by_handle.served_by(), "rsky");
    let by_subject = h
        .post_as(
            "/xrpc/com.atproto.identity.updateHandle",
            CANARY,
            json!({ "handle": "x" }),
        )
        .await;
    assert_eq!(by_subject.served_by(), "rsky");
    let by_email = h
        .post(
            "/xrpc/com.atproto.server.requestPasswordReset",
            json!({ "email": "canary@example.com" }),
        )
        .await;
    assert_eq!(by_email.served_by(), "rsky");
    let by_token = h
        .post(
            "/xrpc/com.atproto.server.resetPassword",
            json!({ "token": "canary-token", "password": "p" }),
        )
        .await;
    assert_eq!(by_token.served_by(), "rsky");
    let by_header = h
        .send(
            h.client
                .post(format!("{}/xrpc/com.atproto.server.createSession", h.url))
                .header("x-atproto-did", CANARY)
                .json(&json!({ "identifier": "someone-else", "password": "p" })),
        )
        .await;
    assert_eq!(by_header.served_by(), "rsky");
    let by_identifier = h
        .post(
            "/xrpc/com.atproto.server.createSession",
            json!({ "identifier": "canary.test", "password": "p" }),
        )
        .await;
    assert_eq!(by_identifier.served_by(), "rsky");
    let by_subject_status = h
        .post(
            "/xrpc/com.atproto.admin.updateSubjectStatus",
            json!({ "subject": { "$type": "com.atproto.repo.strongRef", "uri": format!("at://{CANARY}/app.bsky.feed.post/1"), "cid": "b" } }),
        )
        .await;
    assert_eq!(by_subject_status.served_by(), "rsky");
    let by_account = h
        .post(
            "/xrpc/com.atproto.admin.updateAccountEmail",
            json!({ "account": "canary.test", "email": "n@example.com" }),
        )
        .await;
    assert_eq!(by_account.served_by(), "rsky");
    let by_recipient = h
        .post(
            "/xrpc/com.atproto.admin.sendEmail",
            json!({ "recipientDid": CANARY, "senderDid": ALICE, "content": "x" }),
        )
        .await;
    assert_eq!(by_recipient.served_by(), "rsky");
    let by_body_did = h
        .post(
            "/xrpc/com.atproto.admin.deleteAccount",
            json!({ "did": CANARY }),
        )
        .await;
    assert_eq!(by_body_did.served_by(), "rsky");
    let new_did = h
        .post(
            "/xrpc/com.atproto.server.createAccount",
            json!({ "did": CANARY, "handle": "canary.test" }),
        )
        .await;
    assert_eq!(new_did.served_by(), "rsky");
    let fresh_account = h
        .post(
            "/xrpc/com.atproto.server.createAccount",
            json!({ "handle": "new.test" }),
        )
        .await;
    assert_eq!(fresh_account.served_by(), "ts-main");
    // other accounts referencing the canary only in record content stay on TS
    let follow = h
        .post_as(
            "/xrpc/com.atproto.repo.createRecord",
            ALICE,
            json!({ "repo": ALICE, "collection": "app.bsky.graph.follow", "record": { "subject": CANARY } }),
        )
        .await;
    assert_eq!(follow.served_by(), "ts-main");
    assert_eq!(h.ts_main.hits(), 2);
    let lines = h.journal();
    assert!(lines
        .iter()
        .filter(|line| line["phase"] == "start")
        .all(|line| line["did"] != CANARY || line["backend"] == "rsky"));
    // the kill switch pauses the canary's writer without touching TS
    h.set_policy(&canary_policy("kill_switch = true\n"));
    let paused = h
        .post_as(
            "/xrpc/com.atproto.repo.createRecord",
            CANARY,
            record.clone(),
        )
        .await;
    assert_eq!((paused.status, paused.error()), (503, "RouterKillSwitch"));
    let others = h
        .post_as(
            "/xrpc/com.atproto.repo.createRecord",
            ALICE,
            json!({ "repo": ALICE }),
        )
        .await;
    assert_eq!(others.served_by(), "ts-main");
}

#[tokio::test]
async fn fenced_accounts_are_refused_at_every_mutation_shape() {
    let h = Harness::start().await;
    h.set_policy(&format!(
        "version = 1\n[reads]\n[writes]\ncanary_fence = [\"{CANARY}\"]\n"
    ));
    let shapes: Vec<(&str, Option<&str>, Value)> = vec![
        (
            "/xrpc/com.atproto.repo.createRecord",
            Some(CANARY),
            json!({ "repo": CANARY }),
        ),
        (
            "/xrpc/com.atproto.repo.applyWrites",
            Some(ALICE),
            json!({ "repo": CANARY, "writes": [] }),
        ),
        (
            "/xrpc/app.bsky.actor.putPreferences",
            Some(CANARY),
            json!({ "preferences": [] }),
        ),
        (
            "/xrpc/com.atproto.server.deleteAccount",
            None,
            json!({ "did": CANARY, "password": "p", "token": "t" }),
        ),
        (
            "/xrpc/com.atproto.server.requestAccountDelete",
            Some(CANARY),
            json!({}),
        ),
        (
            "/xrpc/com.atproto.admin.updateSubjectStatus",
            None,
            json!({ "subject": { "did": CANARY } }),
        ),
        (
            "/xrpc/com.atproto.temp.revokeAccountCredentials",
            None,
            json!({ "account": CANARY }),
        ),
        (
            "/@atproto/oauth-provider/~api/update-handle",
            None,
            json!({ "did": CANARY, "handle": "x" }),
        ),
        (
            "/@atproto/oauth-provider/~api/sign-out",
            None,
            json!({ "did": [ALICE, CANARY] }),
        ),
        (
            "/@atproto/oauth-provider/~api/reset-password-request",
            None,
            json!({ "email": "canary@example.com" }),
        ),
        (
            "/@atproto/oauth-provider/~api/reset-password-confirm",
            None,
            json!({ "token": "canary-token", "password": "p" }),
        ),
        (
            "/@atproto/oauth-provider/~api/sign-in",
            None,
            json!({ "username": "canary.test", "password": "p" }),
        ),
        (
            "/@atproto/oauth-provider/~api/sign-up",
            None,
            json!({ "did": CANARY }),
        ),
    ];
    for (path, did, body) in shapes {
        let answer = match did {
            Some(did) => h.post_as(path, did, body).await,
            None => h.post(path, body).await,
        };
        assert_eq!(
            (path, answer.status, answer.error()),
            (path, 503, "RouterFenced")
        );
        assert_eq!(answer.reason, "fenced");
    }
    assert_eq!(
        h.ts_main.hits() + h.ts_sync.hits() + h.rsky.hits() + h.oauth.hits(),
        0
    );
    assert_eq!(h.ts_read.iter().map(Mock::hits).sum::<usize>(), 0);
    assert!(h.journal().is_empty());
}

#[tokio::test]
async fn oauth_ui_api_mutations_reach_ts_for_others_and_503_for_the_canary() {
    let h = Harness::start().await;
    h.set_policy(&canary_policy(""));
    h.set_allowlist(&format!(
        "version = 1\n[entries]\n\"{CANARY}\" = \"active\"\n"
    ));
    let cookie_only = h
        .send(
            h.client
                .post(format!(
                    "{}/@atproto/oauth-provider/~api/deactivate-account",
                    h.url
                ))
                .header("cookie", "session=abc")
                .json(&json!({ "did": ALICE })),
        )
        .await;
    assert_eq!(
        (cookie_only.status, cookie_only.served_by()),
        (200, "ts-main")
    );
    let canary = h
        .post(
            "/@atproto/oauth-provider/~api/deactivate-account",
            json!({ "did": CANARY }),
        )
        .await;
    assert_eq!((canary.status, canary.error()), (503, "RouterNoEquivalent"));
    // session-only endpoints keep working for a canary on the reference UI
    let canary_sign_in = h
        .post(
            "/@atproto/oauth-provider/~api/sign-in",
            json!({ "username": "canary.test", "password": "pw" }),
        )
        .await;
    assert_eq!(
        (
            canary_sign_in.status,
            canary_sign_in.served_by(),
            canary_sign_in.reason.as_str()
        ),
        (200, "ts-main", "session-ui")
    );
    let canary_sign_out = h
        .post(
            "/@atproto/oauth-provider/~api/sign-out",
            json!({ "did": CANARY }),
        )
        .await;
    assert_eq!(
        (canary_sign_out.status, canary_sign_out.served_by()),
        (200, "ts-main")
    );
    let signout_all = h
        .post(
            "/@atproto/oauth-provider/~api/sign-out",
            json!({ "did": [ALICE, BOB] }),
        )
        .await;
    assert_eq!(signout_all.served_by(), "ts-main");
    let split = h
        .post(
            "/@atproto/oauth-provider/~api/sign-out",
            json!({ "did": [ALICE, CANARY] }),
        )
        .await;
    assert_eq!((split.status, split.error()), (503, "RouterSplitTargets"));
    let reversed = h
        .post(
            "/@atproto/oauth-provider/~api/sign-out",
            json!({ "did": [CANARY, ALICE] }),
        )
        .await;
    assert_eq!(
        (reversed.status, reversed.error()),
        (503, "RouterSplitTargets")
    );
    let no_did = h
        .post(
            "/@atproto/oauth-provider/~api/update-handle",
            json!({ "handle": "x" }),
        )
        .await;
    assert_eq!((no_did.status, no_did.error()), (400, "InvalidRequest"));
    let empty_list = h
        .post(
            "/@atproto/oauth-provider/~api/sign-out",
            json!({ "did": [] }),
        )
        .await;
    assert_eq!(empty_list.status, 400);
    let api_read = h.get("/@atproto/oauth-provider/~api/unknown-read").await;
    assert_eq!(api_read.served_by(), "ts-read-1");
    let lines = h.journal();
    assert_eq!(lines[0]["nsid"], "~api/deactivate-account");
    assert_eq!(h.rsky.hits(), 0);
}

#[tokio::test]
async fn authorization_server_traffic_passes_through() {
    let h = Harness::start().await;
    let token = h
        .send(
            h.client
                .post(format!("{}/oauth/token", h.url))
                .header("dpop", "proof")
                .body("grant_type=refresh_token"),
        )
        .await;
    assert_eq!((token.status, token.served_by()), (200, "oauth"));
    assert_eq!(
        (token.backend.as_str(), token.reason.as_str()),
        ("oauth", "authorization-server")
    );
    assert_eq!(token.body["body"], "grant_type=refresh_token");
    let metadata = h.get("/.well-known/oauth-authorization-server").await;
    assert_eq!(metadata.served_by(), "oauth");
    let consent = h
        .post("/@atproto/oauth-provider/~api/consent", json!({}))
        .await;
    assert_eq!(consent.served_by(), "oauth");
    let sessions = h.get("/@atproto/oauth-provider/~api/device-sessions").await;
    assert_eq!(sessions.served_by(), "oauth");
    assert!(h.journal().is_empty());
}

#[tokio::test]
async fn unknown_and_unattributable_mutations_are_refused() {
    let h = Harness::start().await;
    // a procedure the PDS does not implement itself is one it proxies to
    // another service, so the router forwards it to the default writer
    let proxied = h
        .post_as("/xrpc/community.blacksky.example.doThing", ALICE, json!({}))
        .await;
    assert_eq!(
        (proxied.status, proxied.served_by(), proxied.reason.as_str()),
        (200, "ts-main", "proxied")
    );
    let entry = h
        .journal()
        .into_iter()
        .rev()
        .find(|entry| entry["phase"] == "start")
        .expect("the proxied procedure is journaled");
    assert_eq!(entry["nsid"], "community.blacksky.example.doThing");
    assert_eq!(entry["backend"], "ts");
    let not_xrpc = h.post("/tls-check", json!({})).await;
    assert_eq!(not_xrpc.status, 503);
    let unknown_api = h
        .post("/@atproto/oauth-provider/~api/brand-new", json!({}))
        .await;
    assert_eq!(
        (unknown_api.status, unknown_api.error()),
        (503, "RouterUnknownMutation")
    );
    let missing_repo = h
        .post_as(
            "/xrpc/com.atproto.repo.createRecord",
            ALICE,
            json!({ "collection": "x" }),
        )
        .await;
    assert_eq!(
        (missing_repo.status, missing_repo.error()),
        (400, "InvalidRequest")
    );
    assert_eq!(missing_repo.reason, "malformed");
    let no_auth = h
        .post(
            "/xrpc/com.atproto.identity.updateHandle",
            json!({ "handle": "x" }),
        )
        .await;
    assert_eq!(no_auth.status, 400);
    let not_json = h
        .send(
            h.client
                .post(format!("{}/xrpc/com.atproto.repo.createRecord", h.url))
                .header("content-type", "text/plain")
                .body("repo=alice"),
        )
        .await;
    assert_eq!(not_json.status, 400);
    let bare_blob = json!({ "subject": { "$type": "com.atproto.admin.defs#repoBlobRef", "did": null, "cid": "bafy" } });
    let without_canaries = h
        .post(
            "/xrpc/com.atproto.admin.updateSubjectStatus",
            bare_blob.clone(),
        )
        .await;
    assert_eq!(without_canaries.served_by(), "ts-main");
    let no_subject = h
        .post(
            "/xrpc/com.atproto.admin.updateSubjectStatus",
            json!({ "takedown": {} }),
        )
        .await;
    assert_eq!(no_subject.status, 400);
    let bad_uri = h
        .post(
            "/xrpc/com.atproto.admin.updateSubjectStatus",
            json!({ "subject": { "uri": "https://x" } }),
        )
        .await;
    assert_eq!(bad_uri.status, 400);
    h.set_policy(&canary_policy(""));
    let with_canaries = h
        .post("/xrpc/com.atproto.admin.updateSubjectStatus", bare_blob)
        .await;
    assert_eq!(
        (with_canaries.status, with_canaries.error()),
        (503, "RouterUnattributable")
    );
    let unknown_token = h
        .post(
            "/xrpc/com.atproto.server.resetPassword",
            json!({ "token": "nope", "password": "p" }),
        )
        .await;
    assert_eq!(unknown_token.served_by(), "ts-main");
    let no_credentials = h
        .post(
            "/xrpc/com.atproto.server.resetPassword",
            json!({ "password": "p" }),
        )
        .await;
    assert_eq!(no_credentials.status, 400);
    let too_large = h
        .send(
            h.client
                .post(format!("{}/xrpc/com.atproto.repo.createRecord", h.url))
                .bearer_auth(jwt_for(ALICE))
                .header("content-type", "application/json")
                .body(format!(
                    "{{\"repo\":\"{ALICE}\",\"pad\":\"{}\"}}",
                    "x".repeat(JSON_BODY_LIMIT)
                )),
        )
        .await;
    assert_eq!(
        (too_large.status, too_large.error()),
        (413, "PayloadTooLarge")
    );
    // the proxied procedure and the two attributable mutations reached TS
    assert_eq!(h.ts_main.hits(), 3);
}

#[tokio::test]
async fn a_policy_change_during_a_ts_mutation_is_counted_as_a_misroute() {
    let h = Harness::start().await;
    let client = h.client.clone();
    let url = h.url.clone();
    let held = tokio::spawn(async move {
        client
            .post(format!("{url}/xrpc/com.atproto.repo.createRecord"))
            .bearer_auth(jwt_for(BOB))
            .header("x-mock-hold", "1")
            .json(&json!({ "repo": BOB }))
            .send()
            .await
            .unwrap()
    });
    while h.ts_main.hits() == 0 {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let before = misroutes();
    h.set_policy(&format!(
        "version = 1\n[reads]\n[writes]\ncanary_fence = [\"{BOB}\"]\n"
    ));
    h.ts_main.release.notify_one();
    let response = held.await.unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(misroutes(), before + 1);
    let fenced = h
        .post_as(
            "/xrpc/com.atproto.repo.createRecord",
            BOB,
            json!({ "repo": BOB }),
        )
        .await;
    assert_eq!(fenced.status, 503);
    assert_eq!(misroutes(), before + 1);
}

fn misroutes() -> u64 {
    rsky_pds_router::metrics::METRICS.misroutes.get()
}

#[tokio::test]
async fn metrics_render_every_family_after_traffic() {
    let h = Harness::start_with(Some(Mock::closed("rsky").await)).await;
    h.set_policy(&format!(
        "version = 1\n[reads]\npin_rsky = [\"{ALICE}\"]\n[writes]\n"
    ));
    h.get(&format!("/xrpc/app.bsky.actor.getProfile?actor={ALICE}"))
        .await;
    h.post("/@atproto/oauth-provider/~api/brand-new", json!({}))
        .await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, metrics_app()).await.unwrap();
    });
    let text = reqwest::get(format!("{url}/metrics"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    for family in [
        "router_requests_total",
        "router_request_duration_seconds",
        "router_writes_rejected_total",
        "router_journal_failures_total",
        "router_misroute_total",
        "router_failovers_total",
    ] {
        assert!(text.contains(family), "{family} missing from\n{text}");
    }
    assert!(text.contains("router_writes_rejected_total{reason=\"unknown\"}"));
}

/// A mutation whose account lookup fails is refused, never routed as if it
/// named no account: a fenced DID stays fenced while the lookup is down.
#[tokio::test]
async fn a_failed_account_lookup_refuses_attributed_mutations() {
    let h = Harness::start().await;
    h.set_policy("version = 1\n[reads]\n[writes]\ncanary_fence = [\"did:plc:canary\"]\n");
    let conn = rusqlite::Connection::open(h.dir.path().join("account.sqlite")).unwrap();
    conn.execute_batch("DROP TABLE actor; DROP TABLE email_token;")
        .unwrap();
    drop(conn);
    let before = rsky_pds_router::metrics::METRICS
        .writes_rejected
        .with_label_values(&["lookup"])
        .get();
    let create = h
        .post_as(
            "/xrpc/com.atproto.repo.createRecord",
            CANARY,
            json!({ "repo": "canary.test", "collection": "app.bsky.feed.post", "record": { "text": "hi" } }),
        )
        .await;
    assert_eq!(
        (create.status, create.error(), create.reason.as_str()),
        (503, "RouterLookupUnavailable", "lookup")
    );
    let reset = h
        .post(
            "/xrpc/com.atproto.server.resetPassword",
            json!({ "token": "CANARY-TOKEN", "password": "new-password" }),
        )
        .await;
    assert_eq!(
        (reset.status, reset.error()),
        (503, "RouterLookupUnavailable")
    );
    assert_eq!(h.ts_main.hits(), 0);
    assert_eq!(
        rsky_pds_router::metrics::METRICS
            .writes_rejected
            .with_label_values(&["lookup"])
            .get(),
        before + 2
    );
    // an unattributed mutation still needs no lookup
    let invite = h
        .post(
            "/xrpc/com.atproto.server.createInviteCode",
            json!({ "useCount": 1 }),
        )
        .await;
    assert_eq!((invite.status, invite.served_by()), (200, "ts-main"));
}

/// A mutation the upstream never answered may still complete there, so
/// its journal line says the outcome is unknown rather than closing it.
#[tokio::test]
async fn an_unanswered_mutation_is_journaled_as_ambiguous() {
    let h = Harness::start().await;
    let held = h
        .send(
            h.client
                .post(format!(
                    "{}/xrpc/com.atproto.server.createInviteCode",
                    h.url
                ))
                .header("x-mock-hold", "1")
                .json(&json!({ "useCount": 1 })),
        )
        .await;
    assert_eq!((held.status, held.error()), (504, "UpstreamTimeout"));
    let lines = h.journal();
    let last = lines.last().unwrap();
    assert_eq!(last["phase"], "ambiguous");
    assert_eq!(last["status"], 504);
    assert_eq!(lines[lines.len() - 2]["phase"], "start");
    assert_eq!(lines[lines.len() - 2]["id"], last["id"]);
    h.ts_main.release.notify_one();
    // an answered mutation closes its line as before
    let answered = h
        .post(
            "/xrpc/com.atproto.server.createInviteCode",
            json!({ "useCount": 1 }),
        )
        .await;
    assert_eq!(answered.status, 200);
    assert_eq!(h.journal().last().unwrap()["phase"], "end");
}
