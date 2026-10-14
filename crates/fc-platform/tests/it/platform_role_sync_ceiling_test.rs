//! The permission ceiling on the platform role sync (Go F10, `c56a68c`,
//! `SyncPlatformRoles`; owner approval 2026-10-14): `POST
//! /bff/roles/sync-platform` and the boot sync (`RoleSyncService`). The
//! catalogue is code, but the sync adds and removes platform permissions
//! (restoring a role trimmed by hand, sweeping a stale one), so a caller may
//! run it only when it holds every platform permission the sync changes: per
//! catalogue `CODE` role the change, a new role's permissions, a swept stale
//! role's. Collected over the whole sync and checked once, after
//! confinement, before anything is written: 403 `PERMISSION_ABOVE_CALLER`,
//! nothing written. A same-named role of another source is skipped untouched,
//! and a stale role still assigned is kept, so neither is checked. The boot
//! sync runs as the system caller, which holds everything. Requires Docker
//! (or `FC_TEST_PG_BIN`).

use crate::support;

use std::collections::HashSet;
use std::fmt::Debug;

use axum::http::StatusCode;
use serde_json::{json, Value};

use fc_platform::domain::{Principal, UserScope};
use fc_platform::role::entity::{permissions::iam, roles, AuthRole, RoleSource};
use fc_platform::service::RoleSyncService;
use fc_platform::service_account::entity::{AssignmentSource, RoleAssignment};
use fc_platform::shared::authorization_service::{AuthContext, Credential};
use fc_platform::shared::error::PlatformError;
use fc_platform::shared::id::PrincipalId;
use fc_platform::usecase::Caller;
use fc_platform::PrincipalType;
use support::{read_json, TestApp};

const ROLE_GATE: [&str; 3] = [iam::ROLE_CREATE, iam::ROLE_UPDATE, iam::ROLE_DELETE];

/// An anchor-tier token whose `scope` grants exactly `perms`.
fn anchor_holding(app: &TestApp, perms: &[&str]) -> String {
    let p = Principal::new_user("role-sync-ceiling@flowcatalyst.test", UserScope::Anchor);
    let granted: Vec<String> = perms.iter().map(|s| s.to_string()).collect();
    app.auth_service
        .generate_access_token_with_scope(&p, &granted, None)
        .expect("token")
}

async fn sync_platform(app: &TestApp, token: &str) -> (StatusCode, Value) {
    read_json(app.post("/bff/roles/sync-platform", token, json!({})).await).await
}

/// The stored permissions of `name`, sorted; `None` when there is no role.
async fn permissions_of(app: &TestApp, name: &str) -> Option<Vec<String>> {
    let role = app.repos.role_repo.find_by_name(name).await.unwrap()?;
    let mut permissions: Vec<String> = role.permissions.into_iter().collect();
    permissions.sort();
    Some(permissions)
}

fn sorted(role: &AuthRole) -> Vec<String> {
    let mut permissions: Vec<String> = role.permissions.iter().cloned().collect();
    permissions.sort();
    permissions
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

#[tokio::test]
#[ignore = "requires Docker"]
async fn the_bff_platform_role_sync_is_bounded_by_the_callers_permissions() {
    let app = TestApp::setup().await;
    let admin = app.anchor_admin_token().await;
    let lesser = anchor_holding(&app, &ROLE_GATE);

    let (status, body) = sync_platform(&app, &admin).await;
    assert_eq!(status, StatusCode::OK, "the wildcard holder seeds: {body}");
    let (status, body) = sync_platform(&app, &lesser).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "nothing changes, nothing to check: {body}"
    );

    // Restoring a trimmed built-in role adds a permission the caller lacks.
    let readonly = roles::iam_readonly().name;
    let mut trimmed = app
        .repos
        .role_repo
        .find_by_name(&readonly)
        .await
        .unwrap()
        .expect("synced");
    assert!(trimmed.permissions.remove(iam::USER_READ));
    app.repos.role_repo.update(&trimmed).await.unwrap();
    refused(
        "restore",
        sync_platform(&app, &lesser).await,
        iam::USER_READ,
    );
    assert_eq!(
        permissions_of(&app, &readonly).await,
        Some(sorted(&trimmed)),
        "a refused sync writes nothing"
    );
    let holder = anchor_holding(&app, &[&ROLE_GATE[..], &[iam::USER_READ]].concat());
    let (status, body) = sync_platform(&app, &holder).await;
    assert_eq!(status, StatusCode::OK, "a holder restores it: {body}");
    assert_eq!(
        permissions_of(&app, &readonly).await,
        Some(sorted(&roles::iam_readonly()))
    );

    // Sweeping a stale CODE role removes its permissions.
    let mut stale = AuthRole::new("platform", "ceil-stale", "Stale");
    stale.source = RoleSource::Code;
    stale.permissions = ["platform:ceil:thing:edit".to_string()].into();
    app.repos.role_repo.insert(&stale).await.unwrap();
    refused(
        "sweep",
        sync_platform(&app, &lesser).await,
        "platform:ceil:thing:edit",
    );
    assert_eq!(
        permissions_of(&app, "platform:ceil-stale").await,
        Some(vec!["platform:ceil:thing:edit".to_string()]),
        "a refused sweep deletes nothing"
    );
    let (status, body) = sync_platform(&app, &admin).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["removed"], 1, "{body}");
    assert_eq!(permissions_of(&app, "platform:ceil-stale").await, None);
}

const VIEW: &str = "platform:roleceil:thing:view";
const EDIT: &str = "platform:roleceil:thing:edit";
const EXTRA: &str = "platform:roleceil:thing:extra";

fn code_role(name: &str, permissions: &[&str]) -> AuthRole {
    let mut role = AuthRole::new("platform", name, "Ceiling catalogue");
    role.source = RoleSource::Code;
    role.permissions = permissions.iter().map(|p| p.to_string()).collect();
    role
}

/// The built-in catalogue plus `extra`.
fn with(extra: &[AuthRole]) -> Vec<AuthRole> {
    let mut catalogue = roles::all();
    catalogue.extend(extra.iter().cloned());
    catalogue
}

/// An anchor principal holding exactly `perms`, as the BFF route builds it.
fn principal(perms: &[&str]) -> Caller {
    Caller::from_auth(&AuthContext {
        principal_id: PrincipalId::parse("prn_roleceilsync1").unwrap(),
        principal_type: PrincipalType::User,
        scope: UserScope::Anchor,
        email: Some("roleceil@sync.test".to_string()),
        name: "Role ceiling".to_string(),
        accessible_clients: vec!["*".to_string()],
        permissions: perms.iter().map(|p| p.to_string()).collect::<HashSet<_>>(),
        roles: vec![],
        credential: Credential::SessionCookie,
    })
}

fn above(label: &str, result: Result<impl Debug, PlatformError>, named: &str) {
    let err = result.expect_err(label);
    let PlatformError::Coded {
        status,
        code,
        message,
        ..
    } = &err
    else {
        panic!("{label}: expected PERMISSION_ABOVE_CALLER, got {err:?}");
    };
    assert_eq!(*status, StatusCode::FORBIDDEN, "{label}: {message}");
    assert_eq!(code, "PERMISSION_ABOVE_CALLER", "{label}");
    assert_eq!(
        *message,
        format!("You may only add or remove permissions you hold yourself: {named}"),
        "{label}"
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn the_platform_role_sync_is_bounded_by_the_callers_permissions() {
    let app = TestApp::setup().await;
    let sync = RoleSyncService::new(app.repos.role_repo.clone());
    let lesser = principal(&[&ROLE_GATE[..], &[VIEW]].concat());
    let super_admin = principal(&["platform:*:*:*"]);
    let code = "platform:roleceil-code";

    // The boot sync runs as the system, which holds everything.
    sync.sync_code_defined_roles(&Caller::system())
        .await
        .expect("boot sync");
    sync.sync_catalogue(
        &with(&[code_role("roleceil-code", &[VIEW])]),
        &Caller::system(),
    )
    .await
    .expect("the system adds anything");
    sync.sync_catalogue(&with(&[code_role("roleceil-code", &[VIEW])]), &lesser)
        .await
        .expect("nothing changes, nothing to check");

    // An addition the caller lacks refuses the whole sync, a new role planned
    // before it (holding only a held permission) included.
    above(
        "addition",
        sync.sync_catalogue(
            &with(&[
                code_role("roleceil-new", &[VIEW]),
                code_role("roleceil-code", &[VIEW, EDIT]),
            ]),
            &lesser,
        )
        .await,
        EDIT,
    );
    assert_eq!(
        permissions_of(&app, code).await,
        Some(vec![VIEW.to_string()]),
        "a refused sync writes nothing"
    );
    assert_eq!(permissions_of(&app, "platform:roleceil-new").await, None);

    // A new catalogue role: all its permissions.
    above(
        "new role",
        sync.sync_catalogue(
            &with(&[
                code_role("roleceil-code", &[VIEW]),
                code_role("roleceil-fresh", &[VIEW, EDIT]),
            ]),
            &lesser,
        )
        .await,
        EDIT,
    );
    assert_eq!(permissions_of(&app, "platform:roleceil-fresh").await, None);

    // Confinement runs first.
    let err = sync
        .sync_catalogue(
            &with(&[code_role(
                "roleceil-code",
                &[VIEW, EDIT, "other:thing:view"],
            )]),
            &lesser,
        )
        .await
        .expect_err("another application's permission");
    assert!(
        matches!(&err, PlatformError::Coded { code, .. } if code == "PERMISSION_OUTSIDE_APPLICATION"),
        "{err:?}"
    );

    sync.sync_catalogue(
        &with(&[code_role("roleceil-code", &[VIEW, EDIT])]),
        &super_admin,
    )
    .await
    .expect("the wildcard holder may");

    // Removals count: trimming the role, and sweeping it.
    above(
        "removal",
        sync.sync_catalogue(&with(&[code_role("roleceil-code", &[VIEW])]), &lesser)
            .await,
        EDIT,
    );
    above(
        "sweep",
        sync.sync_catalogue(&with(&[]), &lesser).await,
        EDIT,
    );
    assert_eq!(
        permissions_of(&app, code).await,
        Some(vec![EDIT.to_string(), VIEW.to_string()]),
        "a refused removal or sweep writes nothing"
    );

    // The message names each permission once: per role its additions, then
    // its removals, then the swept roles'.
    let other = code_role("roleceil-other", &[EDIT, EXTRA]);
    app.repos.role_repo.insert(&other).await.unwrap();
    above(
        "order",
        sync.sync_catalogue(
            &with(&[code_role("roleceil-code", &[VIEW, EXTRA])]),
            &lesser,
        )
        .await,
        &format!("{EXTRA}, {EDIT}"),
    );
    app.repos.role_repo.delete(&other.id).await.unwrap();

    // Untouched rows are not checked: a same-named role of another source,
    // and a stale CODE role still assigned (kept).
    let mut database = code_role("roleceil-db", &[]);
    database.source = RoleSource::Database;
    app.repos.role_repo.insert(&database).await.unwrap();
    let held = code_role("roleceil-held", &[EDIT]);
    app.repos.role_repo.insert(&held).await.unwrap();
    let mut holder = Principal::new_user("roleceil-holder@sync.test", UserScope::Anchor);
    holder.roles.push(RoleAssignment::with_source(
        "platform:roleceil-held",
        AssignmentSource::AdminAssigned,
    ));
    app.repos.principal_repo.insert(&holder).await.unwrap();
    sync.sync_catalogue(
        &with(&[
            code_role("roleceil-code", &[VIEW, EDIT]),
            code_role("roleceil-db", &[EDIT]),
        ]),
        &lesser,
    )
    .await
    .expect("skipped and kept rows change nothing");
    assert_eq!(
        permissions_of(&app, "platform:roleceil-db").await,
        Some(vec![])
    );
    assert_eq!(
        permissions_of(&app, "platform:roleceil-held").await,
        Some(vec![EDIT.to_string()])
    );

    // The system sweeps.
    let counts = sync
        .sync_catalogue(&with(&[]), &Caller::system())
        .await
        .expect("the system sweeps");
    assert_eq!(counts.removed, 1);
    assert_eq!(permissions_of(&app, code).await, None);
    assert!(permissions_of(&app, "platform:roleceil-held")
        .await
        .is_some());
}
