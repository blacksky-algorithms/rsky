//! The `rsky-pds` binary's maintenance mode, driven as an operator would.

mod common;

use common::pds_binary as binary;

const DID: &str = "did:plc:cli";

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

    // an account this server never wrote does not converge
    let diverged = binary(dir.path())
        .args(["--converge", DID])
        .output()
        .unwrap();
    assert_eq!(
        diverged.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&diverged.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&diverged.stdout).unwrap();
    assert_eq!(report["converged"], false);

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
