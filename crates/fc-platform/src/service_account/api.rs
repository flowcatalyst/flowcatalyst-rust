//! Service Accounts Admin API
//!
//! REST endpoints for service account management.
//! Base path: /api/service-accounts

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::auth::auth_service::AuthService;
use crate::service_account::entity::{SigningAlgorithm, WebhookAuthType, WebhookCredentials};
use crate::service_account::operations::mint_token::{
    MintServiceAccountTokenCommand, RecordServiceAccountTokenMintUseCase,
};
use crate::service_account::operations::{
    AssignRolesCommand, AssignRolesUseCase, CreateServiceAccountCommand,
    CreateServiceAccountUseCase, DeleteServiceAccountCommand, DeleteServiceAccountUseCase,
    RegenerateAuthTokenCommand, RegenerateAuthTokenUseCase, RegenerateSigningSecretCommand,
    RegenerateSigningSecretUseCase, UpdateServiceAccountCommand, UpdateServiceAccountUseCase,
};
use crate::service_account::operations::{
    DeactivateServiceAccountCommand, DeactivateServiceAccountUseCase,
};
use crate::shared::authorization_service::checks;
use crate::shared::enum_str::{non_empty, parse_opt};
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;
use crate::usecase::PgUnitOfWork;
use crate::usecase::{ExecutionContext, UnitOfWork, UseCase};
use crate::ServiceAccount;
use crate::ServiceAccountRepository;
use crate::{PrincipalRepository, RoleRepository};

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

    /// Requested scope: `ANCHOR`, `PARTNER` or `CLIENT`, stored as sent;
    /// anything else is a 400 (X-06). As in Go, the token tier doesn't follow
    /// it but `clientIds`: none → ANCHOR, one → CLIENT, several → PARTNER.
    #[serde(default)]
    pub scope: Option<String>,

    /// Client IDs this account can access
    #[serde(default)]
    pub client_ids: Vec<String>,

    /// Not accepted: an account made here starts with no application access
    /// and is granted applications afterwards
    /// (`PUT /api/principals/{id}/application-access`). Only application
    /// provisioning binds an account to an application. Present, it is a 400
    /// rather than silently ignored.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application_id: Option<String>,

    /// `true` grants the account every application, present and future (Go's
    /// `allApplications`). Omitted or `false`: no application access. Only a
    /// caller that itself reaches every application may ask for it (403);
    /// alongside `applicationId` it is a 400
    /// `ALL_APPLICATIONS_WITH_APPLICATION_ID`.
    #[serde(default)]
    pub all_applications: Option<bool>,

    /// Go's `webhookCredentials`: its `authType` must be a known type (400
    /// `INVALID_AUTH_TYPE` otherwise); the account is still created with
    /// generated bearer credentials, as Go creates it.
    #[serde(default)]
    pub webhook_credentials: Option<WebhookCredentialsRequest>,
}

/// Go `WebhookCredentialsDTO`: how the platform authenticates the
/// account's outbound webhooks. Every member is write-only: no read answers
/// them (a service account read carries `authType` alone), and `token`,
/// `password` and `signingSecret` are stored as `encrypted:` references and
/// redacted from the audit log. A create reads only `authType` (the account
/// is created with generated credentials, as Go creates it); an update
/// replaces the account's credentials with these, as Go's does, so a member
/// left out is cleared.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = WebhookCredentialsDTO)]
pub struct WebhookCredentialsRequest {
    /// `NONE`, `BEARER_TOKEN`, `BASIC_AUTH`, `API_KEY` or `HMAC_SIGNATURE`;
    /// blank is `NONE`, anything else a 400 `INVALID_AUTH_TYPE`.
    #[serde(default)]
    #[schema(required = true)]
    pub auth_type: String,
    /// The bearer token (or API key) the webhooks carry.
    #[serde(default)]
    pub token: Option<String>,
    /// The basic-auth user name.
    #[serde(default)]
    pub username: Option<String>,
    /// The basic-auth password.
    #[serde(default)]
    pub password: Option<String>,
    /// The header the API key travels in.
    #[serde(default)]
    pub header_name: Option<String>,
    /// The HMAC key the webhooks are signed with.
    #[serde(default)]
    pub signing_secret: Option<String>,
    /// `HMAC_SHA256` (`SHA256` is read as it); anything else a 400
    /// `INVALID_SIGNING_ALGORITHM`.
    #[serde(default)]
    pub signing_algorithm: Option<String>,
    /// The header the signature travels in.
    #[serde(default)]
    pub signature_header: Option<String>,
}

impl WebhookCredentialsRequest {
    /// The authentication type (Go `ParseAuthType`: blank is `NONE`, an
    /// unknown one is refused, never coerced).
    fn parse_auth_type(&self) -> Result<WebhookAuthType, PlatformError> {
        if self.auth_type.is_empty() {
            return Ok(WebhookAuthType::None);
        }
        self.auth_type.parse().map_err(|_| {
            PlatformError::bad_request_code(
                "INVALID_AUTH_TYPE",
                format!("unknown webhook auth type {:?}", self.auth_type),
            )
        })
    }

    /// The credentials as sent, in plaintext (the update use case seals the
    /// secrets). A blank member is an absent one.
    pub fn to_credentials(&self) -> Result<WebhookCredentials, PlatformError> {
        let text = |v: &Option<String>| {
            v.as_deref()
                .and_then(|s| non_empty(Some(s)))
                .map(String::from)
        };
        let signing_algorithm = match non_empty(self.signing_algorithm.as_deref()) {
            None => None,
            Some(alg) => Some(alg.parse::<SigningAlgorithm>().map_err(|_| {
                PlatformError::bad_request_code(
                    "INVALID_SIGNING_ALGORITHM",
                    format!("unknown webhook signing algorithm {alg:?}"),
                )
            })?),
        };
        Ok(WebhookCredentials {
            auth_type: self.parse_auth_type()?,
            token: text(&self.token),
            username: text(&self.username),
            password: text(&self.password),
            header_name: text(&self.header_name),
            signing_secret: text(&self.signing_secret),
            signing_algorithm,
            signature_header: text(&self.signature_header),
        })
    }
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

    /// Updated requested scope (`ANCHOR`, `PARTNER`, `CLIENT`), stored as
    /// sent; anything else is a 400. It doesn't move the token tier.
    #[serde(default)]
    pub scope: Option<String>,

    /// Updated client IDs. The token tier follows them, as in Go.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_ids: Option<Vec<String>>,

    /// Replaces the account's webhook credentials (Go's
    /// `webhookCredentials`); absent leaves them as they are.
    #[serde(default)]
    pub webhook_credentials: Option<WebhookCredentialsRequest>,
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
    #[schema(value_type = i64)]
    pub total: usize,
}

/// Service account response DTO
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccountResponse {
    /// The account's own id (`iam_service_accounts.id`, Go's `id`); an
    /// account written before the two ids were split answers with its
    /// principal's, which is the same value there.
    pub id: String,
    pub code: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub active: bool,
    pub client_ids: Vec<String>,
    /// The requested scope as stored (Go's shape: omitted when none was
    /// requested). The token tier follows `clientIds`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application_id: Option<String>,
    pub auth_type: String,
    pub roles: Vec<String>,
    /// The linked SERVICE principal (roles, application access): on the
    /// single-account read only, as Go.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub principal_id: Option<String>,
    /// The public client_id of the account's earliest OAuth client: on the
    /// single-account read only, as Go.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth_client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(format = DateTime)]
    pub last_used_at: Option<String>,
    #[schema(format = DateTime)]
    pub created_at: String,
    #[schema(format = DateTime)]
    pub updated_at: String,
}

impl From<ServiceAccount> for ServiceAccountResponse {
    fn from(sa: ServiceAccount) -> Self {
        Self {
            id: sa.service_account_table_id.unwrap_or(sa.id),
            code: sa.code,
            name: sa.name,
            description: sa.description,
            active: sa.active,
            client_ids: sa.client_ids,
            scope: sa.requested_scope,
            application_id: sa.application_id,
            auth_type: sa.webhook_credentials.auth_type.as_str().to_string(),
            roles: sa.roles.iter().map(|r| r.role.clone()).collect(),
            principal_id: None,
            oauth_client_id: None,
            last_used_at: sa.last_used_at.map(|t| t.to_rfc3339()),
            created_at: sa.created_at.to_rfc3339(),
            updated_at: sa.updated_at.to_rfc3339(),
        }
    }
}

/// OAuth credentials (one-time, shown only at creation)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = ServiceAccountOAuthSecrets)]
pub struct OAuthCredentials {
    pub client_id: String,
    pub client_secret: String,
}

/// Webhook credentials (one-time, shown only at creation)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = ServiceAccountWebhookSecrets)]
pub struct WebhookCredentialsResponse {
    pub auth_token: String,
    pub signing_secret: String,
}

/// Create service account response (includes one-time secrets)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateServiceAccountResponse {
    pub service_account: ServiceAccountResponse,
    /// The linked SERVICE principal (Go's `principalId`).
    pub principal_id: String,
    pub oauth: OAuthCredentials,
    pub webhook: WebhookCredentialsResponse,
}

/// Regenerate token response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = RegenerateAuthTokenResponse)]
pub struct RegenerateTokenResponse {
    /// The service account's id (Go `RegenerateTokenResponse.id`)
    pub id: String,
    /// New auth token (shown only once)
    #[schema(required = false)]
    pub auth_token: String,
}

/// Regenerate secret response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = RegenerateSigningSecretResponse)]
pub struct RegenerateSecretResponse {
    /// The service account's id (Go `RegenerateSecretResponse.id`)
    pub id: String,
    /// New signing secret (shown only once)
    #[schema(required = false)]
    pub signing_secret: String,
}

/// Role assignment response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = RoleAssignmentDTO)]
pub struct RoleAssignmentResponse {
    pub role_name: String,
    /// The client the role is confined to. Role grants here are not
    /// client-scoped (a role applies wherever the account reaches), so it is
    /// always absent; Go never fills it either.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// Omitted when not recorded, as in Go (serviceaccount/api/dto.go:51).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignment_source: Option<String>,
    #[schema(format = DateTime)]
    pub assigned_at: String,
    /// The principal who assigned the role. Go documents it but never
    /// stores it; here it is recorded for roles an administrator assigns or
    /// provisioning grants, and absent for synced roles and older rows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assigned_by: Option<String>,
}

impl From<&crate::service_account::entity::RoleAssignment> for RoleAssignmentResponse {
    fn from(r: &crate::service_account::entity::RoleAssignment) -> Self {
        Self {
            role_name: r.role.clone(),
            client_id: r.client_id.clone(),
            assignment_source: r.assignment_source.map(|s| s.as_str().to_string()),
            assigned_at: r.assigned_at.to_rfc3339(),
            assigned_by: r.assigned_by.clone(),
        }
    }
}

/// Roles response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = ServiceAccountRoleListResponse)]
pub struct RolesResponse {
    pub roles: Vec<RoleAssignmentResponse>,
}

/// Assign roles response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = ServiceAccountRolesAssignedResponse)]
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
    /// Role definitions, for the role ceiling.
    pub role_repo: Arc<crate::RoleRepository>,
    pub create_use_case: Arc<CreateServiceAccountUseCase<U>>,
    pub update_use_case: Arc<UpdateServiceAccountUseCase<U>>,
    pub delete_use_case: Arc<DeleteServiceAccountUseCase<U>>,
    pub assign_roles_use_case: Arc<AssignRolesUseCase<U>>,
    pub regenerate_token_use_case: Arc<RegenerateAuthTokenUseCase<U>>,
    pub regenerate_secret_use_case: Arc<RegenerateSigningSecretUseCase<U>>,
    pub create_oauth_client_use_case: Arc<crate::auth::operations::CreateOAuthClientUseCase<U>>,
    /// The account's OAuth client, for `oauthClientId` on the detail read.
    pub oauth_client_repo: Arc<crate::OAuthClientRepository>,
    /// The caller's application scope, for the `allApplications` opt-in.
    pub app_access: Arc<crate::shared::authorization_service::ApplicationAccessService>,
}

// ============================================================================
// Endpoints
// ============================================================================

/// List service accounts
#[utoipa::path(
    get,
    path = "",
    tag = "service-accounts",
    operation_id = "listServiceAccounts",
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
    auth: Authenticated,
    Query(query): Query<ServiceAccountsQuery>,
) -> Result<Json<ServiceAccountListResponse>, PlatformError> {
    crate::checks::can_read_service_accounts(&auth.0)?;
    // Go lists every account, active or not, ordered by code; the filters
    // are Rust's own narrowing of that list.
    let mut accounts = if let Some(client_id) = query.client_id {
        state.repo.find_by_client(&client_id).await?
    } else if let Some(app_id) = query.application_id {
        state.repo.find_by_application(&app_id).await?
    } else {
        state.repo.find_all().await?
    };

    if let Some(is_active) = query.active {
        accounts.retain(|a| a.active == is_active);
    }
    accounts.sort_by(|a, b| a.code.cmp(&b.code));

    let total = accounts.len();
    // Go's list rows carry no roles (its list read does not hydrate them);
    // the single-account read does.
    let service_accounts: Vec<ServiceAccountResponse> = accounts
        .into_iter()
        .map(|mut a| {
            a.roles.clear();
            ServiceAccountResponse::from(a)
        })
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
    operation_id = "getServiceAccount",
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
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<ServiceAccountResponse>, PlatformError> {
    crate::checks::can_read_service_accounts(&auth.0)?;
    let account = state
        .repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::ServiceAccountNotFound { id: id.clone() })?;

    // Go getByID: the linked principal, and the public client_id of its
    // earliest OAuth client (by created_at, then id).
    let principal_id = account.id.clone();
    let mut clients = state
        .oauth_client_repo
        .find_by_service_account_principal_id(&principal_id)
        .await?;
    clients.sort_by(|a, b| (a.created_at, &a.id).cmp(&(b.created_at, &b.id)));
    let mut response = ServiceAccountResponse::from(account);
    response.principal_id = Some(principal_id);
    response.oauth_client_id = clients.into_iter().next().map(|c| c.client_id);
    Ok(Json(response))
}

/// Get service account by code
#[utoipa::path(
    get,
    path = "/code/{code}",
    tag = "service-accounts",
    operation_id = "getServiceAccountByCode",
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
    auth: Authenticated,
    Path(code): Path<String>,
) -> Result<Json<ServiceAccountResponse>, PlatformError> {
    crate::checks::can_read_service_accounts(&auth.0)?;
    let account = state
        .repo
        .find_by_code(&code)
        .await?
        .ok_or_else(|| PlatformError::ServiceAccountNotFound { id: code.clone() })?;

    Ok(Json(ServiceAccountResponse::from(account)))
}

/// Create service account
#[utoipa::path(
    post,
    path = "",
    tag = "service-accounts",
    operation_id = "createServiceAccount",
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
) -> Result<(StatusCode, Json<CreateServiceAccountResponse>), PlatformError> {
    crate::checks::can_write_service_accounts(&auth.0)?;
    // Go `WebhookCredentialsDTO.toEntity`: an unknown type is refused, never
    // coerced (empty means none). The rest of the members are not used: the
    // account is created with generated credentials, as Go creates it.
    if let Some(creds) = &req.webhook_credentials {
        creds.parse_auth_type()?;
    }
    let all_applications = req.all_applications == Some(true);
    // Go serviceaccount/api/api.go:155-159: only a caller that itself holds
    // all-applications access may grant it. Checked here, where Go checks
    // it (before the body's other rules), and again by the use case.
    let application_scope = if all_applications {
        let scope = state.app_access.scope_for(&auth.0.principal_id).await?;
        crate::checks::require_all_applications_grantor(Some(&scope))?;
        Some(scope)
    } else {
        None
    };
    if all_applications && req.application_id.is_some() {
        return Err(PlatformError::bad_request_code(
            "ALL_APPLICATIONS_WITH_APPLICATION_ID",
            "allApplications cannot be combined with applicationId",
        ));
    }
    if req.application_id.is_some() {
        return Err(PlatformError::validation(
            "applicationId is not accepted: a new service account has no application \
             access; grant applications afterwards, or provision one from the application",
        ));
    }
    let command = CreateServiceAccountCommand {
        code: req.code,
        name: req.name,
        description: req.description,
        scope: parse_opt(req.scope.as_deref())?,
        client_ids: req.client_ids,
        application_id: None,
        all_applications,
    };

    let mut ctx = ExecutionContext::from_auth(&auth.0);
    if let Some(scope) = application_scope {
        ctx = ctx.with_application_scope(scope);
    }

    match state.create_use_case.run(command, ctx).await.into_result() {
        Ok(result) => {
            let account = state
                .repo
                .find_by_id(&result.principal_id)
                .await?
                .ok_or_else(|| PlatformError::internal("Created service account not found"))?;

            // Auto-provision a CONFIDENTIAL OAuth client for this service account.
            // Plaintext secret stays in this handler — only the hashed ref
            // crosses into the use case.
            use base64::Engine;

            let oauth_client_id = crate::shared::tsid::generate(crate::EntityType::OAuthClient);
            let mut secret_bytes = [0u8; 32];
            rand::RngCore::fill_bytes(&mut rand::rng(), &mut secret_bytes);
            let plaintext_secret =
                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(secret_bytes);

            let enc = crate::shared::encryption_service::EncryptionService::from_env().ok_or_else(
                || {
                    PlatformError::internal(
                        "FLOWCATALYST_APP_KEY not configured — cannot hash client secret",
                    )
                },
            )?;

            let oauth_cmd = crate::auth::operations::CreateOAuthClientCommand {
                oauth_client_id: oauth_client_id.clone(),
                client_id: oauth_client_id.clone(),
                // Go: "<name> Client" (create_credentials.go:132)
                client_name: format!("{} Client", account.name),
                client_type: crate::auth::oauth_entity::OAuthClientType::Confidential,
                client_secret_ref: Some(enc.hash_secret(&plaintext_secret)),
                redirect_uris: vec![],
                post_logout_redirect_uris: vec![],
                // create_credentials.go:135-136
                grant_types: vec![
                    crate::auth::oauth_entity::GrantType::ClientCredentials,
                    crate::auth::oauth_entity::GrantType::RefreshToken,
                ],
                default_scopes: vec!["openid".to_string()],
                // Go's entity default (auth.NewOAuthClient).
                pkce_required: true,
                application_ids: vec![],
                allowed_origins: vec![],
                service_account_principal_id: Some(result.principal_id.clone()),
                created_by: Some(auth.0.principal_id.clone()),
                portal_client_id: None,
                portal_app_id: None,
                api_access: false,
            };
            let oauth_ctx = ExecutionContext::from_auth(&auth.0);
            state
                .create_oauth_client_use_case
                .run(oauth_cmd, oauth_ctx)
                .await
                .into_result()?;

            // 201, as Go answers (serviceaccount/api/api.go:62).
            Ok((
                StatusCode::CREATED,
                Json(CreateServiceAccountResponse {
                    principal_id: account.id.clone(),
                    service_account: ServiceAccountResponse::from(account),
                    oauth: OAuthCredentials {
                        client_id: oauth_client_id,
                        client_secret: plaintext_secret,
                    },
                    webhook: WebhookCredentialsResponse {
                        auth_token: result.auth_token,
                        signing_secret: result.signing_secret,
                    },
                }),
            ))
        }
        Err(err) => Err(err.into()),
    }
}

/// Update service account
#[utoipa::path(
    put,
    path = "/{id}",
    tag = "service-accounts",
    operation_id = "updateServiceAccount",
    params(
        ("id" = String, Path, description = "Service account ID")
    ),
    request_body = UpdateServiceAccountRequest,
    responses(
        (status = 204, description = "Service account updated"),
        (status = 404, description = "Service account not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_service_account<U: UnitOfWork>(
    State(state): State<ServiceAccountsState<U>>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<UpdateServiceAccountRequest>,
) -> Result<StatusCode, PlatformError> {
    crate::checks::can_write_service_accounts(&auth.0)?;
    let webhook_credentials = req
        .webhook_credentials
        .as_ref()
        .map(WebhookCredentialsRequest::to_credentials)
        .transpose()?;
    let command = UpdateServiceAccountCommand {
        id: id.clone(),
        name: req.name,
        description: req.description,
        scope: parse_opt(req.scope.as_deref())?,
        client_ids: req.client_ids,
        webhook_credentials,
    };

    let ctx = ExecutionContext::from_auth(&auth.0);

    match state.update_use_case.run(command, ctx).await.into_result() {
        Ok(_event) => Ok(StatusCode::NO_CONTENT),
        Err(err) => Err(err.into()),
    }
}

/// Delete service account
#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "service-accounts",
    operation_id = "deleteServiceAccount",
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
    crate::checks::can_delete_service_accounts(&auth.0)?;
    let command = DeleteServiceAccountCommand { id };

    let ctx = ExecutionContext::from_auth(&auth.0);

    match state.delete_use_case.run(command, ctx).await.into_result() {
        Ok(_) => Ok(StatusCode::NO_CONTENT),
        Err(err) => Err(err.into()),
    }
}

/// Update auth token (regenerate via PUT)
#[utoipa::path(
    put,
    path = "/{id}/auth-token",
    tag = "service-accounts",
    operation_id = "putApiServiceAccountsByIdAuthToken",
    params(
        ("id" = String, Path, description = "Service account ID")
    ),
    responses(
        (status = 200, description = "Token regenerated", body = RegenerateTokenResponse),
        (status = 404, description = "Service account not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_auth_token<U: UnitOfWork>(
    State(state): State<ServiceAccountsState<U>>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<RegenerateTokenResponse>, PlatformError> {
    crate::checks::can_update_service_accounts(&auth.0)?;
    let command = RegenerateAuthTokenCommand {
        service_account_id: id.clone(),
    };

    let ctx = ExecutionContext::from_auth(&auth.0);

    match state
        .regenerate_token_use_case
        .run(command, ctx)
        .await
        .into_result()
    {
        Ok(result) => Ok(Json(RegenerateTokenResponse {
            id,
            auth_token: result.auth_token,
        })),
        Err(err) => Err(err.into()),
    }
}

/// Regenerate auth token
#[utoipa::path(
    post,
    path = "/{id}/regenerate-auth-token",
    tag = "service-accounts",
    operation_id = "regenerateServiceAccountAuthToken_regenerate-auth-token",
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
    crate::checks::can_update_service_accounts(&auth.0)?;
    let command = RegenerateAuthTokenCommand {
        service_account_id: id.clone(),
    };

    let ctx = ExecutionContext::from_auth(&auth.0);

    match state
        .regenerate_token_use_case
        .run(command, ctx)
        .await
        .into_result()
    {
        Ok(result) => Ok(Json(RegenerateTokenResponse {
            id,
            auth_token: result.auth_token,
        })),
        Err(err) => Err(err.into()),
    }
}

/// Regenerate signing secret
#[utoipa::path(
    post,
    path = "/{id}/regenerate-signing-secret",
    tag = "service-accounts",
    operation_id = "regenerateServiceAccountSigningSecret_regenerate-signing-secret",
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
    crate::checks::can_update_service_accounts(&auth.0)?;
    let command = RegenerateSigningSecretCommand {
        service_account_id: id.clone(),
    };

    let ctx = ExecutionContext::from_auth(&auth.0);

    match state
        .regenerate_secret_use_case
        .run(command, ctx)
        .await
        .into_result()
    {
        Ok(result) => Ok(Json(RegenerateSecretResponse {
            id,
            signing_secret: result.signing_secret,
        })),
        Err(err) => Err(err.into()),
    }
}

/// Go's shorter spelling of `regenerate-auth-token`, served by
/// [`regenerate_auth_token`] (see `service_accounts_router`). Documentation
/// only: Go documents each spelling as its own operation.
#[utoipa::path(
    post,
    path = "/{id}/regenerate-token",
    tag = "service-accounts",
    operation_id = "regenerateServiceAccountAuthToken_regenerate-token",
    params(
        ("id" = String, Path, description = "Service account ID")
    ),
    responses(
        (status = 200, description = "Token regenerated", body = RegenerateTokenResponse),
        (status = 404, description = "Service account not found")
    ),
    security(("bearer_auth" = []))
)]
#[allow(dead_code)]
pub(crate) fn regenerate_token_alias() {}

/// Go's shorter spelling of `regenerate-signing-secret`, served by
/// [`regenerate_signing_secret`]. Documentation only.
#[utoipa::path(
    post,
    path = "/{id}/regenerate-secret",
    tag = "service-accounts",
    operation_id = "regenerateServiceAccountSigningSecret_regenerate-secret",
    params(
        ("id" = String, Path, description = "Service account ID")
    ),
    responses(
        (status = 200, description = "Secret regenerated", body = RegenerateSecretResponse),
        (status = 404, description = "Service account not found")
    ),
    security(("bearer_auth" = []))
)]
#[allow(dead_code)]
pub(crate) fn regenerate_secret_alias() {}

/// Get assigned roles
#[utoipa::path(
    get,
    path = "/{id}/roles",
    tag = "service-accounts",
    operation_id = "listServiceAccountRoles",
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
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<RolesResponse>, PlatformError> {
    crate::checks::can_read_service_accounts(&auth.0)?;
    let account = state
        .repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::ServiceAccountNotFound { id: id.clone() })?;

    let roles: Vec<RoleAssignmentResponse> = account
        .roles
        .iter()
        .map(RoleAssignmentResponse::from)
        .collect();

    Ok(Json(RolesResponse { roles }))
}

/// Assign roles (declarative - replaces all)
#[utoipa::path(
    put,
    path = "/{id}/roles",
    tag = "service-accounts",
    operation_id = "assignServiceAccountRoles",
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
    crate::checks::can_update_service_accounts(&auth.0)?;
    let command = AssignRolesCommand {
        service_account_id: id.clone(),
        roles: req.roles,
    };

    let ctx = ExecutionContext::from_auth(&auth.0);

    match state
        .assign_roles_use_case
        .run(command, ctx)
        .await
        .into_result()
    {
        Ok(event) => {
            // Fetch updated account to get role details
            let account = state
                .repo
                .find_by_id(&id)
                .await?
                .ok_or_else(|| PlatformError::ServiceAccountNotFound { id })?;

            let roles: Vec<RoleAssignmentResponse> = account
                .roles
                .iter()
                .map(RoleAssignmentResponse::from)
                .collect();

            Ok(Json(AssignRolesResponse {
                roles,
                added_roles: event.roles_added,
                removed_roles: event.roles_removed,
            }))
        }
        Err(err) => Err(err.into()),
    }
}

// ============================================================================
// Router
// ============================================================================

// ─── Go-parity admin routes (formerly admin_api.rs) ───────────────────────────
//
// Service-account routes Go serves that Rust lacked
// (`serviceaccount/api/api.go:66,84`):
//
// - `POST /api/service-accounts/{id}/deactivate` → 204
// - `POST /api/service-accounts/{id}/token`      → `{accessToken, tokenType, expiresIn, scope?}`
//
// Go's `regenerate-token` / `regenerate-secret` spellings are aliases in
// `service_accounts_router` itself.
//
// Permissions: deactivate is Go's `CanWriteServiceAccounts` plus anchor
// (owner decision #19). The token mint is anchor-only in Go; it issues a
// credential, so Rust asks anchor plus `service-account:update`, as for
// token and secret regeneration (triage S3).

#[derive(Clone)]
pub struct ServiceAccountAdminState {
    pub repo: Arc<ServiceAccountRepository>,
    pub principal_repo: Arc<PrincipalRepository>,
    pub role_repo: Arc<RoleRepository>,
    pub auth_service: Arc<AuthService>,
    pub deactivate_use_case: Arc<DeactivateServiceAccountUseCase<PgUnitOfWork>>,
    pub record_mint_use_case: Arc<RecordServiceAccountTokenMintUseCase<PgUnitOfWork>>,
}

/// Go `MintTokenResponse`.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = ServiceAccountTokenResponse)]
pub struct MintTokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

fn inactive(message: &str) -> PlatformError {
    PlatformError::bad_request_code("SERVICE_ACCOUNT_INACTIVE", message)
}

/// Deactivate a service account (Go `deactivateServiceAccount`).
/// Idempotent; the linked principal and OAuth client are untouched.
#[utoipa::path(
    post,
    path = "/api/service-accounts/{id}/deactivate",
    tag = "service-accounts",
    operation_id = "deactivateServiceAccount",
    params(("id" = String, Path, description = "Service account ID")),
    responses(
        (status = 204, description = "Deactivated"),
        (status = 404, description = "Service account not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn deactivate_service_account(
    State(state): State<ServiceAccountAdminState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<StatusCode, PlatformError> {
    checks::can_write_service_accounts(&auth.0)?;
    state
        .deactivate_use_case
        .run(
            DeactivateServiceAccountCommand { id },
            ExecutionContext::from_auth(&auth.0),
        )
        .await
        .into_result()?;
    Ok(StatusCode::NO_CONTENT)
}

/// Mint an access token for a service account, as the `client_credentials`
/// grant would for its OAuth client (Go `mintServiceAccountToken`): the
/// linked principal's roles, `scope` = its flattened permissions.
#[utoipa::path(
    post,
    path = "/api/service-accounts/{id}/token",
    tag = "service-accounts",
    operation_id = "mintServiceAccountToken",
    params(("id" = String, Path, description = "Service account ID")),
    responses(
        (status = 200, description = "Token minted", body = MintTokenResponse),
        (status = 400, description = "The service account or its principal is deactivated"),
        (status = 404, description = "Service account not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn mint_service_account_token(
    State(state): State<ServiceAccountAdminState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<MintTokenResponse>, PlatformError> {
    checks::can_update_service_accounts(&auth.0)?;
    let sa = state
        .repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found_code("ServiceAccount", &id))?;
    if !sa.active {
        return Err(inactive(
            "the service account is deactivated — reactivate it before minting a token",
        ));
    }
    // The account's `id` is its SERVICE principal's.
    let principal = state
        .principal_repo
        .find_by_id(&sa.id)
        .await?
        .ok_or_else(|| PlatformError::Coded {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "PRINCIPAL".to_string(),
            message: "service account has no linked principal".to_string(),
            details: Default::default(),
        })?;
    if !principal.active {
        return Err(inactive("the service account's principal is deactivated"));
    }
    let granted = state
        .role_repo
        .flatten_permissions(&crate::auth::auth_service::role_names(&principal))
        .await?;
    let access_token = state
        .auth_service
        .generate_access_token_with_scope(&principal, &granted, None)?;
    // Go stamps the account's last_used_at on a mint too ("handing out a
    // bearer is a use"); best-effort bookkeeping.
    let account_row = sa.account_id();
    if let Err(e) = state.repo.touch_last_used(account_row).await {
        tracing::warn!(error = %e, "Failed to stamp service account last_used_at");
    }

    state
        .record_mint_use_case
        .run(
            MintServiceAccountTokenCommand {
                service_account_id: sa.account_id().to_string(),
                code: sa.code.clone(),
            },
            ExecutionContext::from_auth(&auth.0),
        )
        .await
        .into_result()?;

    Ok(Json(MintTokenResponse {
        access_token,
        token_type: "Bearer".to_string(),
        expires_in: state.auth_service.access_token_expiry_secs(),
        scope: (!granted.is_empty()).then(|| granted.join(" ")),
    }))
}
