//! An `fc-server` subprocess (Go's or Rust's): output to `<label>.log`,
//! readiness = `GET /health` answering 200, stop = SIGTERM then kill.
//!
//! Each side gets a freshly picked `FC_API_PORT`, an ephemeral
//! `FC_METRICS_PORT`, and `FC_JWT_ISSUER` / `FC_EXTERNAL_BASE_URL` /
//! `FC_WEBAUTHN_ORIGINS` set to its own `http://localhost:<port>` (the base
//! URL the normaliser masks as `«base»`). `localhost`, not `127.0.0.1`:
//! Rust's webauthn-rs refuses an IP as the RP id, and both sides must share
//! one RP id (`FC_WEBAUTHN_RP_ID=localhost`) for ceremony options to compare.

use anyhow::{bail, Context, Result};
use indexmap::IndexMap;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::process::{Child, Command};

const HEALTH_BUDGET: Duration = Duration::from_secs(120);
const STOP_GRACE: Duration = Duration::from_secs(15);

pub struct SubprocessSide {
    pub label: String,
    pub base_url: String,
    child: Child,
    pub start_duration: Duration,
}

pub fn free_port() -> Result<u16> {
    let l = std::net::TcpListener::bind("127.0.0.1:0").context("pick a free port")?;
    Ok(l.local_addr()?.port())
}

impl SubprocessSide {
    pub async fn start(
        label: &str,
        binary: &Path,
        env: &IndexMap<String, String>,
        log_file: &Path,
    ) -> Result<Self> {
        let port = free_port()?;
        let base_url = format!("http://localhost:{port}");
        let log = std::fs::File::create(log_file)
            .with_context(|| format!("create {}", log_file.display()))?;
        let mut cmd = Command::new(binary);
        cmd.envs(env)
            .env("FC_API_PORT", port.to_string())
            .env("FC_METRICS_PORT", "0")
            .env("FC_JWT_ISSUER", &base_url)
            .env("FC_EXTERNAL_BASE_URL", &base_url)
            .env("FC_WEBAUTHN_ORIGINS", &base_url)
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .kill_on_drop(true);
        let t0 = Instant::now();
        let mut child = cmd
            .spawn()
            .with_context(|| format!("start {}", binary.display()))?;

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .no_proxy()
            .resolve("localhost", ([127, 0, 0, 1], 0).into())
            .build()?;
        loop {
            if let Some(status) = child.try_wait()? {
                bail!(
                    "{label} fc-server exited ({status}) before becoming healthy; see {}\n{}",
                    log_file.display(),
                    tail(log_file)
                );
            }
            if let Ok(r) = client.get(format!("{base_url}/health")).send().await {
                if r.status().as_u16() == 200 {
                    break;
                }
            }
            if t0.elapsed() > HEALTH_BUDGET {
                let _ = child.kill().await;
                bail!(
                    "{label} fc-server did not answer GET {base_url}/health with 200 within {HEALTH_BUDGET:?}; see {}\n{}",
                    log_file.display(),
                    tail(log_file)
                );
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        let start_duration = t0.elapsed();
        tracing::info!(%label, %base_url, ?start_duration, "fc-server healthy");
        Ok(Self {
            label: label.to_string(),
            base_url,
            child,
            start_duration,
        })
    }

    /// SIGTERM, then kill after [`STOP_GRACE`]. Idempotent.
    pub async fn stop(&mut self) {
        if matches!(self.child.try_wait(), Ok(Some(_))) {
            return;
        }
        if let Some(pid) = self.child.id() {
            let _ = std::process::Command::new("kill")
                .args(["-TERM", &pid.to_string()])
                .status();
        }
        if tokio::time::timeout(STOP_GRACE, self.child.wait())
            .await
            .is_err()
        {
            tracing::warn!(label = %self.label, "did not exit within {STOP_GRACE:?} of SIGTERM; killing");
            let _ = self.child.kill().await;
        }
    }
}

fn tail(log_file: &Path) -> String {
    let text = std::fs::read_to_string(log_file).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(30)..].join("\n")
}
