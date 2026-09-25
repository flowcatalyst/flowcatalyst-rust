//! One side (Go or Rust): boot, provision, drive a scenario, collect.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context};
use reqwest::Method;
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tokio::sync::Mutex;

use crate::api::{truncate, Api, ServiceAccount};
use crate::infra::{free_port, Infra};
use crate::receiver::{Delivery, Receiver};
use crate::scenario::{Scenario, Stimulus};
use crate::stack::{Proc, ProcSpec, SideEnv, SideKind};

pub const ADMIN_EMAIL: &str = "harness-admin@harness.test";
/// No identity words (Go refuses PASSWORD_CONTAINS_IDENTITY).
pub const ADMIN_PASSWORD: &str = "Qv7!mZr2#Lp9xW4t-Kd3";

/// Where a side's binaries come from.
#[derive(Debug, Clone)]
pub struct Binaries {
    pub platform: PathBuf,
    pub worker: PathBuf,
    pub router: PathBuf,
    pub outbox: PathBuf,
}

/// One stimulus as sent: the identity the comparison works in.
#[derive(Debug, Clone, Serialize)]
pub struct Sent {
    pub hk: String,
    pub target: String,
    pub group: Option<String>,
    pub seq: i64,
    pub kind: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct JobRow {
    pub status: String,
    pub attempt_count: i32,
    pub message_group: Option<String>,
}

/// What one side produced for one scenario.
#[derive(Debug, Clone, Serialize)]
pub struct SideRun {
    pub side: SideKind,
    pub sent: Vec<Sent>,
    /// Non-2xx ingest answers / per-item rejections, verbatim.
    pub ingest_errors: Vec<String>,
    pub deliveries: Vec<Delivery>,
    pub jobs: Vec<JobRow>,
    /// Outbox rows of this scenario still in the table, by status.
    pub outbox_left: BTreeMap<i16, i64>,
    pub settled: bool,
    pub settle_ms: u64,
    pub disruptions: Vec<String>,
    /// Events of this scenario in `msg_events`: (stored, fanned out).
    pub events: Option<(i64, i64)>,
    /// The side's platform-tenant DEFAULT queue after settling: (visible,
    /// in flight).
    pub queue_depth: Option<(u64, u64)>,
    /// Set when the side could not run the scenario at all.
    pub error: Option<String>,
}

struct TargetSetup {
    endpoint: String,
    event_type: String,
    mode: String,
    pool_id: Option<String>,
    max_retries: Option<u32>,
    timeout_seconds: Option<u32>,
}

pub struct Side {
    pub kind: SideKind,
    pub dir: PathBuf,
    pub env: SideEnv,
    pub bins: Binaries,
    procs: Arc<Mutex<BTreeMap<&'static str, Proc>>>,
    pub api: Option<Api>,
    pub db: Option<PgPool>,
    pub receiver: Receiver,
    pub receiver_base: String,
    pub signer: Option<ServiceAccount>,
    /// (scenario, target) → what was provisioned.
    targets: HashMap<(String, String), TargetSetup>,
    pools: Vec<(String, u32, Option<u32>)>,
    refused_port: u16,
    pub notes: Vec<String>,
    pub boot_error: Option<String>,
    infra_sqs_container: String,
    /// Before booting, run this Go `fc-server` (platform only) against the
    /// side's database once, so the side adopts a Go-migrated, Go-seeded
    /// database — the production cutover path.
    pub adopt_go_schema: Option<PathBuf>,
}

impl Side {
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        kind: SideKind,
        dir: PathBuf,
        infra: &Infra,
        bins: Binaries,
        app_key: &str,
        jwt_private: PathBuf,
        jwt_public: PathBuf,
    ) -> anyhow::Result<Side> {
        std::fs::create_dir_all(&dir)?;
        let db_name = format!("fc_{}", kind.label());
        infra.create_database(&db_name)?;
        let (receiver, addr) = Receiver::start().await?;
        let env = SideEnv {
            side: kind,
            database_url: infra.database_url(&db_name),
            sqs_endpoint: infra.sqs_endpoint(),
            platform_port: free_port(),
            app_key: app_key.to_string(),
            jwt_private,
            jwt_public,
            admin_email: ADMIN_EMAIL.to_string(),
            admin_password: ADMIN_PASSWORD.to_string(),
        };
        Ok(Side {
            kind,
            dir,
            env,
            bins,
            procs: Arc::new(Mutex::new(BTreeMap::new())),
            api: None,
            db: None,
            receiver,
            receiver_base: format!("http://{addr}"),
            signer: None,
            targets: HashMap::new(),
            pools: Vec::new(),
            refused_port: free_port(),
            notes: Vec::new(),
            boot_error: None,
            adopt_go_schema: None,
            infra_sqs_container: infra.sqs_container.clone(),
        })
    }

    fn note(&mut self, s: impl Into<String>) {
        let s = s.into();
        eprintln!("[{}] {s}", self.kind.label());
        self.notes.push(s);
    }

    fn platform_spec(&self) -> ProcSpec {
        let mut env = self.env.common();
        let metrics = free_port();
        let p = self.env.platform_port.to_string();
        env.insert("FC_API_PORT".into(), p.clone());
        env.insert("PORT".into(), p);
        env.insert("FC_METRICS_PORT".into(), metrics.to_string());
        for (k, v) in [
            ("FC_PLATFORM_ENABLED", "true"),
            ("FC_STREAM_PROCESSOR_ENABLED", "true"),
            ("FC_SCHEDULER_ENABLED", "false"),
            ("DISPATCH_SCHEDULER_ENABLED", "false"),
            ("FC_ROUTER_ENABLED", "false"),
            ("MESSAGE_ROUTER_ENABLED", "false"),
            ("FC_OUTBOX_ENABLED", "false"),
            ("FC_SCHEDULED_JOB_ENABLED", "false"),
            ("FC_STANDBY_ENABLED", "false"),
        ] {
            env.insert(k.into(), v.into());
        }
        ProcSpec {
            name: "platform",
            program: self.bins.platform.clone(),
            env,
            health: vec![format!("{}/health", self.env.platform_url())],
        }
    }

    fn worker_spec(&self) -> ProcSpec {
        let mut env = self.env.common();
        let (api, metrics) = (free_port(), free_port());
        env.insert("FC_API_PORT".into(), api.to_string());
        env.insert("PORT".into(), api.to_string());
        env.insert("FC_METRICS_PORT".into(), metrics.to_string());
        for (k, v) in [
            ("FC_PLATFORM_ENABLED", "false"),
            ("PLATFORM_ENABLED", "false"),
            ("FC_STREAM_PROCESSOR_ENABLED", "false"),
            ("FC_SCHEDULER_ENABLED", "true"),
            ("DISPATCH_SCHEDULER_ENABLED", "true"),
            ("FC_ROUTER_ENABLED", "false"),
            ("FC_OUTBOX_ENABLED", "false"),
            ("FC_SCHEDULED_JOB_ENABLED", "false"),
            ("FC_STANDBY_ENABLED", "false"),
        ] {
            env.insert(k.into(), v.into());
        }
        ProcSpec {
            name: "worker",
            program: self.bins.worker.clone(),
            env,
            health: health_urls(api, metrics),
        }
    }

    fn router_spec(&self, config_url: &str, creds: Option<(&str, &str)>) -> ProcSpec {
        let mut env = BTreeMap::new();
        let common = self.env.common();
        for k in [
            "AWS_ACCESS_KEY_ID",
            "AWS_SECRET_ACCESS_KEY",
            "AWS_REGION",
            "AWS_DEFAULT_REGION",
            "AWS_ENDPOINT_URL",
            "AWS_ENDPOINT_URL_SQS",
            "AWS_EC2_METADATA_DISABLED",
            "FC_DRAIN_TIMEOUT_SECONDS",
            "RUST_LOG",
        ] {
            env.insert(k.to_string(), common[k].clone());
        }
        let (api, metrics) = (free_port(), free_port());
        let platform = self.env.platform_url();
        for (k, v) in [
            ("FC_API_PORT", api.to_string()),
            ("API_PORT", api.to_string()),
            ("PORT", api.to_string()),
            ("FC_METRICS_PORT", metrics.to_string()),
            ("FC_PLATFORM_ENABLED", "false".into()),
            ("PLATFORM_ENABLED", "false".into()),
            ("FC_ROUTER_ENABLED", "true".into()),
            ("MESSAGE_ROUTER_ENABLED", "true".into()),
            ("FLOWCATALYST_CONFIG_URL", config_url.to_string()),
            ("FLOWCATALYST_CONFIG_INTERVAL", "5".into()),
            ("FC_ROUTER_CONFIG_INTERVAL_SECONDS", "5".into()),
            ("FC_STANDBY_ENABLED", "false".into()),
            ("FLOWCATALYST_STANDBY_ENABLED", "false".into()),
            ("AUTH_MODE", "NONE".into()),
        ] {
            env.insert(k.to_string(), v);
        }
        if let Some((id, secret)) = creds {
            env.insert("FC_ROUTER_PLATFORM_URL".into(), platform);
            env.insert("FC_ROUTER_CLIENT_ID".into(), id.into());
            env.insert("FC_ROUTER_CLIENT_SECRET".into(), secret.into());
        }
        if self.kind == SideKind::Go {
            // Go's mediator speaks h2c with prior knowledge to any http://
            // target and does not fall back; the platform listener is plain
            // HTTP/1.1. Dev mode switches the mediator to HTTP/1.1 and
            // changes nothing else (Go mediator.go:81-88). Rust's router
            // dev mode swaps in a built-in config instead, so it is not set
            // there.
            env.insert("FLOWCATALYST_DEV_MODE".into(), "true".into());
        }
        ProcSpec {
            name: "router",
            program: self.bins.router.clone(),
            env,
            health: health_urls(api, metrics),
        }
    }

    fn outbox_spec(&self, token: &str) -> ProcSpec {
        let mut env = self.env.common();
        let (api, metrics) = (free_port(), free_port());
        let platform = self.env.platform_url();
        for (k, v) in [
            ("FC_API_PORT", api.to_string()),
            ("PORT", api.to_string()),
            ("FC_METRICS_PORT", metrics.to_string()),
            ("FC_PLATFORM_ENABLED", "false".into()),
            ("PLATFORM_ENABLED", "false".into()),
            ("FC_STREAM_PROCESSOR_ENABLED", "false".into()),
            ("FC_SCHEDULER_ENABLED", "false".into()),
            ("FC_ROUTER_ENABLED", "false".into()),
            ("FC_OUTBOX_ENABLED", "true".into()),
            ("OUTBOX_PROCESSOR_ENABLED", "true".into()),
            // Go names
            ("FC_OUTBOX_PLATFORM_URL", platform.clone()),
            ("FC_OUTBOX_PLATFORM_AUTH_TOKEN", token.to_string()),
            // Rust fc-outbox-processor names
            ("FC_OUTBOX_DB_URL", self.env.database_url.clone()),
            ("FC_API_BASE_URL", platform),
            ("FC_API_TOKEN", token.to_string()),
            ("FC_OUTBOX_POLL_INTERVAL_MS", "250".into()),
        ] {
            env.insert(k.to_string(), v);
        }
        ProcSpec {
            name: "outbox",
            program: self.bins.outbox.clone(),
            env,
            health: health_urls(api, metrics),
        }
    }

    async fn start_proc(
        &self,
        spec: ProcSpec,
        timeout: Duration,
        hard: bool,
    ) -> anyhow::Result<()> {
        let name = spec.name;
        let mut proc = Proc::new(spec, &self.dir);
        proc.start(&self.dir)?;
        let res = proc.wait_healthy(timeout).await;
        self.procs.lock().await.insert(name, proc);
        match res {
            Ok(()) => Ok(()),
            Err(e) if hard => Err(e),
            Err(e) => {
                // Up but without a health endpoint we know: carry on, the
                // scenarios will show whether it works.
                eprintln!("[{}] {name}: {e} — continuing", self.kind.label());
                Ok(())
            }
        }
    }

    /// Boot the platform, provision, then start worker, router, outbox.
    pub async fn boot(&mut self, scenarios: &[Scenario]) {
        if let Err(e) = self.boot_inner(scenarios).await {
            let msg = format!("{e:#}");
            self.note(format!("BOOT FAILED: {msg}"));
            self.boot_error = Some(msg);
        }
    }

    async fn boot_inner(&mut self, scenarios: &[Scenario]) -> anyhow::Result<()> {
        let t0 = Instant::now();
        if let Some(go) = self.adopt_go_schema.clone() {
            let mut spec = self.platform_spec();
            spec.name = "go-schema";
            spec.program = go;
            self.start_proc(spec, Duration::from_secs(120), true)
                .await
                .context("Go schema bootstrap")?;
            if let Some(p) = self.procs.lock().await.get_mut("go-schema") {
                p.stop(Duration::from_secs(15)).await;
            }
            self.procs.lock().await.remove("go-schema");
            self.note(format!(
                "database migrated and seeded by Go fc-server first ({:?}); this side adopts it (the cutover path)",
                t0.elapsed()
            ));
        }
        self.start_proc(self.platform_spec(), Duration::from_secs(300), true)
            .await
            .context("platform start")?;
        self.note(format!("platform healthy after {:?}", t0.elapsed()));

        self.db = Some(
            PgPoolOptions::new()
                .max_connections(4)
                .connect(&self.env.database_url)
                .await?,
        );

        let anon = Api::new(&self.env.platform_url());
        let admin = anon
            .login(&self.env.admin_email, &self.env.admin_password)
            .await
            .context("admin login")?;

        // API caller: a service account with platform:super-admin, bearer.
        let api = match self.provision_api_caller(&admin).await {
            Ok(api) => api,
            Err(e) => {
                self.note(format!(
                    "service-account bearer for the API failed ({e:#}); using the admin session cookie"
                ));
                admin.clone()
            }
        };
        let signer = api
            .create_service_account("harness-signer", "Harness webhook signer")
            .await
            .context("signer service account")?;
        if signer.signing_secret.is_none() {
            self.note("signer service account returned no webhook signing secret");
        }
        self.signer = Some(signer);
        self.api = Some(api.clone());

        // Router credential (Go: platform:router, anchor scope).
        let router_creds = match self.provision_router(&api).await {
            Ok(c) => Some(c),
            Err(e) => {
                self.note(format!("router service account: {e:#}"));
                None
            }
        };

        // SDK outbox table (identical DDL on both sides; both processors
        // create it themselves with IF NOT EXISTS too).
        self.create_outbox_table().await?;

        for s in scenarios {
            if let Err(e) = self.provision_scenario(&api, s).await {
                self.note(format!("scenario {} setup: {e:#}", s.name));
            }
        }

        // Queue: the platform tenant's DEFAULT queue (Go creates queues
        // lazily on the publish path; the router does not create them).
        let queue = format!("{}-platform-DEFAULT.fifo", self.kind.queue_prefix());
        create_queue(&self.infra_sqs_container, &queue)?;

        // Router config: the platform's own document if it serves one,
        // else a harness shim in the same shape.
        let (config_url, creds) = self.router_config_source(&api, &queue, &router_creds).await;

        self.start_proc(self.worker_spec(), Duration::from_secs(120), false)
            .await?;
        self.start_proc(
            self.router_spec(
                &config_url,
                creds.as_ref().map(|(a, b)| (a.as_str(), b.as_str())),
            ),
            Duration::from_secs(90),
            false,
        )
        .await?;
        let token = self.outbox_token().await;
        self.start_proc(self.outbox_spec(&token), Duration::from_secs(60), false)
            .await?;
        // Fan-out subscription cache (1s) and router config fetch.
        tokio::time::sleep(Duration::from_secs(6)).await;
        let mut dead = Vec::new();
        for (name, p) in self.procs.lock().await.iter_mut() {
            if !p.is_running() {
                dead.push(format!(
                    "{name} exited during boot (see {})",
                    p.log.display()
                ));
            }
        }
        for d in dead {
            self.note(d);
        }
        self.note(format!("stack up after {:?}", t0.elapsed()));
        Ok(())
    }

    async fn provision_api_caller(&mut self, admin: &Api) -> anyhow::Result<Api> {
        let sa = admin
            .create_service_account("harness-api", "Harness API caller")
            .await?;
        admin
            .assign_roles(&sa.id, &["platform:super-admin"])
            .await?;
        let (id, secret) = (
            sa.client_id
                .clone()
                .ok_or_else(|| anyhow!("no oauth client id"))?,
            sa.client_secret
                .clone()
                .ok_or_else(|| anyhow!("no oauth client secret"))?,
        );
        let token = admin.client_credentials_token(&id, &secret).await?;
        let api = admin.with_bearer(&token);
        // Prove it works before relying on it.
        let r = api.call(Method::GET, "/api/dispatch-pools", None).await;
        if !(200..300).contains(&r.status) {
            anyhow::bail!("bearer check GET /api/dispatch-pools -> {}", r.status);
        }
        self.note("API caller: service account harness-api (platform:super-admin), client_credentials bearer");
        Ok(api)
    }

    async fn provision_router(&mut self, api: &Api) -> anyhow::Result<(String, String)> {
        let sa = api
            .create_service_account("harness-router", "Harness router")
            .await?;
        api.assign_roles(&sa.id, &["platform:router"]).await?;
        Ok((
            sa.client_id.ok_or_else(|| anyhow!("no oauth client id"))?,
            sa.client_secret
                .ok_or_else(|| anyhow!("no oauth client secret"))?,
        ))
    }

    async fn outbox_token(&mut self) -> String {
        match &self.api {
            Some(api) => match api.bearer_token() {
                Some(t) => t,
                None => {
                    self.note("outbox processor has no bearer token (API caller is cookie-only)");
                    String::new()
                }
            },
            None => String::new(),
        }
    }

    async fn router_config_source(
        &mut self,
        api: &Api,
        queue: &str,
        router_creds: &Option<(String, String)>,
    ) -> (String, Option<(String, String)>) {
        let path = "/api/dispatch/router-config";
        let r = api.call(Method::GET, path, None).await;
        let served = (200..300).contains(&r.status) && r.body.get("queues").is_some();
        if served {
            self.note(format!(
                "router config: the platform's own document ({path}); router authenticates with harness-router"
            ));
            return (
                format!("{}{path}", self.env.platform_url()),
                router_creds.clone(),
            );
        }
        // Shim in Go's document shape: pools as `platform-<code>` (Go
        // composes `{client identifier|platform}-{code}`), the platform
        // tenant's DEFAULT queue.
        let pools: Vec<Value> = self
            .pools
            .iter()
            .map(|(code, conc, rate)| {
                let mut p = json!({"code": format!("platform-{code}"), "concurrency": conc});
                if let Some(r) = rate {
                    p["rateLimitPerMinute"] = json!(r);
                }
                p
            })
            .collect();
        let doc = json!({
            "processingPools": pools,
            "queues": [{
                "queueName": queue,
                "queueUri": format!("https://sqs.us-east-1.amazonaws.com/000000000000/{queue}"),
                "connections": 1,
                "visibilityTimeout": 30,
            }],
        });
        self.receiver.set_router_config(doc);
        self.note(format!(
            "router config: HARNESS SHIM — the platform answered {} on GET {path}; the router reads a harness-served document in Go's shape instead",
            r.status
        ));
        (
            format!("{}/router-config", self.receiver_base),
            router_creds.clone(),
        )
    }

    async fn create_outbox_table(&self) -> anyhow::Result<()> {
        let db = self.db.as_ref().ok_or_else(|| anyhow!("no db"))?;
        for stmt in [
            "CREATE TABLE IF NOT EXISTS outbox_messages (
               id VARCHAR(26) PRIMARY KEY, type VARCHAR(20) NOT NULL, message_group VARCHAR(255),
               payload TEXT NOT NULL, status SMALLINT NOT NULL DEFAULT 0, retry_count SMALLINT NOT NULL DEFAULT 0,
               created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
               error_message TEXT, client_id VARCHAR(26), payload_size INTEGER, headers JSONB)",
            "CREATE INDEX IF NOT EXISTS idx_outbox_messages_pending ON outbox_messages (status, message_group, created_at) WHERE status = 0",
        ] {
            sqlx::query(stmt).execute(db).await?;
        }
        Ok(())
    }

    fn endpoint_for(&self, scenario: &str, target: &str, kind: Option<&str>) -> String {
        match kind {
            Some("refused") => format!(
                "http://127.0.0.1:{}/hook/{scenario}/{target}",
                self.refused_port
            ),
            _ => format!("{}/hook/{scenario}/{target}", self.receiver_base),
        }
    }

    async fn provision_scenario(&mut self, api: &Api, s: &Scenario) -> anyhow::Result<()> {
        let mut pool_ids = HashMap::new();
        for p in &s.pools {
            let code = pool_code(&s.name, &p.code);
            let id = api
                .create_pool(&code, p.concurrency, p.rate_limit)
                .await
                .with_context(|| format!("pool {code}"))?;
            self.pools.push((code, p.concurrency, p.rate_limit));
            pool_ids.insert(p.code.clone(), id);
        }
        let signer = self.signer.as_ref().map(|s| s.id.clone());
        for t in &s.targets {
            let event_type = format!("harness:delivery:{}:{}", s.name, t.name);
            if let Err(e) = api.create_event_type(&event_type).await {
                self.note(format!("event type {event_type}: {e:#}"));
            }
            let endpoint = self.endpoint_for(&s.name, &t.name, t.endpoint.as_deref());
            let pool_id = t.pool.as_ref().and_then(|p| pool_ids.get(p).cloned());
            let code = format!("h-{}-{}", s.name, t.name);
            let mut body = json!({
                "code": code,
                "name": code,
                "endpoint": endpoint,
                "eventTypes": [{"eventTypeCode": event_type}],
                "mode": t.dispatch_mode,
                "dataOnly": true,
            });
            if let Some(id) = &pool_id {
                body["dispatchPoolId"] = json!(id);
            }
            if let Some(id) = &signer {
                body["serviceAccountId"] = json!(id);
            }
            if let Some(n) = t.max_retries {
                body["maxRetries"] = json!(n);
            }
            if let Some(n) = t.timeout_seconds {
                body["timeoutSeconds"] = json!(n);
            }
            let uses_events = s.stimuli.iter().any(|st| {
                matches!(st, Stimulus::Events { target, .. } | Stimulus::OutboxEvents { target, .. } if target == &t.name)
            });
            if uses_events {
                api.create_subscription(body)
                    .await
                    .with_context(|| format!("subscription {code}"))?;
            }
            self.targets.insert(
                (s.name.clone(), t.name.clone()),
                TargetSetup {
                    endpoint,
                    event_type,
                    mode: t.dispatch_mode.clone(),
                    pool_id,
                    max_retries: t.max_retries,
                    timeout_seconds: t.timeout_seconds,
                },
            );
        }
        Ok(())
    }

    pub async fn proc_action(&self, process: &str, action: &str, down_ms: u64) -> String {
        let mut procs = self.procs.lock().await;
        let Some(p) = procs.get_mut(process) else {
            return format!("{process}: no such process");
        };
        let t = Instant::now();
        match action {
            "kill" => p.kill().await,
            _ => p.stop(Duration::from_secs(30)).await,
        }
        let stopped = t.elapsed();
        if action == "down" && down_ms > 0 {
            tokio::time::sleep(Duration::from_millis(down_ms)).await;
        }
        let res = p.start(&self.dir);
        let _ = p.wait_healthy(Duration::from_secs(60)).await;
        format!(
            "{action} {process}: stopped in {stopped:?}, back after {:?}{}",
            t.elapsed(),
            res.err()
                .map(|e| format!(" (start failed: {e})"))
                .unwrap_or_default()
        )
    }

    pub async fn shutdown(&self) {
        let mut procs = self.procs.lock().await;
        // Producers first, consumers last.
        for name in ["outbox", "worker", "router", "platform"] {
            if let Some(p) = procs.get_mut(name) {
                p.stop(Duration::from_secs(15)).await;
            }
        }
    }

    /// Send one scenario's stimuli (with its disruptions alongside), wait
    /// for it to settle, and collect what happened.
    pub async fn run_scenario(self: &Arc<Self>, s: &Scenario) -> SideRun {
        let mut run = SideRun {
            side: self.kind,
            sent: Vec::new(),
            ingest_errors: Vec::new(),
            deliveries: Vec::new(),
            jobs: Vec::new(),
            outbox_left: BTreeMap::new(),
            settled: false,
            settle_ms: 0,
            disruptions: Vec::new(),
            queue_depth: None,
            events: None,
            error: None,
        };
        if let Some(e) = &self.boot_error {
            run.error = Some(format!("side did not boot: {e}"));
            return run;
        }
        for t in &s.targets {
            self.receiver.install(&s.name, &t.name, t.script.clone());
        }
        self.receiver.mark_start(&s.name);
        let start = Instant::now();

        let mut disruption_tasks = Vec::new();
        for d in &s.disruptions {
            let me = Arc::clone(self);
            let d = d.clone();
            disruption_tasks.push(tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(d.at_ms)).await;
                let at = start.elapsed();
                let r = me.proc_action(&d.process, &d.action, d.down_ms).await;
                format!("at {at:?}: {r}")
            }));
        }

        let mut counter = 0usize;
        for st in &s.stimuli {
            if let Err(e) = self.send(s, st, &mut counter, &mut run).await {
                run.ingest_errors.push(format!("{e:#}"));
            }
        }

        self.settle(s, &mut run, start).await;
        for t in disruption_tasks {
            if let Ok(r) = t.await {
                run.disruptions.push(r);
            }
        }
        run.deliveries = self.receiver.deliveries(&s.name);
        run.jobs = self.jobs(&s.name).await.unwrap_or_default();
        run.outbox_left = self.outbox_left(&s.name).await.unwrap_or_default();
        run.queue_depth = self.queue_depth();
        run.events = self.events(&s.name).await.ok();
        run
    }

    async fn events(&self, scenario: &str) -> anyhow::Result<(i64, i64)> {
        let db = self.db.as_ref().ok_or_else(|| anyhow!("no db"))?;
        let row: (i64, i64) = sqlx::query_as(
            "SELECT COUNT(*), COUNT(fanned_out_at) FROM msg_events WHERE subject LIKE $1",
        )
        .bind(format!("{scenario}-%"))
        .fetch_one(db)
        .await?;
        Ok(row)
    }

    async fn send(
        &self,
        s: &Scenario,
        st: &Stimulus,
        counter: &mut usize,
        run: &mut SideRun,
    ) -> anyhow::Result<()> {
        let api = self.api.as_ref().ok_or_else(|| anyhow!("no api"))?;
        let mut make = |target: &str, count: u32, groups: &[String], kind: &'static str| {
            let mut seqs: HashMap<String, i64> = HashMap::new();
            let mut out = Vec::new();
            for i in 0..count as usize {
                let group = if groups.is_empty() {
                    None
                } else {
                    Some(groups[i % groups.len()].clone())
                };
                let seq = {
                    let n = seqs.entry(group.clone().unwrap_or_default()).or_insert(0);
                    *n += 1;
                    *n
                };
                *counter += 1;
                out.push(Sent {
                    hk: format!("{}-{:04}", s.name, *counter),
                    target: target.to_string(),
                    group,
                    seq,
                    kind,
                });
            }
            out
        };
        match st {
            Stimulus::Pause { ms } => {
                tokio::time::sleep(Duration::from_millis(*ms)).await;
            }
            Stimulus::Events {
                target,
                count,
                groups,
                batch,
            } => {
                let sent = make(target, *count, groups, "event");
                let t = self.target(s, target)?;
                for chunk in sent.chunks((*batch).max(1) as usize) {
                    let items: Vec<Value> = chunk
                        .iter()
                        .map(|m| {
                            let mut item = json!({
                                "type": t.event_type,
                                "source": "harness",
                                "subject": m.hk,
                                "data": marker(m),
                            });
                            if let Some(g) = &m.group {
                                item["messageGroup"] = json!(g);
                            }
                            item
                        })
                        .collect();
                    let r = api
                        .call(
                            Method::POST,
                            "/api/events/batch",
                            Some(json!({"items": items})),
                        )
                        .await;
                    record_batch(&mut run.ingest_errors, "events/batch", &r);
                }
                run.sent.extend(sent);
            }
            Stimulus::DispatchJobs {
                target,
                count,
                groups,
                batch,
            } => {
                let sent = make(target, *count, groups, "dispatch-job");
                let t = self.target(s, target)?;
                let signer = self.signer.as_ref().map(|s| s.id.clone());
                for chunk in sent.chunks((*batch).max(1) as usize) {
                    let items: Vec<Value> = chunk
                        .iter()
                        .map(|m| {
                            let mut item = json!({
                                "code": t.event_type,
                                "source": "harness",
                                "subject": m.hk,
                                "targetUrl": t.endpoint,
                                "payload": marker(m).to_string(),
                                "payloadContentType": "application/json",
                                "dataOnly": true,
                                "mode": t.mode,
                            });
                            if let Some(g) = &m.group {
                                item["messageGroup"] = json!(g);
                            }
                            if let Some(p) = &t.pool_id {
                                item["dispatchPoolId"] = json!(p);
                            }
                            if let Some(id) = &signer {
                                item["serviceAccountId"] = json!(id);
                            }
                            if let Some(n) = t.max_retries {
                                item["maxRetries"] = json!(n);
                            }
                            if let Some(n) = t.timeout_seconds {
                                item["timeoutSeconds"] = json!(n);
                            }
                            item
                        })
                        .collect();
                    let r = api
                        .call(
                            Method::POST,
                            "/api/dispatch-jobs/batch",
                            Some(json!({"items": items})),
                        )
                        .await;
                    record_batch(&mut run.ingest_errors, "dispatch-jobs/batch", &r);
                }
                run.sent.extend(sent);
            }
            Stimulus::OutboxEvents {
                target,
                count,
                groups,
            } => {
                let sent = make(target, *count, groups, "outbox-event");
                let t = self.target(s, target)?;
                let db = self.db.as_ref().ok_or_else(|| anyhow!("no db"))?;
                for m in &sent {
                    // 13 characters: Go's msg_events.id width.
                    let id = outbox_id();
                    let mut payload = json!({
                        "id": id,
                        "type": t.event_type,
                        "source": "harness",
                        "subject": m.hk,
                        "data": marker(m),
                    });
                    if let Some(g) = &m.group {
                        payload["messageGroup"] = json!(g);
                    }
                    let text = payload.to_string();
                    sqlx::query(
                        "INSERT INTO outbox_messages (id, type, message_group, payload, status, payload_size)
                         VALUES ($1, 'EVENT', $2, $3, 0, $4)",
                    )
                    .bind(&id)
                    .bind(&m.group)
                    .bind(&text)
                    .bind(text.len() as i32)
                    .execute(db)
                    .await?;
                }
                run.sent.extend(sent);
            }
        }
        Ok(())
    }

    fn target(&self, s: &Scenario, name: &str) -> anyhow::Result<&TargetSetup> {
        self.targets
            .get(&(s.name.clone(), name.to_string()))
            .ok_or_else(|| anyhow!("target {name} was not provisioned"))
    }

    async fn settle(&self, s: &Scenario, run: &mut SideRun, start: Instant) {
        let deadline = start + Duration::from_millis(s.settle.timeout_ms);
        let expected: std::collections::HashSet<&str> =
            run.sent.iter().map(|m| m.hk.as_str()).collect();
        loop {
            let deliveries = self.receiver.deliveries(&s.name);
            let accepted: std::collections::HashSet<&str> = deliveries
                .iter()
                .filter(|d| d.accepted)
                .filter_map(|d| d.hk.as_deref())
                .collect();
            let mut done = s.settle.all_accepted
                && !expected.is_empty()
                && expected.iter().all(|h| accepted.contains(h));
            if let Some(n) = s.settle.min_deliveries {
                done |= deliveries.len() >= n;
            }
            if s.settle.all_terminal && !done {
                if let Ok(jobs) = self.jobs(&s.name).await {
                    done = jobs.len() >= expected.len()
                        && !jobs.is_empty()
                        && jobs.iter().all(|j| is_terminal(&j.status));
                }
            }
            if done {
                run.settled = true;
                run.settle_ms = start.elapsed().as_millis() as u64;
                tokio::time::sleep(Duration::from_millis(s.settle.quiet_ms)).await;
                return;
            }
            if Instant::now() > deadline {
                run.settle_ms = start.elapsed().as_millis() as u64;
                return;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    pub async fn jobs(&self, scenario: &str) -> anyhow::Result<Vec<JobRow>> {
        let db = self.db.as_ref().ok_or_else(|| anyhow!("no db"))?;
        let rows: Vec<(String, i32, Option<String>)> = sqlx::query_as(
            "SELECT status::text, attempt_count, message_group FROM msg_dispatch_jobs
             WHERE target_url LIKE $1 ORDER BY created_at, id",
        )
        .bind(format!("%/hook/{scenario}/%"))
        .fetch_all(db)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(status, attempt_count, message_group)| JobRow {
                status,
                attempt_count,
                message_group,
            })
            .collect())
    }

    async fn outbox_left(&self, scenario: &str) -> anyhow::Result<BTreeMap<i16, i64>> {
        let db = self.db.as_ref().ok_or_else(|| anyhow!("no db"))?;
        let rows: Vec<(i16, i64)> = sqlx::query_as(
            "SELECT status, COUNT(*) FROM outbox_messages WHERE payload LIKE $1 GROUP BY status",
        )
        .bind(format!("%\"{scenario}-%"))
        .fetch_all(db)
        .await?;
        Ok(rows.into_iter().collect())
    }

    pub fn logs(&self) -> Vec<PathBuf> {
        ["platform", "worker", "router", "outbox"]
            .iter()
            .map(|n| self.dir.join(format!("{n}.log")))
            .collect()
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

fn record_batch(errors: &mut Vec<String>, what: &str, r: &crate::api::Response) {
    if !(200..300).contains(&r.status) {
        errors.push(format!("{what} -> {} {}", r.status, truncate(&r.text, 300)));
        return;
    }
    if let Some(results) = r.body.get("results").and_then(Value::as_array) {
        for item in results {
            let st = item.get("status").and_then(Value::as_str).unwrap_or("");
            if !st.is_empty() && st != "SUCCESS" && st != "CREATED" && st != "DUPLICATE" {
                errors.push(format!(
                    "{what} item -> {}",
                    truncate(&item.to_string(), 200)
                ));
            }
        }
    }
}

fn marker(m: &Sent) -> Value {
    json!({"hk": m.hk, "hg": m.group, "hs": m.seq})
}

pub fn pool_code(scenario: &str, code: &str) -> String {
    format!("h-{scenario}-{code}").to_lowercase()
}

pub fn is_terminal(status: &str) -> bool {
    matches!(
        status,
        "COMPLETED" | "FAILED" | "CANCELLED" | "EXPIRED" | "IGNORED"
    )
}

fn outbox_id() -> String {
    const ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let mut s = String::with_capacity(13);
    for _ in 0..13 {
        s.push(ALPHABET[rand::random_range(0..ALPHABET.len())] as char);
    }
    s
}

fn health_urls(api: u16, metrics: u16) -> Vec<String> {
    vec![
        format!("http://127.0.0.1:{api}/health"),
        format!("http://127.0.0.1:{api}/health/live"),
        format!("http://127.0.0.1:{api}/q/health/live"),
        format!("http://127.0.0.1:{metrics}/health"),
        format!("http://127.0.0.1:{metrics}/q/health/live"),
    ]
}

fn create_queue(container: &str, name: &str) -> anyhow::Result<()> {
    let out = std::process::Command::new("docker")
        .args([
            "exec",
            container,
            "awslocal",
            "sqs",
            "create-queue",
            "--queue-name",
            name,
            "--attributes",
            "FifoQueue=true,ContentBasedDeduplication=false",
        ])
        .output()?;
    if !out.status.success() {
        anyhow::bail!(
            "create queue {name}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(())
}

/// Messages visible / in flight on a queue (diagnostics only).
pub fn queue_depth(container: &str, name: &str) -> Option<(u64, u64)> {
    let out = std::process::Command::new("docker")
        .args([
            "exec",
            container,
            "awslocal",
            "sqs",
            "get-queue-attributes",
            "--queue-url",
            &format!("http://localhost:4566/000000000000/{name}"),
            "--attribute-names",
            "ApproximateNumberOfMessages",
            "ApproximateNumberOfMessagesNotVisible",
        ])
        .output()
        .ok()?;
    let v: Value = serde_json::from_slice(&out.stdout).ok()?;
    let a = v.get("Attributes")?;
    let n = |k: &str| a.get(k)?.as_str()?.parse::<u64>().ok();
    Some((
        n("ApproximateNumberOfMessages")?,
        n("ApproximateNumberOfMessagesNotVisible")?,
    ))
}

impl Side {
    pub fn queue_depth(&self) -> Option<(u64, u64)> {
        queue_depth(
            &self.infra_sqs_container,
            &format!("{}-platform-DEFAULT.fifo", self.kind.queue_prefix()),
        )
    }
}
