//! Application routes Go serves that Rust lacked (`application/api/api.go:53,60`):
//!
//! - `POST /api/applications/{id}/service-account` → 204: attach an existing
//!   service account (`{serviceAccountId, serviceAccountCode}`)
//! - `GET  /api/applications/{id}/clients/{clientId}` → the client config

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::application::operations::attach_service_account::{
    AttachServiceAccountToApplicationCommand, AttachServiceAccountToApplicationUseCase,
};
use crate::application::ApplicationClientConfigRepository;
use crate::shared::authorization_service::checks;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;
use crate::usecase::{ExecutionContext, PgUnitOfWork, UseCase};
use crate::PrincipalRepository;

#[derive(Clone)]
pub struct ApplicationGoState {
    pub principal_repo: Arc<PrincipalRepository>,
    pub client_config_repo: Arc<ApplicationClientConfigRepository>,
    pub attach_use_case: Arc<AttachServiceAccountToApplicationUseCase<PgUnitOfWork>>,
}

/// Go `AttachServiceAccountRequest`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AttachServiceAccountRequest {
    #[serde(default)]
    pub service_account_id: String,
    #[serde(default)]
    pub service_account_code: String,
}

/// Go `ClientConfigResponse` (its query never selects the base-URL override
/// or the config JSON, so neither appears).
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GoClientConfigResponse {
    pub id: String,
    pub application_id: String,
    pub client_id: String,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// Attach an existing service account (Go `attachApplicationServiceAccount`).
/// The application stores the account's principal id (the column is a
/// foreign key to `iam_principals`). Go asks anchor scope alone; Rust asks
/// anchor and `application:update`, as for provisioning one (Java S1.2).
#[utoipa::path(
    post,
    path = "/api/applications/{id}/service-account",
    tag = "applications",
    operation_id = "attachApplicationServiceAccount",
    params(("id" = String, Path, description = "Application id")),
    request_body = AttachServiceAccountRequest,
    responses(
        (status = 204, description = "Attached"),
        (status = 404, description = "Unknown application or service account"),
        (status = 409, description = "The application already has one")
    ),
    security(("bearer_auth" = []))
)]
pub async fn attach_application_service_account(
    State(state): State<ApplicationGoState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<AttachServiceAccountRequest>,
) -> Result<StatusCode, PlatformError> {
    checks::require_anchor_scope(&auth.0)?;
    checks::require_permission(&auth.0, crate::permissions::admin::APPLICATION_UPDATE)?;
    let sa_id = req.service_account_id.trim().to_string();
    // Go validates the ids before resolving the principal.
    let principal_id = if sa_id.is_empty() || id.trim().is_empty() {
        sa_id.clone()
    } else {
        state
            .principal_repo
            .find_by_service_account(&sa_id)
            .await?
            .ok_or_else(|| PlatformError::not_found_code("ServiceAccountPrincipal", &sa_id))?
            .id
    };
    state
        .attach_use_case
        .run(
            AttachServiceAccountToApplicationCommand {
                application_id: id,
                service_account_id: principal_id,
                service_account_code: req.service_account_code,
            },
            ExecutionContext::from_auth(&auth.0),
        )
        .await
        .into_result()?;
    Ok(StatusCode::NO_CONTENT)
}

/// One client's config for an application (Go `getApplicationClientConfig`).
#[utoipa::path(
    get,
    path = "/api/applications/{id}/clients/{clientId}",
    tag = "applications",
    operation_id = "getApplicationClientConfig",
    params(
        ("id" = String, Path, description = "Application id"),
        ("clientId" = String, Path, description = "Client id")
    ),
    responses(
        (status = 200, description = "The config", body = GoClientConfigResponse),
        (status = 404, description = "No config for the pair")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_application_client_config(
    State(state): State<ApplicationGoState>,
    auth: Authenticated,
    Path((id, client_id)): Path<(String, String)>,
) -> Result<Json<GoClientConfigResponse>, PlatformError> {
    checks::require_permission(&auth.0, crate::permissions::admin::APPLICATION_READ)?;
    let c = state
        .client_config_repo
        .find_by_application_and_client(&id, &client_id)
        .await?
        .ok_or_else(|| {
            PlatformError::not_found_code("ClientConfig", format!("{id}:{client_id}"))
        })?;
    Ok(Json(c.into()))
}

impl From<crate::application::ApplicationClientConfig> for GoClientConfigResponse {
    fn from(c: crate::application::ApplicationClientConfig) -> Self {
        Self {
            id: c.id,
            application_id: c.application_id,
            client_id: c.client_id,
            enabled: c.enabled,
            created_at: c.created_at.to_rfc3339(),
            updated_at: c.updated_at.to_rfc3339(),
        }
    }
}

/// Full-path router; merged at the root.
pub fn application_go_router(state: ApplicationGoState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(attach_application_service_account))
        .routes(routes!(get_application_client_config))
        .with_state(state)
}
