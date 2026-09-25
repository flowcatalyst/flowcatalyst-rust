//! The whole Rust stack, end to end: the production platform router served
//! on a real socket (Docker Postgres through the harness) and an in-process
//! `fc-fnhost` (fc-fnhost-core's `FnHost`, the real WASM runtime and
//! listener) talking to it over HTTP, exactly as the `fc-fnhost` binary
//! would, authenticated with OAuth `client_credentials` as a service account
//! holding the `function-host` role.
//!
//! 1. A function is created and its artifact (the PDK test guest, a WASI 0.2
//!    component with entrypoint `wasi_http_incoming_handler`) uploaded.
//! 2. It is published with signatures off (dev mode), and its declared
//!    config and secret are set.
//! 3. The host polls desired state (the version is a candidate), downloads
//!    the artifact from `/control/functions/artifacts/{versionId}`, verifies
//!    it and heartbeats `REGISTERED`: the platform marks the version `READY`
//!    with a `version:ready` event naming the host.
//! 4. The version is promoted live; the host serves
//!    `/functions/<address>/…` and the guest sees the config and the
//!    decrypted secret desired state delivered.
//! 5. The guest's emit reaches `msg_events` through
//!    `/control/functions/events`, as `function:<address>`.
//! 6. `runtime: component` (owner decision 5): a core module is refused at
//!    publish; the example component is published as version 2 with no
//!    entrypoint (it defaults), becomes READY on the host (whose heartbeat
//!    reports the runtimes it loads), is promoted with an `expectedVersion`
//!    precondition and serves; republishing it is a 200 no-op.
//! 7. Disabling the function unloads it from the host; deleting it leaves
//!    the pool's document empty.
//!
//! Requires Docker. Its own test binary: it sets process environment.

#[path = "support/mod.rs"]
mod support;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use chrono::Utc;
use serde_json::{json, Value};
use sha2::Digest as _;
use tower::ServiceExt;

use fc_fnhost_core::env::{EnvReader, HostEnv};
use fc_fnhost_core::host::{FnHost, Listener};
use fc_fnhost_core::listener::FnListener;
use fc_fnhost_core::loader::Loaders;
use fc_fnhost_core::wasm::{WasmLoader, WasmRuntime, WasmSettings};
use fc_platform::domain::{Principal, UserScope};
use fc_platform::role::entity::{permissions, roles, AuthRole};
use fc_platform::service_account::entity::RoleAssignment;
use support::{read_json, TestApp};

const APP_KEY: &str = "MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=";
const POOL: &str = "e2e";
/// The PDK guest emits `fixture:pdk:thing:happened`, so its function
/// belongs to the application `fixture`.
const ADDRESS: &str = "fixture.pdk.e2e";
const EVENT_TYPE: &str = "fixture:pdk:thing:happened";
const HOST_ID: &str = "e2e-host-1";

fn guest() -> Vec<u8> {
    guest_fixture("pdk.wasm")
}

fn guest_fixture(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fc-fnhost-core/tests/fixtures/wasm")
        .join(name);
    std::fs::read(path).expect("the committed PDK guest")
}

async fn api(
    app: &TestApp,
    method: Method,
    path: &str,
    token: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", format!("Bearer {token}"));
    let body = match body {
        Some(v) => {
            req = req.header("content-type", "application/json");
            Body::from(v.to_string())
        }
        None => Body::empty(),
    };
    read_json(
        app.router
            .clone()
            .oneshot(req.body(body).unwrap())
            .await
            .unwrap(),
    )
    .await
}

/// An anchor admin who reaches every application.
async fn admin(app: &TestApp) -> String {
    let role =
        AuthRole::new("platform", "e2e-admin", "E2E admin").with_permission(permissions::ADMIN_ALL);
    app.repos.role_repo.insert(&role).await.unwrap();
    let mut principal = Principal::new_user("e2e-admin@flowcatalyst.test", UserScope::Anchor);
    principal.roles = vec![RoleAssignment::new(role.name.clone())];
    principal.all_applications = true;
    app.repos.principal_repo.insert(&principal).await.unwrap();
    app.auth_service.generate_access_token(&principal).unwrap()
}

/// The host's identity, provisioned as an operator would: a service
/// account (with no client ids, so its tokens are anchor-scoped) and its
/// `client_credentials` client, granted the built-in `function-host` role.
async fn host_credentials(app: &TestApp, admin: &str) -> (String, String) {
    let (status, body) = api(
        app,
        Method::POST,
        "/api/service-accounts",
        admin,
        Some(json!({"code": "e2e-fn-host", "name": "E2E function host"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    // The harness does not run start-up role seeding; the role is the
    // code-defined one.
    let host_role = roles::function_host();
    if app
        .repos
        .role_repo
        .find_by_name(&host_role.name)
        .await
        .unwrap()
        .is_none()
    {
        app.repos.role_repo.insert(&host_role).await.unwrap();
    }
    let account_id = body["serviceAccount"]["id"].as_str().unwrap();
    let (status, roles) = api(
        app,
        Method::PUT,
        &format!("/api/service-accounts/{account_id}/roles"),
        admin,
        Some(json!({"roles": [host_role.name]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{roles}");
    (
        body["oauth"]["clientId"].as_str().unwrap().to_string(),
        body["oauth"]["clientSecret"].as_str().unwrap().to_string(),
    )
}

async fn start_host(
    platform_url: &str,
    client_id: &str,
    client_secret: &str,
    cache: &std::path::Path,
) -> FnHost {
    let env = HostEnv::load(&EnvReader::from_pairs([
        ("FC_FN_POOL", POOL),
        ("FC_FN_PLATFORM_URL", platform_url),
        ("FC_FN_CLIENT_ID", client_id),
        ("FC_FN_CLIENT_SECRET", client_secret),
        ("FC_FN_HOST_ID", HOST_ID),
        ("FC_FN_SIGNATURES", "off"),
        ("FLOWCATALYST_DEV_MODE", "true"),
        ("FC_FN_CACHE_DIR", cache.to_str().unwrap()),
        ("FC_FN_PORT", "0"),
        ("FC_METRICS_PORT", "0"),
        ("FC_FN_PUBLIC_PORT", "off"),
        ("FC_FN_MAX_EXECUTING", "2"),
    ]))
    .expect("host environment");
    let wasm = WasmRuntime::new(WasmSettings::from_env(&env)).expect("wasm runtime");
    let loaders = Arc::new(WasmLoader::new(wasm)).register(Loaders::none());
    let listener: Arc<dyn Listener> = Arc::new(FnListener::from_env(&env));
    let mut host = FnHost::new(env, loaders, Some(listener)).expect("host");
    host.start().await.expect("host starts");
    host
}

/// Uploads `bytes` as the function's artifact: its `platform://` ref and
/// digest.
async fn upload_artifact(app: &TestApp, token: &str, bytes: &[u8]) -> (String, String) {
    let digest = format!("sha256:{}", hex::encode(sha2::Sha256::digest(bytes)));
    let request = Request::builder()
        .method(Method::PUT)
        .uri(format!("/api/functions/{ADDRESS}/artifacts/{digest}"))
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/octet-stream")
        .header("content-length", bytes.len())
        .body(Body::from(bytes.to_vec()))
        .unwrap();
    let (status, body) = read_json(app.router.clone().oneshot(request).await.unwrap()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    (body["artifactRef"].as_str().unwrap().to_string(), digest)
}

async fn version_state(app: &TestApp, token: &str, version: i32) -> String {
    let (status, body) = api(
        app,
        Method::GET,
        &format!("/api/functions/{ADDRESS}/versions/{version}"),
        token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["state"].as_str().unwrap().to_string()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires Docker"]
async fn a_function_published_on_the_platform_runs_on_the_host() {
    let store = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    std::env::set_var("FLOWCATALYST_APP_KEY", APP_KEY);
    std::env::set_var("FLOWCATALYST_DEV_MODE", "true");
    std::env::set_var("FC_FN_SIGNATURES", "off");
    std::env::set_var(
        "FC_FN_ARTIFACT_STORE",
        format!("file://{}", store.path().display()),
    );
    let app = TestApp::setup().await;

    // The production router on a real socket, for the host.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let platform_url = format!("http://{}", listener.local_addr().unwrap());
    let served = app.router.clone();
    tokio::spawn(async move {
        axum::serve(
            listener,
            served.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });

    let admin = admin(&app).await;
    let fixture = fc_platform::application::entity::Application::new("fixture", "Fixture");
    app.repos.application_repo.insert(&fixture).await.unwrap();
    sqlx::query(
        "INSERT INTO msg_event_types (id, code, name, status, source, client_scoped, \
         application, subdomain, aggregate, created_at, updated_at) \
         VALUES ($1, $2, 'Thing happened', 'CURRENT', 'API', false, 'fixture', 'pdk', 'thing', NOW(), NOW())",
    )
    .bind(fc_platform::shared::tsid::generate_untyped())
    .bind(EVENT_TYPE)
    .execute(&app.pool)
    .await
    .unwrap();

    let client = fc_platform::client::entity::Client::new("E2E", "e2e");
    app.repos.client_repo.insert(&client).await.unwrap();

    // ── 1. Create the function and upload its artifact ───────────────────
    let (status, body) = api(
        &app,
        Method::POST,
        "/api/functions",
        &admin,
        Some(
            json!({"applicationCode": "fixture", "serviceName": "pdk", "name": "e2e",
                    "runtime": "wasm", "clientId": client.id}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let function_id = body["id"].as_str().unwrap().to_string();

    let bytes = guest();
    let digest = format!("sha256:{}", hex::encode(sha2::Sha256::digest(&bytes)));
    let upload = Request::builder()
        .method(Method::PUT)
        .uri(format!("/api/functions/{ADDRESS}/artifacts/{digest}"))
        .header("authorization", format!("Bearer {admin}"))
        .header("content-type", "application/octet-stream")
        .header("content-length", bytes.len())
        .body(Body::from(bytes.clone()))
        .unwrap();
    let (status, body) = read_json(app.router.clone().oneshot(upload).await.unwrap()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let artifact_ref = body["artifactRef"].as_str().unwrap().to_string();
    assert!(artifact_ref.starts_with("platform://"), "{artifact_ref}");

    // ── 2. Publish with signatures off; set config and a secret ──────────
    let (status, body) = api(
        &app,
        Method::POST,
        &format!("/api/functions/{ADDRESS}/versions"),
        &admin,
        Some(json!({
            "artifactRef": artifact_ref,
            "digest": digest,
            "manifest": {
                "runtime": "wasm",
                "entrypoint": "wasi_http_incoming_handler",
                "pool": POOL,
                "endpoints": [{"path": "/*", "auth": "none"}],
                "config": ["GREETING"],
                "secrets": ["API_KEY"],
            },
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["version"], 1);
    let (status, body) = api(
        &app,
        Method::PUT,
        &format!("/api/functions/{ADDRESS}/config"),
        &admin,
        Some(json!({"values": {"GREETING": "hello from the platform"}})),
    )
    .await;
    assert!(status.is_success(), "{status} {body}");
    let (status, body) = api(
        &app,
        Method::PUT,
        &format!("/api/functions/{ADDRESS}/secrets/API_KEY"),
        &admin,
        Some(json!({"value": "k-e2e-7f1c"})),
    )
    .await;
    assert!(status.is_success(), "{status} {body}");
    assert_eq!(version_state(&app, &admin, 1).await, "PUBLISHED");

    // ── 3. The host fetches, verifies, registers; the platform marks READY ─
    let (client_id, client_secret) = host_credentials(&app, &admin).await;
    let mut host = start_host(&platform_url, &client_id, &client_secret, cache.path()).await;
    // Start-up's first reconcile fetched the document (the version as a
    // candidate), downloaded and verified the artifact, and heartbeated it
    // REGISTERED (a candidate is never loaded): the platform has marked it
    // READY, naming this host. (The loop's next cycle then drops it, as
    // Java's does: a READY version is no longer a candidate until promoted.)
    assert_eq!(version_state(&app, &admin, 1).await, "READY");
    let (ready_host,): (String,) = sqlx::query_as(
        "SELECT data->>'hostId' FROM msg_events WHERE type = 'platform:function:version:ready' \
         AND subject = $1",
    )
    .bind(format!("platform.function.{function_id}"))
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(ready_host, HOST_ID);
    let (_, pools) = api(&app, Method::GET, "/api/function-pools", &admin, None).await;
    assert_eq!(pools, json!([{"pool": POOL, "hosts": 1}]));

    // ── 4. Promote live: the host serves it, with its settings ───────────
    let (status, body) = api(
        &app,
        Method::PUT,
        &format!("/api/functions/{ADDRESS}/aliases/live"),
        &admin,
        Some(json!({"version": 1})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    host.reconciler().reconcile_once(Utc::now()).await;
    let document = host.reconciler().document().unwrap();
    assert_eq!(document.functions.len(), 1);
    assert_eq!(
        document.functions[0].role,
        fc_fnhost_core::desired::Role::Live
    );

    let base = format!("http://127.0.0.1:{}", host.port().expect("listener port"));
    let http = reqwest::Client::new();
    let config: Value = http
        .get(format!("{base}/functions/{ADDRESS}/config?key=GREETING"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(config["value"], "hello from the platform", "{config}");
    let secret: Value = http
        .get(format!("{base}/functions/{ADDRESS}/secret?key=API_KEY"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        secret["value"], "k-e2e-7f1c",
        "decrypted in desired state: {secret}"
    );
    host.reconciler().reconcile_once(Utc::now()).await;
    assert_eq!(
        host.reconciler().heartbeat_report(&document).loaded[0].state,
        fc_fnhost_core::heartbeat::LoadState::Loaded
    );

    let (_, status_body) = api(
        &app,
        Method::GET,
        &format!("/api/functions/{ADDRESS}/status"),
        &admin,
        None,
    )
    .await;
    assert_eq!(status_body["hosts"][0]["hostId"], HOST_ID, "{status_body}");
    assert_eq!(
        status_body["hosts"][0]["loaded"],
        json!([{"version": 1, "state": "LOADED"}]),
        "{status_body}"
    );

    // ── 5. The guest's emit reaches msg_events ───────────────────────────
    let emitted = http
        .post(format!("{base}/functions/{ADDRESS}/emit?dedupId=e2e-1"))
        .header("X-Correlation-Id", "corr-e2e")
        .body(r#"{"n":1}"#)
        .send()
        .await
        .unwrap();
    let emitted: Value = emitted.json().await.unwrap();
    assert_eq!(emitted, json!({"ok": true}), "{emitted}");
    let row: (
        String,
        String,
        Option<String>,
        Option<String>,
        Value,
        Option<String>,
    ) = sqlx::query_as(
        "SELECT type, source, client_id, subject, data, correlation_id FROM msg_events \
         WHERE deduplication_id = 'e2e-1'",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(row.0, EVENT_TYPE);
    assert_eq!(row.1, format!("function:{ADDRESS}"));
    assert_eq!(row.2.as_deref(), Some(client.id.as_str()));
    assert_eq!(row.3.as_deref(), Some("thing-1"));
    assert_eq!(row.4, json!({"n": 1}));
    assert_eq!(row.5.as_deref(), Some("corr-e2e"));

    // ── 6. runtime: component ────────────────────────────────────────────
    // The host says what it loads.
    let (runtimes,): (Value,) = sqlx::query_as("SELECT runtimes FROM fn_hosts WHERE id = $1")
        .bind(HOST_ID)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(runtimes, json!(["component", "wasm"]));
    let component_manifest = json!({
        "runtime": "component",
        "pool": POOL,
        "endpoints": [{"path": "/*", "auth": "none"}],
    });
    // A core module is not a component: refused at publish, not at load.
    let core_module = b"\0asm\x01\0\0\0".to_vec();
    let core_ref = upload_artifact(&app, &admin, &core_module).await;
    let (status, body) = api(
        &app,
        Method::POST,
        &format!("/api/functions/{ADDRESS}/versions"),
        &admin,
        Some(json!({"artifactRef": core_ref.0, "digest": core_ref.1, "manifest": component_manifest})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["error"], "ARTIFACT_RUNTIME_MISMATCH");
    // The manifest check's plan: the pool has a live host that loads it.
    let (status, body) = api(
        &app,
        Method::POST,
        &format!("/api/functions/{ADDRESS}/manifest/check"),
        &admin,
        Some(json!({"manifest": component_manifest})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["valid"], true, "{body}");
    assert_eq!(body["plan"]["warnings"], json!([]), "{body}");

    let hello = upload_artifact(&app, &admin, &guest_fixture("hello.wasm")).await;
    let publish_hello =
        json!({"artifactRef": hello.0, "digest": hello.1, "manifest": component_manifest});
    let (status, published) = api(
        &app,
        Method::POST,
        &format!("/api/functions/{ADDRESS}/versions"),
        &admin,
        Some(publish_hello.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{published}");
    assert_eq!(published["version"], 2);
    let (_, v2) = api(
        &app,
        Method::GET,
        &format!("/api/functions/{ADDRESS}/versions/2"),
        &admin,
        None,
    )
    .await;
    assert_eq!(v2["manifest"]["runtime"], "component");
    assert_eq!(v2["manifest"]["entrypoint"], "wasi:http/incoming-handler");
    // Republishing the same bytes and manifest: the same version, 200.
    let (status, again) = api(
        &app,
        Method::POST,
        &format!("/api/functions/{ADDRESS}/versions"),
        &admin,
        Some(publish_hello),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["version"], 2);

    host.reconciler().reconcile_once(Utc::now()).await;
    assert_eq!(version_state(&app, &admin, 2).await, "READY");
    // Promote with the precondition: live is at 1, as the caller expects.
    let (status, body) = api(
        &app,
        Method::PUT,
        &format!("/api/functions/{ADDRESS}/aliases/live"),
        &admin,
        Some(json!({"version": 2, "expectedVersion": 1})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["previousVersion"], 1);
    host.reconciler().reconcile_once(Utc::now()).await;
    let health = http
        .get(format!("{base}/functions/{ADDRESS}/healthz"))
        .send()
        .await
        .unwrap();
    assert_eq!(health.status().as_u16(), 200);
    assert_eq!(health.json::<Value>().await.unwrap(), json!({"ok": true}));
    assert_eq!(
        host.reconciler()
            .registry()
            .peek(&document_address())
            .map(|f| f.version()),
        Some(2),
        "the component serves; version 1 is closed"
    );

    // ── 7. Disable: the host unloads it. Delete: nothing left to serve ───
    let (status, body) = api(
        &app,
        Method::PUT,
        &format!("/api/functions/{ADDRESS}"),
        &admin,
        Some(json!({"status": "DISABLED"})),
    )
    .await;
    assert!(status.is_success(), "{status} {body}");
    host.reconciler().reconcile_once(Utc::now()).await;
    let document = host.reconciler().document().unwrap();
    assert!(document.functions.is_empty(), "{document:?}");
    assert!(
        host.reconciler()
            .registry()
            .peek(&document_address())
            .is_none(),
        "unloaded"
    );
    let gone = http
        .get(format!("{base}/functions/{ADDRESS}/config?key=GREETING"))
        .send()
        .await
        .unwrap();
    assert_eq!(gone.status().as_u16(), 404);
    // The host's next beat reports nothing loaded.
    host.reconciler().reconcile_once(Utc::now()).await;
    let reported: (Value,) = sqlx::query_as("SELECT loaded FROM fn_hosts WHERE id = $1")
        .bind(HOST_ID)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(reported.0, json!([]));

    let (status, body) = api(
        &app,
        Method::DELETE,
        &format!("/api/functions/{ADDRESS}"),
        &admin,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let token = reqwest::Client::new()
        .post(format!("{platform_url}/oauth/token"))
        .form(&[
            ("grant_type", "client_credentials"),
            ("client_id", client_id.as_str()),
            ("client_secret", client_secret.as_str()),
        ])
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap()["access_token"]
        .as_str()
        .unwrap()
        .to_string();
    let doc: Value = reqwest::Client::new()
        .get(format!(
            "{platform_url}/control/functions/desired-state?pool={POOL}"
        ))
        .bearer_auth(token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        doc,
        json!({"pool": POOL, "functions": [], "unload": [], "publicRoutes": []})
    );

    tokio::time::timeout(Duration::from_secs(30), host.close())
        .await
        .expect("the host shuts down");
}

fn document_address() -> fc_function_abi::FunctionAddress {
    fc_function_abi::FunctionAddress::parse(ADDRESS).unwrap()
}
