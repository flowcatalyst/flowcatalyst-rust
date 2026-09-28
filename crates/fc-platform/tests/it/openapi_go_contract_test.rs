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
//! `OPENAPI_DUMP=<file>` writes the document for tooling (`just regen-sdks`
//! diffs, `docs/sdks.md` measurements).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use fc_platform::auth::auth_service::{AuthConfig, AuthService};
use fc_platform::auth::oidc_sync_service::OidcSyncService;
use fc_platform::auth::password_service::PasswordService;
use fc_platform::repository::Repositories;
use fc_platform::shared::authorization_service::AuthorizationService;
use fc_platform::shared::server_setup::{AuthServices, PlatformContext, PlatformRoutesConfig};
use fc_platform::usecase::PgUnitOfWork;
use serde_json::Value;

const METHODS: &[&str] = &["get", "put", "post", "delete", "patch"];

fn rust_document() -> Value {
    let pool = sqlx::postgres::PgPoolOptions::new()
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
            rate_limit_store: Arc::new(fc_platform::shared::rate_limit_store::NoopRateLimitStore),
            rate_limit_policies: Arc::new(
                fc_platform::shared::rate_limit_store::RateLimitPolicies::from_env(),
            ),
            session_cookie_secure: false,
            session_cookie_same_site: PlatformRoutesConfig::DEFAULT_SAME_SITE.to_string(),
            session_token_expiry_secs: PlatformRoutesConfig::DEFAULT_SESSION_EXPIRY_SECS,
            static_dir: None,
            oidc_login_external_base_url: None,
            well_known_external_base_url: "http://localhost".to_string(),
            password_reset_external_base_url: "http://localhost".to_string(),
        },
        "app_platform".to_string(),
    );
    let (_router, openapi) = fc_platform::router::build(&ctx);
    serde_json::to_value(&openapi).expect("serialise document")
}

fn go_document() -> Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../frontend/openapi/openapi.json"
    );
    serde_json::from_str(&std::fs::read_to_string(path).expect("Go lockfile copy"))
        .expect("Go lockfile parses")
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

#[tokio::test]
async fn shared_operations_carry_gos_operation_ids() {
    let rust = rust_document();
    if let Ok(path) = std::env::var("OPENAPI_DUMP") {
        std::fs::write(&path, serde_json::to_vec_pretty(&rust).unwrap()).unwrap();
    }
    let go = go_document();
    let rust_ops = operations(&rust);
    let go_ops = operations(&go);

    let mut mismatched = Vec::new();
    let mut shared = 0;
    for (key, go_id) in &go_ops {
        if let Some(rust_id) = rust_ops.get(key) {
            shared += 1;
            if rust_id != go_id {
                mismatched.push(format!("{} {}: rust {rust_id}, go {go_id}", key.0, key.1));
            }
        }
    }
    let go_only: Vec<_> = go_ops
        .keys()
        .filter(|k| !rust_ops.contains_key(*k))
        .map(|(m, p)| format!("{m} {p}"))
        .collect();
    let rust_only: Vec<_> = rust_ops
        .keys()
        .filter(|k| !go_ops.contains_key(*k))
        .map(|(m, p)| format!("{m} {p}"))
        .collect();
    println!(
        "operations: go {}, rust {}, shared {shared}, operationId mismatches {}",
        go_ops.len(),
        rust_ops.len(),
        mismatched.len()
    );
    if std::env::var("OPENAPI_REPORT").is_ok() {
        println!("-- mismatched\n{}", mismatched.join("\n"));
        println!("-- go only\n{}", go_only.join("\n"));
        println!("-- rust only\n{}", rust_only.join("\n"));
    }

    // Duplicate operationIds break every generator.
    let mut seen = BTreeSet::new();
    let dupes: Vec<_> = rust_ops
        .values()
        .filter(|id| !seen.insert(id.as_str()))
        .collect();
    assert!(dupes.is_empty(), "duplicate operationIds: {dupes:?}");

    assert!(
        mismatched.is_empty(),
        "operations documented by both platforms must carry Go's operationId:\n{}",
        mismatched.join("\n")
    );
    assert!(
        go_only.is_empty(),
        "operations Go documents but this platform does not:\n{}",
        go_only.join("\n")
    );
}

/// Go's component schemas this document does not carry, and why. None:
/// Go's two orphans (the debug BFF routes' `RawDispatchJobResponse` and
/// `RawEventResponse`) are carried as Go carries them.
const SCHEMAS_NOT_DOCUMENTED: &[(&str, &str)] = &[];

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

#[tokio::test]
async fn shared_operations_name_gos_schemas() {
    let rust = rust_document();
    let go = go_document();

    // Every schema Go names exists under the same name.
    let rust_schemas = rust["components"]["schemas"].as_object().unwrap();
    let missing: Vec<_> = go["components"]["schemas"]
        .as_object()
        .unwrap()
        .keys()
        .filter(|name| !rust_schemas.contains_key(*name))
        .filter(|name| !SCHEMAS_NOT_DOCUMENTED.iter().any(|(n, _)| n == name))
        .cloned()
        .collect();
    assert!(missing.is_empty(), "Go schemas missing here: {missing:?}");

    // Every shared operation names the same request schema and the same
    // success status and schema.
    let mut diffs = Vec::new();
    for (method, path) in operations(&go).keys() {
        let (Some(g), Some(r)) = (operation(&go, method, path), operation(&rust, method, path))
        else {
            continue;
        };
        if request_schema(g) != request_schema(r) {
            diffs.push(format!(
                "{method} {path} request: go {:?}, rust {:?}",
                request_schema(g),
                request_schema(r)
            ));
        }
        let (gs, rs) = (success_response(g), success_response(r));
        // `createEvent` also documents the 200 of an idempotent replay.
        let replay = method == "POST" && path == "/api/events";
        let same = gs == rs || (replay && gs.as_ref().map(|s| &s.1) == rs.as_ref().map(|s| &s.1));
        if !same {
            diffs.push(format!("{method} {path} response: go {gs:?}, rust {rs:?}"));
        }
    }
    assert!(diffs.is_empty(), "{}", diffs.join("\n"));
}
