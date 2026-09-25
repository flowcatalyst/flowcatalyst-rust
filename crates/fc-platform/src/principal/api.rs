//! Principals Admin API
//!
//! REST endpoints for principal (user/service account) management.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::application::client_config_repository::ApplicationClientConfigRepository;
use crate::application::entity::Application;
use crate::application::repository::ApplicationRepository;
use crate::identity_provider::entity::IdentityProviderType;
use crate::principal::entity::{Principal, UserIdentity, UserScope};
use crate::principal::repository::PrincipalRepository;
use crate::service_account::entity::RoleAssignment;
use crate::shared::enum_str::parse_opt;
use crate::shared::error::{NotFoundExt, PlatformError};
use crate::shared::middleware::Authenticated;
use crate::AuditService;

/// Create user request
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

    /// The user's client: its `clt_` id or its identifier (e.g. `inhance`),
    /// resolved as Go's `resolveClientRef` does.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,

    /// Requested tier: `ANCHOR`, `PARTNER` or `CLIENT` (the default). The
    /// email domain only confirms a privileged tier, never grants one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,

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

/// Batch assign roles response
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

/// Check email domain response
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
    /// Scope the user will be created with (ANCHOR / PARTNER / CLIENT).
    /// Derived from anchor domains + email_domain_mappings; unmapped domains
    /// default to CLIENT.
    pub derived_scope: String,
    /// True when the create-user form must supply a `clientId`. False for
    /// anchor domains and for mappings that already pin a primary client.
    pub requires_client_id: bool,
    /// When the user must pick a client, this is the allow-list to choose
    /// from. Empty when `requiresClientId` is false (no input needed) OR
    /// when there is no per-domain restriction (any active client is valid —
    /// the UI shows the full list it already fetches).
    pub allowed_client_ids: Vec<String>,
}

/// Set application access request (batch replace)
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetApplicationAccessRequest {
    /// Application IDs to grant access to (replaces existing)
    pub application_ids: Vec<String>,

    /// Access to every application, present and future. Omitted leaves it
    /// unchanged, so a caller that only edits the list never flips it. Only
    /// a caller that itself has all-applications access may set it true
    /// (Go's rule, principal/api/api.go:1204).
    #[serde(default)]
    pub all_applications: Option<bool>,
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
    /// Whether the principal reaches every application; when true the list
    /// is moot.
    pub all_applications: bool,
}

/// Set application access result response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetApplicationAccessResponse {
    pub applications: Vec<ApplicationAccessResponse>,
    pub added: usize,
    pub removed: usize,
    /// The principal's all-applications flag after the change.
    pub all_applications: bool,
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

/// Client access grant response
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

/// Role assignment DTO (for GET /roles)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RoleAssignmentDto {
    pub id: String,
    pub role_name: String,
    pub assignment_source: String,
    pub assigned_at: String,
}

/// The wire label for a role's source. Unsourced assignments read as `ADMIN`,
/// as in the Go port.
fn assignment_source_label(r: &RoleAssignment) -> String {
    r.assignment_source
        .map_or("ADMIN", |s| s.as_str())
        .to_string()
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

/// Principal response DTO
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
    /// Role names
    pub roles: Vec<String>,
    /// Whether user is an anchor domain user
    pub is_anchor_user: bool,
    /// Granted client IDs
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
            principal_type: p.principal_type.as_str().to_string(),
            scope: p.scope.as_str().to_string(),
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

/// Principal list response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PrincipalListResponse {
    pub principals: Vec<PrincipalResponse>,
    pub total: usize,
}

/// Query parameters for principals list.
///
/// Every field is taken as a string, with no `#[serde(flatten)]`: a
/// flattened struct makes serde_urlencoded hand every value over as a
/// string, so a typed `Option<bool>` beside one rejects `?active=true`.
/// Values are then read the way Go reads them
/// (principal/api/api.go:130-140).
#[derive(Debug, Deserialize, Default, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PrincipalsQuery {
    /// 0-based page (only with a page size)
    pub page: Option<String>,

    /// Page size; absent or `<= 0` returns every row, as Go does. `size`,
    /// `limit` and `page_size` are accepted too.
    #[serde(alias = "size", alias = "limit", alias = "page_size")]
    pub page_size: Option<String>,

    /// Filter by type
    #[serde(rename = "type")]
    pub principal_type: Option<String>,

    /// Filter by scope
    pub scope: Option<String>,

    /// Filter by client ID
    pub client_id: Option<String>,

    /// Search by name or email (substring, case-insensitive)
    pub q: Option<String>,

    /// Exact email match (case-insensitive). Use this when looking up a
    /// specific user — `q` is a substring search and can return unrelated rows.
    pub email: Option<String>,

    /// Filter by active status: `true` or `false`; anything else is no filter
    pub active: Option<String>,

    /// Filter by roles (comma-separated)
    pub roles: Option<String>,

    /// Sort field
    pub sort_field: Option<String>,

    /// Sort order (asc/desc)
    pub sort_order: Option<String>,
}

impl PrincipalsQuery {
    /// Go: only the exact strings `true` and `false` filter
    /// (api.go:172-177).
    pub fn active_filter(&self) -> Option<bool> {
        match self.active.as_deref() {
            Some("true") => Some(true),
            Some("false") => Some(false),
            _ => None,
        }
    }

    /// `(page, page_size)`; `None` when every row is wanted (Go
    /// `paginate`, api.go:272-283: a page size `<= 0` returns everything).
    pub fn paging(&self) -> Result<Option<(usize, usize)>, PlatformError> {
        let int = |name: &str, v: Option<&str>| -> Result<i64, PlatformError> {
            match v.map(str::trim).filter(|v| !v.is_empty()) {
                None => Ok(0),
                Some(v) => v
                    .parse::<i64>()
                    .map_err(|_| PlatformError::validation(format!("{name} must be an integer"))),
            }
        };
        let size = int("pageSize", self.page_size.as_deref())?;
        if size <= 0 {
            return Ok(None);
        }
        let page = int("page", self.page.as_deref())?.max(0);
        Ok(Some((page as usize, size as usize)))
    }
}

/// Principals service state
#[derive(Clone)]
pub struct PrincipalsState {
    pub principal_repo: Arc<PrincipalRepository>,
    /// Role definitions, for the role ceiling.
    pub role_repo: Arc<crate::RoleRepository>,
    /// Resolves a user-create `clientId` given as an id or an identifier
    pub client_repo: Arc<crate::ClientRepository>,
    /// Resolved application scopes, cached per principal; dropped when a
    /// principal's application access changes so the change applies at once.
    pub app_access: Arc<crate::shared::authorization_service::ApplicationAccessService>,
    pub audit_service: Arc<AuditService>,
    pub anchor_domain_repo: Arc<crate::AnchorDomainRepository>,
    pub email_domain_mapping_repo: Arc<crate::EmailDomainMappingRepository>,
    pub identity_provider_repo: Arc<crate::IdentityProviderRepository>,
    pub application_repo: Arc<ApplicationRepository>,
    pub app_client_config_repo: Arc<ApplicationClientConfigRepository>,
    /// Backs `POST /api/principals/{id}/send-password-reset`, which emails the
    /// user a single-use reset link (same flow as user-initiated
    /// `/auth/password-reset/request`), and the magic link sent on create.
    pub password_reset_emailer: Arc<crate::auth::password_reset_api::PasswordResetEmailer>,
    // Use cases — writes go through these so that events + audit logs are
    // emitted atomically via UnitOfWork.
    pub create_user_use_case:
        Arc<crate::principal::operations::CreateUserUseCase<crate::usecase::PgUnitOfWork>>,
    pub grant_client_access_use_case:
        Arc<crate::principal::operations::GrantClientAccessUseCase<crate::usecase::PgUnitOfWork>>,
    pub reset_password_use_case:
        Arc<crate::principal::operations::ResetPasswordUseCase<crate::usecase::PgUnitOfWork>>,
    pub activate_use_case:
        Arc<crate::principal::operations::ActivateUserUseCase<crate::usecase::PgUnitOfWork>>,
    pub deactivate_use_case:
        Arc<crate::principal::operations::DeactivateUserUseCase<crate::usecase::PgUnitOfWork>>,
    pub delete_use_case:
        Arc<crate::principal::operations::DeleteUserUseCase<crate::usecase::PgUnitOfWork>>,
    pub update_use_case:
        Arc<crate::principal::operations::UpdateUserUseCase<crate::usecase::PgUnitOfWork>>,
    pub assign_roles_use_case:
        Arc<crate::principal::operations::AssignUserRolesUseCase<crate::usecase::PgUnitOfWork>>,
    pub revoke_client_access_use_case:
        Arc<crate::principal::operations::RevokeClientAccessUseCase<crate::usecase::PgUnitOfWork>>,
    pub assign_app_access_use_case: Arc<
        crate::principal::operations::AssignApplicationAccessUseCase<crate::usecase::PgUnitOfWork>,
    >,
    /// Direct UoW handle used by legacy handlers that mutate the principal in
    /// ways not yet captured by a dedicated use case (e.g. updating
    /// first_name/last_name or toggling client_id). Writes go via repo and
    /// then emit the event/audit through this UoW.
    pub unit_of_work: Arc<crate::usecase::PgUnitOfWork>,
}

/// Create a new user principal
#[utoipa::path(
    post,
    path = "/users",
    tag = "principals",
    operation_id = "postApiPrincipalsUsers",
    request_body = CreateUserRequest,
    responses(
        (status = 201, description = "User created", body = PrincipalResponse),
        (status = 400, description = "Validation error"),
        (status = 409, description = "Duplicate email")
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_user(
    State(state): State<PrincipalsState>,
    auth: Authenticated,
    Json(req): Json<CreateUserRequest>,
) -> Result<Json<PrincipalResponse>, PlatformError> {
    use crate::principal::operations::{CreateUserCommand, GrantClientAccessCommand};
    use crate::usecase::{ExecutionContext, UseCase};

    // Go `createUser`: the user-write permission first; the tier bound once
    // the scope is derived below.
    crate::checks::can_write_principals(&auth.0)?;

    let domain = req
        .email
        .split('@')
        .nth(1)
        .ok_or_else(|| PlatformError::validation("Invalid email format"))?
        .to_lowercase();

    let is_anchor_domain = state.anchor_domain_repo.is_anchor_domain(&domain).await?;

    let mapping = state
        .email_domain_mapping_repo
        .find_by_email_domain(&domain)
        .await?;

    // Resolve IdP type (INTERNAL / OIDC) so the use case can key its password
    // handling off it. Unmapped domains default to INTERNAL — they can only
    // log in through embedded auth anyway.
    let idp_type = match &mapping {
        Some(m) => state
            .identity_provider_repo
            .find_by_id(&m.identity_provider_id)
            .await?
            .map_or(IdentityProviderType::Internal, |idp| idp.r#type),
        None => IdentityProviderType::Internal,
    };

    // Resolve the client reference (clt_ id or identifier) before the tier,
    // so mapping allow-lists compare canonical ids (Go createUser,
    // principal/api/api.go:594-606).
    let req_client_id = match req.client_id.as_deref().map(str::trim) {
        Some(r) if !r.is_empty() => Some(resolve_client_ref(&state, r).await?),
        _ => None,
    };
    let (scope, primary_client_id) = derive_user_scope(
        req.scope.as_deref(),
        is_anchor_domain,
        mapping.as_ref(),
        req_client_id,
    )?;

    // Anchors create any scope and client. A client administrator creates
    // CLIENT-tier users only, in a client it reaches (Go `createUser` +
    // `RequireUserAdmin`, principal/api/api.go:608-616).
    if !auth.0.is_anchor() && scope != UserScope::Client {
        return Err(PlatformError::forbidden(
            "Client administrators can only create client-scope users",
        ));
    }
    crate::checks::require_user_admin(&auth.0, primary_client_id.as_deref())?;

    // Partner-merge: if a user already exists for this email, emit a
    // ClientAccessGranted event via the grant use case rather than a fresh
    // UserCreated. Keeps events + audit logs accurate.
    let granted_client_ids = if scope == UserScope::Partner {
        let client_id = primary_client_id.clone().unwrap_or_default();
        if let Some(existing) = state.principal_repo.find_by_email(&req.email).await? {
            let already_linked = existing.client_id.as_deref() == Some(client_id.as_str())
                || existing.assigned_clients.iter().any(|c| c == &client_id);
            if already_linked {
                return Err(PlatformError::duplicate("Principal", "email", &req.email));
            }
            let cmd = GrantClientAccessCommand {
                user_id: existing.id.clone(),
                client_id: client_id.clone(),
            };
            let ctx = ExecutionContext::create(&auth.0.principal_id);
            state
                .grant_client_access_use_case
                .run(cmd, ctx)
                .await
                .into_result()?;
            let refreshed = state
                .principal_repo
                .find_by_id(&existing.id)
                .await?
                .or_not_found("Principal", &existing.id)?;
            return Ok(Json(refreshed.into()));
        }
        // New partner user — home client + single grant for the requested
        // client.
        vec![client_id]
    } else {
        Vec::new()
    };

    let cmd = CreateUserCommand {
        email: req.email.clone(),
        name: Some(req.name.clone()),
        scope,
        client_id: primary_client_id,
        granted_client_ids,
        password: req.password.clone(),
        enforce_password_complexity: req.enforce_password_complexity,
        idp_type: Some(idp_type),
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    let event = state
        .create_user_use_case
        .run(cmd, ctx)
        .await
        .into_result()?;

    let created = state
        .principal_repo
        .find_by_id(&event.principal_id)
        .await?
        .or_not_found("Principal", &event.principal_id)?;

    // Magic-link bootstrap: when no password is supplied by the caller and
    // the user is INTERNAL-authenticated (we own the password store), send
    // them a one-time set-password email. The admin never sees a password —
    // the backend generates a random hash to satisfy the NOT NULL column,
    // and the recipient picks their own via this link.
    //
    // SDK / automation callers that DO send a password (e.g. bulk migration)
    // skip this step — they've taken responsibility for credential delivery.
    let should_send_magic_link = req.password.is_none()
        && idp_type == IdentityProviderType::Internal
        && created
            .user_identity
            .as_ref()
            .map(|i| !i.email.is_empty())
            .unwrap_or(false);
    if should_send_magic_link {
        if let Err(e) = state
            .password_reset_emailer
            .send_reset_email(&created)
            .await
        {
            // Don't fail the create — the user is in the DB. Surface the
            // problem in logs; the admin can resend from the detail page.
            tracing::error!(
                principal_id = %created.id,
                error = %e,
                "User created but magic-link email failed to send"
            );
        } else {
            tracing::info!(
                principal_id = %created.id,
                "Sent magic sign-in link to new user"
            );
        }
    }

    Ok(Json(created.into()))
}

/// Go's per-resource user-admin gate (`requireUserResourceAccess` /
/// `requireUserAdmin`, principal/operations/authz.go): a non-anchor
/// administrator (a client administrator) acts only on CLIENT-tier users
/// (403 otherwise, Go `blockNonClientTarget`) homed at a client it reaches;
/// a user out of reach answers the same 404 as a missing one. Anchors pass.
fn require_user_resource_access(
    ctx: &crate::AuthContext,
    target: &crate::Principal,
) -> Result<(), PlatformError> {
    if !ctx.is_anchor() && target.scope != UserScope::Client {
        return Err(PlatformError::forbidden(
            "Client administrators can only manage client-scope users",
        ));
    }
    if !crate::shared::caller_reach::reaches_scope(ctx, target.client_id.as_deref()) {
        return Err(PlatformError::not_found("Principal", &target.id));
    }
    Ok(())
}

/// Load a principal a user-administration write targets, gated by
/// [`require_user_resource_access`].
async fn load_administered_user(
    state: &PrincipalsState,
    ctx: &crate::AuthContext,
    id: &str,
) -> Result<crate::Principal, PlatformError> {
    let target = state
        .principal_repo
        .find_by_id(id)
        .await?
        .or_not_found("Principal", id)?;
    require_user_resource_access(ctx, &target)?;
    Ok(target)
}

/// Go `clientAppIDs`: the applications a client is entitled to (an enabled
/// client config), the bound a client administrator is held to.
async fn client_application_ids(
    state: &PrincipalsState,
    client_id: Option<&str>,
) -> Result<std::collections::HashSet<String>, PlatformError> {
    let Some(client_id) = client_id.filter(|c| !c.is_empty()) else {
        return Ok(Default::default());
    };
    Ok(state
        .app_client_config_repo
        .find_enabled_for_client(client_id)
        .await?
        .into_iter()
        .map(|c| c.application_id)
        .collect())
}

/// The role definitions for `names`, by name, in one query.
async fn role_definitions(
    state: &PrincipalsState,
    names: &[String],
) -> Result<std::collections::HashMap<String, crate::AuthRole>, PlatformError> {
    if names.is_empty() {
        return Ok(Default::default());
    }
    Ok(state
        .role_repo
        .find_by_codes(names)
        .await?
        .into_iter()
        .map(|r| (r.name.clone(), r))
        .collect())
}

/// Go `assertAssignableRoles`: a client administrator assigns (or removes)
/// only application roles of applications the target's client is entitled
/// to, never a platform role.
async fn assert_assignable_roles(
    state: &PrincipalsState,
    names: &[String],
    allowed: &std::collections::HashSet<String>,
) -> Result<(), PlatformError> {
    let definitions = role_definitions(state, names).await?;
    for name in names {
        let Some(role) = definitions.get(name) else {
            return Err(PlatformError::Coded {
                status: StatusCode::BAD_REQUEST,
                code: "UNKNOWN_ROLE".to_string(),
                message: format!("role not found: {name}"),
                details: Default::default(),
            });
        };
        match role.application_id.as_deref() {
            None => {
                return Err(PlatformError::forbidden_code(
                    "PLATFORM_ROLE_FORBIDDEN",
                    "client administrators cannot assign platform roles",
                ))
            }
            Some(app) if !allowed.contains(app) => {
                return Err(PlatformError::forbidden_code(
                    "ROLE_APP_FORBIDDEN",
                    "role belongs to an application the client cannot access",
                ))
            }
            Some(_) => {}
        }
    }
    Ok(())
}

/// Go `protectedRoleNames`: the target's roles a client administrator may
/// not manage (platform roles, unknown roles, other applications' roles). A
/// client administrator's role SET keeps them.
async fn protected_role_names(
    state: &PrincipalsState,
    names: &[String],
    allowed: &std::collections::HashSet<String>,
) -> Result<Vec<String>, PlatformError> {
    let definitions = role_definitions(state, names).await?;
    Ok(names
        .iter()
        .filter(|name| {
            definitions
                .get(name.as_str())
                .and_then(|r| r.application_id.as_deref())
                .is_none_or(|app| !allowed.contains(app))
        })
        .cloned()
        .collect())
}

/// The role set a SET writes: exactly `requested` for an anchor; for a
/// client administrator, `requested` (bounded by
/// [`assert_assignable_roles`]) plus the target's protected roles (Go
/// `assignRoles`, principal/api/api.go:1116-1181).
async fn bounded_role_set(
    state: &PrincipalsState,
    ctx: &crate::AuthContext,
    target: &crate::Principal,
    requested: Vec<String>,
) -> Result<Vec<String>, PlatformError> {
    if ctx.is_anchor() {
        return Ok(requested);
    }
    let allowed = client_application_ids(state, target.client_id.as_deref()).await?;
    assert_assignable_roles(state, &requested, &allowed).await?;
    let current: Vec<String> = target.roles.iter().map(|r| r.role.clone()).collect();
    let mut out = requested;
    for kept in protected_role_names(state, &current, &allowed).await? {
        if !out.contains(&kept) {
            out.push(kept);
        }
    }
    Ok(out)
}

/// `POST /api/principals` request: Go's `CreatePrincipalRequest`
/// (principal/api/dto.go:15-28). `email` and `scope` are required, as huma
/// makes them (no `omitempty`).
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreatePrincipalRequest {
    pub email: String,
    #[serde(default)]
    pub name: Option<String>,
    /// Principal scope: `ANCHOR`, `PARTNER` or `CLIENT`
    pub scope: String,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    /// `OIDC` creates a federated user with no password
    #[serde(default)]
    pub idp_type: Option<String>,
    /// Send the new user an invitation (default true)
    #[serde(default)]
    pub send_invitation: Option<bool>,
    /// Accepted for Go compatibility; Rust mints no invite link, so none is
    /// returned
    #[serde(default)]
    pub return_invite_link: Option<bool>,
    /// Where the invitee goes after setting a password (absolute http(s))
    #[serde(default)]
    pub invite_redirect_uri: Option<String>,
}

/// `POST /api/principals` response: Go's `CreatePrincipalResponse`.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreatePrincipalResponse {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invite_link: Option<String>,
}

/// Create a principal (Go `createPrincipal`, principal/api/api.go:365-391).
///
/// The scope and client are taken as given (no email-domain derivation;
/// that is `POST /api/principals/users`). Anchors create any scope; a
/// non-anchor administrator only CLIENT users in a client it can access,
/// and every caller needs a user-write permission (Go `RequireUserAdmin`).
#[utoipa::path(
    post,
    path = "",
    tag = "principals",
    operation_id = "createPrincipal",
    request_body = CreatePrincipalRequest,
    responses(
        (status = 201, description = "Principal created", body = CreatePrincipalResponse),
        (status = 400, description = "Validation error"),
        (status = 403, description = "Forbidden"),
        (status = 409, description = "Email exists")
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_principal(
    State(state): State<PrincipalsState>,
    auth: Authenticated,
    Json(req): Json<CreatePrincipalRequest>,
) -> Result<(StatusCode, Json<CreatePrincipalResponse>), PlatformError> {
    use crate::principal::operations::CreateUserCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    let ctx = &auth.0;
    if !ctx.is_anchor() && req.scope != "CLIENT" {
        return Err(PlatformError::forbidden(
            "Client administrators can only create client-scope users",
        ));
    }
    let client_id = req
        .client_id
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(str::to_string);
    require_user_admin(ctx, client_id.as_deref())?;
    resolve_invite_redirect(req.invite_redirect_uri.as_deref())?;

    // Go's CreateUser validation (principal/operations/create.go:39-65),
    // ahead of the use case so its codes are Go's.
    let email = req.email.trim().to_lowercase();
    if email.is_empty() {
        return Err(PlatformError::bad_request_code(
            "EMAIL_REQUIRED",
            "email is required",
        ));
    }
    if !go_email_pattern().is_match(&email) {
        return Err(PlatformError::bad_request_code(
            "INVALID_EMAIL",
            "email must be a valid address",
        ));
    }
    let scope = match req.scope.as_str() {
        "ANCHOR" => UserScope::Anchor,
        "PARTNER" => UserScope::Partner,
        "CLIENT" => UserScope::Client,
        _ => {
            return Err(PlatformError::bad_request_code(
                "INVALID_SCOPE",
                "scope must be ANCHOR, PARTNER, or CLIENT",
            ))
        }
    };
    if scope != UserScope::Anchor && client_id.is_none() {
        return Err(PlatformError::bad_request_code(
            "CLIENT_REQUIRED",
            "clientId is required for PARTNER or CLIENT scope",
        ));
    }
    if state.principal_repo.find_by_email(&email).await?.is_some() {
        return Err(PlatformError::Coded {
            status: StatusCode::CONFLICT,
            code: "EMAIL_EXISTS".to_string(),
            message: format!("User with email '{email}' already exists"),
            details: Default::default(),
        });
    }

    let idp_type = if req.idp_type.as_deref() == Some("OIDC") {
        IdentityProviderType::Oidc
    } else {
        IdentityProviderType::Internal
    };
    let password = req.password.clone().filter(|p| !p.is_empty());
    let cmd = CreateUserCommand {
        email: email.clone(),
        name: req.name.clone(),
        scope,
        client_id,
        granted_client_ids: Vec::new(),
        password: password.clone(),
        enforce_password_complexity: None,
        idp_type: Some(idp_type),
    };
    let event = state
        .create_user_use_case
        .run(cmd, ExecutionContext::create(&ctx.principal_id))
        .await
        .into_result()?;

    // Go `notifyNewUser` (api.go:704-745): a passwordless internal user is
    // sent an invitation unless the caller suppresses it.
    let send_invitation = req.send_invitation.unwrap_or(true);
    if send_invitation && password.is_none() && idp_type == IdentityProviderType::Internal {
        if let Some(created) = state.principal_repo.find_by_id(&event.principal_id).await? {
            if let Err(e) = state
                .password_reset_emailer
                .send_reset_email(&created)
                .await
            {
                tracing::warn!(principal_id = %created.id, error = %e, "send account invite failed");
            }
        }
    }

    Ok((
        StatusCode::CREATED,
        Json(CreatePrincipalResponse {
            id: event.principal_id,
            invite_link: None,
        }),
    ))
}

/// Go's email check for a created principal (principal/operations/
/// create.go `emailPattern`).
fn go_email_pattern() -> &'static regex::Regex {
    static PATTERN: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    PATTERN.get_or_init(|| {
        regex::Regex::new(r"^[a-zA-Z0-9._%+\-]+@[a-zA-Z0-9.\-]+\.[a-zA-Z]{2,}$")
            .expect("static email pattern")
    })
}

/// Go `RequireUserAdmin` (shared/auth/auth.go:369-385): an anchor needs a
/// user-write permission; anyone else also needs the target client, and
/// may not create a clientless principal.
fn require_user_admin(
    ctx: &crate::AuthContext,
    target_client_id: Option<&str>,
) -> Result<(), PlatformError> {
    if !ctx.is_anchor() {
        let Some(client_id) = target_client_id else {
            return Err(PlatformError::forbidden_code(
                "ANCHOR_REQUIRED",
                "anchor scope required for platform users",
            ));
        };
        if !ctx.can_access_client(client_id) {
            return Err(PlatformError::forbidden_code(
                "SCOPE_FORBIDDEN",
                "no access to this user's client",
            ));
        }
    }
    crate::checks::can_write_principals(ctx)
}

/// Go `resolveInviteRedirect` (principal/api/api.go:756-767): absent or
/// blank is none; otherwise an absolute http(s) URL with a host and no
/// user info.
fn resolve_invite_redirect(raw: Option<&str>) -> Result<Option<String>, PlatformError> {
    let Some(uri) = raw.map(str::trim).filter(|u| !u.is_empty()) else {
        return Ok(None);
    };
    let valid = reqwest::Url::parse(uri).is_ok_and(|u| {
        matches!(u.scheme(), "http" | "https")
            && u.host_str().is_some_and(|h| !h.is_empty())
            && u.username().is_empty()
            && u.password().is_none()
    });
    if !valid {
        return Err(PlatformError::bad_request_code(
            "INVITE_REDIRECT_URI_INVALID",
            "inviteRedirectUri must be an absolute http or https URL",
        ));
    }
    Ok(Some(uri.to_string()))
}

/// A client reference — its `clt_` id or its identifier — as the client's
/// id. Go `resolveClientRef` (principal/api/api.go:825-845): the id first,
/// then the identifier lower-cased; an unknown reference is a 404
/// `Client_NOT_FOUND`, never a silently mis-scoped user.
async fn resolve_client_ref(
    state: &PrincipalsState,
    reference: &str,
) -> Result<String, PlatformError> {
    if let Some(client) = state.client_repo.find_by_id(reference).await? {
        return Ok(client.id);
    }
    if let Some(client) = state
        .client_repo
        .find_by_identifier(&reference.to_lowercase())
        .await?
    {
        return Ok(client.id);
    }
    Err(PlatformError::Coded {
        status: StatusCode::NOT_FOUND,
        code: "Client_NOT_FOUND".to_string(),
        message: format!("Client not found: {reference}"),
        details: Default::default(),
    })
}

/// The new user's tier and home client. Go `deriveUserScope`
/// (principal/api/api.go:780-819): the requested tier wins (CLIENT when
/// absent) and the email domain can only confirm a privileged one:
/// - ANCHOR needs a registered anchor domain or an ANCHOR mapping, and
///   carries no client;
/// - PARTNER needs a PARTNER mapping and a client it allows;
/// - CLIENT takes the requested client, else a CLIENT mapping's primary.
pub fn derive_user_scope(
    requested: Option<&str>,
    is_anchor_domain: bool,
    mapping: Option<&crate::email_domain_mapping::entity::EmailDomainMapping>,
    client_id: Option<String>,
) -> Result<(UserScope, Option<String>), PlatformError> {
    use crate::email_domain_mapping::entity::ScopeType;
    let scope = requested
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_uppercase)
        .unwrap_or_else(|| "CLIENT".to_string());
    match scope.as_str() {
        "ANCHOR" => {
            let anchor_mapped = mapping.is_some_and(|m| m.scope_type == ScopeType::Anchor);
            if !is_anchor_domain && !anchor_mapped {
                return Err(PlatformError::bad_request_code(
                    "ANCHOR_DOMAIN_REQUIRED",
                    "ANCHOR scope requires the email's domain to be a registered anchor domain",
                ));
            }
            Ok((UserScope::Anchor, None))
        }
        "PARTNER" => {
            let Some(m) = mapping.filter(|m| m.scope_type == ScopeType::Partner) else {
                return Err(PlatformError::bad_request_code(
                    "PARTNER_DOMAIN_REQUIRED",
                    "PARTNER scope requires a PARTNER email-domain mapping for the email's domain",
                ));
            };
            let Some(client_id) = client_id.filter(|c| !c.is_empty()) else {
                return Err(PlatformError::bad_request_code(
                    "CLIENT_REQUIRED",
                    "clientId is required for partner users",
                ));
            };
            let allowed = m.primary_client_id.as_deref() == Some(client_id.as_str())
                || m.granted_client_ids.contains(&client_id);
            if !allowed {
                return Err(PlatformError::bad_request_code(
                    "CLIENT_NOT_ALLOWED",
                    format!(
                        "clientId {client_id} is not allowed for partner domain {}",
                        m.email_domain
                    ),
                ));
            }
            Ok((UserScope::Partner, Some(client_id)))
        }
        "CLIENT" => {
            let client_id = client_id.or_else(|| {
                mapping
                    .filter(|m| m.scope_type == ScopeType::Client)
                    .and_then(|m| m.primary_client_id.clone())
            });
            Ok((UserScope::Client, client_id))
        }
        _ => Err(PlatformError::bad_request_code(
            "INVALID_SCOPE",
            "scope must be ANCHOR, PARTNER, or CLIENT",
        )),
    }
}

/// Get principal by ID
#[utoipa::path(
    get,
    path = "/{id}",
    tag = "principals",
    operation_id = "getApiPrincipalsById",
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
    // Go `getByID` (principal/api/api.go:288-323): a principal reads itself
    // with no permission; anyone else needs the user read permission, and a
    // principal of a client the caller does not reach answers the same 404
    // as a missing one.
    let is_self = auth.0.principal_id == id;
    if !is_self {
        crate::checks::can_read_principals(&auth.0)?;
    }
    let principal = state
        .principal_repo
        .find_by_id(&id)
        .await?
        .or_not_found("Principal", &id)?;
    if !is_self {
        if let Some(cid) = principal.client_id.as_deref() {
            if !crate::shared::caller_reach::reaches_client(&auth.0, cid) {
                return Err(PlatformError::not_found("Principal", &id));
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
    operation_id = "getApiPrincipals",
    params(
        ("page" = Option<u32>, Query, description = "Page number"),
        ("limit" = Option<u32>, Query, description = "Items per page"),
        ("type" = Option<String>, Query, description = "Filter by type"),
        ("scope" = Option<String>, Query, description = "Filter by scope"),
        ("client_id" = Option<String>, Query, description = "Filter by client ID"),
        ("email" = Option<String>, Query, description = "Exact email match (case-insensitive)"),
        ("q" = Option<String>, Query, description = "Search by name or email (substring)"),
        ("active" = Option<bool>, Query, description = "Filter by active status"),
        ("roles" = Option<String>, Query, description = "Filter by roles (comma-separated)")
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
    crate::checks::can_read_principals(&auth.0)?;

    // Validate client_id access upfront
    if let Some(ref client_id) = query.client_id {
        if !auth.0.can_access_client(client_id) {
            return Err(PlatformError::forbidden(format!(
                "No access to client: {}",
                client_id
            )));
        }
    }

    // Apply all combinable filters at the DB level
    let principals = state
        .principal_repo
        .find_with_filters(
            query.client_id.as_deref(),
            parse_opt(query.scope.as_deref())?,
            parse_opt(query.principal_type.as_deref())?,
            query.active_filter(),
            query.q.as_deref(),
            query.email.as_deref(),
        )
        .await?;

    // Post-filter: access control + roles (requires hydrated data)
    let mut filtered: Vec<PrincipalResponse> = principals
        .into_iter()
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
        .filter(|p: &PrincipalResponse| match &query.roles {
            Some(roles_str) if !roles_str.trim().is_empty() => {
                // Go splitCSV (api.go:239-247): trimmed, empties dropped.
                let required: Vec<&str> = roles_str
                    .split(',')
                    .map(str::trim)
                    .filter(|r| !r.is_empty())
                    .collect();
                required
                    .iter()
                    .any(|r| p.roles.iter().any(|role| role == r))
            }
            _ => true,
        })
        .collect();

    // Go sortPrincipals (api.go:251-269): a stable ascending sort, reversed
    // for "desc"; name and email compare case-insensitively.
    match query.sort_field.as_deref() {
        Some("name") => filtered.sort_by_key(|p| p.name.to_lowercase()),
        Some("email") => filtered.sort_by_key(|p| p.email.as_deref().unwrap_or("").to_lowercase()),
        _ => filtered.sort_by(|a, b| a.created_at.cmp(&b.created_at)),
    }
    if query
        .sort_order
        .as_deref()
        .is_some_and(|o| o.eq_ignore_ascii_case("desc"))
    {
        filtered.reverse();
    }

    let total = filtered.len();
    let principals: Vec<PrincipalResponse> = match query.paging()? {
        None => filtered,
        Some((page, size)) => filtered
            .into_iter()
            .skip(page.saturating_mul(size))
            .take(size)
            .collect(),
    };
    Ok(Json(PrincipalListResponse { principals, total }))
}

/// Update principal
#[utoipa::path(
    put,
    path = "/{id}",
    tag = "principals",
    operation_id = "putApiPrincipalsById",
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
    use crate::principal::operations::UpdateUserCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    // The permission before anything is loaded, as Go's `update`
    // (principal/api/api.go:1026-1032): without it, nothing is touched.
    crate::checks::can_write_principals(&auth.0)?;

    // Handler-level auth: target-resource access + high-trust gates on
    // scope/client_id changes. Field-level mutations happen inside the
    // use case so the write commits atomically with the UserUpdated event.
    load_administered_user(&state, &auth.0, &id).await?;

    if (req.scope.is_some() || req.client_id.is_some()) && !auth.0.is_anchor() {
        return Err(PlatformError::forbidden(
            "Only anchor users can change a principal's scope or client",
        ));
    }

    let cmd = UpdateUserCommand {
        principal_id: id.clone(),
        name: req.name,
        first_name: req.first_name,
        last_name: req.last_name,
        active: req.active,
        scope: parse_opt(req.scope.as_deref())?,
        client_id: req.client_id,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state.update_use_case.run(cmd, ctx).await.into_result()?;

    let refreshed = state
        .principal_repo
        .find_by_id(&id)
        .await?
        .or_not_found("Principal", &id)?;
    Ok(Json(refreshed.into()))
}

/// Get roles assigned to a principal
#[utoipa::path(
    get,
    path = "/{id}/roles",
    tag = "principals",
    operation_id = "getApiPrincipalsByIdRoles",
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
    crate::checks::can_read_principals(&auth.0)?;

    let principal = state
        .principal_repo
        .find_by_id(&id)
        .await?
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
    let roles: Vec<RoleAssignmentDto> = principal
        .roles
        .iter()
        .enumerate()
        .map(|(i, r)| RoleAssignmentDto {
            id: format!("{}-role-{}", id, i),
            role_name: r.role.clone(),
            assignment_source: assignment_source_label(r),
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
    operation_id = "postApiPrincipalsByIdRoles",
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
    use crate::principal::operations::AssignUserRolesCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::can_assign_principal_roles(&auth.0)?;

    // Additive assign: take existing roles + new role, run through UoW.
    let principal = load_administered_user(&state, &auth.0, &id).await?;
    if !auth.0.is_anchor() {
        let allowed = client_application_ids(&state, principal.client_id.as_deref()).await?;
        assert_assignable_roles(&state, std::slice::from_ref(&req.role), &allowed).await?;
    }
    let before: Vec<String> = principal.roles.iter().map(|r| r.role.clone()).collect();
    let mut roles = before.clone();
    if !roles.iter().any(|r| r == &req.role) {
        roles.push(req.role.clone());
    }
    crate::role::ceiling::require_role_change(&auth.0, &state.role_repo, &before, &roles).await?;

    let cmd = AssignUserRolesCommand {
        user_id: id.clone(),
        roles,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state
        .assign_roles_use_case
        .run(cmd, ctx)
        .await
        .into_result()?;

    let refreshed = state
        .principal_repo
        .find_by_id(&id)
        .await?
        .or_not_found("Principal", &id)?;
    Ok(Json(refreshed.into()))
}

/// Batch assign roles to principal (declarative - replaces all roles)
#[utoipa::path(
    put,
    path = "/{id}/roles",
    tag = "principals",
    operation_id = "putApiPrincipalsByIdRoles",
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
    use crate::principal::operations::AssignUserRolesCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::can_assign_principal_roles(&auth.0)?;

    let principal = load_administered_user(&state, &auth.0, &id).await?;
    let desired = bounded_role_set(&state, &auth.0, &principal, req.roles).await?;

    let before: Vec<String> = principal.roles.iter().map(|r| r.role.clone()).collect();
    crate::role::ceiling::require_role_change(&auth.0, &state.role_repo, &before, &desired).await?;

    let old_roles: std::collections::HashSet<String> =
        principal.roles.iter().map(|r| r.role.clone()).collect();
    let new_roles_set: std::collections::HashSet<String> = desired.iter().cloned().collect();
    let added: Vec<String> = new_roles_set.difference(&old_roles).cloned().collect();
    let removed: Vec<String> = old_roles.difference(&new_roles_set).cloned().collect();

    let cmd = AssignUserRolesCommand {
        user_id: id.clone(),
        roles: desired,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state
        .assign_roles_use_case
        .run(cmd, ctx)
        .await
        .into_result()?;

    let refreshed = state
        .principal_repo
        .find_by_id(&id)
        .await?
        .or_not_found("Principal", &id)?;
    let roles: Vec<RoleAssignmentDto> = refreshed
        .roles
        .iter()
        .enumerate()
        .map(|(i, r)| RoleAssignmentDto {
            id: format!("{}-role-{}", id, i),
            role_name: r.role.clone(),
            assignment_source: assignment_source_label(r),
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
    operation_id = "deleteApiPrincipalsByIdRolesByRoleName",
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
    use crate::principal::operations::AssignUserRolesCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::can_assign_principal_roles(&auth.0)?;

    let principal = load_administered_user(&state, &auth.0, &id).await?;
    // A client administrator removes only roles it could assign (Go
    // `removeRole`).
    if !auth.0.is_anchor() {
        let allowed = client_application_ids(&state, principal.client_id.as_deref()).await?;
        assert_assignable_roles(&state, std::slice::from_ref(&role), &allowed).await?;
    }
    let before: Vec<String> = principal.roles.iter().map(|r| r.role.clone()).collect();
    let roles: Vec<String> = principal
        .roles
        .iter()
        .filter(|r| r.role != role)
        .map(|r| r.role.clone())
        .collect();
    crate::role::ceiling::require_role_change(&auth.0, &state.role_repo, &before, &roles).await?;

    let cmd = AssignUserRolesCommand {
        user_id: id.clone(),
        roles,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state
        .assign_roles_use_case
        .run(cmd, ctx)
        .await
        .into_result()?;

    let refreshed = state
        .principal_repo
        .find_by_id(&id)
        .await?
        .or_not_found("Principal", &id)?;
    Ok(Json(refreshed.into()))
}

/// Get client access grants for a principal
#[utoipa::path(
    get,
    path = "/{id}/client-access",
    tag = "principals",
    operation_id = "getApiPrincipalsByIdClientAccess",
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
    // Go `listClientAccess`: anchor reach alone (principal/api/api.go:1251).
    crate::checks::require_anchor_scope(&auth.0)?;
    let principal = state
        .principal_repo
        .find_by_id(&id)
        .await?
        .or_not_found("Principal", &id)?;

    // Convert assigned_clients to grants (synthesized since we don't store grant metadata)
    let grants: Vec<ClientAccessGrantResponse> = principal
        .assigned_clients
        .iter()
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
    operation_id = "postApiPrincipalsByIdClientAccess",
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
    use crate::principal::operations::GrantClientAccessCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::can_grant_client_access(&auth.0)?;

    let client_id = req.client_id.clone();
    let granted_at = chrono::Utc::now();
    let cmd = GrantClientAccessCommand {
        user_id: id.clone(),
        client_id: client_id.clone(),
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state
        .grant_client_access_use_case
        .run(cmd, ctx)
        .await
        .into_result()?;

    let refreshed = state
        .principal_repo
        .find_by_id(&id)
        .await?
        .or_not_found("Principal", &id)?;
    Ok(Json(ClientAccessGrantResponse {
        id: format!(
            "{}-{}",
            id,
            refreshed.assigned_clients.len().saturating_sub(1)
        ),
        client_id,
        granted_at: granted_at.to_rfc3339(),
        expires_at: None,
    }))
}

/// Revoke client access from principal
#[utoipa::path(
    delete,
    path = "/{id}/client-access/{clientId}",
    tag = "principals",
    operation_id = "deleteApiPrincipalsByIdClientAccessByClientId",
    params(
        ("id" = String, Path, description = "Principal ID"),
        ("clientId" = String, Path, description = "Client ID to revoke")
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
    use crate::principal::operations::RevokeClientAccessCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::can_revoke_client_access(&auth.0)?;

    let cmd = RevokeClientAccessCommand {
        user_id: id.clone(),
        client_id,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state
        .revoke_client_access_use_case
        .run(cmd, ctx)
        .await
        .into_result()?;

    let refreshed = state
        .principal_repo
        .find_by_id(&id)
        .await?
        .or_not_found("Principal", &id)?;
    Ok(Json(refreshed.into()))
}

/// Delete principal (deactivate)
#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "principals",
    operation_id = "deleteApiPrincipalsById",
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
    use crate::principal::operations::DeleteUserCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::can_delete_principals(&auth.0)?;
    load_administered_user(&state, &auth.0, &id).await?;

    let cmd = DeleteUserCommand { principal_id: id };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state.delete_use_case.run(cmd, ctx).await.into_result()?;

    Ok(StatusCode::NO_CONTENT)
}

// ============================================================================
// Status Management Endpoints
// ============================================================================

/// Platform-level user sync request (Go `SyncUsersRequest`,
/// principal/api/sync.go:16-27).
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyncUsersRequest {
    #[serde(default)]
    #[schema(value_type = Vec<Object>)]
    pub principals: Vec<crate::principal::operations::SyncUserInput>,
}

/// Platform-level user sync response (Go sync.go:30-35, 70-75).
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyncUsersResponse {
    pub created: u32,
    pub updated: u32,
    /// Users deactivated by the sync: always 0 (it removes nothing)
    pub deleted: u32,
    pub synced_emails: Vec<String>,
    /// The emails whose `passwordHash` was ignored because the user already
    /// existed (decision #22: a hash is used only to create). Omitted when
    /// empty, as Java's `passwordHashIgnored`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub password_hash_ignored: Vec<String>,
}

/// Sync users (declarative upsert by email; no application scope)
///
/// Go's `POST /api/principals/sync` (principal/api/api.go:94, sync.go:43-76):
/// creates or updates each listed user, carrying a migrated password hash
/// verbatim. Every row, event and audit entry commits in one transaction.
#[utoipa::path(
    post,
    path = "/sync",
    tag = "principals",
    operation_id = "postApiPrincipalsSync",
    request_body = SyncUsersRequest,
    responses(
        (status = 200, description = "Users synced", body = SyncUsersResponse),
        (status = 400, description = "No principals given"),
        (status = 403, description = "Insufficient permissions")
    ),
    security(("bearer_auth" = []))
)]
pub async fn sync_users(
    State(state): State<PrincipalsState>,
    auth: Authenticated,
    Json(req): Json<SyncUsersRequest>,
) -> Result<Json<SyncUsersResponse>, PlatformError> {
    use crate::principal::operations::{SyncUsersCommand, SyncUsersUseCase};
    use crate::usecase::{ExecutionContext, UseCase};

    // Anchor, as every other principal write here: the sync creates and
    // updates users with no client, which only an anchor may manage.
    crate::checks::require_anchor(&auth.0)?;
    crate::checks::can_sync_principals(&auth.0)?;

    let password_hash_ignored = crate::principal::operations::password_hashes_ignored(
        &state.principal_repo,
        req.principals
            .iter()
            .map(|p| (p.email.as_str(), p.password_hash.as_deref())),
    )
    .await?;
    let command = SyncUsersCommand {
        principals: req.principals,
    };
    let ctx = ExecutionContext::from_auth(&auth.0);
    let principal_repo = state.principal_repo.clone();
    let role_repo = state.role_repo.clone();
    let caller = auth.0.clone();
    let event = state
        .unit_of_work
        .run(|session| async move {
            SyncUsersUseCase::new(principal_repo, role_repo, caller, session)
                .run(command, ctx)
                .await
        })
        .await
        .into_result()?;

    Ok(Json(SyncUsersResponse {
        created: event.created,
        updated: event.updated,
        deleted: event.deactivated,
        synced_emails: event.synced_emails,
        password_hash_ignored,
    }))
}

/// Activate a principal
///
/// Reactivates a deactivated principal.
#[utoipa::path(
    post,
    path = "/{id}/activate",
    tag = "principals",
    operation_id = "postApiPrincipalsByIdActivate",
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
    use crate::principal::operations::ActivateUserCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::can_write_principals(&auth.0)?;
    load_administered_user(&state, &auth.0, &id).await?;

    let cmd = ActivateUserCommand {
        principal_id: id.clone(),
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state.activate_use_case.run(cmd, ctx).await.into_result()?;

    tracing::info!(principal_id = %id, admin_id = %auth.0.principal_id, "Principal activated");

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
    operation_id = "postApiPrincipalsByIdDeactivate",
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
    use crate::principal::operations::DeactivateUserCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::can_write_principals(&auth.0)?;
    load_administered_user(&state, &auth.0, &id).await?;

    let cmd = DeactivateUserCommand {
        principal_id: id.clone(),
        reason: Some("Admin deactivated principal".to_string()),
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state
        .deactivate_use_case
        .run(cmd, ctx)
        .await
        .into_result()?;

    tracing::info!(principal_id = %id, admin_id = %auth.0.principal_id, "Principal deactivated");

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
    operation_id = "postApiPrincipalsByIdResetPassword",
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
    use crate::principal::operations::ResetPasswordCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::can_write_principals(&auth.0)?;
    load_administered_user(&state, &auth.0, &id).await?;

    let cmd = ResetPasswordCommand {
        principal_id: id.clone(),
        new_password: req.new_password,
        enforce_password_complexity: req.enforce_password_complexity,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state
        .reset_password_use_case
        .run(cmd, ctx)
        .await
        .into_result()?;

    tracing::info!(principal_id = %id, admin_id = %auth.0.principal_id, "Password reset");

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
    operation_id = "postApiPrincipalsByIdSendPasswordReset",
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
    crate::checks::can_write_principals(&auth.0)?;

    let emailer = &state.password_reset_emailer;

    let principal = load_administered_user(&state, &auth.0, &id).await?;

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
    if principal
        .user_identity
        .as_ref()
        .map(|i| i.email.is_empty())
        .unwrap_or(true)
    {
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

    state
        .audit_service
        .log(
            &auth.0,
            "Principal",
            &id,
            "Password reset email sent by admin",
        )
        .await;

    Ok(Json(StatusChangeResponse {
        message: "Password reset email sent".to_string(),
    }))
}

/// Check email domain configuration
#[utoipa::path(
    get,
    path = "/check-email-domain",
    tag = "principals",
    operation_id = "getApiPrincipalsCheckEmailDomain",
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
    // Go `checkEmailDomain`: the user read permission, no anchor reach (a
    // client administrator's create form calls it).
    crate::checks::can_read_principals(&auth.0)?;

    // Extract domain from email
    let email = &query.email;
    let domain = email
        .split('@')
        .nth(1)
        .ok_or_else(|| PlatformError::validation("Invalid email format"))?
        .to_lowercase();

    // Check if email already exists
    let email_exists = state.principal_repo.find_by_email(email).await?.is_some();

    // Check if it's an anchor domain
    let is_anchor_domain = state.anchor_domain_repo.is_anchor_domain(&domain).await?;

    // Resolve auth provider + scope derivation. The scope/client_id logic
    // here MUST stay in sync with `create_user` above — the form uses
    // `requires_client_id` to decide whether to show a client picker, and the
    // backend would otherwise reject the submission with CLIENT_ID_REQUIRED.
    let mapping = state
        .email_domain_mapping_repo
        .find_by_email_domain(&domain)
        .await?;

    let (has_auth_config, auth_provider, info, warning) = if is_anchor_domain {
        (
            true,
            Some("INTERNAL".to_string()),
            Some("This is an anchor domain. User will have access to all clients.".to_string()),
            None,
        )
    } else if let Some(ref m) = mapping {
        match state
            .identity_provider_repo
            .find_by_id(&m.identity_provider_id)
            .await?
        {
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
            None => (
                false,
                Some("INTERNAL".to_string()),
                Some("Default: user will sign in with an internal password.".to_string()),
                None,
            ),
        }
    } else {
        (
            false,
            Some("INTERNAL".to_string()),
            Some("Default: user will sign in with an internal password.".to_string()),
            None,
        )
    };

    // Derive scope + whether the form needs to pick a client.
    let (derived_scope, requires_client_id, allowed_client_ids) = if is_anchor_domain {
        ("ANCHOR".to_string(), false, Vec::new())
    } else if let Some(ref m) = mapping {
        match m.scope_type {
            crate::email_domain_mapping::entity::ScopeType::Anchor => {
                ("ANCHOR".to_string(), false, Vec::new())
            }
            crate::email_domain_mapping::entity::ScopeType::Partner => {
                // Partner: client must be picked from the granted set
                // (primary_client_id is also valid; see create_user).
                let mut allowed = m.granted_client_ids.clone();
                if let Some(ref p) = m.primary_client_id {
                    if !allowed.iter().any(|c| c == p) {
                        allowed.push(p.clone());
                    }
                }
                ("PARTNER".to_string(), true, allowed)
            }
            crate::email_domain_mapping::entity::ScopeType::Client => {
                // Client mapping with a primary client pins the choice;
                // without one, the form must collect it.
                let requires = m.primary_client_id.is_none();
                ("CLIENT".to_string(), requires, Vec::new())
            }
        }
    } else {
        // Unmapped domain → client-scoped, any active client is acceptable.
        ("CLIENT".to_string(), true, Vec::new())
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
        derived_scope,
        requires_client_id,
        allowed_client_ids,
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
    operation_id = "getApiPrincipalsByIdApplicationAccess",
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
    crate::checks::can_read_principals(&auth.0)?;

    let principal = state
        .principal_repo
        .find_by_id(&id)
        .await?
        .or_not_found("Principal", &id)?;

    // Check access
    if !auth.0.is_anchor() {
        if let Some(ref cid) = principal.client_id {
            if !auth.0.can_access_client(cid) {
                return Err(PlatformError::forbidden("No access to this principal"));
            }
        }
    }

    let app_repo = &state.application_repo;

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
    Ok(Json(ApplicationAccessListResponse {
        applications,
        total,
        all_applications: principal.all_applications,
    }))
}

/// Set application access for a principal (batch replace)
///
/// Replaces all application access with the provided list.
#[utoipa::path(
    put,
    path = "/{id}/application-access",
    tag = "principals",
    operation_id = "putApiPrincipalsByIdApplicationAccess",
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
    use crate::principal::operations::AssignApplicationAccessCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::can_write_principals(&auth.0)?;

    let principal = load_administered_user(&state, &auth.0, &id).await?;

    if req.all_applications == Some(true) {
        // Granting every application exceeds what the caller may itself
        // reach unless it has every application too (Go's rule).
        if state.app_access.scope_for(&auth.0.principal_id).await?
            != crate::shared::authorization_service::ApplicationScope::All
        {
            return Err(PlatformError::forbidden(
                "Only an all-applications administrator may grant all-applications access",
            ));
        }
    }

    // A client administrator grants only applications the target's client is
    // entitled to, and its SET keeps the grants outside that reach (Go
    // `assignApplicationAccess`, principal/api/api.go:1183-1249).
    let mut req = req;
    if !auth.0.is_anchor() {
        let allowed = client_application_ids(&state, principal.client_id.as_deref()).await?;
        if let Some(app_id) = req.application_ids.iter().find(|a| !allowed.contains(*a)) {
            return Err(PlatformError::forbidden_code(
                "APP_FORBIDDEN",
                format!("application the client cannot access: {app_id}"),
            ));
        }
        for kept in principal
            .accessible_application_ids
            .iter()
            .filter(|a| !allowed.contains(*a))
        {
            if !req.application_ids.contains(kept) {
                req.application_ids.push(kept.clone());
            }
        }
    }

    let app_repo = &state.application_repo;

    // Validate applications exist and are active (kept in handler for 400 mapping).
    for app_id in &req.application_ids {
        match app_repo.find_by_id(app_id).await? {
            Some(app) => {
                if !app.active {
                    return Err(PlatformError::validation(format!(
                        "Application is not active: {}",
                        app_id
                    )));
                }
            }
            None => {
                return Err(PlatformError::validation(format!(
                    "Application not found: {}",
                    app_id
                )));
            }
        }
    }

    let old_set: std::collections::HashSet<&str> = principal
        .accessible_application_ids
        .iter()
        .map(|s| s.as_str())
        .collect();
    let new_set: std::collections::HashSet<&str> =
        req.application_ids.iter().map(|s| s.as_str()).collect();
    let added_count = new_set.difference(&old_set).count();
    let removed_count = old_set.difference(&new_set).count();

    let cmd = AssignApplicationAccessCommand {
        user_id: id.clone(),
        application_ids: req.application_ids.clone(),
        all_applications: req.all_applications,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state
        .assign_app_access_use_case
        .run(cmd, ctx)
        .await
        .into_result()?;
    state.app_access.forget(&id);

    let mut applications = Vec::new();
    for app_id in &req.application_ids {
        if let Some(app) = app_repo.find_by_id(app_id).await? {
            applications.push(ApplicationAccessResponse {
                application_id: app.id,
                application_code: app.code,
                application_name: app.name,
            });
        }
    }

    Ok(Json(SetApplicationAccessResponse {
        applications,
        added: added_count,
        removed: removed_count,
        all_applications: req.all_applications.unwrap_or(principal.all_applications),
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
    operation_id = "getApiPrincipalsByIdAvailableApplications",
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
    crate::checks::can_read_principals(&auth.0)?;

    let principal = state
        .principal_repo
        .find_by_id(&id)
        .await?
        .or_not_found("Principal", &id)?;

    // Check access
    if !auth.0.is_anchor() {
        if let Some(ref cid) = principal.client_id {
            if !auth.0.can_access_client(cid) {
                return Err(PlatformError::forbidden("No access to this principal"));
            }
        }
    }

    let app_repo = &state.application_repo;

    let applications: Vec<AvailableApplicationResponse> = if principal.scope == UserScope::Anchor {
        // Anchor users see all active applications
        let apps = app_repo.find_active().await?;
        apps.into_iter()
            .map(AvailableApplicationResponse::from)
            .collect()
    } else {
        // Client users see only apps enabled for their accessible clients
        let config_repo = &state.app_client_config_repo;

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
    Ok(Json(AvailableApplicationsResponse {
        applications,
        total,
    }))
}

/// Create principals router
pub fn principals_router(state: PrincipalsState) -> OpenApiRouter {
    OpenApiRouter::new()
        // `routes!(...)` groups handlers on the SAME path; `create_user` is
        // `/users` and `list_principals` is `""`, so they must be registered
        // separately or only one gets mounted (previously the cause of 405s).
        .routes(routes!(list_principals, create_principal))
        .routes(routes!(create_user))
        .routes(routes!(sync_users))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn mapping(
        scope_type: crate::email_domain_mapping::entity::ScopeType,
        primary: Option<&str>,
        granted: &[&str],
    ) -> crate::email_domain_mapping::entity::EmailDomainMapping {
        let mut m = crate::email_domain_mapping::entity::EmailDomainMapping::new(
            "acme.test",
            "idp_1",
            scope_type,
        );
        m.primary_client_id = primary.map(String::from);
        m.granted_client_ids = granted.iter().map(|g| g.to_string()).collect();
        m
    }

    fn code(r: Result<(UserScope, Option<String>), PlatformError>) -> String {
        match r {
            Err(PlatformError::Coded { code, status, .. }) => {
                assert_eq!(status, StatusCode::BAD_REQUEST);
                code
            }
            other => panic!("expected a coded 400, got {other:?}"),
        }
    }

    /// Go `deriveUserScope` (principal/api/api.go:780-819), case by case.
    #[test]
    fn user_scope_is_derived_as_go_derives_it() {
        use crate::email_domain_mapping::entity::ScopeType;
        let clt = || Some("clt_1".to_string());

        // Absent → CLIENT with the requested client; an anchor domain alone
        // doesn't make an anchor.
        assert_eq!(
            derive_user_scope(None, true, None, clt()).unwrap(),
            (UserScope::Client, clt())
        );
        // CLIENT falls back to a CLIENT mapping's primary.
        let client_map = mapping(ScopeType::Client, Some("clt_9"), &["clt_8"]);
        assert_eq!(
            derive_user_scope(Some("client"), false, Some(&client_map), None).unwrap(),
            (UserScope::Client, Some("clt_9".to_string()))
        );
        // ANCHOR needs the anchor domain (or an ANCHOR mapping); no client.
        assert_eq!(
            derive_user_scope(Some("ANCHOR"), true, None, clt()).unwrap(),
            (UserScope::Anchor, None)
        );
        let anchor_map = mapping(ScopeType::Anchor, None, &[]);
        assert_eq!(
            derive_user_scope(Some(" anchor "), false, Some(&anchor_map), None).unwrap(),
            (UserScope::Anchor, None)
        );
        assert_eq!(
            code(derive_user_scope(Some("ANCHOR"), false, None, None)),
            "ANCHOR_DOMAIN_REQUIRED"
        );
        // PARTNER needs a PARTNER mapping and an allowed client.
        let partner_map = mapping(ScopeType::Partner, Some("clt_1"), &["clt_2"]);
        assert_eq!(
            derive_user_scope(
                Some("PARTNER"),
                false,
                Some(&partner_map),
                Some("clt_2".into())
            )
            .unwrap(),
            (UserScope::Partner, Some("clt_2".to_string()))
        );
        assert_eq!(
            code(derive_user_scope(Some("PARTNER"), false, None, clt())),
            "PARTNER_DOMAIN_REQUIRED"
        );
        assert_eq!(
            code(derive_user_scope(
                Some("PARTNER"),
                false,
                Some(&partner_map),
                None
            )),
            "CLIENT_REQUIRED"
        );
        assert_eq!(
            code(derive_user_scope(
                Some("PARTNER"),
                false,
                Some(&partner_map),
                Some("clt_3".into())
            )),
            "CLIENT_NOT_ALLOWED"
        );
        assert_eq!(
            code(derive_user_scope(Some("ADMIN"), false, None, None)),
            "INVALID_SCOPE"
        );
    }

    /// integral's `CreateUserUseCase.php:110-122` body, through the Laravel
    /// SDK's `CreateUserRequest::toArray()`.
    #[test]
    fn integrals_create_user_body_deserializes() {
        let req: CreateUserRequest = serde_json::from_value(serde_json::json!({
            "email": "a@inhanceapps.com",
            "name": "A",
            "password": "x",
            "enforcePasswordComplexity": false,
            "scope": "ANCHOR"
        }))
        .unwrap();
        assert_eq!(req.scope.as_deref(), Some("ANCHOR"));
        let req: CreateUserRequest = serde_json::from_value(serde_json::json!({
            "email": "b@tenant.test", "name": "B", "password": "x",
            "clientId": "inhance", "enforcePasswordComplexity": false
        }))
        .unwrap();
        assert_eq!(req.client_id.as_deref(), Some("inhance"));
    }

    fn principals_query(uri: &str) -> PrincipalsQuery {
        let uri: axum::http::Uri = uri.parse().unwrap();
        axum::extract::Query::<PrincipalsQuery>::try_from_uri(&uri)
            .unwrap_or_else(|e| panic!("{uri}: {e}"))
            .0
    }

    /// hr (`PrincipalDirectory.php:151`) and rfp
    /// (`PlatformPrincipalDirectory.php:216`) send `?active=true`; hr's role
    /// import (`ImportRolesRegisterCommand.php:275`) adds `type=USER`. No
    /// page size, so every row comes back, as in Go.
    #[test]
    fn the_apps_list_query_parses_through_the_real_query_parser() {
        let q = principals_query("/api/principals?active=true");
        assert_eq!(q.active_filter(), Some(true));
        assert_eq!(q.paging().unwrap(), None);

        let q = principals_query("/api/principals?type=USER&active=true");
        assert_eq!(q.principal_type.as_deref(), Some("USER"));
        assert_eq!(q.active_filter(), Some(true));

        let q = principals_query("/api/principals?active=false");
        assert_eq!(q.active_filter(), Some(false));

        // Go: anything but "true"/"false" is no filter.
        assert_eq!(
            principals_query("/api/principals?active=1").active_filter(),
            None
        );
        assert_eq!(principals_query("/api/principals").active_filter(), None);
    }

    #[test]
    fn a_page_size_pages_and_its_absence_returns_everything() {
        let q = principals_query("/api/principals?page=2&pageSize=10");
        assert_eq!(q.paging().unwrap(), Some((2, 10)));
        // The Rust-era aliases still page.
        assert_eq!(
            principals_query("/api/principals?page=1&size=5")
                .paging()
                .unwrap(),
            Some((1, 5))
        );
        assert_eq!(
            principals_query("/api/principals?limit=7")
                .paging()
                .unwrap(),
            Some((0, 7))
        );
        // Go paginate: a page size <= 0 returns every row.
        assert_eq!(
            principals_query("/api/principals?pageSize=0")
                .paging()
                .unwrap(),
            None
        );
        assert_eq!(
            principals_query("/api/principals?page=3").paging().unwrap(),
            None
        );
        assert!(principals_query("/api/principals?pageSize=ten")
            .paging()
            .is_err());
    }
    use crate::principal::entity::{Principal, PrincipalType, UserIdentity, UserScope};
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
            roles: vec![RoleAssignment::new("platform:admin")],
            assigned_clients: vec!["clt_CLIENT1234567".to_string()],
            client_identifier_map: std::collections::HashMap::new(),
            accessible_application_ids: vec![],
            application_code_map: std::collections::HashMap::new(),
            all_applications: true,
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
            application_code_map: std::collections::HashMap::new(),
            all_applications: false,
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
