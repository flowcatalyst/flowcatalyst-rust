//! The platform's route table, pinned.
//!
//! Builds the full platform router the way `fc-server` builds it (the
//! platform routes under the `AuthLayer`; no database: the pool is lazy and
//! points nowhere) and records, for every path the router holds:
//!
//! * the methods it answers (the `Allow` header of a 405 to an unused
//!   method, in the order axum merged them), and what that 405 goes through;
//! * for each method, what an unauthenticated request with no body gets:
//!   status, content type and a hash of the body (401 = the route requires
//!   authentication), which in-memory per-IP limiter it sits behind (a
//!   second request from the same address, every limiter at a burst of 1,
//!   answers 429 with the limiter's own `Retry-After`), and which
//!   distributed rate-limit buckets it spends;
//! * whether it is in the published OpenAPI document (`/q/openapi`) and in
//!   the full one (`/q/openapi-full`);
//!
//! plus a hash of every OpenAPI document the platform serves or hands the
//! developer portal. Two configurations: the default one (no app key, no
//! SPA directory) and the full one (`FLOWCATALYST_APP_KEY` set, which mounts
//! `/api/dispatch/*`, and a SPA directory).
//!
//! The router's paths come from its `Debug` output (axum prints every
//! registered path), so a route added or dropped anywhere shows up without
//! anyone listing it. Everything is probed twice, on two routers built
//! independently; a value that differs between the two runs (a random key,
//! a fresh state) is recorded as `*`.
//!
//! The snapshot is `tests/data/route_table.snapshot`. After an intended
//! route change, regenerate it with
//! `UPDATE_ROUTE_SNAPSHOT=1 cargo test -p fc-platform --test route_table_snapshot_test`
//! and review the diff.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Method, Request};
use axum::Router;
use http_body_util::BodyExt;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tower::ServiceExt;

use fc_platform::auth::auth_service::{AuthConfig, AuthService};
use fc_platform::auth::oidc_sync_service::OidcSyncService;
use fc_platform::auth::password_service::PasswordService;
use fc_platform::repository::Repositories;
use fc_platform::shared::authorization_service::AuthorizationService;
use fc_platform::shared::middleware::{AppState, AuthLayer};
use fc_platform::shared::rate_limit_store::{
    Bucket, RateLimitDecision, RateLimitError, RateLimitPolicies, RateLimitPolicy, RateLimitStore,
};
use fc_platform::shared::server_setup::{
    build_platform_routes, AuthServices, PlatformRoutesConfig,
};
use fc_platform::usecase::PgUnitOfWork;

const SNAPSHOT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/data/route_table.snapshot"
);

/// A fixed app key, so the full configuration is the same on every run.
const APP_KEY: &str = "MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=";

/// Records the buckets each request spends; never limits.
#[derive(Default)]
struct RecordingStore {
    spent: Mutex<Vec<&'static str>>,
}

impl RecordingStore {
    fn take(&self) -> Vec<&'static str> {
        let mut spent = std::mem::take(&mut *self.spent.lock().unwrap());
        spent.sort_unstable();
        spent.dedup();
        spent
    }
}

#[async_trait]
impl RateLimitStore for RecordingStore {
    async fn check_and_record(
        &self,
        bucket: Bucket,
        _key: &str,
        _policy: RateLimitPolicy,
    ) -> Result<RateLimitDecision, RateLimitError> {
        self.spent.lock().unwrap().push(bucket.as_str());
        Ok(RateLimitDecision::Allow)
    }
}

/// The platform app as fc-server builds it (`build_platform_app`): the
/// platform routes under the `AuthLayer`. The binary's tracing and CORS
/// layers do not route.
fn build_app(static_dir: Option<String>, store: Arc<RecordingStore>) -> (Router, Value) {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .acquire_timeout(Duration::from_millis(20))
        .connect_lazy("postgres://nobody@127.0.0.1:1/none")
        .expect("lazy pool");
    let repos = Repositories::new(&pool);
    let unit_of_work = Arc::new(PgUnitOfWork::new(pool.clone()));
    let (private_key, public_key) =
        AuthConfig::load_or_generate_rsa_keys(None, None).expect("rsa keys");
    let auth_service = Arc::new(AuthService::new(AuthConfig {
        rsa_private_key: Some(private_key),
        rsa_public_key: Some(public_key),
        rsa_public_key_previous: None,
        secret_key: String::new(),
        issuer: "route-table".to_string(),
        audience: "route-table".to_string(),
        access_token_expiry_secs: 3600,
        session_token_expiry_secs: 28800,
        refresh_token_expiry_secs: 86400,
    }));
    let authz = Arc::new(AuthorizationService::new(repos.role_repo.clone()));
    let auth = AuthServices {
        auth: auth_service.clone(),
        authz: authz.clone(),
        password: Arc::new(PasswordService::default()),
        oidc_sync: Arc::new(OidcSyncService::new(
            repos.principal_repo.clone(),
            repos.idp_role_mapping_repo.clone(),
        )),
    };
    let routes = build_platform_routes(
        &repos,
        &auth,
        &unit_of_work,
        PlatformRoutesConfig {
            rate_limit_store: store,
            rate_limit_policies: Arc::new(RateLimitPolicies::from_env()),
            session_cookie_secure: true,
            session_cookie_same_site: PlatformRoutesConfig::DEFAULT_SAME_SITE.to_string(),
            session_token_expiry_secs: PlatformRoutesConfig::DEFAULT_SESSION_EXPIRY_SECS,
            static_dir,
            oidc_login_external_base_url: None,
            well_known_external_base_url: "http://localhost:8080".to_string(),
            password_reset_external_base_url: "http://localhost:8080".to_string(),
        },
        "app_platform".to_string(),
    );
    let (app, openapi) = routes.build();
    let app = app.layer(AuthLayer::new(AppState {
        auth_service,
        authz_service: authz,
    }));
    (app, openapi)
}

/// Every path the router holds, from its `Debug` output.
fn router_paths(app: &Router) -> BTreeSet<String> {
    let debug = format!("{app:?}");
    let re = regex::Regex::new(r#"RouteId\(\d+\): "([^"]*)""#).unwrap();
    re.captures_iter(&debug).map(|c| c[1].to_string()).collect()
}

/// `/api/x/{id}/y/{*rest}` -> a concrete path no literal route claims.
fn concrete(path: &str) -> String {
    let wildcard = regex::Regex::new(r"\{\*[^}]*\}").unwrap();
    let param = regex::Regex::new(r"\{[^}]*\}").unwrap();
    let path = wildcard.replace_all(path, "zzprobe/zztail");
    param.replace_all(&path, "zzprobe").into_owned()
}

fn short_hash(bytes: &[u8]) -> String {
    let normalised = String::from_utf8_lossy(bytes).replace(fc_common::BUILD_VERSION, "<version>");
    hex::encode(&Sha256::digest(normalised.as_bytes())[..6])
}

/// The in-memory limiter a 429 came from, told apart by its rate.
fn limiter(res: &axum::http::Response<Body>) -> &'static str {
    if res.status() != 429 {
        return "-";
    }
    let secs: u64 = res
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    match secs {
        40.. => "auth-ip",
        25..=39 => "oauth-ip",
        10..=24 => "portal-ip",
        _ => "429?",
    }
}

struct Prober {
    app: Router,
    store: Arc<RecordingStore>,
    next_ip: u32,
}

impl Prober {
    fn fresh_ip(&mut self) -> String {
        self.next_ip += 1;
        let n = self.next_ip;
        format!("10.{}.{}.{}", (n >> 16) & 255, (n >> 8) & 255, n & 255)
    }

    async fn send(&self, method: &Method, path: &str, ip: &str) -> axum::http::Response<Body> {
        let req = Request::builder()
            .method(method.clone())
            .uri(path)
            .header("x-forwarded-for", ip)
            .body(Body::empty())
            .unwrap();
        self.app.clone().oneshot(req).await.unwrap()
    }

    /// One method on one path: the answer, then the limiter a second
    /// request from the same address meets.
    async fn probe(&mut self, method: &Method, path: &str) -> (Vec<String>, Option<String>) {
        let ip = self.fresh_ip();
        let res = self.send(method, path, &ip).await;
        let status = res.status().as_u16().to_string();
        let allow = res
            .headers()
            .get("allow")
            .map(|v| v.to_str().unwrap_or("?").to_string());
        let content_type = res
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("-")
            .to_string();
        let location = res
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .map(|l| format!(" location={l}"))
            .unwrap_or_default();
        let body = res.into_body().collect().await.unwrap().to_bytes();
        let body = if path.contains("jwks") {
            "jwks".to_string()
        } else {
            short_hash(&body)
        };
        let buckets = self.store.take();
        let again = self.send(method, path, &ip).await;
        let limited = limiter(&again);
        let again_buckets = self.store.take();
        let buckets: BTreeSet<&str> = buckets.into_iter().chain(again_buckets).collect();
        let buckets = if buckets.is_empty() {
            "-".to_string()
        } else {
            buckets.into_iter().collect::<Vec<_>>().join(",")
        };
        (
            vec![
                status,
                format!("ct={content_type}{location}"),
                format!("body={body}"),
                format!("limiter={limited}"),
                format!("buckets={buckets}"),
            ],
            allow,
        )
    }
}

fn documented(doc: &Value) -> BTreeSet<(String, String)> {
    let mut out = BTreeSet::new();
    for (path, item) in doc["paths"].as_object().into_iter().flatten() {
        for (method, _) in item.as_object().into_iter().flatten() {
            if ["get", "put", "post", "delete", "patch", "head", "options"]
                .contains(&method.as_str())
            {
                out.insert((method.to_uppercase(), path.clone()));
            }
        }
    }
    out
}

/// Probe one freshly built app: `key -> fields`.
async fn table(static_dir: Option<String>) -> BTreeMap<String, Vec<String>> {
    let store = Arc::new(RecordingStore::default());
    let (app, developer_portal_doc) = build_app(static_dir, store.clone());
    let mut prober = Prober {
        app: app.clone(),
        store,
        next_ip: 0,
    };
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();

    // The documents.
    out.insert(
        "DOC developer-portal (build's returned document)".into(),
        vec![short_hash(
            &serde_json::to_vec(&developer_portal_doc).unwrap(),
        )],
    );
    let mut docs = BTreeMap::new();
    for path in [
        "/q/openapi",
        "/q/openapi-full",
        "/api/openapi.json",
        "/api/openapi.yaml",
    ] {
        let res = prober.send(&Method::GET, path, "127.0.0.1").await;
        let body = res.into_body().collect().await.unwrap().to_bytes();
        out.insert(format!("DOC {path}"), vec![short_hash(&body)]);
        docs.insert(path, body);
    }
    prober.store.take();
    let published = documented(&serde_json::from_slice(&docs["/q/openapi"]).unwrap());
    let full = documented(&serde_json::from_slice(&docs["/q/openapi-full"]).unwrap());
    let mut seen = BTreeSet::new();

    let unused = Method::from_bytes(b"PROBE").unwrap();
    let mut paths = router_paths(&app);
    // What no route claims (the SPA fallback, or axum's 404).
    paths.insert("/zz-no-route".to_string());
    for path in paths {
        let target = concrete(&path);
        let (fields, allow) = prober.probe(&unused, &target).await;
        let mut line = fields.clone();
        line.insert(
            0,
            format!("allow={}", allow.clone().unwrap_or_else(|| "-".into())),
        );
        out.insert(format!("{path} PROBE"), line);
        let methods: Vec<String> = allow
            .map(|a| {
                a.split(',')
                    .map(|m| m.trim().to_string())
                    .filter(|m| !m.is_empty() && m != "HEAD")
                    .collect()
            })
            .unwrap_or_default();
        for m in methods {
            let method = Method::from_bytes(m.as_bytes()).unwrap();
            let (mut fields, _) = prober.probe(&method, &target).await;
            let key = (m.clone(), path.clone());
            fields.push(format!(
                "doc={}{}",
                if published.contains(&key) {
                    "published"
                } else {
                    "-"
                },
                if full.contains(&key) { "+full" } else { "" },
            ));
            seen.insert(key);
            out.insert(format!("{path} {m}"), fields);
        }
    }
    // Documented operations the router does not route (none expected).
    for (method, path) in published.iter().chain(full.iter()) {
        if !seen.contains(&(method.clone(), path.clone())) {
            out.insert(
                format!("{path} {method}"),
                vec!["documented-but-not-routed".into()],
            );
        }
    }
    out
}

/// Two independent builds, merged: a field that differs becomes `*`.
async fn stable_table(static_dir: Option<String>) -> BTreeMap<String, Vec<String>> {
    let a = table(static_dir.clone()).await;
    let b = table(static_dir).await;
    assert_eq!(
        a.keys().collect::<Vec<_>>(),
        b.keys().collect::<Vec<_>>(),
        "two builds route different paths"
    );
    a.into_iter()
        .map(|(k, fa)| {
            let fb = &b[&k];
            let merged = fa
                .iter()
                .zip(fb)
                .map(|(x, y)| if x == y { x.clone() } else { "*".into() })
                .collect();
            (k, merged)
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_route_table_is_unchanged() {
    // Every in-memory limiter at a burst of 1, each at its own rate so its
    // `Retry-After` names it (see `limiter`). Set before any build reads
    // them; this file is its own test binary.
    for (k, v) in [
        ("FC_AUTH_IP_RATE_PER_MIN", "1"),
        ("FC_AUTH_IP_BURST", "1"),
        ("FC_OAUTH_TOKEN_IP_RATE_PER_MIN", "2"),
        ("FC_OAUTH_TOKEN_IP_BURST", "1"),
        ("FC_OIDC_RATE_PER_MIN", "3"),
        ("FC_OIDC_BURST", "1"),
    ] {
        std::env::set_var(k, v);
    }
    std::env::remove_var("FLOWCATALYST_APP_KEY");
    std::env::remove_var("FLOWCATALYST_APP_KEY_PREVIOUS");

    let mut out = String::new();
    out.push_str("# Generated by tests/route_table_snapshot_test.rs; see its header.\n");
    out.push_str(
        "# <path> <method>: status, ct, body hash, limiter on a 2nd request, buckets, doc\n",
    );

    let default = stable_table(None).await;
    out.push_str("\n== default: no app key, no SPA directory\n");
    for (k, v) in &default {
        out.push_str(&format!("{k}: {}\n", v.join(" ")));
    }

    let spa = tempfile::tempdir().unwrap();
    std::fs::write(spa.path().join("index.html"), "<html>shell</html>").unwrap();
    std::fs::create_dir(spa.path().join("assets")).unwrap();
    std::fs::write(spa.path().join("assets/app-1.js"), "1").unwrap();
    std::env::set_var("FLOWCATALYST_APP_KEY", APP_KEY);
    let full = stable_table(Some(spa.path().to_str().unwrap().to_string())).await;
    std::env::remove_var("FLOWCATALYST_APP_KEY");
    out.push_str("\n== full: app key (mounts /api/dispatch/*), SPA directory\n");
    for (k, v) in &full {
        out.push_str(&format!("{k}: {}\n", v.join(" ")));
    }

    if std::env::var("UPDATE_ROUTE_SNAPSHOT").is_ok() {
        std::fs::write(SNAPSHOT, &out).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(SNAPSHOT).unwrap_or_default();
    if expected != out {
        let actual = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../target/route_table.actual"
        );
        std::fs::write(actual, &out).unwrap();
        let exp: BTreeSet<&str> = expected.lines().collect();
        let act: BTreeSet<&str> = out.lines().collect();
        let gone: Vec<&&str> = exp.difference(&act).take(40).collect();
        let new: Vec<&&str> = act.difference(&exp).take(40).collect();
        panic!(
            "\n\nThe route table changed (full table in target/route_table.actual).\n\
             Only in the snapshot:\n  {}\nOnly now:\n  {}\n\n\
             If the change is intended, regenerate with UPDATE_ROUTE_SNAPSHOT=1.\n",
            gone.iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .join("\n  "),
            new.iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .join("\n  "),
        );
    }
}
