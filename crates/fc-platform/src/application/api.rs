//! Applications Admin API
//!
//! REST endpoints for application management.
//! Applications are global platform entities (not client-scoped).

use axum::{
    routing::{get, post, put},
    extract::{State, Path, Query},
    http::StatusCode,
    Json, Router,
};
use utoipa::{ToSchema, IntoParams};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{Application, ServiceAccount, AuthRole};
use crate::{ApplicationRepository, ServiceAccountRepository, RoleRepository, ApplicationClientConfigRepository, ClientRepository};
use crate::shared::error::PlatformError;
use crate::shared::api_common::PaginationParams;
use crate::shared::middleware::Authenticated;
use crate::usecase::{ExecutionContext, UnitOfWork, UseCase, UseCaseResult};
use crate::application::operations::{
    CreateApplicationCommand, CreateApplicationUseCase,
    UpdateApplicationCommand, UpdateApplicationUseCase,
    ActivateApplicationCommand, ActivateApplicationUseCase,
    DeactivateApplicationCommand, DeactivateApplicationUseCase,
};

/// Create application request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateApplicationRequest {
    /// Unique identifier/code (URL-safe)
    pub code: String,

    /// Human-readable name
    pub name: String,

    /// Description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Application type: APPLICATION or INTEGRATION
    #[serde(rename = "type")]
    pub application_type: Option<String>,

    /// Default base URL
    pub default_base_url: Option<String>,

    /// Icon URL
    pub icon_url: Option<String>,
}

/// Update application request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateApplicationRequest {
    /// Human-readable name
    pub name: Option<String>,

    /// Description
    pub description: Option<String>,

    /// Default base URL
    pub default_base_url: Option<String>,

    /// Icon URL
    pub icon_url: Option<String>,
}

/// Application response DTO
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationResponse {
    pub id: String,
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "type")]
    pub application_type: String,
    pub default_base_url: Option<String>,
    pub icon_url: Option<String>,
    pub service_account_id: Option<String>,
    pub active: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl From<Application> for ApplicationResponse {
    fn from(a: Application) -> Self {
        Self {
            id: a.id,
            code: a.code,
            name: a.name,
            description: a.description,
            application_type: format!("{:?}", a.application_type).to_uppercase(),
            default_base_url: a.default_base_url,
            icon_url: a.icon_url,
            service_account_id: a.service_account_id,
            active: a.active,
            created_at: a.created_at.to_rfc3339(),
            updated_at: a.updated_at.to_rfc3339(),
        }
    }
}

/// Query parameters for applications list
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct ApplicationsQuery {
    #[serde(flatten)]
    pub pagination: PaginationParams,

    /// Filter by active status
    pub active: Option<bool>,
}

/// Service account response DTO
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccountResponse {
    pub id: String,
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub active: bool,
    pub application_id: Option<String>,
    pub created_at: String,
}

impl From<ServiceAccount> for ServiceAccountResponse {
    fn from(sa: ServiceAccount) -> Self {
        Self {
            id: sa.id,
            code: sa.code,
            name: sa.name,
            description: sa.description,
            active: sa.active,
            application_id: sa.application_id,
            created_at: sa.created_at.to_rfc3339(),
        }
    }
}

/// Application role response DTO
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationRoleResponse {
    pub id: String,
    pub code: String,
    pub display_name: String,
    pub description: Option<String>,
    pub application_code: String,
    pub permissions: Vec<String>,
    pub source: String,
    pub client_managed: bool,
}

impl From<AuthRole> for ApplicationRoleResponse {
    fn from(r: AuthRole) -> Self {
        Self {
            id: r.id,
            code: r.name,
            display_name: r.display_name,
            description: r.description,
            application_code: r.application_code,
            permissions: r.permissions.into_iter().collect(),
            source: r.source.as_str().to_string(),
            client_managed: r.client_managed,
        }
    }
}

/// Applications service state
#[derive(Clone)]
pub struct ApplicationsState<U: UnitOfWork + 'static> {
    pub application_repo: Arc<ApplicationRepository>,
    pub service_account_repo: Arc<ServiceAccountRepository>,
    pub role_repo: Arc<RoleRepository>,
    pub client_config_repo: Arc<ApplicationClientConfigRepository>,
    pub client_repo: Arc<ClientRepository>,
    pub create_use_case: Arc<CreateApplicationUseCase<U>>,
    pub update_use_case: Arc<UpdateApplicationUseCase<U>>,
    pub activate_use_case: Arc<ActivateApplicationUseCase<U>>,
    pub deactivate_use_case: Arc<DeactivateApplicationUseCase<U>>,
    pub enable_for_client_use_case: Arc<crate::application::operations::EnableApplicationForClientUseCase<U>>,
    pub disable_for_client_use_case: Arc<crate::application::operations::DisableApplicationForClientUseCase<U>>,
    pub update_client_config_use_case: Arc<crate::application::operations::UpdateApplicationClientConfigUseCase<U>>,
    /// Concrete `PgUnitOfWork` for orchestrated operations (provision-service-account)
    /// that span two aggregates. Routed via `run(closure)` — handler owns the
    /// tx boundary. Trait-backed use cases still go through `U`.
    pub pg_unit_of_work: Arc<crate::usecase::PgUnitOfWork>,
}

/// Create a new application
#[utoipa::path(
    post,
    path = "",
    tag = "applications",
    operation_id = "postApiAdminApplications",
    request_body = CreateApplicationRequest,
    responses(
        (status = 201, description = "Application created", body = crate::shared::api_common::CreatedResponse),
        (status = 400, description = "Validation error"),
        (status = 409, description = "Duplicate code")
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_application<U: UnitOfWork>(
    State(state): State<ApplicationsState<U>>,
    auth: Authenticated,
    Json(req): Json<CreateApplicationRequest>,
) -> Result<(StatusCode, Json<crate::shared::api_common::CreatedResponse>), PlatformError> {
    // Only anchor users can manage applications
    crate::shared::authorization_service::checks::require_anchor(&auth.0)?;

    let command = CreateApplicationCommand {
        code: req.code,
        name: req.name,
        description: req.description,
        application_type: req.application_type,
        default_base_url: req.default_base_url,
        icon_url: req.icon_url,
    };

    let ctx = ExecutionContext::create(auth.0.principal_id.clone());

    match state.create_use_case.run(command, ctx).await {
        UseCaseResult::Success(event) => {
            Ok((StatusCode::CREATED, Json(crate::shared::api_common::CreatedResponse::new(event.application_id))))
        }
        UseCaseResult::Failure(err) => Err(err.into()),
    }
}

/// Get application by ID
#[utoipa::path(
    get,
    path = "/{id}",
    tag = "applications",
    operation_id = "getApiAdminApplicationsById",
    params(
        ("id" = String, Path, description = "Application ID")
    ),
    responses(
        (status = 200, description = "Application found", body = ApplicationResponse),
        (status = 404, description = "Application not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_application<U: UnitOfWork>(
    State(state): State<ApplicationsState<U>>,
    _auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<ApplicationResponse>, PlatformError> {
    let app = state.application_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("Application", &id))?;

    Ok(Json(app.into()))
}

/// Applications list response (wrapped)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationListResponse {
    pub applications: Vec<ApplicationResponse>,
    pub total: usize,
}

/// List applications
#[utoipa::path(
    get,
    path = "",
    tag = "applications",
    operation_id = "getApiAdminApplications",
    params(ApplicationsQuery),
    responses(
        (status = 200, description = "List of applications", body = ApplicationListResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_applications<U: UnitOfWork>(
    State(state): State<ApplicationsState<U>>,
    _auth: Authenticated,
    Query(query): Query<ApplicationsQuery>,
) -> Result<Json<ApplicationListResponse>, PlatformError> {
    let apps = if query.active == Some(false) {
        state.application_repo.find_all().await?
    } else {
        // Default: activeOnly = true
        state.application_repo.find_active().await?
    };

    let applications: Vec<ApplicationResponse> = apps.into_iter()
        .map(|a| a.into())
        .collect();
    let total = applications.len();

    Ok(Json(ApplicationListResponse { applications, total }))
}

/// Update application
#[utoipa::path(
    put,
    path = "/{id}",
    tag = "applications",
    operation_id = "putApiAdminApplicationsById",
    params(
        ("id" = String, Path, description = "Application ID")
    ),
    request_body = UpdateApplicationRequest,
    responses(
        (status = 204, description = "Application updated"),
        (status = 404, description = "Application not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_application<U: UnitOfWork>(
    State(state): State<ApplicationsState<U>>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<UpdateApplicationRequest>,
) -> Result<StatusCode, PlatformError> {
    crate::shared::authorization_service::checks::require_anchor(&auth.0)?;

    let command = UpdateApplicationCommand {
        id: id.clone(),
        name: req.name,
        description: req.description,
        default_base_url: req.default_base_url,
        icon_url: req.icon_url,
    };

    let ctx = ExecutionContext::create(auth.0.principal_id.clone());

    match state.update_use_case.run(command, ctx).await {
        UseCaseResult::Success(_event) => Ok(StatusCode::NO_CONTENT),
        UseCaseResult::Failure(err) => Err(err.into()),
    }
}

/// Delete application (deactivate)
#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "applications",
    operation_id = "deleteApiAdminApplicationsById",
    params(
        ("id" = String, Path, description = "Application ID")
    ),
    responses(
        (status = 204, description = "Application deleted"),
        (status = 404, description = "Application not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_application<U: UnitOfWork>(
    State(state): State<ApplicationsState<U>>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<StatusCode, PlatformError> {
    crate::shared::authorization_service::checks::require_anchor(&auth.0)?;

    let command = DeactivateApplicationCommand { id };
    let ctx = ExecutionContext::create(auth.0.principal_id.clone());

    match state.deactivate_use_case.run(command, ctx).await {
        UseCaseResult::Success(_event) => Ok(StatusCode::NO_CONTENT),
        UseCaseResult::Failure(err) => Err(err.into()),
    }
}

/// Activate application
#[utoipa::path(
    post,
    path = "/{id}/activate",
    tag = "applications",
    operation_id = "postApiAdminApplicationsByIdActivate",
    params(
        ("id" = String, Path, description = "Application ID")
    ),
    responses(
        (status = 200, description = "Application activated", body = ApplicationResponse),
        (status = 404, description = "Application not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn activate_application<U: UnitOfWork>(
    State(state): State<ApplicationsState<U>>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<ApplicationResponse>, PlatformError> {
    crate::shared::authorization_service::checks::require_anchor(&auth.0)?;

    let command = ActivateApplicationCommand { id: id.clone() };
    let ctx = ExecutionContext::create(auth.0.principal_id.clone());

    match state.activate_use_case.run(command, ctx).await {
        UseCaseResult::Success(_event) => {
            let app = state.application_repo.find_by_id(&id).await?
                .ok_or_else(|| PlatformError::not_found("Application", &id))?;
            Ok(Json(app.into()))
        }
        UseCaseResult::Failure(err) => Err(err.into()),
    }
}

/// Deactivate application
#[utoipa::path(
    post,
    path = "/{id}/deactivate",
    tag = "applications",
    operation_id = "postApiAdminApplicationsByIdDeactivate",
    params(
        ("id" = String, Path, description = "Application ID")
    ),
    responses(
        (status = 200, description = "Application deactivated", body = ApplicationResponse),
        (status = 404, description = "Application not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn deactivate_application<U: UnitOfWork>(
    State(state): State<ApplicationsState<U>>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<ApplicationResponse>, PlatformError> {
    crate::shared::authorization_service::checks::require_anchor(&auth.0)?;

    let command = DeactivateApplicationCommand { id: id.clone() };
    let ctx = ExecutionContext::create(auth.0.principal_id.clone());

    match state.deactivate_use_case.run(command, ctx).await {
        UseCaseResult::Success(_event) => {
            let app = state.application_repo.find_by_id(&id).await?
                .ok_or_else(|| PlatformError::not_found("Application", &id))?;
            Ok(Json(app.into()))
        }
        UseCaseResult::Failure(err) => Err(err.into()),
    }
}

/// Get application by code
#[utoipa::path(
    get,
    path = "/by-code/{code}",
    tag = "applications",
    operation_id = "getApiAdminApplicationsByCodeByCode",
    params(
        ("code" = String, Path, description = "Application code")
    ),
    responses(
        (status = 200, description = "Application found", body = ApplicationResponse),
        (status = 404, description = "Application not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_application_by_code<U: UnitOfWork>(
    State(state): State<ApplicationsState<U>>,
    _auth: Authenticated,
    Path(code): Path<String>,
) -> Result<Json<ApplicationResponse>, PlatformError> {
    let app = state.application_repo.find_by_code(&code).await?
        .ok_or_else(|| PlatformError::not_found("Application", &code))?;

    Ok(Json(app.into()))
}

/// Provision a service account for an application.
///
/// Two-aggregate operation: creates a ServiceAccount + updates Application.
/// Both commits happen inside a single DB transaction via
/// `PgUnitOfWork::run(…)` — either both land or both roll back. The
/// handler owns the tx boundary; the use cases stay tx-agnostic.
#[utoipa::path(
    post,
    path = "/{id}/provision-service-account",
    tag = "applications",
    operation_id = "postApiAdminApplicationsByIdProvisionServiceAccount",
    params(
        ("id" = String, Path, description = "Application ID")
    ),
    responses(
        (status = 201, description = "Service account provisioned", body = ServiceAccountResponse),
        (status = 404, description = "Application not found"),
        (status = 409, description = "Service account already exists")
    ),
    security(("bearer_auth" = []))
)]
pub async fn provision_service_account<U: UnitOfWork>(
    State(state): State<ApplicationsState<U>>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<ServiceAccountResponse>, PlatformError> {
    use crate::application::operations::{
        AttachServiceAccountToApplicationCommand,
        AttachServiceAccountToApplicationUseCase,
    };
    use crate::service_account::operations::{
        CreateServiceAccountCommand,
        CreateServiceAccountUseCase,
    };

    crate::shared::authorization_service::checks::require_anchor(&auth.0)?;

    // Pre-validate: fail early before opening a tx. Mirrors the business
    // rule inside AttachServiceAccountToApplicationUseCase.
    let app = state.application_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("Application", &id))?;
    if app.service_account_id.is_some() {
        return Err(PlatformError::conflict(
            "Application already has a service account provisioned",
        ));
    }

    let sa_code = format!("app:{}", app.code);
    let sa_name = format!("{} Service Account", app.name);
    let sa_description = format!("Service account for application: {}", app.name);

    let app_id = app.id.clone();
    let principal_id = auth.0.principal_id.clone();
    let sa_repo = state.service_account_repo.clone();
    let app_repo = state.application_repo.clone();

    // One DB tx for both use cases. If the attach fails for any reason
    // (rare — we pre-checked), the ServiceAccount insert rolls back too.
    let result = state.pg_unit_of_work.run(|session| async move {
        let create_sa_uc = CreateServiceAccountUseCase::new(sa_repo, session.clone());
        let attach_uc = AttachServiceAccountToApplicationUseCase::new(app_repo, session);

        let create_cmd = CreateServiceAccountCommand {
            code: sa_code.clone(),
            name: sa_name,
            description: Some(sa_description),
            client_ids: Vec::new(),
            application_id: Some(app_id.clone()),
        };
        let ctx = crate::usecase::ExecutionContext::create(&principal_id);
        let created = match create_sa_uc.run(create_cmd, ctx.clone()).await.into_result() {
            Ok(c) => c,
            Err(err) => return crate::usecase::UseCaseResult::failure(err),
        };
        let sa_id = created.event.service_account_id.clone();

        let attach_cmd = AttachServiceAccountToApplicationCommand {
            application_id: app_id,
            service_account_id: sa_id.clone(),
            service_account_code: sa_code,
        };
        // `.map()` transforms the attach event into the SA id without
        // bypassing the Result seal — success can only come from a UoW
        // commit, then we re-shape the success payload.
        attach_uc.run(attach_cmd, ctx).await.map(move |_| sa_id)
    }).await;

    let sa_id = result.into_result()?;

    // Fetch the committed service account for the response. This is a read
    // after the tx closed — safe, the SA row is guaranteed to exist on the
    // success path.
    let service_account = state.service_account_repo.find_by_id(&sa_id).await?
        .ok_or_else(|| PlatformError::not_found("ServiceAccount", &sa_id))?;

    Ok(Json(service_account.into()))
}

/// Get service account for an application
#[utoipa::path(
    get,
    path = "/{id}/service-account",
    tag = "applications",
    operation_id = "getApiAdminApplicationsByIdServiceAccount",
    params(
        ("id" = String, Path, description = "Application ID")
    ),
    responses(
        (status = 200, description = "Service account found", body = ServiceAccountResponse),
        (status = 404, description = "Application or service account not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_application_service_account<U: UnitOfWork>(
    State(state): State<ApplicationsState<U>>,
    _auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<ServiceAccountResponse>, PlatformError> {
    // Get the application
    let app = state.application_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("Application", &id))?;

    // Get the service account
    let sa_id = app.service_account_id
        .ok_or_else(|| PlatformError::not_found("ServiceAccount", "for application"))?;

    let service_account = state.service_account_repo.find_by_id(&sa_id).await?
        .ok_or_else(|| PlatformError::not_found("ServiceAccount", &sa_id))?;

    Ok(Json(service_account.into()))
}

/// List roles for an application (admin, by TSID).
///
/// Mounted under a `/by-id` prefix so it doesn't collide with the SDK's
/// `/{app_code}/roles` route. The SDK path takes the application code;
/// this admin path takes the TSID (which the frontend has on hand).
#[utoipa::path(
    get,
    path = "/by-id/{id}/roles",
    tag = "applications",
    operation_id = "getApiAdminApplicationsByIdRoles",
    params(
        ("id" = String, Path, description = "Application ID")
    ),
    responses(
        (status = 200, description = "Application roles", body = Vec<ApplicationRoleResponse>),
        (status = 404, description = "Application not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_application_roles<U: UnitOfWork>(
    State(state): State<ApplicationsState<U>>,
    _auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<Vec<ApplicationRoleResponse>>, PlatformError> {
    // Get the application
    let app = state.application_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("Application", &id))?;

    // Find roles by application code
    let roles = state.role_repo.find_by_application(&app.code).await?;

    let response: Vec<ApplicationRoleResponse> = roles.into_iter()
        .map(|r| r.into())
        .collect();

    Ok(Json(response))
}

/// Client config response DTO
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClientConfigResponse {
    pub id: String,
    pub application_id: String,
    pub client_id: String,
    pub client_name: String,
    pub client_identifier: String,
    pub enabled: bool,
    pub base_url_override: Option<String>,
    pub effective_base_url: Option<String>,
    pub config: Option<serde_json::Value>,
}

/// Client configs list response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClientConfigsResponse {
    pub client_configs: Vec<ClientConfigResponse>,
    pub total: usize,
}

/// Client config request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClientConfigRequest {
    pub enabled: Option<bool>,
    pub base_url_override: Option<String>,
    pub config: Option<serde_json::Value>,
}

/// List client configs for an application
#[utoipa::path(
    get,
    path = "/{id}/clients",
    tag = "applications",
    operation_id = "getApiAdminApplicationsByIdClients",
    params(
        ("id" = String, Path, description = "Application ID")
    ),
    responses(
        (status = 200, description = "Client configurations", body = ClientConfigsResponse),
        (status = 404, description = "Application not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_client_configs<U: UnitOfWork>(
    State(state): State<ApplicationsState<U>>,
    _auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<ClientConfigsResponse>, PlatformError> {
    // Verify application exists
    let app = state.application_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("Application", &id))?;

    let configs = state.client_config_repo.find_by_application(&id).await?;

    let mut client_configs = Vec::new();
    for config in configs {
        // Get client details
        if let Some(client) = state.client_repo.find_by_id(&config.client_id).await? {
            client_configs.push(ClientConfigResponse {
                id: config.id,
                application_id: config.application_id,
                client_id: config.client_id,
                client_name: client.name,
                client_identifier: client.identifier,
                enabled: config.enabled,
                base_url_override: config.base_url_override.clone(),
                effective_base_url: config.base_url_override.or(app.default_base_url.clone()),
                config: config.config_json,
            });
        }
    }

    let total = client_configs.len();
    Ok(Json(ClientConfigsResponse {
        client_configs,
        total,
    }))
}

/// Update client config for an application.
/// Routes through UpdateApplicationClientConfigUseCase so the change is
/// atomic with an `ApplicationClientConfigUpdated` event + audit log.
#[utoipa::path(
    put,
    path = "/{id}/clients/{client_id}",
    tag = "applications",
    operation_id = "putApiAdminApplicationsByIdClientsByClientId",
    params(
        ("id" = String, Path, description = "Application ID"),
        ("client_id" = String, Path, description = "Client ID")
    ),
    request_body = ClientConfigRequest,
    responses(
        (status = 200, description = "Configuration updated", body = ClientConfigResponse),
        (status = 404, description = "Application or client not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_client_config<U: UnitOfWork>(
    State(state): State<ApplicationsState<U>>,
    auth: Authenticated,
    Path((id, client_id)): Path<(String, String)>,
    Json(req): Json<ClientConfigRequest>,
) -> Result<Json<ClientConfigResponse>, PlatformError> {
    crate::shared::authorization_service::checks::require_anchor(&auth.0)?;

    let cmd = crate::application::operations::UpdateApplicationClientConfigCommand {
        application_id: id.clone(),
        client_id: client_id.clone(),
        enabled: req.enabled,
        base_url_override: req.base_url_override,
        config: req.config,
    };
    let ctx = ExecutionContext::create(auth.0.principal_id.clone());
    state.update_client_config_use_case.run(cmd, ctx).await.into_result()?;

    // Refetch to build the response — the use case returns an event, not the
    // hydrated config, and the response carries the application's effective
    // base URL + client name/identifier alongside the updated config.
    let app = state.application_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("Application", &id))?;
    let client = state.client_repo.find_by_id(&client_id).await?
        .ok_or_else(|| PlatformError::not_found("Client", &client_id))?;
    let config = state.client_config_repo
        .find_by_application_and_client(&id, &client_id).await?
        .ok_or_else(|| PlatformError::not_found("ApplicationClientConfig", "for (app, client)"))?;

    Ok(Json(ClientConfigResponse {
        id: config.id,
        application_id: config.application_id,
        client_id: config.client_id,
        client_name: client.name,
        client_identifier: client.identifier,
        enabled: config.enabled,
        base_url_override: config.base_url_override.clone(),
        effective_base_url: config.base_url_override.or(app.default_base_url),
        config: config.config_json,
    }))
}

/// Enable application for a client.
/// Routes through EnableApplicationForClientUseCase (UoW-backed).
#[utoipa::path(
    post,
    path = "/{id}/clients/{client_id}/enable",
    tag = "applications",
    operation_id = "postApiAdminApplicationsByIdClientsByClientIdEnable",
    params(
        ("id" = String, Path, description = "Application ID"),
        ("client_id" = String, Path, description = "Client ID")
    ),
    responses(
        (status = 200, description = "Application enabled for client", body = ClientConfigResponse),
        (status = 404, description = "Application or client not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn enable_for_client<U: UnitOfWork>(
    State(state): State<ApplicationsState<U>>,
    auth: Authenticated,
    Path((id, client_id)): Path<(String, String)>,
) -> Result<Json<ClientConfigResponse>, PlatformError> {
    crate::shared::authorization_service::checks::require_anchor(&auth.0)?;

    let cmd = crate::application::operations::EnableApplicationForClientCommand {
        application_id: id.clone(),
        client_id: client_id.clone(),
    };
    let ctx = ExecutionContext::create(auth.0.principal_id.clone());
    state.enable_for_client_use_case.run(cmd, ctx).await.into_result()?;

    build_client_config_response(&state, &id, &client_id).await
}

/// Disable application for a client.
/// Routes through DisableApplicationForClientUseCase (UoW-backed).
#[utoipa::path(
    post,
    path = "/{id}/clients/{client_id}/disable",
    tag = "applications",
    operation_id = "postApiAdminApplicationsByIdClientsByClientIdDisable",
    params(
        ("id" = String, Path, description = "Application ID"),
        ("client_id" = String, Path, description = "Client ID")
    ),
    responses(
        (status = 200, description = "Application disabled for client", body = ClientConfigResponse),
        (status = 404, description = "Application or client not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn disable_for_client<U: UnitOfWork>(
    State(state): State<ApplicationsState<U>>,
    auth: Authenticated,
    Path((id, client_id)): Path<(String, String)>,
) -> Result<Json<ClientConfigResponse>, PlatformError> {
    crate::shared::authorization_service::checks::require_anchor(&auth.0)?;

    let cmd = crate::application::operations::DisableApplicationForClientCommand {
        application_id: id.clone(),
        client_id: client_id.clone(),
    };
    let ctx = ExecutionContext::create(auth.0.principal_id.clone());
    state.disable_for_client_use_case.run(cmd, ctx).await.into_result()?;

    build_client_config_response(&state, &id, &client_id).await
}

/// Build the `ClientConfigResponse` by re-loading the app, client, and
/// config after a use case has mutated the config. Shared by
/// `enable_for_client` / `disable_for_client` / `update_client_config`.
async fn build_client_config_response<U: UnitOfWork>(
    state: &ApplicationsState<U>,
    app_id: &str,
    client_id: &str,
) -> Result<Json<ClientConfigResponse>, PlatformError> {
    let app = state.application_repo.find_by_id(app_id).await?
        .ok_or_else(|| PlatformError::not_found("Application", app_id))?;
    let client = state.client_repo.find_by_id(client_id).await?
        .ok_or_else(|| PlatformError::not_found("Client", client_id))?;
    let config = state.client_config_repo
        .find_by_application_and_client(app_id, client_id).await?
        .ok_or_else(|| PlatformError::not_found("ApplicationClientConfig", "for (app, client)"))?;

    Ok(Json(ClientConfigResponse {
        id: config.id,
        application_id: config.application_id,
        client_id: config.client_id,
        client_name: client.name,
        client_identifier: client.identifier,
        enabled: config.enabled,
        base_url_override: config.base_url_override.clone(),
        effective_base_url: config.base_url_override.or(app.default_base_url),
        config: config.config_json,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::entity::{Application, ApplicationType};
    use chrono::Utc;

    fn make_test_application() -> Application {
        let now = Utc::now();
        Application {
            id: "app_ABCDEFGHIJKLM".to_string(),
            application_type: ApplicationType::Application,
            code: "my-app".to_string(),
            name: "My Application".to_string(),
            description: Some("A test application".to_string()),
            icon_url: Some("https://example.com/icon.png".to_string()),
            website: None,
            logo: None,
            logo_mime_type: None,
            default_base_url: Some("https://api.example.com".to_string()),
            service_account_id: Some("sac_SERVICEID12345".to_string()),
            active: true,
            created_at: now,
            updated_at: now,
        }
    }

    // --- ApplicationResponse serialization ---

    #[test]
    fn test_application_response_serialization() {
        let app = make_test_application();
        let response = ApplicationResponse::from(app);

        let json = serde_json::to_value(&response).unwrap();

        assert_eq!(json["id"], "app_ABCDEFGHIJKLM");
        assert_eq!(json["code"], "my-app");
        assert_eq!(json["name"], "My Application");
        assert_eq!(json["description"], "A test application");
        assert_eq!(json["type"], "APPLICATION");
        assert_eq!(json["defaultBaseUrl"], "https://api.example.com");
        assert_eq!(json["iconUrl"], "https://example.com/icon.png");
        assert_eq!(json["serviceAccountId"], "sac_SERVICEID12345");
        assert_eq!(json["active"], true);
        // Verify camelCase field names
        assert!(json.get("createdAt").is_some());
        assert!(json.get("updatedAt").is_some());
        // Verify no snake_case leak
        assert!(json.get("application_type").is_none());
        assert!(json.get("default_base_url").is_none());
        assert!(json.get("icon_url").is_none());
        assert!(json.get("service_account_id").is_none());
    }

    #[test]
    fn test_application_response_integration_type() {
        let mut app = make_test_application();
        app.application_type = ApplicationType::Integration;

        let response = ApplicationResponse::from(app);
        let json = serde_json::to_value(&response).unwrap();

        assert_eq!(json["type"], "INTEGRATION");
    }

    #[test]
    fn test_application_response_null_optionals() {
        let now = Utc::now();
        let app = Application {
            id: "app_MINIMALAPPTEST".to_string(),
            application_type: ApplicationType::Application,
            code: "minimal".to_string(),
            name: "Minimal".to_string(),
            description: None,
            icon_url: None,
            website: None,
            logo: None,
            logo_mime_type: None,
            default_base_url: None,
            service_account_id: None,
            active: false,
            created_at: now,
            updated_at: now,
        };

        let response = ApplicationResponse::from(app);
        let json = serde_json::to_value(&response).unwrap();

        assert!(json["description"].is_null());
        assert!(json["defaultBaseUrl"].is_null());
        assert!(json["iconUrl"].is_null());
        assert!(json["serviceAccountId"].is_null());
        assert_eq!(json["active"], false);
    }

    // --- CreateApplicationRequest deserialization ---

    #[test]
    fn test_create_application_request_deserialization() {
        let json = serde_json::json!({
            "code": "new-app",
            "name": "New Application",
            "description": "A new app",
            "type": "INTEGRATION",
            "defaultBaseUrl": "https://api.example.com",
            "iconUrl": "https://example.com/icon.png"
        });

        let req: CreateApplicationRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.code, "new-app");
        assert_eq!(req.name, "New Application");
        assert_eq!(req.description, Some("A new app".to_string()));
        assert_eq!(req.application_type, Some("INTEGRATION".to_string()));
        assert_eq!(req.default_base_url, Some("https://api.example.com".to_string()));
        assert_eq!(req.icon_url, Some("https://example.com/icon.png".to_string()));
    }

    #[test]
    fn test_create_application_request_minimal() {
        let json = serde_json::json!({
            "code": "app",
            "name": "App"
        });

        let req: CreateApplicationRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.code, "app");
        assert_eq!(req.name, "App");
        assert!(req.description.is_none());
        assert!(req.application_type.is_none());
        assert!(req.default_base_url.is_none());
        assert!(req.icon_url.is_none());
    }

    #[test]
    fn test_create_application_request_missing_code() {
        let json = serde_json::json!({
            "name": "Test"
        });

        let result = serde_json::from_value::<CreateApplicationRequest>(json);
        assert!(result.is_err(), "Should fail without code");
    }

    #[test]
    fn test_create_application_request_missing_name() {
        let json = serde_json::json!({
            "code": "test"
        });

        let result = serde_json::from_value::<CreateApplicationRequest>(json);
        assert!(result.is_err(), "Should fail without name");
    }

    #[test]
    fn test_create_application_request_empty_json() {
        let json = serde_json::json!({});
        let result = serde_json::from_value::<CreateApplicationRequest>(json);
        assert!(result.is_err(), "Should fail with empty JSON");
    }

    // --- UpdateApplicationRequest ---

    #[test]
    fn test_update_application_request_deserialization() {
        let json = serde_json::json!({
            "name": "Updated Name",
            "description": "Updated description"
        });

        let req: UpdateApplicationRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.name, Some("Updated Name".to_string()));
        assert_eq!(req.description, Some("Updated description".to_string()));
    }

    #[test]
    fn test_update_application_request_empty() {
        let json = serde_json::json!({});
        let req: UpdateApplicationRequest = serde_json::from_value(json).unwrap();
        assert!(req.name.is_none());
        assert!(req.description.is_none());
        assert!(req.default_base_url.is_none());
        assert!(req.icon_url.is_none());
    }

    // --- ApplicationListResponse ---

    #[test]
    fn test_application_list_response_serialization() {
        let app = make_test_application();
        let list = ApplicationListResponse {
            applications: vec![ApplicationResponse::from(app)],
            total: 1,
        };

        let json = serde_json::to_value(&list).unwrap();
        assert!(json["applications"].is_array());
        assert_eq!(json["applications"].as_array().unwrap().len(), 1);
        assert_eq!(json["total"], 1);
    }

    // --- ClientConfigRequest ---

    #[test]
    fn test_client_config_request_deserialization() {
        let json = serde_json::json!({
            "enabled": true,
            "baseUrlOverride": "https://custom.example.com",
            "config": {"key": "value"}
        });

        let req: ClientConfigRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.enabled, Some(true));
        assert_eq!(req.base_url_override, Some("https://custom.example.com".to_string()));
        assert!(req.config.is_some());
    }
}

/// Create applications router
pub fn applications_router<U: UnitOfWork + Clone>(state: ApplicationsState<U>) -> Router {
    Router::new()
        .route("/", post(create_application::<U>).get(list_applications::<U>))
        .route("/{id}", get(get_application::<U>).put(update_application::<U>).delete(delete_application::<U>))
        .route("/{id}/activate", post(activate_application::<U>))
        .route("/{id}/deactivate", post(deactivate_application::<U>))
        .route("/{id}/provision-service-account", post(provision_service_account::<U>))
        .route("/{id}/service-account", get(get_application_service_account::<U>))
        .route("/by-id/{id}/roles", get(list_application_roles::<U>))
        .route("/{id}/clients", get(list_client_configs::<U>))
        .route("/{id}/clients/{client_id}", put(update_client_config::<U>))
        .route("/{id}/clients/{client_id}/enable", post(enable_for_client::<U>))
        .route("/{id}/clients/{client_id}/disable", post(disable_for_client::<U>))
        .route("/by-code/{code}", get(get_application_by_code::<U>))
        .with_state(state)
}
