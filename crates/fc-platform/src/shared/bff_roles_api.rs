//! BFF Roles API
//!
//! Backend-For-Frontend endpoints for role management.
//! Provides a UI-friendly view of roles at `/bff/roles`.

use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::application::repository::ApplicationRepository;
use crate::role::entity::AuthRole;
use crate::role::operations::{
    CreateRoleCommand, CreateRoleUseCase, DeleteRoleCommand, DeleteRoleUseCase, UpdateRoleCommand,
    UpdateRoleUseCase,
};
use crate::role::repository::RoleRepository;
use crate::shared::api_common::CreatedResponse;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;
use crate::usecase::{ExecutionContext, PgUnitOfWork, UseCase};

// ── Response DTOs ──────────────────────────────────────────────────────────

/// BFF role response, Go's `bffRoleResponse` (shared/bff/roles.go):
/// `description` absent when unset, permissions sorted.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BffRoleResponse {
    pub id: String,
    /// Full name e.g. "myapp:admin"
    pub name: String,
    /// The name without its `{applicationCode}:` prefix, e.g. "admin"
    pub short_name: String,
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub permissions: Vec<String>,
    pub application_code: String,
    /// "CODE", "DATABASE", "SDK"
    pub source: String,
    pub client_managed: bool,
    /// ISO8601
    pub created_at: String,
    /// ISO8601
    pub updated_at: String,
}

impl From<AuthRole> for BffRoleResponse {
    fn from(r: AuthRole) -> Self {
        // Go `Role.ShortName`: strip exactly the `{applicationCode}:`
        // prefix, so a multi-colon name keeps its inner colons.
        let short_name = r
            .name
            .strip_prefix(&format!("{}:", r.application_code))
            .unwrap_or(&r.name)
            .to_string();
        let mut permissions: Vec<String> = r.permissions.into_iter().collect();
        permissions.sort();
        Self {
            id: r.id,
            name: r.name,
            short_name,
            display_name: r.display_name,
            description: r.description,
            application_code: r.application_code,
            permissions,
            source: r.source.as_str().to_string(),
            client_managed: r.client_managed,
            created_at: r.created_at.to_rfc3339(),
            updated_at: r.updated_at.to_rfc3339(),
        }
    }
}

/// BFF role list response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BffRoleListResponse {
    pub items: Vec<BffRoleResponse>,
    pub total: usize,
}

/// Application option for filter dropdown
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BffApplicationOption {
    pub id: String,
    pub code: String,
    pub name: String,
}

/// Application options response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BffApplicationOptionsResponse {
    pub options: Vec<BffApplicationOption>,
}

/// Permission response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BffPermissionResponse {
    pub permission: String,
    pub application: String,
    pub context: String,
    pub aggregate: String,
    pub action: String,
    pub description: String,
}

/// Permission list response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BffPermissionListResponse {
    pub items: Vec<BffPermissionResponse>,
    pub total: usize,
}

// ── Request DTOs ──────────────────────────────────────────────────────────

/// Create role request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BffCreateRoleRequest {
    /// Application code this role belongs to
    pub application_code: String,
    /// Role name (will be combined with app code to form full name)
    pub role_name: String,
    /// Display name
    pub display_name: String,
    /// Description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Initial permissions
    #[serde(default)]
    pub permissions: Vec<String>,
    /// Whether clients can manage this role
    #[serde(default)]
    pub client_managed: bool,
}

/// Update role request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BffUpdateRoleRequest {
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub client_managed: Option<bool>,
    pub permissions: Option<Vec<String>>,
}

// ── Query parameters ──────────────────────────────────────────────────────

/// Query parameters for BFF roles list
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct BffRolesQuery {
    /// Filter by application code
    pub application: Option<String>,
    /// Filter by source (CODE, DATABASE, SDK)
    pub source: Option<String>,
}

// ── State ─────────────────────────────────────────────────────────────────

/// BFF roles service state
#[derive(Clone)]
pub struct BffRolesState {
    pub role_repo: Arc<RoleRepository>,
    pub application_repo: Arc<ApplicationRepository>,
    pub unit_of_work: Arc<PgUnitOfWork>,
    pub role_sync_service: Arc<crate::shared::role_sync_service::RoleSyncService>,
    /// The persistent permission catalogue (`iam_permissions`), merged into
    /// the catalogue listing as Go does.
    pub permission_repo: Arc<crate::role::permission_repository::PermissionCatalogRepository>,
}

// ── Helpers ───────────────────────────────────────────────────────────────

// ── Handlers ──────────────────────────────────────────────────────────────

/// List roles with optional filters
#[utoipa::path(
    get,
    path = "",
    tag = "bff-roles",
    operation_id = "getBffRoles",
    params(BffRolesQuery),
    responses(
        (status = 200, description = "List of roles", body = BffRoleListResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_roles(
    State(state): State<BffRolesState>,
    _auth: Authenticated,
    Query(query): Query<BffRolesQuery>,
) -> Result<Json<BffRoleListResponse>, PlatformError> {
    // Go filters every role in memory: `application` exactly,
    // `source` case-insensitively, both when both are given.
    let application = query.application.as_deref().filter(|s| !s.is_empty());
    let source = query.source.as_deref().filter(|s| !s.is_empty());
    let roles: Vec<BffRoleResponse> = state
        .role_repo
        .find_all()
        .await?
        .into_iter()
        .filter(|r| application.is_none_or(|a| r.application_code == a))
        .filter(|r| source.is_none_or(|s| r.source.as_str().eq_ignore_ascii_case(s)))
        .map(Into::into)
        .collect();
    let total = roles.len();
    Ok(Json(BffRoleListResponse {
        items: roles,
        total,
    }))
}

/// Get applications for role filter dropdown
#[utoipa::path(
    get,
    path = "/filters/applications",
    tag = "bff-roles",
    operation_id = "getBffRolesFiltersApplications",
    responses(
        (status = 200, description = "Application options", body = BffApplicationOptionsResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_filter_applications(
    State(state): State<BffRolesState>,
    _auth: Authenticated,
) -> Result<Json<BffApplicationOptionsResponse>, PlatformError> {
    // Go `FindActive`: active applications ordered by code.
    let mut apps = state.application_repo.find_active().await?;
    apps.sort_by(|a, b| a.code.cmp(&b.code));
    let options = apps
        .into_iter()
        .map(|a| BffApplicationOption {
            id: a.id,
            code: a.code,
            name: a.name,
        })
        .collect();

    Ok(Json(BffApplicationOptionsResponse { options }))
}

/// Query parameters for the permission catalogue.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct BffPermissionsQuery {
    /// `platform` for the built-ins, another code for that application's
    /// role-derived permissions; omitted for everything.
    pub application: Option<String>,
}

/// A four-segment permission as a catalogue entry (Go `parsePermission`);
/// `None` for any other arity.
fn parse_permission(p: &str) -> Option<BffPermissionResponse> {
    let parts: Vec<&str> = p.split(':').collect();
    if parts.len() != 4 {
        return None;
    }
    Some(BffPermissionResponse {
        permission: p.to_string(),
        application: parts[0].to_string(),
        context: parts[1].to_string(),
        aggregate: parts[2].to_string(),
        action: parts[3].to_string(),
        description: String::new(),
    })
}

/// The permission catalogue for an application code (Go
/// `permissionCatalog`, shared/bff/roles.go): `""` is the built-ins plus
/// every non-platform role's permissions, `platform` the built-ins, any
/// other code that application's role permissions; then the persistent
/// catalogue rows for the same filter, a code kept at its first appearance.
async fn permission_catalog(
    state: &BffRolesState,
    app: &str,
) -> Result<Vec<BffPermissionResponse>, PlatformError> {
    let role_derived = |roles: &[AuthRole], app: &str| {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for role in roles {
            if role.application_code == "platform" {
                continue;
            }
            if !app.is_empty() && role.application_code != app {
                continue;
            }
            let mut perms: Vec<&String> = role.permissions.iter().collect();
            perms.sort();
            for p in perms {
                if !seen.insert(p.clone()) {
                    continue;
                }
                if let Some(entry) = parse_permission(p) {
                    if app.is_empty() || entry.application == app {
                        out.push(entry);
                    }
                }
            }
        }
        out.sort_by(|a, b| a.permission.cmp(&b.permission));
        out
    };
    let base = match app {
        "platform" => get_builtin_permissions(),
        _ => {
            let roles = state.role_repo.find_all().await?;
            let derived = role_derived(&roles, app);
            if app.is_empty() {
                let mut all = get_builtin_permissions();
                all.extend(derived);
                all
            } else {
                derived
            }
        }
    };
    let catalogue = state
        .permission_repo
        .find_all()
        .await?
        .into_iter()
        .filter_map(|row| {
            let mut entry = parse_permission(&row.code)?;
            if !app.is_empty() && entry.application != app {
                return None;
            }
            if let Some(d) = row.description {
                entry.description = d;
            }
            Some(entry)
        });
    let mut seen = std::collections::HashSet::new();
    Ok(base
        .into_iter()
        .chain(catalogue)
        .filter(|p| seen.insert(p.permission.clone()))
        .collect())
}

/// List the permission catalogue (Go `listPermissions`)
#[utoipa::path(
    get,
    path = "/permissions",
    tag = "bff-roles",
    operation_id = "getBffRolesPermissions",
    params(BffPermissionsQuery),
    responses(
        (status = 200, description = "List of permissions", body = BffPermissionListResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_permissions(
    State(state): State<BffRolesState>,
    _auth: Authenticated,
    Query(query): Query<BffPermissionsQuery>,
) -> Result<Json<BffPermissionListResponse>, PlatformError> {
    let permissions =
        permission_catalog(&state, query.application.as_deref().unwrap_or("")).await?;
    let total = permissions.len();
    Ok(Json(BffPermissionListResponse {
        items: permissions,
        total,
    }))
}

/// Get one catalogue entry, from every application (Go `getPermission`)
#[utoipa::path(
    get,
    path = "/permissions/{permission}",
    tag = "bff-roles",
    operation_id = "getBffRolesPermissionsByPermission",
    params(
        ("permission" = String, Path, description = "Permission string")
    ),
    responses(
        (status = 200, description = "Permission found", body = BffPermissionResponse),
        (status = 404, description = "Permission not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_permission(
    State(state): State<BffRolesState>,
    _auth: Authenticated,
    Path(permission): Path<String>,
) -> Result<Json<BffPermissionResponse>, PlatformError> {
    let found = permission_catalog(&state, "")
        .await?
        .into_iter()
        .find(|p| p.permission == permission)
        .ok_or_else(|| PlatformError::not_found("Permission", &permission))?;

    Ok(Json(found))
}

/// Get role by name (code)
#[utoipa::path(
    get,
    path = "/{roleName}",
    tag = "bff-roles",
    operation_id = "getBffRolesByName",
    params(
        ("roleName" = String, Path, description = "Role name (code) or ID")
    ),
    responses(
        (status = 200, description = "Role found", body = BffRoleResponse),
        (status = 404, description = "Role not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_role(
    State(state): State<BffRolesState>,
    _auth: Authenticated,
    Path(role_name): Path<String>,
) -> Result<Json<BffRoleResponse>, PlatformError> {
    let role = if role_name.contains(':') {
        state.role_repo.find_by_name(&role_name).await?
    } else {
        state.role_repo.find_by_id(&role_name).await?
    };

    let role = role.ok_or_else(|| PlatformError::not_found("Role", &role_name))?;
    Ok(Json(role.into()))
}

/// Create a new role
#[utoipa::path(
    post,
    path = "",
    tag = "bff-roles",
    operation_id = "postBffRoles",
    request_body = BffCreateRoleRequest,
    responses(
        (status = 201, description = "Role created", body = CreatedResponse),
        (status = 400, description = "Validation error"),
        (status = 409, description = "Duplicate role code")
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_role(
    State(state): State<BffRolesState>,
    auth: Authenticated,
    Json(req): Json<BffCreateRoleRequest>,
) -> Result<(axum::http::StatusCode, Json<CreatedResponse>), PlatformError> {
    crate::shared::authorization_service::checks::can_administer_bff_roles(
        &auth.0,
        crate::permissions::iam::ROLE_CREATE,
    )?;
    // Owner ruling 14: only permissions the caller holds.
    crate::role::ceiling::require_permissions(
        Some(&auth.0),
        req.permissions.iter().map(String::as_str),
    )?;

    let cmd = CreateRoleCommand {
        application_code: req.application_code,
        role_name: req.role_name,
        display_name: req.display_name,
        description: req.description,
        permissions: req.permissions,
        client_managed: req.client_managed,
        source: crate::role::entity::RoleSource::Database,
        // Owner ruling 15: a super-admin may use another application's
        // permissions through the admin API.
        cross_application: auth.0.has_permission(crate::permissions::ADMIN_ALL),
    };

    let ctx = ExecutionContext::from_auth(&auth.0);
    let use_case = CreateRoleUseCase::new(state.role_repo.clone(), state.unit_of_work.clone());
    let event = use_case.run(cmd, ctx).await.into_result()?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(CreatedResponse::new(event.role_id)),
    ))
}

/// Update role
#[utoipa::path(
    put,
    path = "/{roleName}",
    tag = "bff-roles",
    operation_id = "putBffRolesByName",
    params(
        ("roleName" = String, Path, description = "Role name (code) or ID")
    ),
    request_body = BffUpdateRoleRequest,
    responses(
        (status = 204, description = "Role updated"),
        (status = 404, description = "Role not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_role(
    State(state): State<BffRolesState>,
    auth: Authenticated,
    Path(role_name): Path<String>,
    Json(req): Json<BffUpdateRoleRequest>,
) -> Result<axum::http::StatusCode, PlatformError> {
    crate::shared::authorization_service::checks::can_administer_bff_roles(
        &auth.0,
        crate::permissions::iam::ROLE_UPDATE,
    )?;

    // Resolve role name to ID
    let role = if role_name.contains(':') {
        state.role_repo.find_by_name(&role_name).await?
    } else {
        state.role_repo.find_by_id(&role_name).await?
    }
    .ok_or_else(|| PlatformError::not_found("Role", &role_name))?;

    let role_id = role.id.clone();
    // Owner ruling 14: only permissions the caller holds may be added or
    // removed.
    if let Some(ref permissions) = req.permissions {
        let before: Vec<String> = role.permissions.iter().cloned().collect();
        crate::role::ceiling::require_permissions(
            Some(&auth.0),
            crate::role::ceiling::changed(&before, permissions)
                .iter()
                .map(String::as_str),
        )?;
    }

    let cmd = UpdateRoleCommand {
        role_id: role_id.clone(),
        display_name: req.display_name,
        description: req.description,
        permissions: req.permissions,
        client_managed: req.client_managed,
        cross_application: auth.0.has_permission(crate::permissions::ADMIN_ALL),
    };

    let ctx = ExecutionContext::from_auth(&auth.0);
    let use_case = UpdateRoleUseCase::new(state.role_repo.clone(), state.unit_of_work.clone());
    use_case.run(cmd, ctx).await.into_result()?;

    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// Delete role
#[utoipa::path(
    delete,
    path = "/{roleName}",
    tag = "bff-roles",
    operation_id = "deleteBffRolesByName",
    params(
        ("roleName" = String, Path, description = "Role name (code) or ID")
    ),
    responses(
        (status = 204, description = "Role deleted"),
        (status = 404, description = "Role not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_role(
    State(state): State<BffRolesState>,
    auth: Authenticated,
    Path(role_name): Path<String>,
) -> Result<axum::http::StatusCode, PlatformError> {
    crate::shared::authorization_service::checks::can_administer_bff_roles(
        &auth.0,
        crate::permissions::iam::ROLE_DELETE,
    )?;

    // Resolve role name to ID
    let role = if role_name.contains(':') {
        state.role_repo.find_by_name(&role_name).await?
    } else {
        state.role_repo.find_by_id(&role_name).await?
    }
    .ok_or_else(|| PlatformError::not_found("Role", &role_name))?;

    // Owner ruling 14: deleting a role withdraws every permission it holds.
    crate::role::ceiling::require_permissions(
        Some(&auth.0),
        role.permissions.iter().map(String::as_str),
    )?;

    let cmd = DeleteRoleCommand {
        role_id: role.id.clone(),
    };

    let ctx = ExecutionContext::from_auth(&auth.0);
    let use_case = DeleteRoleUseCase::new(state.role_repo.clone(), state.unit_of_work.clone());
    use_case.run(cmd, ctx).await.into_result()?;

    Ok(axum::http::StatusCode::NO_CONTENT)
}

// ── Permissions registry ──────────────────────────────────────────────────

/// Go's `builtinPermissions`: the static platform catalogue, sorted by
/// code.
fn get_builtin_permissions() -> Vec<BffPermissionResponse> {
    let mut all = vec![
        // IAM Permissions
        perm("platform", "iam", "user", "view", "View users"),
        perm("platform", "iam", "user", "create", "Create users"),
        perm("platform", "iam", "user", "update", "Update users"),
        perm("platform", "iam", "user", "delete", "Delete users"),
        perm("platform", "iam", "role", "view", "View roles"),
        perm("platform", "iam", "role", "create", "Create roles"),
        perm("platform", "iam", "role", "update", "Update roles"),
        perm("platform", "iam", "role", "delete", "Delete roles"),
        perm("platform", "iam", "permission", "view", "View permissions"),
        perm(
            "platform",
            "iam",
            "service-account",
            "view",
            "View service accounts",
        ),
        perm(
            "platform",
            "iam",
            "service-account",
            "create",
            "Create service accounts",
        ),
        perm(
            "platform",
            "iam",
            "service-account",
            "update",
            "Update service accounts",
        ),
        perm(
            "platform",
            "iam",
            "service-account",
            "delete",
            "Delete service accounts",
        ),
        perm(
            "platform",
            "iam",
            "idp",
            "manage",
            "Manage identity providers",
        ),
        // Admin Permissions
        perm("platform", "admin", "client", "view", "View clients"),
        perm("platform", "admin", "client", "create", "Create clients"),
        perm("platform", "admin", "client", "update", "Update clients"),
        perm("platform", "admin", "client", "delete", "Delete clients"),
        perm(
            "platform",
            "admin",
            "application",
            "view",
            "View applications",
        ),
        perm(
            "platform",
            "admin",
            "application",
            "create",
            "Create applications",
        ),
        perm(
            "platform",
            "admin",
            "application",
            "update",
            "Update applications",
        ),
        perm(
            "platform",
            "admin",
            "application",
            "delete",
            "Delete applications",
        ),
        perm(
            "platform",
            "admin",
            "config",
            "view",
            "View platform config",
        ),
        perm(
            "platform",
            "admin",
            "config",
            "update",
            "Update platform config",
        ),
        // Messaging Permissions
        perm("platform", "messaging", "event", "view", "View events"),
        perm(
            "platform",
            "messaging",
            "event",
            "view-raw",
            "View raw event data",
        ),
        perm(
            "platform",
            "messaging",
            "event-type",
            "view",
            "View event types",
        ),
        perm(
            "platform",
            "messaging",
            "event-type",
            "create",
            "Create event types",
        ),
        perm(
            "platform",
            "messaging",
            "event-type",
            "update",
            "Update event types",
        ),
        perm(
            "platform",
            "messaging",
            "event-type",
            "delete",
            "Delete event types",
        ),
        perm(
            "platform",
            "messaging",
            "subscription",
            "view",
            "View subscriptions",
        ),
        perm(
            "platform",
            "messaging",
            "subscription",
            "create",
            "Create subscriptions",
        ),
        perm(
            "platform",
            "messaging",
            "subscription",
            "update",
            "Update subscriptions",
        ),
        perm(
            "platform",
            "messaging",
            "subscription",
            "delete",
            "Delete subscriptions",
        ),
        perm(
            "platform",
            "messaging",
            "dispatch-job",
            "view",
            "View dispatch jobs",
        ),
        perm(
            "platform",
            "messaging",
            "dispatch-job",
            "view-raw",
            "View raw dispatch job data",
        ),
        perm(
            "platform",
            "messaging",
            "dispatch-job",
            "create",
            "Create dispatch jobs",
        ),
        perm(
            "platform",
            "messaging",
            "dispatch-job",
            "retry",
            "Retry dispatch jobs",
        ),
        perm(
            "platform",
            "messaging",
            "dispatch-pool",
            "view",
            "View dispatch pools",
        ),
        perm(
            "platform",
            "messaging",
            "dispatch-pool",
            "create",
            "Create dispatch pools",
        ),
        perm(
            "platform",
            "messaging",
            "dispatch-pool",
            "update",
            "Update dispatch pools",
        ),
        perm(
            "platform",
            "messaging",
            "dispatch-pool",
            "delete",
            "Delete dispatch pools",
        ),
    ];
    all.sort_by(|a, b| a.permission.cmp(&b.permission));
    all
}

fn perm(app: &str, ctx: &str, agg: &str, action: &str, desc: &str) -> BffPermissionResponse {
    BffPermissionResponse {
        permission: format!("{}:{}:{}:{}", app, ctx, agg, action),
        application: app.to_string(),
        context: ctx.to_string(),
        aggregate: agg.to_string(),
        action: action.to_string(),
        description: desc.to_string(),
    }
}

// ── Sync platform roles ───────────────────────────────────────────────────

/// Response for the platform-roles sync endpoint. Mirrors the shape of the
/// EventTypes sync response so the dashboard can render both with one toast.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyncPlatformRolesResponse {
    pub created: u32,
    pub updated: u32,
    pub removed: u32,
    /// Total count of code-defined roles after sync.
    pub total: u32,
}

/// Re-run the code-defined role sync (the same one the binary runs at boot).
///
/// Used by the admin dashboard to pick up newly-added platform roles without
/// restarting the server. Anchor-only.
#[utoipa::path(
    post,
    path = "/sync-platform",
    tag = "bff-roles",
    operation_id = "postBffRolesSyncPlatform",
    responses(
        (status = 200, description = "Platform roles synced", body = SyncPlatformRolesResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn sync_platform_roles(
    State(state): State<BffRolesState>,
    auth: Authenticated,
) -> Result<axum::Json<SyncPlatformRolesResponse>, PlatformError> {
    crate::shared::authorization_service::checks::can_sync_platform_roles(&auth.0)?;

    let counts = state
        .role_sync_service
        .sync_code_defined_roles()
        .await
        .map_err(|e| PlatformError::internal(format!("Role sync failed: {}", e)))?;

    Ok(axum::Json(SyncPlatformRolesResponse {
        created: counts.created,
        updated: counts.updated,
        removed: counts.removed,
        total: counts.total,
    }))
}

// ── Router ────────────────────────────────────────────────────────────────

/// Create BFF roles router (mounted at `/bff/roles`)
pub fn bff_roles_router(state: BffRolesState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(create_role, list_roles))
        .routes(routes!(get_filter_applications))
        .routes(routes!(list_permissions))
        .routes(routes!(get_permission))
        .routes(routes!(sync_platform_roles))
        .routes(routes!(get_role, update_role, delete_role))
        .with_state(state)
}
