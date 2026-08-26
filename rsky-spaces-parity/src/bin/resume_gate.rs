//! Resume-across-swap gate: the upgrade the storage convergence ships,
//! simulated end to end with real processes.
//!
//! Three eras run against one space-host database and one daemon index:
//! the legacy multi-tenant store, the one-off conversion, and the converged
//! per-account stores. The daemon keeps its durable cursors across all three,
//! and the gate asserts what it projected on the far side.
//!
//! Run it through `rsky-spaces-parity/resume-gate/run.sh`, which builds every
//! binary involved and passes their paths in.

use anyhow::{bail, Context, Result};
use rsky_spaces_parity::layer2::process::{free_port, Server};
use rsky_spaces_parity::layer2::{directory::Directory, tokens, Scoreboard, Verdict};
use rsky_spaces_parity::resume::sink::Sink;
use rsky_spaces_parity::resume::{
    converged_head, converged_oplog, duplicates, final_state, labels, legacy_oplog, read_index,
    unclean_resume_lines,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const AUTHOR_DID: &str = "did:plc:resumewriteraaaaaaaaaa";
const DAEMON_DID: &str = "did:plc:resumedaemonaaaaaaaaaa";
const HANDLE: &str = "writer.resume.test";
const SPACE_TYPE: &str = "community.blacksky.feed";
const SPACE_SKEY: &str = "main";
const COLLECTION: &str = "app.bsky.feed.post";
const HS256_SECRET: &str = "resume-local-authorization-server-secret";
const OAUTH_ISSUER: &str = "http://localhost:0/oauth";
const OAUTH_AUDIENCE: &str = "did:web:localho.st";
const MINT_TOKEN: &str = "resume-mint-token";

/// Fixed local key material. None of it protects anything: the whole stack is
/// created and destroyed inside one run directory.
const AUTHOR_KEY: &str = "3333333333333333333333333333333333333333333333333333333333333333";
const DAEMON_KEY: &str = "2222222222222222222222222222222222222222222222222222222222222222";

/// The record keys written, in the order the script writes them.
const POST_1: &str = "3kresumepost1";
const POST_2: &str = "3kresumepost2";
const POST_3: &str = "3kresumepost3";
const POST_4: &str = "3kresumepost4";
const POST_5: &str = "3kresumepost5";

const READY: Duration = Duration::from_secs(30);
const SETTLE: Duration = Duration::from_secs(45);

struct Writer {
    client: reqwest::Client,
    shim_url: String,
    space: String,
}

impl Writer {
    /// Create a record as the author, over XRPC, exactly as a client does.
    async fn create(&self, rkey: &str, text: &str) -> Result<String> {
        let body = json!({
            "space": self.space,
            "repo": AUTHOR_DID,
            "collection": COLLECTION,
            "rkey": rkey,
            "record": {
                "$type": COLLECTION,
                "text": text,
                "createdAt": "2026-08-21T00:00:00.000Z",
            },
        });
        self.write("com.atproto.space.createRecord", &body).await
    }

    async fn delete(&self, rkey: &str) -> Result<String> {
        let body = json!({
            "space": self.space,
            "repo": AUTHOR_DID,
            "collection": COLLECTION,
            "rkey": rkey,
        });
        self.write("com.atproto.space.deleteRecord", &body).await
    }

    /// Returns the revision the host committed the write at.
    async fn write(&self, nsid: &str, body: &Value) -> Result<String> {
        let url = format!("{}/xrpc/{nsid}", self.shim_url);
        let token = tokens::access_token(HS256_SECRET, OAUTH_ISSUER, OAUTH_AUDIENCE, AUTHOR_DID);
        let proof = tokens::dpop_proof("POST", &url, Some(&token));
        let response = self
            .client
            .post(&url)
            .header("authorization", format!("DPoP {token}"))
            .header("dpop", proof)
            .json(body)
            .send()
            .await
            .with_context(|| format!("POST {url}"))?;
        let status = response.status().as_u16();
        let text = response.text().await.unwrap_or_default();
        let parsed: Value = serde_json::from_str(&text).unwrap_or(Value::String(text.clone()));
        if status != 200 {
            bail!("{nsid} returned {status}: {parsed}");
        }
        Ok(parsed["commit"]["rev"]
            .as_str()
            .with_context(|| format!("{nsid} response has no commit revision: {parsed}"))?
            .to_string())
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

/// The PDS actor-store layout both eras of the space host read signing keys
/// from: `{root}/{sha256(did)[..2]}/{did}/{key,store.sqlite}`.
fn seed_actor_store(root: &Path, did: &str, key_hex: &str) -> Result<PathBuf> {
    let digest = hex::encode(Sha256::digest(did.as_bytes()));
    let account = root.join(&digest[..2]).join(did);
    std::fs::create_dir_all(&account)?;
    let key = hex::decode(key_hex).context("actor key is not hex")?;
    std::fs::write(account.join("key"), key)?;
    let store = account.join("store.sqlite");
    rsky_space_host::actor_schema::get_migrated_db(&store)
        .map_err(|error| anyhow::anyhow!("migrate empty store: {error}"))?;
    Ok(store)
}

/// `space_stores` is `None` for the legacy era, which predates the split of
/// the key source from the space-store write target.
fn shim_env(
    port: u16,
    public_url: &str,
    db_path: &Path,
    actors: &Path,
    space_stores: Option<&Path>,
    plc_url: &str,
) -> Vec<(String, String)> {
    let mut env = vec![
        ("SPACEHOST_BIND", format!("127.0.0.1:{port}")),
        ("SPACEHOST_PUBLIC_URL", public_url.to_string()),
        ("SPACEHOST_AUTHORITY_DID", AUTHOR_DID.to_string()),
        ("SPACEHOST_SIGNING_KEY_HEX", AUTHOR_KEY.to_string()),
        ("SPACEHOST_POLICY", "public".to_string()),
        ("SPACEHOST_PLC_URL", plc_url.to_string()),
        ("SPACEHOST_DB_PATH", db_path.display().to_string()),
        ("SPACEHOST_ACTOR_STORE_DIR", actors.display().to_string()),
        ("SPACEHOST_OAUTH_ISSUER", OAUTH_ISSUER.to_string()),
        ("SPACEHOST_OAUTH_JWKS_URI", format!("{OAUTH_ISSUER}/jwks")),
        ("SPACEHOST_OAUTH_AUDIENCE", OAUTH_AUDIENCE.to_string()),
        ("SPACEHOST_OAUTH_CLIENT_IDS", tokens::CLIENT_ID.to_string()),
        ("SPACEHOST_OAUTH_HS256_SECRET", HS256_SECRET.to_string()),
        ("SPACEHOST_MINT_TOKEN", MINT_TOKEN.to_string()),
        ("SPACEHOST_DAEMON_SERVICE_DID", DAEMON_DID.to_string()),
        ("SPACEHOST_APPVIEW_SERVICE_DID", DAEMON_DID.to_string()),
        ("RUST_LOG", "warn".to_string()),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_string(), value.to_string()))
    .collect::<Vec<(String, String)>>();
    if let Some(stores) = space_stores {
        env.push((
            "SPACEHOST_SPACE_STORE_DIR".to_string(),
            stores.display().to_string(),
        ));
    }
    env
}

struct DaemonSetup {
    index_db: PathBuf,
    dpop_key: PathBuf,
    notify_port: u16,
    sink_url: String,
}

fn daemon_env(
    space: &str,
    shim_url: &str,
    setup: &DaemonSetup,
    plc_url: &str,
) -> Vec<(String, String)> {
    vec![
        ("DAEMON_SPACE_URI", space.to_string()),
        ("DAEMON_SPACE_HOST_URL", shim_url.to_string()),
        ("DAEMON_SERVICE_IDENTITY", DAEMON_DID.to_string()),
        ("DAEMON_SERVICE_SIGNING_KEY_HEX", DAEMON_KEY.to_string()),
        ("DAEMON_SPACE_HOST_MINT_TOKEN", MINT_TOKEN.to_string()),
        ("DAEMON_DPOP_KEY_PATH", setup.dpop_key.display().to_string()),
        ("DAEMON_INDEX_DB_PATH", setup.index_db.display().to_string()),
        (
            "DAEMON_NOTIFY_BIND",
            format!("127.0.0.1:{}", setup.notify_port),
        ),
        ("DAEMON_SWEEP_INTERVAL_SECS", "1".to_string()),
        ("DAEMON_PLC_URL", plc_url.to_string()),
        ("DAEMON_FEEDS_URL", setup.sink_url.clone()),
        ("DAEMON_FEEDS_SERVICE_DID", DAEMON_DID.to_string()),
        ("RUST_LOG", "info".to_string()),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_string(), value))
    .collect()
}

/// Poll `check` until it holds, or fail with what was last seen.
async fn wait_until<F>(what: &str, timeout: Duration, mut check: F) -> Result<()>
where
    F: FnMut() -> Result<Option<String>>,
{
    let deadline = Instant::now() + timeout;
    loop {
        let Some(state) = check()? else {
            return Ok(());
        };
        if Instant::now() >= deadline {
            bail!("timed out waiting for {what}; last seen: {state}");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let run_dir = env_path("RESUME_RUN_DIR", "target/resume/run");
    let legacy_bin = env_path("RESUME_LEGACY_SHIM_BIN", "");
    let shim_bin = env_path("RESUME_SHIM_BIN", "target/debug/rsky-space-host");
    let daemon_bin = env_path("RESUME_DAEMON_BIN", "target/debug/rsky-daemon");
    let convert_bin = env_path("RESUME_CONVERT_BIN", "target/debug/convert_store");
    for (label, path) in [
        ("RESUME_LEGACY_SHIM_BIN", &legacy_bin),
        ("RESUME_SHIM_BIN", &shim_bin),
        ("RESUME_DAEMON_BIN", &daemon_bin),
        ("RESUME_CONVERT_BIN", &convert_bin),
    ] {
        if !path.is_file() {
            bail!("{label} must point at a built binary (got {path:?})");
        }
    }

    if run_dir.exists() {
        std::fs::remove_dir_all(&run_dir).context("clear run directory")?;
    }
    std::fs::create_dir_all(&run_dir)?;
    let run_dir = run_dir.canonicalize()?;
    let host_dir = run_dir.join("host");
    let legacy_actors = host_dir.join("actors-legacy");
    let converged_actors = host_dir.join("actors");
    let host_db = host_dir.join("space_host.db");
    std::fs::create_dir_all(&legacy_actors)?;
    std::fs::create_dir_all(&converged_actors)?;

    // The legacy era keeps its repos in the multi-tenant `space_host.db`; the
    // actor store beside it holds nothing but the account's signing key.
    seed_actor_store(&legacy_actors, AUTHOR_DID, AUTHOR_KEY)?;

    let mut keys = BTreeMap::new();
    keys.insert(AUTHOR_DID.to_string(), multibase_of(AUTHOR_KEY)?);
    keys.insert(DAEMON_DID.to_string(), multibase_of(DAEMON_KEY)?);
    let directory = Directory::start(keys, HANDLE.to_string())?;

    let shim_port = free_port()?;
    let shim_url = format!("http://127.0.0.1:{shim_port}");
    let space = format!("at://{AUTHOR_DID}/space/{SPACE_TYPE}/{SPACE_SKEY}");

    let warm_sink = Sink::start()?;
    let warm = DaemonSetup {
        index_db: run_dir.join("daemon/index.sqlite"),
        dpop_key: run_dir.join("daemon/dpop.json"),
        notify_port: free_port()?,
        sink_url: warm_sink.url(),
    };
    std::fs::create_dir_all(run_dir.join("daemon"))?;

    let mut board = Scoreboard::default();
    let client = rsky_spaces_parity::layer2::http_client()?;

    // ---- Phase A: the legacy era ------------------------------------------
    let mut legacy = Server::spawn(
        "legacy space host",
        &legacy_bin,
        &run_dir,
        &shim_env(
            shim_port,
            &shim_url,
            &host_db,
            &legacy_actors,
            None,
            &directory.url(),
        ),
        &run_dir.join("shim-legacy.log"),
    )?;
    legacy
        .wait_ready(&format!("{shim_url}/xrpc/_health"), READY)
        .await?;
    println!("legacy space host ready on {shim_url}");

    let writer = Writer {
        client: client.clone(),
        shim_url: shim_url.clone(),
        space: space.clone(),
    };
    let rev_1 = writer.create(POST_1, "legacy one").await?;
    let rev_2 = writer.create(POST_2, "legacy two").await?;
    println!("legacy writes committed at {rev_1}, {rev_2}");

    let mut daemon = Server::spawn(
        "daemon (legacy era)",
        &daemon_bin,
        &run_dir,
        &daemon_env(&space, &shim_url, &warm, &directory.url()),
        &run_dir.join("daemon-legacy.log"),
    )?;
    daemon.wait_log("daemon starting", READY).await?;
    println!("daemon running against the legacy store");

    // The daemon has caught up when its durable cursor is the head revision
    // and the feeds projector has confirmed that revision.
    let index_db = warm.index_db.clone();
    {
        let (space, rev_2) = (space.clone(), rev_2.clone());
        let index_db = index_db.clone();
        wait_until(
            "the daemon to persist its legacy-era cursor",
            SETTLE,
            move || {
                let snapshot = read_index(&index_db, &space, AUTHOR_DID)?;
                let acked = snapshot.head_rev.as_deref() == Some(rev_2.as_str())
                    && snapshot.projector_cursors.get("feeds").map(String::as_str)
                        == Some(rev_2.as_str());
                Ok((!acked).then(|| format!("{snapshot:?}")))
            },
        )
        .await?;
    }
    let legacy_projected = warm_sink.drain();
    let before_swap = read_index(&index_db, &space, AUTHOR_DID)?;
    println!(
        "daemon cursor persisted at {:?} after projecting {:?}",
        before_swap.head_rev,
        labels(&legacy_projected)
    );

    board.equal_if(
        "legacy era: the daemon projects the writes it saw, once each",
        labels(&legacy_projected)
            == vec![post_label(POST_1, "create"), post_label(POST_2, "create")]
            && duplicates(&legacy_projected).is_empty(),
        format!("{:?}", labels(&legacy_projected)),
    );
    board.equal_if(
        "legacy era: the cursor is durable",
        before_swap.head_rev.as_deref() == Some(rev_2.as_str()),
        format!("cursor {:?}, head write {rev_2}", before_swap.head_rev),
    );

    // Stop the daemon mid-stream, then land writes it never acknowledged.
    daemon.stop();
    let legacy_daemon_log = std::fs::read_to_string(&daemon.log).unwrap_or_default();
    println!("daemon stopped; landing writes it will never have seen");
    let rev_3 = writer.create(POST_3, "legacy three, unseen").await?;
    let rev_4 = writer.create(POST_4, "legacy four, unseen").await?;
    let rev_5 = writer.delete(POST_2).await?;
    println!("unacknowledged legacy writes at {rev_3}, {rev_4}, {rev_5}");

    let still = read_index(&index_db, &space, AUTHOR_DID)?;
    board.equal_if(
        "legacy era: writes after the last ack leave the cursor behind",
        still.head_rev.as_deref() == Some(rev_2.as_str()) && rev_5 > rev_2,
        format!("cursor {:?}, latest write {rev_5}", still.head_rev),
    );

    legacy.stop();
    println!("legacy space host stopped");

    // ---- Phase B: the swap ------------------------------------------------
    let legacy_ops = legacy_oplog(&host_db, &space, AUTHOR_DID)?;
    let convert = std::process::Command::new(&convert_bin)
        .arg("--from")
        .arg(&host_db)
        .arg("--into")
        .arg(&converged_actors)
        .output()
        .context("run the store converter")?;
    if !convert.status.success() {
        bail!(
            "conversion failed: {}{}",
            String::from_utf8_lossy(&convert.stdout),
            String::from_utf8_lossy(&convert.stderr)
        );
    }
    let conversion = String::from_utf8_lossy(&convert.stdout).trim().to_string();
    println!("conversion: {conversion}");
    // The converted stores need the account key beside them, exactly as the
    // deploy leaves it: the same actor-store directory, new store files.
    let key_source = legacy_actors
        .join(&hex::encode(Sha256::digest(AUTHOR_DID.as_bytes()))[..2])
        .join(AUTHOR_DID)
        .join("key");
    let store = rsky_space_host::actor_repos::store_path(&converged_actors, AUTHOR_DID)
        .map_err(|error| anyhow::anyhow!("store path: {error}"))?;
    std::fs::copy(
        &key_source,
        store.parent().context("store has no parent")?.join("key"),
    )?;

    let converged_ops = converged_oplog(&store, &space)?;
    board.equal_if(
        "conversion: oplog ids and revisions survive verbatim",
        converged_ops == legacy_ops && !legacy_ops.is_empty(),
        format!(
            "{} legacy ops -> {} converged ops; ids {:?}",
            legacy_ops.len(),
            converged_ops.len(),
            converged_ops.iter().map(|op| op.0).collect::<Vec<_>>()
        ),
    );
    let (converged_rev, floor) = converged_head(&store, &space)?;
    board.equal_if(
        "conversion: no history is placed out of reach",
        floor.is_none() && converged_rev == rev_5,
        format!("head {converged_rev}, oplog floor {floor:?}"),
    );

    // ---- Phase C: the converged era ---------------------------------------
    let mut converged = Server::spawn(
        "converged space host",
        &shim_bin,
        &run_dir,
        &shim_env(
            shim_port,
            &shim_url,
            &host_db,
            &legacy_actors,
            Some(&converged_actors),
            &directory.url(),
        ),
        &run_dir.join("shim-converged.log"),
    )?;
    converged
        .wait_ready(&format!("{shim_url}/xrpc/_health"), READY)
        .await?;
    println!("converged space host ready on {shim_url}");

    // The same daemon, the same index, the same DPoP key, the same arguments.
    let mut daemon = Server::spawn(
        "daemon (converged era)",
        &daemon_bin,
        &run_dir,
        &daemon_env(&space, &shim_url, &warm, &directory.url()),
        &run_dir.join("daemon-converged.log"),
    )?;
    daemon.wait_log("daemon starting", READY).await?;
    println!("daemon restarted with its existing cursors");

    {
        let (space, rev_5) = (space.clone(), rev_5.clone());
        let index_db = index_db.clone();
        wait_until(
            "the daemon to resume to the pre-swap head",
            SETTLE,
            move || {
                let snapshot = read_index(&index_db, &space, AUTHOR_DID)?;
                let caught_up = snapshot.head_rev.as_deref() == Some(rev_5.as_str())
                    && snapshot.projector_cursors.get("feeds").map(String::as_str)
                        == Some(rev_5.as_str());
                Ok((!caught_up).then(|| format!("{snapshot:?}")))
            },
        )
        .await?;
    }
    let resumed = warm_sink.drain();
    println!("resume projected {:?}", labels(&resumed));

    let mut expected_resume = vec![
        post_label(POST_2, "delete"),
        post_label(POST_3, "create"),
        post_label(POST_4, "create"),
    ];
    expected_resume.sort();
    let mut got_resume = labels(&resumed);
    got_resume.sort();
    board.equal_if(
        "resume: exactly the operations after the cursor, once each",
        got_resume == expected_resume,
        format!("expected {expected_resume:?}, got {got_resume:?}"),
    );
    board.equal_if(
        "resume: nothing already projected is projected again",
        duplicates(&resumed).is_empty()
            && !got_resume.contains(&post_label(POST_1, "create"))
            && !got_resume.contains(&post_label(POST_2, "create")),
        format!("duplicates {:?}", duplicates(&resumed)),
    );

    // A write in the converged era, at a server-minted revision.
    let rev_6 = writer.create(POST_5, "converged five").await?;
    board.equal_if(
        "converged era: a server-minted revision follows the carried one",
        rev_6 > rev_5,
        format!("carried {rev_5}, minted {rev_6}"),
    );
    {
        let (space, rev_6) = (space.clone(), rev_6.clone());
        let index_db = index_db.clone();
        wait_until(
            "the daemon to project the converged-era write",
            SETTLE,
            move || {
                let snapshot = read_index(&index_db, &space, AUTHOR_DID)?;
                let caught_up = snapshot.head_rev.as_deref() == Some(rev_6.as_str())
                    && snapshot.projector_cursors.get("feeds").map(String::as_str)
                        == Some(rev_6.as_str());
                Ok((!caught_up).then(|| format!("{snapshot:?}")))
            },
        )
        .await?;
    }
    let after_swap_write = warm_sink.drain();
    board.equal_if(
        "converged era: the new write projects once",
        labels(&after_swap_write) == vec![post_label(POST_5, "create")],
        format!("{:?}", labels(&after_swap_write)),
    );

    daemon.stop();
    let converged_daemon_log = std::fs::read_to_string(&daemon.log).unwrap_or_default();
    let unclean = unclean_resume_lines(&converged_daemon_log);
    board.equal_if(
        "resume: no cursor refusal, divergence or full-state recovery",
        unclean.is_empty(),
        if unclean.is_empty() {
            "the daemon advanced incrementally throughout".to_string()
        } else {
            unclean.join("\n")
        },
    );
    board.push(
        "legacy-era daemon log was also clean",
        if unclean_resume_lines(&legacy_daemon_log).is_empty() {
            Verdict::Equal
        } else {
            Verdict::Differs
        },
        unclean_resume_lines(&legacy_daemon_log).join("\n"),
    );

    let warm_index = read_index(&index_db, &space, AUTHOR_DID)?;

    // ---- A cold daemon on the converted store -----------------------------
    let cold_sink = Sink::start()?;
    let cold = DaemonSetup {
        index_db: run_dir.join("cold/index.sqlite"),
        dpop_key: run_dir.join("cold/dpop.json"),
        notify_port: free_port()?,
        sink_url: cold_sink.url(),
    };
    std::fs::create_dir_all(run_dir.join("cold"))?;
    let mut cold_daemon = Server::spawn(
        "cold daemon",
        &daemon_bin,
        &run_dir,
        &daemon_env(&space, &shim_url, &cold, &directory.url()),
        &run_dir.join("daemon-cold.log"),
    )?;
    cold_daemon.wait_log("daemon starting", READY).await?;
    {
        let (space, rev_6) = (space.clone(), rev_6.clone());
        let cold_db = cold.index_db.clone();
        wait_until("the cold daemon to sync from scratch", SETTLE, move || {
            let snapshot = read_index(&cold_db, &space, AUTHOR_DID)?;
            let caught_up = snapshot.head_rev.as_deref() == Some(rev_6.as_str())
                && snapshot.projector_cursors.get("feeds").map(String::as_str)
                    == Some(rev_6.as_str());
            Ok((!caught_up).then(|| format!("{snapshot:?}")))
        })
        .await?;
    }
    let cold_projected = cold_sink.seen();
    cold_daemon.stop();
    converged.stop();

    let cold_index = read_index(&cold.index_db, &space, AUTHOR_DID)?;
    board.equal_if(
        "cold sync: the same records, revisions and digest as the resumed daemon",
        cold_index.records == warm_index.records
            && cold_index.head_rev == warm_index.head_rev
            && cold_index.lthash_state == warm_index.lthash_state,
        format!(
            "warm {} records at {:?}, cold {} records at {:?}",
            warm_index.records.len(),
            warm_index.head_rev,
            cold_index.records.len(),
            cold_index.head_rev
        ),
    );

    let warm_stream: Vec<_> = legacy_projected
        .iter()
        .chain(resumed.iter())
        .chain(after_swap_write.iter())
        .cloned()
        .collect();
    board.equal_if(
        "cold sync: the same projected end state as the resumed daemon",
        final_state(&cold_projected) == final_state(&warm_stream),
        format!(
            "warm {:?}\ncold {:?}",
            final_state(&warm_stream),
            final_state(&cold_projected)
        ),
    );
    board.equal_if(
        "whole run: no operation is projected twice to either destination",
        duplicates(&warm_stream).is_empty() && duplicates(&cold_projected).is_empty(),
        format!(
            "warm {:?}, cold {:?}",
            duplicates(&warm_stream),
            duplicates(&cold_projected)
        ),
    );
    board.push(
        "oplog window was never the constraint",
        Verdict::Note,
        format!(
            "{} ops in a {} row window; the converted floor is open, so no `since` can be refused",
            converged_ops.len(),
            rsky_space_host::actor_repos::DEFAULT_OPLOG_WINDOW
        ),
    );

    let report = format!(
        "resume-across-swap gate\n\n\
         space: {space}\n\
         legacy head at hand-off: {rev_2}\n\
         unacknowledged legacy writes: {rev_3}, {rev_4}, {rev_5}\n\
         converged-era write: {rev_6}\n\
         {conversion}\n\n{}",
        board.render()
    );
    print!("{report}");
    std::fs::write(run_dir.join("report.txt"), &report)?;
    println!("\nlogs: {}", run_dir.display());

    if board.failures() > 0 {
        bail!("resume gate failed: {} check(s) differ", board.failures());
    }
    Ok(())
}

fn post_label(rkey: &str, operation: &str) -> String {
    format!("{COLLECTION}/{rkey}:{operation}")
}
