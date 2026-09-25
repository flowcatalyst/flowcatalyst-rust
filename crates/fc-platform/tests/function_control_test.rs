//! The host control plane over HTTP against a real database (Java
//! `FunctionControlApiTest`, plus the artifact download route): the gate
//! (401 / 403 / the host role reaches nothing under `/api/functions`),
//! desired state's pool check, `ETag` and 304, heartbeat validation, the
//! host upsert with no event or audit, `READY` on the first ok report and
//! exactly once under a race, the stale-host purge, every emit check in
//! order with the ingest's `source` and `clientId`, and the artifact
//! stream. Requires Docker.

#[path = "support/mod.rs"]
mod support;

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::Router;
use chrono::{Duration, Utc};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

use fc_platform::application::entity::Application;
use fc_platform::domain::{Principal, UserScope};
use fc_platform::function::artifact::{ArtifactBlobStore, FileArtifactBlobStore};
use fc_platform::function::control_api::{function_control_router, FunctionControlState};
use fc_platform::function::desired_state::DesiredStateBuilder;
use fc_platform::function::settings_repository::FunctionSettingsRepository;
use fc_platform::function::Digest;
use fc_platform::role::entity::{permissions, AuthRole};
use fc_platform::service_account::entity::RoleAssignment;
use fc_platform::service_account::outbound_credentials::OutboundCredentialsResolver;
use fc_platform::shared::authorization_service::AuthorizationService;
use fc_platform::shared::middleware::{AppState, AuthLayer};
use support::TestApp;

use permissions::function::{FUNCTION_HOST_CONTROL, FUNCTION_VIEW};

// ── Harness ─────────────────────────────────────────────────────────────────

struct Harness {
    app: TestApp,
    router: Router,
    /// Anchor with `platform:function:host:control` only (the host role).
    host: String,
    /// Anchor with `platform:function:function:view` only.
    no_role: String,
    _artifacts: tempfile::TempDir,
    store: Arc<dyn ArtifactBlobStore>,
}

fn run() -> String {
    fc_platform::shared::tsid::generate_untyped().to_lowercase()
}

async fn token(app: &TestApp, scope: UserScope, grants: &[&str]) -> String {
    let n = run();
    let role = AuthRole::new("platform", format!("fnc-test-{n}"), "Control test")
        .with_permissions(grants.iter().map(|p| p.to_string()));
    app.repos.role_repo.insert(&role).await.expect("role");
    let mut principal = Principal::new_user(format!("fnc-{n}@flowcatalyst.test"), scope);
    principal.roles = vec![RoleAssignment::new(role.name.clone())];
    principal.all_applications = true;
    app.repos
        .principal_repo
        .insert(&principal)
        .await
        .expect("principal");
    app.auth_service
        .generate_access_token(&principal)
        .expect("token")
}

async fn harness() -> Harness {
    let app = TestApp::setup().await;
    let artifacts = tempfile::tempdir().unwrap();
    let store: Arc<dyn ArtifactBlobStore> =
        Arc::new(FileArtifactBlobStore::new(artifacts.path().to_path_buf()).unwrap());
    let state = FunctionControlState {
        desired: Arc::new(DesiredStateBuilder {
            functions: app.repos.function_repo.clone(),
            versions: app.repos.function_version_repo.clone(),
            hosts: app.repos.function_host_repo.clone(),
            settings: Arc::new(FunctionSettingsRepository::new(&app.pool, None)),
            routes: app.repos.function_route_repo.clone(),
            credentials: Arc::new(OutboundCredentialsResolver::new(
                app.repos.service_account_repo.clone(),
                None,
            )),
        }),
        functions: app.repos.function_repo.clone(),
        versions: app.repos.function_version_repo.clone(),
        hosts: app.repos.function_host_repo.clone(),
        applications: app.repos.application_repo.clone(),
        event_types: app.repos.event_type_repo.clone(),
        events: app.repos.event_repo.clone(),
        artifacts: Some(store.clone()),
        unit_of_work: app.unit_of_work.clone(),
    };
    // This suite's own control router (its own store); everything else goes
    // to the production router, so a host token can be shown to reach
    // nothing under /api/functions.
    let router = function_control_router(state).layer(AuthLayer::new(AppState {
        auth_service: app.auth_service.clone(),
        authz_service: Arc::new(AuthorizationService::new(app.repos.role_repo.clone())),
    }));
    let host = token(&app, UserScope::Anchor, &[FUNCTION_HOST_CONTROL]).await;
    let no_role = token(&app, UserScope::Anchor, &[FUNCTION_VIEW]).await;
    Harness {
        app,
        router,
        host,
        no_role,
        _artifacts: artifacts,
        store,
    }
}

struct Reply {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: Vec<u8>,
}

impl Reply {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body)
            .unwrap_or_else(|_| panic!("not JSON: {}", String::from_utf8_lossy(&self.body)))
    }

    fn error(&self) -> String {
        self.json()["error"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    }

    fn etag(&self) -> String {
        self.headers["etag"].to_str().unwrap().to_string()
    }
}

impl Harness {
    async fn send(
        &self,
        method: Method,
        path: &str,
        token: Option<&str>,
        body: Option<String>,
        extra: &[(&str, &str)],
    ) -> Reply {
        let mut req = Request::builder().method(method).uri(path);
        if let Some(t) = token {
            req = req.header("authorization", format!("Bearer {t}"));
        }
        for (k, v) in extra {
            req = req.header(*k, *v);
        }
        let body = match body {
            Some(b) => {
                req = req.header("content-type", "application/json");
                Body::from(b)
            }
            None => Body::empty(),
        };
        let router = if path.starts_with("/control/") {
            &self.router
        } else {
            &self.app.router
        };
        let resp = router
            .clone()
            .oneshot(req.body(body).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        let headers = resp.headers().clone();
        let body = resp
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec();
        Reply {
            status,
            headers,
            body,
        }
    }

    async fn get(&self, path: &str, token: Option<&str>) -> Reply {
        self.send(Method::GET, path, token, None, &[]).await
    }

    async fn post(&self, path: &str, token: Option<&str>, body: Value) -> Reply {
        self.send(Method::POST, path, token, Some(body.to_string()), &[])
            .await
    }

    async fn heartbeat(&self, host_id: &str, pool: &str, loaded: Value) -> Reply {
        self.post(
            "/control/functions/heartbeat",
            Some(&self.host.clone()),
            json!({"hostId": host_id, "pool": pool, "state": "ACTIVE", "loaded": loaded}),
        )
        .await
    }

    async fn emit(&self, host_id: &str, address: &str, version: i32, events: Value) -> Reply {
        self.post(
            "/control/functions/events",
            Some(&self.host.clone()),
            json!({"hostId": host_id, "address": address, "version": version, "events": events}),
        )
        .await
    }

    async fn scalar_i64(&self, sql: &str, bind: &str) -> i64 {
        let (n,): (i64,) = sqlx::query_as(sql)
            .bind(bind)
            .fetch_one(&self.app.pool)
            .await
            .unwrap();
        n
    }

    async fn ready_events_for(&self, function_id: &str) -> i64 {
        self.scalar_i64(
            "SELECT COUNT(*) FROM msg_events WHERE subject = $1 \
             AND type = 'platform:function:version:ready'",
            &format!("platform.function.{function_id}"),
        )
        .await
    }

    async fn version_state(&self, version_id: &str) -> String {
        let (state,): (String,) = sqlx::query_as("SELECT state FROM fn_versions WHERE id = $1")
            .bind(version_id)
            .fetch_one(&self.app.pool)
            .await
            .unwrap();
        state
    }
}

/// A function of a fresh application `fc-<run>-<tag>`, owned by a client
/// (or the platform), inserted directly: this suite tests the control
/// plane, not the management routes.
struct Fixture {
    id: String,
    address: String,
    app_code: String,
}

async fn function(h: &Harness, tag: &str, client_id: Option<&str>) -> Fixture {
    let app_code = format!("fc-{}-{tag}", run());
    let a = Application::new(&app_code, format!("Control {tag}"));
    h.app.repos.application_repo.insert(&a).await.unwrap();
    let id = format!("fnc_{}", &run()[..13]);
    sqlx::query(
        "INSERT INTO fn_functions (id, application_id, application_code, service_name, name, \
         client_id, runtime, status) VALUES ($1, $2, $3, 'svc', 'fn', $4, 'WASM', 'ACTIVE')",
    )
    .bind(&id)
    .bind(&a.id)
    .bind(&app_code)
    .bind(client_id)
    .execute(&h.app.pool)
    .await
    .unwrap();
    Fixture {
        address: format!("{app_code}.svc.fn"),
        id,
        app_code,
    }
}

async fn publish(
    h: &Harness,
    f: &Fixture,
    version: i32,
    pool: &str,
    artifact_ref: Option<&str>,
) -> String {
    let id = format!("fnv_{}", &run()[..13]);
    let digest = format!("sha256:{:064x}", version as u64 + 0x1000 * id.len() as u64);
    let manifest = json!({"runtime": "wasm", "entrypoint": "wasi_http_incoming_handler",
        "pool": pool, "warm": false,
        "limits": {"maxDurationMs": 30000, "maxConcurrency": 32, "wasmMemoryMb": 64},
        "endpoints": [], "subscriptions": [], "schedules": [], "public": [],
        "config": [], "secrets": [], "db": [], "httpAllow": []});
    sqlx::query(
        "INSERT INTO fn_versions (id, function_id, version, artifact_ref, digest, manifest, \
         state, published_by) VALUES ($1, $2, $3, $4, $5, $6::jsonb, 'PUBLISHED', 'prn_publisher')",
    )
    .bind(&id)
    .bind(&f.id)
    .bind(version)
    .bind(artifact_ref.unwrap_or("oci://artifact"))
    .bind(&digest)
    .bind(manifest.to_string())
    .execute(&h.app.pool)
    .await
    .unwrap();
    id
}

async fn promote(h: &Harness, f: &Fixture, version_id: &str) {
    sqlx::query(
        "INSERT INTO fn_aliases (function_id, alias, version_id, updated_by) \
         VALUES ($1, 'live', $2, 'prn_promoter') \
         ON CONFLICT (function_id, alias) DO UPDATE SET version_id = EXCLUDED.version_id",
    )
    .bind(&f.id)
    .bind(version_id)
    .execute(&h.app.pool)
    .await
    .unwrap();
}

async fn event_type(h: &Harness, code: &str, archived: bool) {
    let parts: Vec<&str> = code.split(':').collect();
    sqlx::query(
        "INSERT INTO msg_event_types (id, code, name, status, source, client_scoped, \
         application, subdomain, aggregate, created_at, updated_at) \
         VALUES ($1, $2, 'E', $3, 'API', false, $4, $5, $6, NOW(), NOW())",
    )
    .bind(fc_platform::shared::tsid::generate_untyped())
    .bind(code)
    .bind(if archived { "ARCHIVED" } else { "CURRENT" })
    .bind(parts[0])
    .bind(parts[1])
    .bind(parts[2])
    .execute(&h.app.pool)
    .await
    .unwrap();
}

fn event(event_type: &str, dedup: &str, data: Value) -> Value {
    json!({"type": event_type, "dedupId": dedup, "data": data})
}

// ── The gate ────────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires Docker"]
async fn every_route_is_401_without_a_credential_and_403_without_the_host_role() {
    let h = harness().await;
    let routes = [
        (Method::GET, "/control/functions/desired-state?pool=default"),
        (Method::POST, "/control/functions/heartbeat"),
        (Method::POST, "/control/functions/events"),
        (Method::GET, "/control/functions/artifacts/fnv_x"),
    ];
    let client_host = token(&h.app, UserScope::Client, &[FUNCTION_HOST_CONTROL]).await;
    for (method, path) in routes {
        let body = (method == Method::POST).then(|| "{}".to_string());
        let none = h.send(method.clone(), path, None, body.clone(), &[]).await;
        assert_eq!(none.status, StatusCode::UNAUTHORIZED, "{path}");
        assert_eq!(none.error(), "UNAUTHORIZED", "{path}");

        let no_role = h
            .send(method.clone(), path, Some(&h.no_role), body.clone(), &[])
            .await;
        assert_eq!(no_role.status, StatusCode::FORBIDDEN, "{path}");
        assert_eq!(no_role.error(), "PERMISSION_REQUIRED", "{path}");

        let not_anchor = h
            .send(method.clone(), path, Some(&client_host), body, &[])
            .await;
        assert_eq!(not_anchor.status, StatusCode::FORBIDDEN, "{path}");
        assert_eq!(not_anchor.error(), "ANCHOR_REQUIRED", "{path}");
    }
    let ok = h
        .get(
            "/control/functions/desired-state?pool=default",
            Some(&h.host),
        )
        .await;
    assert_eq!(ok.status, StatusCode::OK);
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn the_host_role_can_do_nothing_under_api_functions() {
    let h = harness().await;
    let get = h.get("/api/functions", Some(&h.host)).await;
    assert_eq!(get.status, StatusCode::FORBIDDEN);
    let post = h
        .post(
            "/api/functions",
            Some(&h.host.clone()),
            json!({"applicationCode": "nope", "serviceName": "svc", "name": "fn", "runtime": "wasm"}),
        )
        .await;
    assert_eq!(post.status, StatusCode::FORBIDDEN);
}

// ── Desired state ───────────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_missing_or_invalid_pool_is_400_pool_invalid() {
    let h = harness().await;
    for path in [
        "/control/functions/desired-state",
        "/control/functions/desired-state?pool=",
        "/control/functions/desired-state?pool=Not_A_Label",
    ] {
        let r = h.get(path, Some(&h.host)).await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{path}");
        assert_eq!(r.error(), "POOL_INVALID", "{path}");
    }
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn the_etag_is_stable_a_match_is_304_and_a_promote_moves_it() {
    let h = harness().await;
    let f = function(&h, "etag", Some("clt_etag")).await;
    let pool = format!("etag{}", &run()[..8]);
    let v1 = publish(&h, &f, 1, &pool, None).await;
    promote(&h, &f, &v1).await;
    let path = format!("/control/functions/desired-state?pool={pool}");

    let first = h.get(&path, Some(&h.host)).await;
    assert_eq!(first.status, StatusCode::OK);
    assert_eq!(first.headers["content-type"], "application/json");
    let etag = first.etag();
    assert_eq!(etag.len(), 66, "a quoted sha256: {etag}");
    let again = h.get(&path, Some(&h.host)).await;
    assert_eq!(again.etag(), etag, "the same state, the same ETag");
    assert_eq!(again.body, first.body);
    assert_eq!(
        format!(
            "\"{}\"",
            hex::encode(<sha2::Sha256 as sha2::Digest>::digest(&first.body))
        ),
        etag,
        "the ETag is the sha256 of exactly the body"
    );

    for header in [
        etag.clone(),
        format!("W/{etag}"),
        format!("\"x\", {etag}"),
        "*".into(),
    ] {
        let r = h
            .send(
                Method::GET,
                &path,
                Some(&h.host),
                None,
                &[("if-none-match", &header)],
            )
            .await;
        assert_eq!(r.status, StatusCode::NOT_MODIFIED, "{header}");
        assert!(r.body.is_empty(), "a 304 has no body");
        assert_eq!(r.etag(), etag, "the ETag is repeated on a 304");
    }
    let stale = h
        .send(
            Method::GET,
            &path,
            Some(&h.host),
            None,
            &[("if-none-match", "\"nope\"")],
        )
        .await;
    assert_eq!(stale.status, StatusCode::OK);

    let v2 = publish(&h, &f, 2, &pool, None).await;
    let with_candidate = h.get(&path, Some(&h.host)).await;
    assert_ne!(
        with_candidate.etag(),
        etag,
        "a new candidate moves the ETag"
    );
    promote(&h, &f, &v2).await;
    let after = h.get(&path, Some(&h.host)).await;
    assert_ne!(
        after.etag(),
        with_candidate.etag(),
        "a promote moves the ETag"
    );
    let doc = after.json();
    assert_eq!(doc["functions"][0]["versionId"], v2.as_str());
    assert_eq!(doc["functions"][0]["role"], "live");
}

// ── Heartbeat ───────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires Docker"]
async fn heartbeat_bodies_are_validated_as_javas() {
    let h = harness().await;
    let beat = |body: Value| {
        let h = &h;
        async move {
            h.post("/control/functions/heartbeat", Some(&h.host.clone()), body)
                .await
        }
    };
    for host_id in [
        json!(""),
        json!("a".repeat(101)),
        json!("bad host!"),
        Value::Null,
    ] {
        let r =
            beat(json!({"hostId": host_id, "pool": "default", "state": "ACTIVE", "loaded": []}))
                .await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST);
        assert_eq!(r.error(), "HOST_ID_INVALID", "{host_id}");
    }
    let r = beat(json!({"hostId": "h-pool", "pool": "Not_Valid", "state": "ACTIVE", "loaded": []}))
        .await;
    assert_eq!(r.error(), "POOL_INVALID");
    let r =
        beat(json!({"hostId": "h-state", "pool": "default", "state": "BOGUS", "loaded": []})).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(r.error(), "HOST_STATE_INVALID");
    for (entry, message) in [
        (
            json!({"address": "not.valid", "version": 1, "state": "LOADED"}),
            "loaded[0]: address: ",
        ),
        (
            json!({"address": "a.b.c", "version": 0, "state": "LOADED"}),
            "loaded[0]: version must be a positive integer",
        ),
        (
            json!({"address": "a.b.c", "version": 1, "state": "WEIRD"}),
            "loaded[0]: state must be REGISTERED, LOADED, or FAILED",
        ),
        (Value::Null, "loaded[0]: entry is required"),
    ] {
        let r = beat(
            json!({"hostId": "h-loaded", "pool": "default", "state": "ACTIVE", "loaded": [entry]}),
        )
        .await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST);
        assert_eq!(r.error(), "LOADED_INVALID");
        let got = r.json()["message"].as_str().unwrap().to_string();
        assert!(got.starts_with(message), "{got}");
    }
    let r = h
        .send(
            Method::POST,
            "/control/functions/heartbeat",
            Some(&h.host),
            Some("not json".into()),
            &[],
        )
        .await;
    assert_eq!(r.error(), "INVALID_JSON");
    assert!(h
        .app
        .repos
        .function_host_repo
        .find_by_id("h-loaded")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_heartbeat_upserts_the_host_with_no_event_or_audit_and_the_pool_is_fixed() {
    let h = harness().await;
    let host_id = format!("host-upsert-{}", run());
    let pool = format!("upsert{}", &run()[..6]);
    let r = h.heartbeat(&host_id, &pool, json!([])).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert!(r.body.is_empty());
    let first = h
        .app
        .repos
        .function_host_repo
        .find_by_id(&host_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.pool, pool);
    assert_eq!(first.state.as_str(), "ACTIVE");

    let r = h
        .post(
            "/control/functions/heartbeat",
            Some(&h.host.clone()),
            json!({"hostId": host_id, "pool": "elsewhere", "state": "DRAINING",
                   "loaded": [{"address": "a.b.c", "version": 2, "state": "FAILED", "error": "x".repeat(1500)}]}),
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let second = h
        .app
        .repos
        .function_host_repo
        .find_by_id(&host_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        second.state.as_str(),
        "DRAINING",
        "updated, not re-registered"
    );
    assert_eq!(
        second.pool, pool,
        "the pool never changes after the first registration"
    );
    assert_eq!(second.started_at, first.started_at);
    assert!(second.last_heartbeat > first.last_heartbeat);
    assert_eq!(second.loaded.len(), 1);
    assert_eq!(
        second.loaded[0].state.error().unwrap().len(),
        1000,
        "error cut at 1000"
    );

    assert_eq!(
        h.scalar_i64(
            "SELECT COUNT(*) FROM aud_logs WHERE entity_id = $1",
            &host_id
        )
        .await,
        0,
        "no audit"
    );
    assert_eq!(
        h.scalar_i64(
            "SELECT COUNT(*) FROM msg_events WHERE data::text LIKE $1",
            &format!("%{host_id}%")
        )
        .await,
        0,
        "no event"
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn an_ok_report_marks_a_published_version_ready_once_with_one_event() {
    let h = harness().await;
    let f = function(&h, "ready", Some("clt_ready")).await;
    let pool = format!("ready{}", &run()[..6]);
    let v1 = publish(&h, &f, 1, &pool, None).await;
    let v2 = publish(&h, &f, 2, &pool, None).await;
    let host_id = format!("host-ready-{}", run());

    // A FAILED report never marks anything; unknown addresses and versions
    // are ignored.
    let r = h
        .heartbeat(
            &host_id,
            &pool,
            json!([{"address": f.address, "version": 1, "state": "FAILED", "error": "boom"},
                   {"address": "nosuch.svc.fn", "version": 1, "state": "LOADED"},
                   {"address": f.address, "version": 99, "state": "LOADED"}]),
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(h.version_state(&v1).await, "PUBLISHED");
    assert_eq!(h.ready_events_for(&f.id).await, 0);

    // REGISTERED is ok too (Java LoadState.ok): v2 on a REGISTERED report,
    // v1 on a LOADED one.
    let loaded = json!([{"address": f.address, "version": 1, "state": "LOADED"},
                        {"address": f.address, "version": 2, "state": "REGISTERED"}]);
    let r = h.heartbeat(&host_id, &pool, loaded.clone()).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(h.version_state(&v1).await, "READY");
    assert_eq!(h.version_state(&v2).await, "READY");
    assert_eq!(h.ready_events_for(&f.id).await, 2);
    let (data, audits): (Value, i64) = (
        sqlx::query_as::<_, (Value,)>(
            "SELECT data FROM msg_events WHERE type = 'platform:function:version:ready' \
             AND data->>'versionId' = $1",
        )
        .bind(&v1)
        .fetch_one(&h.app.pool)
        .await
        .unwrap()
        .0,
        h.scalar_i64("SELECT COUNT(*) FROM aud_logs WHERE entity_id = $1", &f.id)
            .await,
    );
    assert_eq!(
        data,
        json!({"functionId": f.id, "address": f.address, "versionId": v1, "version": 1, "hostId": host_id})
    );
    assert_eq!(audits, 2, "one audit row per version made ready");

    let r = h.heartbeat(&host_id, &pool, loaded).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(
        h.ready_events_for(&f.id).await,
        2,
        "a second identical beat emits nothing"
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn two_hosts_racing_to_mark_one_version_ready_both_get_204_and_one_event() {
    let h = Arc::new(harness().await);
    for i in 0..10 {
        let f = function(&h, &format!("race{i}"), Some("clt_race")).await;
        let pool = format!("race{i}{}", &run()[..6]);
        let v = publish(&h, &f, 1, &pool, None).await;
        let loaded = json!([{"address": f.address, "version": 1, "state": "LOADED"}]);
        let beats = (0..2).map(|n| {
            let (h, pool, loaded) = (h.clone(), pool.clone(), loaded.clone());
            tokio::spawn(async move {
                h.heartbeat(&format!("host-race-{i}-{n}-{}", run()), &pool, loaded)
                    .await
                    .status
            })
        });
        for status in futures::future::join_all(beats).await {
            assert_eq!(status.unwrap(), StatusCode::NO_CONTENT, "iteration {i}");
        }
        assert_eq!(h.version_state(&v).await, "READY");
        assert_eq!(
            h.ready_events_for(&f.id).await,
            1,
            "iteration {i}: exactly one version:ready"
        );
    }
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn stale_hosts_are_purged_on_any_heartbeat_but_never_the_heartbeating_host() {
    let h = harness().await;
    let now = Utc::now();
    let (stale, fresh, me) = (
        format!("host-stale-{}", run()),
        format!("host-fresh-{}", run()),
        format!("host-self-{}", run()),
    );
    for (id, age) in [(&stale, 25), (&fresh, 23), (&me, 25)] {
        let at = now - Duration::hours(age);
        sqlx::query(
            "INSERT INTO fn_hosts (id, pool, state, loaded, started_at, last_heartbeat) \
             VALUES ($1, 'purge', 'ACTIVE', '[]', $2, $2)",
        )
        .bind(id)
        .bind(at)
        .execute(&h.app.pool)
        .await
        .unwrap();
    }
    let r = h.heartbeat(&me, "purge", json!([])).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let hosts = &h.app.repos.function_host_repo;
    assert!(
        hosts.find_by_id(&stale).await.unwrap().is_none(),
        "25 h stale: purged"
    );
    assert!(
        hosts.find_by_id(&fresh).await.unwrap().is_some(),
        "23 h: kept"
    );
    let me = hosts.find_by_id(&me).await.unwrap().unwrap();
    assert!(
        me.last_heartbeat > now - Duration::seconds(60),
        "refreshed, not purged"
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn status_and_pools_show_what_heartbeats_report() {
    let h = harness().await;
    let f = function(&h, "status", None).await;
    let pool = format!("status{}", &run()[..6]);
    publish(&h, &f, 1, &pool, None).await;
    let host_id = format!("host-status-{}", run());
    let r = h
        .heartbeat(
            &host_id,
            &pool,
            json!([{"address": f.address, "version": 1, "state": "LOADED"}]),
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let admin = h.no_role.clone();
    let status = h
        .get(
            &format!("/api/functions/{}/status", f.address),
            Some(&admin),
        )
        .await;
    assert_eq!(
        status.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&status.body)
    );
    let hosts = status.json()["hosts"].clone();
    assert_eq!(hosts.as_array().unwrap().len(), 1, "{hosts}");
    assert_eq!(hosts[0]["hostId"], host_id.as_str());
    assert_eq!(hosts[0]["pool"], pool.as_str());
    assert_eq!(hosts[0]["stale"], false);
    assert_eq!(
        hosts[0]["loaded"],
        json!([{"version": 1, "state": "LOADED"}])
    );
    assert_eq!(
        status.json()["versions"],
        json!([{"version": 1, "state": "READY"}])
    );
    let pools = h.get("/api/function-pools", Some(&admin)).await;
    let pools = pools.json();
    let mine: Vec<&Value> = pools
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["pool"] == pool.as_str())
        .collect();
    assert_eq!(mine, vec![&json!({"pool": pool, "hosts": 1})]);
}

// ── Events ──────────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires Docker"]
async fn an_unknown_or_stale_host_is_409_host_unknown() {
    let h = harness().await;
    let f = function(&h, "emithu", Some("clt_x")).await;
    let pool = format!("emithu{}", &run()[..6]);
    let v = publish(&h, &f, 1, &pool, None).await;
    promote(&h, &f, &v).await;
    let events = json!([event("whatever:sub:agg:evt", "dd-1", json!({}))]);
    let r = h.emit("no-such-host", &f.address, 1, events.clone()).await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert_eq!(r.error(), "HOST_UNKNOWN");
    let stale = format!("host-stale-{}", run());
    sqlx::query(
        "INSERT INTO fn_hosts (id, pool, state, loaded, started_at, last_heartbeat) \
         VALUES ($1, $2, 'ACTIVE', '[]', $3, $3)",
    )
    .bind(&stale)
    .bind(&pool)
    .bind(Utc::now() - Duration::seconds(75))
    .execute(&h.app.pool)
    .await
    .unwrap();
    let r = h.emit(&stale, &f.address, 1, events).await;
    assert_eq!(
        r.error(),
        "HOST_UNKNOWN",
        "a row outside the live window is unknown too"
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_function_or_version_the_host_does_not_serve_is_409() {
    let h = harness().await;
    let host_id = format!("host-fnsb-{}", run());
    let pool = format!("fnsb{}", &run()[..6]);
    assert_eq!(
        h.heartbeat(&host_id, &pool, json!([])).await.status,
        StatusCode::NO_CONTENT
    );
    let events = json!([event("whatever:sub:agg:evt", "dd-1", json!({}))]);
    let not_served = |r: Reply, why: &str| {
        assert_eq!(r.status, StatusCode::CONFLICT, "{why}");
        assert_eq!(r.error(), "FUNCTION_NOT_SERVED_BY_HOST", "{why}");
    };
    not_served(
        h.emit(&host_id, "nosuch.svc.fn", 1, events.clone()).await,
        "unknown address",
    );
    not_served(
        h.emit(&host_id, "Not.An.Address", 1, events.clone()).await,
        "malformed address",
    );
    let f = function(&h, "fnsb", Some("clt_x")).await;
    not_served(
        h.emit(&host_id, &f.address, 1, events.clone()).await,
        "never published",
    );
    let v = publish(&h, &f, 1, "otherpool", None).await;
    promote(&h, &f, &v).await;
    not_served(
        h.emit(&host_id, &f.address, 1, events.clone()).await,
        "another pool",
    );
    not_served(
        h.emit(&host_id, &f.address, 0, events.clone()).await,
        "version 0",
    );

    let g = function(&h, "fnsbdis", Some("clt_x")).await;
    let gv = publish(&h, &g, 1, &pool, None).await;
    promote(&h, &g, &gv).await;
    sqlx::query("UPDATE fn_functions SET status = 'DISABLED' WHERE id = $1")
        .bind(&g.id)
        .execute(&h.app.pool)
        .await
        .unwrap();
    not_served(
        h.emit(&host_id, &g.address, 1, events.clone()).await,
        "disabled",
    );

    // Live in this pool: checks 1 and 2 pass (it fails later, on ownership).
    let k = function(&h, "fnsbok", Some("clt_x")).await;
    let kv = publish(&h, &k, 1, &pool, None).await;
    promote(&h, &k, &kv).await;
    let r = h.emit(&host_id, &k.address, 1, events.clone()).await;
    assert_eq!(r.error(), "EVENT_TYPE_NOT_OWNED");
    // The newest published candidate is served too; an older one is not.
    publish(&h, &k, 2, &pool, None).await;
    let r = h.emit(&host_id, &k.address, 2, events.clone()).await;
    assert_eq!(r.error(), "EVENT_TYPE_NOT_OWNED", "the candidate is served");
    publish(&h, &k, 3, &pool, None).await;
    not_served(
        h.emit(&host_id, &k.address, 2, events).await,
        "no longer the newest candidate",
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_batch_must_be_one_to_a_hundred_unique_dedup_ids_with_bounded_object_data() {
    let h = harness().await;
    let host_id = format!("host-batch-{}", run());
    let pool = format!("batch{}", &run()[..6]);
    h.heartbeat(&host_id, &pool, json!([])).await;
    let f = function(&h, "batch", Some("clt_x")).await;
    let v = publish(&h, &f, 1, &pool, None).await;
    promote(&h, &f, &v).await;
    let t = "whatever:sub:agg:evt";
    let cases = [
        (json!([]), "BATCH_SIZE_INVALID"),
        (Value::Null, "BATCH_SIZE_INVALID"),
        (
            json!((0..101)
                .map(|i| event(t, &format!("d{i}"), json!({})))
                .collect::<Vec<_>>()),
            "BATCH_SIZE_INVALID",
        ),
        (json!([{"type": t, "data": {}}]), "DEDUP_ID_REQUIRED"),
        (
            json!([{"type": t, "dedupId": "  ", "data": {}}]),
            "DEDUP_ID_REQUIRED",
        ),
        (
            json!([event(t, "same", json!({})), event(t, "same", json!({}))]),
            "DEDUP_ID_DUPLICATE",
        ),
        (json!([{"type": t, "dedupId": "d"}]), "EVENT_DATA_INVALID"),
        (json!([event(t, "d", Value::Null)]), "EVENT_DATA_INVALID"),
        (json!([event(t, "d", json!([1]))]), "EVENT_DATA_INVALID"),
        (
            json!([event(t, "d", json!({"v": "x".repeat(300_000)}))]),
            "EVENT_DATA_TOO_LARGE",
        ),
    ];
    for (events, code) in cases {
        let r = h.emit(&host_id, &f.address, 1, events).await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{code}");
        assert_eq!(r.error(), code);
    }
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn ownership_gates_emit_all_or_nothing_and_a_success_is_ingested_as_the_function() {
    let h = harness().await;
    let host_id = format!("host-own-{}", run());
    let pool = format!("own{}", &run()[..6]);
    h.heartbeat(&host_id, &pool, json!([])).await;
    let f = function(&h, "own", Some("clt_own")).await;
    let v = publish(&h, &f, 1, &pool, None).await;
    promote(&h, &f, &v).await;
    let owned = format!("{}:orders:order:created", f.app_code);
    let archived = format!("{}:orders:order:archived", f.app_code);
    let other = format!("fc-{}-notmine:orders:order:created", run());
    event_type(&h, &owned, false).await;
    event_type(&h, &archived, true).await;
    event_type(&h, &other, false).await;

    for (t, why) in [
        (format!("{}:no:such:type", f.app_code), "unknown"),
        (archived.clone(), "archived"),
        (other.clone(), "another application's"),
    ] {
        let r = h
            .emit(
                &host_id,
                &f.address,
                1,
                json!([event(&t, "dd-x", json!({}))]),
            )
            .await;
        assert_eq!(r.status, StatusCode::FORBIDDEN, "{why}");
        assert_eq!(r.error(), "EVENT_TYPE_NOT_OWNED", "{why}");
    }
    let good = format!("dd-good-{}", run());
    let r = h
        .emit(
            &host_id,
            &f.address,
            1,
            json!([
                event(&owned, &good, json!({})),
                event(&other, "dd-bad", json!({}))
            ]),
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert!(r.json()["message"]
        .as_str()
        .unwrap()
        .starts_with("events[1]: "));
    assert_eq!(
        h.scalar_i64(
            "SELECT COUNT(*) FROM msg_events WHERE deduplication_id = $1",
            &good
        )
        .await,
        0,
        "nothing is written unless every event passes"
    );

    let dedup = format!("dd-ok-{}", run());
    let body = json!([{"type": owned, "dedupId": dedup, "data": {"k": 1}, "subject": "orders.1",
                       "correlationId": "corr-1", "causationId": "cause-1", "messageGroup": "orders:1"}]);
    let r = h.emit(&host_id, &f.address, 1, body.clone()).await;
    assert_eq!(
        r.status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&r.body)
    );
    let result = r.json();
    assert_eq!(result["results"][0]["status"], "SUCCESS");
    let id = result["results"][0]["id"].as_str().unwrap().to_string();
    let (row,): (Value,) = sqlx::query_as(
        "SELECT to_jsonb(e) FROM (SELECT type, source, client_id, subject, correlation_id, \
         causation_id, message_group, data, spec_version FROM msg_events WHERE id = $1) e",
    )
    .bind(&id)
    .fetch_one(&h.app.pool)
    .await
    .unwrap();
    assert_eq!(
        row,
        json!({"type": owned, "source": format!("function:{}", f.address), "client_id": "clt_own",
               "subject": "orders.1", "correlation_id": "corr-1", "causation_id": "cause-1",
               "message_group": "orders:1", "data": {"k": 1}, "spec_version": "1.0"})
    );

    // A repeated dedupId is the ingest path's own business: SUCCESS again.
    let r = h.emit(&host_id, &f.address, 1, body).await;
    assert_eq!(r.status, StatusCode::CREATED);
    assert_eq!(r.json()["results"][0]["status"], "SUCCESS");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_platform_owned_functions_emit_carries_no_client_id() {
    let h = harness().await;
    let host_id = format!("host-plat-{}", run());
    let pool = format!("plat{}", &run()[..6]);
    h.heartbeat(&host_id, &pool, json!([])).await;
    let f = function(&h, "plat", None).await;
    let v = publish(&h, &f, 1, &pool, None).await;
    promote(&h, &f, &v).await;
    let t = format!("{}:orders:order:created", f.app_code);
    event_type(&h, &t, false).await;
    let dedup = format!("dd-plat-{}", run());
    let r = h
        .emit(
            &host_id,
            &f.address,
            1,
            json!([event(&t, &dedup, json!({}))]),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED);
    let (client,): (Option<String>,) =
        sqlx::query_as("SELECT client_id FROM msg_events WHERE deduplication_id = $1")
            .bind(&dedup)
            .fetch_one(&h.app.pool)
            .await
            .unwrap();
    assert_eq!(client, None);
}

// ── Artifacts ───────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires Docker"]
async fn an_artifact_streams_from_the_store_and_anything_else_is_404() {
    let h = harness().await;
    let f = function(&h, "art", Some("clt_x")).await;
    let bytes: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    let digest = Digest::from_sha256(&<sha2::Sha256 as sha2::Digest>::digest(&bytes));
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), &bytes).unwrap();
    h.store.put(&f.id, &digest, file.path()).await.unwrap();
    let platform_ref = format!("platform://{}/{}", f.id, digest.hex());
    let v = publish(&h, &f, 1, "arts", Some(&platform_ref)).await;
    sqlx::query("UPDATE fn_versions SET digest = $2 WHERE id = $1")
        .bind(&v)
        .bind(digest.value())
        .execute(&h.app.pool)
        .await
        .unwrap();

    let r = h
        .get(&format!("/control/functions/artifacts/{v}"), Some(&h.host))
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.headers["content-type"], "application/octet-stream");
    assert_eq!(
        r.headers["content-length"],
        bytes.len().to_string().as_str()
    );
    assert!(
        r.headers.get("digest").is_none(),
        "no second source of truth for the digest"
    );
    assert_eq!(r.body, bytes);

    let r = h
        .get("/control/functions/artifacts/fnv_nosuch", Some(&h.host))
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    assert_eq!(r.error(), "FunctionVersion_NOT_FOUND");
    let oci = publish(&h, &f, 2, "arts", Some("oci://registry/x@sha256:00")).await;
    let r = h
        .get(
            &format!("/control/functions/artifacts/{oci}"),
            Some(&h.host),
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "not a platform:// ref");
    // A platform ref whose blob is gone.
    let gone = publish(
        &h,
        &f,
        3,
        "arts",
        Some(&format!("platform://{}/{}", f.id, "ab".repeat(32))),
    )
    .await;
    let r = h
        .get(
            &format!("/control/functions/artifacts/{gone}"),
            Some(&h.host),
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
}
