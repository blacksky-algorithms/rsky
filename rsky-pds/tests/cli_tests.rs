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
    let status: serde_json::Value = serde_json::from_slice(&drained.stdout)
        .expect("the drain prints only its status on stdout; logs go to stderr");
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

    // the batch form reports only the accounts that diverge
    let list = dir.path().join("dids.txt");
    std::fs::write(&list, format!("{DID}\n\n  \n")).unwrap();
    let batch = binary(dir.path())
        .args(["--converge-file", list.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(batch.status.code(), Some(1));
    let summary: serde_json::Value = serde_json::from_slice(&batch.stdout).unwrap();
    assert_eq!(summary["checked"], 1);
    assert_eq!(summary["converged"], 0);
    assert_eq!(summary["diverged"][0]["did"], DID);
    std::fs::write(&list, "").unwrap();
    let empty = binary(dir.path())
        .args(["--converge-file", list.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(empty.status.code(), Some(0));
    let unreadable = binary(dir.path())
        .args([
            "--converge-file",
            dir.path().join("missing.txt").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(unreadable.status.code(), Some(2));

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

/// Waits for the process to exit, killing it if it outlives the budget.
fn wait_bounded(mut child: std::process::Child) -> std::process::Output {
    let started = std::time::Instant::now();
    while started.elapsed() < std::time::Duration::from_secs(60) {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    child.kill().unwrap();
    panic!("the server did not exit within the budget");
}

#[tokio::test]
async fn collect_commands_report_and_exit_by_outcome() {
    let dir = tempfile::tempdir().unwrap();

    // the collector refuses to run without its registry
    let missing = binary(dir.path())
        .args(["--collect", DID])
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(2));
    rsky_pds::blob_generations::Generations::open(dir.path().join("rsky/blob-generations.sqlite"))
        .await
        .unwrap();
    // and while another implementation shares the store
    let shared = binary(dir.path())
        .env("PDS_COEXISTENCE", "true")
        .args(["--collect-all"])
        .output()
        .unwrap();
    assert_eq!(shared.status.code(), Some(2));
    let booted = binary(dir.path())
        .env("PDS_COEXISTENCE", "true")
        .env("PDS_BLOB_GC_ENABLED", "true")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let refused = wait_bounded(booted);
    assert_ne!(refused.status.code(), Some(0));
    assert!(
        String::from_utf8_lossy(&refused.stderr)
            .contains("PDS_BLOB_GC_ENABLED is set while PDS_COEXISTENCE is true"),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );

    // an actor this server never wrote owes nothing
    let clean = binary(dir.path())
        .args(["--collect", DID])
        .output()
        .unwrap();
    assert_eq!(
        clean.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&clean.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&clean.stdout).unwrap();
    assert_eq!(report["did"], DID);
    assert_eq!(report["checked"], 0);
    assert!(report["purge"].is_null());

    // a deleted account whose namespace has an unconfirmed write stays open
    let lifecycle =
        rsky_pds::lifecycle::LifecycleStore::open(dir.path().join("rsky/lifecycle.sqlite"))
            .await
            .unwrap();
    lifecycle
        .record_purge_obligation(&rsky_pds::lifecycle::PurgeObligation {
            did: DID.to_owned(),
            requested_at: rsky_common::now(),
            namespace_prefixes: vec![],
            manifest: serde_json::json!({}),
        })
        .await
        .unwrap();
    drop(lifecycle);
    let attempts = rsky_pds::blob_attempts::AttemptJournal::open(
        dir.path().join("rsky/blob-attempts.sqlite"),
        false,
    )
    .await
    .unwrap();
    let pending = attempts.begin(DID, "blocks/x", "put").await.unwrap();
    let open = binary(dir.path())
        .args(["--collect", DID])
        .output()
        .unwrap();
    assert_eq!(open.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&open.stdout).unwrap();
    assert_eq!(report["purge"]["outcome"], "open");
    let all = binary(dir.path()).args(["--collect-all"]).output().unwrap();
    assert_eq!(all.status.code(), Some(1));
    let reports: serde_json::Value = serde_json::from_slice(&all.stdout).unwrap();
    assert_eq!(reports.as_array().unwrap().len(), 1);
    assert_eq!(reports[0]["purge"]["outcome"], "open");

    // with the write confirmed and no other writer, the purge is proven
    attempts
        .resolve(pending, rsky_pds::blob_attempts::AttemptOutcome::Succeeded)
        .await
        .unwrap();
    drop(attempts);
    let purged = binary(dir.path()).args(["--collect-all"]).output().unwrap();
    assert_eq!(
        purged.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&purged.stderr)
    );
    let reports: serde_json::Value = serde_json::from_slice(&purged.stdout).unwrap();
    assert_eq!(reports[0]["purge"]["outcome"], "verified-purged");
    let done = binary(dir.path())
        .args(["--collect", DID])
        .output()
        .unwrap();
    assert_eq!(done.status.code(), Some(0));
    let report: serde_json::Value = serde_json::from_slice(&done.stdout).unwrap();
    assert!(report["purge"].is_null());
}
