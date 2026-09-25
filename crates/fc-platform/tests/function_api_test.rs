//! The function management API over HTTP against a real database (Java
//! `FunctionApiTest`, `FunctionSettingsApiTest`, `FunctionPolicyApiTest`,
//! `FunctionDomainApiTest`): status codes, Java's error bodies, reach that
//! answers 404 and never 403, pagination, secret masking, and the zone
//! conflict rules. Requires Docker.
//!
//! Versions, hosts, routes and trigger-object links are written by later
//! workstreams (publish, the host control plane, promote), so these tests
//! insert those rows directly to exercise the reads.

#[path = "support/mod.rs"]
mod support;

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

use fc_platform::application::entity::Application;
use fc_platform::domain::{Principal, UserScope};
use fc_platform::function::api::{functions_router, FunctionsState};
use fc_platform::function::operations::{FunctionOperations, PublishChecks, TriggerSync};
use fc_platform::function::settings_repository::FunctionSettingsRepository;
use fc_platform::function::{ClientCeilings, FunctionLimits, JsonNode, Manifest, Runtime};
use fc_platform::role::entity::{permissions, AuthRole};
use fc_platform::service_account::entity::RoleAssignment;
use fc_platform::shared::authorization_service::{ApplicationAccessService, AuthorizationService};
use fc_platform::shared::encryption_service::EncryptionService;
use fc_platform::shared::middleware::{AppState, AuthLayer};
use fc_platform::Client;
use support::{read_json, TestApp};

use permissions::function::{
    FUNCTION_DOMAIN_MANAGE, FUNCTION_MANAGE, FUNCTION_POLICY_MANAGE, FUNCTION_SECRET_MANAGE,
    FUNCTION_VIEW,
};

const ALL: &[&str] = &[
    FUNCTION_VIEW,
    FUNCTION_MANAGE,
    FUNCTION_SECRET_MANAGE,
    FUNCTION_POLICY_MANAGE,
    FUNCTION_DOMAIN_MANAGE,
];

// ── Harness helpers ─────────────────────────────────────────────────────────

/// Who a token is for.
struct As<'a> {
    scope: UserScope,
    /// The client of a CLIENT principal, or a PARTNER's assigned clients.
    clients: &'a [&'a str],
    permissions: &'a [&'a str],
    /// `None`: every application. `Some`: only these application ids.
    applications: Option<&'a [&'a str]>,
}

impl<'a> As<'a> {
    fn anchor(permissions: &'a [&'a str]) -> Self {
        As {
            scope: UserScope::Anchor,
            clients: &[],
            permissions,
            applications: None,
        }
    }

    fn client(client: &'a [&'a str], permissions: &'a [&'a str]) -> Self {
        As {
            scope: UserScope::Client,
            clients: client,
            permissions,
            applications: None,
        }
    }
}

/// Persist a principal holding a role with exactly `who.permissions` and
/// mint its token. The principal must exist: application scope is read
/// from its row.
async fn token(app: &TestApp, who: As<'_>) -> String {
    let n = fc_platform::shared::tsid::generate_untyped().to_lowercase();
    let role_code = format!("fn-test-{n}");
    let role = AuthRole::new("platform", &role_code, "Function test role")
        .with_permissions(who.permissions.iter().map(|p| p.to_string()));
    app.repos.role_repo.insert(&role).await.expect("role");

    let mut principal = Principal::new_user(format!("fn-{n}@flowcatalyst.test"), who.scope);
    match who.scope {
        UserScope::Client => principal = principal.with_client_id(who.clients[0]),
        UserScope::Partner => {
            principal.assigned_clients = who.clients.iter().map(|c| c.to_string()).collect()
        }
        UserScope::Anchor => {}
    }
    principal.roles = vec![RoleAssignment::new(role.name.clone())];
    principal.all_applications = who.applications.is_none();
    app.repos
        .principal_repo
        .insert(&principal)
        .await
        .expect("principal");
    if let Some(ids) = who.applications {
        principal.accessible_application_ids = ids.iter().map(|a| a.to_string()).collect();
        app.repos
            .principal_repo
            .update(&principal)
            .await
            .expect("grants");
    }
    app.auth_service
        .generate_access_token(&principal)
        .expect("token")
}

async fn application(app: &TestApp, code: &str) -> Application {
    let a = Application::new(code, code.to_uppercase());
    app.repos.application_repo.insert(&a).await.expect("app");
    a
}

async fn client(app: &TestApp, identifier: &str) -> Client {
    let c = Client::new(identifier.to_uppercase(), identifier);
    app.repos.client_repo.insert(&c).await.expect("client");
    c
}

/// A raw-body request, for bodies that are not JSON at all.
async fn send_raw(
    app: &TestApp,
    method: Method,
    path: &str,
    token: &str,
    body: &str,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    read_json(app.router.clone().oneshot(req).await.unwrap()).await
}

async fn get(app: &TestApp, path: &str, token: &str) -> (StatusCode, Value) {
    read_json(app.get(path, token).await).await
}

async fn post(app: &TestApp, path: &str, token: &str, body: Value) -> (StatusCode, Value) {
    read_json(app.post(path, token, body).await).await
}

async fn put(app: &TestApp, path: &str, token: &str, body: Value) -> (StatusCode, Value) {
    read_json(app.put(path, token, body).await).await
}

async fn delete(app: &TestApp, path: &str, token: &str) -> (StatusCode, Value) {
    read_json(app.delete(path, token).await).await
}

fn assert_error(got: &(StatusCode, Value), status: StatusCode, code: &str) {
    assert_eq!(got.0, status, "body: {}", got.1);
    assert_eq!(got.1["error"], code, "body: {}", got.1);
    assert!(got.1["message"].is_string(), "body: {}", got.1);
}

async fn create_function(
    app: &TestApp,
    token: &str,
    application_code: &str,
    service: &str,
    name: &str,
    client_id: Option<&str>,
) -> Value {
    let mut body = json!({
        "applicationCode": application_code,
        "serviceName": service,
        "name": name,
        "runtime": "wasm",
    });
    if let Some(c) = client_id {
        body["clientId"] = json!(c);
    }
    let (status, out) = post(app, "/api/functions", token, body).await;
    assert_eq!(status, StatusCode::CREATED, "body: {out}");
    out
}

fn stored_manifest(config: &[&str], secrets: &[&str]) -> String {
    let text = json!({
        "runtime": "wasm",
        "entrypoint": "handle",
        "config": config,
        "secrets": secrets,
    })
    .to_string();
    let defaults = FunctionLimits::defaults();
    Manifest::parse_strict(
        Some(&JsonNode::parse(&text).unwrap()),
        Runtime::Wasm,
        &defaults,
        &ClientCeilings::of(&defaults),
    )
    .expect("manifest")
    .to_json()
    .to_json_string()
}

/// A version row as publish (P4) would write it.
async fn insert_version(
    app: &TestApp,
    function_id: &str,
    version: i32,
    state: &str,
    manifest: &str,
) -> String {
    let id = fc_platform::shared::tsid::generate(fc_platform::EntityType::FunctionVersion);
    let digest = format!("sha256:{:064x}", version);
    sqlx::query(
        "INSERT INTO fn_versions (id, function_id, version, artifact_ref, digest, manifest, state, \
         published_by, ready_at, retired_at) \
         VALUES ($1, $2, $3, 'file:///a.wasm', $4, $5::jsonb, $6, 'prn_pub', \
                 CASE WHEN $6 = 'READY' THEN NOW() END, CASE WHEN $6 = 'RETIRED' THEN NOW() END)",
    )
    .bind(&id)
    .bind(function_id)
    .bind(version)
    .bind(&digest)
    .bind(manifest)
    .bind(state)
    .execute(&app.pool)
    .await
    .expect("insert version");
    id
}

/// Point `live` at a version, as promote (P5) would.
async fn set_live(app: &TestApp, function_id: &str, version_id: &str) {
    sqlx::query(
        "INSERT INTO fn_aliases (function_id, alias, version_id, updated_by) VALUES ($1, 'live', $2, 'prn_pub')",
    )
    .bind(function_id)
    .bind(version_id)
    .execute(&app.pool)
    .await
    .expect("set live");
}

/// The function routes alone, built with the given app key (or none) rather
/// than whatever `FLOWCATALYST_APP_KEY` the process has, behind the same
/// auth layer as the full router.
fn function_router(app: &TestApp, encryption: Option<Arc<EncryptionService>>) -> Router {
    let settings = Arc::new(FunctionSettingsRepository::new(&app.pool, encryption));
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
        limits: FunctionLimits::defaults(),
        ops: FunctionOperations {
            functions: app.repos.function_repo.clone(),
            versions: app.repos.function_version_repo.clone(),
            applications: app.repos.application_repo.clone(),
            clients: app.repos.client_repo.clone(),
            settings: settings.clone(),
            policies: app.repos.function_policy_repo.clone(),
            domains: app.repos.function_domain_repo.clone(),
            routes: app.repos.function_route_repo.clone(),
            trigger_sync: TriggerSync::from_repositories(
                &app.repos,
                settings.clone(),
                fc_platform::function::PoolUrlTemplate::parse("http://fn-{pool}:8080").unwrap(),
            ),
            limits: FunctionLimits::defaults(),
            signatures: fc_function_signing::Signatures::Off,
            artifacts: None,
            publish_checks: PublishChecks {
                event_types: app.repos.event_type_repo.clone(),
                service_accounts: app.repos.service_account_repo.clone(),
                versions: app.repos.function_version_repo.clone(),
                functions: app.repos.function_repo.clone(),
                domains: app.repos.function_domain_repo.clone(),
                routes: app.repos.function_route_repo.clone(),
                limits: FunctionLimits::defaults(),
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

fn is_micros_timestamp(v: &Value) -> bool {
    let s = v.as_str().unwrap_or("");
    s.len() == 27 && s.ends_with('Z') && s.as_bytes()[19] == b'.'
}

// ── Functions ───────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires Docker"]
async fn functions_crud_reach_and_pagination() {
    let app = TestApp::setup().await;
    let billing = application(&app, "billing").await;
    let shipping = application(&app, "shipping").await;
    let a = client(&app, "acme").await;
    let b = client(&app, "bravo").await;
    let anchor = token(&app, As::anchor(ALL)).await;

    // Create: 201, Java's response shape.
    let platform_fn = create_function(&app, &anchor, "billing", "invoices", "create", None).await;
    assert_eq!(platform_fn["address"], "billing.invoices.create");
    assert_eq!(platform_fn["applicationCode"], "billing");
    assert_eq!(platform_fn["serviceName"], "invoices");
    assert_eq!(platform_fn["name"], "create");
    assert_eq!(platform_fn["applicationId"], billing.id);
    assert_eq!(platform_fn["runtime"], "wasm");
    assert_eq!(platform_fn["status"], "ACTIVE");
    assert!(platform_fn["id"].as_str().unwrap().starts_with("fnc_"));
    for absent in ["clientId", "description", "live"] {
        assert!(
            platform_fn.get(absent).is_none(),
            "{absent} present: {platform_fn}"
        );
    }
    assert!(
        is_micros_timestamp(&platform_fn["createdAt"]),
        "{platform_fn}"
    );
    let a_fn = create_function(&app, &anchor, "billing", "invoices", "a-only", Some(&a.id)).await;
    assert_eq!(a_fn["clientId"], a.id);
    let b_fn = create_function(&app, &anchor, "billing", "invoices", "b-only", Some(&b.id)).await;
    let a_ship = create_function(&app, &anchor, "shipping", "labels", "print", Some(&a.id)).await;

    // Create failures, with Java's codes.
    let body = |app_code: &str, runtime: &str| json!({"applicationCode": app_code, "serviceName": "invoices", "name": "create", "runtime": runtime});
    assert_error(
        &post(&app, "/api/functions", &anchor, body("billing", "wasm")).await,
        StatusCode::CONFLICT,
        "FUNCTION_EXISTS",
    );
    assert_error(
        &post(&app, "/api/functions", &anchor, body("nowhere", "wasm")).await,
        StatusCode::NOT_FOUND,
        "Application_NOT_FOUND",
    );
    assert_error(
        &post(&app, "/api/functions", &anchor, body("billing", "python")).await,
        StatusCode::BAD_REQUEST,
        "RUNTIME_INVALID",
    );
    assert_error(
        &post(
            &app,
            "/api/functions",
            &anchor,
            json!({"serviceName": "x", "name": "y", "runtime": "jvm"}),
        )
        .await,
        StatusCode::BAD_REQUEST,
        "APPLICATION_CODE_REQUIRED",
    );
    let mut missing_client = body("billing", "wasm");
    missing_client["name"] = json!("other");
    missing_client["clientId"] = json!("clt_missing");
    assert_error(
        &post(&app, "/api/functions", &anchor, missing_client).await,
        StatusCode::NOT_FOUND,
        "Client_NOT_FOUND",
    );
    assert_error(
        &send_raw(&app, Method::POST, "/api/functions", &anchor, "{not json").await,
        StatusCode::BAD_REQUEST,
        "INVALID_JSON",
    );
    let snake = Application::new("logistics_portal", "LP");
    app.repos.application_repo.insert(&snake).await.unwrap();
    assert_error(
        &post(
            &app,
            "/api/functions",
            &anchor,
            body("logistics_portal", "wasm"),
        )
        .await,
        StatusCode::BAD_REQUEST,
        "APPLICATION_CODE_NOT_ADDRESSABLE",
    );

    // A client principal creating for another client, or for the platform:
    // 403 SCOPE_FORBIDDEN (nothing exists yet to hide).
    let a_manager = token(
        &app,
        As::client(&[&a.id], &[FUNCTION_VIEW, FUNCTION_MANAGE]),
    )
    .await;
    let mut for_b = body("billing", "wasm");
    for_b["name"] = json!("sneaky");
    for_b["clientId"] = json!(b.id);
    assert_error(
        &post(&app, "/api/functions", &a_manager, for_b).await,
        StatusCode::FORBIDDEN,
        "SCOPE_FORBIDDEN",
    );

    // Permissions: a view-only anchor cannot create; reads need only view.
    let viewer = token(&app, As::anchor(&[FUNCTION_VIEW])).await;
    let mut other = body("billing", "wasm");
    other["name"] = json!("viewer-made");
    assert_error(
        &post(&app, "/api/functions", &viewer, other).await,
        StatusCode::FORBIDDEN,
        "PERMISSION_REQUIRED",
    );
    assert_eq!(
        get(&app, "/api/functions/billing.invoices.create", &viewer)
            .await
            .0,
        StatusCode::OK
    );
    // The permission is checked before the body is bound, as in Java: a
    // caller without it gets 403 whatever the body holds.
    assert_error(
        &send_raw(&app, Method::POST, "/api/functions", &viewer, "{not json").await,
        StatusCode::FORBIDDEN,
        "PERMISSION_REQUIRED",
    );
    let nobody = token(&app, As::anchor(&[])).await;
    assert_error(
        &get(&app, "/api/functions", &nobody).await,
        StatusCode::FORBIDDEN,
        "PERMISSION_REQUIRED",
    );

    // Get: dots intact; a two-part address is 400, never 404.
    let (status, got) = get(&app, "/api/functions/billing.invoices.create", &anchor).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(got["id"], platform_fn["id"]);
    assert_error(
        &get(&app, "/api/functions/billing.invoices", &anchor).await,
        StatusCode::BAD_REQUEST,
        "ADDRESS_INVALID",
    );
    let unknown = get(&app, "/api/functions/billing.invoices.nope", &anchor).await;
    assert_error(&unknown, StatusCode::NOT_FOUND, "Function_NOT_FOUND");
    assert_eq!(
        unknown.1["message"],
        "Function not found: billing.invoices.nope"
    );

    // List: anchor sees every owner, in address order, Java's page envelope.
    let (status, list) = get(&app, "/api/functions", &anchor).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(list["total"], 4);
    assert_eq!(list["page"], 0);
    assert_eq!(list["size"], 20);
    assert_eq!(list["total_pages"], 1);
    let addresses: Vec<&str> = list["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["address"].as_str().unwrap())
        .collect();
    assert_eq!(
        addresses,
        [
            "billing.invoices.a-only",
            "billing.invoices.b-only",
            "billing.invoices.create",
            "shipping.labels.print"
        ]
    );
    let (_, page) = get(&app, "/api/functions?page=1&size=3", &anchor).await;
    assert_eq!(page["data"].as_array().unwrap().len(), 1);
    assert_eq!(
        (page["page"].clone(), page["size"].clone()),
        (json!(1), json!(3))
    );
    assert_eq!(page["total_pages"], 2);
    let (_, aliased) = get(&app, "/api/functions?limit=2", &anchor).await;
    assert_eq!(aliased["size"], 2);
    let bad = get(&app, "/api/functions?page=x&size=2", &anchor).await;
    assert_error(&bad, StatusCode::BAD_REQUEST, "VALIDATION");
    assert_eq!(bad.1["message"], "validation failed");
    assert_eq!(bad.1["details"]["errors"][0]["location"], "query.page");
    let (_, platform_only) = get(&app, "/api/functions?clientId=platform", &anchor).await;
    assert_eq!(platform_only["total"], 1);
    let (_, of_a) = get(&app, &format!("/api/functions?clientId={}", a.id), &anchor).await;
    assert_eq!(of_a["total"], 2);
    let (_, service) = get(&app, "/api/functions?address=billing.invoices.*", &anchor).await;
    assert_eq!(service["total"], 3);
    let (_, one) = get(&app, "/api/functions?address=shipping.*", &anchor).await;
    assert_eq!(one["total"], 1);
    assert_error(
        &get(&app, "/api/functions?address=bill*", &anchor).await,
        StatusCode::BAD_REQUEST,
        "ADDRESS_PATTERN_INVALID",
    );
    assert_error(
        &get(&app, "/api/functions?status=GONE", &anchor).await,
        StatusCode::BAD_REQUEST,
        "STATUS_INVALID",
    );

    // Reach: a client principal sees only its own client's functions, and
    // everything else answers 404, never 403, for reads and writes alike.
    let (_, a_list) = get(&app, "/api/functions", &a_manager).await;
    assert_eq!(a_list["total"], 2, "{a_list}");
    let ids: Vec<&Value> = a_list["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| &f["id"])
        .collect();
    assert!(ids.contains(&&a_fn["id"]) && ids.contains(&&a_ship["id"]));
    for hidden in ["billing.invoices.create", "billing.invoices.b-only"] {
        let path = format!("/api/functions/{hidden}");
        assert_error(
            &get(&app, &path, &a_manager).await,
            StatusCode::NOT_FOUND,
            "Function_NOT_FOUND",
        );
        assert_error(
            &put(&app, &path, &a_manager, json!({"description": "x"})).await,
            StatusCode::NOT_FOUND,
            "Function_NOT_FOUND",
        );
        assert_error(
            &delete(&app, &path, &a_manager).await,
            StatusCode::NOT_FOUND,
            "Function_NOT_FOUND",
        );
        assert_error(
            &get(&app, &format!("{path}/status"), &a_manager).await,
            StatusCode::NOT_FOUND,
            "Function_NOT_FOUND",
        );
        assert_error(
            &get(&app, &format!("{path}/config"), &a_manager).await,
            StatusCode::NOT_FOUND,
            "Function_NOT_FOUND",
        );
    }
    assert!(app
        .repos
        .function_repo
        .find_by_id(b_fn["id"].as_str().unwrap())
        .await
        .unwrap()
        .is_some());
    assert_eq!(
        get(&app, "/api/functions/billing.invoices.a-only", &a_manager)
            .await
            .0,
        StatusCode::OK
    );

    // An application-scoped principal reaches only its applications, even
    // inside its own client; with no grants it reaches nothing.
    let a_billing = token(
        &app,
        As {
            applications: Some(&[&billing.id]),
            ..As::client(&[&a.id], &[FUNCTION_VIEW])
        },
    )
    .await;
    let (_, scoped) = get(&app, "/api/functions", &a_billing).await;
    assert_eq!(scoped["total"], 1, "{scoped}");
    assert_eq!(scoped["data"][0]["id"], a_fn["id"]);
    assert_error(
        &get(&app, "/api/functions/shipping.labels.print", &a_billing).await,
        StatusCode::NOT_FOUND,
        "Function_NOT_FOUND",
    );
    let no_grants = token(
        &app,
        As {
            applications: Some(&[]),
            ..As::anchor(&[FUNCTION_VIEW])
        },
    )
    .await;
    let (_, none) = get(&app, "/api/functions", &no_grants).await;
    assert_eq!(none["total"], 0, "{none}");
    // Creating in an application out of the caller's scope: 403 FORBIDDEN.
    let a_billing_manager = token(
        &app,
        As {
            applications: Some(&[&billing.id]),
            ..As::client(&[&a.id], &[FUNCTION_MANAGE])
        },
    )
    .await;
    let forbidden = post(
        &app,
        "/api/functions",
        &a_billing_manager,
        json!({"applicationCode": "shipping", "serviceName": "x", "name": "y", "runtime": "wasm", "clientId": a.id}),
    )
    .await;
    assert_error(&forbidden, StatusCode::FORBIDDEN, "FORBIDDEN");
    assert_eq!(
        forbidden.1["message"],
        "Not authorised for application 'shipping'"
    );
    let _ = shipping;

    // Update: immutable fields refused against the raw body; description
    // and status applied; a no-op transition is 409.
    let path = "/api/functions/billing.invoices.create";
    for field in [
        "serviceName",
        "name",
        "applicationCode",
        "clientId",
        "runtime",
    ] {
        let refused = put(
            &app,
            path,
            &anchor,
            json!({ field: null, "description": "x" }),
        )
        .await;
        assert_error(
            &refused,
            StatusCode::BAD_REQUEST,
            "FUNCTION_IMMUTABLE_FIELD",
        );
        assert_eq!(
            refused.1["message"],
            format!("field '{field}' cannot be changed after creation")
        );
    }
    let (status, _) = put(
        &app,
        path,
        &anchor,
        json!({"description": "Makes invoices", "status": "DISABLED"}),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, updated) = get(&app, path, &anchor).await;
    assert_eq!(updated["description"], "Makes invoices");
    assert_eq!(updated["status"], "DISABLED");
    assert_error(
        &put(&app, path, &anchor, json!({"status": "DISABLED"})).await,
        StatusCode::CONFLICT,
        "FUNCTION_ALREADY_DISABLED",
    );
    assert_error(
        &put(&app, path, &anchor, json!({"status": "PAUSED"})).await,
        StatusCode::BAD_REQUEST,
        "STATUS_INVALID",
    );
    let (status, _) = put(
        &app,
        path,
        &anchor,
        json!({"description": " ", "status": "ACTIVE"}),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, cleared) = get(&app, path, &anchor).await;
    assert!(
        cleared.get("description").is_none(),
        "a blank description clears it: {cleared}"
    );
    let (_, disabled) = get(&app, "/api/functions?status=DISABLED", &anchor).await;
    assert_eq!(disabled["total"], 0);

    // One event and one audit row per write.
    let fid = platform_fn["id"].as_str().unwrap();
    assert_eq!(
        app.event_count_by_type("platform:function:function:created")
            .await,
        4
    );
    assert_eq!(
        app.event_count_by_type("platform:function:function:updated")
            .await,
        2
    );
    assert_eq!(
        app.event_count_for(&format!("platform.function.{fid}"))
            .await,
        3
    );
    assert_eq!(app.audit_count_for(fid).await, 3);
    let (source, group, data): (String, String, Value) = sqlx::query_as(
        "SELECT source, message_group, data FROM msg_events WHERE type = 'platform:function:function:updated' \
         AND subject = $1 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(format!("platform.function.{fid}"))
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(source, "platform:function");
    assert_eq!(group, format!("platform:function:{fid}"));
    assert_eq!(
        data,
        json!({"functionId": fid, "address": "billing.invoices.create", "status": "ACTIVE"})
    );

    // Delete: 204, then 404; versions cascade.
    let v = insert_version(&app, fid, 1, "PUBLISHED", &stored_manifest(&[], &[])).await;
    assert_eq!(delete(&app, path, &anchor).await.0, StatusCode::NO_CONTENT);
    assert_error(
        &get(&app, path, &anchor).await,
        StatusCode::NOT_FOUND,
        "Function_NOT_FOUND",
    );
    let (left,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM fn_versions WHERE id = $1")
        .bind(&v)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(left, 0);
    assert_eq!(
        app.event_count_by_type("platform:function:function:deleted")
            .await,
        1
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn status_pools_and_live() {
    let app = TestApp::setup().await;
    application(&app, "billing").await;
    let a = client(&app, "acme").await;
    let b = client(&app, "bravo").await;
    let anchor = token(&app, As::anchor(ALL)).await;
    let f = create_function(&app, &anchor, "billing", "invoices", "create", Some(&a.id)).await;
    let fid = f["id"].as_str().unwrap();
    let path = "/api/functions/billing.invoices.create";

    // Before anything is published: empty versions, hosts and wiring.
    let (status, empty) = get(&app, &format!("{path}/status"), &anchor).await;
    assert_eq!(status, StatusCode::OK, "{empty}");
    assert_eq!(
        empty,
        json!({"address": "billing.invoices.create", "status": "ACTIVE", "versions": [], "hosts": [], "wiring": []})
    );

    let manifest = stored_manifest(&[], &[]);
    let v1 = insert_version(&app, fid, 1, "READY", &manifest).await;
    insert_version(&app, fid, 2, "PUBLISHED", &manifest).await;
    set_live(&app, fid, &v1).await;
    let host_loaded = json!([
        {"address": "billing.invoices.create", "version": 1, "state": "LOADED"},
        {"address": "billing.invoices.create", "version": 2, "state": "FAILED", "error": "boom"},
        {"address": "other.svc.fn", "version": 1, "state": "LOADED"},
    ]);
    for (id, pool, loaded, ago) in [
        ("host-a", "default", host_loaded.clone(), 0),
        (
            "host-b",
            "default",
            json!([{"address": "other.svc.fn", "version": 1, "state": "LOADED"}]),
            0,
        ),
        (
            "host-c",
            "gpu",
            json!([{"address": "billing.invoices.create", "version": 1, "state": "REGISTERED"}]),
            600,
        ),
    ] {
        sqlx::query(
            "INSERT INTO fn_hosts (id, pool, state, loaded, last_heartbeat) \
             VALUES ($1, $2, 'ACTIVE', $3, NOW() - make_interval(secs => $4))",
        )
        .bind(id)
        .bind(pool)
        .bind(loaded)
        .bind(ago as f64)
        .execute(&app.pool)
        .await
        .unwrap();
    }
    sqlx::query(
        "INSERT INTO fn_trigger_objects (function_id, kind, object_id, trigger_key) \
         VALUES ($1, 'SUBSCRIPTION', 'sub_gone', 'fn-x-abcd1234')",
    )
    .bind(fid)
    .execute(&app.pool)
    .await
    .unwrap();

    let (_, status) = get(&app, &format!("{path}/status"), &anchor).await;
    assert_eq!(status["live"], json!({"version": 1}));
    assert_eq!(
        status["versions"],
        json!([{"version": 2, "state": "PUBLISHED"}, {"version": 1, "state": "READY"}])
    );
    let hosts = status["hosts"].as_array().unwrap();
    assert_eq!(
        hosts.len(),
        2,
        "only hosts reporting this address: {status}"
    );
    assert_eq!(hosts[0]["hostId"], "host-a");
    assert_eq!(hosts[0]["stale"], false);
    assert_eq!(
        hosts[0]["loaded"],
        json!([{"version": 1, "state": "LOADED"}, {"version": 2, "state": "FAILED", "error": "boom"}])
    );
    assert_eq!(hosts[1]["hostId"], "host-c");
    assert_eq!(hosts[1]["stale"], true);
    assert!(is_micros_timestamp(&hosts[0]["lastHeartbeat"]));
    assert_eq!(
        status["wiring"],
        json!([{"kind": "SUBSCRIPTION", "code": "fn-x-abcd1234", "objectId": "sub_gone", "present": false}])
    );

    // The function's `live` now shows on get and list.
    let (_, got) = get(&app, path, &anchor).await;
    assert_eq!(got["live"], json!({"version": 1, "versionId": v1}));
    let (_, list) = get(&app, "/api/functions", &anchor).await;
    assert_eq!(list["data"][0]["live"]["version"], 1);

    // Status reach: the owning client yes, another client 404.
    let owner = token(&app, As::client(&[&a.id], &[FUNCTION_VIEW])).await;
    assert_eq!(
        get(&app, &format!("{path}/status"), &owner).await.0,
        StatusCode::OK
    );
    let stranger = token(&app, As::client(&[&b.id], &[FUNCTION_VIEW])).await;
    assert_error(
        &get(&app, &format!("{path}/status"), &stranger).await,
        StatusCode::NOT_FOUND,
        "Function_NOT_FOUND",
    );

    // Pools: anchor and view; only hosts seen within 45 s count.
    let (status, pools) = get(&app, "/api/function-pools", &anchor).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(pools, json!([{"pool": "default", "hosts": 2}]));
    assert_error(
        &get(&app, "/api/function-pools", &owner).await,
        StatusCode::FORBIDDEN,
        "ANCHOR_REQUIRED",
    );
    let anchor_no_view = token(&app, As::anchor(&[FUNCTION_MANAGE])).await;
    assert_error(
        &get(&app, "/api/function-pools", &anchor_no_view).await,
        StatusCode::FORBIDDEN,
        "PERMISSION_REQUIRED",
    );
}

// ── Config and secrets ──────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires Docker"]
async fn config_and_secrets() {
    // Secrets need an app key: the function routes are built with one here
    // rather than through the process environment, which other tests read.
    let key = EncryptionService::generate_key();
    let mut app = TestApp::setup().await;
    app.router = function_router(&app, Some(Arc::new(EncryptionService::new(&key).unwrap())));

    application(&app, "billing").await;
    let anchor = token(&app, As::anchor(ALL)).await;
    let f = create_function(&app, &anchor, "billing", "invoices", "create", None).await;
    let fid = f["id"].as_str().unwrap().to_string();
    let path = "/api/functions/billing.invoices.create";

    // Config: empty, then a full replacement each time.
    let (status, empty) = get(&app, &format!("{path}/config"), &anchor).await;
    assert_eq!(status, StatusCode::OK, "{empty}");
    assert_eq!(
        empty,
        json!({"values": {}, "declared": [], "missing": [], "declaredBy": []})
    );
    let (status, set) = put(
        &app,
        &format!("{path}/config"),
        &anchor,
        json!({"values": {"GREETING": "hi", "A": "1"}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{set}");
    assert_eq!(set["values"], json!({"A": "1", "GREETING": "hi"}));
    let (_, replaced) = put(
        &app,
        &format!("{path}/config"),
        &anchor,
        json!({"values": {"OTHER": "x"}}),
    )
    .await;
    assert_eq!(replaced["values"], json!({"OTHER": "x"}));
    assert_error(
        &put(
            &app,
            &format!("{path}/config"),
            &anchor,
            json!({"values": {"1BAD": "x"}}),
        )
        .await,
        StatusCode::BAD_REQUEST,
        "SETTING_KEY_INVALID",
    );
    let many: serde_json::Map<String, Value> =
        (0..101).map(|i| (format!("K{i}"), json!("v"))).collect();
    assert_error(
        &put(
            &app,
            &format!("{path}/config"),
            &anchor,
            json!({ "values": many }),
        )
        .await,
        StatusCode::BAD_REQUEST,
        "SETTING_TOO_LARGE",
    );
    assert_error(
        &put(
            &app,
            &format!("{path}/config"),
            &anchor,
            json!({"values": {"BIG": "x".repeat(8193)}}),
        )
        .await,
        StatusCode::BAD_REQUEST,
        "SETTING_TOO_LARGE",
    );
    let view_only = token(&app, As::anchor(&[FUNCTION_VIEW])).await;
    assert_error(
        &put(
            &app,
            &format!("{path}/config"),
            &view_only,
            json!({"values": {}}),
        )
        .await,
        StatusCode::FORBIDDEN,
        "PERMISSION_REQUIRED",
    );
    let (keys,): (Value,) = sqlx::query_as(
        "SELECT data->'keys' FROM msg_events WHERE type = 'platform:function:config:updated' \
         ORDER BY created_at DESC LIMIT 1",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(keys, json!(["OTHER"]), "keys only, never values");

    // declared: before any promote it comes from the newest non-retired
    // version; then live first, the candidate's extras after.
    let v1 = insert_version(
        &app,
        &fid,
        1,
        "READY",
        &stored_manifest(&["A"], &["API_KEY"]),
    )
    .await;
    let (_, before) = get(&app, &format!("{path}/config"), &anchor).await;
    assert_eq!(before["declared"], json!(["A"]));
    assert_eq!(before["missing"], json!(["A"]));
    assert_eq!(before["declaredBy"], json!([{"version": 1, "keys": ["A"]}]));
    set_live(&app, &fid, &v1).await;
    insert_version(
        &app,
        &fid,
        2,
        "PUBLISHED",
        &stored_manifest(&["A", "B", "OTHER"], &[]),
    )
    .await;
    let (_, both) = get(&app, &format!("{path}/config"), &anchor).await;
    assert_eq!(both["declared"], json!(["A", "B", "OTHER"]));
    assert_eq!(both["missing"], json!(["A", "B"]));
    assert_eq!(
        both["declaredBy"],
        json!([{"version": 1, "keys": ["A"]}, {"version": 2, "keys": ["A", "B", "OTHER"]}])
    );
    let (_, narrowed) = get(&app, &format!("{path}/config?version=1"), &anchor).await;
    assert_eq!(
        narrowed["declaredBy"],
        json!([{"version": 1, "keys": ["A"]}])
    );
    assert_error(
        &get(&app, &format!("{path}/config?version=9"), &anchor).await,
        StatusCode::NOT_FOUND,
        "FunctionVersion_NOT_FOUND",
    );
    assert_error(
        &get(&app, &format!("{path}/config?version=x"), &anchor).await,
        StatusCode::BAD_REQUEST,
        "VERSION_INVALID",
    );

    // Secrets: never a value anywhere.
    const MARKER: &str = "sk_live_MARKER_31337";
    let secret = format!("{path}/secrets/API_KEY");
    let viewer_secret = token(&app, As::anchor(&[FUNCTION_VIEW, FUNCTION_MANAGE])).await;
    assert_error(
        &put(&app, &secret, &viewer_secret, json!({"value": MARKER})).await,
        StatusCode::FORBIDDEN,
        "PERMISSION_REQUIRED",
    );
    let put1 = app.put(&secret, &anchor, json!({"value": MARKER})).await;
    assert_eq!(put1.status(), StatusCode::NO_CONTENT);
    assert!(
        put1.into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .is_empty(),
        "no echo"
    );
    let (status, _) = put(
        &app,
        &secret,
        &anchor,
        json!({"value": format!("{MARKER}-2")}),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (value_ref,): (String,) = sqlx::query_as(
        "SELECT value_ref FROM fn_secrets WHERE function_id = $1 AND key = 'API_KEY'",
    )
    .bind(&fid)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert!(value_ref.starts_with("encrypted:"), "{value_ref}");
    assert!(!value_ref.contains(MARKER));
    let settings = fc_platform::function::settings_repository::FunctionSettingsRepository::new(
        &app.pool,
        Some(Arc::new(EncryptionService::new(&key).unwrap())),
    );
    let decrypted = settings
        .decrypt_secrets(&fid, &["API_KEY".to_string(), "NONE".to_string()])
        .await
        .unwrap();
    assert_eq!(
        decrypted.get("API_KEY").map(String::as_str),
        Some(&*format!("{MARKER}-2"))
    );
    assert_eq!(decrypted.len(), 1);

    let (status, secrets) = get(&app, &format!("{path}/secrets"), &anchor).await;
    assert_eq!(status, StatusCode::OK, "{secrets}");
    assert_eq!(secrets["keys"][0]["key"], "API_KEY");
    assert!(is_micros_timestamp(&secrets["keys"][0]["updatedAt"]));
    assert!(secrets["keys"][0]["updatedBy"].is_string());
    assert_eq!(
        secrets["keys"][0].as_object().unwrap().len(),
        3,
        "no value field"
    );
    assert_eq!(secrets["declared"], json!(["API_KEY"]));
    assert_eq!(secrets["missing"], json!([]));
    assert!(!secrets.to_string().contains(MARKER));

    let (event_data,): (String,) =
        sqlx::query_as("SELECT string_agg(data::text, ' ') FROM msg_events WHERE subject = $1")
            .bind(format!("platform.function.{fid}"))
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert!(!event_data.contains(MARKER), "secret in an event");
    let (audits,): (String,) = sqlx::query_as(
        "SELECT string_agg(COALESCE(operation_json::text, ''), ' ') FROM aud_logs WHERE entity_id = $1",
    )
    .bind(&fid)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert!(!audits.contains(MARKER), "secret in an audit row");
    assert!(
        audits.contains("\"value\": \"***\"") || audits.contains("\"value\":\"***\""),
        "{audits}"
    );
    assert_eq!(
        app.event_count_by_type("platform:function:secret:set")
            .await,
        2
    );

    assert_error(
        &put(&app, &secret, &anchor, json!({"value": ""})).await,
        StatusCode::BAD_REQUEST,
        "SETTING_VALUE_REQUIRED",
    );
    assert_error(
        &put(&app, &secret, &anchor, json!({})).await,
        StatusCode::BAD_REQUEST,
        "SETTING_VALUE_REQUIRED",
    );
    assert_error(
        &put(&app, &secret, &anchor, json!({"value": "x".repeat(8193)})).await,
        StatusCode::BAD_REQUEST,
        "SETTING_TOO_LARGE",
    );
    assert_error(
        &put(
            &app,
            &format!("{path}/secrets/bad%20key"),
            &anchor,
            json!({"value": "v"}),
        )
        .await,
        StatusCode::BAD_REQUEST,
        "SETTING_KEY_INVALID",
    );
    assert_eq!(
        delete(&app, &secret, &anchor).await.0,
        StatusCode::NO_CONTENT
    );
    assert!(!settings.has_secret(&fid, "API_KEY").await.unwrap());
    let again = delete(&app, &secret, &anchor).await;
    assert_error(&again, StatusCode::NOT_FOUND, "FunctionSecret_NOT_FOUND");
    assert_eq!(
        app.event_count_by_type("platform:function:secret:deleted")
            .await,
        1
    );

    // With no app key: every secret route is 503 and nothing is stored.
    app.router = function_router(&app, None);
    for got in [
        put(&app, &secret, &anchor, json!({"value": MARKER})).await,
        get(&app, &format!("{path}/secrets"), &anchor).await,
        delete(&app, &secret, &anchor).await,
    ] {
        assert_error(
            &got,
            StatusCode::SERVICE_UNAVAILABLE,
            "ENCRYPTION_UNCONFIGURED",
        );
    }
    assert!(!settings.has_secret(&fid, "API_KEY").await.unwrap());
    // Config needs no key.
    assert_eq!(
        get(&app, &format!("{path}/config"), &anchor).await.0,
        StatusCode::OK
    );
}

// ── Policies ────────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires Docker"]
async fn policies() {
    let app = TestApp::setup().await;
    let low = client(&app, "alpha").await;
    let high = client(&app, "zulu").await;
    let admin = token(&app, As::anchor(&[FUNCTION_POLICY_MANAGE])).await;
    let defaults = FunctionLimits::defaults();

    // No row: the effective default, stored false, no updatedAt.
    let (status, none) = get(&app, &format!("/api/function-policies/{}", low.id), &admin).await;
    assert_eq!(status, StatusCode::OK, "{none}");
    assert_eq!(
        none,
        json!({
            "owner": low.id,
            "signers": [],
            "ceilings": {
                "maxDurationMs": defaults.max_duration_ms(),
                "maxConcurrency": defaults.max_concurrency(),
                "maxWasmMemoryMb": defaults.wasm_memory_mb(),
                "maxDbPoolSize": defaults.db_pool_size(),
            },
            "stored": false,
        })
    );

    // PUT replaces; unset ceilings resolve to the defaults.
    let body = json!({
        "signers": [{"issuer": "https://issuer", "subject": "repo:acme/fns", "runtimes": ["JVM"]}],
        "ceilings": {"maxDurationMs": 9000},
    });
    let (status, saved) = put(
        &app,
        &format!("/api/function-policies/{}", high.id),
        &admin,
        body.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["stored"], true);
    assert_eq!(saved["signers"][0]["runtimes"], json!(["jvm"]));
    assert_eq!(saved["ceilings"]["maxDurationMs"], 9000);
    assert_eq!(
        saved["ceilings"]["maxConcurrency"],
        defaults.max_concurrency()
    );
    assert!(is_micros_timestamp(&saved["updatedAt"]));
    put(
        &app,
        &format!("/api/function-policies/{}", low.id),
        &admin,
        json!({}),
    )
    .await;
    let (status, platform) = put(&app, "/api/function-policies/platform", &admin, body).await;
    assert_eq!(status, StatusCode::OK, "{platform}");
    assert_eq!(platform["owner"], "platform");

    // List: stored rows only, the platform first, then client ids ascending.
    let (_, list) = get(&app, "/api/function-policies", &admin).await;
    let owners: Vec<&str> = list["policies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["owner"].as_str().unwrap())
        .collect();
    let mut clients = [low.id.as_str(), high.id.as_str()];
    clients.sort();
    assert_eq!(owners, [&["platform"][..], &clients[..]].concat());

    // Audit and event: the platform's entity id is PLATFORM, never null.
    let (count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM aud_logs WHERE entity_id = 'PLATFORM' AND operation = 'PutPolicyCommand'",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
    let (group, data): (String, Value) = sqlx::query_as(
        "SELECT message_group, data FROM msg_events WHERE subject = 'platform.function-policy.PLATFORM'",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(group, "platform:function-policy:PLATFORM");
    assert_eq!(data, json!({"owner": "platform", "signerCount": 1}));

    // Errors.
    assert_error(
        &put(
            &app,
            "/api/function-policies/clt_missing",
            &admin,
            json!({}),
        )
        .await,
        StatusCode::NOT_FOUND,
        "Client_NOT_FOUND",
    );
    let dup = json!({"signers": [
        {"issuer": "i", "subject": "s", "runtimes": ["jvm"]},
        {"issuer": "i", "subject": "s", "runtimes": ["wasm"]},
    ]});
    assert_error(
        &put(&app, "/api/function-policies/platform", &admin, dup).await,
        StatusCode::BAD_REQUEST,
        "SIGNER_DUPLICATE",
    );
    assert_error(
        &put(
            &app,
            "/api/function-policies/platform",
            &admin,
            json!({"ceilings": {"maxConcurrency": 0}}),
        )
        .await,
        StatusCode::BAD_REQUEST,
        "CEILING_INVALID",
    );
    let non_anchor = token(&app, As::client(&[&low.id], &[FUNCTION_POLICY_MANAGE])).await;
    assert_error(
        &get(&app, "/api/function-policies", &non_anchor).await,
        StatusCode::FORBIDDEN,
        "ANCHOR_REQUIRED",
    );
    let no_permission = token(&app, As::anchor(&[FUNCTION_VIEW])).await;
    assert_error(
        &put(
            &app,
            "/api/function-policies/platform",
            &no_permission,
            json!({}),
        )
        .await,
        StatusCode::FORBIDDEN,
        "PERMISSION_REQUIRED",
    );
}

// ── Domains and routes ──────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires Docker"]
async fn domains_and_routes() {
    let app = TestApp::setup().await;
    application(&app, "billing").await;
    let a = client(&app, "acme").await;
    let b = client(&app, "bravo").await;
    let anchor = token(&app, As::anchor(ALL)).await;

    // Claim: 201, no verification key; the same hostname again is taken.
    let (status, claimed) = post(
        &app,
        "/api/function-domains",
        &anchor,
        json!({"hostname": "Acme.COM"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{claimed}");
    assert_eq!(claimed["hostname"], "acme.com");
    assert_eq!(claimed["owner"], "platform");
    assert!(claimed.get("verification").is_none());
    assert!(claimed["id"].as_str().unwrap().starts_with("fnd_"));
    assert!(is_micros_timestamp(&claimed["createdAt"]));
    let taken = post(
        &app,
        "/api/function-domains",
        &anchor,
        json!({"hostname": "acme.com"}),
    )
    .await;
    assert_error(&taken, StatusCode::CONFLICT, "DOMAIN_TAKEN");
    assert_eq!(taken.1["message"], "hostname is already claimed");
    // Covered by an existing claim, whoever asks.
    assert_error(
        &post(
            &app,
            "/api/function-domains",
            &anchor,
            json!({"hostname": "api.acme.com", "clientId": a.id}),
        )
        .await,
        StatusCode::CONFLICT,
        "DOMAIN_TAKEN",
    );
    // Covering an existing claim.
    let (status, _) = post(
        &app,
        "/api/function-domains",
        &anchor,
        json!({"hostname": "api.example.org", "clientId": a.id}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_error(
        &post(
            &app,
            "/api/function-domains",
            &anchor,
            json!({"hostname": "example.org"}),
        )
        .await,
        StatusCode::CONFLICT,
        "DOMAIN_TAKEN",
    );
    assert_error(
        &post(
            &app,
            "/api/function-domains",
            &anchor,
            json!({"hostname": "localhost"}),
        )
        .await,
        StatusCode::BAD_REQUEST,
        "HOSTNAME_INVALID",
    );
    let a_manager = token(
        &app,
        As::client(&[&a.id], &[FUNCTION_VIEW, FUNCTION_DOMAIN_MANAGE]),
    )
    .await;
    assert_error(
        &post(
            &app,
            "/api/function-domains",
            &a_manager,
            json!({"hostname": "bravo.io", "clientId": b.id}),
        )
        .await,
        StatusCode::FORBIDDEN,
        "SCOPE_FORBIDDEN",
    );
    let viewer = token(&app, As::anchor(&[FUNCTION_VIEW])).await;
    assert_error(
        &post(
            &app,
            "/api/function-domains",
            &viewer,
            json!({"hostname": "view.io"}),
        )
        .await,
        StatusCode::FORBIDDEN,
        "PERMISSION_REQUIRED",
    );

    // List: clientId required; an owner out of reach is an empty list.
    assert_error(
        &get(&app, "/api/function-domains", &anchor).await,
        StatusCode::BAD_REQUEST,
        "CLIENT_ID_REQUIRED",
    );
    let (_, platform) = get(&app, "/api/function-domains?clientId=platform", &anchor).await;
    assert_eq!(platform.as_array().unwrap().len(), 1);
    let (_, of_a) = get(
        &app,
        &format!("/api/function-domains?clientId={}", a.id),
        &a_manager,
    )
    .await;
    assert_eq!(of_a[0]["hostname"], "api.example.org");
    assert_eq!(of_a[0]["owner"], a.id);
    let (status, hidden) = get(&app, "/api/function-domains?clientId=platform", &a_manager).await;
    assert_eq!((status, hidden), (StatusCode::OK, json!([])));

    // Get: a deeper hostname resolves to its zone; out of reach is 404.
    let (status, zone) = get(
        &app,
        "/api/function-domains/deep.api.example.org",
        &a_manager,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{zone}");
    assert_eq!(zone["hostname"], "api.example.org");
    let b_viewer = token(
        &app,
        As::client(&[&b.id], &[FUNCTION_VIEW, FUNCTION_DOMAIN_MANAGE]),
    )
    .await;
    assert_error(
        &get(&app, "/api/function-domains/api.example.org", &b_viewer).await,
        StatusCode::NOT_FOUND,
        "FunctionDomain_NOT_FOUND",
    );
    assert_error(
        &delete(&app, "/api/function-domains/api.example.org", &b_viewer).await,
        StatusCode::NOT_FOUND,
        "FunctionDomain_NOT_FOUND",
    );
    assert_error(
        &get(&app, "/api/function-domains/nowhere.io", &anchor).await,
        StatusCode::NOT_FOUND,
        "FunctionDomain_NOT_FOUND",
    );
    assert_error(
        &get(&app, "/api/function-domains/no_host", &anchor).await,
        StatusCode::BAD_REQUEST,
        "HOSTNAME_INVALID",
    );

    // Routes (written at promote in P5; inserted here).
    let a_fn = create_function(&app, &anchor, "billing", "invoices", "create", Some(&a.id)).await;
    let b_fn = create_function(&app, &anchor, "billing", "invoices", "other", Some(&b.id)).await;
    for (id, fid, host, prefix, aliases) in [
        (
            "fnr_1",
            a_fn["id"].as_str().unwrap(),
            "deep.api.example.org",
            "/v1",
            vec!["qa"],
        ),
        (
            "fnr_2",
            a_fn["id"].as_str().unwrap(),
            "deep.api.example.org",
            "/api",
            vec![],
        ),
        (
            "fnr_3",
            b_fn["id"].as_str().unwrap(),
            "deep.api.example.org",
            "/b",
            vec![],
        ),
    ] {
        sqlx::query(
            "INSERT INTO fn_routes (id, function_id, hostname, path_prefix, alias_prefixes) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(id)
        .bind(fid)
        .bind(host)
        .bind(prefix)
        .bind(aliases)
        .execute(&app.pool)
        .await
        .unwrap();
    }
    let (status, by_address) = get(
        &app,
        "/api/function-routes?address=billing.invoices.create",
        &a_manager,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{by_address}");
    assert_eq!(
        by_address,
        json!([
            {"hostname": "deep.api.example.org", "pathPrefix": "/api", "address": "billing.invoices.create", "aliasPrefixes": []},
            {"hostname": "deep.api.example.org", "pathPrefix": "/v1", "address": "billing.invoices.create", "aliasPrefixes": ["qa"]},
        ])
    );
    let (_, by_host_anchor) = get(
        &app,
        "/api/function-routes?hostname=deep.api.example.org",
        &anchor,
    )
    .await;
    assert_eq!(by_host_anchor.as_array().unwrap().len(), 3);
    let (_, by_host_a) = get(
        &app,
        "/api/function-routes?hostname=deep.api.example.org",
        &a_manager,
    )
    .await;
    assert_eq!(
        by_host_a.as_array().unwrap().len(),
        2,
        "b's route is left out: {by_host_a}"
    );
    assert_error(
        &get(
            &app,
            "/api/function-routes?address=billing.invoices.other",
            &a_manager,
        )
        .await,
        StatusCode::NOT_FOUND,
        "Function_NOT_FOUND",
    );
    assert_error(
        &get(&app, "/api/function-routes", &anchor).await,
        StatusCode::BAD_REQUEST,
        "FUNCTION_ROUTE_FILTER_REQUIRED",
    );

    // Release: refused while any route is under the zone, naming the
    // functions in order; then released.
    let in_use = delete(&app, "/api/function-domains/api.example.org", &anchor).await;
    assert_error(&in_use, StatusCode::CONFLICT, "DOMAIN_IN_USE");
    assert_eq!(
        in_use.1["message"],
        "domain is in use by: billing.invoices.create, billing.invoices.other"
    );
    sqlx::query("DELETE FROM fn_routes")
        .execute(&app.pool)
        .await
        .unwrap();
    assert_eq!(
        delete(
            &app,
            "/api/function-domains/deep.api.example.org",
            &a_manager
        )
        .await
        .0,
        StatusCode::NO_CONTENT,
        "any hostname under the zone releases the zone"
    );
    let (_, after) = get(
        &app,
        &format!("/api/function-domains?clientId={}", a.id),
        &a_manager,
    )
    .await;
    assert_eq!(after, json!([]));
    assert_eq!(
        app.event_count_by_type("platform:function:domain:claimed")
            .await,
        2
    );
    assert_eq!(
        app.event_count_by_type("platform:function:domain:released")
            .await,
        1
    );
    let (group,): (String,) = sqlx::query_as(
        "SELECT message_group FROM msg_events WHERE type = 'platform:function:domain:released'",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert!(
        group.starts_with("platform:function-domain:fnd_"),
        "{group}"
    );
}

// ── The OpenAPI document and FUNCTION-sourced subscriptions ─────────────────

#[tokio::test]
#[ignore = "requires Docker"]
async fn openapi_document_and_function_subscriptions() {
    let app = TestApp::setup().await;

    // Java's document, verbatim and without a token.
    let resp = app.get_unauth("/api/openapi-functions.json").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        &bytes[..],
        fc_platform::function::openapi::FUNCTIONS_OPENAPI
    );

    // The function routes are in the platform's own document too.
    let spec = app.get_unauth("/q/openapi").await;
    let (_, spec) = read_json(spec).await;
    for path in [
        "/api/functions",
        "/api/functions/{address}/secrets/{key}",
        "/api/function-policies/{owner}",
        "/api/function-domains/{hostname}",
        "/api/function-routes",
    ] {
        assert!(
            spec["paths"].get(path).is_some(),
            "{path} missing from /q/openapi"
        );
    }

    // A subscription Java wrote for a function (source FUNCTION) reads back.
    let mut sub = fc_platform::Subscription::new(
        "fn-fnc-abcd1234",
        "Function sub",
        "https://host/functions/a.b.c/x",
    );
    sub.source = fc_platform::subscription::entity::SubscriptionSource::Function;
    app.repos
        .subscription_repo
        .insert(&sub)
        .await
        .expect("insert");
    let (stored,): (String,) = sqlx::query_as("SELECT source FROM msg_subscriptions WHERE id = $1")
        .bind(&sub.id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(stored, "FUNCTION");
    let read = app
        .repos
        .subscription_repo
        .find_by_id(&sub.id)
        .await
        .expect("strict read")
        .expect("row");
    assert_eq!(
        read.source,
        fc_platform::subscription::entity::SubscriptionSource::Function
    );
}
