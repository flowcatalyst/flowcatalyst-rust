//! Go F15 ruling 2 (owner 2026-10-13; Go `3aeb08c`) on
//! `POST /api/applications/{appCode}/principals/sync`: role names are
//! lower-cased and matched exactly (no `app:` prefix added). A name that is
//! not a role, whatever its prefix, is not assigned and not refused, and is
//! reported in `skippedRoles` (lower-cased, each once across the request, in
//! payload order, omitted when empty); the rest of the sync applies. A role of
//! another application still refuses the whole sync, 403 `ROLE_APP_FORBIDDEN`.
//! The platform route `POST /api/principals/sync` is unchanged. Requires
//! Docker (or `FC_TEST_PG_BIN`).

use axum::http::StatusCode;
use serde_json::{json, Value};

use crate::support::{self, read_json, TestApp};
use fc_platform::application::entity::Application;
use fc_platform::domain::{Principal, UserScope};
use fc_platform::role::entity::{permissions, roles, AuthRole};
use fc_platform::service_account::entity::{AssignmentSource, RoleAssignment};

const APP: &str = "unkrolesdk";
const OTHER: &str = "unkroleother";

async fn setup() -> TestApp {
    support::set_app_key();
    let app = TestApp::setup().await;
    for code in [APP, OTHER] {
        app.repos
            .application_repo
            .insert(&Application::new(code, code))
            .await
            .unwrap();
    }
    for role in [
        AuthRole::new(APP, "clerk", "Clerk").with_permission("unkrolesdk:order:view"),
        AuthRole::new(OTHER, "clerk", "Clerk").with_permission("unkroleother:order:view"),
        roles::viewer(),
    ] {
        app.repos.role_repo.insert(&role).await.unwrap();
    }
    app
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

async fn roles_of(app: &TestApp, email: &str) -> Option<Vec<String>> {
    app.repos
        .principal_repo
        .find_by_email(email)
        .await
        .unwrap()
        .map(|p| {
            let mut r: Vec<String> = p.roles.iter().map(|a| a.role.clone()).collect();
            r.sort();
            r
        })
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn the_application_sync_skips_names_that_are_not_roles() {
    let app = setup().await;
    let syncer = stored_caller(
        &app,
        "unk-syncer@iam.test",
        &[permissions::iam::USER_CREATE],
    )
    .await;
    let sync = |principals: Value| {
        let (app, syncer) = (&app, syncer.clone());
        async move {
            read_json(
                app.post(
                    &format!("/api/applications/{APP}/principals/sync"),
                    &syncer,
                    json!({ "principals": principals }),
                )
                .await,
            )
            .await
        }
    };
    let clerk = "unkrolesdk:clerk";
    let pending = "unkrolesdk:not-yet-synced";

    // Unknown names, own-prefixed, unprefixed, an unknown application's
    // prefix, an existing application's and the platform's, are skipped; the
    // known role is assigned; the sync is not refused. Reported lower-cased,
    // each once, in payload order.
    let (status, body) = sync(json!([
        {
            "email": "A@unkrolesdk.test", "name": "A",
            "roles": [clerk, pending, "Unprefixed", "ghostapp:admin", "Platform:Nope", "unkroleother:ghost"]
        },
        {
            "email": "b@unkrolesdk.test", "name": "B",
            "roles": ["UNKROLESDK:Not-Yet-Synced", "unprefixed"]
        }
    ]))
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["created"], 2, "{body}");
    assert_eq!(
        body["syncedCodes"],
        json!(["a@unkrolesdk.test", "b@unkrolesdk.test"])
    );
    assert_eq!(
        body["skippedRoles"],
        json!([
            pending,
            "unprefixed",
            "ghostapp:admin",
            "platform:nope",
            "unkroleother:ghost"
        ]),
        "{body}"
    );
    assert_eq!(roles_of(&app, "a@unkrolesdk.test").await, Some(s(&[clerk])));
    // Every name skipped: the user is still synced, with no role.
    assert_eq!(roles_of(&app, "b@unkrolesdk.test").await, Some(vec![]));

    // Once the role exists, the same sync assigns it and reports nothing.
    let super_admin = stored_caller(&app, "unk-super@iam.test", &[permissions::ADMIN_ALL]).await;
    let (status, body) = read_json(
        app.post(
            &format!("/api/applications/{APP}/roles/sync"),
            &super_admin,
            json!({ "roles": [{ "name": "not-yet-synced" }] }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = sync(json!([
        { "email": "a@unkrolesdk.test", "name": "A", "roles": [clerk, pending] }
    ]))
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body.get("skippedRoles").is_none(),
        "omitted when none: {body}"
    );
    assert_eq!(
        roles_of(&app, "a@unkrolesdk.test").await,
        Some(s(&[clerk, pending]))
    );

    // An existing user whose every name is skipped loses this application's
    // SDK_SYNC set and keeps everything else.
    let mut kept = Principal::new_user("kept@unkrolesdk.test", UserScope::Client);
    kept.roles.push(RoleAssignment::with_source(
        clerk,
        AssignmentSource::SdkSync,
    ));
    kept.roles.push(RoleAssignment::with_source(
        "unkroleother:clerk",
        AssignmentSource::SdkSync,
    ));
    kept.assign_role("platform:viewer");
    app.repos.principal_repo.insert(&kept).await.unwrap();
    let (status, body) = sync(json!([
        { "email": "kept@unkrolesdk.test", "name": "K", "roles": ["unkrolesdk:ghost"] }
    ]))
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["updated"], 1, "{body}");
    assert_eq!(body["skippedRoles"], json!(["unkrolesdk:ghost"]));
    assert_eq!(
        roles_of(&app, "kept@unkrolesdk.test").await,
        Some(s(&["platform:viewer", "unkroleother:clerk"]))
    );

    // A role of another application, or a platform role, is still refused,
    // the whole sync with it, even beside an unknown name; nothing written.
    for foreign in ["unkroleother:clerk", "Platform:Viewer"] {
        let (status, body) = sync(json!([
            { "email": "c@unkrolesdk.test", "name": "C", "roles": [clerk] },
            { "email": "d@unkrolesdk.test", "name": "D", "roles": ["ghostapp:admin", foreign] }
        ]))
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{foreign}: {body}");
        assert_eq!(body["error"], "ROLE_APP_FORBIDDEN", "{foreign}: {body}");
        assert_eq!(
            body["message"],
            format!(
                "role '{}' does not belong to application '{APP}'",
                foreign.to_lowercase()
            )
        );
        assert_eq!(roles_of(&app, "c@unkrolesdk.test").await, None);
        assert_eq!(roles_of(&app, "d@unkrolesdk.test").await, None);
    }
}

/// The platform route is unchanged: an anchor's unknown name is stored as an
/// assignment, and the response has no `skippedRoles`.
#[tokio::test]
#[ignore = "requires Docker"]
async fn the_platform_sync_is_unchanged() {
    let app = setup().await;
    let token = app.anchor_admin_token().await;
    let (status, body) = read_json(
        app.post(
            "/api/principals/sync",
            &token,
            json!({"principals": [{"email": "e@unkrolesdk.test", "name": "E", "roles": ["unkrolesdk:ghost"]}]}),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.get("skippedRoles").is_none(), "{body}");
    assert_eq!(
        roles_of(&app, "e@unkrolesdk.test").await,
        Some(s(&["unkrolesdk:ghost"]))
    );
}
