//! Go ledger items F29-F33 mirrored 2026-10-19 (docs/go-mirror/2026-10-19-identity-fixes-and-catch-up.md).
//! Requires a Postgres (see `support::db`).

use crate::support;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use fc_platform::application::entity::Application;
use fc_platform::auth::config_entity::IdpRoleMapping;
use fc_platform::client::entity::Client;
use fc_platform::domain::{Principal, UserScope};
use fc_platform::role::entity::AuthRole;
use fc_platform::service_account::entity::AssignmentSource;
use fc_platform::usecase::ExecutionContext;
use fc_platform::OidcSyncService;
use serde_json::{json, Value};
use std::collections::HashSet;
use support::assert_status;
use support::TestApp;
use tower::ServiceExt;

async fn setup() -> TestApp {
    support::set_app_key();
    TestApp::setup().await
}

/// F29: the router-config document carries an ETag and answers 304 to a
/// matching `If-None-Match`; a changed document changes the ETag.
#[tokio::test]
#[ignore = "requires Docker"]
async fn router_config_is_conditional() {
    let app = setup().await;
    let admin = app.anchor_admin_token().await;
    let fetch = |if_none_match: Option<String>| {
        let router = app.router.clone();
        let admin = admin.clone();
        async move {
            let mut req = Request::builder()
                .uri("/api/dispatch/router-config")
                .header("authorization", format!("Bearer {admin}"));
            if let Some(v) = if_none_match {
                req = req.header("if-none-match", v);
            }
            router
                .oneshot(req.body(Body::empty()).unwrap())
                .await
                .unwrap()
        }
    };

    let first = fetch(None).await;
    assert_eq!(first.status(), StatusCode::OK);
    let etag = first
        .headers()
        .get("etag")
        .expect("an ETag")
        .to_str()
        .unwrap()
        .to_string();
    assert!(etag.starts_with('"') && etag.ends_with('"'), "{etag}");

    for same in [
        etag.clone(),
        format!("W/{etag}"),
        format!("\"other\", {etag}"),
        "*".to_string(),
    ] {
        let resp = fetch(Some(same.clone())).await;
        assert_eq!(resp.status(), StatusCode::NOT_MODIFIED, "{same}");
        assert_eq!(resp.headers().get("etag").unwrap().to_str().unwrap(), etag);
    }
    let resp = fetch(Some("\"stale\"".to_string())).await;
    assert_eq!(resp.status(), StatusCode::OK);

    // A new client adds its queues: the validator no longer matches.
    app.repos
        .client_repo
        .insert(&Client::new("Fresh", "fresh"))
        .await
        .unwrap();
    let resp = fetch(Some(etag.clone())).await;
    assert_eq!(resp.status(), StatusCode::OK, "the document changed");
    assert_ne!(resp.headers().get("etag").unwrap().to_str().unwrap(), etag);
}

// ── F30: unchanged definitions are not rewritten or counted ──────────────────

async fn sync_post(app: &TestApp, token: &str, path: &str, body: Value) -> Value {
    assert_status(app.post(path, token, body).await, StatusCode::OK).await
}

async fn event_count(app: &TestApp, like: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM msg_events WHERE type LIKE $1")
        .bind(like)
        .fetch_one(&app.pool)
        .await
        .unwrap()
}

async fn updated_at(
    app: &TestApp,
    table: &str,
    col: &str,
    key: &str,
) -> chrono::DateTime<chrono::Utc> {
    sqlx::query_scalar(&format!("SELECT updated_at FROM {table} WHERE {col} = $1"))
        .bind(key)
        .fetch_one(&app.pool)
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn an_unchanged_sdk_role_sync_writes_and_counts_nothing() {
    let app = setup().await;
    let admin = app.anchor_admin_token_all_applications().await;
    app.repos
        .application_repo
        .insert(&Application::new("f30r", "F30"))
        .await
        .unwrap();
    let path = "/api/applications/f30r/roles/sync";
    let role = |display: &str, perms: Value| {
        json!({ "roles": [{ "name": "viewer", "displayName": display, "description": "d",
                            "permissions": perms }] })
    };
    let perms = json!(["f30r:orders:order:view", "f30r:orders:order:edit"]);

    let first = sync_post(&app, &admin, path, role("Viewer", perms.clone())).await;
    assert_eq!(first["created"], 1, "{first}");
    let stamp = updated_at(&app, "iam_roles", "name", "f30r:viewer").await;
    let events = event_count(&app, "%role:updated").await;

    // The same definition, the permissions reordered, and no permissions at
    // all (keep the stored ones): nothing is written or counted.
    for body in [
        role("Viewer", perms.clone()),
        role(
            "Viewer",
            json!(["f30r:orders:order:edit", "f30r:orders:order:view"]),
        ),
        role("Viewer", json!([])),
    ] {
        let again = sync_post(&app, &admin, path, body).await;
        assert_eq!(again["created"], 0, "{again}");
        assert_eq!(again["updated"], 0, "{again}");
    }
    assert_eq!(
        updated_at(&app, "iam_roles", "name", "f30r:viewer").await,
        stamp
    );
    assert_eq!(
        event_count(&app, "%role:updated").await,
        events,
        "no *Updated event"
    );

    // Each managed field still updates exactly this one row.
    for (what, body) in [
        ("display name", role("Viewer 2", perms.clone())),
        (
            "permissions",
            role("Viewer 2", json!(["f30r:orders:order:view"])),
        ),
    ] {
        let changed = sync_post(&app, &admin, path, body).await;
        assert_eq!(changed["updated"], 1, "{what}: {changed}");
    }
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn an_unchanged_platform_role_sync_counts_nothing() {
    let app = setup().await;
    let admin = app.anchor_admin_token_all_applications().await;
    // The boot state, then a sync: everything matches the catalogue.
    let _ = sync_post(&app, &admin, "/bff/roles/sync-platform", json!({})).await;
    let again = sync_post(&app, &admin, "/bff/roles/sync-platform", json!({})).await;
    assert_eq!(again["updated"], 0, "{again}");
    // One drifted role is the only one rewritten.
    sqlx::query("UPDATE iam_roles SET display_name = 'drifted' WHERE name = 'platform:router'")
        .execute(&app.pool)
        .await
        .unwrap();
    let drifted = sync_post(&app, &admin, "/bff/roles/sync-platform", json!({})).await;
    assert_eq!(drifted["updated"], 1, "{drifted}");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn unchanged_connections_and_subscriptions_are_not_rewritten() {
    support::set_app_key();
    let app = TestApp::setup().await;
    let admin = app.anchor_admin_token_all_applications().await;
    let created = assert_status(
        app.post(
            "/api/service-accounts",
            &admin,
            json!({ "code": "f30-bot", "name": "Bot" }),
        )
        .await,
        StatusCode::CREATED,
    )
    .await;
    let mut application = Application::new("f30c", "F30");
    application.service_account_id = Some(created["principalId"].as_str().unwrap().to_string());
    app.repos
        .application_repo
        .insert(&application)
        .await
        .unwrap();

    // Connections.
    let conns = |name: &str| json!({ "connections": [{ "code": "f30-conn", "name": name }] });
    let path = "/api/applications/f30c/connections/sync";
    assert_eq!(
        sync_post(&app, &admin, path, conns("One")).await["created"],
        1
    );
    let stamp = updated_at(&app, "msg_connections", "code", "f30-conn").await;
    let again = sync_post(&app, &admin, path, conns("One")).await;
    assert_eq!(
        (again["created"].clone(), again["updated"].clone()),
        (json!(0), json!(0)),
        "{again}"
    );
    assert_eq!(
        updated_at(&app, "msg_connections", "code", "f30-conn").await,
        stamp
    );
    assert_eq!(
        sync_post(&app, &admin, path, conns("Two")).await["updated"],
        1
    );

    // Subscriptions.
    let subs = |name: &str, events: Value| {
        json!({ "subscriptions": [{
            "code": "f30-sub", "name": name,
            "target": "https://receiver.example.test/hook",
            "eventTypes": events
        }]})
    };
    let ab = json!([{ "eventTypeCode": "f30c:orders:order:created" }, { "eventTypeCode": "f30c:orders:order:shipped" }]);
    let ba = json!([{ "eventTypeCode": "f30c:orders:order:shipped" }, { "eventTypeCode": "f30c:orders:order:created" }]);
    let path = "/api/applications/f30c/subscriptions/sync";
    assert_eq!(
        sync_post(&app, &admin, path, subs("S", ab.clone())).await["created"],
        1
    );
    let stamp = updated_at(&app, "msg_subscriptions", "code", "f30-sub").await;
    let events = event_count(&app, "%subscription:updated").await;
    for body in [subs("S", ab.clone()), subs("S", ba.clone())] {
        let again = sync_post(&app, &admin, path, body).await;
        assert_eq!(again["updated"], 0, "{again}");
    }
    assert_eq!(
        updated_at(&app, "msg_subscriptions", "code", "f30-sub").await,
        stamp
    );
    assert_eq!(event_count(&app, "%subscription:updated").await, events);
    assert_eq!(
        sync_post(&app, &admin, path, subs("S renamed", ab)).await["updated"],
        1
    );
    assert_eq!(
        sync_post(
            &app,
            &admin,
            path,
            subs(
                "S renamed",
                json!([{ "eventTypeCode": "f30c:orders:order:created" }])
            )
        )
        .await["updated"],
        1,
        "a changed binding set is a change"
    );
}

// ── F31: the scheduled-job sync rollup is keyed by the application code ─────

#[tokio::test]
#[ignore = "requires Docker"]
async fn scheduled_job_sync_rollup_is_audited_under_the_application_code() {
    let app = setup().await;
    let admin = app.anchor_admin_token_all_applications().await;
    app.repos
        .application_repo
        .insert(&Application::new("f31sj", "F31"))
        .await
        .unwrap();
    let body = json!({ "jobs": [{
        "code": "f31-a", "name": "A", "crons": ["*/5 * * * *"], "timezone": "UTC",
        "deliveryMaxAttempts": 3
    }]});
    for name in ["A", "A2"] {
        let mut b = body.clone();
        b["jobs"][0]["name"] = json!(name);
        sync_post(
            &app,
            &admin,
            "/api/applications/f31sj/scheduled-jobs/sync",
            b,
        )
        .await;
    }
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT entity_type, entity_id FROM aud_logs
         WHERE operation_json::text LIKE '%f31sj%' ORDER BY performed_at, id",
    )
    .fetch_all(&app.pool)
    .await
    .unwrap();
    let rollups: Vec<&(String, String)> = rows
        .iter()
        .filter(|(t, _)| t.eq_ignore_ascii_case("scheduledjobs"))
        .collect();
    assert_eq!(rollups.len(), 2, "one rollup per sync: {rows:?}");
    for (_, id) in rollups {
        assert_eq!(
            id, "f31sj",
            "the rollup is keyed by the application code, not 'synced'"
        );
    }
    let subject: String = sqlx::query_scalar(
        "SELECT subject FROM msg_events WHERE type = 'platform:admin:scheduledjobs:synced' LIMIT 1",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(subject, "platform.scheduledjobs.f31sj");
}

// ── F33: IdP role mappings are checked at write; a stale one is skipped at login ──

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_mapping_to_an_unknown_role_is_refused_at_write_when_required() {
    use fc_platform::usecase::UseCase;
    use fc_platform_iam::auth::operations::create_idp_role_mapping::{
        CreateIdpRoleMappingCommand, CreateIdpRoleMappingUseCase,
    };
    let app = setup().await;
    let use_case = |required: bool| {
        CreateIdpRoleMappingUseCase::new(
            app.repos.idp_role_mapping_repo.clone(),
            app.repos.role_repo.clone(),
            app.unit_of_work.clone(),
        )
        .with_required_role(required)
    };
    let cmd = |idp_role: &str, platform_role: &str| CreateIdpRoleMappingCommand {
        idp_type: "OIDC".to_string(),
        idp_role_name: idp_role.to_string(),
        platform_role_name: platform_role.to_string(),
    };
    // Required: an unknown role is a 400 ROLE_NOT_FOUND and nothing is stored.
    let err = use_case(true)
        .run(cmd("staff", "f33:ghost"), anchor_caller())
        .await
        .into_result()
        .expect_err("refused");
    assert_eq!(err.code(), "ROLE_NOT_FOUND");
    assert_eq!(
        app.repos
            .idp_role_mapping_repo
            .find_all()
            .await
            .unwrap()
            .len(),
        0
    );
    // A role that exists passes either way.
    app.repos
        .role_repo
        .insert(&AuthRole::new("f33", "clerk", "Clerk"))
        .await
        .unwrap();
    use_case(true)
        .run(cmd("staff", "f33:clerk"), anchor_caller())
        .await
        .into_result()
        .map_err(|e| e.code().to_string())
        .expect("a known role");
    // The default (off) keeps today's behaviour: the unknown role is accepted.
    use_case(false)
        .run(cmd("legacy", "f33:ghost"), anchor_caller())
        .await
        .into_result()
        .map_err(|e| e.code().to_string())
        .expect("accepted while the switch is off");
}

fn anchor_caller() -> ExecutionContext {
    use fc_platform::shared::authorization_service::{AuthContext, Credential};
    use fc_platform::shared::id::PrincipalId;
    ExecutionContext::from_auth(&AuthContext {
        principal_id: PrincipalId::parse("prn_caller000001").unwrap(),
        principal_type: fc_platform::PrincipalType::User,
        scope: UserScope::Anchor,
        email: Some("anchor@f33.test".to_string()),
        name: "Anchor".to_string(),
        accessible_clients: vec![],
        permissions: HashSet::new(),
        roles: vec![],
        credential: Credential::BearerToken,
    })
}

/// One stale mapping (its role deleted since) is skipped with a warning; the
/// valid mapped roles are granted and the user's old IdP roles still go.
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_stale_mapping_is_skipped_not_fatal_at_login() {
    let app = setup().await;
    app.repos
        .role_repo
        .insert(&AuthRole::new("f33", "clerk", "Clerk"))
        .await
        .unwrap();
    for (idp_role, platform_role) in [("staff", "f33:clerk"), ("legacy", "f33:deleted")] {
        app.repos
            .idp_role_mapping_repo
            .insert(&IdpRoleMapping::new("OIDC", idp_role, platform_role))
            .await
            .unwrap();
    }
    let mut user = Principal::new_user("f33@example.test", UserScope::Anchor);
    user.assign_role_with_source("f33:old-idp-role", AssignmentSource::IdpSync);
    app.repos.principal_repo.insert(&user).await.unwrap();

    let sync = OidcSyncService::new(
        app.repos.principal_repo.clone(),
        app.repos.idp_role_mapping_repo.clone(),
    )
    .with_role_repo(app.repos.role_repo.clone());
    let granted = sync
        .sync_idp_roles(&mut user, &["staff".to_string(), "legacy".to_string()])
        .await
        .expect("a stale mapping does not fail the sync");
    assert_eq!(granted.into_iter().collect::<Vec<_>>(), ["f33:clerk"]);
    let stored = app
        .repos
        .principal_repo
        .find_by_id(&user.id)
        .await
        .unwrap()
        .unwrap();
    let names: Vec<&str> = stored.roles.iter().map(|r| r.role.as_str()).collect();
    assert_eq!(
        names,
        ["f33:clerk"],
        "the valid role is granted, the lost one removed"
    );

    // The same sync without the existence check (a service built without a
    // role repository) is today's behaviour: the dangling role is assigned.
    let lenient = OidcSyncService::new(
        app.repos.principal_repo.clone(),
        app.repos.idp_role_mapping_repo.clone(),
    );
    let granted = lenient
        .sync_idp_roles(&mut user, &["legacy".to_string()])
        .await
        .unwrap();
    assert_eq!(granted.into_iter().collect::<Vec<_>>(), ["f33:deleted"]);
}
