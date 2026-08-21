//! Layer 2 acceptance gate: the same record script fired at two real servers
//! over XRPC, then the Layer 1 comparator pointed at the two store files.
//!
//! Run it through `rsky-spaces-parity/layer2/run.sh`, which builds both binaries
//! and passes their paths in.

use anyhow::{bail, Context, Result};
use rsky_spaces_parity::layer2::normalize::{self, Revs};
use rsky_spaces_parity::layer2::process::{copy_tree, free_port, reset_stores, Server};
use rsky_spaces_parity::layer2::{car, directory::Directory, tokens, Scoreboard, Verdict};
use rsky_spaces_parity::{compare_tables, dump_tables, revs_are_well_formed};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

const AUTHOR_DID: &str = "did:plc:layer2writeraaaaaaaaaaa";
const HANDLE_DOMAIN: &str = ".layer2.test";
const HANDLE: &str = "writer.layer2.test";
const PASSWORD: &str = "layer2-local-password";
const ADMIN_PASS: &str = "layer2-local-admin";
const SPACE_TYPE: &str = "community.blacksky.feed";
const SPACE_SKEY: &str = "main";
const COLLECTION: &str = "com.example.post";
const OTHER_COLLECTION: &str = "com.example.note";
const HS256_SECRET: &str = "layer2-local-authorization-server-secret";
const OAUTH_ISSUER: &str = "http://localhost:0/oauth";
const PDS_SERVICE_DID: &str = "did:web:localho.st";
const DAEMON_DID: &str = "did:plc:layer2daemonaaaaaaaaaaa";

/// Fixed local key material. None of it protects anything: the whole stack is
/// created and destroyed inside one run directory.
const AUTHORITY_SPACE_KEY: &str =
    "1111111111111111111111111111111111111111111111111111111111111111";
const DAEMON_KEY: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const PDS_JWT_KEY: &str = "9d5907143471e8f0e8df0f8b9512a8c5377878ee767f18fcf961055ecfc071cd";
const PDS_ROTATION_KEY: &str = "fb478b39dd2ddf84bef135dd60f90381903eefadbb9df4b18a2b9b174ae72582";
const PDS_SIGNING_KEY: &str = "71cfcf4882a6cff494c3d0affadd3858eb3a5838e7b5e15170e696a590a4fa01";

struct Gate {
    client: reqwest::Client,
    pds_url: String,
    shim_url: String,
    space: String,
    session: String,
    pds_credential: String,
    shim_credential: String,
}

/// One step of the write script, applied identically to both servers.
enum Step {
    Create {
        collection: &'static str,
        rkey: &'static str,
        record: Value,
    },
    Delete {
        collection: &'static str,
        rkey: &'static str,
    },
}

impl Step {
    fn nsid(&self) -> &'static str {
        match self {
            Step::Create { .. } => "com.atproto.space.createRecord",
            Step::Delete { .. } => "com.atproto.space.deleteRecord",
        }
    }

    fn label(&self) -> String {
        match self {
            Step::Create {
                collection, rkey, ..
            } => format!("createRecord {collection}/{rkey}"),
            Step::Delete { collection, rkey } => format!("deleteRecord {collection}/{rkey}"),
        }
    }

    fn body(&self, space: &str, repo: &str) -> Value {
        match self {
            Step::Create {
                collection,
                rkey,
                record,
            } => json!({
                "space": space,
                "repo": repo,
                "collection": collection,
                "rkey": rkey,
                "record": record,
            }),
            Step::Delete { collection, rkey } => json!({
                "space": space,
                "repo": repo,
                "collection": collection,
                "rkey": rkey,
            }),
        }
    }
}

fn script() -> Vec<Step> {
    vec![
        Step::Create {
            collection: COLLECTION,
            rkey: "3kaaaaaaaaaa1",
            record: json!({"text": "first", "n": 1}),
        },
        Step::Create {
            collection: COLLECTION,
            rkey: "3kaaaaaaaaaa2",
            record: json!({"text": "second", "n": 2}),
        },
        Step::Create {
            collection: OTHER_COLLECTION,
            rkey: "3kaaaaaaaaaa3",
            record: json!({"text": "other collection", "nested": {"a": [1, 2, 3], "b": true}}),
        },
        Step::Create {
            collection: COLLECTION,
            rkey: "unicode.rkey_1~",
            record: json!({"text": "\u{e9}\u{4e16}\u{754c}\u{1f600}", "empty": ""}),
        },
        // A duplicate rkey: both sides must refuse it the same way.
        Step::Create {
            collection: COLLECTION,
            rkey: "3kaaaaaaaaaa1",
            record: json!({"text": "duplicate"}),
        },
        Step::Delete {
            collection: COLLECTION,
            rkey: "3kaaaaaaaaaa1",
        },
        // Delete then recreate the same key.
        Step::Create {
            collection: COLLECTION,
            rkey: "3kaaaaaaaaaa1",
            record: json!({"text": "recreated"}),
        },
        // A delete of something absent: both sides must refuse it the same way.
        Step::Delete {
            collection: COLLECTION,
            rkey: "3kmissingaaaa",
        },
        Step::Delete {
            collection: OTHER_COLLECTION,
            rkey: "3kaaaaaaaaaa3",
        },
        Step::Create {
            collection: OTHER_COLLECTION,
            rkey: "3kaaaaaaaaaa4",
            record: json!({"text": "after the delete"}),
        },
    ]
}

impl Gate {
    async fn post(
        &self,
        base: &str,
        nsid: &str,
        headers: Vec<(&str, String)>,
        body: &Value,
    ) -> Result<(u16, Value)> {
        let url = format!("{base}/xrpc/{nsid}");
        let mut request = self.client.post(&url).json(body);
        for (name, value) in headers {
            request = request.header(name, value);
        }
        let response = request
            .send()
            .await
            .with_context(|| format!("POST {url}"))?;
        let status = response.status().as_u16();
        let text = response.text().await.unwrap_or_default();
        Ok((
            status,
            serde_json::from_str(&text).unwrap_or(Value::String(text)),
        ))
    }

    async fn get(
        &self,
        base: &str,
        nsid: &str,
        query: &str,
        headers: Vec<(&str, String)>,
    ) -> Result<(u16, Value)> {
        let url = format!("{base}/xrpc/{nsid}?{query}");
        let mut request = self.client.get(&url);
        for (name, value) in headers {
            request = request.header(name, value);
        }
        let response = request.send().await.with_context(|| format!("GET {url}"))?;
        let status = response.status().as_u16();
        let text = response.text().await.unwrap_or_default();
        Ok((
            status,
            serde_json::from_str(&text).unwrap_or(Value::String(text)),
        ))
    }

    async fn get_bytes(
        &self,
        base: &str,
        nsid: &str,
        query: &str,
        headers: Vec<(&str, String)>,
    ) -> Result<(u16, Vec<u8>)> {
        let url = format!("{base}/xrpc/{nsid}?{query}");
        let mut request = self.client.get(&url);
        for (name, value) in headers {
            request = request.header(name, value);
        }
        let response = request.send().await.with_context(|| format!("GET {url}"))?;
        let status = response.status().as_u16();
        Ok((status, response.bytes().await?.to_vec()))
    }

    /// Write auth: the PDS takes its own session token, the space host takes a
    /// DPoP-bound access token from the authorization server it trusts.
    fn pds_write_headers(&self) -> Vec<(&'static str, String)> {
        vec![("authorization", format!("Bearer {}", self.session))]
    }

    fn shim_write_headers(&self, nsid: &str) -> Vec<(&'static str, String)> {
        let token = tokens::access_token(HS256_SECRET, OAUTH_ISSUER, PDS_SERVICE_DID, AUTHOR_DID);
        let proof = tokens::dpop_proof(
            "POST",
            &format!("{}/xrpc/{nsid}", self.shim_url),
            Some(&token),
        );
        vec![("authorization", format!("DPoP {token}")), ("dpop", proof)]
    }

    /// Read auth: a space credential presented under DPoP on both sides.
    fn pds_read_headers(&self, nsid: &str) -> Vec<(&'static str, String)> {
        let proof = tokens::dpop_proof(
            "GET",
            &format!("{}/xrpc/{nsid}", self.pds_url),
            Some(&self.pds_credential),
        );
        vec![
            ("authorization", format!("DPoP {}", self.pds_credential)),
            ("dpop", proof),
        ]
    }

    fn shim_read_headers(&self, nsid: &str) -> Vec<(&'static str, String)> {
        let proof = tokens::dpop_proof(
            "GET",
            &format!("{}/xrpc/{nsid}", self.shim_url),
            Some(&self.shim_credential),
        );
        vec![
            ("authorization", format!("DPoP {}", self.shim_credential)),
            ("dpop", proof),
        ]
    }
}

fn env_path(key: &str, fallback: &str) -> PathBuf {
    PathBuf::from(std::env::var(key).unwrap_or_else(|_| fallback.to_string()))
}

fn multibase_of(hex_key: &str) -> Result<String> {
    let signer = rsky_space_host::signing::Signer::from_hex(hex_key)
        .map_err(|error| anyhow::anyhow!("signer: {error}"))?;
    Ok(signer
        .did_key()
        .strip_prefix("did:key:")
        .unwrap_or(signer.did_key())
        .to_string())
}

#[tokio::main]
async fn main() -> Result<()> {
    let run_dir = env_path("LAYER2_RUN_DIR", "target/layer2/run");
    let pds_bin = env_path("LAYER2_PDS_BIN", "");
    let shim_bin = env_path("LAYER2_SHIM_BIN", "target/debug/rsky-space-host");
    if !pds_bin.is_file() {
        bail!("LAYER2_PDS_BIN must point at the pinned oracle binary (got {pds_bin:?})");
    }
    if !shim_bin.is_file() {
        bail!("LAYER2_SHIM_BIN must point at the space-host binary (got {shim_bin:?})");
    }

    if run_dir.exists() {
        std::fs::remove_dir_all(&run_dir).context("clear run directory")?;
    }
    std::fs::create_dir_all(&run_dir)?;
    let run_dir = run_dir.canonicalize()?;
    let pds_dir = run_dir.join("pds");
    let shim_dir = run_dir.join("shim");
    let pds_actors = pds_dir.join("actors");
    let shim_actors = shim_dir.join("actors");
    for dir in [&pds_dir, &shim_dir, &pds_dir.join("blobs")] {
        std::fs::create_dir_all(dir)?;
    }

    let mut keys = BTreeMap::new();
    keys.insert(DAEMON_DID.to_string(), multibase_of(DAEMON_KEY)?);
    let directory = Directory::start(keys, HANDLE.to_string())?;

    let pds_port = free_port()?;
    let shim_port = free_port()?;
    let pds_url = format!("http://localhost:{pds_port}");
    let shim_url = format!("http://127.0.0.1:{shim_port}");

    let pds_env: Vec<(String, String)> = vec![
        ("ROCKET_ADDRESS", "127.0.0.1".to_string()),
        ("ROCKET_PORT", pds_port.to_string()),
        ("PDS_PORT", pds_port.to_string()),
        ("PDS_HOSTNAME", "localhost".to_string()),
        ("PDS_SERVICE_DID", PDS_SERVICE_DID.to_string()),
        ("PDS_SERVICE_HANDLE_DOMAINS", HANDLE_DOMAIN.to_string()),
        ("PDS_ADMIN_PASS", ADMIN_PASS.to_string()),
        ("PDS_INVITE_REQUIRED", "false".to_string()),
        ("PDS_DID_PLC_URL", directory.url()),
        ("PDS_JWT_KEY_K256_PRIVATE_KEY_HEX", PDS_JWT_KEY.to_string()),
        (
            "PDS_PLC_ROTATION_KEY_K256_PRIVATE_KEY_HEX",
            PDS_ROTATION_KEY.to_string(),
        ),
        (
            "PDS_REPO_SIGNING_KEY_K256_PRIVATE_KEY_HEX",
            PDS_SIGNING_KEY.to_string(),
        ),
        (
            "PDS_ACCOUNT_DB_LOCATION",
            pds_dir.join("account.sqlite").display().to_string(),
        ),
        (
            "PDS_SEQUENCER_DB_LOCATION",
            pds_dir.join("sequencer.sqlite").display().to_string(),
        ),
        (
            "PDS_DID_CACHE_DB_LOCATION",
            pds_dir.join("did_cache.sqlite").display().to_string(),
        ),
        (
            "PDS_ACTOR_STORE_DIRECTORY",
            pds_actors.display().to_string(),
        ),
        (
            "PDS_BLOBSTORE_DISK_LOCATION",
            pds_dir.join("blobs").display().to_string(),
        ),
        ("RUST_LOG", "warn".to_string()),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_string(), value))
    .collect();

    let mut pds = Server::spawn(
        "oracle pds",
        &pds_bin,
        &run_dir,
        &pds_env,
        &run_dir.join("pds.log"),
    )?;
    pds.wait_ready(&format!("{pds_url}/xrpc/_health"), Duration::from_secs(60))
        .await?;
    println!("oracle pds ready on {pds_url}");

    let client = rsky_spaces_parity::layer2::http_client()?;
    let admin = format!("Basic {}", base64_standard(&format!("admin:{ADMIN_PASS}")));

    // Account, activation, session.
    let (status, body) = post_raw(
        &client,
        &format!("{pds_url}/xrpc/com.atproto.server.createAccount"),
        vec![("authorization", admin.clone())],
        &json!({
            "did": AUTHOR_DID,
            "email": "writer@layer2.test",
            "handle": HANDLE,
            "password": PASSWORD,
        }),
    )
    .await?;
    if status != 200 {
        bail!("createAccount failed ({status}): {body}\n{}", pds.tail());
    }
    let (status, body) = post_raw(
        &client,
        &format!("{pds_url}/xrpc/com.atproto.admin.updateSubjectStatus"),
        vec![("authorization", admin.clone())],
        &json!({
            "subject": {"$type": "com.atproto.admin.defs#repoRef", "did": AUTHOR_DID},
            "deactivated": {"applied": false},
        }),
    )
    .await?;
    if status != 200 {
        bail!("activation failed ({status}): {body}\n{}", pds.tail());
    }
    let (status, body) = post_raw(
        &client,
        &format!("{pds_url}/xrpc/com.atproto.server.createSession"),
        vec![],
        &json!({"identifier": HANDLE, "password": PASSWORD}),
    )
    .await?;
    if status != 200 {
        bail!("createSession failed ({status}): {body}\n{}", pds.tail());
    }
    let session = body["accessJwt"]
        .as_str()
        .context("session has no accessJwt")?
        .to_string();

    let (status, body) = post_raw(
        &client,
        &format!("{pds_url}/xrpc/com.atproto.simplespace.createSpace"),
        vec![("authorization", format!("Bearer {session}"))],
        &json!({"type": SPACE_TYPE, "skey": SPACE_SKEY}),
    )
    .await?;
    if status != 200 {
        bail!("createSpace failed ({status}): {body}\n{}", pds.tail());
    }
    let space = body["uri"]
        .as_str()
        .context("space has no uri")?
        .to_string();
    let expected = format!("at://{AUTHOR_DID}/space/{SPACE_TYPE}/{SPACE_SKEY}");
    if space != expected {
        bail!("space uri {space} is not the shared {expected}");
    }
    println!("space created on the oracle: {space}");

    // The space host reads account signing keys from a PDS-shaped actor store
    // directory and writes its own stores beside them.
    copy_tree(&pds_actors, &shim_actors)?;
    let accounts = reset_stores(&shim_actors)?;
    println!("space host store directory prepared for {accounts} account(s)");

    let shim_env: Vec<(String, String)> = vec![
        ("SPACEHOST_BIND", format!("127.0.0.1:{shim_port}")),
        ("SPACEHOST_PUBLIC_URL", shim_url.clone()),
        ("SPACEHOST_AUTHORITY_DID", AUTHOR_DID.to_string()),
        ("SPACEHOST_SIGNING_KEY_HEX", AUTHORITY_SPACE_KEY.to_string()),
        ("SPACEHOST_POLICY", "public".to_string()),
        ("SPACEHOST_PLC_URL", directory.url()),
        (
            "SPACEHOST_DB_PATH",
            shim_dir.join("space_host.db").display().to_string(),
        ),
        (
            "SPACEHOST_ACTOR_STORE_DIR",
            shim_actors.display().to_string(),
        ),
        ("SPACEHOST_OAUTH_ISSUER", OAUTH_ISSUER.to_string()),
        ("SPACEHOST_OAUTH_JWKS_URI", format!("{OAUTH_ISSUER}/jwks")),
        ("SPACEHOST_OAUTH_AUDIENCE", PDS_SERVICE_DID.to_string()),
        ("SPACEHOST_OAUTH_CLIENT_IDS", tokens::CLIENT_ID.to_string()),
        ("SPACEHOST_OAUTH_HS256_SECRET", HS256_SECRET.to_string()),
        ("SPACEHOST_MINT_TOKEN", "layer2-mint-token".to_string()),
        ("SPACEHOST_DAEMON_SERVICE_DID", DAEMON_DID.to_string()),
        ("SPACEHOST_APPVIEW_SERVICE_DID", DAEMON_DID.to_string()),
        ("RUST_LOG", "warn".to_string()),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_string(), value))
    .collect();

    let mut shim = Server::spawn(
        "space host",
        &shim_bin,
        &run_dir,
        &shim_env,
        &run_dir.join("shim.log"),
    )?;
    shim.wait_ready(&format!("{shim_url}/xrpc/_health"), Duration::from_secs(30))
        .await?;
    println!("space host ready on {shim_url}");

    let mut board = Scoreboard::default();

    // Credentials for the read endpoints, minted by each side's own authority.
    let pds_credential = mint_pds_credential(&client, &pds_url, &session, &space).await?;
    let shim_credential = mint_shim_credential(&client, &shim_url, &space).await?;
    println!("space credentials minted on both sides");

    let gate = Gate {
        client,
        pds_url: pds_url.clone(),
        shim_url: shim_url.clone(),
        space: space.clone(),
        session,
        pds_credential,
        shim_credential,
    };

    run_write_script(&gate, &mut board).await?;
    compare_reads(&gate, &mut board).await?;
    probe_pds_only_surface(&gate, &mut board).await?;

    // Both servers hold their stores open in WAL mode; stop them before the
    // stored-row comparison so the files are quiescent.
    shim.stop();
    pds.stop();

    compare_stores(&pds_actors, &shim_actors, &mut board)?;

    let report = board.render();
    print!("{report}");
    std::fs::write(run_dir.join("report.txt"), &report)?;
    println!("\nlogs: {}", run_dir.display());

    if board.failures() > 0 {
        bail!("layer 2 gate failed: {} check(s) differ", board.failures());
    }
    Ok(())
}

fn base64_standard(text: &str) -> String {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    STANDARD.encode(text.as_bytes())
}

async fn post_raw(
    client: &reqwest::Client,
    url: &str,
    headers: Vec<(&str, String)>,
    body: &Value,
) -> Result<(u16, Value)> {
    let mut request = client.post(url).json(body);
    for (name, value) in headers {
        request = request.header(name, value);
    }
    let response = request
        .send()
        .await
        .with_context(|| format!("POST {url}"))?;
    let status = response.status().as_u16();
    let text = response.text().await.unwrap_or_default();
    Ok((
        status,
        serde_json::from_str(&text).unwrap_or(Value::String(text)),
    ))
}

/// getDelegationToken then getSpaceCredential, the flow a member's own PDS runs.
async fn mint_pds_credential(
    client: &reqwest::Client,
    pds_url: &str,
    session: &str,
    space: &str,
) -> Result<String> {
    let nsid = "com.atproto.space.getDelegationToken";
    let url = format!("{pds_url}/xrpc/{nsid}?space={space}");
    let response = client
        .get(&url)
        .header("authorization", format!("Bearer {session}"))
        .send()
        .await?;
    let status = response.status().as_u16();
    let body: Value = serde_json::from_str(&response.text().await?).unwrap_or(Value::Null);
    if status != 200 {
        bail!("getDelegationToken failed ({status}): {body}");
    }
    let delegation = body["token"]
        .as_str()
        .context("delegation response has no token")?
        .to_string();

    let nsid = "com.atproto.space.getSpaceCredential";
    let url = format!("{pds_url}/xrpc/{nsid}");
    let response = client
        .post(&url)
        .header("authorization", format!("Bearer {delegation}"))
        .header("dpop", tokens::dpop_proof("POST", &url, None))
        .json(&json!({"space": space}))
        .send()
        .await?;
    let status = response.status().as_u16();
    let body: Value = serde_json::from_str(&response.text().await?).unwrap_or(Value::Null);
    if status != 200 {
        bail!("getSpaceCredential failed ({status}): {body}");
    }
    Ok(body["credential"]
        .as_str()
        .context("credential response has no credential")?
        .to_string())
}

/// The space host's administrative mint, which stands in for a syncer asking
/// for a credential.
async fn mint_shim_credential(
    client: &reqwest::Client,
    shim_url: &str,
    space: &str,
) -> Result<String> {
    let signer = rsky_space_host::signing::Signer::from_hex(DAEMON_KEY)
        .map_err(|error| anyhow::anyhow!("daemon signer: {error}"))?;
    let jwt = tokens::service_jwt(
        &signer,
        DAEMON_DID,
        AUTHOR_DID,
        "community.blacksky.space.mintCredential",
    )?;
    let url = format!("{shim_url}/admin/mintCredential");
    let response = client
        .post(format!("{url}?space={space}"))
        .header("authorization", format!("Bearer {jwt}"))
        .header("x-spacehost-mint-token", "layer2-mint-token")
        .header("dpop", tokens::dpop_proof("POST", &url, None))
        .send()
        .await?;
    let status = response.status().as_u16();
    let body: Value = serde_json::from_str(&response.text().await?).unwrap_or(Value::Null);
    if status != 200 {
        bail!("mintCredential failed ({status}): {body}");
    }
    Ok(body["credential"]
        .as_str()
        .context("mint response has no credential")?
        .to_string())
}

async fn run_write_script(gate: &Gate, board: &mut Scoreboard) -> Result<()> {
    let mut shim_revs = Revs::default();
    let mut pds_revs = Revs::default();
    for (index, step) in script().into_iter().enumerate() {
        let nsid = step.nsid();
        let body = step.body(&gate.space, AUTHOR_DID);
        let (shim_status, shim_body) = gate
            .post(&gate.shim_url, nsid, gate.shim_write_headers(nsid), &body)
            .await?;
        let (pds_status, pds_body) = gate
            .post(&gate.pds_url, nsid, gate.pds_write_headers(), &body)
            .await?;

        let name = format!("write {:02} {}", index + 1, step.label());
        let shim_view = write_view(shim_status, &shim_body, &mut shim_revs);
        let pds_view = write_view(pds_status, &pds_body, &mut pds_revs);
        board.equal_if(
            name,
            shim_view == pds_view,
            if shim_view == pds_view {
                shim_view.clone()
            } else {
                format!("shim: {shim_view}\npds:  {pds_view}")
            },
        );
    }
    board.push(
        "write revisions are TIDs",
        if normalize::revs_are_tids(&shim_revs) && normalize::revs_are_tids(&pds_revs) {
            Verdict::Equal
        } else {
            Verdict::Differs
        },
        format!(
            "shim minted {} revisions, pds minted {}",
            shim_revs.count(),
            pds_revs.count()
        ),
    );
    Ok(())
}

/// A write's comparable outcome: HTTP status, the XRPC error name when it
/// failed, and the record identity when it succeeded.
fn write_view(status: u16, body: &Value, revs: &mut Revs) -> String {
    if status != 200 {
        return format!(
            "{status} {}",
            body["error"].as_str().unwrap_or("<no error name>")
        );
    }
    let normalized = normalize::normalize(body, revs);
    let uri = normalized["uri"].as_str().unwrap_or("-").to_string();
    let cid = normalized["cid"].as_str().unwrap_or("-").to_string();
    let commit = &normalized["commit"];
    format!(
        "200 uri={uri} cid={cid} rev={} hash={}",
        commit["rev"].as_str().unwrap_or("-"),
        commit["hash"].as_str().unwrap_or("-"),
    )
}

async fn compare_reads(gate: &Gate, board: &mut Scoreboard) -> Result<()> {
    let space = gate.space.clone();

    for (nsid, query) in [
        (
            "com.atproto.space.listRepoOps",
            format!("space={space}&repo={AUTHOR_DID}"),
        ),
        (
            "com.atproto.space.listRepoOps",
            format!("space={space}&repo={AUTHOR_DID}&limit=2"),
        ),
        (
            "com.atproto.space.getLatestCommit",
            format!("space={space}&repo={AUTHOR_DID}"),
        ),
        ("com.atproto.space.listRepos", format!("space={space}")),
        ("com.atproto.space.getSpace", format!("space={space}")),
    ] {
        let (shim_status, shim_body) = gate
            .get(&gate.shim_url, nsid, &query, gate.shim_read_headers(nsid))
            .await?;
        let (pds_status, pds_body) = gate
            .get(&gate.pds_url, nsid, &query, gate.pds_read_headers(nsid))
            .await?;
        let mut shim_revs = Revs::default();
        let mut pds_revs = Revs::default();
        let shim_view = normalize::render(&normalize::normalize(&shim_body, &mut shim_revs));
        let pds_view = normalize::render(&normalize::normalize(&pds_body, &mut pds_revs));
        let label = match query.contains("limit=") {
            true => format!("read {nsid} (paged)"),
            false => format!("read {nsid}"),
        };
        let equal = shim_status == pds_status && shim_view == pds_view;
        // `getSpace` answers from the space definition, which the storage
        // convergence deliberately leaves out of scope: the space host holds it
        // in configuration, the PDS in its own `space_def` rows.
        if nsid == "com.atproto.space.getSpace" && !equal {
            board.push(
                label,
                Verdict::Documented,
                format!(
                    "space definition is configured on the host and stored on the pds\n\
                     shim: {shim_status} {shim_view}\npds:  {pds_status} {pds_view}"
                ),
            );
            continue;
        }
        board.equal_if(
            label,
            equal,
            if equal {
                format!("{shim_status}")
            } else {
                format!("shim: {shim_status} {shim_view}\npds:  {pds_status} {pds_view}")
            },
        );
    }

    // getRepo returns a CAR whose commit block carries fresh random key
    // material, so the record blocks are what can match.
    let nsid = "com.atproto.space.getRepo";
    let query = format!("space={space}&repo={AUTHOR_DID}");
    let (shim_status, shim_car) = gate
        .get_bytes(&gate.shim_url, nsid, &query, gate.shim_read_headers(nsid))
        .await?;
    let (pds_status, pds_car) = gate
        .get_bytes(&gate.pds_url, nsid, &query, gate.pds_read_headers(nsid))
        .await?;
    if shim_status != 200 || pds_status != 200 {
        board.equal_if(
            format!("read {nsid}"),
            false,
            format!("shim: {shim_status}, pds: {pds_status}"),
        );
        return Ok(());
    }
    let diff = car::diff(&shim_car, &pds_car)?;
    let commit_only = diff.only_shim.len() == 1 && diff.only_pds.len() == 1;
    board.equal_if(
        format!("read {nsid} record blocks"),
        commit_only && diff.shared > 0,
        format!(
            "{} shared blocks; unique to shim: {:?}; unique to pds: {:?}",
            diff.shared, diff.only_shim, diff.only_pds
        ),
    );
    Ok(())
}

/// The four methods the PDS serves and the space host does not. Recorded as a
/// surface difference: a client moving from the host to the PDS gains them.
async fn probe_pds_only_surface(gate: &Gate, board: &mut Scoreboard) -> Result<()> {
    let space = gate.space.clone();
    let probes: Vec<(&str, &str, String, Option<Value>)> = vec![
        (
            "com.atproto.space.getRecord",
            "GET",
            format!("space={space}&repo={AUTHOR_DID}&collection={COLLECTION}&rkey=3kaaaaaaaaaa2"),
            None,
        ),
        (
            "com.atproto.space.listRecords",
            "GET",
            format!("space={space}&repo={AUTHOR_DID}"),
            None,
        ),
        (
            "com.atproto.space.getRepoState",
            "GET",
            format!("space={space}&repo={AUTHOR_DID}"),
            None,
        ),
        (
            "com.atproto.space.applyWrites",
            "POST",
            String::new(),
            // Deliberately invalid, so the probe cannot write to one side only:
            // a routed method rejects it, an unrouted one is not found.
            Some(json!({
                "space": space,
                "repo": AUTHOR_DID,
                "writes": [{
                    "action": "not-an-action",
                    "collection": COLLECTION,
                    "rkey": "3kprobeaaaaaa",
                }],
            })),
        ),
    ];
    for (nsid, method, query, body) in probes {
        let (pds_status, shim_status) = if method == "GET" {
            let (pds_status, _) = gate
                .get(&gate.pds_url, nsid, &query, gate.pds_read_headers(nsid))
                .await?;
            let (shim_status, _) = gate
                .get(&gate.shim_url, nsid, &query, gate.shim_read_headers(nsid))
                .await?;
            (pds_status, shim_status)
        } else {
            let body = body.unwrap_or(Value::Null);
            let (pds_status, _) = gate
                .post(&gate.pds_url, nsid, gate.pds_write_headers(), &body)
                .await?;
            let (shim_status, _) = gate
                .post(&gate.shim_url, nsid, gate.shim_write_headers(nsid), &body)
                .await?;
            (pds_status, shim_status)
        };
        board.push(
            format!("surface {nsid}"),
            Verdict::Note,
            format!("pds {pds_status}, space host {shim_status} (not routed)"),
        );
    }
    Ok(())
}

fn compare_stores(pds_actors: &Path, shim_actors: &Path, board: &mut Scoreboard) -> Result<()> {
    let pds_store = rsky_space_host::actor_repos::store_path(pds_actors, AUTHOR_DID)
        .map_err(|error| anyhow::anyhow!("pds store path: {error}"))?;
    let shim_store = rsky_space_host::actor_repos::store_path(shim_actors, AUTHOR_DID)
        .map_err(|error| anyhow::anyhow!("shim store path: {error}"))?;
    for path in [&pds_store, &shim_store] {
        if !path.exists() {
            bail!("no store file at {}", path.display());
        }
    }
    let shim = dump_tables(&shim_store);
    let pds = dump_tables(&pds_store);
    let equal = compare_tables("store", &shim, &pds);
    board.equal_if(
        "stored space_* rows",
        equal,
        format!("{} / {}", shim_store.display(), pds_store.display()),
    );
    let sound =
        revs_are_well_formed("store", &shim, "shim") && revs_are_well_formed("store", &pds, "pds");
    board.equal_if(
        "stored revisions well formed",
        sound,
        format!(
            "shim {} distinct revisions, pds {}",
            shim.revs.len(),
            pds.revs.len()
        ),
    );
    Ok(())
}
