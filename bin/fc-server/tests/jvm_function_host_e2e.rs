//! `runtime: jvm` functions end to end: the **Rust platform** is the control
//! plane and **Java's function host** (`flowcatalyst-javalin`'s
//! `function-host`) runs the jar (owner, 2026-09-28: the supported path for
//! Java functions).
//!
//! Against a Postgres container, with signatures `required` (a private
//! Sigstore trust root given to the platform and every host):
//! - `fc-server` in its platform role (with the stream processor, so an
//!   ingested event fans out to the function's subscription, and scheduled
//!   jobs), and no Rust function host in the `jvm` pool;
//! - Java's function host, from a scratch build of the javalin checkout,
//!   in pool `jvm`, authenticated with `client_credentials` as a service
//!   account holding the built-in `function-host` role;
//! - later, `fc-server` function hosts (Rust) for the mixed-pool checks.
//!
//! The function is javalin's `examples/function-hello` (its shrunk jar and
//! its own `manifest.json`, with the pool set to `jvm`), published through
//! the platform's API as `fc-dev fn deploy` does: a `function-publisher`
//! service account uploads the jar, publishes version 1 with its signature
//! bundle, waits for `READY` and promotes it with an `expectedVersion`
//! precondition.
//!
//! 1. The Java host fetches desired state, downloads the jar from
//!    `/control/functions/artifacts/{versionId}`, verifies its signature
//!    against the recorded signer, registers the candidate and the platform
//!    marks it `READY`.
//! 2. Promoted live, the Java host serves `GET /healthz` (`auth: none`) and
//!    `GET /api/hello/{name}` (`auth: platform`): a platform token holding
//!    `hello:greeting:greet` is answered, one without it is refused by the
//!    function, and no token is refused by the host.
//! 3. An ingested `hello:greeting:greeting:requested` event fans out to the
//!    subscription the promote wired (`FC_FN_POOL_URL` → the Java host);
//!    the platform's `/api/dispatch/process` delivers it, signed with the
//!    application's webhook secret, and the function reads its config and
//!    secret and emits `hello:greeting:greeting:sent`, which lands in
//!    `msg_events` as `function:hello.default.hello`. A config change
//!    reloads the function; a pinned version (`<address>:1`) needs
//!    `platform:function:version:invoke`. A scheduled-job firing, fired by
//!    hand, reaches a one-class JVM function compiled here and parses with
//!    Java's `Webhook.schedule`.
//! 4. Mixed pools: a Rust host joining pool `jvm` reports the jar `FAILED`
//!    `RUNTIME_UNSUPPORTED` (logged once, no crash loop) while the Java host
//!    keeps it `LOADED`; a `jvm` publish to a pool served only by Rust hosts
//!    is refused (`409 POOL_RUNTIME_UNSUPPORTED`); a Rust `component` in the
//!    Java host's pool is `FAILED` there (unreadable runtime), and a Rust
//!    `wasm` function is registered as a candidate but `FAILED` once live.
//! 5. Disabling the function unloads it; the Java host stops cleanly on
//!    `SIGTERM`.
//!
//! Run (needs Docker, a JDK 25 and Maven 3.9; skips with a message when one
//! is missing):
//!
//! ```text
//! cargo test -p fc-server --test jvm_function_host_e2e -- --ignored --nocapture
//! ```
//!
//! The javalin checkout is `FC_JAVALIN_DIR` (default `../flowcatalyst-javalin`
//! beside this repository). It is never built in place: its `HEAD` is
//! exported with `git archive` into `target/jvm-e2e/javalin-<commit>` and
//! built there once (`mvn -Pexamples -pl function-host,examples/function-hello
//! -am package`). `FC_JVM_BUILD_DIR` names an already-built javalin tree
//! instead (no export, no build).

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use sha2::Digest as _;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;

/// fc-function-signing's port of Java's `TestSigstore`: a private CA and
/// transparency log whose `trusted_root.json` both the platform and the
/// hosts are given (`FC_FN_TRUST_ROOT`), so publishing runs with
/// `FC_FN_SIGNATURES=required`, as production does.
#[path = "../../../crates/fc-function-signing/tests/support/sigstore.rs"]
mod sigstore;

const APP_KEY: &str = "MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=";
const ADMIN_EMAIL: &str = "ops@jvm-e2e.test";
const ADMIN_PASSWORD: &str = "Correct-Horse-Battery-9!";
const POOL: &str = "jvm";
const ADDRESS: &str = "hello.default.hello";
const REQUESTED: &str = "hello:greeting:greeting:requested";
const SENT: &str = "hello:greeting:greeting:sent";
const GREET: &str = "hello:greeting:greet";
const JAVA_HOST_ID: &str = "jvm-e2e-java-host";
const RUST_HOST_ID: &str = "jvm-e2e-rust-host";
const RUST_ONLY_HOST_ID: &str = "jvm-e2e-rust-only-host";

// ── Processes ────────────────────────────────────────────────────────────────

/// A child process whose stdout and stderr are kept; killed on drop.
struct Proc {
    name: &'static str,
    child: Child,
    logs: Arc<Mutex<Vec<String>>>,
}

impl Proc {
    fn spawn(name: &'static str, mut command: Command) -> Self {
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .unwrap_or_else(|e| panic!("spawn {name}: {e}"));
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
        Self { name, child, logs }
    }

    fn log_text(&self) -> String {
        self.logs.lock().unwrap().join("\n")
    }

    fn count(&self, needle: &str) -> usize {
        self.logs
            .lock()
            .unwrap()
            .iter()
            .filter(|l| l.contains(needle))
            .count()
    }

    fn assert_running(&mut self) {
        if let Some(status) = self.child.try_wait().unwrap() {
            panic!("{} exited ({status}):\n{}", self.name, self.log_text());
        }
    }
}

impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn succeeds(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// `None` (with the reason) when a tool the check needs is missing.
fn missing_prerequisite() -> Option<&'static str> {
    if !succeeds("docker", &["info"]) {
        return Some("Docker is not running");
    }
    let prebuilt = std::env::var_os("FC_JVM_BUILD_DIR").is_some();
    if !succeeds("java", &["-version"]) || !succeeds("javac", &["-version"]) {
        return Some("a JDK (java, javac) is not on PATH");
    }
    if !prebuilt && !succeeds("mvn", &["-v"]) {
        return Some("mvn is not on PATH");
    }
    if !prebuilt {
        let javalin = std::env::var_os("FC_JAVALIN_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| repo_root().join("../flowcatalyst-javalin"));
        if !javalin.join(".git").exists() {
            return Some("no javalin checkout (set FC_JAVALIN_DIR or FC_JVM_BUILD_DIR)");
        }
    }
    None
}

// ── The Java build ───────────────────────────────────────────────────────────

struct JavaJars {
    /// The host's executable jar (`java -jar`, as the image runs it).
    host: PathBuf,
    /// `examples/function-hello`'s shrunk jar, and its `manifest.json`.
    hello: PathBuf,
    manifest: Value,
    /// A one-class function with a schedule endpoint (see [`PROBE_SOURCE`]).
    probe: PathBuf,
}

/// A JVM function whose `/tick` endpoint parses a scheduled-job firing
/// with `Webhook.schedule` and emits what it read, so a firing the
/// platform delivers is checked by Java's own parser.
const PROBE_SOURCE: &str = r#"package e2e.probe;

import io.flowcatalyst.function.EmitResult;
import io.flowcatalyst.function.Function;
import io.flowcatalyst.function.FunctionContext;
import io.flowcatalyst.function.OutboundEvent;
import io.flowcatalyst.function.Request;
import io.flowcatalyst.function.Result;
import io.flowcatalyst.function.Schedule;
import io.flowcatalyst.function.Webhook;
import java.nio.charset.StandardCharsets;

public final class ScheduleProbe implements Function {
    @Override
    public Result handle(Request in, FunctionContext ctx) {
        Schedule s = Webhook.schedule(in);
        String data = "{\"jobCode\":\"" + s.jobCode() + "\",\"triggerKind\":\"" + s.triggerKind()
                + "\",\"concurrent\":" + s.concurrent() + ",\"tracksCompletion\":" + s.tracksCompletion() + "}";
        OutboundEvent event = new OutboundEvent("hello:greeting:greeting:sent", "function:" + ctx.address().render(),
                s.instanceId(), "application/json", data.getBytes(StandardCharsets.UTF_8), s.correlationId(),
                null, null, s.instanceId());
        return switch (ctx.events().emit(event)) {
            case EmitResult.Emitted emitted -> Result.ack();
            case EmitResult.Refused refused -> Result.fail("emit refused: " + refused.code());
        };
    }
}
"#;

fn run(command: &mut Command, what: &str) -> Result<(), String> {
    let status = command.status().map_err(|e| format!("{what}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{what} failed ({status})"))
    }
}

/// A built javalin tree: `FC_JVM_BUILD_DIR` when set (already built), else
/// an export of the checkout's `HEAD` under `target/jvm-e2e`, built once.
fn javalin_tree() -> Result<PathBuf, String> {
    if let Some(dir) = std::env::var_os("FC_JVM_BUILD_DIR") {
        eprintln!(
            "[jvm-e2e] using the built javalin tree {}",
            dir.to_string_lossy()
        );
        return Ok(dir.into());
    }
    let javalin = std::env::var_os("FC_JAVALIN_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("../flowcatalyst-javalin"));
    let head = Command::new("git")
        .args(["-C", javalin.to_str().unwrap(), "rev-parse", "HEAD"])
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !head.status.success() {
        return Err(format!(
            "{} is not a git checkout (set FC_JAVALIN_DIR)",
            javalin.display()
        ));
    }
    let commit = String::from_utf8_lossy(&head.stdout).trim().to_string();
    eprintln!("[jvm-e2e] javalin {} at {commit}", javalin.display());
    let tree = repo_root().join(format!("target/jvm-e2e/javalin-{}", &commit[..12]));
    if tree.join(".built").is_file() {
        return Ok(tree);
    }
    std::fs::create_dir_all(&tree).map_err(|e| e.to_string())?;
    run(
        Command::new("sh").arg("-c").arg(format!(
            "git -C '{}' archive {commit} | tar -x -C '{}'",
            javalin.display(),
            tree.display()
        )),
        "git archive of the javalin checkout",
    )?;
    eprintln!("[jvm-e2e] building the Java host and function-hello (mvn package)");
    run(
        Command::new("mvn").current_dir(&tree).args([
            "-B",
            "-q",
            "-DskipTests",
            "-Pexamples",
            "-pl",
            "function-host,examples/function-hello",
            "-am",
            "package",
        ]),
        "mvn package of the javalin export",
    )?;
    std::fs::write(tree.join(".built"), &commit).map_err(|e| e.to_string())?;
    Ok(tree)
}

fn java_jars(work: &Path) -> Result<JavaJars, String> {
    let tree = javalin_tree()?;
    let host = tree.join("function-host/target/flowcatalyst-function-host-0.0.1-SNAPSHOT-exec.jar");
    let hello =
        tree.join("examples/function-hello/target/function-hello-0.0.1-SNAPSHOT-shrunk.jar");
    let api = tree.join("function-api/target/flowcatalyst-function-api-0.0.1-SNAPSHOT.jar");
    for jar in [&host, &hello, &api] {
        if !jar.is_file() {
            return Err(format!("{} is missing", jar.display()));
        }
    }
    let manifest = std::fs::read(tree.join("examples/function-hello/manifest.json"))
        .map_err(|e| format!("function-hello's manifest: {e}"))?;

    // The probe: compiled against function-api alone, as a function jar is.
    let probe_dir = work.join("probe");
    let source = probe_dir.join("src/e2e/probe/ScheduleProbe.java");
    std::fs::create_dir_all(source.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&source, PROBE_SOURCE).map_err(|e| e.to_string())?;
    let classes = probe_dir.join("classes");
    run(
        Command::new("javac")
            .args(["--release", "21", "-cp"])
            .arg(&api)
            .arg("-d")
            .arg(&classes)
            .arg(&source),
        "javac of the schedule probe",
    )?;
    let probe = probe_dir.join("probe.jar");
    run(
        Command::new("jar")
            .arg("--create")
            .arg("--file")
            .arg(&probe)
            .arg("-C")
            .arg(&classes)
            .arg("."),
        "jar of the schedule probe",
    )?;
    Ok(JavaJars {
        host,
        hello,
        manifest: serde_json::from_slice(&manifest).map_err(|e| e.to_string())?,
        probe,
    })
}

// ── Signing ──────────────────────────────────────────────────────────────────

/// The keyless signer every bundle here names (the TestSigstore leaf's
/// defaults), and the policy entry that permits it.
const SIGNER_ISSUER: &str = "https://example.test/issuer";
const SIGNER_SUBJECT: &str = "https://example.test/workflow.yml";

struct Signing {
    eco: sigstore::Ecosystem,
    trust_root: PathBuf,
}

impl Signing {
    fn new(dir: &Path) -> Self {
        use base64::Engine as _;
        let now = chrono::Utc::now();
        let eco = sigstore::build(&sigstore::LeafSpec::valid(
            now - chrono::Duration::hours(1),
            now + chrono::Duration::hours(1),
        ));
        let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
        let log_spki = sigstore::spki_der(&eco.log_key);
        let start = (now - chrono::Duration::days(1)).to_rfc3339();
        // A Sigstore `trusted_root.json`, the shape both Java's and Rust's
        // `TrustRoot` read.
        let document = json!({
            "mediaType": "application/vnd.dev.sigstore.trustedroot+json;version=0.1",
            "certificateAuthorities": [{
                "certChain": {"certificates": [{"rawBytes": b64(&eco.root_der)}]},
                "validFor": {"start": start},
            }],
            "tlogs": [{
                "logId": {"keyId": b64(&sigstore::sha256(&log_spki))},
                "publicKey": {"rawBytes": b64(&log_spki), "validFor": {"start": start}},
            }],
        });
        let trust_root = dir.join("trusted_root.json");
        std::fs::write(&trust_root, document.to_string()).unwrap();
        Self { eco, trust_root }
    }

    /// A Sigstore bundle v0.3 over `bytes`, as `cosign sign-blob
    /// --new-bundle-format` writes one.
    fn sign(&self, bytes: &[u8]) -> String {
        sigstore::valid_bundle_json(
            &self.eco,
            sigstore::sha256(bytes),
            chrono::Utc::now() - chrono::Duration::minutes(1),
            7,
        )
    }
}

// ── HTTP ─────────────────────────────────────────────────────────────────────

#[derive(Clone)]
enum Auth {
    Session(String),
    Bearer(String),
}

struct Platform {
    url: String,
    http: reqwest::Client,
}

impl Platform {
    async fn call(
        &self,
        method: reqwest::Method,
        path: &str,
        auth: &Auth,
        body: Option<Value>,
    ) -> (u16, Value) {
        let mut req = self.http.request(method, format!("{}{path}", self.url));
        req = match auth {
            Auth::Session(c) => req.header("cookie", format!("fc_session={c}")),
            Auth::Bearer(t) => req.bearer_auth(t),
        };
        if let Some(body) = body {
            req = req.json(&body);
        }
        let resp = req.send().await.unwrap();
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap();
        (
            status,
            serde_json::from_str(&text).unwrap_or(Value::String(text)),
        )
    }

    async fn get(&self, path: &str, auth: &Auth) -> (u16, Value) {
        self.call(reqwest::Method::GET, path, auth, None).await
    }

    async fn post(&self, path: &str, auth: &Auth, body: Value) -> (u16, Value) {
        self.call(reqwest::Method::POST, path, auth, Some(body))
            .await
    }

    async fn put(&self, path: &str, auth: &Auth, body: Value) -> (u16, Value) {
        self.call(reqwest::Method::PUT, path, auth, Some(body))
            .await
    }

    async fn client_credentials(&self, id: &str, secret: &str) -> String {
        let resp = self
            .http
            .post(format!("{}/oauth/token", self.url))
            .form(&[
                ("grant_type", "client_credentials"),
                ("client_id", id),
                ("client_secret", secret),
            ])
            .send()
            .await
            .unwrap();
        let status = resp.status();
        let body: Value = resp.json().await.unwrap();
        assert!(status.is_success(), "client_credentials: {status} {body}");
        body["access_token"].as_str().unwrap().to_string()
    }

    /// `PUT /api/functions/{address}/artifacts/{digest}`, as `fc-dev fn
    /// publish` uploads: the `platform://` ref and the digest.
    async fn upload(&self, auth: &Auth, address: &str, bytes: &[u8]) -> (String, String) {
        let digest = format!("sha256:{}", hex::encode(sha2::Sha256::digest(bytes)));
        let Auth::Bearer(token) = auth else {
            panic!("uploads use a bearer token")
        };
        let resp = self
            .http
            .put(format!(
                "{}/api/functions/{address}/artifacts/{digest}",
                self.url
            ))
            .bearer_auth(token)
            .header("content-type", "application/octet-stream")
            .body(bytes.to_vec())
            .send()
            .await
            .unwrap();
        let status = resp.status().as_u16();
        let body: Value = resp.json().await.unwrap();
        assert_eq!(status, 200, "upload to {address}: {body}");
        (body["artifactRef"].as_str().unwrap().to_string(), digest)
    }

    /// A service account holding `roles`: its `client_credentials` pair.
    /// `all_applications`: what a publishing pipeline needs to reach the
    /// function's application (`fc-dev`'s own publisher has it too).
    async fn service_account(
        &self,
        admin: &Auth,
        code: &str,
        roles: &[&str],
        all_applications: bool,
    ) -> (String, String) {
        let (status, body) = self
            .post(
                "/api/service-accounts",
                admin,
                json!({"code": code, "name": code, "allApplications": all_applications}),
            )
            .await;
        assert_eq!(status, 201, "create service account {code}: {body}");
        let id = body["serviceAccount"]["id"].as_str().unwrap();
        let (status, assigned) = self
            .put(
                &format!("/api/service-accounts/{id}/roles"),
                admin,
                json!({"roles": roles}),
            )
            .await;
        assert_eq!(status, 200, "assign {roles:?} to {code}: {assigned}");
        (
            body["oauth"]["clientId"].as_str().unwrap().to_string(),
            body["oauth"]["clientSecret"].as_str().unwrap().to_string(),
        )
    }
}

/// Polls `check` every 500 ms until it answers `Some`, or panics with `what`
/// and the processes' logs after `timeout`.
async fn wait_for<T, F, Fut>(what: &str, timeout: Duration, procs: &[&Proc], mut check: F) -> T
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Option<T>>,
{
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(value) = check().await {
            return value;
        }
        if Instant::now() > deadline {
            let logs: Vec<String> = procs
                .iter()
                .map(|p| format!("── {} ──\n{}", p.name, p.log_text()))
                .collect();
            panic!("timed out waiting for {what}\n{}", logs.join("\n"));
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

// ── The processes' environments ──────────────────────────────────────────────

fn platform_command(env: &BTreeMap<&str, String>, dir: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fc-server"));
    command
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .envs(env)
        .current_dir(dir);
    command
}

fn rust_host(
    host_id: &str,
    pool: &str,
    platform_url: &str,
    creds: &(String, String),
    trust_root: &Path,
    dir: &Path,
) -> (Command, u16) {
    let port = free_port();
    let cache = dir.join(format!("cache-{host_id}"));
    std::fs::create_dir_all(&cache).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_fc-server"));
    command
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("RUST_LOG", "info")
        .env("FC_PLATFORM_ENABLED", "false")
        .env("FC_FUNCTION_HOST_ENABLED", "true")
        .env("FC_FN_POOL", pool)
        .env("FC_FN_PLATFORM_URL", platform_url)
        .env("FC_FN_CLIENT_ID", &creds.0)
        .env("FC_FN_CLIENT_SECRET", &creds.1)
        .env("FC_FN_HOST_ID", host_id)
        .env("FC_FN_SIGNATURES", "required")
        .env("FC_FN_TRUST_ROOT", trust_root)
        .env("FC_FN_CACHE_DIR", &cache)
        .env("FC_FN_PORT", port.to_string())
        .env("FC_METRICS_PORT", free_port().to_string())
        .env("FC_FN_PUBLIC_PORT", "off")
        .current_dir(dir);
    (command, port)
}

async fn host_row(pool: &sqlx::PgPool, host_id: &str) -> Option<(String, Value, Option<Value>)> {
    sqlx::query_as::<_, (String, Value, Option<Value>)>(
        "SELECT pool, loaded, runtimes FROM fnr_hosts WHERE id = $1",
    )
    .bind(host_id)
    .fetch_optional(pool)
    .await
    .unwrap()
}

/// The entry a host reported for `(address, version)`, from its row.
fn reported(loaded: &Value, address: &str, version: i64) -> Option<Value> {
    loaded
        .as_array()?
        .iter()
        .find_map(|e| (e["address"] == address && e["version"] == version).then(|| e.clone()))
}

/// The dispatch job an ingested event (by its subject) fanned out to.
async fn wait_job(db: &sqlx::PgPool, subject: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT id FROM msg_dispatch_jobs WHERE code = $1 AND subject = $2")
                .bind(REQUESTED)
                .bind(subject)
                .fetch_optional(db)
                .await
                .unwrap();
        if let Some((id,)) = row {
            return id;
        }
        assert!(Instant::now() < deadline, "no dispatch job for {subject}");
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// The router's call for one job: `/api/dispatch/process` with the job's
/// HMAC token. The platform delivers the webhook and answers `{ack}`.
async fn process(http: &reqwest::Client, platform_url: &str, job_id: &str) -> Value {
    let token = fc_platform::scheduler::auth::DispatchAuthService::from_app_key(APP_KEY)
        .unwrap()
        .sign(job_id);
    http.post(format!("{platform_url}/api/dispatch/process"))
        .bearer_auth(token)
        .json(&json!({"messageId": job_id}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs Docker, Java 25 and Maven"]
async fn a_jvm_function_published_on_the_rust_platform_runs_on_the_java_host() {
    if let Some(reason) = missing_prerequisite() {
        eprintln!("[jvm-e2e] SKIPPED: {reason}");
        return;
    }
    let work = tempfile::tempdir().unwrap();
    let jars = java_jars(work.path()).unwrap_or_else(|e| panic!("the Java build: {e}"));

    let container = Postgres::default()
        .with_db_name("flowcatalyst")
        .with_user("postgres")
        .with_password("postgres")
        .start()
        .await
        .expect("start postgres");
    let pg_port = container.get_host_port_ipv4(5432).await.unwrap();
    let database_url = format!("postgresql://postgres:postgres@127.0.0.1:{pg_port}/flowcatalyst");

    let signing = Signing::new(work.path());
    let store = work.path().join("artifacts");
    std::fs::create_dir_all(&store).unwrap();

    let platform_port = free_port();
    let java_port = free_port();
    let platform_url = format!("http://127.0.0.1:{platform_port}");
    let java_url = format!("http://127.0.0.1:{java_port}");

    // ── The platform ─────────────────────────────────────────────────────
    let env: BTreeMap<&str, String> = [
        ("RUST_LOG", "info".to_string()),
        ("FC_DATABASE_URL", database_url.clone()),
        ("FC_API_PORT", platform_port.to_string()),
        ("FC_METRICS_PORT", free_port().to_string()),
        ("FLOWCATALYST_APP_KEY", APP_KEY.into()),
        ("FC_JWT_ISSUER", platform_url.clone()),
        ("FC_EXTERNAL_BASE_URL", platform_url.clone()),
        ("FLOWCATALYST_BOOTSTRAP_ADMIN_EMAIL", ADMIN_EMAIL.into()),
        (
            "FLOWCATALYST_BOOTSTRAP_ADMIN_PASSWORD",
            ADMIN_PASSWORD.into(),
        ),
        ("FC_FN_SIGNATURES", "required".into()),
        ("FC_FN_TRUST_ROOT", signing.trust_root.display().to_string()),
        (
            "FC_FN_ARTIFACT_STORE",
            format!("file://{}", store.display()),
        ),
        // Every pool's functions are reached at the Java host here.
        ("FC_FN_POOL_URL", java_url.clone()),
        ("FC_PLATFORM_ENABLED", "true".into()),
        ("FC_STREAM_PROCESSOR_ENABLED", "true".into()),
        ("FC_STREAM_FAN_OUT_SUBS_REFRESH_SECS", "1".into()),
        ("FC_SCHEDULER_ENABLED", "false".into()),
        ("FC_ROUTER_ENABLED", "false".into()),
        ("FC_OUTBOX_ENABLED", "false".into()),
        // Delivers the firings of the probe's schedule.
        ("FC_SCHEDULED_JOB_ENABLED", "true".into()),
        ("FC_STANDBY_ENABLED", "false".into()),
    ]
    .into_iter()
    .collect();
    let mut platform_proc =
        Proc::spawn("fc-server (platform)", platform_command(&env, work.path()));
    let platform = Platform {
        url: platform_url.clone(),
        http: reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap(),
    };
    wait_for(
        "the platform's /health",
        Duration::from_secs(180),
        &[&platform_proc],
        || async {
            let r = platform
                .http
                .get(format!("{platform_url}/health"))
                .send()
                .await
                .ok()?;
            r.status().is_success().then_some(())
        },
    )
    .await;
    platform_proc.assert_running();
    let db = sqlx::PgPool::connect(&database_url).await.unwrap();

    // ── The operator's set-up: an application, its event types and roles,
    //    and three service accounts ────────────────────────────────────────
    let login = platform
        .http
        .post(format!("{platform_url}/auth/login"))
        .json(&json!({"email": ADMIN_EMAIL, "password": ADMIN_PASSWORD}))
        .send()
        .await
        .unwrap();
    assert_eq!(login.status(), 200, "{}", platform_proc.log_text());
    let session = login
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find_map(|c| c.strip_prefix("fc_session="))
        .map(|c| c.split(';').next().unwrap().to_string())
        .expect("session cookie");
    let admin = Auth::Session(session);

    let (status, body) = platform
        .post(
            "/api/applications",
            &admin,
            json!({"code": "hello", "name": "Hello"}),
        )
        .await;
    assert_eq!(status, 201, "create application: {body}");
    // The application's service account signs the webhook deliveries its
    // function's subscriptions receive (APPLICATION_SIGNING_SECRET_REQUIRED
    // otherwise).
    let application_id = body["id"].as_str().unwrap().to_string();
    let (status, body) = platform
        .post(
            &format!("/api/applications/{application_id}/provision-service-account"),
            &admin,
            json!({}),
        )
        .await;
    assert!(
        (200..300).contains(&status),
        "provision the application's service account: {status} {body}"
    );
    for (code, name) in [(REQUESTED, "Greeting requested"), (SENT, "Greeting sent")] {
        let (status, body) = platform
            .post(
                "/api/event-types",
                &admin,
                json!({"code": code, "name": name}),
            )
            .await;
        assert_eq!(status, 201, "create event type {code}: {body}");
    }
    let (status, body) = platform
        .post(
            "/api/roles",
            &admin,
            json!({"applicationCode": "hello", "roleName": "greeter", "displayName": "Greeter",
                   "permissions": [GREET]}),
        )
        .await;
    assert!(
        status == 201 || status == 200,
        "create role: {status} {body}"
    );
    let greeter_role = body["name"].as_str().unwrap_or("hello:greeter").to_string();

    let host_creds = platform
        .service_account(
            &admin,
            "jvm-e2e-fn-host",
            &["platform:function-host"],
            false,
        )
        .await;
    let publisher_creds = platform
        .service_account(
            &admin,
            "jvm-e2e-publisher",
            &["platform:function-publisher"],
            true,
        )
        .await;
    let greeter_creds = platform
        .service_account(&admin, "jvm-e2e-greeter", &[greeter_role.as_str()], false)
        .await;
    let outsider_creds = platform
        .service_account(
            &admin,
            "jvm-e2e-outsider",
            &["platform:function-publisher"],
            false,
        )
        .await;

    // ── The Java host, pool `jvm` ─────────────────────────────────────────
    let java_cache = work.path().join("java-cache");
    std::fs::create_dir_all(&java_cache).unwrap();
    let mut java = Command::new("java");
    // The flags the host image's entrypoint passes (javalin
    // `function-host/docker/entrypoint.sh`): the build uses preview features.
    java.args([
        "--enable-preview",
        "--enable-native-access=ALL-UNNAMED",
        "-jar",
        jars.host.to_str().unwrap(),
    ])
    .env("FC_FN_POOL", POOL)
    .env("FC_FN_PLATFORM_URL", &platform_url)
    .env("FC_FN_CLIENT_ID", &host_creds.0)
    .env("FC_FN_CLIENT_SECRET", &host_creds.1)
    .env("FC_FN_HOST_ID", JAVA_HOST_ID)
    .env("FC_FN_SIGNATURES", "required")
    .env("FC_FN_TRUST_ROOT", &signing.trust_root)
    .env("FC_FN_CACHE_DIR", &java_cache)
    .env("FC_FN_PORT", java_port.to_string())
    .env("FC_METRICS_PORT", free_port().to_string())
    .env("FC_FN_PUBLIC_PORT", "off")
    .current_dir(work.path());
    let mut java_proc = Proc::spawn("java function host", java);
    wait_for(
        "the Java host's first heartbeat",
        Duration::from_secs(60),
        &[&java_proc],
        || async { host_row(&db, JAVA_HOST_ID).await },
    )
    .await;
    java_proc.assert_running();
    let (pool, _, runtimes) = host_row(&db, JAVA_HOST_ID).await.unwrap();
    assert_eq!(pool, POOL);
    // Java's heartbeat carries no `runtimes`: the platform reads it as
    // "unknown", never as "supports nothing".
    assert_eq!(runtimes, None, "the Java host reports no runtimes");

    // ── Publish, as `fc-dev fn deploy` does ──────────────────────────────
    let publisher = Auth::Bearer(
        platform
            .client_credentials(&publisher_creds.0, &publisher_creds.1)
            .await,
    );
    let (status, body) = platform
        .post(
            "/api/functions",
            &admin,
            json!({"applicationCode": "hello", "serviceName": "default", "name": "hello",
                   "runtime": "jvm"}),
        )
        .await;
    assert_eq!(status, 201, "create function: {body}");
    let function_id = body["id"].as_str().unwrap().to_string();

    // The owner's signer policy: the pipeline's identity may publish every
    // runtime this check uses.
    let (status, body) = platform
        .put(
            "/api/function-policies/platform",
            &admin,
            json!({"signers": [{"issuer": SIGNER_ISSUER, "subject": SIGNER_SUBJECT,
                                "runtimes": ["jvm", "wasm", "component"]}]}),
        )
        .await;
    assert!(
        (200..300).contains(&status),
        "signer policy: {status} {body}"
    );
    let jar = std::fs::read(&jars.hello).unwrap();
    let (artifact_ref, digest) = platform.upload(&publisher, ADDRESS, &jar).await;

    let mut manifest = jars.manifest.clone();
    manifest.as_object_mut().unwrap().remove("$schema");
    manifest["pool"] = json!(POOL);
    let (status, body) = platform
        .post(
            &format!("/api/functions/{ADDRESS}/manifest/check"),
            &publisher,
            json!({"manifest": manifest}),
        )
        .await;
    assert_eq!(status, 200, "manifest check: {body}");
    assert_eq!(body["valid"], true, "{body}");
    eprintln!(
        "[jvm-e2e] manifest check plan for pool jvm: {}",
        body["plan"]
    );
    // Unsigned: refused.
    let (status, body) = platform
        .post(
            &format!("/api/functions/{ADDRESS}/versions"),
            &publisher,
            json!({"artifactRef": artifact_ref, "digest": digest, "manifest": manifest}),
        )
        .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (400, Some("SIGNATURE_REQUIRED")),
        "{body}"
    );
    let (status, body) = platform
        .post(
            &format!("/api/functions/{ADDRESS}/versions"),
            &publisher,
            json!({"artifactRef": artifact_ref, "digest": digest, "manifest": manifest,
                   "signatureBundle": signing.sign(&jar)}),
        )
        .await;
    assert_eq!(status, 201, "publish: {body}");
    assert_eq!(body["version"], 1);
    eprintln!("[jvm-e2e] published: {body}");

    // Config and a secret, before it goes live.
    let (status, body) = platform
        .put(
            &format!("/api/functions/{ADDRESS}/config"),
            &admin,
            json!({"values": {"GREETING": "Howdy"}}),
        )
        .await;
    assert!((200..300).contains(&status), "config: {status} {body}");
    let (status, body) = platform
        .put(
            &format!("/api/functions/{ADDRESS}/secrets/API_KEY"),
            &publisher,
            json!({"value": "k-jvm-e2e-5b2d"}),
        )
        .await;
    assert!((200..300).contains(&status), "secret: {status} {body}");

    // ── 1. The Java host registers the candidate: READY ───────────────────
    wait_for(
        "version 1 READY (the Java host registered it)",
        Duration::from_secs(60),
        &[&java_proc, &platform_proc],
        || async {
            let (_, v) = platform
                .get(&format!("/api/functions/{ADDRESS}/versions/1"), &publisher)
                .await;
            (v["state"] == "READY").then_some(())
        },
    )
    .await;
    let (ready_host,): (String,) = sqlx::query_as(
        "SELECT data->>'hostId' FROM msg_events WHERE type = 'platform:function:version:ready' \
         AND subject = $1",
    )
    .bind(format!("platform.function.{function_id}"))
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(ready_host, JAVA_HOST_ID);

    // ── 2. Promote; the Java host serves it ──────────────────────────────
    let (status, body) = platform
        .put(
            &format!("/api/functions/{ADDRESS}/aliases/live"),
            &publisher,
            json!({"version": 1, "expectedVersion": 0}),
        )
        .await;
    assert_eq!(status, 200, "promote: {body}");

    let http = reqwest::Client::new();
    let fn_url = format!("{java_url}/functions/{ADDRESS}");
    let health = wait_for(
        "the Java host serving /healthz",
        Duration::from_secs(60),
        &[&java_proc],
        || async {
            let r = http.get(format!("{fn_url}/healthz")).send().await.ok()?;
            if r.status() == 200 {
                r.json::<Value>().await.ok()
            } else {
                None
            }
        },
    )
    .await;
    assert_eq!(health, json!({"status": "ok"}));

    wait_for(
        "the Java host reporting 1 LOADED",
        Duration::from_secs(30),
        &[&java_proc],
        || async {
            let (_, loaded, _) = host_row(&db, JAVA_HOST_ID).await?;
            (reported(&loaded, ADDRESS, 1)?["state"] == "LOADED").then_some(())
        },
    )
    .await;

    // `auth: platform`: the Java host verifies a Rust-minted token against
    // the platform's discovery document and JWKS.
    let greeter = platform
        .client_credentials(&greeter_creds.0, &greeter_creds.1)
        .await;
    let r = http
        .get(format!("{fn_url}/api/hello/Ada"))
        .bearer_auth(&greeter)
        .send()
        .await
        .unwrap();
    let status = r.status().as_u16();
    let body: Value = r.json().await.unwrap_or(Value::Null);
    assert_eq!(
        status,
        200,
        "platform-auth call: {body}\n{}",
        java_proc.log_text()
    );
    assert_eq!(body["message"], "hello, Ada!");
    assert!(
        body["principalId"].as_str().is_some_and(|p| !p.is_empty()),
        "{body}"
    );
    let outsider = platform
        .client_credentials(&outsider_creds.0, &outsider_creds.1)
        .await;
    let r = http
        .get(format!("{fn_url}/api/hello/Ada"))
        .bearer_auth(&outsider)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403, "no {GREET}: the function refuses");
    let r = http
        .get(format!("{fn_url}/api/hello/Ada"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401, "no token: the host refuses");

    // ── 3. An event reaches the function through the platform's delivery ─
    let (status, body) = platform
        .post(
            "/api/events/batch",
            &admin,
            json!({"items": [{
                "type": REQUESTED,
                "source": "jvm-e2e",
                "subject": "greeting-1",
                "data": {"name": "Ada"},
                "correlationId": "corr-jvm-e2e",
                "deduplicationId": "jvm-e2e-requested-1",
            }]}),
        )
        .await;
    assert!((200..300).contains(&status), "ingest: {status} {body}");
    let (job_id, target): (String, String) = wait_for(
        "the subscription's dispatch job",
        Duration::from_secs(60),
        &[&platform_proc],
        || async {
            sqlx::query_as::<_, (String, String)>(
                "SELECT id, target_url FROM msg_dispatch_jobs WHERE code = $1 AND subject = 'greeting-1'",
            )
            .bind(REQUESTED)
            .fetch_optional(&db)
            .await
            .unwrap()
        },
    )
    .await;
    assert_eq!(
        target,
        format!("{java_url}/functions/{ADDRESS}/events/greeting-requested"),
        "the promote wired the subscription to the pool's URL"
    );
    let body = process(&http, &platform_url, &job_id).await;
    assert_eq!(body["ack"], true, "{body}");
    let (job_status,): (String,) =
        sqlx::query_as("SELECT status FROM msg_dispatch_jobs WHERE id = $1")
            .bind(&job_id)
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(job_status, "COMPLETED", "{}", java_proc.log_text());

    let (source, subject, data, correlation): (String, Option<String>, Value, Option<String>) =
        wait_for(
            "the function's emitted event",
            Duration::from_secs(30),
            &[&java_proc],
            || async {
                sqlx::query_as(
                    "SELECT source, subject, data, correlation_id FROM msg_events \
                 WHERE type = $1 AND subject = 'greeting-1'",
                )
                .bind(SENT)
                .fetch_optional(&db)
                .await
                .unwrap()
            },
        )
        .await;
    assert_eq!(source, format!("function:{ADDRESS}"));
    assert_eq!(subject.as_deref(), Some("greeting-1"));
    assert_eq!(
        data,
        json!({"name": "Ada", "greeting": "Howdy, Ada!"}),
        "the config value"
    );
    assert_eq!(correlation.as_deref(), Some("corr-jvm-e2e"));
    assert!(
        java_proc.count("apiKeyPresent=true") >= 1,
        "the function saw its secret:\n{}",
        java_proc.log_text()
    );
    assert!(
        !java_proc.log_text().contains("k-jvm-e2e-5b2d"),
        "the secret's value is never logged"
    );

    // A settings edit reaches the loaded function: the desired state's
    // settings change, the Java host reloads it, the next delivery sees it.
    let (status, body) = platform
        .put(
            &format!("/api/functions/{ADDRESS}/config"),
            &admin,
            json!({"values": {"GREETING": "Hi"}}),
        )
        .await;
    assert!((200..300).contains(&status), "config: {status} {body}");
    let mut attempt = 0;
    let greeting: String = wait_for(
        "the Java host serving the new GREETING",
        Duration::from_secs(90),
        &[&java_proc, &platform_proc],
        || {
            attempt += 1;
            let dedup = format!("jvm-e2e-requested-bob-{attempt}");
            let platform = &platform;
            let db = &db;
            let admin = admin.clone();
            let http = http.clone();
            let platform_url = platform_url.clone();
            async move {
                let (status, _) = platform
                    .post(
                        "/api/events/batch",
                        &admin,
                        json!({"items": [{"type": REQUESTED, "source": "jvm-e2e",
                                          "subject": dedup, "data": {"name": "Bob"},
                                          "deduplicationId": dedup}]}),
                    )
                    .await;
                assert!((200..300).contains(&status));
                let job_id = wait_job(db, &dedup).await;
                process(&http, &platform_url, &job_id).await;
                let (data,): (Value,) =
                    sqlx::query_as("SELECT data FROM msg_events WHERE type = $1 AND subject = $2")
                        .bind(SENT)
                        .bind(&dedup)
                        .fetch_optional(db)
                        .await
                        .unwrap()?;
                let greeting = data["greeting"].as_str()?.to_string();
                (greeting == "Hi, Bob!").then_some(greeting)
            }
        },
    )
    .await;
    assert_eq!(greeting, "Hi, Bob!");

    // A pinned version (`<address>:<n>`) needs `platform:function:version:invoke`.
    let publisher_token = match &publisher {
        Auth::Bearer(t) => t.clone(),
        _ => unreachable!(),
    };
    let r = http
        .get(format!("{java_url}/functions/{ADDRESS}:1/healthz"))
        .bearer_auth(&publisher_token)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "pinned call with version:invoke");
    let r = http
        .get(format!("{java_url}/functions/{ADDRESS}:1/healthz"))
        .bearer_auth(&greeter)
        .send()
        .await
        .unwrap();
    eprintln!(
        "[jvm-e2e] a pinned call without version:invoke: {}",
        r.status()
    );
    assert!(r.status() == 403 || r.status() == 401, "{}", r.status());

    // ── 3b. A scheduled-job firing reaches a JVM function ────────────────
    // The probe's `/tick` parses the firing with Java's `Webhook.schedule`.
    let ticker = "hello.default.ticker";
    let (status, body) = platform
        .post(
            "/api/functions",
            &admin,
            json!({"applicationCode": "hello", "serviceName": "default", "name": "ticker",
                   "runtime": "jvm"}),
        )
        .await;
    assert_eq!(status, 201, "create the probe function: {body}");
    let probe = std::fs::read(&jars.probe).unwrap();
    let (probe_ref, probe_digest) = platform.upload(&publisher, ticker, &probe).await;
    let (status, body) = platform
        .post(
            &format!("/api/functions/{ticker}/versions"),
            &publisher,
            json!({"artifactRef": probe_ref, "digest": probe_digest,
                   "signatureBundle": signing.sign(&probe),
                   "manifest": {"runtime": "jvm", "entrypoint": "e2e.probe.ScheduleProbe",
                                "pool": POOL, "warm": true,
                                "endpoints": [{"path": "/tick", "auth": "webhook"}],
                                // Once a year: the check fires it by hand.
                                "schedules": [{"cron": "0 0 1 1 *", "path": "/tick"}]}}),
        )
        .await;
    assert_eq!(status, 201, "publish the probe: {body}");
    wait_for(
        "the probe READY",
        Duration::from_secs(60),
        &[&java_proc],
        || async {
            let (_, v) = platform
                .get(&format!("/api/functions/{ticker}/versions/1"), &publisher)
                .await;
            (v["state"] == "READY").then_some(())
        },
    )
    .await;
    let (status, body) = platform
        .put(
            &format!("/api/functions/{ticker}/aliases/live"),
            &publisher,
            json!({"version": 1, "expectedVersion": 0}),
        )
        .await;
    assert_eq!(status, 200, "promote the probe: {body}");
    wait_for(
        "the Java host loading the probe",
        Duration::from_secs(60),
        &[&java_proc],
        || async {
            let (_, loaded, _) = host_row(&db, JAVA_HOST_ID).await?;
            (reported(&loaded, ticker, 1)?["state"] == "LOADED").then_some(())
        },
    )
    .await;
    let (job_id, job_target): (String, String) =
        sqlx::query_as("SELECT id, target_url FROM msg_scheduled_jobs WHERE target_url LIKE $1")
            .bind(format!("%/functions/{ticker}/%"))
            .fetch_one(&db)
            .await
            .expect("the promote created the probe's scheduled job");
    assert_eq!(job_target, format!("{java_url}/functions/{ticker}/tick"));
    let (status, fired) = platform
        .post(
            &format!("/api/scheduled-jobs/{job_id}/fire"),
            &admin,
            json!({"correlationId": "corr-jvm-e2e-tick"}),
        )
        .await;
    assert_eq!(status, 202, "fire: {fired}");
    let instance_id = fired["instanceId"].as_str().unwrap().to_string();
    let (status, delivery_error): (String, Option<String>) = wait_for(
        "the firing delivered",
        Duration::from_secs(60),
        &[&platform_proc, &java_proc],
        || async {
            let row: (String, Option<String>) = sqlx::query_as(
                "SELECT status, delivery_error FROM msg_scheduled_job_instances WHERE id = $1",
            )
            .bind(&instance_id)
            .fetch_optional(&db)
            .await
            .unwrap()?;
            (row.0 != "QUEUED" && row.0 != "IN_FLIGHT").then_some(row)
        },
    )
    .await;
    assert_eq!(
        status,
        "DELIVERED",
        "{delivery_error:?}\n{}",
        java_proc.log_text()
    );
    let (data, correlation): (Value, Option<String>) = sqlx::query_as(
        "SELECT data, correlation_id FROM msg_events WHERE type = $1 AND source = $2",
    )
    .bind(SENT)
    .bind(format!("function:{ticker}"))
    .fetch_one(&db)
    .await
    .expect("the probe's event");
    assert_eq!(data["triggerKind"], "MANUAL", "{data}");
    assert_eq!(data["concurrent"], false, "{data}");
    assert_eq!(correlation.as_deref(), Some("corr-jvm-e2e-tick"));

    // ── 4. Mixed pools ───────────────────────────────────────────────────
    // (a) A Rust host joins pool `jvm`: the jar is FAILED RUNTIME_UNSUPPORTED
    //     there, never fetched; the Java host keeps it LOADED.
    let (command, rust_port) = rust_host(
        RUST_HOST_ID,
        POOL,
        &platform_url,
        &host_creds,
        &signing.trust_root,
        work.path(),
    );
    let mut rust_proc = Proc::spawn("rust function host (pool jvm)", command);
    let failed = wait_for(
        "the Rust host's report",
        Duration::from_secs(60),
        &[&rust_proc],
        || async {
            let (_, loaded, _) = host_row(&db, RUST_HOST_ID).await?;
            reported(&loaded, ADDRESS, 1)
        },
    )
    .await;
    assert_eq!(failed["state"], "FAILED", "{failed}");
    assert_eq!(failed["error"], "RUNTIME_UNSUPPORTED", "{failed}");
    let (_, status_body) = platform
        .get(&format!("/api/functions/{ADDRESS}/status"), &publisher)
        .await;
    eprintln!("[jvm-e2e] status with a mixed pool: {status_body}");
    let hosts = status_body["hosts"].as_array().expect("hosts");
    let by_id = |id: &str| hosts.iter().find(|h| h["hostId"] == id).cloned();
    assert_eq!(
        by_id(JAVA_HOST_ID).unwrap()["loaded"],
        json!([{"version": 1, "state": "LOADED"}])
    );
    let rust_status = by_id(RUST_HOST_ID).unwrap();
    assert_eq!(rust_status["loaded"][0]["state"], "FAILED", "{rust_status}");
    assert_eq!(
        rust_status["loaded"][0]["error"], "RUNTIME_UNSUPPORTED",
        "{rust_status}"
    );
    // The version stays live and READY: a FAILED report never fails it.
    let (_, v1) = platform
        .get(&format!("/api/functions/{ADDRESS}/versions/1"), &publisher)
        .await;
    assert_eq!(v1["state"], "READY", "{v1}");
    // A request that lands on the Rust host is not served there.
    let r = http
        .get(format!(
            "http://127.0.0.1:{rust_port}/functions/{ADDRESS}/healthz"
        ))
        .send()
        .await
        .unwrap();
    let rust_answer = (r.status().as_u16(), r.text().await.unwrap_or_default());
    eprintln!("[jvm-e2e] the Rust host answers a jvm function's call with {rust_answer:?}");
    assert_ne!(rust_answer.0, 200);
    // No crash loop: still running after two more cycles, and the failure
    // logged once.
    tokio::time::sleep(Duration::from_secs(32)).await;
    rust_proc.assert_running();
    assert_eq!(
        rust_proc.count(&format!(
            r#""msg":"failed to prepare a function version","address":"{ADDRESS}""#
        )),
        1,
        "{}",
        rust_proc.log_text()
    );
    // The platform's publish-time view of a mixed pool: the Rust host
    // reports runtimes without `jvm`, the Java host reports none, so
    // support is unknown and the plan warns.
    let (_, check) = platform
        .post(
            &format!("/api/functions/{ADDRESS}/manifest/check"),
            &publisher,
            json!({"manifest": manifest}),
        )
        .await;
    eprintln!(
        "[jvm-e2e] manifest check plan for the mixed pool: {}",
        check["plan"]
    );
    assert_eq!(check["valid"], true, "{check}");

    // (b) A `jvm` function in a pool served only by Rust hosts is refused
    //     at publish.
    let (command, _) = rust_host(
        RUST_ONLY_HOST_ID,
        "wasm",
        &platform_url,
        &host_creds,
        &signing.trust_root,
        work.path(),
    );
    let rust_only = Proc::spawn("rust function host (pool wasm)", command);
    wait_for(
        "the pool-wasm Rust host's heartbeat",
        Duration::from_secs(60),
        &[&rust_only],
        || async { host_row(&db, RUST_ONLY_HOST_ID).await },
    )
    .await;
    // A second jvm function, so the refusal is the pool's and not the
    // digest's (version 1 of `hello` already holds this jar).
    let misplaced = "hello.default.misplaced";
    let (status, body) = platform
        .post(
            "/api/functions",
            &admin,
            json!({"applicationCode": "hello", "serviceName": "default", "name": "misplaced",
                   "runtime": "jvm"}),
        )
        .await;
    assert_eq!(status, 201, "create function: {body}");
    let (misplaced_ref, _) = platform.upload(&publisher, misplaced, &jar).await;
    let mut wasm_manifest = manifest.clone();
    wasm_manifest["pool"] = json!("wasm");
    let (status, body) = platform
        .post(
            &format!("/api/functions/{misplaced}/versions"),
            &publisher,
            json!({"artifactRef": misplaced_ref, "digest": digest, "manifest": wasm_manifest,
                   "signatureBundle": signing.sign(&jar)}),
        )
        .await;
    eprintln!("[jvm-e2e] publishing a jvm function to a Rust-only pool: {status} {body}");
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["error"], "POOL_RUNTIME_UNSUPPORTED", "{body}");

    // (c) A Rust `component` placed in the Java host's pool: the Java host
    //     cannot read the runtime and reports it FAILED.
    let (status, body) = platform
        .post(
            "/api/functions",
            &admin,
            json!({"applicationCode": "hello", "serviceName": "default", "name": "component",
                   "runtime": "component"}),
        )
        .await;
    assert_eq!(status, 201, "create component function: {body}");
    let component_address = "hello.default.component";
    let component =
        std::fs::read(repo_root().join("crates/fc-fnhost-core/tests/fixtures/wasm/hello.wasm"))
            .unwrap();
    let (component_ref, component_digest) = platform
        .upload(&publisher, component_address, &component)
        .await;
    let (status, body) = platform
        .post(
            &format!("/api/functions/{component_address}/versions"),
            &publisher,
            json!({"artifactRef": component_ref, "digest": component_digest,
                   "signatureBundle": signing.sign(&component),
                   "manifest": {"runtime": "component", "pool": POOL,
                                "endpoints": [{"path": "/*", "auth": "none"}]}}),
        )
        .await;
    assert_eq!(status, 201, "publish the component to pool jvm: {body}");
    let java_report = wait_for(
        "the Java host's report on the component",
        Duration::from_secs(60),
        &[&java_proc],
        || async {
            let (_, loaded, _) = host_row(&db, JAVA_HOST_ID).await?;
            reported(&loaded, component_address, 1)
        },
    )
    .await;
    eprintln!("[jvm-e2e] the Java host reports a component as {java_report}");
    assert_eq!(java_report["state"], "FAILED", "{java_report}");
    // The Rust host in the same pool registers it, so it still goes READY.
    wait_for(
        "the component READY via the Rust host",
        Duration::from_secs(60),
        &[&rust_proc],
        || async {
            let (_, v) = platform
                .get(
                    &format!("/api/functions/{component_address}/versions/1"),
                    &publisher,
                )
                .await;
            (v["state"] == "READY").then_some(())
        },
    )
    .await;

    // (d) A Rust `wasm` function (a WASI component; the Rust platform's
    //     `wasm` and `component` are the same artifact) in the Java host's
    //     pool: Java reads `wasm` as its own Extism core-module runtime and
    //     refuses the component at load.
    let (status, body) = platform
        .post(
            "/api/functions",
            &admin,
            json!({"applicationCode": "hello", "serviceName": "default", "name": "rustwasm",
                   "runtime": "wasm"}),
        )
        .await;
    assert_eq!(status, 201, "create wasm function: {body}");
    let wasm_address = "hello.default.rustwasm";
    let (wasm_ref, wasm_digest) = platform.upload(&publisher, wasm_address, &component).await;
    let (status, body) = platform
        .post(
            &format!("/api/functions/{wasm_address}/versions"),
            &publisher,
            json!({"artifactRef": wasm_ref, "digest": wasm_digest,
                   "signatureBundle": signing.sign(&component),
                   "manifest": {"runtime": "wasm", "entrypoint": "wasi_http_incoming_handler",
                                "pool": POOL, "warm": true,
                                "endpoints": [{"path": "/*", "auth": "none"}]}}),
        )
        .await;
    assert_eq!(
        status, 201,
        "publish a Rust wasm function to pool jvm: {body}"
    );
    let java_report = wait_for(
        "the Java host's report on the wasm function",
        Duration::from_secs(60),
        &[&java_proc],
        || async {
            let (_, loaded, _) = host_row(&db, JAVA_HOST_ID).await?;
            reported(&loaded, wasm_address, 1)
        },
    )
    .await;
    // A candidate is only fetched and verified, never loaded, so the Java
    // host registers it: READY says nothing about whether it can run it.
    eprintln!("[jvm-e2e] the Java host reports a Rust wasm candidate as {java_report}");
    assert_eq!(java_report["state"], "REGISTERED", "{java_report}");
    let (status, body) = platform
        .put(
            &format!("/api/functions/{wasm_address}/aliases/live"),
            &publisher,
            json!({"version": 1, "expectedVersion": 0}),
        )
        .await;
    assert_eq!(status, 200, "promote the wasm function: {body}");
    let java_report = wait_for(
        "the Java host's load of the wasm function",
        Duration::from_secs(60),
        &[&java_proc],
        || async {
            let (_, loaded, _) = host_row(&db, JAVA_HOST_ID).await?;
            let entry = reported(&loaded, wasm_address, 1)?;
            (entry["state"] != "REGISTERED").then_some(entry)
        },
    )
    .await;
    eprintln!("[jvm-e2e] the Java host reports a live Rust wasm function as {java_report}");
    assert_eq!(java_report["state"], "FAILED", "{java_report}");
    let r = http
        .get(format!("{java_url}/functions/{wasm_address}/"))
        .send()
        .await
        .unwrap();
    eprintln!(
        "[jvm-e2e] the Java host answers a call to it with {}",
        r.status()
    );
    assert_ne!(r.status(), 200);

    // ── 5. Disable unloads; SIGTERM stops the host cleanly ─────────────────────────────
    let (status, body) = platform
        .put(
            &format!("/api/functions/{ADDRESS}"),
            &admin,
            json!({"status": "DISABLED"}),
        )
        .await;
    assert!((200..300).contains(&status), "disable: {status} {body}");
    wait_for(
        "the Java host unloading the disabled function",
        Duration::from_secs(60),
        &[&java_proc],
        || async {
            let r = http.get(format!("{fn_url}/healthz")).send().await.ok()?;
            (r.status() == 404).then_some(())
        },
    )
    .await;
    let _ = Command::new("kill")
        .args(["-TERM", &java_proc.child.id().to_string()])
        .status();
    let deadline = Instant::now() + Duration::from_secs(90);
    let exit = loop {
        if let Some(status) = java_proc.child.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "the Java host never exited:\n{}",
            java_proc.log_text()
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    };
    // The JVM runs its shutdown hook (FnHost.close: drain, close the
    // listener and every function) and then exits 128 + SIGTERM.
    assert!(
        exit.success() || exit.code() == Some(143),
        "the Java host stops cleanly after SIGTERM: {exit}\n{}",
        java_proc.log_text()
    );
    // Java's drain only flags the reconciler: a DRAINING heartbeat goes out
    // only if a reconcile cycle runs during the drain, so the row usually
    // keeps its last state until the platform marks it stale.
    let (state,): (String,) = sqlx::query_as("SELECT state FROM fnr_hosts WHERE id = $1")
        .bind(JAVA_HOST_ID)
        .fetch_one(&db)
        .await
        .unwrap();
    eprintln!("[jvm-e2e] the Java host's row after SIGTERM: {state}");

    platform_proc.assert_running();
    rust_proc.assert_running();
    drop(rust_only);
}
