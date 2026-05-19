//! Applications Admin API
//!
//! REST endpoints for application management.
//! Applications are global platform entities (not client-scoped).

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::{IntoParams, ToSchema};

use crate::application::operations::{
    ActivateApplicationCommand, ActivateApplicationUseCase, CreateApplicationCommand,
    CreateApplicationUseCase, DeactivateApplicationCommand, DeactivateApplicationUseCase,
    UpdateApplicationCommand, UpdateApplicationUseCase,
};
use crate::auth::oauth_entity::{GrantType, OAuthClientType};
use crate::auth::operations::CreateOAuthClientUseCase;
use crate::shared::api_common::PaginationParams;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;
use crate::usecase::{ExecutionContext, UnitOfWork, UseCase, UseCaseResult};
use crate::{Application, AuthRole, OAuthClientRepository, ServiceAccount};
use crate::{
    ApplicationClientConfigRepository, ApplicationRepository, ClientRepository, RoleRepository,
    ServiceAccountRepository,
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
    /// True iff this application has a login OAuth client provisioned (used
    /// to gate the "Provision Login Client" button in the UI). Populated by
    /// the detail endpoint only; list responses leave it `None` to avoid an
    /// N+1 lookup across rows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_login_client: Option<bool>,
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
            has_login_client: None,
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

/// OAuth client credentials returned from a provisioning endpoint.
///
/// The `clientSecret` is only populated for CONFIDENTIAL clients and only at
/// the moment of creation — the platform stores it encrypted and never
/// returns it again. Rotate via `POST /api/oauth-clients/{id}/regenerate-secret`.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OAuthClientCredentials {
    /// OAuth client row id (`oac_…`).
    pub id: String,
    /// Public `clientId` used in the OAuth flows.
    pub client_id: String,
    /// Plaintext client secret — only on creation, only for CONFIDENTIAL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
}

/// Wrapper returned from `POST /api/applications/{id}/provision-service-account`.
///
/// Matches the frontend's existing `ServiceAccountCredentials` shape so the
/// page can display the freshly-minted credentials in one modal.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccountCredentialsResponse {
    /// Principal id of the service account (`sac_…`).
    pub principal_id: String,
    /// Service account display name (used in the credentials dialog).
    pub name: String,
    /// OAuth client minted alongside the service account, with the only
    /// chance to read the plaintext secret.
    pub oauth_client: OAuthClientCredentials,
}

/// Wrapper response from `POST /api/applications/{id}/provision-service-account`.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProvisionServiceAccountResponse {
    pub message: String,
    pub service_account: ServiceAccountCredentialsResponse,
}

/// Request body for `POST /api/applications/{id}/provision-login-client`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProvisionLoginClientRequest {
    /// `"PUBLIC"` (default, PKCE-only) or `"CONFIDENTIAL"` (has a client secret).
    /// PUBLIC is the right choice for SPAs and native apps; CONFIDENTIAL is for
    /// server-rendered apps that can keep a secret.
    #[serde(default)]
    pub client_type: Option<String>,
    /// One or more URLs the application redirects to after login. At least one is required.
    pub redirect_uris: Vec<String>,
    /// Origins permitted by the browser for CORS preflight on the auth endpoints.
    /// Optional; defaults to the redirect URI origins.
    #[serde(default)]
    pub allowed_origins: Vec<String>,
}

/// Response from `POST /api/applications/{id}/provision-login-client`.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProvisionLoginClientResponse {
    pub message: String,
    pub login_client: LoginClientCredentialsResponse,
}

/// Login-client credentials returned at provision time. `clientSecret` is
/// populated only for CONFIDENTIAL clients.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoginClientCredentialsResponse {
    pub client_type: String,
    pub oauth_client: OAuthClientCredentials,
    pub redirect_uris: Vec<String>,
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
    pub enable_for_client_use_case:
        Arc<crate::application::operations::EnableApplicationForClientUseCase<U>>,
    pub disable_for_client_use_case:
        Arc<crate::application::operations::DisableApplicationForClientUseCase<U>>,
    pub update_client_config_use_case:
        Arc<crate::application::operations::UpdateApplicationClientConfigUseCase<U>>,
    /// OAuth client repo + create use case — used by the provision-service-account
    /// and provision-login-client endpoints to mint a client_credentials or
    /// authorization_code OAuth client for the application.
    pub oauth_client_repo: Arc<OAuthClientRepository>,
    pub create_oauth_client_use_case: Arc<CreateOAuthClientUseCase<U>>,
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
    operation_id = "postApiApplications",
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
        UseCaseResult::Success(event) => Ok((
            StatusCode::CREATED,
            Json(crate::shared::api_common::CreatedResponse::new(
                event.application_id,
            )),
        )),
        UseCaseResult::Failure(err) => Err(err.into()),
    }
}

/// Get application by ID
#[utoipa::path(
    get,
    path = "/{id}",
    tag = "applications",
    operation_id = "getApiApplicationsById",
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
    let app = state
        .application_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("Application", &id))?;

    // Populate `hasLoginClient` so the UI can hide the "Provision Login
    // Client" button when one is already in place. Only computed for the
    // detail endpoint — list responses leave it absent.
    let has_login_client = app_has_login_client(&state.oauth_client_repo, &app.id).await?;
    let mut response: ApplicationResponse = app.into();
    response.has_login_client = Some(has_login_client);
    Ok(Json(response))
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
    operation_id = "getApiApplications",
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

    let applications: Vec<ApplicationResponse> = apps.into_iter().map(|a| a.into()).collect();
    let total = applications.len();

    Ok(Json(ApplicationListResponse {
        applications,
        total,
    }))
}

/// Update application
#[utoipa::path(
    put,
    path = "/{id}",
    tag = "applications",
    operation_id = "putApiApplicationsById",
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
    operation_id = "deleteApiApplicationsById",
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
    operation_id = "postApiApplicationsByIdActivate",
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
            let app = state
                .application_repo
                .find_by_id(&id)
                .await?
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
    operation_id = "postApiApplicationsByIdDeactivate",
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
            let app = state
                .application_repo
                .find_by_id(&id)
                .await?
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
    operation_id = "getApiApplicationsByCodeByCode",
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
    let app = state
        .application_repo
        .find_by_code(&code)
        .await?
        .ok_or_else(|| PlatformError::not_found("Application", &code))?;

    Ok(Json(app.into()))
}

/// Provision a service account for an application.
///
/// Three-aggregate operation in a single PG transaction (via
/// `PgUnitOfWork::run`): creates a `ServiceAccount`, attaches it to the
/// `Application`, and mints a CONFIDENTIAL OAuth client with
/// `grant_types: ["client_credentials"]` so consumer apps can authenticate
/// AS the service account. All three commits land together or all roll back.
///
/// The plaintext `clientSecret` is included in the response **only on this
/// call** — the platform stores only the encrypted form and can never
/// return it again. The frontend's credentials dialog is the user's only
/// chance to capture it. Rotate later via
/// `POST /api/oauth-clients/{id}/regenerate-secret` if needed.
#[utoipa::path(
    post,
    path = "/{id}/provision-service-account",
    tag = "applications",
    operation_id = "postApiApplicationsByIdProvisionServiceAccount",
    params(
        ("id" = String, Path, description = "Application ID")
    ),
    responses(
        (status = 201, description = "Service account provisioned", body = ProvisionServiceAccountResponse),
        (status = 404, description = "Application not found"),
        (status = 409, description = "Service account already exists")
    ),
    security(("bearer_auth" = []))
)]
pub async fn provision_service_account<U: UnitOfWork>(
    State(state): State<ApplicationsState<U>>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<ProvisionServiceAccountResponse>, PlatformError> {
    use crate::application::operations::{
        AttachServiceAccountToApplicationCommand, AttachServiceAccountToApplicationUseCase,
    };
    use crate::auth::operations::CreateOAuthClientCommand;
    use crate::service_account::operations::{
        CreateServiceAccountCommand, CreateServiceAccountUseCase,
    };

    crate::shared::authorization_service::checks::require_anchor(&auth.0)?;

    // Pre-validate: fail early before opening a tx. Mirrors the business
    // rule inside AttachServiceAccountToApplicationUseCase.
    let app = state
        .application_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("Application", &id))?;
    if app.service_account_id.is_some() {
        return Err(PlatformError::conflict(
            "Application already has a service account provisioned",
        ));
    }

    // Mint the OAuth client identifiers and a fresh secret BEFORE opening
    // the tx. We hand the encrypted ref into the closure and keep the
    // plaintext to return in the response.
    let oauth_row_id = crate::TsidGenerator::generate(crate::EntityType::OAuthClient);
    let oauth_public_client_id =
        crate::TsidGenerator::generate(crate::EntityType::OAuthClient);
    let (client_secret_plaintext, client_secret_ref) = generate_and_encrypt_client_secret()?;

    let sa_code = format!("app:{}", app.code);
    let sa_name = format!("{} Service Account", app.name);
    let sa_description = format!("Service account for application: {}", app.name);
    let oauth_client_name = format!("{} Service Account Client", app.name);

    let app_id = app.id.clone();
    let principal_id = auth.0.principal_id.clone();
    let sa_repo = state.service_account_repo.clone();
    let app_repo = state.application_repo.clone();
    let oauth_client_repo = state.oauth_client_repo.clone();

    let oauth_row_id_for_cmd = oauth_row_id.clone();
    let oauth_public_client_id_for_cmd = oauth_public_client_id.clone();

    // One DB tx for all three use cases. If any step fails, all rows
    // (SA insert, Application update, OAuth client insert) roll back.
    let result = state
        .pg_unit_of_work
        .run(|session| async move {
            let create_sa_uc = CreateServiceAccountUseCase::new(sa_repo, session.clone());
            let attach_uc =
                AttachServiceAccountToApplicationUseCase::new(app_repo, session.clone());
            let create_oauth_uc =
                CreateOAuthClientUseCase::new(oauth_client_repo, session);

            let ctx = crate::usecase::ExecutionContext::create(&principal_id);

            // 1. Create the ServiceAccount (a Principal row is created
            //    behind it; SA.id == Principal.id).
            let create_cmd = CreateServiceAccountCommand {
                code: sa_code.clone(),
                name: sa_name,
                description: Some(sa_description),
                client_ids: Vec::new(),
                application_id: Some(app_id.clone()),
            };
            let created = match create_sa_uc
                .run(create_cmd, ctx.clone())
                .await
                .into_result()
            {
                Ok(c) => c,
                Err(err) => return crate::usecase::UseCaseResult::failure(err),
            };
            let sa_id = created.event.service_account_id.clone();

            // 2. Attach SA to Application — sets `application.service_account_id`.
            let attach_cmd = AttachServiceAccountToApplicationCommand {
                application_id: app_id.clone(),
                service_account_id: sa_id.clone(),
                service_account_code: sa_code,
            };
            if let Err(err) = attach_uc
                .run(attach_cmd, ctx.clone())
                .await
                .into_result()
            {
                return crate::usecase::UseCaseResult::failure(err);
            }

            // 3. Mint the OAuth client (client_credentials grant) so the
            //    consumer can actually authenticate AS this service account.
            let oauth_cmd = CreateOAuthClientCommand {
                oauth_client_id: oauth_row_id_for_cmd,
                client_id: oauth_public_client_id_for_cmd,
                client_name: oauth_client_name,
                client_type: OAuthClientType::Confidential.as_str().to_string(),
                client_secret_ref: Some(format!("encrypted:{}", client_secret_ref)),
                redirect_uris: Vec::new(),
                grant_types: vec![GrantType::ClientCredentials.as_str().to_string()],
                default_scopes: Vec::new(),
                pkce_required: false,
                application_ids: vec![app_id],
                allowed_origins: Vec::new(),
                service_account_principal_id: Some(sa_id.clone()),
                created_by: Some(principal_id.clone()),
            };
            create_oauth_uc.run(oauth_cmd, ctx).await.map(move |_| sa_id)
        })
        .await;

    let sa_id = result.into_result()?;

    // Fetch the SA for its display name. The OAuth client id + client_id
    // are the ones we minted above, so we don't need to re-read the row.
    let service_account = state
        .service_account_repo
        .find_by_id(&sa_id)
        .await?
        .ok_or_else(|| PlatformError::not_found("ServiceAccount", &sa_id))?;

    Ok(Json(ProvisionServiceAccountResponse {
        message: "Service account provisioned".to_string(),
        service_account: ServiceAccountCredentialsResponse {
            principal_id: service_account.id,
            name: service_account.name,
            oauth_client: OAuthClientCredentials {
                id: oauth_row_id,
                client_id: oauth_public_client_id,
                client_secret: Some(client_secret_plaintext),
            },
        },
    }))
}

/// Provision an OAuth Login Client for an application.
///
/// Creates a single OAuth client with `grant_types: ["authorization_code"]`
/// scoped to this application. Used for OIDC-driven user login flows in the
/// app's frontend — separate from `provision-service-account`, which mints
/// a CONFIDENTIAL `client_credentials` client for M2M auth.
///
/// `clientType` defaults to `"PUBLIC"` (PKCE enforced — right answer for
/// SPAs / native apps). Pass `"CONFIDENTIAL"` for server-rendered apps that
/// can keep a secret; in that case the response includes a plaintext
/// `clientSecret` exactly once.
///
/// 409 if a login client (any OAuth client with `grant_types` containing
/// `authorization_code` AND no service-account link) already exists for
/// the application. Rotate or delete the existing one via the OAuth
/// Clients page before re-provisioning.
#[utoipa::path(
    post,
    path = "/{id}/provision-login-client",
    tag = "applications",
    operation_id = "postApiApplicationsByIdProvisionLoginClient",
    params(
        ("id" = String, Path, description = "Application ID")
    ),
    request_body = ProvisionLoginClientRequest,
    responses(
        (status = 201, description = "Login client provisioned", body = ProvisionLoginClientResponse),
        (status = 400, description = "Validation error"),
        (status = 404, description = "Application not found"),
        (status = 409, description = "Login client already exists")
    ),
    security(("bearer_auth" = []))
)]
pub async fn provision_login_client<U: UnitOfWork>(
    State(state): State<ApplicationsState<U>>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<ProvisionLoginClientRequest>,
) -> Result<Json<ProvisionLoginClientResponse>, PlatformError> {
    use crate::auth::operations::CreateOAuthClientCommand;

    crate::shared::authorization_service::checks::require_anchor(&auth.0)?;

    // Validate the request body before we touch the DB.
    if req.redirect_uris.is_empty() {
        return Err(PlatformError::validation(
            "At least one redirect URI is required",
        ));
    }

    let client_type = match req.client_type.as_deref() {
        Some("CONFIDENTIAL") => OAuthClientType::Confidential,
        _ => OAuthClientType::Public,
    };

    // Pre-validate: app must exist; reject if a login client already exists
    // (one per app — rotate or delete the existing one if you need fresh
    // credentials).
    let app = state
        .application_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("Application", &id))?;
    if app_has_login_client(&state.oauth_client_repo, &app.id).await? {
        return Err(PlatformError::conflict(
            "Application already has a login OAuth client provisioned",
        ));
    }

    let oauth_row_id = crate::TsidGenerator::generate(crate::EntityType::OAuthClient);
    let oauth_public_client_id =
        crate::TsidGenerator::generate(crate::EntityType::OAuthClient);
    let client_name = format!("{} Login", app.name);

    // CONFIDENTIAL clients get a secret at the edge (only confidential
    // clients have one — PUBLIC clients use PKCE alone).
    let (client_secret_plaintext, client_secret_ref) = if client_type
        == OAuthClientType::Confidential
    {
        let (plaintext, encrypted) = generate_and_encrypt_client_secret()?;
        (Some(plaintext), Some(format!("encrypted:{}", encrypted)))
    } else {
        (None, None)
    };

    let cmd = CreateOAuthClientCommand {
        oauth_client_id: oauth_row_id.clone(),
        client_id: oauth_public_client_id.clone(),
        client_name,
        client_type: client_type.as_str().to_string(),
        client_secret_ref,
        redirect_uris: req.redirect_uris.clone(),
        grant_types: vec![GrantType::AuthorizationCode.as_str().to_string()],
        default_scopes: vec![
            "openid".to_string(),
            "profile".to_string(),
            "email".to_string(),
        ],
        pkce_required: client_type == OAuthClientType::Public,
        application_ids: vec![app.id.clone()],
        allowed_origins: req.allowed_origins.clone(),
        service_account_principal_id: None,
        created_by: Some(auth.0.principal_id.clone()),
    };
    let ctx = ExecutionContext::create(auth.0.principal_id.clone());
    state
        .create_oauth_client_use_case
        .run(cmd, ctx)
        .await
        .into_result()?;

    Ok(Json(ProvisionLoginClientResponse {
        message: "Login client provisioned".to_string(),
        login_client: LoginClientCredentialsResponse {
            client_type: client_type.as_str().to_string(),
            oauth_client: OAuthClientCredentials {
                id: oauth_row_id,
                client_id: oauth_public_client_id,
                client_secret: client_secret_plaintext,
            },
            redirect_uris: req.redirect_uris,
        },
    }))
}

/// Check whether the application already has an OAuth client provisioned
/// for the user-login flow (authorization_code grant + NOT linked to a
/// service account). Filters in-memory off `find_by_application` — the
/// list is small per app, so no extra index needed.
async fn app_has_login_client(
    repo: &OAuthClientRepository,
    app_id: &str,
) -> Result<bool, PlatformError> {
    let clients = repo.find_by_application(app_id).await?;
    Ok(clients.iter().any(|c| {
        c.service_account_principal_id.is_none()
            && c.grant_types.contains(&GrantType::AuthorizationCode)
    }))
}

/// Generate a fresh 32-byte client secret and encrypt it for storage.
/// Returns `(plaintext, encrypted_payload)`. The caller wraps the second
/// value as `format!("encrypted:{}", …)` before persistence (matches what
/// the OAuth client API at `oauth_clients_api.rs:226` does).
fn generate_and_encrypt_client_secret() -> Result<(String, String), PlatformError> {
    use base64::Engine;
    let mut secret_bytes = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rand::rng(), &mut secret_bytes);
    let plaintext = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(secret_bytes);

    let enc = crate::shared::encryption_service::EncryptionService::from_env().ok_or_else(
        || {
            PlatformError::internal(
                "FLOWCATALYST_APP_KEY not configured — cannot encrypt client secret",
            )
        },
    )?;
    let encrypted = enc
        .encrypt(&plaintext)
        .map_err(|e| PlatformError::internal(format!("Failed to encrypt client secret: {}", e)))?;
    Ok((plaintext, encrypted))
}

/// Get service account for an application
#[utoipa::path(
    get,
    path = "/{id}/service-account",
    tag = "applications",
    operation_id = "getApiApplicationsByIdServiceAccount",
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
    let app = state
        .application_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("Application", &id))?;

    // Get the service account
    let sa_id = app
        .service_account_id
        .ok_or_else(|| PlatformError::not_found("ServiceAccount", "for application"))?;

    let service_account = state
        .service_account_repo
        .find_by_id(&sa_id)
        .await?
        .ok_or_else(|| PlatformError::not_found("ServiceAccount", &sa_id))?;

    Ok(Json(service_account.into()))
}

/// List roles for an application (admin, by TSID).
///
/// Mounted under a `/by-id` prefix so it doesn't collide with the SDK's
/// `/{appCode}/roles` route. The SDK path takes the application code;
/// this admin path takes the TSID (which the frontend has on hand).
#[utoipa::path(
    get,
    path = "/by-id/{id}/roles",
    tag = "applications",
    operation_id = "getApiApplicationsByIdRoles",
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
    let app = state
        .application_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("Application", &id))?;

    // Find roles by application code
    let roles = state.role_repo.find_by_application(&app.code).await?;

    let response: Vec<ApplicationRoleResponse> = roles.into_iter().map(|r| r.into()).collect();

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
    operation_id = "getApiApplicationsByIdClients",
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
    let app = state
        .application_repo
        .find_by_id(&id)
        .await?
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
    path = "/{id}/clients/{clientId}",
    tag = "applications",
    operation_id = "putApiApplicationsByIdClientsByClientId",
    params(
        ("id" = String, Path, description = "Application ID"),
        ("clientId" = String, Path, description = "Client ID")
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
    state
        .update_client_config_use_case
        .run(cmd, ctx)
        .await
        .into_result()?;

    // Refetch to build the response — the use case returns an event, not the
    // hydrated config, and the response carries the application's effective
    // base URL + client name/identifier alongside the updated config.
    let app = state
        .application_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("Application", &id))?;
    let client = state
        .client_repo
        .find_by_id(&client_id)
        .await?
        .ok_or_else(|| PlatformError::not_found("Client", &client_id))?;
    let config = state
        .client_config_repo
        .find_by_application_and_client(&id, &client_id)
        .await?
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
    path = "/{id}/clients/{clientId}/enable",
    tag = "applications",
    operation_id = "postApiApplicationsByIdClientsByClientIdEnable",
    params(
        ("id" = String, Path, description = "Application ID"),
        ("clientId" = String, Path, description = "Client ID")
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
    state
        .enable_for_client_use_case
        .run(cmd, ctx)
        .await
        .into_result()?;

    build_client_config_response(&state, &id, &client_id).await
}

/// Disable application for a client.
/// Routes through DisableApplicationForClientUseCase (UoW-backed).
#[utoipa::path(
    post,
    path = "/{id}/clients/{clientId}/disable",
    tag = "applications",
    operation_id = "postApiApplicationsByIdClientsByClientIdDisable",
    params(
        ("id" = String, Path, description = "Application ID"),
        ("clientId" = String, Path, description = "Client ID")
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
    state
        .disable_for_client_use_case
        .run(cmd, ctx)
        .await
        .into_result()?;

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
    let app = state
        .application_repo
        .find_by_id(app_id)
        .await?
        .ok_or_else(|| PlatformError::not_found("Application", app_id))?;
    let client = state
        .client_repo
        .find_by_id(client_id)
        .await?
        .ok_or_else(|| PlatformError::not_found("Client", client_id))?;
    let config = state
        .client_config_repo
        .find_by_application_and_client(app_id, client_id)
        .await?
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

/// Create applications router
pub fn applications_router<U: UnitOfWork + Clone>(state: ApplicationsState<U>) -> Router {
    Router::new()
        .route(
            "/",
            post(create_application::<U>).get(list_applications::<U>),
        )
        .route(
            "/{id}",
            get(get_application::<U>)
                .put(update_application::<U>)
                .delete(delete_application::<U>),
        )
        .route("/{id}/activate", post(activate_application::<U>))
        .route("/{id}/deactivate", post(deactivate_application::<U>))
        .route(
            "/{id}/provision-service-account",
            post(provision_service_account::<U>),
        )
        .route(
            "/{id}/provision-login-client",
            post(provision_login_client::<U>),
        )
        .route(
            "/{id}/service-account",
            get(get_application_service_account::<U>),
        )
        .route("/by-id/{id}/roles", get(list_application_roles::<U>))
        .route("/{id}/clients", get(list_client_configs::<U>))
        .route("/{id}/clients/{clientId}", put(update_client_config::<U>))
        .route(
            "/{id}/clients/{clientId}/enable",
            post(enable_for_client::<U>),
        )
        .route(
            "/{id}/clients/{clientId}/disable",
            post(disable_for_client::<U>),
        )
        .route("/by-code/{code}", get(get_application_by_code::<U>))
        .with_state(state)
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
        assert_eq!(
            req.default_base_url,
            Some("https://api.example.com".to_string())
        );
        assert_eq!(
            req.icon_url,
            Some("https://example.com/icon.png".to_string())
        );
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
        assert_eq!(
            req.base_url_override,
            Some("https://custom.example.com".to_string())
        );
        assert!(req.config.is_some());
    }
}
