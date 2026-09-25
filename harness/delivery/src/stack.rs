//! One side's stack: platform, worker (dispatch scheduler), router and
//! outbox processor, as separate OS processes in the production topology
//! (inhance `iac/compute/flowcatalyst.ts` + `fc-router.ts`): the platform
//! task runs the API and the stream processor with the scheduler off, a
//! worker task runs the scheduler, the router is its own service.

use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use anyhow::{bail, Context};
use serde::Serialize;
use tokio::process::{Child, Command};

use crate::infra::free_port;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum SideKind {
    Go,
    Rust,
}

impl SideKind {
    pub fn label(self) -> &'static str {
        match self {
            SideKind::Go => "go",
            SideKind::Rust => "rust",
        }
    }
    /// Queue prefix (`FC_DISPATCH_QUEUE_PREFIX`); distinct per side so the
    /// two stacks share one SQS emulator without sharing a queue.
    pub fn queue_prefix(self) -> &'static str {
        match self {
            SideKind::Go => "FC-go",
            SideKind::Rust => "FC-rs",
        }
    }
}

/// Which binary a process runs and how it is told its role.
#[derive(Debug, Clone)]
pub struct ProcSpec {
    pub name: &'static str,
    pub program: PathBuf,
    pub env: BTreeMap<String, String>,
    /// URLs any one of which answering 2xx means "up".
    pub health: Vec<String>,
}

pub struct Proc {
    pub spec: ProcSpec,
    pub log: PathBuf,
    child: Option<Child>,
    pub starts: u32,
}

impl Proc {
    pub fn new(spec: ProcSpec, log_dir: &Path) -> Proc {
        let log = log_dir.join(format!("{}.log", spec.name));
        Proc {
            spec,
            log,
            child: None,
            starts: 0,
        }
    }

    pub fn start(&mut self, cwd: &Path) -> anyhow::Result<()> {
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log)?;
        use std::io::Write;
        writeln!(
            f,
            "\n===== harness: start #{} of {} ({}) =====",
            self.starts + 1,
            self.spec.name,
            self.spec.program.display()
        )?;
        let err = f.try_clone()?;
        let mut cmd = Command::new(&self.spec.program);
        // A clean environment: nothing from the caller's shell (FC_*,
        // AWS_*, DATABASE_URL …) can leak into either side.
        cmd.env_clear();
        for k in ["PATH", "HOME", "TMPDIR", "USER", "LANG"] {
            if let Ok(v) = std::env::var(k) {
                cmd.env(k, v);
            }
        }
        cmd.envs(&self.spec.env)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::from(f))
            .stderr(Stdio::from(err))
            .kill_on_drop(true);
        let child = cmd
            .spawn()
            .with_context(|| format!("spawn {}", self.spec.program.display()))?;
        self.child = Some(child);
        self.starts += 1;
        Ok(())
    }

    pub fn is_running(&mut self) -> bool {
        match self.child.as_mut() {
            Some(c) => matches!(c.try_wait(), Ok(None)),
            None => false,
        }
    }

    /// SIGTERM, wait up to `grace`, then SIGKILL.
    pub async fn stop(&mut self, grace: Duration) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        if let Some(pid) = child.id() {
            let _ = std::process::Command::new("kill")
                .args(["-TERM", &pid.to_string()])
                .status();
        }
        if tokio::time::timeout(grace, child.wait()).await.is_err() {
            let _ = child.kill().await;
        }
    }

    pub async fn kill(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill().await;
        }
    }

    /// Wait until a health URL answers 2xx, or the process exits.
    pub async fn wait_healthy(&mut self, timeout: Duration) -> anyhow::Result<()> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()?;
        let deadline = Instant::now() + timeout;
        loop {
            if !self.is_running() {
                bail!(
                    "{} exited during start-up (see {})",
                    self.spec.name,
                    self.log.display()
                );
            }
            for url in &self.spec.health {
                if let Ok(r) = http.get(url).send().await {
                    if r.status().is_success() {
                        return Ok(());
                    }
                }
            }
            if Instant::now() > deadline {
                bail!(
                    "{} not healthy after {:?} (see {})",
                    self.spec.name,
                    timeout,
                    self.log.display()
                );
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
    }
}

/// Everything shared by a side's processes.
pub struct SideEnv {
    pub side: SideKind,
    pub database_url: String,
    pub sqs_endpoint: String,
    pub platform_port: u16,
    pub app_key: String,
    pub jwt_private: PathBuf,
    pub jwt_public: PathBuf,
    pub admin_email: String,
    pub admin_password: String,
}

impl SideEnv {
    pub fn platform_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.platform_port)
    }

    /// The environment every process on this side gets. Both
    /// implementations' names are set where they differ; a knob one side
    /// does not know is harmless.
    pub fn common(&self) -> BTreeMap<String, String> {
        let mut e = BTreeMap::new();
        let mut set = |k: &str, v: &str| {
            e.insert(k.to_string(), v.to_string());
        };
        let base = self.platform_url();
        let process = format!("{base}/api/dispatch/process");
        set("FC_DATABASE_URL", &self.database_url);
        set("FLOWCATALYST_APP_KEY", &self.app_key);
        // JWT: Go reads FC_JWT_SIGNING_KEY_PATH, Rust the key pair paths.
        set(
            "FC_JWT_SIGNING_KEY_PATH",
            &self.jwt_private.to_string_lossy(),
        );
        set(
            "FC_JWT_PRIVATE_KEY_PATH",
            &self.jwt_private.to_string_lossy(),
        );
        set("FC_JWT_PUBLIC_KEY_PATH", &self.jwt_public.to_string_lossy());
        set("FC_JWT_ISSUER", &base);
        set("FC_EXTERNAL_BASE_URL", &base);
        set("FC_JWT_ACCESS_TOKEN_TTL_SECS", "86400");
        set("FC_ACCESS_TOKEN_EXPIRY_SECS", "86400");
        set("FLOWCATALYST_BOOTSTRAP_ADMIN_EMAIL", &self.admin_email);
        set(
            "FLOWCATALYST_BOOTSTRAP_ADMIN_PASSWORD",
            &self.admin_password,
        );
        set("FLOWCATALYST_BOOTSTRAP_ADMIN_NAME", "Harness Admin");
        // SQS via LocalStack. Queue URIs stay in the amazonaws form both
        // implementations require (LocalStack resolves them by path).
        set("AWS_ACCESS_KEY_ID", "test");
        set("AWS_SECRET_ACCESS_KEY", "test");
        set("AWS_REGION", "us-east-1");
        set("AWS_DEFAULT_REGION", "us-east-1");
        set("AWS_ENDPOINT_URL", &self.sqs_endpoint);
        set("AWS_ENDPOINT_URL_SQS", &self.sqs_endpoint);
        set("AWS_EC2_METADATA_DISABLED", "true");
        set("FC_DISPATCH_QUEUE_TYPE", "SQS");
        set("DISPATCH_QUEUE_TYPE", "SQS");
        let prefix = self.side.queue_prefix();
        set(
            "FC_DISPATCH_QUEUE_URL",
            &format!("https://sqs.us-east-1.amazonaws.com/000000000000/{prefix}-dispatch.fifo"),
        );
        set("FC_DISPATCH_QUEUE_REGION", "us-east-1");
        set("FC_DISPATCH_QUEUE_PREFIX", prefix);
        // The router POSTs {messageId} here (Go and Rust names).
        set("FC_DISPATCH_PROCESSING_ENDPOINT", &process);
        set("DISPATCH_SCHEDULER_PROCESSING_ENDPOINT", &process);
        set("FC_SCHEDULER_PROCESSING_ENDPOINT", &process);
        // Fan-out picks up new subscriptions within a second.
        set("FC_STREAM_FAN_OUT_SUBS_REFRESH_SECS", "1");
        set("FC_DRAIN_TIMEOUT_SECONDS", "10");
        set("RUST_LOG", "info");
        e
    }
}

pub fn ports() -> (u16, u16) {
    (free_port(), free_port())
}
