//! A stop signal drains the process: it stops accepting, lets in-flight
//! work finish, and exits cleanly with the reference log shape.

mod common;

use common::pds_binary;
use std::io::Read;
use std::net::TcpListener;
use std::process::Stdio;
use std::time::{Duration, Instant};

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

async fn wait_for(url: &str, deadline: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < deadline {
        if reqwest::get(url).await.is_ok_and(|res| res.status() == 200) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

#[tokio::test]
async fn sigterm_drains_and_exits_with_json_logs() {
    let dir = tempfile::tempdir().unwrap();
    let port = free_port();
    let mut child = pds_binary(dir.path())
        .env("PDS_PORT", port.to_string())
        .env("PDS_SHUTDOWN_GRACE_SECS", "5")
        .env("RUST_LOG", "info")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let base = format!("http://127.0.0.1:{port}");
    assert!(
        wait_for(
            &format!("{base}/xrpc/_health/live"),
            Duration::from_secs(30)
        )
        .await,
        "server did not come up"
    );
    let health = reqwest::get(format!("{base}/xrpc/_health")).await.unwrap();
    assert_eq!(health.status(), 200);

    // SAFETY: the pid names the child spawned above, which is still ours
    assert_eq!(unsafe { libc::kill(child.id() as i32, libc::SIGTERM) }, 0);
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "process did not exit after SIGTERM"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert!(status.success(), "{status:?}");
    let mut stdout = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut stdout)
        .unwrap();
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    let logs = format!("{stdout}{stderr}");
    let lines: Vec<serde_json::Value> = logs
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    let messages: Vec<&str> = lines
        .iter()
        .filter_map(|line| line["msg"].as_str())
        .collect();
    assert!(messages.contains(&"shutdown requested; draining"), "{logs}");
    assert!(messages.contains(&"drained; exiting"), "{logs}");
    let request_line = lines
        .iter()
        .find(|line| line["msg"] == "request completed" && line["req"]["url"] == "/xrpc/_health")
        .unwrap_or_else(|| panic!("no request log line in:\n{logs}"));
    assert_eq!(request_line["res"]["statusCode"], 200);
    assert_eq!(request_line["name"], "pds");
    assert_eq!(request_line["level"], 30);
    assert!(request_line["responseTime"].is_number());
}
