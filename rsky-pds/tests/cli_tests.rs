//! The `rsky-pds` binary's maintenance mode, driven as an operator would.

use std::process::Command;

const DID: &str = "did:plc:cli";

fn binary(dir: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rsky-pds"));
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
        .env("PDS_REPAIR_DB", path("rsky/repair.sqlite"));
    if let Some(profile) = std::env::var_os("LLVM_PROFILE_FILE") {
        command.env("LLVM_PROFILE_FILE", profile);
    }
    command
}

#[tokio::test]
async fn drain_mode_reports_and_exits_by_outcome() {
    let dir = tempfile::tempdir().unwrap();

    let bogus = binary(dir.path()).arg("--bogus").output().unwrap();
    assert_eq!(bogus.status.code(), Some(2));

    // nothing is owed for an actor this server never wrote
    let drained = binary(dir.path())
        .args(["--drain-did", DID, "--timeout-secs", "1"])
        .output()
        .unwrap();
    assert_eq!(
        drained.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&drained.stderr)
    );
    let status: serde_json::Value =
        serde_json::from_slice(&drained.stdout).expect("the drain prints its status");
    assert_eq!(status["did"], DID);
    assert_eq!(status["fullyDrained"], true);

    // an in-progress deletion keeps the actor from being fully drained
    let lifecycle =
        rsky_pds::lifecycle::LifecycleStore::open(dir.path().join("rsky/lifecycle.sqlite"))
            .await
            .unwrap();
    lifecycle.tombstone(DID).await.unwrap();
    drop(lifecycle);
    let owed = binary(dir.path())
        .args(["--drain-did", DID, "--timeout-secs", "1"])
        .output()
        .unwrap();
    assert_eq!(owed.status.code(), Some(1));
    let status: serde_json::Value = serde_json::from_slice(&owed.stdout).unwrap();
    assert_eq!(status["lifecyclePending"], 1);

    // a repair is created from a file, refused while the account is not
    // fenced for it, and reported; a quarantine opens and reports
    let spec = dir.path().join("repair.json");
    std::fs::write(
        &spec,
        format!(
            r#"{{"id":"r1","did":"{DID}","kind":{{"kind":"empty-commit","boundary":"3zzzzzzzzzzzz"}}}}"#
        ),
    )
    .unwrap();
    let created = binary(dir.path())
        .args(["--repair-create"])
        .arg(&spec)
        .output()
        .unwrap();
    assert_eq!(
        created.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    let repair: serde_json::Value = serde_json::from_slice(&created.stdout).unwrap();
    assert_eq!(repair["state"], "pending");
    let status = binary(dir.path())
        .args(["--repair-status", "r1"])
        .output()
        .unwrap();
    assert_eq!(status.status.code(), Some(0));
    let missing = binary(dir.path())
        .args(["--repair-status", "nope"])
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(1));
    let refused = binary(dir.path())
        .args(["--repair-run", "r1", "--timeout-secs", "1"])
        .output()
        .unwrap();
    assert_eq!(refused.status.code(), Some(2), "not fenced for maintenance");
    let unknown_run = binary(dir.path())
        .args(["--repair-run", "nope"])
        .output()
        .unwrap();
    assert_eq!(unknown_run.status.code(), Some(2));
    let bad_file = binary(dir.path())
        .args(["--repair-create", "/nonexistent/repair.json"])
        .output()
        .unwrap();
    assert_eq!(bad_file.status.code(), Some(2));

    let opened = binary(dir.path())
        .args([
            "--quarantine-open",
            "999",
            "--did",
            DID,
            "--kind",
            "bad-event",
            "--repair",
            "r1",
        ])
        .output()
        .unwrap();
    assert_eq!(
        opened.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&opened.stderr)
    );
    let quarantine: serde_json::Value = serde_json::from_slice(&opened.stdout).unwrap();
    assert_eq!(quarantine["state"], "opened");
    let reconciled = binary(dir.path())
        .args(["--quarantine-local-reconciled", "999"])
        .output()
        .unwrap();
    assert_eq!(reconciled.status.code(), Some(0));
    let blocked_close = binary(dir.path())
        .args(["--quarantine-close", "999", "--external", "verified"])
        .output()
        .unwrap();
    assert_eq!(
        blocked_close.status.code(),
        Some(2),
        "the linked repair is pending"
    );
    let unknown_close = binary(dir.path())
        .args(["--quarantine-close", "998", "--external", "verified"])
        .output()
        .unwrap();
    assert_eq!(unknown_close.status.code(), Some(2));
    let status = binary(dir.path())
        .args(["--quarantine-status", "999"])
        .output()
        .unwrap();
    assert_eq!(status.status.code(), Some(0));
    let missing = binary(dir.path())
        .args(["--quarantine-status", "998"])
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(1));

    // a lock another process holds exclusively makes the drain give up
    let locks = rsky_pds::locks::LockDir::new(dir.path().join("rsky/locks")).unwrap();
    let held = locks.try_exclusive(DID).unwrap().unwrap();
    let blocked = binary(dir.path())
        .args(["--drain-did", DID, "--timeout-secs", "0"])
        .output()
        .unwrap();
    assert_eq!(blocked.status.code(), Some(2));
    drop(held);
}
