//! Promote, aliases and wiring over HTTP against a real database (Java
//! `FunctionTriggerSyncTest`, `PublishPromoteRetireTest`'s alias cases and
//! `FunctionApiTest`'s alias routes): publish, then promote `live` wires the
//! pool, subscriptions, scheduled jobs, links and public routes through each
//! object's own event; the same manifest again writes nothing; a changed
//! manifest creates, updates and deletes; a named alias wires nothing;
//! disabling pauses, enabling resumes, deleting the function deletes its
//! wiring; and every promote and remove-alias code. Requires Docker.

#[path = "support/mod.rs"]
mod support;

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::Router;
use chrono::Utc;
use serde_json::{json, Value};
use tower::ServiceExt;

use fc_function_signing::{Signatures, SignaturesMode};
use fc_platform::application::entity::Application;
use fc_platform::domain::{Principal, UserScope};
use fc_platform::function::api::{functions_router, FunctionsState};
use fc_platform::function::operations::trigger_sync::{pool_key, schedule_key, subscription_key};
use fc_platform::function::operations::{FunctionOperations, PublishChecks, TriggerSync};
use fc_platform::function::settings_repository::FunctionSettingsRepository;
use fc_platform::function::{FunctionLimits, PoolUrlTemplate};
use fc_platform::role::entity::{permissions, AuthRole};
use fc_platform::scheduled_job::scheduler::poller::latest_slot_in_window;
use fc_platform::service_account::entity::RoleAssignment;
use fc_platform::shared::authorization_service::{ApplicationAccessService, AuthorizationService};
use fc_platform::shared::middleware::{AppState, AuthLayer};
use support::{read_json, TestApp};

use permissions::function::{
    FUNCTION_DOMAIN_MANAGE, FUNCTION_MANAGE, FUNCTION_PROMOTE, FUNCTION_PUBLISH,
    FUNCTION_SECRET_MANAGE, FUNCTION_VIEW,
};

const ALL: &[&str] = &[
    FUNCTION_VIEW,
    FUNCTION_MANAGE,
    FUNCTION_PUBLISH,
    FUNCTION_PROMOTE,
    FUNCTION_DOMAIN_MANAGE,
    FUNCTION_SECRET_MANAGE,
];

const POOL_URL: &str = "http://fn-{pool}:8080";

// ── Harness ─────────────────────────────────────────────────────────────────

fn router(app: &TestApp) -> Router {
    router_hashing(app, fc_platform::function::operations::trigger_sync::hash8)
}

/// With a given trigger-key hasher: Java's test seam for a collision.
fn router_hashing(app: &TestApp, hasher: fn(&str) -> String) -> Router {
    let settings = Arc::new(FunctionSettingsRepository::new(&app.pool, None));
    let limits = FunctionLimits::defaults();
    let state = FunctionsState {
        functions: app.repos.function_repo.clone(),
        versions: app.repos.function_version_repo.clone(),
        hosts: app.repos.function_host_repo.clone(),
        settings: settings.clone(),
        policies: app.repos.function_policy_repo.clone(),
        domains: app.repos.function_domain_repo.clone(),
        routes: app.repos.function_route_repo.clone(),
        trigger_objects: app.repos.function_trigger_object_repo.clone(),
        app_access: Arc::new(ApplicationAccessService::new(
            app.repos.principal_repo.clone(),
            app.repos.application_repo.clone(),
        )),
        limits,
        ops: FunctionOperations {
            functions: app.repos.function_repo.clone(),
            versions: app.repos.function_version_repo.clone(),
            applications: app.repos.application_repo.clone(),
            clients: app.repos.client_repo.clone(),
            settings: settings.clone(),
            policies: app.repos.function_policy_repo.clone(),
            domains: app.repos.function_domain_repo.clone(),
            routes: app.repos.function_route_repo.clone(),
            trigger_sync: TriggerSync {
                hasher,
                ..TriggerSync::from_repositories(
                    &app.repos,
                    settings,
                    PoolUrlTemplate::parse(POOL_URL).unwrap(),
                )
            },
            limits,
            signatures: Signatures::resolve(SignaturesMode::Off, true, "").unwrap(),
            artifacts: None,
            publish_checks: PublishChecks {
                event_types: app.repos.event_type_repo.clone(),
                service_accounts: app.repos.service_account_repo.clone(),
                versions: app.repos.function_version_repo.clone(),
                functions: app.repos.function_repo.clone(),
                domains: app.repos.function_domain_repo.clone(),
                routes: app.repos.function_route_repo.clone(),
                hosts: app.repos.function_host_repo.clone(),
                limits,
            },
            unit_of_work: app.unit_of_work.clone(),
        },
    };
    let (router, _) = functions_router(state).split_for_parts();
    router.layer(AuthLayer::new(AppState {
        auth_service: app.auth_service.clone(),
        authz_service: Arc::new(AuthorizationService::new(app.repos.role_repo.clone())),
    }))
}

async fn anchor(app: &TestApp) -> String {
    let n = fc_platform::shared::tsid::generate_untyped().to_lowercase();
    let role = AuthRole::new("platform", format!("fnp-test-{n}"), "Promote test")
        .with_permissions(ALL.iter().map(|p| p.to_string()));
    app.repos.role_repo.insert(&role).await.expect("role");
    let mut principal =
        Principal::new_user(format!("fnp-{n}@flowcatalyst.test"), UserScope::Anchor);
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

async fn send(
    r: &Router,
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
    read_json(r.clone().oneshot(req.body(body).unwrap()).await.unwrap()).await
}

#[track_caller]
fn assert_error(got: &(StatusCode, Value), status: StatusCode, code: &str) {
    assert_eq!(got.0, status, "body: {}", got.1);
    assert_eq!(got.1["error"], code, "body: {}", got.1);
}

/// An application whose oldest active service account signs, and the
/// event types its manifests subscribe to.
async fn application(app: &TestApp, code: &str, event_types: &[&str]) -> Application {
    let a = Application::new(code, code.to_uppercase());
    app.repos.application_repo.insert(&a).await.expect("app");
    sqlx::query(
        "INSERT INTO iam_service_accounts (id, code, name, application_id, active, \
         wh_signing_secret_ref) VALUES ($1, $2, 'SA', $3, true, 'encrypted:x')",
    )
    .bind(fc_platform::shared::tsid::generate_untyped())
    .bind(format!("{code}-sa"))
    .bind(&a.id)
    .execute(&app.pool)
    .await
    .expect("service account");
    for et in event_types {
        let parts: Vec<&str> = et.split(':').collect();
        sqlx::query(
            "INSERT INTO msg_event_types (id, code, name, status, source, client_scoped, \
             application, subdomain, aggregate, created_at, updated_at) \
             VALUES ($1, $2, 'E', 'CURRENT', 'API', false, $3, $4, $5, NOW(), NOW())",
        )
        .bind(fc_platform::shared::tsid::generate_untyped())
        .bind(et)
        .bind(parts[0])
        .bind(parts[1])
        .bind(parts[2])
        .execute(&app.pool)
        .await
        .expect("event type");
    }
    a
}

async fn create_function(r: &Router, t: &str, app_code: &str, name: &str) -> String {
    let (status, out) = send(
        r,
        Method::POST,
        "/api/functions",
        t,
        Some(
            json!({"applicationCode": app_code, "serviceName": "svc", "name": name,
                    "runtime": "wasm"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{out}");
    out["id"].as_str().unwrap().to_string()
}

/// Publish `manifest` and make it READY, as a host's heartbeat would (P6).
async fn publish_ready(app: &TestApp, r: &Router, t: &str, path: &str, manifest: Value) -> i64 {
    let digest = format!(
        "sha256:{:064x}",
        rand_u64() as u128 * 1_000_003 + manifest.to_string().len() as u128
    );
    let (status, out) = send(
        r,
        Method::POST,
        &format!("{path}/versions"),
        t,
        Some(json!({"artifactRef": "oci://r/fn", "digest": digest, "manifest": manifest})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{out}");
    sqlx::query("UPDATE fn_versions SET state = 'READY', ready_at = NOW() WHERE id = $1")
        .bind(out["id"].as_str().unwrap())
        .execute(&app.pool)
        .await
        .unwrap();
    out["version"].as_i64().unwrap()
}

fn rand_u64() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos() as u64
        ^ std::process::id() as u64
}

async fn promote(
    r: &Router,
    t: &str,
    path: &str,
    alias: &str,
    version: i64,
) -> (StatusCode, Value) {
    send(
        r,
        Method::PUT,
        &format!("{path}/aliases/{alias}"),
        t,
        Some(json!({"version": version})),
    )
    .await
}

async fn count(app: &TestApp, sql: &str, bind: &str) -> i64 {
    let (n,): (i64,) = sqlx::query_as(sql)
        .bind(bind)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    n
}

async fn events_of_type(app: &TestApp, event_type: &str) -> i64 {
    count(
        app,
        "SELECT COUNT(*) FROM msg_events WHERE type = $1",
        event_type,
    )
    .await
}

/// `(kind, trigger_key, object_id)` rows, in key order.
async fn links(app: &TestApp, function_id: &str) -> Vec<(String, String, String)> {
    sqlx::query_as(
        "SELECT kind, trigger_key, object_id FROM fn_trigger_objects WHERE function_id = $1 \
         ORDER BY kind, trigger_key",
    )
    .bind(function_id)
    .fetch_all(&app.pool)
    .await
    .unwrap()
}

/// `(id, crons, timezone, application_id, target_url, payload, client_id)`.
type JobRow = (
    String,
    Vec<String>,
    String,
    Option<String>,
    Option<String>,
    Option<Value>,
    Option<String>,
);

const ET_A: &str = "billing:invoices:invoice:created";
const ET_B: &str = "billing:invoices:invoice:paid";

fn wired(concurrency: i32, subscriptions: Value, schedules: Value, public: Value) -> Value {
    json!({"runtime": "wasm", "entrypoint": "handle",
        "limits": {"maxConcurrency": concurrency},
        "endpoints": [{"path": "/events/*", "auth": "webhook"},
                      {"path": "/jobs/*", "auth": "webhook"},
                      {"path": "/api/*", "auth": "none"}],
        "subscriptions": subscriptions, "schedules": schedules, "public": public})
}

// ── live: wire, re-promote, change, roll back ───────────────────────────────

#[tokio::test]
#[ignore = "requires Docker"]
async fn promoting_live_wires_the_manifest_through_each_objects_own_events() {
    let app = TestApp::setup().await;
    let billing = application(&app, "billing", &[ET_A, ET_B]).await;
    let r = router(&app);
    let t = anchor(&app).await;
    let fid = create_function(&r, &t, "billing", "create").await;
    let path = "/api/functions/billing.svc.create";
    let (status, claim) = send(
        &r,
        Method::POST,
        "/api/function-domains",
        &t,
        Some(json!({"hostname": "acme.test"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{claim}");

    let m1 = wired(
        7,
        json!([{"eventType": ET_A, "path": "/events/a", "maxRetries": 5, "dataOnly": true}]),
        json!([{"cron": "0 0 9 * * 1-5", "timezone": "Europe/Amsterdam", "path": "/jobs/a",
                "payload": {"k": 1}}]),
        json!([{"hostname": "api.acme.test", "aliasPrefixes": ["qa"]}]),
    );
    let v1 = publish_ready(&app, &r, &t, path, m1.clone()).await;
    assert!(
        links(&app, &fid).await.is_empty(),
        "publish creates nothing"
    );

    // The dry run says what promote will do.
    let (_, check) = send(
        &r,
        Method::POST,
        &format!("{path}/manifest/check"),
        &t,
        Some(json!({"manifest": m1})),
    )
    .await;
    let plan = &check["plan"];
    assert_eq!(plan["pool"]["action"], "create", "{check}");
    assert_eq!(plan["subscriptions"][0]["action"], "create");
    assert_eq!(plan["schedules"][0]["action"], "create");
    assert_eq!(plan["publicRoutes"]["action"], "replace");
    assert_eq!(plan["toVersion"], v1 + 1);

    let (status, out) = promote(&r, &t, path, "live", v1).await;
    assert_eq!(status, StatusCode::OK, "{out}");
    assert_eq!(out["alias"], "live");
    assert_eq!(out["version"], v1);
    assert!(out.get("previousVersion").is_none(), "{out}");
    assert!(out["versionId"].as_str().unwrap().starts_with("fnv_"));

    // The pool: fn-<fid>, the manifest's maxConcurrency, platform-wide.
    let pool_code = pool_key(&fid);
    let (pool_id, concurrency, pool_client): (String, i32, Option<String>) =
        sqlx::query_as("SELECT id, concurrency, client_id FROM msg_dispatch_pools WHERE code = $1")
            .bind(&pool_code)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!((concurrency, pool_client), (7, None));

    // The subscription: source FUNCTION, the application's code, the pool,
    // the endpoint with no version in it, the entry's fields.
    let sub_code = subscription_key(&fid, ET_A);
    let sub: (
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        i32,
        bool,
        String,
    ) = sqlx::query_as(
        "SELECT id, source, target, name, application_code, dispatch_pool_id, max_retries, \
             data_only, mode FROM msg_subscriptions WHERE code = $1",
    )
    .bind(&sub_code)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(sub.1, "FUNCTION");
    assert_eq!(
        sub.2,
        "http://fn-default:8080/functions/billing.svc.create/events/a"
    );
    assert_eq!(sub.3, format!("billing.svc.create: {ET_A}"));
    assert_eq!(sub.4.as_deref(), Some("billing"));
    assert_eq!(sub.5.as_deref(), Some(pool_id.as_str()));
    assert_eq!((sub.6, sub.7, sub.8.as_str()), (5, true, "IMMEDIATE"));

    // The scheduled job: the application, the cron and the zone as
    // written, the target and payload.
    let job_code = schedule_key(&fid, "0 0 9 * * 1-5", Some("Europe/Amsterdam"));
    let job: JobRow = sqlx::query_as(
        "SELECT id, crons, timezone, application_id, target_url, payload, client_id \
             FROM msg_scheduled_jobs WHERE code = $1",
    )
    .bind(&job_code)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(job.1, vec!["0 0 9 * * 1-5".to_string()]);
    assert_eq!(job.2, "Europe/Amsterdam");
    assert_eq!(job.3.as_deref(), Some(billing.id.as_str()));
    assert_eq!(
        job.4.as_deref(),
        Some("http://fn-default:8080/functions/billing.svc.create/jobs/a")
    );
    assert_eq!(job.5, Some(json!({"k": 1})));
    assert_eq!(job.6, None, "a platform function's job is platform-scoped");
    // It fires on the Rust scheduler: a weekday 09:00 Amsterdam in a week.
    let now = Utc::now();
    let slot = latest_slot_in_window(&job.1, &job.2, now - chrono::Duration::days(7), now)
        .expect("fires within a week");
    let local = slot.with_timezone(&chrono_tz::Europe::Amsterdam);
    use chrono::{Datelike, Timelike};
    assert_eq!((local.hour(), local.minute()), (9, 0));
    assert!(local.weekday().number_from_monday() <= 5, "{local}");

    // Links, one per object, and the public route with its alias prefixes.
    assert_eq!(
        links(&app, &fid).await,
        vec![
            ("POOL".to_string(), pool_code.clone(), pool_id.clone()),
            ("SCHEDULED_JOB".to_string(), job_code.clone(), job.0.clone()),
            ("SUBSCRIPTION".to_string(), sub_code.clone(), sub.0.clone()),
        ]
    );
    let route: (String, String, Vec<String>) = sqlx::query_as(
        "SELECT hostname, path_prefix, alias_prefixes FROM fn_routes WHERE function_id = $1",
    )
    .bind(&fid)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(
        route,
        ("api.acme.test".into(), "/".into(), vec!["qa".to_string()])
    );

    // Each object's own event (and audit row); one alias:changed.
    assert_eq!(
        events_of_type(&app, "platform:function:alias:changed").await,
        1
    );
    assert_eq!(
        events_of_type(&app, "platform:admin:dispatch-pool:created").await,
        1
    );
    assert_eq!(
        events_of_type(&app, "platform:admin:subscription:created").await,
        1
    );
    assert_eq!(
        events_of_type(&app, "platform:admin:scheduledjob:created").await,
        1
    );
    assert_eq!(app.audit_count_for(&sub.0).await, 1);
    assert_eq!(app.audit_count_for(&job.0).await, 1);
    assert_eq!(app.audit_count_for(&pool_id).await, 1);

    // The same manifest as v2: the plan is all unchanged, the promote writes
    // no wiring and no wiring event.
    let v2 = publish_ready(&app, &r, &t, path, m1.clone()).await;
    let (_, check) = send(
        &r,
        Method::POST,
        &format!("{path}/manifest/check"),
        &t,
        Some(json!({"manifest": m1})),
    )
    .await;
    let plan = &check["plan"];
    assert_eq!(plan["fromVersion"], v1);
    assert_eq!(plan["pool"]["action"], "unchanged", "{check}");
    assert_eq!(plan["subscriptions"][0]["action"], "unchanged", "{check}");
    assert_eq!(plan["schedules"][0]["action"], "unchanged", "{check}");
    assert_eq!(plan["publicRoutes"]["action"], "unchanged", "{check}");
    let events_before = count(
        &app,
        "SELECT COUNT(*) FROM msg_events WHERE type LIKE $1",
        "platform:admin:%",
    )
    .await;
    let (status, out) = promote(&r, &t, path, "live", v2).await;
    assert_eq!(status, StatusCode::OK, "{out}");
    assert_eq!(out["previousVersion"], v1);
    assert_eq!(
        count(
            &app,
            "SELECT COUNT(*) FROM msg_events WHERE type LIKE $1",
            "platform:admin:%"
        )
        .await,
        events_before,
        "re-promoting the same manifest writes no wiring"
    );
    assert_eq!(links(&app, &fid).await.len(), 3);
    assert_eq!(
        events_of_type(&app, "platform:function:alias:changed").await,
        2
    );

    // v3: ET_A dropped, ET_B added, concurrency changed, the schedule moved,
    // the route dropped.
    let m3 = wired(
        9,
        json!([{"eventType": ET_B, "path": "/events/b"}]),
        json!([{"cron": "0 0 0 13 * 5", "path": "/jobs/b"}]),
        json!([]),
    );
    let v3 = publish_ready(&app, &r, &t, path, m3).await;
    let (status, out) = promote(&r, &t, path, "live", v3).await;
    assert_eq!(status, StatusCode::OK, "{out}");
    let got = links(&app, &fid).await;
    let keys: Vec<&str> = got.iter().map(|l| l.1.as_str()).collect();
    let job_b = schedule_key(&fid, "0 0 0 13 * 5", None);
    let sub_b = subscription_key(&fid, ET_B);
    assert_eq!(
        keys,
        vec![pool_code.as_str(), job_b.as_str(), sub_b.as_str()]
    );
    assert_eq!(got[0].2, pool_id, "the pool is updated in place");
    assert_eq!(
        count(
            &app,
            "SELECT COUNT(*) FROM msg_subscriptions WHERE id = $1",
            &sub.0
        )
        .await,
        0
    );
    assert_eq!(
        count(
            &app,
            "SELECT COUNT(*) FROM msg_scheduled_jobs WHERE id = $1",
            &job.0
        )
        .await,
        0
    );
    let (concurrency,): (i32,) =
        sqlx::query_as("SELECT concurrency FROM msg_dispatch_pools WHERE id = $1")
            .bind(&pool_id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(concurrency, 9);
    // Java's either-day cron, stored as written: the scheduler reads it.
    let (crons, tz): (Vec<String>, String) =
        sqlx::query_as("SELECT crons, timezone FROM msg_scheduled_jobs WHERE code = $1")
            .bind(&job_b)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(crons, vec!["0 0 0 13 * 5"]);
    assert_eq!(tz, "UTC");
    assert_eq!(
        count(
            &app,
            "SELECT COUNT(*) FROM fn_routes WHERE function_id = $1",
            &fid
        )
        .await,
        0
    );
    assert_eq!(
        events_of_type(&app, "platform:admin:subscription:deleted").await,
        1
    );
    assert_eq!(
        events_of_type(&app, "platform:admin:scheduledjob:deleted").await,
        1
    );
    assert_eq!(
        events_of_type(&app, "platform:admin:dispatch-pool:updated").await,
        1
    );

    // Rolling back to v1 restores v1's set.
    let (status, _) = promote(&r, &t, path, "live", v1).await;
    assert_eq!(status, StatusCode::OK);
    let keys: Vec<String> = links(&app, &fid).await.into_iter().map(|l| l.1).collect();
    assert_eq!(
        keys,
        vec![pool_code.clone(), job_code.clone(), sub_code.clone()]
    );
    assert_eq!(
        count(
            &app,
            "SELECT COUNT(*) FROM fn_routes WHERE function_id = $1",
            &fid
        )
        .await,
        1
    );

    // A named alias wires nothing, and GET lists both aliases.
    let wiring_before = links(&app, &fid).await;
    let (status, out) = promote(&r, &t, path, "qa", v3).await;
    assert_eq!(status, StatusCode::OK, "{out}");
    assert_eq!(links(&app, &fid).await, wiring_before);
    let (status, aliases) = send(&r, Method::GET, &format!("{path}/aliases"), &t, None).await;
    assert_eq!(status, StatusCode::OK);
    let names: Vec<(&str, i64)> = aliases
        .as_array()
        .unwrap()
        .iter()
        .map(|a| (a["alias"].as_str().unwrap(), a["version"].as_i64().unwrap()))
        .collect();
    assert_eq!(names, vec![("live", v1), ("qa", v3)]);
    assert!(aliases[0]["updatedBy"]
        .as_str()
        .unwrap()
        .starts_with("prn_"));
    assert!(aliases[0]["updatedAt"].as_str().unwrap().ends_with('Z'));

    // Disable pauses the linked subscription and job, never the pool;
    // enable resumes them.
    let status_of = |table: &'static str| {
        let app = &app;
        let sub_code = sub_code.clone();
        let job_code = job_code.clone();
        async move {
            let code = if table == "msg_subscriptions" {
                sub_code
            } else {
                job_code
            };
            let (s,): (String,) =
                sqlx::query_as(&format!("SELECT status FROM {table} WHERE code = $1"))
                    .bind(code)
                    .fetch_one(&app.pool)
                    .await
                    .unwrap();
            s
        }
    };
    let (status, _) = send(
        &r,
        Method::PUT,
        path,
        &t,
        Some(json!({"status": "DISABLED"})),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(status_of("msg_subscriptions").await, "PAUSED");
    assert_eq!(status_of("msg_scheduled_jobs").await, "PAUSED");
    assert_eq!(
        events_of_type(&app, "platform:admin:subscription:paused").await,
        1
    );
    assert_eq!(
        events_of_type(&app, "platform:admin:scheduledjob:paused").await,
        1
    );
    // A disabled function cannot be promoted.
    assert_error(
        &promote(&r, &t, path, "live", v2).await,
        StatusCode::CONFLICT,
        "FUNCTION_DISABLED",
    );
    let (status, _) = send(&r, Method::PUT, path, &t, Some(json!({"status": "ACTIVE"}))).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(status_of("msg_subscriptions").await, "ACTIVE");
    assert_eq!(status_of("msg_scheduled_jobs").await, "ACTIVE");

    // Delete removes the wiring, each object with its own event, and the
    // links and routes cascade.
    let (status, _) = send(&r, Method::DELETE, path, &t, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        count(
            &app,
            "SELECT COUNT(*) FROM msg_subscriptions WHERE code LIKE $1",
            &format!("fn-{}%", &pool_code[3..])
        )
        .await,
        0
    );
    assert_eq!(
        count(
            &app,
            "SELECT COUNT(*) FROM msg_scheduled_jobs WHERE code LIKE $1",
            &format!("fn-{}%", &pool_code[3..])
        )
        .await,
        0
    );
    assert_eq!(
        count(
            &app,
            "SELECT COUNT(*) FROM msg_dispatch_pools WHERE code = $1",
            &pool_code
        )
        .await,
        0
    );
    assert!(links(&app, &fid).await.is_empty());
    assert_eq!(
        count(
            &app,
            "SELECT COUNT(*) FROM fn_routes WHERE function_id = $1",
            &fid
        )
        .await,
        0
    );
    assert_eq!(
        events_of_type(&app, "platform:admin:dispatch-pool:deleted").await,
        1
    );
    // v3 dropped one of each, the rollback dropped v3's, the delete v1's.
    assert_eq!(
        events_of_type(&app, "platform:admin:subscription:deleted").await,
        3
    );
    assert_eq!(
        events_of_type(&app, "platform:admin:scheduledjob:deleted").await,
        3
    );
}

// ── Every code ──────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires Docker"]
async fn promote_and_remove_alias_codes() {
    let app = TestApp::setup().await;
    application(&app, "billing", &[]).await;
    let r = router(&app);
    let t = anchor(&app).await;
    let fid = create_function(&r, &t, "billing", "codes").await;
    let path = "/api/functions/billing.svc.codes";
    let plain = json!({"runtime": "wasm", "entrypoint": "handle", "config": ["GREETING"]});

    // ALIAS_INVALID before anything is read (an unready version too).
    let (status, published) = send(
        &r,
        Method::POST,
        &format!("{path}/versions"),
        &t,
        Some(
            json!({"artifactRef": "oci://r/x", "digest": format!("sha256:{}", "1".repeat(64)),
                    "manifest": plain}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{published}");
    let v1 = published["version"].as_i64().unwrap();
    assert_error(
        &promote(&r, &t, path, "BAD", v1).await,
        StatusCode::BAD_REQUEST,
        "ALIAS_INVALID",
    );
    let not_ready = promote(&r, &t, path, "live", v1).await;
    assert_error(&not_ready, StatusCode::CONFLICT, "VERSION_NOT_READY");
    assert_eq!(
        not_ready.1["message"],
        format!("version {v1} has not been verified by any host in pool 'default' yet")
    );
    assert_error(
        &promote(&r, &t, path, "qa", v1).await,
        StatusCode::CONFLICT,
        "VERSION_NOT_READY",
    );
    let missing = promote(&r, &t, path, "live", 99).await;
    assert_error(
        &missing,
        StatusCode::NOT_FOUND,
        "FUNCTION_VERSION_NOT_FOUND",
    );
    assert_eq!(
        missing.1["message"],
        "FunctionVersion not found: billing.svc.codes#99"
    );
    assert_error(
        &promote(&r, &t, "/api/functions/billing.svc.nope", "live", 1).await,
        StatusCode::NOT_FOUND,
        "FUNCTION_NOT_FOUND",
    );

    sqlx::query("UPDATE fn_versions SET state = 'READY', ready_at = NOW() WHERE function_id = $1")
        .bind(&fid)
        .execute(&app.pool)
        .await
        .unwrap();
    let settings = promote(&r, &t, path, "live", v1).await;
    assert_error(&settings, StatusCode::CONFLICT, "SETTINGS_MISSING");
    assert_eq!(
        settings.1["message"],
        "the following config/secret keys have no value set: GREETING"
    );
    let (status, _) = send(
        &r,
        Method::PUT,
        &format!("{path}/config"),
        &t,
        Some(json!({"values": {"GREETING": "hi"}})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, out) = promote(&r, &t, path, "live", v1).await;
    assert_eq!(status, StatusCode::OK, "{out}");
    // A function with no triggers still gets its pool.
    assert_eq!(links(&app, &fid).await.len(), 1);
    assert_eq!(out["changed"], true);
    let changes = events_of_type(&app, "platform:function:alias:changed").await;
    // The alias already names v1: a no-op, 200 with changed false and
    // nothing written.
    let (status, unchanged) = promote(&r, &t, path, "live", v1).await;
    assert_eq!(status, StatusCode::OK, "{unchanged}");
    assert_eq!(
        unchanged,
        json!({"alias": "live", "version": v1, "versionId": out["versionId"], "previousVersion": v1, "changed": false})
    );
    assert_eq!(
        events_of_type(&app, "platform:function:alias:changed").await,
        changes,
        "no event for a no-op"
    );

    // The optional precondition: qa has no version yet (0), so a stale
    // expectation is 412 and writes nothing; the right one promotes.
    let stale = send(
        &r,
        Method::PUT,
        &format!("{path}/aliases/qa"),
        &t,
        Some(json!({"version": v1, "expectedVersion": 3})),
    )
    .await;
    assert_error(
        &stale,
        StatusCode::PRECONDITION_FAILED,
        "ALIAS_VERSION_CONFLICT",
    );
    assert_eq!(
        stale.1["details"],
        json!({"alias": "qa", "expectedVersion": 3, "currentVersion": 0})
    );
    assert_eq!(
        stale.1["message"],
        "alias 'qa' points at no version, not the expected version 3"
    );
    let (status, body) = send(
        &r,
        Method::PUT,
        &format!("{path}/aliases/qa"),
        &t,
        Some(json!({"version": v1, "expectedVersion": 0})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["changed"], true);
    // If-Match carries the same precondition.
    let if_match = |value: &'static str| {
        let r = r.clone();
        let t = t.clone();
        async move {
            let req = Request::builder()
                .method(Method::PUT)
                .uri(format!("{path}/aliases/qa"))
                .header("authorization", format!("Bearer {t}"))
                .header("content-type", "application/json")
                .header("if-match", value)
                .body(Body::from(json!({"version": v1}).to_string()))
                .unwrap();
            read_json(r.oneshot(req).await.unwrap()).await
        }
    };
    assert_error(
        &if_match("\"7\"").await,
        StatusCode::PRECONDITION_FAILED,
        "ALIAS_VERSION_CONFLICT",
    );
    assert_error(
        &if_match("latest").await,
        StatusCode::BAD_REQUEST,
        "IF_MATCH_INVALID",
    );
    let (status, body) = if_match("W/\"1\"").await;
    assert_eq!(
        (status, &body["changed"]),
        (StatusCode::OK, &json!(false)),
        "{body}"
    );

    // Aliases: qa → v1, live cannot be removed, an unknown one is 404.
    assert_eq!(promote(&r, &t, path, "qa", v1).await.0, StatusCode::OK);
    let protected = send(
        &r,
        Method::DELETE,
        &format!("{path}/aliases/live"),
        &t,
        None,
    )
    .await;
    assert_error(&protected, StatusCode::CONFLICT, "ALIAS_PROTECTED");
    assert_eq!(
        protected.1["message"],
        "promote another version; live cannot be removed"
    );
    let unknown = send(
        &r,
        Method::DELETE,
        &format!("{path}/aliases/nope"),
        &t,
        None,
    )
    .await;
    assert_error(&unknown, StatusCode::NOT_FOUND, "ALIAS_NOT_FOUND");
    assert_eq!(unknown.1["message"], "Alias not found: nope");
    let (status, _) = send(&r, Method::DELETE, &format!("{path}/aliases/qa"), &t, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        events_of_type(&app, "platform:function:alias:removed").await,
        1
    );
    let (_, aliases) = send(&r, Method::GET, &format!("{path}/aliases"), &t, None).await;
    assert_eq!(aliases.as_array().unwrap().len(), 1);

    // A retired version: retire needs it off every alias first.
    let (status, v2) = send(
        &r,
        Method::POST,
        &format!("{path}/versions"),
        &t,
        Some(
            json!({"artifactRef": "oci://r/y", "digest": format!("sha256:{}", "2".repeat(64)),
                    "manifest": plain}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{v2}");
    let v2 = v2["version"].as_i64().unwrap();
    let (status, _) = send(
        &r,
        Method::POST,
        &format!("{path}/versions/{v2}/retire"),
        &t,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let retired = promote(&r, &t, path, "live", v2).await;
    assert_error(&retired, StatusCode::CONFLICT, "VERSION_RETIRED");
}

// ── A route taken between publish and promote ───────────────────────────────

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_route_promoted_by_another_function_first_is_taken() {
    let app = TestApp::setup().await;
    application(&app, "billing", &[]).await;
    let r = router(&app);
    let t = anchor(&app).await;
    let (status, _) = send(
        &r,
        Method::POST,
        "/api/function-domains",
        &t,
        Some(json!({"hostname": "shared.test"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let manifest = json!({"runtime": "wasm", "entrypoint": "handle",
        "endpoints": [{"path": "/api/*", "auth": "none"}],
        "public": [{"hostname": "shared.test"}]});
    let one = create_function(&r, &t, "billing", "one").await;
    let two = create_function(&r, &t, "billing", "two").await;
    // Both publish: neither has promoted yet.
    let v_one = publish_ready(
        &app,
        &r,
        &t,
        "/api/functions/billing.svc.one",
        manifest.clone(),
    )
    .await;
    let v_two = publish_ready(
        &app,
        &r,
        &t,
        "/api/functions/billing.svc.two",
        manifest.clone(),
    )
    .await;
    let (status, out) = promote(&r, &t, "/api/functions/billing.svc.one", "live", v_one).await;
    assert_eq!(status, StatusCode::OK, "{out}");

    let (_, check) = send(
        &r,
        Method::POST,
        "/api/functions/billing.svc.two/manifest/check",
        &t,
        Some(json!({"manifest": manifest})),
    )
    .await;
    // Publish itself now refuses it too; promote of the version already
    // published is where the race is caught.
    assert_eq!(check["valid"], false);
    let taken = promote(&r, &t, "/api/functions/billing.svc.two", "live", v_two).await;
    assert_error(&taken, StatusCode::CONFLICT, "PUBLIC_ROUTE_TAKEN");
    assert_eq!(
        taken.1["message"],
        "route 'shared.test/' is already taken by function 'billing.svc.one'"
    );
    // Nothing of the second promote was written: no alias, no pool.
    let (_, aliases) = send(
        &r,
        Method::GET,
        "/api/functions/billing.svc.two/aliases",
        &t,
        None,
    )
    .await;
    assert_eq!(aliases, json!([]));
    assert!(links(&app, &two).await.is_empty());
    assert_eq!(links(&app, &one).await.len(), 1);
}

// ── A trigger-key collision ─────────────────────────────────────────────────

/// Java `twoSubscriptionsWithCollidingKeysThrowsAtPromoteAndWritesNothing`:
/// two entries whose keys collide are an internal error at promote, and the
/// whole promote is rolled back, never one link silently overwritten.
#[tokio::test]
#[ignore = "requires Docker"]
async fn colliding_trigger_keys_fail_the_promote_and_write_nothing() {
    let app = TestApp::setup().await;
    application(&app, "billing", &[ET_A, ET_B]).await;
    let r = router_hashing(&app, |_| "00000000".to_string());
    let t = anchor(&app).await;
    let fid = create_function(&r, &t, "billing", "clash").await;
    let path = "/api/functions/billing.svc.clash";
    let m = wired(
        2,
        json!([{"eventType": ET_A, "path": "/events/a"}, {"eventType": ET_B, "path": "/events/b"}]),
        json!([]),
        json!([]),
    );
    let v = publish_ready(&app, &r, &t, path, m.clone()).await;
    let (_, check) = send(
        &r,
        Method::POST,
        &format!("{path}/manifest/check"),
        &t,
        Some(json!({"manifest": m})),
    )
    .await;
    assert_eq!(
        check["plan"]["conflicts"][0]["code"], "TRIGGER_KEY_COLLISION",
        "{check}"
    );
    let got = promote(&r, &t, path, "live", v).await;
    // An internal error: its body is the platform's masked 500; the code
    // itself is the plan's conflict, above.
    assert_eq!(got.0, StatusCode::INTERNAL_SERVER_ERROR, "{}", got.1);
    assert!(links(&app, &fid).await.is_empty());
    let (_, aliases) = send(&r, Method::GET, &format!("{path}/aliases"), &t, None).await;
    assert_eq!(aliases, json!([]));
    assert_eq!(
        events_of_type(&app, "platform:function:alias:changed").await,
        0
    );
}

// ── SDK syncs leave a function's objects alone (§4.2) ───────────────────────

#[tokio::test]
#[ignore = "requires Docker"]
async fn sdk_syncs_leave_a_functions_pool_and_jobs_alone() {
    use fc_platform::dispatch_pool::operations::{
        SyncDispatchPoolInput, SyncDispatchPoolsCommand, SyncDispatchPoolsUseCase,
    };
    use fc_platform::function::entity::TriggerObjectKind;
    use fc_platform::scheduled_job::operations::{
        ScheduledJobSyncEntry, SyncScheduledJobsCommand, SyncScheduledJobsUseCase,
    };
    use fc_platform::usecase::{ExecutionContext, UseCase};

    let app = TestApp::setup().await;
    application(&app, "billing", &[]).await;
    let r = router(&app);
    let t = anchor(&app).await;
    let fid = create_function(&r, &t, "billing", "kept").await;
    let path = "/api/functions/billing.svc.kept";
    let m = wired(
        3,
        json!([]),
        json!([{"cron": "0 0 1 * * *", "path": "/jobs/a"}]),
        json!([]),
    );
    let v = publish_ready(&app, &r, &t, path, m).await;
    assert_eq!(promote(&r, &t, path, "live", v).await.0, StatusCode::OK);
    let job_code = schedule_key(&fid, "0 0 1 * * *", None);
    let objects = app.repos.function_trigger_object_repo.clone();

    // A pool sync that lists the function's pool's code and removes the
    // unlisted: the function's pool is neither updated nor archived.
    let pools = SyncDispatchPoolsUseCase::new(
        app.repos.dispatch_pool_repo.clone(),
        app.unit_of_work.clone(),
    );
    let command = SyncDispatchPoolsCommand {
        application_code: "billing".into(),
        pools: vec![SyncDispatchPoolInput {
            code: pool_key(&fid),
            name: "hijacked".into(),
            description: None,
            rate_limit: None,
            concurrency: 1,
        }],
        remove_unlisted: true,
        protected_ids: objects
            .object_ids(TriggerObjectKind::Pool)
            .await
            .unwrap()
            .into_iter()
            .collect(),
    };
    pools
        .run(command, ExecutionContext::create("prn_t"))
        .await
        .into_result()
        .unwrap();
    let (name, concurrency, status): (String, i32, String) =
        sqlx::query_as("SELECT name, concurrency, status FROM msg_dispatch_pools WHERE code = $1")
            .bind(pool_key(&fid))
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(
        (name.as_str(), concurrency, status.as_str()),
        ("function billing.svc.kept", 3, "ACTIVE")
    );
    let command = SyncDispatchPoolsCommand {
        application_code: "billing".into(),
        pools: vec![],
        remove_unlisted: true,
        protected_ids: objects
            .object_ids(TriggerObjectKind::Pool)
            .await
            .unwrap()
            .into_iter()
            .collect(),
    };
    pools
        .run(command, ExecutionContext::create("prn_t"))
        .await
        .into_result()
        .unwrap();
    let (status,): (String,) =
        sqlx::query_as("SELECT status FROM msg_dispatch_pools WHERE code = $1")
            .bind(pool_key(&fid))
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(status, "ACTIVE");

    // A platform-scope job sync that archives the unlisted, and one that
    // lists the job's code: the function's job stays as promote made it.
    let jobs = SyncScheduledJobsUseCase::new(
        app.repos.scheduled_job_repo.clone(),
        app.unit_of_work.clone(),
    );
    for entries in [
        vec![],
        vec![ScheduledJobSyncEntry {
            code: job_code.clone(),
            name: "hijacked".into(),
            description: None,
            crons: vec!["0 0 2 * * *".into()],
            timezone: "UTC".into(),
            payload: None,
            concurrent: false,
            tracks_completion: false,
            timeout_seconds: None,
            delivery_max_attempts: 3,
            target_url: None,
        }],
    ] {
        let command = SyncScheduledJobsCommand {
            scope: "billing".into(),
            client_id: None,
            jobs: entries,
            archive_unlisted: true,
            protected_ids: objects
                .object_ids(TriggerObjectKind::ScheduledJob)
                .await
                .unwrap()
                .into_iter()
                .collect(),
        };
        jobs.run(command, ExecutionContext::create("prn_t"))
            .await
            .into_result()
            .unwrap();
        let (name, status, crons): (String, String, Vec<String>) =
            sqlx::query_as("SELECT name, status, crons FROM msg_scheduled_jobs WHERE code = $1")
                .bind(&job_code)
                .fetch_one(&app.pool)
                .await
                .unwrap();
        assert_eq!(
            (name.as_str(), status.as_str(), crons),
            (
                "billing.svc.kept: 0 0 1 * * *",
                "ACTIVE",
                vec!["0 0 1 * * *".to_string()]
            )
        );
    }
}
