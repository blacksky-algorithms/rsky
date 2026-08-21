//! Child-process supervision: start a server, wait for it to answer, and make
//! sure it is gone when the gate finishes however the gate finishes.

use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub struct Server {
    pub name: &'static str,
    child: Child,
    pub log: std::path::PathBuf,
}

impl Server {
    pub fn spawn(
        name: &'static str,
        binary: &Path,
        cwd: &Path,
        env: &[(String, String)],
        log: &Path,
    ) -> Result<Self> {
        let file =
            std::fs::File::create(log).with_context(|| format!("create {}", log.display()))?;
        let errors = file.try_clone()?;
        let mut command = Command::new(binary);
        command
            .current_dir(cwd)
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", std::env::var("HOME").unwrap_or_default())
            .stdin(Stdio::null())
            .stdout(Stdio::from(file))
            .stderr(Stdio::from(errors));
        for (key, value) in env {
            command.env(key, value);
        }
        let child = command
            .spawn()
            .with_context(|| format!("spawn {}", binary.display()))?;
        Ok(Self {
            name,
            child,
            log: log.to_path_buf(),
        })
    }

    /// Poll `url` until it answers or the deadline passes; a child that has
    /// already exited fails immediately with its log.
    pub async fn wait_ready(&mut self, url: &str, timeout: Duration) -> Result<()> {
        let client = crate::layer2::http_client()?;
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.child.try_wait()? {
                bail!(
                    "{} exited early ({status}); log:\n{}",
                    self.name,
                    self.tail()
                );
            }
            if let Ok(response) = client.get(url).send().await {
                if response.status().is_success() {
                    return Ok(());
                }
            }
            if Instant::now() >= deadline {
                bail!(
                    "{} did not become ready at {url}; log:\n{}",
                    self.name,
                    self.tail()
                );
            }
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
    }

    pub fn tail(&self) -> String {
        let text = std::fs::read_to_string(&self.log).unwrap_or_default();
        text.lines()
            .rev()
            .take(30)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn free_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

pub fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Replace every copied per-account store with an empty one created by the
/// space host's own schema code, keeping the signing keys beside it. The space
/// host requires the file to be present at startup, so it cannot simply be
/// deleted and left to appear on first write.
pub fn reset_stores(root: &Path) -> Result<usize> {
    if !root.exists() {
        return Ok(0);
    }
    let mut created = 0;
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        if entry.file_name().to_string_lossy().starts_with("did:") {
            let account = entry.path();
            for stale in std::fs::read_dir(&account)? {
                let stale = stale?;
                if stale
                    .file_name()
                    .to_string_lossy()
                    .starts_with("store.sqlite")
                {
                    std::fs::remove_file(stale.path())?;
                }
            }
            rsky_space_host::actor_schema::get_migrated_db(account.join("store.sqlite"))
                .map_err(|error| anyhow::anyhow!("migrate shim store: {error}"))?;
            created += 1;
        } else {
            created += reset_stores(&entry.path())?;
        }
    }
    Ok(created)
}
