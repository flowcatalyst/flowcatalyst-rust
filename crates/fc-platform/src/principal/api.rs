//! Principals Admin API
//!
//! REST endpoints for principal (user/service account) management.

use axum::{
    extract::{State, Path, Query},
    http::StatusCode,
    Json,
};
use utoipa_axum::{router::OpenApiRouter, routes};
use utoipa::ToSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::principal::entity::{Principal, UserScope, UserIdentity};
use crate::service_account::entity::RoleAssignment;
use crate::principal::repository::PrincipalRepository;
use crate::application::entity::Application;
use crate::application::repository::ApplicationRepository;
use crate::application::client_config_repository::ApplicationClientConfigRepository;
use crate::shared::error::{PlatformError, NotFoundExt};
use crate::shared::api_common::{PaginationParams, CreatedResponse};
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

    /// When false, the platform skips its password complexity rules
    /// (uppercase/lowercase/digit/special) and only enforces a 2-character
    /// minimum. Intended for SDK callers that apply their own policy.
    /// Defaults to true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enforce_password_complexity: Option<bool>,
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

    /// User scope (ANCHOR / PARTNER / CLIENT). Changing scope requires anchor.
    pub scope: Option<String>,

    /// Home client ID (required when scope is CLIENT, ignored otherwise).
    /// Changing client requires anchor.
    pub client_id: Option<String>,
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

/// Set application access request (batch replace)
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetApplicationAccessRequest {
    /// Application IDs to grant access to (replaces existing)
    pub application_ids: Vec<String>,
}

/// Application access response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationAccessResponse {
    pub application_id: String,
    pub application_code: String,
    pub application_name: String,
}

/// Application access list response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationAccessListResponse {
    pub applications: Vec<ApplicationAccessResponse>,
    pub total: usize,
}

/// Set application access result response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetApplicationAccessResponse {
    pub applications: Vec<ApplicationAccessResponse>,
    pub added: usize,
    pub removed: usize,
}

/// Available application response (slim DTO)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AvailableApplicationResponse {
    pub id: String,
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "type")]
    pub application_type: String,
    pub active: bool,
}

impl From<Application> for AvailableApplicationResponse {
    fn from(a: Application) -> Self {
        Self {
            id: a.id,
            code: a.code,
            name: a.name,
            description: a.description,
            application_type: a.application_type.as_str().to_string(),
            active: a.active,
        }
    }
}

/// Available applications list response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AvailableApplicationsResponse {
    pub applications: Vec<AvailableApplicationResponse>,
    pub total: usize,
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
    /// New password (min 8 characters)
    pub new_password: String,

    /// When false, the platform skips its password complexity rules
    /// (uppercase/lowercase/digit/special) and only enforces a 2-character
    /// minimum. Intended for SDK callers that apply their own policy.
    /// Defaults to true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enforce_password_complexity: Option<bool>,
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

    /// Search by name or email
    pub q: Option<String>,

    /// Filter by active status
    pub active: Option<bool>,

    /// Filter by roles (comma-separated)
    pub roles: Option<String>,

    /// Sort field
    pub sort_field: Option<String>,

    /// Sort order (asc/desc)
    pub sort_order: Option<String>,
}

/// Principals service state
#[derive(Clone)]
pub struct PrincipalsState {
    pub principal_repo: Arc<PrincipalRepository>,
    pub audit_service: Option<Arc<AuditService>>,
    pub password_service: Option<Arc<PasswordService>>,
    pub anchor_domain_repo: Option<Arc<crate::AnchorDomainRepository>>,
    pub client_auth_config_repo: Option<Arc<crate::ClientAuthConfigRepository>>,
    pub email_domain_mapping_repo: Option<Arc<crate::EmailDomainMappingRepository>>,
    pub identity_provider_repo: Option<Arc<crate::IdentityProviderRepository>>,
    pub application_repo: Option<Arc<ApplicationRepository>>,
    pub app_client_config_repo: Option<Arc<ApplicationClientConfigRepository>>,
    /// When configured, enables `POST /api/admin/principals/{id}/send-password-reset`
    /// which emails the user a single-use reset link (same flow as
    /// user-initiated `/auth/password-reset/request`).
    pub password_reset_emailer: Option<Arc<crate::auth::password_reset_api::PasswordResetEmailer>>,
}


/// Create a new user principal
#[utoipa::path(
    post,
    path = "/users",
    tag = "principals",
    operation_id = "postApiAdminPrincipalsUsers",
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
    crate::checks::require_anchor(&auth.0)?;

    let enforce_complexity = req.enforce_password_complexity.unwrap_or(true);

    let password_hash: Option<String> = match req.password.as_deref() {
        Some(pwd) => {
            let service = state.password_service.as_ref()
                .ok_or_else(|| PlatformError::internal("Password service not configured"))?;
            Some(service.hash_password_with_complexity(pwd, enforce_complexity)?)
        }
        None => None,
    };

    let domain = req.email.split('@').nth(1)
        .ok_or_else(|| PlatformError::validation("Invalid email format"))?
        .to_lowercase();

    let is_anchor_domain = if let Some(ref anchor_repo) = state.anchor_domain_repo {
        anchor_repo.is_anchor_domain(&domain).await?
    } else {
        false
    };

    // Anchor domain: ignore any client_id on the request.
    if is_anchor_domain {
        if state.principal_repo.find_by_email(&req.email).await?.is_some() {
            return Err(PlatformError::duplicate("Principal", "email", &req.email));
        }
        let mut principal = Principal::new_user(&req.email, UserScope::Anchor);
        principal.name = req.name.clone();
        if let Some(hash) = password_hash.clone() {
            if let Some(ref mut identity) = principal.user_identity {
                identity.password_hash = Some(hash);
            }
        }
        let id = principal.id.clone();
        state.principal_repo.insert(&principal).await?;
        if let Some(ref audit) = state.audit_service {
            let _ = audit.log_create(&auth.0, "Principal", &id, format!("Created anchor user {}", req.email)).await;
        }
        return Ok(Json(CreatedResponse::new(id)));
    }

    let mapping = if let Some(ref edm_repo) = state.email_domain_mapping_repo {
        edm_repo.find_by_email_domain(&domain).await?
    } else {
        None
    };

    // Partner domain: validate client_id, merge onto existing user if present.
    if let Some(ref m) = mapping {
        if m.scope_type == crate::email_domain_mapping::entity::ScopeType::Partner {
            let client_id = req.client_id.as_deref()
                .ok_or_else(|| PlatformError::validation("clientId is required for partner users"))?;

            let allowed = m.granted_client_ids.iter().any(|c| c == client_id)
                || m.primary_client_id.as_deref() == Some(client_id);
            if !allowed {
                return Err(PlatformError::validation(format!(
                    "clientId {} is not allowed for partner domain {}", client_id, domain
                )));
            }

            if let Some(existing) = state.principal_repo.find_by_email(&req.email).await? {
                let already_linked = existing.client_id.as_deref() == Some(client_id)
                    || existing.assigned_clients.iter().any(|c| c == client_id);
                if already_linked {
                    return Err(PlatformError::duplicate("Principal", "email", &req.email));
                }
                state.principal_repo.grant_client_access(&existing.id, client_id).await?;
                if let Some(ref audit) = state.audit_service {
                    let _ = audit.log_client_access_granted(&auth.0, &existing.id, client_id).await;
                }
                return Ok(Json(CreatedResponse::new(existing.id)));
            }

            let mut principal = Principal::new_user(&req.email, UserScope::Partner)
                .with_client_id(client_id);
            principal.name = req.name.clone();
            if let Some(hash) = password_hash.clone() {
                if let Some(ref mut identity) = principal.user_identity {
                    identity.password_hash = Some(hash);
                }
            }
            principal.grant_client_access(client_id);
            let id = principal.id.clone();
            state.principal_repo.insert(&principal).await?;
            state.principal_repo.grant_client_access(&id, client_id).await?;
            if let Some(ref audit) = state.audit_service {
                let _ = audit.log_create(&auth.0, "Principal", &id, format!("Created partner user {}", req.email)).await;
            }
            return Ok(Json(CreatedResponse::new(id)));
        }
    }

    // Client-scoped (mapped EDM=CLIENT, or no mapping at all).
    if state.principal_repo.find_by_email(&req.email).await?.is_some() {
        return Err(PlatformError::duplicate("Principal", "email", &req.email));
    }

    let (primary_client_id, granted_client_ids): (Option<String>, Vec<String>) = match mapping {
        Some(m) => {
            let primary = req.client_id.clone().or(m.primary_client_id.clone());
            (primary, m.granted_client_ids.clone())
        }
        None => (req.client_id.clone(), Vec::new()),
    };

    let mut principal = Principal::new_user(&req.email, UserScope::Client);
    principal.name = req.name.clone();
    if let Some(hash) = password_hash {
        if let Some(ref mut identity) = principal.user_identity {
            identity.password_hash = Some(hash);
        }
    }
    if let Some(ref cid) = primary_client_id {
        principal = principal.with_client_id(cid.clone());
    }
    for cid in &granted_client_ids {
        principal.grant_client_access(cid.clone());
    }

    let id = principal.id.clone();
    state.principal_repo.insert(&principal).await?;
    for cid in &granted_client_ids {
        state.principal_repo.grant_client_access(&id, cid).await?;
    }

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
    operation_id = "getApiAdminPrincipalsById",
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
        .or_not_found("Principal", &id)?;

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
    operation_id = "getApiAdminPrincipals",
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
    // Validate client_id access upfront
    if let Some(ref client_id) = query.client_id {
        if !auth.0.can_access_client(client_id) {
            return Err(PlatformError::forbidden(format!("No access to client: {}", client_id)));
        }
    }

    // Apply all combinable filters at the DB level
    let principals = state.principal_repo.find_with_filters(
        query.client_id.as_deref(),
        query.scope.as_deref(),
        query.principal_type.as_deref(),
        query.active,
        query.q.as_deref(),
    ).await?;

    // Post-filter: access control + roles (requires hydrated data)
    let mut filtered: Vec<PrincipalResponse> = principals.into_iter()
        // Access control
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
        // Roles filter (requires checking hydrated roles, stays in-memory)
        .filter(|p: &PrincipalResponse| {
            match &query.roles {
                Some(roles_str) if !roles_str.is_empty() => {
                    let required: Vec<&str> = roles_str.split(',').collect();
                    required.iter().any(|r| p.roles.contains(&r.to_string()))
                }
                _ => true,
            }
        })
        .collect();

    // Sort
    let sort_desc = query.sort_order.as_deref() == Some("desc");
    match query.sort_field.as_deref() {
        Some("name") => filtered.sort_by(|a, b| {
            let cmp = a.name.to_lowercase().cmp(&b.name.to_lowercase());
            if sort_desc { cmp.reverse() } else { cmp }
        }),
        Some("email") => filtered.sort_by(|a, b| {
            let cmp = a.email.cmp(&b.email);
            if sort_desc { cmp.reverse() } else { cmp }
        }),
        _ => filtered.sort_by(|a, b| {
            let cmp = a.created_at.cmp(&b.created_at);
            if sort_desc { cmp.reverse() } else { cmp }
        }),
    }

    let total = filtered.len();
    Ok(Json(PrincipalListResponse { principals: filtered, total }))
}

/// Update principal
#[utoipa::path(
    put,
    path = "/{id}",
    tag = "principals",
    operation_id = "putApiAdminPrincipalsById",
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
        .or_not_found("Principal", &id)?;

    // Check access on the current principal
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

    // Scope / client_id changes are high-trust: require anchor.
    if req.scope.is_some() || req.client_id.is_some() {
        if !auth.0.is_anchor() {
            return Err(PlatformError::forbidden(
                "Only anchor users can change a principal's scope or client",
            ));
        }
    }

    if let Some(scope_str) = req.scope.as_deref() {
        principal.scope = match scope_str.to_uppercase().as_str() {
            "ANCHOR" => crate::principal::entity::UserScope::Anchor,
            "PARTNER" => crate::principal::entity::UserScope::Partner,
            "CLIENT" => crate::principal::entity::UserScope::Client,
            other => return Err(PlatformError::validation(format!(
                "Invalid scope '{}'. Must be ANCHOR, PARTNER, or CLIENT.", other,
            ))),
        };
    }

    // Apply client_id according to the (possibly updated) scope.
    if req.client_id.is_some() || req.scope.is_some() {
        match principal.scope {
            crate::principal::entity::UserScope::Client => {
                let cid = req.client_id.clone()
                    .or_else(|| principal.client_id.clone())
                    .ok_or_else(|| PlatformError::validation(
                        "client_id is required when scope is CLIENT",
                    ))?;
                if cid.trim().is_empty() {
                    return Err(PlatformError::validation(
                        "client_id cannot be empty when scope is CLIENT",
                    ));
                }
                principal.client_id = Some(cid);
            }
            // Anchor / Partner principals don't have a home client.
            _ => {
                principal.client_id = None;
            }
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

    Ok(Json(PrincipalResponse::from(principal)))
}

/// Get roles assigned to a principal
#[utoipa::path(
    get,
    path = "/{id}/roles",
    tag = "principals",
    operation_id = "getApiAdminPrincipalsByIdRoles",
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
        .or_not_found("Principal", &id)?;

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
    operation_id = "postApiAdminPrincipalsByIdRoles",
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
        .or_not_found("Principal", &id)?;

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
    operation_id = "putApiAdminPrincipalsByIdRoles",
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
        .or_not_found("Principal", &id)?;

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
    operation_id = "deleteApiAdminPrincipalsByIdRolesByRoleName",
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
        .or_not_found("Principal", &id)?;

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
    operation_id = "getApiAdminPrincipalsByIdClientAccess",
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
        .or_not_found("Principal", &id)?;

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
    operation_id = "postApiAdminPrincipalsByIdClientAccess",
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
        .or_not_found("Principal", &id)?;

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
    operation_id = "deleteApiAdminPrincipalsByIdClientAccessByClientId",
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
        .or_not_found("Principal", &id)?;

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
    operation_id = "deleteApiAdminPrincipalsById",
    params(
        ("id" = String, Path, description = "Principal ID")
    ),
    responses(
        (status = 204, description = "Principal deleted"),
        (status = 404, description = "Principal not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_principal(
    State(state): State<PrincipalsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<StatusCode, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    let mut principal = state.principal_repo.find_by_id(&id).await?
        .or_not_found("Principal", &id)?;

    principal.deactivate();
    state.principal_repo.update(&principal).await?;

    // Audit log
    if let Some(ref audit) = state.audit_service {
        let _ = audit.log_archive(&auth.0, "Principal", &id, format!("Deactivated principal {}", principal.name)).await;
    }

    Ok(StatusCode::NO_CONTENT)
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
    operation_id = "postApiAdminPrincipalsByIdActivate",
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
        .or_not_found("Principal", &id)?;

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
    operation_id = "postApiAdminPrincipalsByIdDeactivate",
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
        .or_not_found("Principal", &id)?;

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
    operation_id = "postApiAdminPrincipalsByIdResetPassword",
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

    let enforce_complexity = req.enforce_password_complexity.unwrap_or(true);

    // Validate password
    password_service.validate_password_with_complexity(&req.new_password, enforce_complexity)?;

    let mut principal = state.principal_repo.find_by_id(&id).await?
        .or_not_found("Principal", &id)?;

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
    let password_hash = password_service.hash_password_with_complexity(&req.new_password, enforce_complexity)?;

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

/// Trigger a password reset email for an internal-auth user.
///
/// Sends the same single-use email as the user-initiated
/// `/auth/password-reset/request` flow. The user clicks the link and sets
/// their own password; the admin never sees or handles the password.
///
/// Rejects OIDC-federated users (they manage credentials at their IDP) and
/// users without an email address.
#[utoipa::path(
    post,
    path = "/{id}/send-password-reset",
    tag = "principals",
    operation_id = "postApiAdminPrincipalsByIdSendPasswordReset",
    params(
        ("id" = String, Path, description = "Principal ID")
    ),
    responses(
        (status = 200, description = "Reset email queued", body = StatusChangeResponse),
        (status = 400, description = "User is not eligible (OIDC, service account, or no email)"),
        (status = 404, description = "Principal not found"),
        (status = 403, description = "Insufficient permissions")
    ),
    security(("bearer_auth" = []))
)]
pub async fn send_password_reset(
    State(state): State<PrincipalsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<StatusChangeResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    let emailer = state.password_reset_emailer.as_ref()
        .ok_or_else(|| PlatformError::internal("Password reset emailer not configured"))?;

    let principal = state.principal_repo.find_by_id(&id).await?
        .or_not_found("Principal", &id)?;

    if !principal.is_user() {
        return Err(PlatformError::validation(
            "Password reset only applies to user accounts",
        ));
    }
    if principal.external_identity.is_some() {
        return Err(PlatformError::validation(
            "Cannot send password reset for OIDC-federated users — they manage credentials at their IDP",
        ));
    }
    if principal.user_identity.as_ref().map(|i| i.email.is_empty()).unwrap_or(true) {
        return Err(PlatformError::validation(
            "User does not have an email address on file",
        ));
    }

    emailer.send_reset_email(&principal).await?;

    tracing::info!(
        principal_id = %id,
        admin_id = %auth.0.principal_id,
        "Admin triggered password reset email"
    );

    if let Some(ref audit) = state.audit_service {
        let _ = audit.log_update(
            &auth.0,
            "Principal",
            &id,
            "Password reset email sent by admin".to_string(),
        ).await;
    }

    Ok(Json(StatusChangeResponse {
        message: "Password reset email sent".to_string(),
    }))
}

/// Check email domain configuration
#[utoipa::path(
    get,
    path = "/check-email-domain",
    tag = "principals",
    operation_id = "getApiAdminPrincipalsCheckEmailDomain",
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

    // Resolve auth provider: anchor domain → INTERNAL; mapped domain → mapped IdP;
    // otherwise default to INTERNAL (no warning — the internal store is the fallback).
    let (has_auth_config, auth_provider, info, warning) = if is_anchor_domain {
        (true, Some("INTERNAL".to_string()),
         Some("This is an anchor domain. User will have access to all clients.".to_string()),
         None)
    } else if let (Some(ref edm_repo), Some(ref idp_repo)) =
        (&state.email_domain_mapping_repo, &state.identity_provider_repo)
    {
        match edm_repo.find_by_email_domain(&domain).await? {
            Some(mapping) => match idp_repo.find_by_id(&mapping.identity_provider_id).await? {
                Some(idp) => {
                    let provider = match idp.r#type {
                        crate::IdentityProviderType::Oidc => "OIDC",
                        crate::IdentityProviderType::Internal => "INTERNAL",
                    };
                    let info_msg = if provider == "OIDC" {
                        Some("This domain uses external OIDC authentication.".to_string())
                    } else {
                        Some("This domain uses internal authentication.".to_string())
                    };
                    (true, Some(provider.to_string()), info_msg, None)
                }
                None => (false, Some("INTERNAL".to_string()),
                         Some("Default: user will sign in with an internal password.".to_string()),
                         None),
            },
            None => (false, Some("INTERNAL".to_string()),
                     Some("Default: user will sign in with an internal password.".to_string()),
                     None),
        }
    } else {
        (false, Some("INTERNAL".to_string()),
         Some("Default: user will sign in with an internal password.".to_string()),
         None)
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

// ============================================================================
// Application Access Endpoints
// ============================================================================

/// Get application access for a principal
///
/// Returns all applications the principal has been granted access to.
#[utoipa::path(
    get,
    path = "/{id}/application-access",
    tag = "principals",
    operation_id = "getApiAdminPrincipalsByIdApplicationAccess",
    params(
        ("id" = String, Path, description = "Principal ID")
    ),
    responses(
        (status = 200, description = "Application access list", body = ApplicationAccessListResponse),
        (status = 404, description = "Principal not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_application_access(
    State(state): State<PrincipalsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<ApplicationAccessListResponse>, PlatformError> {
    let principal = state.principal_repo.find_by_id(&id).await?
        .or_not_found("Principal", &id)?;

    // Check access
    if !auth.0.is_anchor() {
        if let Some(ref cid) = principal.client_id {
            if !auth.0.can_access_client(cid) {
                return Err(PlatformError::forbidden("No access to this principal"));
            }
        }
    }

    let app_repo = state.application_repo.as_ref()
        .ok_or_else(|| PlatformError::internal("Application repository not configured"))?;

    // Resolve application details for each accessible application ID
    let mut applications = Vec::new();
    for app_id in &principal.accessible_application_ids {
        if let Some(app) = app_repo.find_by_id(app_id).await? {
            applications.push(ApplicationAccessResponse {
                application_id: app.id,
                application_code: app.code,
                application_name: app.name,
            });
        }
    }

    let total = applications.len();
    Ok(Json(ApplicationAccessListResponse { applications, total }))
}

/// Set application access for a principal (batch replace)
///
/// Replaces all application access with the provided list.
#[utoipa::path(
    put,
    path = "/{id}/application-access",
    tag = "principals",
    operation_id = "putApiAdminPrincipalsByIdApplicationAccess",
    params(
        ("id" = String, Path, description = "Principal ID")
    ),
    request_body = SetApplicationAccessRequest,
    responses(
        (status = 200, description = "Application access updated", body = SetApplicationAccessResponse),
        (status = 404, description = "Principal not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn set_application_access(
    State(state): State<PrincipalsState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<SetApplicationAccessRequest>,
) -> Result<Json<SetApplicationAccessResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    let mut principal = state.principal_repo.find_by_id(&id).await?
        .or_not_found("Principal", &id)?;

    let app_repo = state.application_repo.as_ref()
        .ok_or_else(|| PlatformError::internal("Application repository not configured"))?;

    // Validate all requested applications exist and are active
    for app_id in &req.application_ids {
        match app_repo.find_by_id(app_id).await? {
            Some(app) => {
                if !app.active {
                    return Err(PlatformError::validation(format!(
                        "Application is not active: {}", app_id
                    )));
                }
            }
            None => {
                return Err(PlatformError::validation(format!(
                    "Application not found: {}", app_id
                )));
            }
        }
    }

    // Compute delta
    let old_set: std::collections::HashSet<&str> = principal.accessible_application_ids
        .iter().map(|s| s.as_str()).collect();
    let new_set: std::collections::HashSet<&str> = req.application_ids
        .iter().map(|s| s.as_str()).collect();

    let added_count = new_set.difference(&old_set).count();
    let removed_count = old_set.difference(&new_set).count();

    // Update principal
    principal.accessible_application_ids = req.application_ids;
    principal.updated_at = chrono::Utc::now();
    state.principal_repo.update(&principal).await?;

    // Build response with resolved application details
    let mut applications = Vec::new();
    for app_id in &principal.accessible_application_ids {
        if let Some(app) = app_repo.find_by_id(app_id).await? {
            applications.push(ApplicationAccessResponse {
                application_id: app.id,
                application_code: app.code,
                application_name: app.name,
            });
        }
    }

    // Audit log
    if let Some(ref audit) = state.audit_service {
        let _ = audit.log_update(
            &auth.0, "Principal", &id,
            format!("Updated application access: +{} -{}", added_count, removed_count),
        ).await;
    }

    Ok(Json(SetApplicationAccessResponse {
        applications,
        added: added_count,
        removed: removed_count,
    }))
}

/// Get available applications for a principal
///
/// ANCHOR users see all active applications.
/// CLIENT users see only applications enabled for their accessible client configs.
#[utoipa::path(
    get,
    path = "/{id}/available-applications",
    tag = "principals",
    operation_id = "getApiAdminPrincipalsByIdAvailableApplications",
    params(
        ("id" = String, Path, description = "Principal ID")
    ),
    responses(
        (status = 200, description = "Available applications", body = AvailableApplicationsResponse),
        (status = 404, description = "Principal not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_available_applications(
    State(state): State<PrincipalsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<AvailableApplicationsResponse>, PlatformError> {
    let principal = state.principal_repo.find_by_id(&id).await?
        .or_not_found("Principal", &id)?;

    // Check access
    if !auth.0.is_anchor() {
        if let Some(ref cid) = principal.client_id {
            if !auth.0.can_access_client(cid) {
                return Err(PlatformError::forbidden("No access to this principal"));
            }
        }
    }

    let app_repo = state.application_repo.as_ref()
        .ok_or_else(|| PlatformError::internal("Application repository not configured"))?;

    let applications: Vec<AvailableApplicationResponse> = if principal.scope == UserScope::Anchor {
        // Anchor users see all active applications
        let apps = app_repo.find_active().await?;
        apps.into_iter().map(AvailableApplicationResponse::from).collect()
    } else {
        // Client users see only apps enabled for their accessible clients
        let config_repo = state.app_client_config_repo.as_ref()
            .ok_or_else(|| PlatformError::internal("Application client config repository not configured"))?;

        // Gather all client IDs this principal can access
        let mut client_ids: Vec<String> = principal.assigned_clients.clone();
        if let Some(ref home_client) = principal.client_id {
            if !client_ids.contains(home_client) {
                client_ids.push(home_client.clone());
            }
        }

        // Collect unique application IDs from enabled client configs
        let mut app_ids = std::collections::HashSet::new();
        for client_id in &client_ids {
            let configs = config_repo.find_enabled_for_client(client_id).await?;
            for config in configs {
                app_ids.insert(config.application_id);
            }
        }

        // Resolve application details
        let mut apps = Vec::new();
        for app_id in app_ids {
            if let Some(app) = app_repo.find_by_id(&app_id).await? {
                if app.active {
                    apps.push(AvailableApplicationResponse::from(app));
                }
            }
        }
        apps
    };

    let total = applications.len();
    Ok(Json(AvailableApplicationsResponse { applications, total }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::principal::entity::{Principal, PrincipalType, UserScope, UserIdentity};
    use crate::service_account::entity::RoleAssignment;
    use chrono::Utc;

    fn make_test_principal() -> Principal {
        let now = Utc::now();
        Principal {
            id: "prn_ABCDEFGHIJKLM".to_string(),
            principal_type: PrincipalType::User,
            scope: UserScope::Anchor,
            client_id: None,
            application_id: None,
            name: "Jane Admin".to_string(),
            active: true,
            user_identity: Some(UserIdentity::new("jane@example.com")),
            service_account_id: None,
            roles: vec![
                RoleAssignment::new("platform:admin"),
            ],
            assigned_clients: vec!["clt_CLIENT1234567".to_string()],
            client_identifier_map: std::collections::HashMap::new(),
            accessible_application_ids: vec![],
            created_at: now,
            updated_at: now,
            external_identity: None,
        }
    }

    // --- PrincipalResponse serialization ---

    #[test]
    fn test_principal_response_serialization() {
        let principal = make_test_principal();
        let response = PrincipalResponse::from(principal);

        let json = serde_json::to_value(&response).unwrap();

        assert_eq!(json["id"], "prn_ABCDEFGHIJKLM");
        assert_eq!(json["type"], "USER");
        assert_eq!(json["scope"], "ANCHOR");
        assert_eq!(json["name"], "Jane Admin");
        assert_eq!(json["active"], true);
        assert_eq!(json["email"], "jane@example.com");
        assert_eq!(json["idpType"], "INTERNAL");
        assert_eq!(json["isAnchorUser"], true);
        assert!(json["roles"].is_array());
        assert_eq!(json["roles"][0], "platform:admin");
        assert!(json["grantedClientIds"].is_array());
        assert_eq!(json["grantedClientIds"][0], "clt_CLIENT1234567");
        // Verify camelCase field names
        assert!(json.get("createdAt").is_some());
        assert!(json.get("updatedAt").is_some());
        // Verify no snake_case leak
        assert!(json.get("principal_type").is_none());
        assert!(json.get("client_id").is_none());
        assert!(json.get("is_anchor_user").is_none());
        assert!(json.get("granted_client_ids").is_none());
    }

    #[test]
    fn test_principal_response_without_user_identity() {
        let now = Utc::now();
        let principal = Principal {
            id: "prn_SERVICEID12345".to_string(),
            principal_type: PrincipalType::Service,
            scope: UserScope::Client,
            client_id: Some("clt_CLIENT1234567".to_string()),
            application_id: None,
            name: "My Service Account".to_string(),
            active: true,
            user_identity: None,
            service_account_id: None,
            roles: vec![],
            assigned_clients: vec![],
            client_identifier_map: std::collections::HashMap::new(),
            accessible_application_ids: vec![],
            created_at: now,
            updated_at: now,
            external_identity: None,
        };

        let response = PrincipalResponse::from(principal);
        let json = serde_json::to_value(&response).unwrap();

        assert_eq!(json["type"], "SERVICE");
        assert_eq!(json["scope"], "CLIENT");
        assert!(json["email"].is_null());
        assert!(json["idpType"].is_null());
        assert_eq!(json["isAnchorUser"], false);
        assert_eq!(json["clientId"], "clt_CLIENT1234567");
    }

    // --- CreateUserRequest deserialization ---

    #[test]
    fn test_create_user_request_deserialization() {
        let json = serde_json::json!({
            "email": "user@example.com",
            "name": "Test User",
            "password": "secret123456"
        });

        let req: CreateUserRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.email, "user@example.com");
        assert_eq!(req.name, "Test User");
        assert_eq!(req.password, Some("secret123456".to_string()));
        assert!(req.client_id.is_none());
    }

    #[test]
    fn test_create_user_request_with_client_id() {
        let json = serde_json::json!({
            "email": "user@example.com",
            "name": "Client User",
            "clientId": "clt_ABCDEFGHIJKLM"
        });

        let req: CreateUserRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.client_id, Some("clt_ABCDEFGHIJKLM".to_string()));
        assert!(req.password.is_none());
    }

    #[test]
    fn test_create_user_request_missing_email() {
        let json = serde_json::json!({
            "name": "Test User"
        });

        let result = serde_json::from_value::<CreateUserRequest>(json);
        assert!(result.is_err(), "Should fail without email");
    }

    #[test]
    fn test_create_user_request_missing_name() {
        let json = serde_json::json!({
            "email": "user@example.com"
        });

        let result = serde_json::from_value::<CreateUserRequest>(json);
        assert!(result.is_err(), "Should fail without name");
    }

    // --- UserIdentityResponse ---

    #[test]
    fn test_user_identity_response_serialization() {
        let identity = UserIdentity::new("user@example.com");
        let response = UserIdentityResponse::from(&identity);
        let json = serde_json::to_value(&response).unwrap();

        assert_eq!(json["email"], "user@example.com");
        assert_eq!(json["emailVerified"], false);
        assert!(json["firstName"].is_null());
        assert!(json["lastName"].is_null());
    }

    // --- AssignRoleRequest ---

    #[test]
    fn test_assign_role_request_deserialization() {
        let json = serde_json::json!({
            "role": "platform:admin",
            "clientId": "clt_123"
        });

        let req: AssignRoleRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.role, "platform:admin");
        assert_eq!(req.client_id, Some("clt_123".to_string()));
    }

    #[test]
    fn test_assign_role_request_missing_role() {
        let json = serde_json::json!({});
        let result = serde_json::from_value::<AssignRoleRequest>(json);
        assert!(result.is_err(), "Should fail without role");
    }
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
        .routes(routes!(send_password_reset))
        .routes(routes!(get_roles, assign_role, batch_assign_roles))
        .routes(routes!(remove_role))
        .routes(routes!(get_client_access, grant_client_access))
        .routes(routes!(revoke_client_access))
        .routes(routes!(get_application_access, set_application_access))
        .routes(routes!(get_available_applications))
        .with_state(state)
}
