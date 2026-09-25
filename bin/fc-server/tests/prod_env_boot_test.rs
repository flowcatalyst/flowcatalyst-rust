//! Boots the real `fc-server` binary with the production task definitions'
//! environment (`inhance/iac/compute/flowcatalyst.ts`: the platform task and
//! the worker task), names and all, against a Postgres container.
//!
//! What it proves:
//! - the database is reached through `DB_SECRET_PROVIDER=aws` +
//!   `DB_SECRET_ARN` + `DB_HOST` + `DB_NAME`, the credentials (and port)
//!   coming from the secret — served here by a fake Secrets Manager the SDK is
//!   pointed at with `AWS_ENDPOINT_URL`, so no call leaves the machine;
//! - a rotated password (RDS rotation) is picked up by every pool: after the
//!   rotation every `fc_app` connection is killed and the platform keeps
//!   answering, and nothing logs an authentication failure;
//! - the platform task serves `/health` (the ALB target group's check) on
//!   `PORT` and a password login, with the JWT private key given as SSM hands
//!   it over (literal `\n`), JWKS carrying Go's key ids;
//! - a token signed with `FLOWCATALYST_JWT_PREVIOUS_PUBLIC_KEY`'s private half
//!   (a token Go issued before a key rotation) still validates;
//! - the worker task (`PLATFORM_ENABLED=false`, dispatch + scheduled-job
//!   schedulers on) serves only `/health`, and reports its subsystems.
//!
//! Needs Docker: `cargo test -p fc-server --test prod_env_boot_test -- --ignored`.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::{http::HeaderMap, routing::post, Router};
use fc_platform::auth::auth_service::{AuthConfig, AuthService};
use fc_platform::auth::signing_keys;
use rsa::pkcs1::EncodeRsaPrivateKey;
use rsa::pkcs8::{EncodePublicKey, LineEnding};
use rsa::{RsaPrivateKey, RsaPublicKey};
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;

const SECRET_ARN: &str = "arn:aws:secretsmanager:eu-west-1:000000000000:secret:rds!db-fc-test";
const APP_ROLE: &str = "fc_app";
const EXTERNAL_BASE_URL: &str = "https://platform.example.test";
const ADMIN_EMAIL: &str = "ops@example.test";
const ADMIN_PASSWORD: &str = "Correct-Horse-Battery-9!";

// ── Fake Secrets Manager ─────────────────────────────────────────────────────

#[derive(Clone)]
struct FakeSecretsManager {
    secret: Arc<Mutex<String>>,
    calls: Arc<AtomicUsize>,
}

/// Answers `secretsmanager.GetSecretValue` (awsJson1.1) with the current
/// secret string; any other AWS call gets a 400.
async fn fake_aws(
    axum::extract::State(sm): axum::extract::State<FakeSecretsManager>,
    headers: HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let target = headers
        .get("x-amz-target")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if target != "secretsmanager.GetSecretValue" {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            [("content-type", "application/x-amz-json-1.1")],
            r#"{"__type":"UnknownOperationException","message":"not faked"}"#,
        )
            .into_response();
    }
    sm.calls.fetch_add(1, Ordering::SeqCst);
    let body = serde_json::json!({
        "ARN": SECRET_ARN,
        "Name": "rds!db-fc-test",
        "VersionId": "v1",
        "SecretString": sm.secret.lock().unwrap().clone(),
        "VersionStages": ["AWSCURRENT"],
        "CreatedDate": 1.7e9,
    });
    (
        [("content-type", "application/x-amz-json-1.1")],
        body.to_string(),
    )
        .into_response()
}

async fn start_fake_aws(sm: FakeSecretsManager) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = Router::new()
        .route("/", post(fake_aws))
        .fallback(fake_aws)
        .with_state(sm);
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    port
}

// ── The server process ───────────────────────────────────────────────────────

struct Server {
    child: Child,
    logs: Arc<Mutex<Vec<String>>>,
    port: u16,
    metrics_port: u16,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Server {
    fn spawn(env: &HashMap<String, String>) -> Self {
        let port = free_port();
        let metrics_port = free_port();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_fc-server"));
        cmd.env_clear()
            .envs(env)
            .env("PORT", port.to_string())
            // The task definitions leave the metrics port at its default
            // (9090); a test run can't count on that port being free.
            .env("FC_METRICS_PORT", metrics_port.to_string())
            .current_dir(std::env::temp_dir())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().expect("spawn fc-server");
        let logs = Arc::new(Mutex::new(Vec::new()));
        for stream in [
            Box::new(child.stdout.take().unwrap()) as Box<dyn std::io::Read + Send>,
            Box::new(child.stderr.take().unwrap()),
        ] {
            let logs = logs.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(stream).lines().map_while(|l| l.ok()) {
                    logs.lock().unwrap().push(line);
                }
            });
        }
        Self {
            child,
            logs,
            port,
            metrics_port,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{}", self.port, path)
    }

    fn log_text(&self) -> String {
        self.logs.lock().unwrap().join("\n")
    }

    async fn wait_healthy(&mut self, http: &reqwest::Client) {
        let deadline = Instant::now() + Duration::from_secs(180);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                panic!("fc-server exited ({status}):\n{}", self.log_text());
            }
            if let Ok(r) = http.get(self.url("/health")).send().await {
                if r.status().is_success() {
                    return;
                }
            }
            assert!(
                Instant::now() < deadline,
                "fc-server never became healthy:\n{}",
                self.log_text()
            );
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// A PEM as an SSM parameter hands it to the container: one line, with
/// literal `\n` escapes.
fn ssm_mangled(pem: &str) -> String {
    pem.trim().replace('\n', "\\n")
}

fn secret_json(password: &str, port: u16) -> String {
    serde_json::json!({
        "username": APP_ROLE,
        "password": password,
        "engine": "postgres",
        "port": port,
    })
    .to_string()
}

/// The environment both task definitions share (`sharedEnv` +
/// `sharedSecrets`), with test values.
fn shared_env(
    fake_aws_port: u16,
    current_key: &str,
    previous_public: &str,
) -> HashMap<String, String> {
    let mut env: HashMap<String, String> = [
        ("RUST_LOG", "info"),
        ("DB_SECRET_PROVIDER", "aws"),
        ("DB_SECRET_ARN", SECRET_ARN),
        ("DB_HOST", "127.0.0.1"),
        ("DB_NAME", "flowcatalyst"),
        // Standby is off in both tasks, so this is never dialled.
        ("REDIS_URL", "rediss://127.0.0.1:1"),
        ("DISPATCH_QUEUE_TYPE", "SQS"),
        (
            "DISPATCH_QUEUE_URL",
            "https://sqs.eu-west-1.amazonaws.com/000000000000/inhance-fc-test-dispatch.fifo",
        ),
        ("DISPATCH_QUEUE_REGION", "eu-west-1"),
        ("FC_DISPATCH_QUEUE_PREFIX", "FC-test"),
        (
            "FLOWCATALYST_APP_KEY",
            "MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=",
        ),
        (
            "DISPATCH_SCHEDULER_PROCESSING_ENDPOINT",
            "http://fc-platform:8080/api/dispatch/process",
        ),
        // Test plumbing, not task-definition env: fake AWS credentials and
        // every AWS endpoint pointed at the local fake, so nothing reaches
        // AWS; a fast rotation poll; and the first admin.
        ("AWS_ACCESS_KEY_ID", "test"),
        ("AWS_SECRET_ACCESS_KEY", "test"),
        ("AWS_EC2_METADATA_DISABLED", "true"),
        ("DB_SECRET_REFRESH_INTERVAL_MS", "300"),
        ("FLOWCATALYST_BOOTSTRAP_ADMIN_EMAIL", ADMIN_EMAIL),
        ("FLOWCATALYST_BOOTSTRAP_ADMIN_PASSWORD", ADMIN_PASSWORD),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    env.insert(
        "AWS_ENDPOINT_URL".into(),
        format!("http://127.0.0.1:{fake_aws_port}"),
    );
    env.insert(
        "FLOWCATALYST_JWT_PRIVATE_KEY".into(),
        ssm_mangled(current_key),
    );
    env.insert(
        "FLOWCATALYST_JWT_PREVIOUS_PUBLIC_KEY".into(),
        ssm_mangled(previous_public),
    );
    env
}

fn platform_env(shared: &HashMap<String, String>) -> HashMap<String, String> {
    let mut env = shared.clone();
    for (k, v) in [
        ("PLATFORM_ENABLED", "true"),
        ("STREAM_PROCESSOR_ENABLED", "true"),
        ("DISPATCH_SCHEDULER_ENABLED", "false"),
        ("MESSAGE_ROUTER_ENABLED", "false"),
        ("STANDBY_ENABLED", "false"),
        ("EXTERNAL_BASE_URL", EXTERNAL_BASE_URL),
        ("OIDC_ACCESS_TOKEN_TTL", "3600"),
        ("OIDC_SESSION_TTL", "28800"),
        ("OIDC_REFRESH_TOKEN_TTL", "2592000"),
        ("FC_WEBAUTHN_RP_ID", "example.test"),
        ("FC_WEBAUTHN_ORIGINS", EXTERNAL_BASE_URL),
        ("SMTP_HOST", "127.0.0.1"),
        ("SMTP_PORT", "587"),
        ("SMTP_SECURE", "false"),
        ("SMTP_USERNAME", "apikey"),
        ("SMTP_FROM", "mailer@example.test"),
        ("SMTP_PASSWORD", "not-a-real-password"),
    ] {
        env.insert(k.into(), v.into());
    }
    env
}

fn worker_env(shared: &HashMap<String, String>) -> HashMap<String, String> {
    let mut env = shared.clone();
    for (k, v) in [
        ("PLATFORM_ENABLED", "false"),
        ("STREAM_PROCESSOR_ENABLED", "false"),
        ("MESSAGE_ROUTER_ENABLED", "false"),
        ("DISPATCH_SCHEDULER_ENABLED", "true"),
        ("FC_SCHEDULED_JOB_ENABLED", "true"),
        ("STANDBY_ENABLED", "false"),
    ] {
        env.insert(k.into(), v.into());
    }
    env
}

async fn login(http: &reqwest::Client, server: &Server) -> reqwest::Response {
    http.post(server.url("/auth/login"))
        .json(&serde_json::json!({ "email": ADMIN_EMAIL, "password": ADMIN_PASSWORD }))
        .send()
        .await
        .unwrap()
}

fn session_cookie(resp: &reqwest::Response) -> Option<String> {
    resp.headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find_map(|c| c.strip_prefix("fc_session="))
        .map(|c| c.split(';').next().unwrap_or("").to_string())
        .filter(|c| !c.is_empty())
}

#[tokio::test]
#[ignore = "needs Docker"]
async fn fc_server_runs_with_the_production_task_definitions_env() {
    // Postgres, with an application role whose credentials only the secret
    // knows (the container's own superuser is never given to fc-server).
    let container = Postgres::default()
        .with_db_name("flowcatalyst")
        .with_user("postgres")
        .with_password("postgres")
        .start()
        .await
        .expect("start postgres");
    let pg_port = container.get_host_port_ipv4(5432).await.unwrap();
    let admin_url = format!("postgresql://postgres:postgres@127.0.0.1:{pg_port}/flowcatalyst");
    let admin_pool = sqlx::PgPool::connect(&admin_url).await.unwrap();
    sqlx::query(&format!(
        "CREATE ROLE {APP_ROLE} LOGIN SUPERUSER PASSWORD 'initial-Pa55:word@'"
    ))
    .execute(&admin_pool)
    .await
    .unwrap();

    let sm = FakeSecretsManager {
        secret: Arc::new(Mutex::new(secret_json("initial-Pa55:word@", pg_port))),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let fake_aws_port = start_fake_aws(sm.clone()).await;

    // The current signing key (PKCS#1, as openssl writes it) and a previous
    // key pair from before a rotation.
    let mut rng = rsa::rand_core::OsRng;
    let current = RsaPrivateKey::new(&mut rng, 2048).unwrap();
    let current_pem = current.to_pkcs1_pem(LineEnding::LF).unwrap().to_string();
    let previous = RsaPrivateKey::new(&mut rng, 2048).unwrap();
    let previous_pem = previous.to_pkcs1_pem(LineEnding::LF).unwrap().to_string();
    let previous_public = RsaPublicKey::from(&previous)
        .to_public_key_pem(LineEnding::LF)
        .unwrap();

    let shared = shared_env(fake_aws_port, &current_pem, &previous_public);
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    // ── Platform task ────────────────────────────────────────────────────
    let mut platform = Server::spawn(&platform_env(&shared));
    platform.wait_healthy(&http).await;
    assert!(
        sm.calls.load(Ordering::SeqCst) >= 1,
        "the DB credentials came from Secrets Manager"
    );

    // The ALB target group's health check.
    let health = http.get(platform.url("/health")).send().await.unwrap();
    assert_eq!(health.status(), 200);

    // JWKS: the current key under Go's kid (a hash of the PEM derived from
    // the private key) and the previous key under its own.
    let current_kid =
        signing_keys::key_id(&signing_keys::public_pem_from_private_pem(&current_pem).unwrap());
    let previous_kid = signing_keys::key_id(previous_public.trim());
    let jwks: serde_json::Value = http
        .get(platform.url("/.well-known/jwks.json"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let kids: Vec<&str> = jwks["keys"]
        .as_array()
        .expect("jwks keys")
        .iter()
        .filter_map(|k| k["kid"].as_str())
        .collect();
    assert_eq!(kids, vec![current_kid.as_str(), previous_kid.as_str()]);

    // A password login sets the session cookie, and the cookie works.
    let resp = login(&http, &platform).await;
    assert_eq!(resp.status(), 200, "login: {}", platform.log_text());
    let cookie = session_cookie(&resp).expect("fc_session cookie");
    let me = http
        .get(platform.url("/auth/me"))
        .header("cookie", format!("fc_session={cookie}"))
        .send()
        .await
        .unwrap();
    assert_eq!(me.status(), 200);

    // Tokens signed with the previous key — issued before a rotation — still
    // validate: an access token on Go's own previous-key paths
    // (/oauth/userinfo) and the API, and a session cookie.
    let principal = fc_platform::principal::repository::PrincipalRepository::new(&admin_pool)
        .find_by_email(ADMIN_EMAIL)
        .await
        .unwrap()
        .expect("bootstrap admin");
    let old_signer = AuthService::new_with_rsa(
        AuthConfig {
            issuer: EXTERNAL_BASE_URL.into(),
            audience: EXTERNAL_BASE_URL.into(),
            ..AuthConfig::default()
        },
        &previous_pem,
        previous_public.trim(),
    )
    .unwrap();
    assert_eq!(old_signer.key_id(), Some(previous_kid.as_str()));
    let old_access = old_signer.generate_access_token(&principal).unwrap();
    for path in ["/oauth/userinfo", "/api/me"] {
        let r = http
            .get(platform.url(path))
            .bearer_auth(&old_access)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200, "{path} with a previous-key token");
    }
    let old_session = old_signer.generate_session_token(&principal).unwrap();
    let r = http
        .get(platform.url("/auth/me"))
        .header("cookie", format!("fc_session={old_session}"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "a previous-key session cookie");

    // A token under a key the platform never had is refused.
    let stranger = RsaPrivateKey::new(&mut rng, 2048).unwrap();
    let stranger_pem = stranger.to_pkcs1_pem(LineEnding::LF).unwrap().to_string();
    let stranger_signer = AuthService::new_with_rsa(
        AuthConfig {
            issuer: EXTERNAL_BASE_URL.into(),
            audience: EXTERNAL_BASE_URL.into(),
            ..AuthConfig::default()
        },
        &stranger_pem,
        &signing_keys::public_pem_from_private_pem(&stranger_pem).unwrap(),
    )
    .unwrap();
    let forged = stranger_signer.generate_access_token(&principal).unwrap();
    let r = http
        .get(platform.url("/api/me"))
        .bearer_auth(&forged)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);

    // ── RDS rotation ─────────────────────────────────────────────────────
    // Rotate the password and the secret, give the refresh tasks a few polls,
    // then kill every fc_app connection: each pool (the platform's and the
    // stream processor's) must reconnect with the new password.
    sqlx::query(&format!(
        "ALTER ROLE {APP_ROLE} PASSWORD 'rotated-Pa55/word#'"
    ))
    .execute(&admin_pool)
    .await
    .unwrap();
    *sm.secret.lock().unwrap() = secret_json("rotated-Pa55/word#", pg_port);
    let calls_at_rotation = sm.calls.load(Ordering::SeqCst);
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(
        sm.calls.load(Ordering::SeqCst) >= calls_at_rotation + 4,
        "the secret is polled for rotation"
    );
    let log_mark = platform.logs.lock().unwrap().len();
    sqlx::query("SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE usename = $1")
        .bind(APP_ROLE)
        .execute(&admin_pool)
        .await
        .unwrap();

    let mut status = 0;
    for _ in 0..20 {
        status = login(&http, &platform).await.status().as_u16();
        if status == 200 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    assert_eq!(
        status,
        200,
        "login after rotation:\n{}",
        platform.log_text()
    );

    // The stream processor polls continuously; give it time to reconnect,
    // then require live fc_app connections and no authentication failure.
    tokio::time::sleep(Duration::from_secs(4)).await;
    let (live,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM pg_stat_activity WHERE usename = $1")
            .bind(APP_ROLE)
            .fetch_one(&admin_pool)
            .await
            .unwrap();
    assert!(live >= 2, "fc_app reconnected ({live} connections)");
    let after: String = platform.logs.lock().unwrap()[log_mark..].join("\n");
    assert!(
        !after.contains("password authentication failed"),
        "a pool kept the rotated-out password:\n{after}"
    );

    // ── Worker task ──────────────────────────────────────────────────────
    let mut worker = Server::spawn(&worker_env(&shared));
    worker.wait_healthy(&http).await;
    // Only the health surface: no platform API, no SPA.
    for path in ["/api/me", "/auth/login", "/.well-known/jwks.json"] {
        let r = http.get(worker.url(path)).send().await.unwrap();
        assert_eq!(r.status(), 404, "the worker does not serve {path}");
    }
    let ready: serde_json::Value = http
        .get(format!("http://127.0.0.1:{}/ready", worker.metrics_port))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(ready["platform"], false);
    assert_eq!(ready["scheduler"], true);
    assert_eq!(ready["scheduled_job"], true);
    assert_eq!(ready["stream"], false);
    assert_eq!(ready["router"], false);
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(
        worker.child.try_wait().unwrap().is_none(),
        "the worker stays up:\n{}",
        worker.log_text()
    );
    let worker_logs = worker.log_text();
    assert!(
        worker_logs.contains("Starting scheduler subsystem")
            && worker_logs.contains("Starting scheduled-job scheduler subsystem"),
        "{worker_logs}"
    );
}
