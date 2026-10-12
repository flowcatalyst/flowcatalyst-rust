//! Confinement (owner ruling 15, Go F12) on the platform role sync
//! (`RoleSyncService`, the boot sync and `POST /bff/roles/sync-platform`):
//! the built-in catalogue (every role a `platform:` role holding `platform:`
//! permissions) syncs as before, but a catalogue role cannot gain another
//! application's permission, not even for a super-admin (the catalogue sync
//! never crosses applications), and a refused sync writes nothing. Only what
//! the sync adds is checked (Go's and Rust's change-only rule), so a CODE row
//! already holding a foreign permission re-syncs and can lose it. Requires
//! Docker.

use crate::support;

use axum::http::StatusCode;
use fc_platform::role::entity::{roles, AuthRole, RoleSource};
use fc_platform::service::RoleSyncService;
use fc_platform::shared::error::PlatformError;
use serde_json::json;
use support::{assert_status, TestApp};

fn code_role(name: &str, permissions: &[&str]) -> AuthRole {
    let mut role = AuthRole::new("platform", name, "Confinement catalogue");
    role.source = RoleSource::Code;
    role.permissions = permissions.iter().map(|p| p.to_string()).collect();
    role
}

fn with(extra: &[AuthRole]) -> Vec<AuthRole> {
    let mut catalogue = roles::all();
    catalogue.extend(extra.iter().cloned());
    catalogue
}

async fn permissions_of(app: &TestApp, name: &str) -> Option<Vec<String>> {
    let role = app.repos.role_repo.find_by_name(name).await.unwrap()?;
    let mut permissions: Vec<String> = role.permissions.into_iter().collect();
    permissions.sort();
    Some(permissions)
}

fn assert_outside(err: PlatformError, permission: &str) {
    let PlatformError::Coded {
        status,
        code,
        message,
        ..
    } = &err
    else {
        panic!("expected PERMISSION_OUTSIDE_APPLICATION, got {err:?}");
    };
    assert_eq!(*status, StatusCode::BAD_REQUEST, "{message}");
    assert_eq!(code, "PERMISSION_OUTSIDE_APPLICATION");
    assert_eq!(
        *message,
        format!(
            "Permission '{permission}' does not belong to application 'platform'; \
             a role may only hold its own application's permissions"
        )
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn the_platform_role_sync_confines_permissions_to_their_application() {
    let app = TestApp::setup().await;
    let sync = RoleSyncService::new(app.repos.role_repo.clone());

    sync.sync_catalogue(&with(&[]))
        .await
        .expect("the built-in catalogue is confined");
    sync.sync_catalogue(&with(&[code_role(
        "confcat",
        &["platform:confcat:thing:view"],
    )]))
    .await
    .expect("an own-application permission");

    // An existing catalogue role gaining another application's permission:
    // the whole sync is refused, a new role planned before it included.
    let err = sync
        .sync_catalogue(&with(&[
            code_role("confcat-new", &["platform:confcat:thing:view"]),
            code_role(
                "confcat",
                &["platform:confcat:thing:view", "other:thing:view"],
            ),
        ]))
        .await
        .expect_err("another application's permission");
    assert_outside(err, "other:thing:view");
    assert_eq!(
        permissions_of(&app, "platform:confcat").await,
        Some(vec!["platform:confcat:thing:view".to_string()]),
        "a refused sync writes nothing"
    );
    assert_eq!(permissions_of(&app, "platform:confcat-new").await, None);

    // The stale sweep removes it; a new catalogue role holding another
    // application's permission is refused and not created.
    sync.sync_catalogue(&with(&[])).await.unwrap();
    assert_eq!(permissions_of(&app, "platform:confcat").await, None);
    let err = sync
        .sync_catalogue(&with(&[code_role("confcat", &["other:thing:view"])]))
        .await
        .expect_err("a new role with another application's permission");
    assert_outside(err, "other:thing:view");
    assert_eq!(permissions_of(&app, "platform:confcat").await, None);

    // Change-only: a CODE row that already holds a foreign permission
    // re-syncs with the same list, then loses it.
    let mut legacy = code_role(
        "legacy",
        &["platform:legacy:thing:view", "other:thing:view"],
    );
    app.repos.role_repo.insert(&legacy).await.unwrap();
    sync.sync_catalogue(&with(&[legacy.clone()]))
        .await
        .expect("a held foreign permission is not an addition");
    legacy.permissions.remove("other:thing:view");
    sync.sync_catalogue(&with(&[legacy])).await.unwrap();
    assert_eq!(
        permissions_of(&app, "platform:legacy").await,
        Some(vec!["platform:legacy:thing:view".to_string()]),
        "a removal is never refused"
    );

    // The BFF route still syncs the built-in catalogue.
    let token = app.anchor_admin_token().await;
    let body = assert_status(
        app.post("/bff/roles/sync-platform", &token, json!({}))
            .await,
        StatusCode::OK,
    )
    .await;
    assert!(body["total"].as_u64().unwrap() > 0, "{body}");
}
