//! Runs a router binary with the production router task definition's
//! environment (`inhance/iac/compute/fc-router.ts`, every name it sets,
//! fake values) against local stand-ins:
//!
//! - a **platform** (its own origin): `POST /oauth/token` mints a token for
//!   the router's client credentials; `GET /api/dispatch/router-config`
//!   answers only that bearer; `POST /api/dispatch/settled`;
//! - an **Integral** config service (another origin): `GET /api/config`,
//!   and the Teams webhook the notifications point at;
//! - **SQS** (`AWS_ENDPOINT_URL_SQS`): answers the SDK's JSON protocol, an
//!   empty `ReceiveMessage` after a short long-poll.
//!
//! Only three things are added to the production names, all test plumbing:
//! free ports (`FC_API_PORT`, `FC_METRICS_PORT`), fake AWS credentials,
//! and the SQS endpoint. No database variable is set — a router that tried
//! to connect to Postgres would fail to boot.
//!
//! Shared by `fc-server`'s and the standalone `fc-router`'s tests.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Router;
use tokio::process::{Child, Command};

pub const CLIENT_ID: &str = "oac_router_test";
pub const CLIENT_SECRET: &str = "router-test-secret";
pub const TOKEN: &str = "router-token-1";
pub const PLATFORM_POOL: &str = "platform-DEFAULT";
pub const INTEGRAL_POOL: &str = "INTEGRAL-POOL";
pub const PLATFORM_QUEUE: &str = "FC-test-platform-DEFAULT.fifo";
pub const INTEGRAL_QUEUE: &str = "integral-dispatch.fifo";

/// One request a stand-in received.
#[derive(Debug, Clone)]
pub struct Recorded {
    pub method: String,
    pub path: String,
    pub authorization: Option<String>,
    pub target: Option<String>,
    pub body: String,
}

#[derive(Clone, Default)]
pub struct Recorder(Arc<parking_lot::Mutex<Vec<Recorded>>>);

impl Recorder {
    fn push(&self, method: &Method, uri: &Uri, headers: &HeaderMap, body: &Bytes) {
        let header = |name: &str| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        };
        self.0.lock().push(Recorded {
            method: method.to_string(),
            path: uri.path().to_string(),
            authorization: header("authorization"),
            target: header("x-amz-target"),
            body: String::from_utf8_lossy(body).to_string(),
        });
    }

    pub fn all(&self) -> Vec<Recorded> {
        self.0.lock().clone()
    }
}

fn sqs_uri(queue: &str) -> String {
    format!("https://sqs.eu-west-1.amazonaws.com/000000000000/{queue}")
}

async fn platform(
    State(rec): State<Recorder>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    rec.push(&method, &uri, &headers, &body);
    match (method.as_str(), uri.path()) {
        ("POST", "/oauth/token") => {
            let form = String::from_utf8_lossy(&body);
            let ok = form.contains("grant_type=client_credentials")
                && form.contains(&format!("client_id={CLIENT_ID}"))
                && form.contains(&format!("client_secret={CLIENT_SECRET}"));
            if !ok {
                return (StatusCode::UNAUTHORIZED, "invalid_client").into_response();
            }
            axum::Json(serde_json::json!({
                "access_token": TOKEN, "token_type": "Bearer", "expires_in": 3600
            }))
            .into_response()
        }
        ("GET", "/api/dispatch/router-config") => {
            let bearer = headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default();
            if bearer != format!("Bearer {TOKEN}") {
                return StatusCode::UNAUTHORIZED.into_response();
            }
            // Explicit zeros: Go reads them as "unstated" (1 / 120s).
            axum::Json(serde_json::json!({
                "processingPools": [{"code": PLATFORM_POOL, "concurrency": 3}],
                "queues": [{
                    "queueName": PLATFORM_QUEUE,
                    "queueUri": sqs_uri(PLATFORM_QUEUE),
                    "connections": 0,
                    "visibilityTimeout": 0,
                }],
            }))
            .into_response()
        }
        ("POST", "/api/dispatch/settled") => {
            axum::Json(serde_json::json!({"settled": 0, "ids": []})).into_response()
        }
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn integral(
    State(rec): State<Recorder>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    rec.push(&method, &uri, &headers, &body);
    match (method.as_str(), uri.path()) {
        ("GET", "/api/config") => axum::Json(serde_json::json!({
            "processingPools": [{"code": INTEGRAL_POOL, "concurrency": 2}],
            "queues": [{"queueName": INTEGRAL_QUEUE, "queueUri": sqs_uri(INTEGRAL_QUEUE)}],
        }))
        .into_response(),
        ("POST", "/teams") => StatusCode::OK.into_response(),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn sqs(
    State(rec): State<Recorder>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    rec.push(&method, &uri, &headers, &body);
    let target = headers
        .get("x-amz-target")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let body = if target.ends_with("ReceiveMessage") {
        // A short long-poll so the consumers don't spin.
        tokio::time::sleep(Duration::from_millis(500)).await;
        serde_json::json!({"Messages": []})
    } else if target.ends_with("GetQueueAttributes") {
        serde_json::json!({"Attributes": {
            "ApproximateNumberOfMessages": "0",
            "ApproximateNumberOfMessagesNotVisible": "0"
        }})
    } else {
        serde_json::json!({})
    };
    (
        [("content-type", "application/x-amz-json-1.0")],
        body.to_string(),
    )
        .into_response()
}

async fn serve(app: Router) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    addr
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// The stand-ins and what they received.
pub struct StandIns {
    pub platform: SocketAddr,
    pub integral: SocketAddr,
    pub sqs: SocketAddr,
    pub platform_rec: Recorder,
    pub integral_rec: Recorder,
    pub sqs_rec: Recorder,
}

impl StandIns {
    pub async fn start() -> Self {
        let (platform_rec, integral_rec, sqs_rec) = (
            Recorder::default(),
            Recorder::default(),
            Recorder::default(),
        );
        let platform_addr = serve(
            Router::new()
                .fallback(platform)
                .with_state(platform_rec.clone()),
        )
        .await;
        let integral_addr = serve(
            Router::new()
                .fallback(integral)
                .with_state(integral_rec.clone()),
        )
        .await;
        let sqs_addr = serve(Router::new().fallback(sqs).with_state(sqs_rec.clone())).await;
        Self {
            platform: platform_addr,
            integral: integral_addr,
            sqs: sqs_addr,
            platform_rec,
            integral_rec,
            sqs_rec,
        }
    }

    pub fn platform_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.platform.port())
    }

    pub fn integral_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.integral.port())
    }

    /// Every variable the production router task definition sets, with
    /// fake values pointing at the stand-ins. The config URL lists
    /// Integral's source first and the platform's document second, as the
    /// deployed stack config does.
    pub fn production_env(&self) -> BTreeMap<String, String> {
        let platform = self.platform_url();
        let integral = self.integral_url();
        [
            ("RUST_LOG", "info".to_string()),
            ("API_PORT", "8080".to_string()),
            ("AWS_REGION", "eu-west-1".to_string()),
            ("MESSAGE_ROUTER_ENABLED", "true".to_string()),
            ("PLATFORM_ENABLED", "false".to_string()),
            (
                "FLOWCATALYST_CONFIG_URL",
                format!("{integral}/api/config,{platform}/api/dispatch/router-config"),
            ),
            ("FLOWCATALYST_CONFIG_INTERVAL", "300".to_string()),
            ("FC_ROUTER_PLATFORM_URL", platform),
            ("FLOWCATALYST_STANDBY_ENABLED", "false".to_string()),
            ("AUTH_MODE", "NONE".to_string()),
            ("NOTIFICATION_TEAMS_ENABLED", "true".to_string()),
            (
                "NOTIFICATION_TEAMS_WEBHOOK_URL",
                format!("{integral}/teams"),
            ),
            ("NOTIFICATION_MIN_SEVERITY", "WARNING".to_string()),
            ("NOTIFICATION_BATCH_INTERVAL", "60".to_string()),
            ("FC_ROUTER_CLIENT_ID", CLIENT_ID.to_string()),
            ("FC_ROUTER_CLIENT_SECRET", CLIENT_SECRET.to_string()),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect()
    }
}

/// A router process, the ports it serves on and its log file.
pub struct RouterProcess {
    pub child: Child,
    pub api_port: u16,
    pub metrics_port: u16,
    pub log_path: std::path::PathBuf,
}

impl RouterProcess {
    pub fn api(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.api_port)
    }

    /// Everything the process has logged so far.
    pub fn log(&self) -> String {
        std::fs::read_to_string(&self.log_path).unwrap_or_default()
    }
}

impl Drop for RouterProcess {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
        let log = self.log();
        if std::thread::panicking() {
            let tail: String = log
                .chars()
                .rev()
                .take(8000)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            eprintln!(
                "---- router log tail ({}) ----\n{tail}",
                self.log_path.display()
            );
        }
        let _ = std::fs::remove_file(&self.log_path);
    }
}

/// Start `binary` with `env` in an otherwise clean environment, plus the
/// test plumbing (ports, fake AWS credentials, the SQS endpoint).
pub fn spawn_router(
    binary: &Path,
    stand_ins: &StandIns,
    env: BTreeMap<String, String>,
) -> RouterProcess {
    let (api_port, metrics_port) = (free_port(), free_port());
    // A file, not a pipe: nothing drains a pipe while the router runs, and
    // a full one would block its logging.
    let log_path = std::env::temp_dir().join(format!(
        "fc-router-prod-env-{}-{api_port}.log",
        std::process::id()
    ));
    let log = std::fs::File::create(&log_path).expect("router log file");
    let mut cmd = Command::new(binary);
    cmd.env_clear();
    for key in ["PATH", "HOME", "TMPDIR"] {
        if let Ok(v) = std::env::var(key) {
            cmd.env(key, v);
        }
    }
    cmd.envs(env)
        .env("FC_API_PORT", api_port.to_string())
        .env("FC_METRICS_PORT", metrics_port.to_string())
        .env("AWS_ACCESS_KEY_ID", "AKIAFAKEFAKEFAKE")
        .env("AWS_SECRET_ACCESS_KEY", "fake-aws-secret")
        .env("AWS_EC2_METADATA_DISABLED", "true")
        .env(
            "AWS_ENDPOINT_URL_SQS",
            format!("http://127.0.0.1:{}", stand_ins.sqs.port()),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone().expect("log handle")))
        .stderr(Stdio::from(log))
        .kill_on_drop(true);
    RouterProcess {
        child: cmd.spawn().expect("router binary starts"),
        api_port,
        metrics_port,
        log_path,
    }
}

/// Poll `check` until it returns `Some`, or panic with `what` after
/// `timeout`.
pub async fn eventually<T, F, Fut>(timeout: Duration, what: &str, mut check: F) -> T
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Option<T>>,
{
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(v) = check().await {
            return v;
        }
        assert!(Instant::now() < deadline, "timed out waiting for: {what}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// GET `url`, returning the status and body when it answered at all.
pub async fn get(url: &str) -> Option<(u16, String)> {
    let resp = reqwest::get(url).await.ok()?;
    let status = resp.status().as_u16();
    Some((status, resp.text().await.unwrap_or_default()))
}

/// Everything the production contract promises, asserted against a router
/// started by [`spawn_router`] with [`StandIns::production_env`].
/// `router_base` is where the router's own HTTP surface is mounted
/// (`/router` under fc-server, the root for the standalone binary).
pub async fn assert_production_contract(
    stand_ins: &StandIns,
    router: &RouterProcess,
    router_base: &str,
) {
    let wait = Duration::from_secs(30);

    // The load balancer's probe answers.
    eventually(wait, "GET /health answers 200", || async {
        get(&router.api("/health"))
            .await
            .filter(|(status, _)| *status == 200)
    })
    .await;

    // Both sources merge: every pool from each comes up.
    let pools_url = router.api(&format!("{router_base}/monitoring/pools"));
    eventually(wait, "pools from both config sources", || async {
        get(&pools_url).await.filter(|(status, body)| {
            *status == 200 && body.contains(PLATFORM_POOL) && body.contains(INTEGRAL_POOL)
        })
    })
    .await;

    // Both queues come up: a consumer polls each through SQS.
    eventually(wait, "a consumer polling each queue", || async {
        let polled = |queue: &str| {
            stand_ins.sqs_rec.all().iter().any(|r| {
                r.target
                    .as_deref()
                    .is_some_and(|t| t.ends_with("ReceiveMessage"))
                    && r.body.contains(&sqs_uri(queue))
            })
        };
        (polled(PLATFORM_QUEUE) && polled(INTEGRAL_QUEUE)).then_some(())
    })
    .await;

    // The token was minted at {FC_ROUTER_PLATFORM_URL}/oauth/token with the
    // client credentials, and the platform's document fetched with it.
    let platform = stand_ins.platform_rec.all();
    let mints: Vec<&Recorded> = platform
        .iter()
        .filter(|r| r.method == "POST" && r.path == "/oauth/token")
        .collect();
    assert_eq!(mints.len(), 1, "one token, cached: {platform:#?}");
    assert!(mints[0].body.contains("grant_type=client_credentials"));
    let config_fetches: Vec<&Recorded> = platform
        .iter()
        .filter(|r| r.path == "/api/dispatch/router-config")
        .collect();
    assert!(!config_fetches.is_empty());
    for r in &config_fetches {
        assert_eq!(
            r.authorization.as_deref(),
            Some(format!("Bearer {TOKEN}").as_str())
        );
    }

    // The token — and the client secret — never reach the other origin.
    let integral = stand_ins.integral_rec.all();
    assert!(integral.iter().any(|r| r.path == "/api/config"));
    for r in &integral {
        assert_eq!(r.authorization, None, "credential sent to Integral: {r:?}");
        assert!(!r.body.contains(TOKEN) && !r.body.contains(CLIENT_SECRET));
    }
    for r in stand_ins.sqs_rec.all() {
        assert!(!r.body.contains(TOKEN) && !r.body.contains(CLIENT_SECRET));
    }
}

/// Wait for `router` to exit, returning its status and log.
pub async fn wait_exit(
    mut router: RouterProcess,
    timeout: Duration,
) -> (std::process::ExitStatus, String) {
    let status = tokio::time::timeout(timeout, router.child.wait())
        .await
        .expect("the router exits")
        .unwrap();
    (status, router.log())
}
