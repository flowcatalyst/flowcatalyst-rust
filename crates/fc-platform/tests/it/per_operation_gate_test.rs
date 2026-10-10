//! Every write checks the permission for its operation, never "any of
//! create/update/delete" (Go 3d536a7, 6be78ee, 210e4b5, 1fa33ef, 1f83887,
//! e03e54a, b49b73a, 4c87621, c59e246; docs/platform-bug-fixes.md,
//! "Permission gates"). Each write is sent by an anchor caller holding one
//! permission of its resource's create/update/delete trio: every one but the
//! operation's own is refused with 403 `PERMISSION_REQUIRED`, and the
//! operation's own passes the gate (whatever the use case then answers).
//! Before the fix a delete-only role could create and edit. Requires Docker
//! (or `FC_TEST_PG_BIN`).

use crate::support;

use axum::http::{Method, StatusCode};
use serde_json::{json, Value};

use fc_platform::domain::{Principal, UserScope};
use fc_platform::role::entity::permissions::{admin, iam};
use support::{read_json, TestApp};

/// An anchor-tier token whose `scope` grants exactly `perm`.
fn holding(app: &TestApp, perm: &str) -> String {
    let p = Principal::new_user("gates@per-op.test", UserScope::Anchor);
    app.auth_service
        .generate_access_token_with_scope(&p, &[perm.to_string()], None)
        .expect("token")
}

async fn send(
    app: &TestApp,
    method: &Method,
    path: &str,
    token: &str,
    body: &Value,
) -> (StatusCode, Value) {
    let resp = match *method {
        Method::POST => app.post(path, token, body.clone()).await,
        Method::PUT => app.put(path, token, body.clone()).await,
        Method::DELETE => app.delete(path, token).await,
        Method::GET => app.get(path, token).await,
        _ => unreachable!("{method}"),
    };
    read_json(resp).await
}

fn refused_for_permission(status: StatusCode, body: &Value) -> bool {
    status == StatusCode::FORBIDDEN && body["error"] == "PERMISSION_REQUIRED"
}

/// `method path` with `body`: each permission of `trio` but `required` is
/// refused with `PERMISSION_REQUIRED`; `required` passes the gate.
async fn assert_gate(
    app: &TestApp,
    method: Method,
    path: &str,
    body: Value,
    trio: [&str; 3],
    required: &str,
) {
    for perm in trio {
        let token = holding(app, perm);
        let (status, resp) = send(app, &method, path, &token, &body).await;
        if perm == required {
            assert!(
                !refused_for_permission(status, &resp),
                "{method} {path} with {perm} (its own permission) was refused: {status} {resp}"
            );
        } else {
            assert!(
                refused_for_permission(status, &resp),
                "{method} {path} with only {perm} should be 403 PERMISSION_REQUIRED, got {status} {resp}"
            );
            assert_eq!(
                resp["message"],
                format!("permission required: {required}"),
                "{method} {path} with {perm}"
            );
        }
    }
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn messaging_writes_check_the_permission_for_the_operation() {
    let app = TestApp::setup().await;

    // Subscriptions (bug 8).
    let trio = [
        admin::SUBSCRIPTION_CREATE,
        admin::SUBSCRIPTION_UPDATE,
        admin::SUBSCRIPTION_DELETE,
    ];
    let sub =
        json!({ "code": "gate-sub", "name": "Gate", "endpoint": "https://example.test/hook" });
    assert_gate(
        &app,
        Method::POST,
        "/api/subscriptions",
        sub,
        trio,
        admin::SUBSCRIPTION_CREATE,
    )
    .await;
    assert_gate(
        &app,
        Method::PUT,
        "/api/subscriptions/sub_nope",
        json!({}),
        trio,
        admin::SUBSCRIPTION_UPDATE,
    )
    .await;
    for action in ["pause", "resume"] {
        let path = format!("/api/subscriptions/sub_nope/{action}");
        assert_gate(
            &app,
            Method::POST,
            &path,
            json!({}),
            trio,
            admin::SUBSCRIPTION_UPDATE,
        )
        .await;
    }
    assert_gate(
        &app,
        Method::DELETE,
        "/api/subscriptions/sub_nope",
        json!({}),
        trio,
        admin::SUBSCRIPTION_DELETE,
    )
    .await;

    // Dispatch pools (c59e246): archive is a delete.
    let trio = [
        admin::DISPATCH_POOL_CREATE,
        admin::DISPATCH_POOL_UPDATE,
        admin::DISPATCH_POOL_DELETE,
    ];
    let pool = json!({ "code": "gate-pool", "name": "Gate" });
    assert_gate(
        &app,
        Method::POST,
        "/api/dispatch-pools",
        pool,
        trio,
        admin::DISPATCH_POOL_CREATE,
    )
    .await;
    assert_gate(
        &app,
        Method::PUT,
        "/api/dispatch-pools/dpl_nope",
        json!({}),
        trio,
        admin::DISPATCH_POOL_UPDATE,
    )
    .await;
    for action in ["suspend", "activate"] {
        let path = format!("/api/dispatch-pools/dpl_nope/{action}");
        assert_gate(
            &app,
            Method::POST,
            &path,
            json!({}),
            trio,
            admin::DISPATCH_POOL_UPDATE,
        )
        .await;
    }
    assert_gate(
        &app,
        Method::POST,
        "/api/dispatch-pools/dpl_nope/archive",
        json!({}),
        trio,
        admin::DISPATCH_POOL_DELETE,
    )
    .await;

    // Event types (1fa33ef): adding a schema version is an update.
    let trio = [
        admin::EVENT_TYPE_CREATE,
        admin::EVENT_TYPE_UPDATE,
        admin::EVENT_TYPE_DELETE,
    ];
    let et = json!({ "code": "gate:orders:order:created", "name": "Gate" });
    assert_gate(
        &app,
        Method::POST,
        "/api/event-types",
        et.clone(),
        trio,
        admin::EVENT_TYPE_CREATE,
    )
    .await;
    assert_gate(
        &app,
        Method::PUT,
        "/api/event-types/evt_nope",
        json!({ "name": "x" }),
        trio,
        admin::EVENT_TYPE_UPDATE,
    )
    .await;
    assert_gate(
        &app,
        Method::DELETE,
        "/api/event-types/evt_nope",
        json!({}),
        trio,
        admin::EVENT_TYPE_DELETE,
    )
    .await;
    // Add schema answers Go's `requireAny` ("one of: …"): the update or the
    // application-service grants.
    for perm in [admin::EVENT_TYPE_CREATE, admin::EVENT_TYPE_DELETE] {
        let (status, resp) = send(
            &app,
            &Method::POST,
            "/api/event-types/evt_nope/schemas",
            &holding(&app, perm),
            &json!({ "version": "1.1", "schema": {} }),
        )
        .await;
        assert!(
            refused_for_permission(status, &resp),
            "add schema with {perm}: {status} {resp}"
        );
    }
    let (status, resp) = send(
        &app,
        &Method::POST,
        "/api/event-types/evt_nope/schemas",
        &holding(&app, admin::EVENT_TYPE_UPDATE),
        &json!({ "version": "1.1", "schema": {} }),
    )
    .await;
    assert!(
        !refused_for_permission(status, &resp),
        "add schema with update: {status} {resp}"
    );

    // The BFF event-type writes check the same permissions (Go bff/event_types.go).
    let bff_et =
        json!({ "code": "gate:orders:order:shipped", "name": "Gate", "clientScoped": false });
    assert_gate(
        &app,
        Method::POST,
        "/bff/event-types",
        bff_et,
        trio,
        admin::EVENT_TYPE_CREATE,
    )
    .await;
    assert_gate(
        &app,
        Method::PUT,
        "/bff/event-types/evt_nope",
        json!({ "name": "x" }),
        trio,
        admin::EVENT_TYPE_UPDATE,
    )
    .await;
    assert_gate(
        &app,
        Method::DELETE,
        "/bff/event-types/evt_nope",
        json!({}),
        trio,
        admin::EVENT_TYPE_DELETE,
    )
    .await;
    for path in [
        "/bff/event-types/evt_nope/archive",
        "/bff/event-types/evt_nope/schemas/1.0/finalise",
        "/bff/event-types/evt_nope/schemas/1.0/deprecate",
    ] {
        assert_gate(
            &app,
            Method::POST,
            path,
            json!({}),
            trio,
            admin::EVENT_TYPE_UPDATE,
        )
        .await;
    }
    let schema = json!({ "version": "1.1", "schema": {}, "mimeType": "application/schema+json" });
    assert_gate(
        &app,
        Method::POST,
        "/bff/event-types/evt_nope/schemas",
        schema,
        trio,
        admin::EVENT_TYPE_UPDATE,
    )
    .await;

    // Processes (1f83887): archive is an update.
    let trio = [
        admin::PROCESS_CREATE,
        admin::PROCESS_UPDATE,
        admin::PROCESS_DELETE,
    ];
    assert_gate(
        &app,
        Method::POST,
        "/api/processes/prc_nope/archive",
        json!({}),
        trio,
        admin::PROCESS_UPDATE,
    )
    .await;
    assert_gate(
        &app,
        Method::POST,
        "/api/processes",
        json!({ "code": "gate:p", "name": "Gate" }),
        trio,
        admin::PROCESS_CREATE,
    )
    .await;
    assert_gate(
        &app,
        Method::PUT,
        "/api/processes/prc_nope",
        json!({}),
        trio,
        admin::PROCESS_UPDATE,
    )
    .await;
    assert_gate(
        &app,
        Method::DELETE,
        "/api/processes/prc_nope",
        json!({}),
        trio,
        admin::PROCESS_DELETE,
    )
    .await;

    // Scheduled jobs (bug 17): pause, resume and archive are updates.
    let trio = [
        admin::SCHEDULED_JOB_CREATE,
        admin::SCHEDULED_JOB_UPDATE,
        admin::SCHEDULED_JOB_DELETE,
    ];
    let job = json!({ "code": "gate-job", "name": "Gate", "crons": ["0 * * * *"] });
    assert_gate(
        &app,
        Method::POST,
        "/api/scheduled-jobs",
        job,
        trio,
        admin::SCHEDULED_JOB_CREATE,
    )
    .await;
    assert_gate(
        &app,
        Method::PUT,
        "/api/scheduled-jobs/sjb_nope",
        json!({}),
        trio,
        admin::SCHEDULED_JOB_UPDATE,
    )
    .await;
    for action in ["pause", "resume", "archive"] {
        let path = format!("/api/scheduled-jobs/sjb_nope/{action}");
        assert_gate(
            &app,
            Method::POST,
            &path,
            json!({}),
            trio,
            admin::SCHEDULED_JOB_UPDATE,
        )
        .await;
    }
    assert_gate(
        &app,
        Method::DELETE,
        "/api/scheduled-jobs/sjb_nope",
        json!({}),
        trio,
        admin::SCHEDULED_JOB_DELETE,
    )
    .await;
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn iam_writes_check_the_permission_for_the_operation() {
    support::set_app_key();
    let app = TestApp::setup().await;

    // Applications (bug 13).
    let trio = [
        admin::APPLICATION_CREATE,
        admin::APPLICATION_UPDATE,
        admin::APPLICATION_DELETE,
    ];
    assert_gate(
        &app,
        Method::POST,
        "/api/applications",
        json!({ "code": "gateapp", "name": "Gate" }),
        trio,
        admin::APPLICATION_CREATE,
    )
    .await;
    assert_gate(
        &app,
        Method::PUT,
        "/api/applications/app_nope",
        json!({}),
        trio,
        admin::APPLICATION_UPDATE,
    )
    .await;
    for action in ["activate", "deactivate"] {
        let path = format!("/api/applications/app_nope/{action}");
        assert_gate(
            &app,
            Method::POST,
            &path,
            json!({}),
            trio,
            admin::APPLICATION_UPDATE,
        )
        .await;
    }
    assert_gate(
        &app,
        Method::DELETE,
        "/api/applications/app_nope",
        json!({}),
        trio,
        admin::APPLICATION_DELETE,
    )
    .await;

    // Roles (e03e54a): granting and revoking a permission are updates.
    let trio = [iam::ROLE_CREATE, iam::ROLE_UPDATE, iam::ROLE_DELETE];
    let role = json!({ "applicationCode": "gateapp", "roleName": "gate", "displayName": "Gate" });
    assert_gate(
        &app,
        Method::POST,
        "/api/roles",
        role,
        trio,
        iam::ROLE_CREATE,
    )
    .await;
    assert_gate(
        &app,
        Method::PUT,
        "/api/roles/gateapp:nope",
        json!({}),
        trio,
        iam::ROLE_UPDATE,
    )
    .await;
    let grant = json!({ "permission": "gateapp:orders:order:view" });
    assert_gate(
        &app,
        Method::POST,
        "/api/roles/gateapp:nope/permissions",
        grant,
        trio,
        iam::ROLE_UPDATE,
    )
    .await;
    for method in [Method::POST, Method::DELETE] {
        let path = "/api/roles/gateapp:nope/permissions/gateapp:orders:order:view";
        assert_gate(&app, method, path, json!({}), trio, iam::ROLE_UPDATE).await;
    }
    assert_gate(
        &app,
        Method::DELETE,
        "/api/roles/gateapp:nope",
        json!({}),
        trio,
        iam::ROLE_DELETE,
    )
    .await;

    // Service accounts (b49b73a): deactivate is an update.
    let trio = [
        admin::SERVICE_ACCOUNT_CREATE,
        admin::SERVICE_ACCOUNT_UPDATE,
        admin::SERVICE_ACCOUNT_DELETE,
    ];
    assert_gate(
        &app,
        Method::POST,
        "/api/service-accounts",
        json!({ "code": "gate-sa", "name": "Gate" }),
        trio,
        admin::SERVICE_ACCOUNT_CREATE,
    )
    .await;
    assert_gate(
        &app,
        Method::PUT,
        "/api/service-accounts/sac_nope",
        json!({}),
        trio,
        admin::SERVICE_ACCOUNT_UPDATE,
    )
    .await;
    assert_gate(
        &app,
        Method::POST,
        "/api/service-accounts/sac_nope/deactivate",
        json!({}),
        trio,
        admin::SERVICE_ACCOUNT_UPDATE,
    )
    .await;
    assert_gate(
        &app,
        Method::DELETE,
        "/api/service-accounts/sac_nope",
        json!({}),
        trio,
        admin::SERVICE_ACCOUNT_DELETE,
    )
    .await;

    // Principals (4c87621): creating users needs create; every change to an
    // existing user, and the reset-approval queue, update.
    let trio = [iam::USER_CREATE, iam::USER_UPDATE, iam::USER_DELETE];
    let principal = json!({ "email": "made@gate.test", "scope": "ANCHOR", "name": "Made" });
    assert_gate(
        &app,
        Method::POST,
        "/api/principals",
        principal,
        trio,
        iam::USER_CREATE,
    )
    .await;
    let user = json!({ "email": "sdk@gate.test", "name": "Sdk" });
    assert_gate(
        &app,
        Method::POST,
        "/api/principals/users",
        user,
        trio,
        iam::USER_CREATE,
    )
    .await;
    let import = json!({ "clientId": "clt_0GATEIMPORT01", "users": [] });
    assert_gate(
        &app,
        Method::POST,
        "/api/principals/bulk-import",
        import,
        trio,
        iam::USER_CREATE,
    )
    .await;
    assert_gate(
        &app,
        Method::PUT,
        "/api/principals/prn_nope",
        json!({}),
        trio,
        iam::USER_UPDATE,
    )
    .await;
    for action in [
        "activate",
        "deactivate",
        "send-password-reset",
        "reset-2fa",
        "developer-credential",
    ] {
        let path = format!("/api/principals/prn_nope/{action}");
        assert_gate(&app, Method::POST, &path, json!({}), trio, iam::USER_UPDATE).await;
    }
    assert_gate(
        &app,
        Method::DELETE,
        "/api/principals/prn_nope/developer-credential",
        json!({}),
        trio,
        iam::USER_UPDATE,
    )
    .await;
    let reset = json!({ "newPassword": "Gate-Password-123!" });
    assert_gate(
        &app,
        Method::POST,
        "/api/principals/prn_nope/reset-password",
        reset,
        trio,
        iam::USER_UPDATE,
    )
    .await;
    let access = json!({ "applicationIds": [] });
    assert_gate(
        &app,
        Method::PUT,
        "/api/principals/prn_nope/application-access",
        access,
        trio,
        iam::USER_UPDATE,
    )
    .await;
    assert_gate(
        &app,
        Method::GET,
        "/api/reset-approvals",
        json!({}),
        trio,
        iam::USER_UPDATE,
    )
    .await;
    assert_gate(
        &app,
        Method::DELETE,
        "/api/principals/prn_nope",
        json!({}),
        trio,
        iam::USER_DELETE,
    )
    .await;
}
