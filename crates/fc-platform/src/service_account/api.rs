//! Service Accounts Admin API
//!
//! REST endpoints for service account management.
//! Base path: /api/admin/platform/service-accounts

use axum::{
    routing::{get, post},
    extract::{State, Path, Query},
    http::StatusCode,
    response::IntoResponse,
    Json, Router,
};
use utoipa::ToSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::ServiceAccount;
use crate::ServiceAccountRepository;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;
use crate::usecase::{ExecutionContext, UnitOfWork, UseCaseResult};
use crate::service_account::operations::{
    CreateServiceAccountCommand, CreateServiceAccountUseCase,
    UpdateServiceAccountCommand, UpdateServiceAccountUseCase,
    DeleteServiceAccountCommand, DeleteServiceAccountUseCase,
    AssignRolesCommand, AssignRolesUseCase,
    RegenerateAuthTokenCommand, RegenerateAuthTokenUseCase,
    RegenerateSigningSecretCommand, RegenerateSigningSecretUseCase,
};

// ============================================================================
// Request/Response DTOs
// ============================================================================

/// Create service account request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateServiceAccountRequest {
    /// Unique code (1-50 chars)
    pub code: String,

    /// Human-readable name (1-100 chars)
    pub name: String,

    /// Optional description (max 500 chars)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Client IDs this account can access
    #[serde(default)]
    pub client_ids: Vec<String>,

    /// Application ID (if created for an application)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application_id: Option<String>,
}

/// Update service account request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateServiceAccountRequest {
    /// Updated name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Updated description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Updated client IDs
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_ids: Option<Vec<String>>,
}

/// Assign roles request (declarative - replaces all)
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AssignRolesRequest {
    /// Role names to assign
    pub roles: Vec<String>,
}

/// Query parameters for service accounts list
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccountsQuery {
    /// Filter by client ID
    pub client_id: Option<String>,

    /// Filter by application ID
    pub application_id: Option<String>,

    /// Filter by active status
    pub active: Option<bool>,
}

/// Service account list response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccountListResponse {
    pub service_accounts: Vec<ServiceAccountResponse>,
    pub total: usize,
}

/// Service account response DTO
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccountResponse {
    pub id: String,
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub client_ids: Vec<String>,
    pub application_id: Option<String>,
    pub active: bool,
    pub auth_type: String,
    pub roles: Vec<String>,
    pub last_used_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<ServiceAccount> for ServiceAccountResponse {
    fn from(sa: ServiceAccount) -> Self {
        Self {
            id: sa.id,
            code: sa.code,
            name: sa.name,
            description: sa.description,
            client_ids: sa.client_ids,
            application_id: sa.application_id,
            active: sa.active,
            auth_type: format!("{:?}", sa.webhook_credentials.auth_type).to_uppercase(),
            roles: sa.roles.iter().map(|r| r.role.clone()).collect(),
            last_used_at: sa.last_used_at.map(|t| t.to_rfc3339()),
            created_at: sa.created_at.to_rfc3339(),
            updated_at: sa.updated_at.to_rfc3339(),
        }
    }
}

/// Create service account response (includes one-time secrets)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateServiceAccountResponse {
    pub service_account: ServiceAccountResponse,
    /// Auth token (shown only once)
    pub auth_token: String,
    /// Signing secret (shown only once)
    pub signing_secret: String,
}

/// Regenerate token response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RegenerateTokenResponse {
    /// New auth token (shown only once)
    pub auth_token: String,
}

/// Regenerate secret response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RegenerateSecretResponse {
    /// New signing secret (shown only once)
    pub signing_secret: String,
}

/// Role assignment response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RoleAssignmentResponse {
    pub role_name: String,
    pub assignment_source: Option<String>,
    pub assigned_at: String,
}

/// Roles response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RolesResponse {
    pub roles: Vec<RoleAssignmentResponse>,
}

/// Assign roles response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AssignRolesResponse {
    pub roles: Vec<RoleAssignmentResponse>,
    pub added_roles: Vec<String>,
    pub removed_roles: Vec<String>,
}

// ============================================================================
// State
// ============================================================================

/// Service accounts API state with use cases
#[derive(Clone)]
pub struct ServiceAccountsState<U: UnitOfWork + 'static> {
    pub repo: Arc<ServiceAccountRepository>,
    pub create_use_case: Arc<CreateServiceAccountUseCase<U>>,
    pub update_use_case: Arc<UpdateServiceAccountUseCase<U>>,
    pub delete_use_case: Arc<DeleteServiceAccountUseCase<U>>,
    pub assign_roles_use_case: Arc<AssignRolesUseCase<U>>,
    pub regenerate_token_use_case: Arc<RegenerateAuthTokenUseCase<U>>,
    pub regenerate_secret_use_case: Arc<RegenerateSigningSecretUseCase<U>>,
}

// ============================================================================
// Endpoints
// ============================================================================

/// List service accounts
#[utoipa::path(
    get,
    path = "",
    tag = "service-accounts",
    params(
        ("clientId" = Option<String>, Query, description = "Filter by client ID"),
        ("applicationId" = Option<String>, Query, description = "Filter by application ID"),
        ("active" = Option<bool>, Query, description = "Filter by active status")
    ),
    responses(
        (status = 200, description = "List of service accounts", body = ServiceAccountListResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_service_accounts<U: UnitOfWork>(
    State(state): State<ServiceAccountsState<U>>,
    _auth: Authenticated,
    Query(query): Query<ServiceAccountsQuery>,
) -> Result<Json<ServiceAccountListResponse>, PlatformError> {
    let mut accounts = if let Some(client_id) = query.client_id {
        state.repo.find_by_client(&client_id).await?
    } else if let Some(app_id) = query.application_id {
        state.repo.find_by_application(&app_id).await?
    } else if query.active == Some(true) {
        state.repo.find_active().await?
    } else {
        state.repo.find_active().await?
    };

    if let Some(is_active) = query.active {
        accounts.retain(|a| a.active == is_active);
    }

    let total = accounts.len();
    let service_accounts: Vec<ServiceAccountResponse> = accounts.into_iter()
        .map(ServiceAccountResponse::from)
        .collect();

    Ok(Json(ServiceAccountListResponse {
        service_accounts,
        total,
    }))
}

/// Get service account by ID
#[utoipa::path(
    get,
    path = "/{id}",
    tag = "service-accounts",
    params(
        ("id" = String, Path, description = "Service account ID")
    ),
    responses(
        (status = 200, description = "Service account found", body = ServiceAccountResponse),
        (status = 404, description = "Service account not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_service_account<U: UnitOfWork>(
    State(state): State<ServiceAccountsState<U>>,
    _auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<ServiceAccountResponse>, PlatformError> {
    let account = state.repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::ServiceAccountNotFound { id: id.clone() })?;

    Ok(Json(ServiceAccountResponse::from(account)))
}

/// Get service account by code
#[utoipa::path(
    get,
    path = "/code/{code}",
    tag = "service-accounts",
    params(
        ("code" = String, Path, description = "Service account code")
    ),
    responses(
        (status = 200, description = "Service account found", body = ServiceAccountResponse),
        (status = 404, description = "Service account not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_service_account_by_code<U: UnitOfWork>(
    State(state): State<ServiceAccountsState<U>>,
    _auth: Authenticated,
    Path(code): Path<String>,
) -> Result<Json<ServiceAccountResponse>, PlatformError> {
    let account = state.repo.find_by_code(&code).await?
        .ok_or_else(|| PlatformError::ServiceAccountNotFound { id: code.clone() })?;

    Ok(Json(ServiceAccountResponse::from(account)))
}

/// Create service account
#[utoipa::path(
    post,
    path = "",
    tag = "service-accounts",
    request_body = CreateServiceAccountRequest,
    responses(
        (status = 201, description = "Service account created", body = CreateServiceAccountResponse),
        (status = 400, description = "Validation error"),
        (status = 409, description = "Duplicate code")
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_service_account<U: UnitOfWork>(
    State(state): State<ServiceAccountsState<U>>,
    auth: Authenticated,
    Json(req): Json<CreateServiceAccountRequest>,
) -> Result<Json<CreateServiceAccountResponse>, PlatformError> {
    let command = CreateServiceAccountCommand {
        code: req.code,
        name: req.name,
        description: req.description,
        client_ids: req.client_ids,
        application_id: req.application_id,
    };

    let ctx = ExecutionContext::create(auth.0.principal_id.clone());

    match state.create_use_case.execute(command, ctx).await {
        UseCaseResult::Success(result) => {
            // Fetch the created service account to return
            let account = state.repo.find_by_id(&result.event.service_account_id).await?
                .ok_or_else(|| PlatformError::internal("Created service account not found"))?;

            Ok(Json(CreateServiceAccountResponse {
                service_account: ServiceAccountResponse::from(account),
                auth_token: result.auth_token,
                signing_secret: result.signing_secret,
            }))
        }
        UseCaseResult::Failure(err) => Err(err.into()),
    }
}

/// Update service account
#[utoipa::path(
    put,
    path = "/{id}",
    tag = "service-accounts",
    params(
        ("id" = String, Path, description = "Service account ID")
    ),
    request_body = UpdateServiceAccountRequest,
    responses(
        (status = 200, description = "Service account updated", body = ServiceAccountResponse),
        (status = 404, description = "Service account not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_service_account<U: UnitOfWork>(
    State(state): State<ServiceAccountsState<U>>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<UpdateServiceAccountRequest>,
) -> Result<Json<ServiceAccountResponse>, PlatformError> {
    let command = UpdateServiceAccountCommand {
        id: id.clone(),
        name: req.name,
        description: req.description,
        client_ids: req.client_ids,
    };

    let ctx = ExecutionContext::create(auth.0.principal_id.clone());

    match state.update_use_case.execute(command, ctx).await {
        UseCaseResult::Success(event) => {
            let account = state.repo.find_by_id(&event.service_account_id).await?
                .ok_or_else(|| PlatformError::ServiceAccountNotFound { id })?;

            Ok(Json(ServiceAccountResponse::from(account)))
        }
        UseCaseResult::Failure(err) => Err(err.into()),
    }
}

/// Delete service account
#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "service-accounts",
    params(
        ("id" = String, Path, description = "Service account ID")
    ),
    responses(
        (status = 204, description = "Service account deleted"),
        (status = 404, description = "Service account not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_service_account<U: UnitOfWork>(
    State(state): State<ServiceAccountsState<U>>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, PlatformError> {
    let command = DeleteServiceAccountCommand { id };

    let ctx = ExecutionContext::create(auth.0.principal_id.clone());

    match state.delete_use_case.execute(command, ctx).await {
        UseCaseResult::Success(_) => Ok(StatusCode::NO_CONTENT),
        UseCaseResult::Failure(err) => Err(err.into()),
    }
}

/// Regenerate auth token
#[utoipa::path(
    post,
    path = "/{id}/regenerate-token",
    tag = "service-accounts",
    params(
        ("id" = String, Path, description = "Service account ID")
    ),
    responses(
        (status = 200, description = "Token regenerated", body = RegenerateTokenResponse),
        (status = 404, description = "Service account not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn regenerate_auth_token<U: UnitOfWork>(
    State(state): State<ServiceAccountsState<U>>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<RegenerateTokenResponse>, PlatformError> {
    let command = RegenerateAuthTokenCommand { service_account_id: id };

    let ctx = ExecutionContext::create(auth.0.principal_id.clone());

    match state.regenerate_token_use_case.execute(command, ctx).await {
        UseCaseResult::Success(result) => {
            Ok(Json(RegenerateTokenResponse {
                auth_token: result.auth_token,
            }))
        }
        UseCaseResult::Failure(err) => Err(err.into()),
    }
}

/// Regenerate signing secret
#[utoipa::path(
    post,
    path = "/{id}/regenerate-secret",
    tag = "service-accounts",
    params(
        ("id" = String, Path, description = "Service account ID")
    ),
    responses(
        (status = 200, description = "Secret regenerated", body = RegenerateSecretResponse),
        (status = 404, description = "Service account not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn regenerate_signing_secret<U: UnitOfWork>(
    State(state): State<ServiceAccountsState<U>>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<RegenerateSecretResponse>, PlatformError> {
    let command = RegenerateSigningSecretCommand { service_account_id: id };

    let ctx = ExecutionContext::create(auth.0.principal_id.clone());

    match state.regenerate_secret_use_case.execute(command, ctx).await {
        UseCaseResult::Success(result) => {
            Ok(Json(RegenerateSecretResponse {
                signing_secret: result.signing_secret,
            }))
        }
        UseCaseResult::Failure(err) => Err(err.into()),
    }
}

/// Get assigned roles
#[utoipa::path(
    get,
    path = "/{id}/roles",
    tag = "service-accounts",
    params(
        ("id" = String, Path, description = "Service account ID")
    ),
    responses(
        (status = 200, description = "Roles retrieved", body = RolesResponse),
        (status = 404, description = "Service account not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_roles<U: UnitOfWork>(
    State(state): State<ServiceAccountsState<U>>,
    _auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<RolesResponse>, PlatformError> {
    let account = state.repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::ServiceAccountNotFound { id: id.clone() })?;

    let roles: Vec<RoleAssignmentResponse> = account.roles.iter()
        .map(|r| RoleAssignmentResponse {
            role_name: r.role.clone(),
            assignment_source: r.assignment_source.clone(),
            assigned_at: r.assigned_at.to_rfc3339(),
        })
        .collect();

    Ok(Json(RolesResponse { roles }))
}

/// Assign roles (declarative - replaces all)
#[utoipa::path(
    put,
    path = "/{id}/roles",
    tag = "service-accounts",
    params(
        ("id" = String, Path, description = "Service account ID")
    ),
    request_body = AssignRolesRequest,
    responses(
        (status = 200, description = "Roles assigned", body = AssignRolesResponse),
        (status = 404, description = "Service account not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn assign_roles<U: UnitOfWork>(
    State(state): State<ServiceAccountsState<U>>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<AssignRolesRequest>,
) -> Result<Json<AssignRolesResponse>, PlatformError> {
    let command = AssignRolesCommand {
        service_account_id: id.clone(),
        roles: req.roles,
    };

    let ctx = ExecutionContext::create(auth.0.principal_id.clone());

    match state.assign_roles_use_case.execute(command, ctx).await {
        UseCaseResult::Success(event) => {
            // Fetch updated account to get role details
            let account = state.repo.find_by_id(&id).await?
                .ok_or_else(|| PlatformError::ServiceAccountNotFound { id })?;

            let roles: Vec<RoleAssignmentResponse> = account.roles.iter()
                .map(|r| RoleAssignmentResponse {
                    role_name: r.role.clone(),
                    assignment_source: r.assignment_source.clone(),
                    assigned_at: r.assigned_at.to_rfc3339(),
                })
                .collect();

            Ok(Json(AssignRolesResponse {
                roles,
                added_roles: event.roles_added,
                removed_roles: event.roles_removed,
            }))
        }
        UseCaseResult::Failure(err) => Err(err.into()),
    }
}

// ============================================================================
// Router
// ============================================================================

/// Create the service accounts router
pub fn service_accounts_router<U: UnitOfWork + Clone>(state: ServiceAccountsState<U>) -> Router {
    Router::new()
        .route("/", get(list_service_accounts::<U>).post(create_service_account::<U>))
        .route("/:id", get(get_service_account::<U>).put(update_service_account::<U>).delete(delete_service_account::<U>))
        .route("/code/:code", get(get_service_account_by_code::<U>))
        .route("/:id/regenerate-token", post(regenerate_auth_token::<U>))
        .route("/:id/regenerate-secret", post(regenerate_signing_secret::<U>))
        .route("/:id/roles", get(get_roles::<U>).put(assign_roles::<U>))
        .with_state(state)
}
