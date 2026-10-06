//! Resource-level authorization lives in the use cases (platform-uniformity
//! phase 2): each test runs a use case directly, with no HTTP handler in
//! front, as an out-of-reach [`Caller`] and asserts the refusal is exactly
//! the one the handler used to answer, rendered the same way, with nothing
//! written. That proves the rule holds for every caller (API, BFF, fc-web,
//! orchestrations), not just the route that used to check it. Requires
//! Docker.

use crate::support;
use fc_platform::shared::id::PrincipalId;
use fc_platform_core::shared::id::ClientId;

use std::collections::HashSet;

use axum::body;
use axum::response::IntoResponse;
use fc_platform::checks;
use fc_platform::client::entity::Client;
use fc_platform::role::entity::roles;
use fc_platform::shared::authorization_service::{AuthContext, Credential};
use fc_platform::usecase::{ExecutionContext, UseCase, UseCaseError};
use fc_platform::{PlatformError, PrincipalType, UserScope};
use std::fmt;
use support::TestApp;

/// A principal caller with the given tier, clients and permissions.
fn caller(scope: UserScope, clients: &[&str], perms: &[&str]) -> ExecutionContext {
    ExecutionContext::from_auth(&AuthContext {
        principal_id: PrincipalId::parse("prn_caller000001").unwrap(),
        principal_type: PrincipalType::User,
        scope,
        email: Some("caller@authz.test".to_string()),
        name: "Caller".to_string(),
        accessible_clients: clients.iter().map(|c| c.to_string()).collect(),
        permissions: perms.iter().map(|p| p.to_string()).collect::<HashSet<_>>(),
        roles: vec![],
        credential: Credential::BearerToken,
    })
}

/// Status and body a use-case failure renders with, through the handler's
/// `into_result()?` conversion.
async fn rendered(err: UseCaseError) -> (u16, String) {
    render(PlatformError::from(err)).await
}

async fn render(err: PlatformError) -> (u16, String) {
    let resp = err.into_response();
    let status = resp.status().as_u16();
    let bytes = body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

fn refusal<T: fmt::Debug>(r: fc_platform::UseCaseResult<T>) -> UseCaseError {
    match r.into_result() {
        Err(e) => e,
        Ok(v) => panic!("expected a refusal, got {v:?}"),
    }
}

async fn insert_client(app: &TestApp, identifier: &str) -> String {
    let client = Client::new(identifier.to_uppercase(), identifier);
    app.repos.client_repo.insert(&client).await.unwrap();
    client.id.to_string()
}

async fn insert_user(
    app: &TestApp,
    email: &str,
    scope: UserScope,
    client: Option<&str>,
) -> fc_platform::Principal {
    let mut p = fc_platform::Principal::new_user(email, scope);
    if let Some(c) = client {
        p = p.with_client_id(ClientId::parse(c).unwrap());
    }
    app.repos.principal_repo.insert(&p).await.unwrap();
    p
}

async fn setup() -> TestApp {
    support::set_app_key();
    TestApp::setup().await
}

// ── IAM ──────────────────────────────────────────────────────────────────

/// A client administrator can't update a user of another client: the
/// same `Principal_NOT_FOUND` a missing id gets. A PARTNER-tier target is
/// the 403 `blockNonClientTarget` refusal. Nothing is written.
#[tokio::test]
#[ignore = "requires Docker"]
async fn user_writes_are_confined_to_the_callers_clients() {
    use fc_platform::principal::operations::{UpdateUserCommand, UpdateUserUseCase};
    let app = setup().await;
    let mine = insert_client(&app, "authz-mine").await;
    let theirs = insert_client(&app, "authz-theirs").await;
    let other = insert_user(&app, "other@authz.test", UserScope::Client, Some(&theirs)).await;
    let partner = insert_user(&app, "partner@authz.test", UserScope::Partner, Some(&mine)).await;
    let use_case =
        UpdateUserUseCase::new(app.repos.principal_repo.clone(), app.unit_of_work.clone());
    let admin = || caller(UserScope::Client, &[&mine], &["platform:iam:user:update"]);
    let cmd = |id: &str| UpdateUserCommand {
        principal_id: PrincipalId::from_wire(id),
        name: Some("Renamed".to_string()),
        first_name: None,
        last_name: None,
        active: None,
        scope: None,
        client_id: None,
        email: None,
    };

    let err = refusal(use_case.run(cmd(other.id.as_str()), admin()).await);
    assert_eq!(
        rendered(err).await,
        render(PlatformError::not_found("Principal", other.id.as_str())).await
    );
    let err = refusal(use_case.run(cmd(partner.id.as_str()), admin()).await);
    assert_eq!(
        rendered(err).await,
        render(PlatformError::forbidden(
            "Client administrators can only manage client-scope users"
        ))
        .await
    );
    assert_eq!(app.audit_count_for(other.id.as_str()).await, 0);

    // The system caller reaches every user.
    use_case
        .run(cmd(other.id.as_str()), ExecutionContext::system("system"))
        .await
        .into_result()
        .expect("system update");
}

/// Owner ruling 14 in the use case: nobody assigns a role whose platform
/// permissions they don't hold, whichever surface runs it.
#[tokio::test]
#[ignore = "requires Docker"]
async fn role_assignment_is_bounded_by_the_callers_ceiling() {
    use fc_platform::principal::operations::{AssignUserRolesCommand, AssignUserRolesUseCase};
    let app = setup().await;
    let target = insert_user(&app, "target@authz.test", UserScope::Anchor, None).await;
    let super_admin = roles::super_admin();
    if app
        .repos
        .role_repo
        .find_by_name(&super_admin.name)
        .await
        .unwrap()
        .is_none()
    {
        app.repos.role_repo.insert(&super_admin).await.unwrap();
    }
    let use_case = AssignUserRolesUseCase::new(
        app.repos.principal_repo.clone(),
        app.repos.role_repo.clone(),
        app.unit_of_work.clone(),
    );
    let anchor = caller(
        UserScope::Anchor,
        &["*"],
        &["platform:iam:user:assign-roles"],
    );
    let err = refusal(
        use_case
            .run(
                AssignUserRolesCommand {
                    user_id: target.id.clone(),
                    roles: vec![super_admin.name.clone()],
                },
                anchor,
            )
            .await,
    );
    assert_eq!(err.http_status_code(), 403);
    assert_eq!(err.code(), "ROLE_ABOVE_CALLER");
    assert_eq!(app.audit_count_for(target.id.as_str()).await, 0);
}

/// A role carries only platform permissions its creator holds, also when
/// no admin handler checked first (the application-scoped SDK route).
#[tokio::test]
#[ignore = "requires Docker"]
async fn role_permissions_are_bounded_by_the_callers_ceiling() {
    use fc_platform::role::entity::RoleSource;
    use fc_platform::role::operations::{CreateRoleCommand, CreateRoleUseCase};
    let app = setup().await;
    let use_case = CreateRoleUseCase::new(app.repos.role_repo.clone(), app.unit_of_work.clone());
    let err = refusal(
        use_case
            .run(
                CreateRoleCommand {
                    application_code: "platform".to_string(),
                    role_name: "escalator".to_string(),
                    display_name: "Escalator".to_string(),
                    description: None,
                    permissions: vec!["platform:iam:user:create".to_string()],
                    client_managed: false,
                    source: RoleSource::Sdk,
                    cross_application: false,
                },
                caller(UserScope::Client, &["clt_x"], &["platform:iam:role:create"]),
            )
            .await,
    );
    assert_eq!(err.http_status_code(), 403);
    assert_eq!(err.code(), "PERMISSION_ABOVE_CALLER");
}

/// Platform-owner aggregates refuse a non-anchor caller with the gate's
/// 403 `ANCHOR_REQUIRED`, even when it holds the permission.
#[tokio::test]
#[ignore = "requires Docker"]
async fn platform_owner_writes_need_anchor_scope() {
    use fc_platform::client::operations::{CreateClientCommand, CreateClientUseCase};
    let app = setup().await;
    let use_case =
        CreateClientUseCase::new(app.repos.client_repo.clone(), app.unit_of_work.clone());
    let err = refusal(
        use_case
            .run(
                CreateClientCommand {
                    name: "Sneaky".to_string(),
                    identifier: "sneaky".to_string(),
                },
                caller(
                    UserScope::Partner,
                    &["clt_a"],
                    &["platform:admin:client:create"],
                ),
            )
            .await,
    );
    assert_eq!(
        rendered(err).await,
        render(
            checks::require_anchor_scope(&AuthContext {
                principal_id: PrincipalId::from_wire("p"),
                principal_type: PrincipalType::User,
                scope: UserScope::Client,
                email: None,
                name: String::new(),
                accessible_clients: vec![],
                permissions: HashSet::new(),
                roles: vec![],
                credential: Credential::BearerToken,
            })
            .unwrap_err()
        )
        .await
    );
    assert!(app
        .repos
        .client_repo
        .find_by_identifier("sneaky")
        .await
        .unwrap()
        .is_none());
}

/// A service account's roles: a missing account is the handler's
/// `ServiceAccount_NOT_FOUND`, and the ceiling bounds the change.
#[tokio::test]
#[ignore = "requires Docker"]
async fn service_account_roles_are_bounded_in_the_use_case() {
    use fc_platform::service_account::operations::{AssignRolesCommand, AssignRolesUseCase};
    let app = setup().await;
    let use_case = AssignRolesUseCase::new(
        app.repos.service_account_repo.clone(),
        app.repos.role_repo.clone(),
        app.unit_of_work.clone(),
    );
    let anchor = || {
        caller(
            UserScope::Anchor,
            &["*"],
            &["platform:iam:service-account:update"],
        )
    };
    let err = refusal(
        use_case
            .run(
                AssignRolesCommand {
                    service_account_id: PrincipalId::from_wire("sac_missing00001"),
                    roles: vec![],
                },
                anchor(),
            )
            .await,
    );
    assert_eq!(
        rendered(err).await,
        render(PlatformError::ServiceAccountNotFound {
            id: "sac_missing00001".to_string()
        })
        .await
    );
}

/// A developer credential for a user out of the caller's reach is the
/// same `User_NOT_FOUND` a missing user gets.
#[tokio::test]
#[ignore = "requires Docker"]
async fn developer_credentials_are_confined_to_the_callers_clients() {
    use fc_platform::developer_credential::operations::{
        RevokeDeveloperCredentialCommand, RevokeDeveloperCredentialUseCase,
    };
    let app = setup().await;
    let theirs = insert_client(&app, "authz-dev-theirs").await;
    let other = insert_user(&app, "dev@authz.test", UserScope::Client, Some(&theirs)).await;
    let use_case = RevokeDeveloperCredentialUseCase::new(
        app.repos.principal_repo.clone(),
        app.unit_of_work.clone(),
    );
    let err = refusal(
        use_case
            .run(
                RevokeDeveloperCredentialCommand {
                    principal_id: other.id.clone(),
                },
                caller(
                    UserScope::Client,
                    &["clt_mine"],
                    &["platform:iam:user:update"],
                ),
            )
            .await,
    );
    assert_eq!(
        rendered(err).await,
        render(PlatformError::not_found("User", other.id.as_str())).await
    );
}

// ── Messaging ────────────────────────────────────────────────────────────

/// Go `CheckScopeAccess` on a stored event type, now in the use case: the
/// `/bff` and fc-web routes (which never checked it) get it too. Event
/// types are stored platform-wide (no client column), so a non-anchor
/// caller is refused them.
#[tokio::test]
#[ignore = "requires Docker"]
async fn event_type_writes_check_the_stored_types_scope() {
    use fc_platform::event_type::entity::{EventType, EventTypeCode};
    use fc_platform::event_type::operations::{UpdateEventTypeCommand, UpdateEventTypeUseCase};
    let app = setup().await;
    let et = EventType::new(
        EventTypeCode::parse("authz:orders:order:placed").unwrap(),
        "Order placed",
    );
    app.repos.event_type_repo.insert(&et).await.unwrap();
    let use_case =
        UpdateEventTypeUseCase::new(app.repos.event_type_repo.clone(), app.unit_of_work.clone());
    let err = refusal(
        use_case
            .run(
                UpdateEventTypeCommand {
                    event_type_id: et.id.clone(),
                    name: Some("Renamed".to_string()),
                    description: None,
                    client_scoped: None,
                },
                caller(
                    UserScope::Client,
                    &["clt_mine"],
                    &["platform:messaging:event-type:update"],
                ),
            )
            .await,
    );
    assert_eq!(err.http_status_code(), 403);
    assert_eq!(err.code(), "SCOPE_FORBIDDEN");
    assert_eq!(err.message(), "anchor scope required for this resource");
    assert_eq!(app.audit_count_for(et.id.as_str()).await, 0);
}

/// A platform pool is an anchor's: archiving it as a client caller is 403
/// `SCOPE_FORBIDDEN`, and a non-anchor may not sweep pools by sync.
#[tokio::test]
#[ignore = "requires Docker"]
async fn dispatch_pool_writes_check_scope_in_the_use_case() {
    use fc_platform::dispatch_pool::operations::{
        ArchiveDispatchPoolCommand, ArchiveDispatchPoolUseCase, SyncDispatchPoolsCommand,
        SyncDispatchPoolsUseCase,
    };
    let app = setup().await;
    let pool = fc_platform::DispatchPool::new("authz-pool", "Authz pool");
    app.repos.dispatch_pool_repo.insert(&pool).await.unwrap();
    let client_admin = || {
        caller(
            UserScope::Client,
            &["clt_mine"],
            &["platform:messaging:dispatch-pool:update"],
        )
    };
    let err = refusal(
        ArchiveDispatchPoolUseCase::new(
            app.repos.dispatch_pool_repo.clone(),
            app.unit_of_work.clone(),
        )
        .run(
            ArchiveDispatchPoolCommand {
                id: pool.id.clone(),
            },
            client_admin(),
        )
        .await,
    );
    assert_eq!(
        (err.http_status_code(), err.code(), err.message()),
        (
            403,
            "SCOPE_FORBIDDEN",
            "anchor scope required for this resource"
        )
    );

    let err = refusal(
        SyncDispatchPoolsUseCase::new(
            app.repos.dispatch_pool_repo.clone(),
            app.unit_of_work.clone(),
        )
        .run(
            SyncDispatchPoolsCommand {
                application_code: "authz".to_string(),
                pools: vec![],
                remove_unlisted: true,
                protected_ids: Default::default(),
            },
            client_admin(),
        )
        .await,
    );
    assert_eq!(err.code(), "ANCHOR_REQUIRED_FOR_PLATFORM_SWEEP");
    assert_eq!(app.audit_count_for(pool.id.as_str()).await, 0);
}

// ── Platform admin ───────────────────────────────────────────────────────

/// Writing an application's config needs anchor scope or a write grant on
/// one of the caller's roles (Go's set-property use case), whoever runs it.
#[tokio::test]
#[ignore = "requires Docker"]
async fn config_writes_need_a_write_grant_in_the_use_case() {
    use fc_platform::platform_config::operations::{
        SetPlatformConfigPropertyCommand, SetPlatformConfigPropertyUseCase,
    };
    use fc_platform::ConfigScope;
    let app = setup().await;
    let use_case = SetPlatformConfigPropertyUseCase::new(
        app.repos.platform_config_repo.clone(),
        app.unit_of_work.clone(),
        None,
        app.repos.platform_config_access_repo.clone(),
    );
    let cmd = || SetPlatformConfigPropertyCommand {
        application_code: "authz".to_string(),
        section: "general".to_string(),
        property: "colour".to_string(),
        value: "blue".to_string(),
        scope: ConfigScope::Global,
        client_id: None,
        value_type: None,
        description: None,
    };
    let err = refusal(
        use_case
            .run(cmd(), caller(UserScope::Client, &["clt_a"], &[]))
            .await,
    );
    assert_eq!(
        rendered(err).await,
        render(PlatformError::forbidden(
            "No write access to platform config for authz"
        ))
        .await
    );
    use_case
        .run(cmd(), caller(UserScope::Anchor, &["*"], &[]))
        .await
        .into_result()
        .expect("an anchor writes config");
}
