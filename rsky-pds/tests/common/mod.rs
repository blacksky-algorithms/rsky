use http_auth_basic::Credentials;
pub mod oauth;

use rocket::http::{ContentType, Header};
use rocket::local::asynchronous::Client;
use rocket::serde::json::json;
use rsky_common::env::env_str;
use rsky_lexicon::com::atproto::server::CreateInviteCodeOutput;
use rsky_pds::config::{ServerConfig, ServiceDbConfig};
use rsky_pds::{build_rocket, RocketConfig};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Mutex, Once};
use tempfile::TempDir;

static INIT_ENV: Once = Once::new();

/// The `atproto` verification method the mock directory serves from
/// `/{did}/data`. Tests that exercise DID-document validation set it.
static PUBLISHED_SIGNING_KEY: Mutex<Option<String>> = Mutex::new(None);

#[allow(dead_code)] // only the signing-key test binary drives this
pub fn set_published_signing_key(signing_key: Option<String>) {
    *PUBLISHED_SIGNING_KEY.lock().unwrap() = signing_key;
}

/// Serves DID documents for any did requested, claiming the handle of the
/// account created by `create_account`. Keeps DID resolution hermetic.
fn start_mock_plc_directory() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock plc directory");
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
            let did = path
                .trim_start_matches('/')
                .replace("%3A", ":")
                .replace("%3a", ":");
            let domain = std::env::var("PDS_SERVICE_HANDLE_DOMAINS")
                .unwrap_or(".rsky.com".to_string())
                .split(',')
                .next()
                .unwrap()
                .to_string();
            // Every doc claims a PDS service endpoint pointing back at this
            // mock, which answers 200 to any request. This lets best-effort
            // notification fan-out resolve and deliver hermetically.
            // `/{did}/data` is the PLC document-data shape an account's own
            // DID document is validated against.
            let body = if let Some(did) = did.strip_suffix("/log/last") {
                // The last operation of any did, in the shape the PLC client
                // builds the next one from; a POST of that next one lands in
                // the branch below and is acknowledged.
                let hostname =
                    std::env::var("PDS_HOSTNAME").unwrap_or_else(|_| "localhost".to_string());
                let rotation_key = rsky_crypto::utils::encode_did_key(
                    &rsky_pds::apis::com::atproto::server::PDS_PLC_ROTATION_KEYPAIR.public_key(),
                );
                let signing_key = PUBLISHED_SIGNING_KEY
                    .lock()
                    .unwrap()
                    .clone()
                    .unwrap_or_else(|| rotation_key.clone());
                format!(
                    "{{\"type\":\"plc_operation\",\"rotationKeys\":[\"{rotation_key}\"],\
                     \"verificationMethods\":{{\"atproto\":\"{signing_key}\"}},\
                     \"alsoKnownAs\":[\"at://foo{domain}\"],\
                     \"services\":{{\"atproto_pds\":{{\"type\":\"AtprotoPersonalDataServer\",\
                     \"endpoint\":\"https://{hostname}\"}}}},\"prev\":null,\
                     \"sig\":\"c2ln\"}}"
                )
                .replace("{did}", did)
            } else if req.starts_with("POST ") {
                "{}".to_string()
            } else if let Some(did) = did.strip_suffix("/log/audit") {
                let hostname =
                    std::env::var("PDS_HOSTNAME").unwrap_or_else(|_| "localhost".to_string());
                format!(
                    "[{{\"did\":\"{did}\",\"cid\":\"op0\",\"nullified\":false,\
                     \"createdAt\":\"2026-01-01T00:00:00.000Z\",\"operation\":{{\
                     \"type\":\"plc_operation\",\"services\":{{\"atproto_pds\":{{\
                     \"type\":\"AtprotoPersonalDataServer\",\"endpoint\":\"https://{hostname}\"}}}}}}}}]"
                )
            } else if let Some(did) = did.strip_suffix("/data") {
                let hostname =
                    std::env::var("PDS_HOSTNAME").unwrap_or_else(|_| "localhost".to_string());
                let rotation_key = rsky_crypto::utils::encode_did_key(
                    &rsky_pds::apis::com::atproto::server::PDS_PLC_ROTATION_KEYPAIR.public_key(),
                );
                let signing_key = PUBLISHED_SIGNING_KEY
                    .lock()
                    .unwrap()
                    .clone()
                    .unwrap_or_default();
                format!(
                    "{{\"did\":\"{did}\",\"rotationKeys\":[\"{rotation_key}\"],\
                     \"verificationMethods\":{{\"atproto\":\"{signing_key}\"}},\
                     \"alsoKnownAs\":[\"at://foo{domain}\"],\
                     \"services\":{{\"atproto_pds\":{{\"type\":\"AtprotoPersonalDataServer\",\
                     \"endpoint\":\"https://{hostname}\"}}}}}}"
                )
            } else {
                // The published key, when set, is also the document's
                // `#atproto` verification method so service tokens the
                // account signs verify against it.
                let verification_methods = PUBLISHED_SIGNING_KEY
                    .lock()
                    .unwrap()
                    .as_deref()
                    .and_then(|key| key.strip_prefix("did:key:"))
                    .map(|multibase| {
                        format!(
                            "{{\"id\":\"{did}#atproto\",\"type\":\"Multikey\",\
                             \"controller\":\"{did}\",\"publicKeyMultibase\":\"{multibase}\"}}"
                        )
                    })
                    .unwrap_or_default();
                format!(
                    "{{\"id\":\"{did}\",\"alsoKnownAs\":[\"at://foo{domain}\"],\
                     \"verificationMethod\":[{verification_methods}],\
                     \"service\":[{{\"id\":\"#atproto_pds\",\"type\":\"AtprotoPersonalDataServer\",\
                     \"serviceEndpoint\":\"http://127.0.0.1:{port}\"}}]}}"
                )
            };
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });
    port
}

/// Provides the environment the server expects when the caller (e.g. a
/// local `cargo test` run) hasn't configured it. CI sets its own values.
fn init_env() {
    INIT_ENV.call_once(|| {
        let defaults = [
            ("PDS_HOSTNAME", "rsky.com"),
            ("PDS_SERVICE_DID", "did:web:localho.st"),
            ("PDS_SERVICE_HANDLE_DOMAINS", ".rsky.com"),
            ("PDS_ADMIN_PASS", "3ed1c7b568d3328c44430add531a099f"),
            // no mail transport: messages are logged, never sent
            ("PDS_MAILGUN_API_KEY", ""),
            // pin the proxy targets so a developer's .env cannot leak real
            // services into the tests; localhost is rejected by is_safe_url
            ("PDS_MOD_SERVICE_URL", "http://localhost:1"),
            ("PDS_MOD_SERVICE_DID", "did:web:mod.invalid"),
            ("PDS_REPORT_SERVICE_URL", "http://localhost:1"),
            ("PDS_REPORT_SERVICE_DID", "did:web:report.invalid"),
            (
                "PDS_JWT_KEY_K256_PRIVATE_KEY_HEX",
                "9d5907143471e8f0e8df0f8b9512a8c5377878ee767f18fcf961055ecfc071cd",
            ),
            (
                "PDS_PLC_ROTATION_KEY_K256_PRIVATE_KEY_HEX",
                "fb478b39dd2ddf84bef135dd60f90381903eefadbb9df4b18a2b9b174ae72582",
            ),
            (
                "PDS_REPO_SIGNING_KEY_K256_PRIVATE_KEY_HEX",
                "71cfcf4882a6cff494c3d0affadd3858eb3a5838e7b5e15170e696a590a4fa01",
            ),
        ];
        for (key, value) in defaults {
            if std::env::var(key).is_err() {
                std::env::set_var(key, value);
            }
        }
        // tests never send mail, whatever the caller's environment (CI carries
        // real Mailgun secrets): messages are logged, never sent
        for key in [
            "PDS_MAILGUN_API_KEY",
            "PDS_MAILGUN_DOMAIN",
            "PDS_EMAIL_SMTP_URL",
        ] {
            std::env::set_var(key, "");
        }
        if std::env::var("PDS_BLOBSTORE_DISK_LOCATION").is_err()
            && std::env::var("PDS_BLOBSTORE_S3_BUCKET").is_err()
        {
            let blob_dir =
                std::env::temp_dir().join(format!("rsky-pds-test-blobs-{}", std::process::id()));
            std::fs::create_dir_all(&blob_dir).expect("create test blobstore dir");
            std::env::set_var("PDS_BLOBSTORE_DISK_LOCATION", &blob_dir);
        }
        if std::env::var("PDS_DID_PLC_URL").is_err() {
            let port = start_mock_plc_directory();
            std::env::set_var("PDS_DID_PLC_URL", format!("http://127.0.0.1:{port}"));
        }
        if std::env::var("PDS_BSKY_APP_VIEW_URL").is_err() {
            let port = start_mock_plc_directory();
            std::env::set_var("PDS_BSKY_APP_VIEW_URL", format!("http://127.0.0.1:{port}"));
            std::env::set_var("PDS_BSKY_APP_VIEW_DID", "did:web:appview.invalid");
        }
    });
}

/**
    Fetch PDS_ADMIN_PASS to be used for creating initial accounts
*/
pub fn get_admin_token() -> String {
    let credentials = Credentials::new("admin", env_str("PDS_ADMIN_PASS").unwrap().as_str());
    credentials.as_http_header()
}

/**
    Start a client for the rsky-pds rocket instance backed by sqlite
    databases under a fresh temporary directory
*/
#[allow(dead_code)] // the compatibility binary boots over the fixture instead
pub async fn get_client() -> (TempDir, Client) {
    let dir = tempfile::tempdir().expect("Valid temporary directory");
    let client = get_client_in(dir.path()).await;
    (dir, client)
}

/// Boots a PDS over an existing data directory, as a restart would.
#[allow(dead_code)]
pub async fn get_client_in(dir: &std::path::Path) -> Client {
    init_env();
    let path = |name: &str| dir.join(name).to_str().unwrap().to_owned();
    let rocket_cfg = RocketConfig {
        service_db: Some(ServiceDbConfig {
            account_db_location: path("account.sqlite"),
            sequencer_db_location: path("sequencer.sqlite"),
            did_cache_db_location: path("did_cache.sqlite"),
            lifecycle_db_location: path("rsky/lifecycle.sqlite"),
            lock_dir: path("rsky/locks"),
            blob_attempts_db_location: path("rsky/blob-attempts.sqlite"),
            blob_generations_db_location: path("rsky/blob-generations.sqlite"),
            repair_db_location: path("rsky/repair.sqlite"),
        }),
        actor_store_directory: Some(path("actors")),
    };
    Client::untracked(build_rocket(Some(rocket_cfg)).await)
        .await
        .expect("Valid Rocket instance")
}

/**
    Creates a mock account for testing purposes
*/
#[allow(dead_code)] // not every test binary drives the account flow
pub async fn create_account(client: &Client) -> (String, String) {
    let domain = client
        .rocket()
        .state::<ServerConfig>()
        .unwrap()
        .identity
        .service_handle_domains
        .first()
        .unwrap();
    let input = json!({
        "useCount": 1
    });

    let response = client
        .post("/xrpc/com.atproto.server.createInviteCode")
        .header(ContentType::JSON)
        .header(Header::new("Authorization", get_admin_token()))
        .body(input.to_string())
        .dispatch()
        .await;
    let invite_code = response
        .into_json::<CreateInviteCodeOutput>()
        .await
        .unwrap()
        .code;

    let account_input = json!({
        "did": "did:plc:khvyd3oiw46vif5gm7hijslk",
        "email": "foo@example.com",
        "handle": format!("foo{domain}"),
        "password": "password",
        "inviteCode": invite_code
    });

    client
        .post("/xrpc/com.atproto.server.createAccount")
        .header(ContentType::JSON)
        .header(Header::new("Authorization", get_admin_token()))
        .body(account_input.to_string())
        .dispatch()
        .await;

    ("foo@example.com".to_string(), "password".to_string())
}

/// A private copy of the reference-PDS fixture under `tests/fixtures`,
/// together with its manifest. `PDS_COMPAT_DATA_DIR` points at a freshly
/// built fixture instead of the checked-in one.
#[allow(dead_code)] // only the compatibility test binary drives this
pub struct Fixture {
    pub dir: std::path::PathBuf,
    pub source: std::path::PathBuf,
    pub manifest: serde_json::Value,
}

#[allow(dead_code)]
impl Fixture {
    pub fn account(&self, name: &str) -> &serde_json::Value {
        &self.manifest["accounts"][name]
    }

    pub fn did(&self, name: &str) -> String {
        self.account(name)["did"].as_str().unwrap().to_owned()
    }

    pub fn token(&self, name: &str) -> String {
        self.manifest["tokens"][name].as_str().unwrap().to_owned()
    }

    pub fn secret(&self, name: &str) -> String {
        self.manifest["secrets"][name].as_str().unwrap().to_owned()
    }

    /// An OAuth session the reference PDS issued, as recorded by the builder.
    pub fn oauth(&self, name: &str) -> serde_json::Value {
        serde_json::from_slice(
            &std::fs::read(self.source.join("oauth").join(format!("{name}.json"))).unwrap(),
        )
        .unwrap()
    }

    pub fn expected(&self, name: &str) -> Vec<u8> {
        std::fs::read(self.source.join("expected").join(name)).expect("expected fixture output")
    }

    pub fn expected_json(&self, name: &str) -> serde_json::Value {
        serde_json::from_slice(&self.expected(name)).expect("expected fixture json")
    }

    pub fn expected_status(&self, name: &str) -> u16 {
        self.manifest["expected_status"][name]
            .as_str()
            .unwrap()
            .parse()
            .unwrap()
    }

    pub fn did_key(&self, name: &str) -> String {
        let doc: serde_json::Value = serde_json::from_slice(
            &std::fs::read(
                self.source
                    .join("plc")
                    .join(format!("{}.json", self.did(name))),
            )
            .unwrap(),
        )
        .unwrap();
        format!(
            "did:key:{}",
            doc["verificationMethod"][0]["publicKeyMultibase"]
                .as_str()
                .unwrap()
        )
    }

    pub fn data(&self, relative: &str) -> std::path::PathBuf {
        self.dir.join("data").join(relative)
    }

    /// `(seq, eventType, event)` rows of the reference sequencer for `did`.
    pub fn reference_events(&self, did: &str) -> Vec<(i64, String, Vec<u8>)> {
        sequencer_events(&self.source.join("data").join("sequencer.sqlite"), did)
    }

    pub fn actor_store(&self, name: &str) -> std::path::PathBuf {
        let did = self.did(name);
        let hash = hex::encode(<sha2::Sha256 as sha2::Digest>::digest(did.as_bytes()));
        self.data("actors")
            .join(&hash[..2])
            .join(did)
            .join("store.sqlite")
    }
}

#[allow(dead_code)]
static FIXTURE: std::sync::OnceLock<(TempDir, Fixture)> = std::sync::OnceLock::new();

#[allow(dead_code)]
fn fixture_source() -> std::path::PathBuf {
    match std::env::var("PDS_COMPAT_DATA_DIR") {
        Ok(dir) => std::path::PathBuf::from(dir),
        Err(_) => std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("ts-pds-0.5.27"),
    }
}

/// OAuth session lifetimes are measured from the row timestamps, so a copy
/// of the fixture starts every recorded session as if it were created now.
#[allow(dead_code)]
fn refresh_oauth_session_times(data: &std::path::Path) {
    let conn = rusqlite::Connection::open(data.join("account.sqlite")).unwrap();
    conn.execute(
        "UPDATE token SET \"createdAt\" = ?1, \"updatedAt\" = ?1",
        [rsky_common::now()],
    )
    .unwrap();
}

#[allow(dead_code)]
fn copy_dir(src: &std::path::Path, dst: &std::path::Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let target = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// Serves the DID documents the fixture was built against, so identity
/// lookups resolve to exactly what the reference PDS saw.
#[allow(dead_code)]
fn start_fixture_plc_directory(plc_dir: std::path::PathBuf) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture plc directory");
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let path = req
                .split_whitespace()
                .nth(1)
                .unwrap_or("/")
                .replace("%3A", ":")
                .replace("%3a", ":");
            let mut parts = path.trim_start_matches('/').splitn(2, '/');
            let did = parts.next().unwrap_or_default().to_owned();
            let file = match parts.next() {
                None => format!("{did}.json"),
                Some("data") => format!("{did}.data.json"),
                Some("log/audit") => format!("{did}.audit.json"),
                Some(_) => String::new(),
            };
            let (status, body) = match std::fs::read(plc_dir.join(&file)) {
                Ok(body) if !file.is_empty() => ("200 OK", body),
                _ => (
                    "404 Not Found",
                    b"{\"message\":\"DID not registered\"}".to_vec(),
                ),
            };
            let header = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(header.as_bytes());
            let _ = stream.write_all(&body);
        }
    });
    port
}

#[allow(dead_code)]
fn fixture() -> &'static Fixture {
    &FIXTURE
        .get_or_init(|| {
            let source = fixture_source();
            let manifest: serde_json::Value =
                serde_json::from_slice(&std::fs::read(source.join("manifest.json")).unwrap())
                    .unwrap();
            let tmp = tempfile::tempdir().expect("Valid temporary directory");
            copy_dir(&source.join("data"), &tmp.path().join("data"));
            refresh_oauth_session_times(&tmp.path().join("data"));
            let secrets = manifest["secrets"].as_object().unwrap();
            for (key, value) in secrets {
                std::env::set_var(key, value.as_str().unwrap());
            }
            std::env::set_var(
                "PDS_ADMIN_PASS",
                secrets["PDS_ADMIN_PASSWORD"].as_str().unwrap(),
            );
            std::env::set_var("PDS_INVITE_REQUIRED", "false");
            // the fixture is a shared data directory; nothing leaves blob storage
            std::env::set_var("PDS_COEXISTENCE", "true");
            std::env::set_var(
                "PDS_BLOBSTORE_DISK_LOCATION",
                tmp.path().join("data").join("blocks"),
            );
            let port = start_fixture_plc_directory(source.join("plc"));
            std::env::set_var("PDS_DID_PLC_URL", format!("http://127.0.0.1:{port}"));
            let dir = tmp.path().to_path_buf();
            (
                tmp,
                Fixture {
                    dir,
                    source,
                    manifest,
                },
            )
        })
        .1
}

/// Start a client over a private copy of the reference-PDS fixture. The
/// copy is shared by every test in the process, so tests must not mutate
/// the accounts the fixture ships with.
#[allow(dead_code)]
pub async fn get_client_with_fixture() -> (&'static Fixture, Client) {
    let fixture = fixture();
    init_env();
    let path = |name: &str| fixture.data(name).to_str().unwrap().to_owned();
    let rocket_cfg = RocketConfig {
        service_db: Some(ServiceDbConfig {
            account_db_location: path("account.sqlite"),
            sequencer_db_location: path("sequencer.sqlite"),
            did_cache_db_location: path("did_cache.sqlite"),
            lifecycle_db_location: path("rsky/lifecycle.sqlite"),
            lock_dir: path("rsky/locks"),
            blob_attempts_db_location: path("rsky/blob-attempts.sqlite"),
            blob_generations_db_location: path("rsky/blob-generations.sqlite"),
            repair_db_location: path("rsky/repair.sqlite"),
        }),
        actor_store_directory: Some(path("actors")),
    };
    let client = Client::untracked(build_rocket(Some(rocket_cfg)).await)
        .await
        .expect("Valid Rocket instance");
    (fixture, client)
}

/// Start a client over a fresh, private copy of the fixture's data
/// directory, for tests that change session or account rows.
#[allow(dead_code)]
pub async fn get_client_with_fixture_copy() -> (&'static Fixture, TempDir, Client) {
    let fixture = fixture();
    init_env();
    let dir = tempfile::tempdir().expect("Valid temporary directory");
    copy_dir(&fixture.source.join("data"), &dir.path().join("data"));
    refresh_oauth_session_times(&dir.path().join("data"));
    let path = |name: &str| {
        dir.path()
            .join("data")
            .join(name)
            .to_str()
            .unwrap()
            .to_owned()
    };
    let rocket_cfg = RocketConfig {
        service_db: Some(ServiceDbConfig {
            account_db_location: path("account.sqlite"),
            sequencer_db_location: path("sequencer.sqlite"),
            did_cache_db_location: path("did_cache.sqlite"),
            lifecycle_db_location: path("rsky/lifecycle.sqlite"),
            lock_dir: path("rsky/locks"),
            blob_attempts_db_location: path("rsky/blob-attempts.sqlite"),
            blob_generations_db_location: path("rsky/blob-generations.sqlite"),
            repair_db_location: path("rsky/repair.sqlite"),
        }),
        actor_store_directory: Some(path("actors")),
    };
    let client = Client::untracked(build_rocket(Some(rocket_cfg)).await)
        .await
        .expect("Valid Rocket instance");
    (fixture, dir, client)
}

/// `(seq, eventType, event)` rows of a sequencer database for `did`.
#[allow(dead_code)]
pub fn sequencer_events(path: &std::path::Path, did: &str) -> Vec<(i64, String, Vec<u8>)> {
    let conn =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let mut stmt = conn
        .prepare("SELECT seq, \"eventType\", event FROM repo_seq WHERE did = ?1 ORDER BY seq")
        .unwrap();
    stmt.query_map([did], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

/// The `rsky-pds` binary configured over `dir`, with the environment of
/// this process left out.
#[allow(dead_code)]
pub fn pds_binary(dir: &std::path::Path) -> std::process::Command {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_rsky-pds"));
    let path = |name: &str| dir.join(name);
    command
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", dir)
        .env("PDS_HOSTNAME", "rsky.com")
        .env("PDS_SERVICE_DID", "did:web:localho.st")
        .env("PDS_ADMIN_PASS", "3ed1c7b568d3328c44430add531a099f")
        .env(
            "PDS_JWT_KEY_K256_PRIVATE_KEY_HEX",
            "9d5907143471e8f0e8df0f8b9512a8c5377878ee767f18fcf961055ecfc071cd",
        )
        .env(
            "PDS_PLC_ROTATION_KEY_K256_PRIVATE_KEY_HEX",
            "6a5e8b2ffbf2d9c9e7c5a2c62f0f19c4ac0a9da1a2a3e9d2f0a4a4a6a5b4c3d2",
        )
        .env(
            "PDS_REPO_SIGNING_KEY_K256_PRIVATE_KEY_HEX",
            "71cfcf4882a6cff494c3d0affadd3858eb3a5838e7b5e15170e696a590a4fa01",
        )
        .env("PDS_BLOBSTORE_DISK_LOCATION", path("blobs"))
        .env("PDS_ACTOR_STORE_DIRECTORY", path("actors"))
        .env("PDS_ACCOUNT_DB_LOCATION", path("account.sqlite"))
        .env("PDS_SEQUENCER_DB_LOCATION", path("sequencer.sqlite"))
        .env("PDS_DID_CACHE_DB_LOCATION", path("did_cache.sqlite"))
        .env("PDS_LIFECYCLE_DB", path("rsky/lifecycle.sqlite"))
        .env("PDS_LOCK_DIR", path("rsky/locks"))
        .env("PDS_BLOB_ATTEMPTS_DB", path("rsky/blob-attempts.sqlite"))
        .env(
            "PDS_BLOB_GENERATIONS_DB",
            path("rsky/blob-generations.sqlite"),
        )
        .env("PDS_REPAIR_DB", path("rsky/repair.sqlite"));
    if let Some(profile) = std::env::var_os("LLVM_PROFILE_FILE") {
        command.env("LLVM_PROFILE_FILE", profile);
    }
    command
}
