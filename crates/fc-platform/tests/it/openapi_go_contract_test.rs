//! The platform's OpenAPI document (`/q/openapi`) against Go's contract.
//!
//! The published SDKs' generated clients (TypeScript `src/generated`, Laravel
//! `src/Generated`, the Java models) are generated from Go's huma document,
//! vendored here as `frontend/openapi/openapi.json` (a copy of
//! `flowcatalyst-go/api/openapi.lock.json`). Regenerating them from this
//! platform's document must give the same code, so every operation both
//! platforms document carries Go's `operationId`, and the component schemas
//! the SDK generators name classes after carry Go's names.
//!
//! The document is built the way the binaries build it (no database: the pool
//! is lazy and never connected).
//!
//! Known gaps are a ratchet, not an assertion of emptiness: every difference
//! from Go's contract (Go-only operations, differing operationIds, missing
//! schemas, differing request or response schemas, Rust-only operations) is
//! listed in `tests/data/openapi_go_gaps.baseline.txt`. The test fails when the
//! current gaps differ from the baseline in either direction: a NEW gap is a
//! regression; a gap that was CLOSED must be removed from the baseline in the
//! same change, so the baseline never goes stale. `OPENAPI_UPDATE_BASELINE=1`
//! rewrites the baseline from the current gaps.
//!
//! Environment:
//! - `GO_LOCK_PATH=<file>` compares against that Go lock instead of the vendored
//!   copy `frontend/openapi/openapi.json` (the weekly spec diff points it at
//!   `../flowcatalyst-go/api/openapi.lock.json`; the baseline then differs
//!   from the vendored one by exactly what Go changed since the last refresh).
//! - `OPENAPI_DUMP=<file>` writes the document for tooling (`just regen-sdks`
//!   diffs, `docs/sdks.md` measurements).
//! - `OPENAPI_REPORT=1` prints the gap list.

use fc_platform::shared::id::ApplicationId;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use fc_platform::auth::auth_service::{AuthConfig, AuthService};
use fc_platform::auth::oidc_sync_service::OidcSyncService;
use fc_platform::auth::password_service::PasswordService;
use fc_platform::repository::Repositories;
use fc_platform::router;
use fc_platform::shared::authorization_service::AuthorizationService;
use fc_platform::shared::rate_limit_store::NoopRateLimitStore;
use fc_platform::shared::rate_limit_store::RateLimitPolicies;
use fc_platform::shared::server_setup::{AuthServices, PlatformContext, PlatformRoutesConfig};
use fc_platform::usecase::PgUnitOfWork;
use serde_json::Value;
use sqlx::postgres::PgPoolOptions;
use std::env;
use std::fs;

const METHODS: &[&str] = &["get", "put", "post", "delete", "patch"];

fn rust_document() -> Value {
    let pool = PgPoolOptions::new()
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
        issuer: "openapi-test".to_string(),
        audience: "openapi-test".to_string(),
        access_token_expiry_secs: 3600,
        session_token_expiry_secs: 28800,
        refresh_token_expiry_secs: 86400,
    }));
    let auth = AuthServices {
        auth: auth_service,
        authz: Arc::new(AuthorizationService::new(repos.role_repo.clone())),
        password: Arc::new(PasswordService::default()),
        oidc_sync: Arc::new(OidcSyncService::new(
            repos.principal_repo.clone(),
            repos.idp_role_mapping_repo.clone(),
        )),
    };
    let ctx = PlatformContext::new(
        &repos,
        &auth,
        &unit_of_work,
        PlatformRoutesConfig {
            rate_limit_store: Arc::new(NoopRateLimitStore),
            rate_limit_policies: Arc::new(RateLimitPolicies::from_env()),
            session_cookie_secure: false,
            session_cookie_same_site: PlatformRoutesConfig::DEFAULT_SAME_SITE.to_string(),
            session_token_expiry_secs: PlatformRoutesConfig::DEFAULT_SESSION_EXPIRY_SECS,
            static_dir: None,
            oidc_login_external_base_url: None,
            well_known_external_base_url: "http://localhost".to_string(),
            password_reset_external_base_url: "http://localhost".to_string(),
        },
        ApplicationId::parse("app_platform").unwrap(),
    );
    let (_router, openapi) = router::build(&ctx);
    serde_json::to_value(&openapi).expect("serialise document")
}

/// Go's lock: `GO_LOCK_PATH` if set, else the vendored copy.
fn go_document() -> Value {
    let path = env::var("GO_LOCK_PATH").unwrap_or_else(|_| {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../frontend/openapi/openapi.json"
        )
        .to_string()
    });
    serde_json::from_str(&fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}")))
        .unwrap_or_else(|e| panic!("{path} parses: {e}"))
}

/// `(METHOD, path with parameters as {}) -> operationId`.
fn operations(doc: &Value) -> BTreeMap<(String, String), String> {
    let mut out = BTreeMap::new();
    for (path, item) in doc["paths"].as_object().into_iter().flatten() {
        for method in METHODS {
            if let Some(op) = item.get(*method) {
                out.insert(
                    (method.to_uppercase(), normalise_path(path)),
                    op["operationId"].as_str().unwrap_or("").to_string(),
                );
            }
        }
    }
    out
}

/// `/api/x/{id}/y/{otherId}` -> `/api/x/{}/y/{}`: parameter names differ
/// between the generators without changing the route.
fn normalise_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut in_param = false;
    for c in path.chars() {
        match c {
            '{' => {
                in_param = true;
                out.push_str("{}");
            }
            '}' => in_param = false,
            _ if in_param => {}
            _ => out.push(c),
        }
    }
    out
}

/// Path of the checked-in baseline of known gaps.
const BASELINE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/data/openapi_go_gaps.baseline.txt"
);

/// The schema an operation's JSON request body or first success response
/// names (`$ref`, or `[$ref]` for an array of them).
fn body_schema_name(schema: &Value) -> Option<String> {
    if let Some(r) = schema.get("$ref").and_then(Value::as_str) {
        return r.rsplit('/').next().map(str::to_string);
    }
    if schema.get("type").and_then(Value::as_str) == Some("array") {
        return body_schema_name(&schema["items"]).map(|n| format!("[{n}]"));
    }
    None
}

fn request_schema(op: &Value) -> Option<String> {
    body_schema_name(&op["requestBody"]["content"]["application/json"]["schema"])
}

fn success_response(op: &Value) -> Option<(String, Option<String>)> {
    let responses = op["responses"].as_object()?;
    let (code, response) = responses
        .iter()
        .filter(|(c, _)| c.starts_with('2'))
        .min_by_key(|(c, _)| c.as_str())?;
    Some((
        code.clone(),
        body_schema_name(&response["content"]["application/json"]["schema"]),
    ))
}

fn operation<'a>(doc: &'a Value, method: &str, path: &str) -> Option<&'a Value> {
    doc["paths"]
        .as_object()?
        .iter()
        .find(|(p, _)| normalise_path(p) == path)
        .and_then(|(_, item)| item.get(method.to_lowercase()))
}

/// Every difference from Go's contract, one sorted line each.
fn gaps(rust: &Value, go: &Value) -> BTreeSet<String> {
    let rust_ops = operations(rust);
    let go_ops = operations(go);
    let mut out = BTreeSet::new();
    for (key, go_id) in &go_ops {
        let (method, path) = (&key.0, &key.1);
        match rust_ops.get(key) {
            None => {
                out.insert(format!("go-only: {method} {path}"));
            }
            Some(rust_id) => {
                if rust_id != go_id {
                    out.insert(format!("operation-id: {method} {path} (go {go_id})"));
                }
                let (Some(g), Some(r)) =
                    (operation(go, method, path), operation(rust, method, path))
                else {
                    continue;
                };
                if request_schema(g) != request_schema(r) {
                    out.insert(format!(
                        "request: {method} {path} (go {:?}, rust {:?})",
                        request_schema(g),
                        request_schema(r)
                    ));
                }
                let (gs, rs) = (success_response(g), success_response(r));
                // `createEvent` also documents the 200 of an idempotent replay.
                let replay = method == "POST" && path == "/api/events";
                let same =
                    gs == rs || (replay && gs.as_ref().map(|s| &s.1) == rs.as_ref().map(|s| &s.1));
                if !same {
                    out.insert(format!(
                        "response: {method} {path} (go {gs:?}, rust {rs:?})"
                    ));
                }
            }
        }
    }
    for key in rust_ops.keys().filter(|k| !go_ops.contains_key(*k)) {
        out.insert(format!("rust-only: {} {}", key.0, key.1));
    }
    let rust_schemas = rust["components"]["schemas"].as_object().unwrap();
    for name in go["components"]["schemas"].as_object().unwrap().keys() {
        if !rust_schemas.contains_key(name) {
            out.insert(format!("schema-missing: {name}"));
        }
    }
    out
}

fn read_baseline() -> BTreeSet<String> {
    fs::read_to_string(BASELINE)
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect()
}

const BASELINE_HEADER: &str = "\
# Known differences between this platform's OpenAPI document and Go's lock.
# A RATCHET (see openapi_go_contract_test.rs): the test fails if the current gaps differ from
# this list in either direction. Closing a gap means deleting its line in the same change;
# a new gap is a regression. Regenerate with:
#   OPENAPI_UPDATE_BASELINE=1 cargo test -p fc-platform --test it -- openapi_go_contract_test
# Kinds: go-only, operation-id, request, response, rust-only, schema-missing.
";

#[tokio::test]
async fn gaps_with_gos_contract_match_the_baseline() {
    let rust = rust_document();
    if let Ok(path) = env::var("OPENAPI_DUMP") {
        fs::write(&path, serde_json::to_vec_pretty(&rust).unwrap()).unwrap();
    }
    let go = go_document();
    let current = gaps(&rust, &go);
    let rust_ops = operations(&rust);
    let go_ops = operations(&go);
    println!(
        "operations: go {}, rust {}, shared {}; gaps {}",
        go_ops.len(),
        rust_ops.len(),
        go_ops.keys().filter(|k| rust_ops.contains_key(*k)).count(),
        current.len()
    );
    if env::var("OPENAPI_REPORT").is_ok() {
        println!("{}", current.iter().cloned().collect::<Vec<_>>().join("\n"));
    }

    // Duplicate operationIds break every generator.
    let mut seen = BTreeSet::new();
    let dupes: Vec<_> = rust_ops
        .values()
        .filter(|id| !seen.insert(id.as_str()))
        .collect();
    assert!(dupes.is_empty(), "duplicate operationIds: {dupes:?}");

    if env::var("OPENAPI_UPDATE_BASELINE").is_ok() {
        let body: String = current.iter().map(|l| format!("{l}\n")).collect();
        fs::write(BASELINE, format!("{BASELINE_HEADER}{body}")).unwrap();
        return;
    }
    let baseline = read_baseline();
    let new: Vec<_> = current.difference(&baseline).cloned().collect();
    let closed: Vec<_> = baseline.difference(&current).cloned().collect();
    assert!(
        new.is_empty() && closed.is_empty(),
        "OpenAPI gaps differ from tests/data/openapi_go_gaps.baseline.txt.\n\
         NEW gaps (a regression, or Go moved: re-vendor and record it):\n{}\n\
         CLOSED gaps (delete these lines from the baseline):\n{}",
        new.join("\n"),
        closed.join("\n")
    );
}
