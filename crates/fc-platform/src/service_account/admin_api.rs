//! Service-account routes Go serves that Rust lacked
//! (`serviceaccount/api/api.go:66,84`):
//!
//! - `POST /api/service-accounts/{id}/deactivate` → 204
//! - `POST /api/service-accounts/{id}/token`      → `{accessToken, tokenType, expiresIn, scope?}`
//!
//! Go's `regenerate-token` / `regenerate-secret` spellings are aliases in
//! `service_accounts_router` itself.
//!
//! Permissions: deactivate is Go's `CanWriteServiceAccounts` plus anchor
//! (owner decision #19). The token mint is anchor-only in Go; it issues a
//! credential, so Rust asks anchor plus `service-account:update`, as for
//! token and secret regeneration (triage S3).

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Serialize;
use std::sync::Arc;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::auth::auth_service::AuthService;
use crate::service_account::operations::mint_token::{
    MintServiceAccountTokenCommand, RecordServiceAccountTokenMintUseCase,
};
use crate::service_account::operations::{
    DeactivateServiceAccountCommand, DeactivateServiceAccountUseCase,
};
use crate::shared::authorization_service::checks;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;
use crate::usecase::{ExecutionContext, PgUnitOfWork, UseCase};
use crate::{PrincipalRepository, RoleRepository, ServiceAccountRepository};

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
    let account_row = sa.service_account_table_id.as_deref().unwrap_or(&sa.id);
    if let Err(e) = state.repo.touch_last_used(account_row).await {
        tracing::warn!(error = %e, "Failed to stamp service account last_used_at");
    }

    state
        .record_mint_use_case
        .run(
            MintServiceAccountTokenCommand {
                service_account_id: sa.id.clone(),
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

/// Full-path router; merged at the root.
pub fn service_account_admin_router(state: ServiceAccountAdminState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(deactivate_service_account))
        .routes(routes!(mint_service_account_token))
        .with_state(state)
}
