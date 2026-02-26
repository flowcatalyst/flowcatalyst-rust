//! Principals Admin API
//!
//! REST endpoints for principal (user/service account) management.

use axum::{
    extract::{State, Path, Query},
    Json,
};
use utoipa_axum::{router::OpenApiRouter, routes};
use utoipa::ToSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::principal::entity::{Principal, UserScope, UserIdentity};
use crate::service_account::entity::RoleAssignment;
use crate::principal::repository::PrincipalRepository;
use crate::shared::error::PlatformError;
use crate::shared::api_common::{PaginationParams, CreatedResponse, SuccessResponse};
use crate::shared::middleware::Authenticated;
use crate::{AuditService, PasswordService};

/// Create user request (matches Java CreateUserRequest)
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserRequest {
    /// Email address
    pub email: String,

    /// Password (optional - only for internal auth users)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,

    /// Display name
    pub name: String,

    /// Client ID (for client-bound users)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
}

/// Update principal request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePrincipalRequest {
    /// Display name
    pub name: Option<String>,

    /// First name (for users)
    pub first_name: Option<String>,

    /// Last name (for users)
    pub last_name: Option<String>,

    /// Active status
    pub active: Option<bool>,
}

/// Assign role request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AssignRoleRequest {
    /// Role code
    pub role: String,

    /// Client ID (optional, for client-scoped roles)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
}

/// Batch assign roles request (for PUT /roles - declarative update)
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BatchAssignRolesRequest {
    /// List of role codes to assign (replaces existing roles)
    pub roles: Vec<String>,
}

/// Batch assign roles response (matches Java RolesAssignedResponse)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BatchAssignRolesResponse {
    /// Current role assignments after update
    pub roles: Vec<RoleAssignmentDto>,
    /// Roles that were added
    pub added: Vec<String>,
    /// Roles that were removed
    pub removed: Vec<String>,
}

/// Check email domain query params
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckEmailDomainQuery {
    /// Email address to check
    pub email: String,
}

/// Check email domain response (matches Java EmailDomainCheckResponse)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CheckEmailDomainResponse {
    /// The domain that was checked
    pub domain: String,
    /// Auth provider if configured (INTERNAL, OIDC)
    pub auth_provider: Option<String>,
    /// Whether this is an anchor domain
    pub is_anchor_domain: bool,
    /// Whether this domain has auth configuration
    pub has_auth_config: bool,
    /// Whether the email already exists
    pub email_exists: bool,
    /// Informational message
    pub info: Option<String>,
    /// Warning message
    pub warning: Option<String>,
}

/// Grant client access request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GrantClientAccessRequest {
    /// Client ID to grant access to
    pub client_id: String,
}

/// Client access grant response (matches Java ClientAccessGrantDto)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClientAccessGrantResponse {
    pub id: String,
    pub client_id: String,
    pub granted_at: String,
    pub expires_at: Option<String>,
}

/// Client access list response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClientAccessListResponse {
    pub grants: Vec<ClientAccessGrantResponse>,
}

/// Reset password request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResetPasswordRequest {
    /// New password (min 12 characters)
    pub new_password: String,
}

/// Status change response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StatusChangeResponse {
    pub message: String,
}

/// Role assignment response (for individual role details)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RoleAssignmentResponse {
    pub role: String,
    pub client_id: Option<String>,
    pub assigned_at: String,
}

impl From<&RoleAssignment> for RoleAssignmentResponse {
    fn from(r: &RoleAssignment) -> Self {
        Self {
            role: r.role.clone(),
            client_id: r.client_id.clone(),
            assigned_at: r.assigned_at.to_rfc3339(),
        }
    }
}

/// Role assignment DTO (matches Java RoleAssignmentDto for GET /roles)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RoleAssignmentDto {
    pub id: String,
    pub role_name: String,
    pub assignment_source: String,
    pub assigned_at: String,
}

/// Roles list response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RolesListResponse {
    pub roles: Vec<RoleAssignmentDto>,
}

/// User identity response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserIdentityResponse {
    pub email: String,
    pub email_verified: bool,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub picture_url: Option<String>,
    pub last_login_at: Option<String>,
}

impl From<&UserIdentity> for UserIdentityResponse {
    fn from(i: &UserIdentity) -> Self {
        Self {
            email: i.email.clone(),
            email_verified: i.email_verified,
            first_name: i.first_name.clone(),
            last_name: i.last_name.clone(),
            picture_url: i.picture_url.clone(),
            last_login_at: i.last_login_at.map(|t| t.to_rfc3339()),
        }
    }
}

/// Principal response DTO (matches Java PrincipalDto)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PrincipalResponse {
    pub id: String,
    #[serde(rename = "type")]
    pub principal_type: String,
    pub scope: String,
    pub client_id: Option<String>,
    pub name: String,
    pub active: bool,
    pub email: Option<String>,
    pub idp_type: Option<String>,
    /// Role names (matches Java's Set<String>)
    pub roles: Vec<String>,
    /// Whether user is an anchor domain user
    pub is_anchor_user: bool,
    /// Granted client IDs (matches Java's Set<String>)
    pub granted_client_ids: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<Principal> for PrincipalResponse {
    fn from(p: Principal) -> Self {
        let (email, idp_type) = match &p.user_identity {
            Some(i) => (Some(i.email.clone()), Some("INTERNAL".to_string())),
            None => (None, None),
        };

        Self {
            id: p.id,
            principal_type: format!("{:?}", p.principal_type).to_uppercase(),
            scope: format!("{:?}", p.scope).to_uppercase(),
            client_id: p.client_id,
            name: p.name,
            active: p.active,
            email,
            idp_type,
            roles: p.roles.iter().map(|r| r.role.clone()).collect(),
            is_anchor_user: p.scope == UserScope::Anchor,
            granted_client_ids: p.assigned_clients,
            created_at: p.created_at.to_rfc3339(),
            updated_at: p.updated_at.to_rfc3339(),
        }
    }
}

/// Principal list response (matches Java PrincipalListResponse)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PrincipalListResponse {
    pub principals: Vec<PrincipalResponse>,
    pub total: usize,
}

/// Query parameters for principals list
#[derive(Debug, Deserialize, Default, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PrincipalsQuery {
    #[serde(flatten)]
    pub pagination: PaginationParams,

    /// Filter by type
    #[serde(rename = "type")]
    pub principal_type: Option<String>,

    /// Filter by scope
    pub scope: Option<String>,

    /// Filter by client ID
    pub client_id: Option<String>,
}

/// Principals service state
#[derive(Clone)]
pub struct PrincipalsState {
    pub principal_repo: Arc<PrincipalRepository>,
    pub audit_service: Option<Arc<AuditService>>,
    pub password_service: Option<Arc<PasswordService>>,
    pub anchor_domain_repo: Option<Arc<crate::AnchorDomainRepository>>,
    pub client_auth_config_repo: Option<Arc<crate::ClientAuthConfigRepository>>,
}

fn parse_scope(s: &str) -> Result<UserScope, PlatformError> {
    match s.to_uppercase().as_str() {
        "ANCHOR" => Ok(UserScope::Anchor),
        "PARTNER" => Ok(UserScope::Partner),
        "CLIENT" => Ok(UserScope::Client),
        _ => Err(PlatformError::validation(format!("Invalid scope: {}", s))),
    }
}

/// Create a new user principal
#[utoipa::path(
    post,
    path = "",
    tag = "principals",
    operation_id = "postApiAdminPlatformPrincipals",
    request_body = CreateUserRequest,
    responses(
        (status = 201, description = "User created", body = CreatedResponse),
        (status = 400, description = "Validation error"),
        (status = 409, description = "Duplicate email")
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_user(
    State(state): State<PrincipalsState>,
    auth: Authenticated,
    Json(req): Json<CreateUserRequest>,
) -> Result<Json<CreatedResponse>, PlatformError> {
    // Only anchor or appropriate access
    crate::checks::require_anchor(&auth.0)?;

    // Check for duplicate email
    if let Some(_) = state.principal_repo.find_by_email(&req.email).await? {
        return Err(PlatformError::duplicate("Principal", "email", &req.email));
    }

    // Determine scope based on client_id
    let scope = if req.client_id.is_some() {
        UserScope::Client
    } else {
        UserScope::Anchor
    };

    let mut principal = Principal::new_user(&req.email, scope);
    principal.name = req.name.clone();

    if let Some(cid) = req.client_id.clone() {
        principal = principal.with_client_id(cid);
    }

    let id = principal.id.clone();
    state.principal_repo.insert(&principal).await?;

    // Audit log
    if let Some(ref audit) = state.audit_service {
        let _ = audit.log_create(&auth.0, "Principal", &id, format!("Created user {}", req.email)).await;
    }

    Ok(Json(CreatedResponse::new(id)))
}

/// Get principal by ID
#[utoipa::path(
    get,
    path = "/{id}",
    tag = "principals",
    operation_id = "getApiAdminPlatformPrincipalsById",
    params(
        ("id" = String, Path, description = "Principal ID")
    ),
    responses(
        (status = 200, description = "Principal found", body = PrincipalResponse),
        (status = 404, description = "Principal not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_principal(
    State(state): State<PrincipalsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<PrincipalResponse>, PlatformError> {
    let principal = state.principal_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("Principal", &id))?;

    // Check access - anchor can see all, others only their client
    if !auth.0.is_anchor() {
        if let Some(ref cid) = principal.client_id {
            if !auth.0.can_access_client(cid) {
                return Err(PlatformError::forbidden("No access to this principal"));
            }
        }
    }

    Ok(Json(principal.into()))
}

/// List principals
#[utoipa::path(
    get,
    path = "",
    tag = "principals",
    operation_id = "getApiAdminPlatformPrincipals",
    params(
        ("page" = Option<u32>, Query, description = "Page number"),
        ("limit" = Option<u32>, Query, description = "Items per page"),
        ("type" = Option<String>, Query, description = "Filter by type"),
        ("scope" = Option<String>, Query, description = "Filter by scope"),
        ("client_id" = Option<String>, Query, description = "Filter by client ID")
    ),
    responses(
        (status = 200, description = "List of principals", body = PrincipalListResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_principals(
    State(state): State<PrincipalsState>,
    auth: Authenticated,
    Query(query): Query<PrincipalsQuery>,
) -> Result<Json<PrincipalListResponse>, PlatformError> {
    let principals = if let Some(ref client_id) = query.client_id {
        if !auth.0.can_access_client(client_id) {
            return Err(PlatformError::forbidden(format!("No access to client: {}", client_id)));
        }
        state.principal_repo.find_by_client(client_id).await?
    } else if let Some(ref scope) = query.scope {
        let s = parse_scope(scope)?;
        state.principal_repo.find_by_scope(s).await?
    } else if query.principal_type.as_deref() == Some("USER") {
        state.principal_repo.find_users().await?
    } else if query.principal_type.as_deref() == Some("SERVICE") {
        state.principal_repo.find_services().await?
    } else {
        state.principal_repo.find_active().await?
    };

    // Filter by access
    let filtered: Vec<PrincipalResponse> = principals.into_iter()
        .filter(|p| {
            if auth.0.is_anchor() {
                return true;
            }
            match &p.client_id {
                Some(cid) => auth.0.can_access_client(cid),
                None => p.scope == UserScope::Anchor && auth.0.is_anchor(),
            }
        })
        .map(|p| p.into())
        .collect();

    let total = filtered.len();
    Ok(Json(PrincipalListResponse { principals: filtered, total }))
}

/// Update principal
#[utoipa::path(
    put,
    path = "/{id}",
    tag = "principals",
    operation_id = "putApiAdminPlatformPrincipalsById",
    params(
        ("id" = String, Path, description = "Principal ID")
    ),
    request_body = UpdatePrincipalRequest,
    responses(
        (status = 200, description = "Principal updated", body = PrincipalResponse),
        (status = 404, description = "Principal not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_principal(
    State(state): State<PrincipalsState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<UpdatePrincipalRequest>,
) -> Result<Json<PrincipalResponse>, PlatformError> {
    let mut principal = state.principal_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("Principal", &id))?;

    // Check access
    if !auth.0.is_anchor() {
        if let Some(ref cid) = principal.client_id {
            if !auth.0.can_access_client(cid) {
                return Err(PlatformError::forbidden("No access to this principal"));
            }
        } else {
            return Err(PlatformError::forbidden("Only anchor users can modify anchor-level principals"));
        }
    }

    // Update fields
    if let Some(name) = req.name {
        principal.name = name;
    }
    if let Some(active) = req.active {
        if active {
            principal.activate();
        } else {
            principal.deactivate();
        }
    }

    // Update user identity if applicable
    if principal.is_user() {
        if let Some(ref mut identity) = principal.user_identity {
            if let Some(first) = req.first_name {
                identity.first_name = Some(first);
            }
            if let Some(last) = req.last_name {
                identity.last_name = Some(last);
            }
        }
    }

    principal.updated_at = chrono::Utc::now();
    state.principal_repo.update(&principal).await?;

    // Audit log
    if let Some(ref audit) = state.audit_service {
        let _ = audit.log_update(&auth.0, "Principal", &id, format!("Updated principal {}", principal.name)).await;
    }

    Ok(Json(principal.into()))
}

/// Get roles assigned to a principal
#[utoipa::path(
    get,
    path = "/{id}/roles",
    tag = "principals",
    operation_id = "getApiAdminPlatformPrincipalsByIdRoles",
    params(
        ("id" = String, Path, description = "Principal ID")
    ),
    responses(
        (status = 200, description = "List of roles", body = RolesListResponse),
        (status = 404, description = "Principal not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_roles(
    State(state): State<PrincipalsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<RolesListResponse>, PlatformError> {
    let principal = state.principal_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("Principal", &id))?;

    // Check access
    if !auth.0.is_anchor() {
        if let Some(ref cid) = principal.client_id {
            if !auth.0.can_access_client(cid) {
                return Err(PlatformError::forbidden("No access to this principal"));
            }
        }
    }

    // Convert role assignments to DTOs
    let roles: Vec<RoleAssignmentDto> = principal.roles.iter()
        .enumerate()
        .map(|(i, r)| RoleAssignmentDto {
            id: format!("{}-role-{}", id, i),
            role_name: r.role.clone(),
            assignment_source: "ADMIN".to_string(), // Default source
            assigned_at: r.assigned_at.to_rfc3339(),
        })
        .collect();

    Ok(Json(RolesListResponse { roles }))
}

/// Assign role to principal
#[utoipa::path(
    post,
    path = "/{id}/roles",
    tag = "principals",
    operation_id = "postApiAdminPlatformPrincipalsByIdRoles",
    params(
        ("id" = String, Path, description = "Principal ID")
    ),
    request_body = AssignRoleRequest,
    responses(
        (status = 200, description = "Role assigned", body = PrincipalResponse),
        (status = 404, description = "Principal not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn assign_role(
    State(state): State<PrincipalsState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<AssignRoleRequest>,
) -> Result<Json<PrincipalResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    let mut principal = state.principal_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("Principal", &id))?;

    let role = req.role.clone();
    let client_id = req.client_id.clone();

    if let Some(cid) = req.client_id {
        principal.assign_role_for_client(req.role, cid);
    } else {
        principal.assign_role(req.role);
    }

    state.principal_repo.update(&principal).await?;

    // Audit log
    if let Some(ref audit) = state.audit_service {
        let _ = audit.log_role_assigned(&auth.0, &id, &role, client_id.as_deref()).await;
    }

    Ok(Json(principal.into()))
}

/// Batch assign roles to principal (declarative - replaces all roles)
#[utoipa::path(
    put,
    path = "/{id}/roles",
    tag = "principals",
    operation_id = "putApiAdminPlatformPrincipalsByIdRoles",
    params(
        ("id" = String, Path, description = "Principal ID")
    ),
    request_body = BatchAssignRolesRequest,
    responses(
        (status = 200, description = "Roles updated", body = BatchAssignRolesResponse),
        (status = 404, description = "Principal not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn batch_assign_roles(
    State(state): State<PrincipalsState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<BatchAssignRolesRequest>,
) -> Result<Json<BatchAssignRolesResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    let mut principal = state.principal_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("Principal", &id))?;

    // Track what was added/removed
    let old_roles: std::collections::HashSet<String> = principal.roles.iter()
        .map(|r| r.role.clone())
        .collect();
    let new_roles: std::collections::HashSet<String> = req.roles.iter().cloned().collect();

    let added: Vec<String> = new_roles.difference(&old_roles).cloned().collect();
    let removed: Vec<String> = old_roles.difference(&new_roles).cloned().collect();

    // Clear existing roles and assign new ones
    principal.roles.clear();
    for role in req.roles {
        principal.assign_role(role);
    }
    principal.updated_at = chrono::Utc::now();

    state.principal_repo.update(&principal).await?;

    // Build response with role DTOs
    let roles: Vec<RoleAssignmentDto> = principal.roles.iter()
        .enumerate()
        .map(|(i, r)| RoleAssignmentDto {
            id: format!("{}-role-{}", id, i),
            role_name: r.role.clone(),
            assignment_source: "ADMIN".to_string(),
            assigned_at: r.assigned_at.to_rfc3339(),
        })
        .collect();

    Ok(Json(BatchAssignRolesResponse {
        roles,
        added,
        removed,
    }))
}

/// Remove role from principal
#[utoipa::path(
    delete,
    path = "/{id}/roles/{role}",
    tag = "principals",
    operation_id = "deleteApiAdminPlatformPrincipalsByIdRolesByRole",
    params(
        ("id" = String, Path, description = "Principal ID"),
        ("role" = String, Path, description = "Role to remove")
    ),
    responses(
        (status = 200, description = "Role removed", body = PrincipalResponse),
        (status = 404, description = "Principal not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn remove_role(
    State(state): State<PrincipalsState>,
    auth: Authenticated,
    Path((id, role)): Path<(String, String)>,
) -> Result<Json<PrincipalResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    let mut principal = state.principal_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("Principal", &id))?;

    principal.roles.retain(|r| r.role != role);
    principal.updated_at = chrono::Utc::now();

    state.principal_repo.update(&principal).await?;

    // Audit log
    if let Some(ref audit) = state.audit_service {
        let _ = audit.log_role_unassigned(&auth.0, &id, &role).await;
    }

    Ok(Json(principal.into()))
}

/// Get client access grants for a principal
#[utoipa::path(
    get,
    path = "/{id}/client-access",
    tag = "principals",
    operation_id = "getApiAdminPlatformPrincipalsByIdClientAccess",
    params(
        ("id" = String, Path, description = "Principal ID")
    ),
    responses(
        (status = 200, description = "Client access grants", body = ClientAccessListResponse),
        (status = 404, description = "Principal not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_client_access(
    State(state): State<PrincipalsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<ClientAccessListResponse>, PlatformError> {
    let principal = state.principal_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("Principal", &id))?;

    // Check access
    if !auth.0.is_anchor() {
        if let Some(ref cid) = principal.client_id {
            if !auth.0.can_access_client(cid) {
                return Err(PlatformError::forbidden("No access to this principal"));
            }
        }
    }

    // Convert assigned_clients to grants (synthesized since we don't store grant metadata)
    let grants: Vec<ClientAccessGrantResponse> = principal.assigned_clients.iter()
        .enumerate()
        .map(|(i, client_id)| ClientAccessGrantResponse {
            id: format!("{}-{}", id, i), // Synthetic ID
            client_id: client_id.clone(),
            granted_at: principal.created_at.to_rfc3339(), // Use principal creation as fallback
            expires_at: None,
        })
        .collect();

    Ok(Json(ClientAccessListResponse { grants }))
}

/// Grant client access to principal
#[utoipa::path(
    post,
    path = "/{id}/client-access",
    tag = "principals",
    operation_id = "postApiAdminPlatformPrincipalsByIdClientAccess",
    params(
        ("id" = String, Path, description = "Principal ID")
    ),
    request_body = GrantClientAccessRequest,
    responses(
        (status = 201, description = "Client access granted", body = ClientAccessGrantResponse),
        (status = 404, description = "Principal not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn grant_client_access(
    State(state): State<PrincipalsState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<GrantClientAccessRequest>,
) -> Result<Json<ClientAccessGrantResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    let mut principal = state.principal_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("Principal", &id))?;

    let client_id = req.client_id.clone();
    let granted_at = chrono::Utc::now();
    principal.grant_client_access(req.client_id);
    state.principal_repo.update(&principal).await?;

    // Audit log
    if let Some(ref audit) = state.audit_service {
        let _ = audit.log_client_access_granted(&auth.0, &id, &client_id).await;
    }

    Ok(Json(ClientAccessGrantResponse {
        id: format!("{}-{}", id, principal.assigned_clients.len() - 1),
        client_id,
        granted_at: granted_at.to_rfc3339(),
        expires_at: None,
    }))
}

/// Revoke client access from principal
#[utoipa::path(
    delete,
    path = "/{id}/client-access/{client_id}",
    tag = "principals",
    operation_id = "deleteApiAdminPlatformPrincipalsByIdClientAccessByClientId",
    params(
        ("id" = String, Path, description = "Principal ID"),
        ("client_id" = String, Path, description = "Client ID to revoke")
    ),
    responses(
        (status = 204, description = "Client access revoked"),
        (status = 404, description = "Principal not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn revoke_client_access(
    State(state): State<PrincipalsState>,
    auth: Authenticated,
    Path((id, client_id)): Path<(String, String)>,
) -> Result<Json<PrincipalResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    let mut principal = state.principal_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("Principal", &id))?;

    principal.revoke_client_access(&client_id);
    state.principal_repo.update(&principal).await?;

    // Audit log
    if let Some(ref audit) = state.audit_service {
        let _ = audit.log_client_access_revoked(&auth.0, &id, &client_id).await;
    }

    Ok(Json(principal.into()))
}

/// Delete principal (deactivate)
#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "principals",
    operation_id = "deleteApiAdminPlatformPrincipalsById",
    params(
        ("id" = String, Path, description = "Principal ID")
    ),
    responses(
        (status = 200, description = "Principal deleted", body = SuccessResponse),
        (status = 404, description = "Principal not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_principal(
    State(state): State<PrincipalsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<SuccessResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    let mut principal = state.principal_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("Principal", &id))?;

    principal.deactivate();
    state.principal_repo.update(&principal).await?;

    // Audit log
    if let Some(ref audit) = state.audit_service {
        let _ = audit.log_archive(&auth.0, "Principal", &id, format!("Deactivated principal {}", principal.name)).await;
    }

    Ok(Json(SuccessResponse::ok()))
}

// ============================================================================
// Status Management Endpoints
// ============================================================================

/// Activate a principal
///
/// Reactivates a deactivated principal.
#[utoipa::path(
    post,
    path = "/{id}/activate",
    tag = "principals",
    operation_id = "postApiAdminPlatformPrincipalsByIdActivate",
    params(
        ("id" = String, Path, description = "Principal ID")
    ),
    responses(
        (status = 200, description = "Principal activated", body = StatusChangeResponse),
        (status = 404, description = "Principal not found"),
        (status = 403, description = "Insufficient permissions")
    ),
    security(("bearer_auth" = []))
)]
pub async fn activate_principal(
    State(state): State<PrincipalsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<StatusChangeResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    let mut principal = state.principal_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("Principal", &id))?;

    principal.activate();
    state.principal_repo.update(&principal).await?;

    tracing::info!(principal_id = %id, admin_id = %auth.0.principal_id, "Principal activated");

    // Audit log
    if let Some(ref audit) = state.audit_service {
        let _ = audit.log_update(&auth.0, "Principal", &id, "Activated principal".to_string()).await;
    }

    Ok(Json(StatusChangeResponse {
        message: "Principal activated".to_string(),
    }))
}

/// Deactivate a principal
///
/// Deactivates an active principal.
#[utoipa::path(
    post,
    path = "/{id}/deactivate",
    tag = "principals",
    operation_id = "postApiAdminPlatformPrincipalsByIdDeactivate",
    params(
        ("id" = String, Path, description = "Principal ID")
    ),
    responses(
        (status = 200, description = "Principal deactivated", body = StatusChangeResponse),
        (status = 404, description = "Principal not found"),
        (status = 403, description = "Insufficient permissions")
    ),
    security(("bearer_auth" = []))
)]
pub async fn deactivate_principal(
    State(state): State<PrincipalsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<StatusChangeResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    let mut principal = state.principal_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("Principal", &id))?;

    principal.deactivate();
    state.principal_repo.update(&principal).await?;

    tracing::info!(principal_id = %id, admin_id = %auth.0.principal_id, "Principal deactivated");

    // Audit log
    if let Some(ref audit) = state.audit_service {
        let _ = audit.log_update(&auth.0, "Principal", &id, "Deactivated principal".to_string()).await;
    }

    Ok(Json(StatusChangeResponse {
        message: "Principal deactivated".to_string(),
    }))
}

/// Reset a user's password
///
/// Resets the password for an internal auth user. Does not work for OIDC users.
#[utoipa::path(
    post,
    path = "/{id}/reset-password",
    tag = "principals",
    operation_id = "postApiAdminPlatformPrincipalsByIdResetPassword",
    params(
        ("id" = String, Path, description = "Principal ID")
    ),
    request_body = ResetPasswordRequest,
    responses(
        (status = 200, description = "Password reset", body = StatusChangeResponse),
        (status = 400, description = "User is not internal auth or invalid password"),
        (status = 404, description = "Principal not found"),
        (status = 403, description = "Insufficient permissions")
    ),
    security(("bearer_auth" = []))
)]
pub async fn reset_password(
    State(state): State<PrincipalsState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<ResetPasswordRequest>,
) -> Result<Json<StatusChangeResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    // Get password service
    let password_service = state.password_service.as_ref()
        .ok_or_else(|| PlatformError::internal("Password service not configured"))?;

    // Validate password
    password_service.validate_password(&req.new_password)?;

    let mut principal = state.principal_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("Principal", &id))?;

    // Check that this is a user with internal auth
    if !principal.is_user() {
        return Err(PlatformError::validation("Password reset only applies to users"));
    }

    // Check for OIDC user (cannot reset password)
    if principal.external_identity.is_some() {
        return Err(PlatformError::validation(
            "Cannot reset password for OIDC-authenticated users"
        ));
    }

    // Hash the new password
    let password_hash = password_service.hash_password(&req.new_password)?;

    // Update the password hash
    if let Some(ref mut identity) = principal.user_identity {
        identity.password_hash = Some(password_hash);
    }

    principal.updated_at = chrono::Utc::now();
    state.principal_repo.update(&principal).await?;

    tracing::info!(principal_id = %id, admin_id = %auth.0.principal_id, "Password reset");

    // Audit log
    if let Some(ref audit) = state.audit_service {
        let _ = audit.log_update(&auth.0, "Principal", &id, "Password reset by admin".to_string()).await;
    }

    Ok(Json(StatusChangeResponse {
        message: "Password reset successfully".to_string(),
    }))
}

/// Check email domain configuration
#[utoipa::path(
    get,
    path = "/check-email-domain",
    tag = "principals",
    operation_id = "getApiAdminPlatformPrincipalsCheckEmailDomain",
    params(
        ("domain" = String, Query, description = "Email domain to check")
    ),
    responses(
        (status = 200, description = "Domain check result", body = CheckEmailDomainResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn check_email_domain(
    State(state): State<PrincipalsState>,
    auth: Authenticated,
    Query(query): Query<CheckEmailDomainQuery>,
) -> Result<Json<CheckEmailDomainResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    // Extract domain from email
    let email = &query.email;
    let domain = email.split('@').nth(1)
        .ok_or_else(|| PlatformError::validation("Invalid email format"))?
        .to_lowercase();

    // Check if email already exists
    let email_exists = state.principal_repo.find_by_email(email).await?.is_some();

    // Check if it's an anchor domain
    let is_anchor_domain = if let Some(ref anchor_repo) = state.anchor_domain_repo {
        anchor_repo.is_anchor_domain(&domain).await?
    } else {
        false
    };

    // Check client auth config
    let (has_auth_config, auth_provider, info, warning) = if is_anchor_domain {
        (true, Some("INTERNAL".to_string()),
         Some("This is an anchor domain. User will have access to all clients.".to_string()),
         None)
    } else if let Some(ref auth_config_repo) = state.client_auth_config_repo {
        if let Some(config) = auth_config_repo.find_by_email_domain(&domain).await? {
            let provider = format!("{:?}", config.auth_provider).to_uppercase();
            let info_msg = if provider == "OIDC" {
                Some("This domain uses external OIDC authentication.".to_string())
            } else {
                Some("This domain uses internal authentication.".to_string())
            };
            (true, Some(provider), info_msg, None)
        } else {
            (false, Some("INTERNAL".to_string()), None,
             Some("No authentication configuration found for this email domain.".to_string()))
        }
    } else {
        (false, Some("INTERNAL".to_string()), None, None)
    };

    // Add warning if email already exists
    let warning = if email_exists {
        Some("A user with this email address already exists.".to_string())
    } else {
        warning
    };

    Ok(Json(CheckEmailDomainResponse {
        domain,
        auth_provider,
        is_anchor_domain,
        has_auth_config,
        email_exists,
        info,
        warning,
    }))
}

/// Create principals router
pub fn principals_router(state: PrincipalsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(create_user, list_principals))
        .routes(routes!(check_email_domain))
        .routes(routes!(get_principal, update_principal, delete_principal))
        .routes(routes!(activate_principal))
        .routes(routes!(deactivate_principal))
        .routes(routes!(reset_password))
        .routes(routes!(get_roles, assign_role, batch_assign_roles))
        .routes(routes!(remove_role))
        .routes(routes!(get_client_access, grant_client_access))
        .routes(routes!(revoke_client_access))
        .with_state(state)
}
