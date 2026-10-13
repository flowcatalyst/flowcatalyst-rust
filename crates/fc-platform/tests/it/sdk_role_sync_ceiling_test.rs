//! Go F15 ruling 1 (owner 2026-10-13; Go `2ba9763`, rule since F10 `c56a68c`):
//! the SDK role sync (`POST /api/applications/{appCode}/roles/sync`) applies
//! the permission ceiling as the role-edit routes do. A platform permission a
//! synced role gains or loses, every permission of a new role and of a role
//! `removeUnlisted` deletes, must be one the caller holds: 403
//! `PERMISSION_ABOVE_CALLER`, the whole sync refused, nothing written. An
//! empty list keeps the stored permissions and checks nothing; a same-named
//! CODE or DATABASE role is skipped untouched and not checked. Confinement
//! (400) and `ROLE_HAS_ASSIGNMENTS` come first.
//!
//! On an ordinary application confinement refuses a platform permission
//! before the ceiling runs, so the ceiling is reached on the `platform`
//! application (seeded in every deployment), whose SDK roles may hold platform
//! permissions. Requires Docker (or `FC_TEST_PG_BIN`).

use axum::http::StatusCode;
use serde_json::{json, Value};

use crate::support::{self, read_json, TestApp};
use fc_platform::application::entity::Application;
use fc_platform::domain::{Principal, UserScope};
use fc_platform::role::entity::{permissions, AuthRole, RoleSource};
use fc_platform::service_account::entity::{AssignmentSource, RoleAssignment};

async fn setup() -> TestApp {
    support::set_app_key();
    TestApp::setup().await
}

/// A stored anchor user whose token grants exactly `perms`.
async fn stored_caller(app: &TestApp, email: &str, perms: &[&str]) -> String {
    let p = Principal::new_user(email, UserScope::Anchor);
    app.repos.principal_repo.insert(&p).await.unwrap();
    let granted: Vec<String> = perms.iter().map(|s| s.to_string()).collect();
    app.auth_service
        .generate_access_token_with_scope(&p, &granted, None)
        .unwrap()
}

async fn sync(
    app: &TestApp,
    token: &str,
    code: &str,
    remove: bool,
    roles: Value,
) -> (StatusCode, Value) {
    let q = if remove { "?removeUnlisted=true" } else { "" };
    read_json(
        app.post(
            &format!("/api/applications/{code}/roles/sync{q}"),
            token,
            json!({ "roles": roles }),
        )
        .await,
    )
    .await
}

/// The stored permissions of `name`, sorted; `None` when there is no role.
async fn permissions_of(app: &TestApp, name: &str) -> Option<Vec<String>> {
    app.repos
        .role_repo
        .find_by_name(name)
        .await
        .unwrap()
        .map(|r| {
            let mut p: Vec<String> = r.permissions.into_iter().collect();
            p.sort();
            p
        })
}

fn refused(label: &str, (status, body): (StatusCode, Value), named: &str) {
    assert_eq!(status, StatusCode::FORBIDDEN, "{label}: {body}");
    assert_eq!(body["error"], "PERMISSION_ABOVE_CALLER", "{label}: {body}");
    assert_eq!(
        body["message"],
        format!("You may only add or remove permissions you hold yourself: {named}"),
        "{label}"
    );
}

fn ok(label: &str, (status, body): (StatusCode, Value)) -> Value {
    assert_eq!(status, StatusCode::OK, "{label}: {body}");
    body
}

fn v(perms: &[&str]) -> Option<Vec<String>> {
    let mut p: Vec<String> = perms.iter().map(|s| s.to_string()).collect();
    p.sort();
    Some(p)
}

const SDK: [&str; 3] = [
    permissions::application_service::ROLE_CREATE,
    permissions::application_service::ROLE_UPDATE,
    permissions::application_service::ROLE_DELETE,
];
const VIEW: &str = "platform:iam:user:view";
const DELETE: &str = "platform:iam:user:delete";
const VIEWER: &str = "platform:ceilplat-viewer";

/// Go `TestSyncRoles_PlatformApplicationAdditionsBoundedByCaller`, with the
/// empty list, the deletion and the CODE row of the F15 rule.
#[tokio::test]
#[ignore = "requires Docker"]
async fn the_sdk_role_sync_is_bounded_by_the_callers_permissions() {
    let app = setup().await;
    let sdk_only = stored_caller(&app, "ceil-sdk@iam.test", &SDK).await;
    let mut held_perms = vec![VIEW];
    held_perms.extend(SDK);
    let held = stored_caller(&app, "ceil-held@iam.test", &held_perms).await;
    let mut holder_perms = vec![DELETE];
    holder_perms.extend(&held_perms);
    let holder = stored_caller(&app, "ceil-holder@iam.test", &holder_perms).await;

    // A held platform permission: synced.
    ok(
        "held",
        sync(
            &app,
            &held,
            "platform",
            false,
            json!([{ "name": "ceilplat-viewer", "permissions": [VIEW] }]),
        )
        .await,
    );
    assert_eq!(permissions_of(&app, VIEWER).await, v(&[VIEW]));

    // One the caller lacks, on a new role: the whole sync refused, the held
    // role before it not created either.
    refused(
        "new role",
        sync(
            &app,
            &held,
            "platform",
            false,
            json!([
                { "name": "ceilplat-other", "permissions": [VIEW] },
                { "name": "ceilplat-escalated", "permissions": ["platform:*:*:*"] }
            ]),
        )
        .await,
        "platform:*:*:*",
    );
    assert_eq!(permissions_of(&app, "platform:ceilplat-other").await, None);
    assert_eq!(
        permissions_of(&app, "platform:ceilplat-escalated").await,
        None
    );

    // Added to an existing role: refused, unchanged.
    refused(
        "existing role",
        sync(
            &app,
            &held,
            "platform",
            false,
            json!([{ "name": "ceilplat-viewer", "permissions": [VIEW, DELETE] }]),
        )
        .await,
        DELETE,
    );
    assert_eq!(permissions_of(&app, VIEWER).await, v(&[VIEW]));

    // Only the change: a caller lacking the kept permission re-syncs the same
    // list, and an empty or omitted list keeps the stored one unchecked.
    ok(
        "kept",
        sync(
            &app,
            &sdk_only,
            "platform",
            false,
            json!([{ "name": "ceilplat-viewer", "permissions": [VIEW] }]),
        )
        .await,
    );
    ok(
        "empty",
        sync(
            &app,
            &sdk_only,
            "platform",
            false,
            json!([{ "name": "ceilplat-viewer", "permissions": [] }]),
        )
        .await,
    );
    ok(
        "omitted",
        sync(
            &app,
            &sdk_only,
            "platform",
            false,
            json!([{ "name": "ceilplat-viewer" }]),
        )
        .await,
    );
    assert_eq!(permissions_of(&app, VIEWER).await, v(&[VIEW]));

    // Removal counts.
    refused(
        "removal",
        sync(
            &app,
            &sdk_only,
            "platform",
            false,
            json!([{ "name": "ceilplat-viewer", "permissions": [permissions::application_service::ROLE_CREATE] }]),
        )
        .await,
        VIEW,
    );
    assert_eq!(permissions_of(&app, VIEWER).await, v(&[VIEW]));

    // A role removeUnlisted deletes loses every permission: refused, kept.
    refused(
        "deletion",
        sync(
            &app,
            &sdk_only,
            "platform",
            true,
            json!([{ "name": "ceilplat-plain" }]),
        )
        .await,
        VIEW,
    );
    assert_eq!(permissions_of(&app, VIEWER).await, v(&[VIEW]));
    assert_eq!(permissions_of(&app, "platform:ceilplat-plain").await, None);

    // A same-named CODE role is skipped untouched, so not checked, and never
    // deleted by removeUnlisted.
    let mut code_role = AuthRole::new("platform", "ceilplat-code", "Code").with_permission(DELETE);
    code_role.source = RoleSource::Code;
    app.repos.role_repo.insert(&code_role).await.unwrap();
    ok(
        "code role",
        sync(
            &app,
            &sdk_only,
            "platform",
            false,
            json!([{ "name": "ceilplat-code", "permissions": ["platform:*:*:*"] }]),
        )
        .await,
    );
    assert_eq!(
        permissions_of(&app, "platform:ceilplat-code").await,
        v(&[DELETE])
    );

    // A holder of the permission adds it.
    ok(
        "holder",
        sync(
            &app,
            &holder,
            "platform",
            false,
            json!([{ "name": "ceilplat-viewer", "permissions": [VIEW, DELETE] }]),
        )
        .await,
    );
    assert_eq!(permissions_of(&app, VIEWER).await, v(&[DELETE, VIEW]));

    // Every role's change is collected and checked once, each permission
    // named once in the order collected: per role in payload order its added
    // then its removed permissions, then the deleted roles'. The super-admin
    // sets up a second role first.
    let super_admin = stored_caller(&app, "ceil-super@iam.test", &[permissions::ADMIN_ALL]).await;
    ok(
        "setup",
        sync(
            &app,
            &super_admin,
            "platform",
            false,
            json!([{ "name": "ceilplat-doomed", "permissions": ["platform:iam:role:delete"] }]),
        )
        .await,
    );
    refused(
        "collected",
        sync(
            &app,
            &held,
            "platform",
            true,
            json!([
                { "name": "ceilplat-viewer", "permissions": [VIEW, "platform:iam:client:view"] },
                { "name": "ceilplat-fresh", "permissions": ["platform:iam:client:view", "platform:iam:client:delete", VIEW] },
                { "name": "ceilplat-code" }
            ]),
        )
        .await,
        "platform:iam:client:view, platform:iam:user:delete, platform:iam:client:delete, platform:iam:role:delete",
    );
    assert_eq!(permissions_of(&app, VIEWER).await, v(&[DELETE, VIEW]));
    assert_eq!(
        permissions_of(&app, "platform:ceilplat-doomed").await,
        v(&["platform:iam:role:delete"])
    );
    assert_eq!(permissions_of(&app, "platform:ceilplat-fresh").await, None);

    // A super-admin may do all of it.
    let body = ok(
        "super-admin",
        sync(
            &app,
            &super_admin,
            "platform",
            true,
            json!([
                { "name": "ceilplat-viewer", "permissions": [VIEW, "platform:iam:client:view"] },
                { "name": "ceilplat-fresh", "permissions": ["platform:iam:client:delete"] },
                { "name": "ceilplat-code" }
            ]),
        )
        .await,
    );
    assert_eq!(body["created"], 1, "{body}");
    assert_eq!(permissions_of(&app, "platform:ceilplat-doomed").await, None);
}

/// The order of the refusals: confinement (400, per role) before the
/// ceiling, `ROLE_HAS_ASSIGNMENTS` before the ceiling. An application's own
/// permissions are never checked.
#[tokio::test]
#[ignore = "requires Docker"]
async fn the_ceiling_runs_after_confinement_and_assignments() {
    let app = setup().await;
    let ord = Application::new("ceilord", "ceilord");
    app.repos.application_repo.insert(&ord).await.unwrap();
    let sdk_only = stored_caller(&app, "ceilord-sdk@iam.test", &SDK).await;

    // Application permissions pass for a caller holding none of them.
    ok(
        "own",
        sync(
            &app,
            &sdk_only,
            "ceilord",
            false,
            json!([{ "name": "clerk", "permissions": ["ceilord:order:view"] }]),
        )
        .await,
    );

    // A platform permission on an ordinary application: confinement's 400.
    let (status, body) = sync(
        &app,
        &sdk_only,
        "ceilord",
        false,
        json!([{ "name": "clerk", "permissions": ["ceilord:order:view", DELETE] }]),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "PERMISSION_OUTSIDE_APPLICATION", "{body}");

    // A held role that removeUnlisted would delete, above the caller:
    // ROLE_HAS_ASSIGNMENTS, not the ceiling.
    let super_admin =
        stored_caller(&app, "ceilord-super@iam.test", &[permissions::ADMIN_ALL]).await;
    ok(
        "setup",
        sync(
            &app,
            &super_admin,
            "platform",
            false,
            json!([{ "name": "ceilord-held", "permissions": [DELETE] }]),
        )
        .await,
    );
    let mut p = Principal::new_user("ceilord-holder@iam.test", UserScope::Anchor);
    p.roles.push(RoleAssignment::with_source(
        "platform:ceilord-held",
        AssignmentSource::SdkSync,
    ));
    app.repos.principal_repo.insert(&p).await.unwrap();
    let mut held = vec![VIEW];
    held.extend(SDK);
    let caller = stored_caller(&app, "ceilord-held@iam.test", &held).await;
    let (status, body) = sync(
        &app,
        &caller,
        "platform",
        true,
        json!([{ "name": "ceilord-other" }]),
    )
    .await;
    assert_eq!(body["error"], "ROLE_HAS_ASSIGNMENTS", "{status}: {body}");
}
