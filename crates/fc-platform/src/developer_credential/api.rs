//! `/api/principals/developer-users` and
//! `/api/principals/{id}/developer-credential` (Go principal/api/api.go:
//! `listDeveloperUsers`, `setDeveloperCredential`,
//! `revokeDeveloperCredential`).

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use base64::Engine;
use rand::Rng;
use serde::Serialize;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::operations::{
    RevokeDeveloperCredentialCommand, RevokeDeveloperCredentialUseCase,
    SetDeveloperCredentialCommand, SetDeveloperCredentialUseCase,
};
use super::DEVELOPER_ROLE;
use crate::shared::authorization_service::{checks, AuthContext};
use crate::shared::encryption_service::EncryptionService;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;
use crate::usecase::{ExecutionContext, PgUnitOfWork, UseCase};
use crate::{Principal, PrincipalRepository, UserScope};

#[derive(Clone)]
pub struct DeveloperCredentialsState {
    pub principal_repo: Arc<PrincipalRepository>,
    pub set_use_case: Arc<SetDeveloperCredentialUseCase<PgUnitOfWork>>,
    pub revoke_use_case: Arc<RevokeDeveloperCredentialUseCase<PgUnitOfWork>>,
    /// Hashes the secret (`hashed:v1:`); none: a credential can't be set.
    pub encryption: Option<Arc<EncryptionService>>,
}

/// Go `SetDeveloperCredentialResponse`: the plaintext secret, once.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetDeveloperCredentialResponse {
    pub id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub client_secret: String,
}

/// Go `DeveloperUserListResponse`: each principal as the principal API
/// answers it, plus `hasDeveloperCredential` / `developerCredentialUpdatedAt`.
#[derive(Debug, Serialize, ToSchema)]
pub struct DeveloperUserListResponse {
    #[schema(value_type = Vec<Object>)]
    pub principals: Vec<serde_json::Value>,
    pub total: usize,
}

/// The per-resource gate after the load (Go `requireSelfOrUserAdmin` →
/// `requireUserAdmin`): acting on another user, a non-anchor caller only
/// reaches CLIENT-scope users (403), of a client it can access (else the
/// same 404 a missing id gets).
async fn load_target(
    state: &DeveloperCredentialsState,
    ctx: &AuthContext,
    id: &str,
) -> Result<Principal, PlatformError> {
    let p = state
        .principal_repo
        .find_by_id(id)
        .await?
        .ok_or_else(|| PlatformError::not_found("User", id))?;
    if ctx.principal_id == p.id {
        return Ok(p);
    }
    if !ctx.is_anchor() && p.scope != UserScope::Client {
        return Err(PlatformError::forbidden(
            "Client administrators can only manage client-scope users",
        ));
    }
    let in_scope = match p.client_id.as_deref() {
        Some(client_id) => ctx.can_access_client(client_id),
        None => ctx.is_anchor() || ctx.has_permission(crate::role::entity::permissions::ADMIN_ALL),
    };
    if !in_scope {
        return Err(PlatformError::not_found("User", id));
    }
    Ok(p)
}

/// List the developer-role users
#[utoipa::path(
    get,
    path = "/developer-users",
    tag = "principals",
    operation_id = "listDeveloperUsers",
    responses((status = 200, description = "USER principals holding the developer role", body = DeveloperUserListResponse)),
    security(("bearer_auth" = []))
)]
pub async fn list_developer_users(
    State(state): State<DeveloperCredentialsState>,
    auth: Authenticated,
) -> Result<Json<DeveloperUserListResponse>, PlatformError> {
    checks::require_permission(&auth.0, crate::role::entity::permissions::iam::USER_READ)?;
    let users: Vec<Principal> = state
        .principal_repo
        .find_with_role(DEVELOPER_ROLE)
        .await?
        .into_iter()
        .filter(Principal::is_user)
        .collect();
    let ids: Vec<String> = users.iter().map(|p| p.id.clone()).collect();
    let times = state
        .principal_repo
        .find_developer_secret_times(&ids)
        .await?;
    let principals: Vec<serde_json::Value> = users
        .into_iter()
        .map(|p| {
            let updated = times.get(&p.id).copied();
            let mut v = serde_json::to_value(crate::principal::api::PrincipalResponse::from(p))
                .unwrap_or_default();
            v["hasDeveloperCredential"] = serde_json::Value::Bool(updated.is_some());
            if let Some(at) = updated {
                v["developerCredentialUpdatedAt"] = serde_json::Value::String(at.to_rfc3339());
            }
            v
        })
        .collect();
    let total = principals.len();
    Ok(Json(DeveloperUserListResponse { principals, total }))
}

/// Create or rotate a developer credential
#[utoipa::path(
    post,
    path = "/{id}/developer-credential",
    tag = "principals",
    operation_id = "setPrincipalDeveloperCredential",
    params(("id" = String, Path, description = "Principal ID")),
    responses(
        (status = 200, description = "The new secret, shown once", body = SetDeveloperCredentialResponse),
        (status = 409, description = "NOT_A_USER / NOT_A_DEVELOPER")
    ),
    security(("bearer_auth" = []))
)]
pub async fn set_developer_credential(
    State(state): State<DeveloperCredentialsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<SetDeveloperCredentialResponse>, PlatformError> {
    // The coarse gate before any load (Go `requireDeveloperCredentialAccess`):
    // your own credential needs the self-service permission; someone else's
    // needs the user-admin write permission.
    if auth.0.principal_id == id {
        checks::require_permission(
            &auth.0,
            crate::role::entity::permissions::developer::API_CREDENTIAL_MANAGE,
        )?;
    } else {
        checks::can_write_principals(&auth.0)?;
    }
    let target = load_target(&state, &auth.0, &id).await?;
    let enc = state.encryption.as_ref().ok_or_else(|| {
        PlatformError::internal(
            "FLOWCATALYST_APP_KEY not configured; cannot hash developer client secret",
        )
    })?;
    let mut bytes = [0u8; 32];
    rand::rng().fill(&mut bytes[..]);
    let plaintext = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let command = SetDeveloperCredentialCommand {
        principal_id: target.id.clone(),
        secret_ref: enc.hash_secret(&plaintext),
    };
    let event = state
        .set_use_case
        .run(command, ExecutionContext::from_auth(&auth.0))
        .await
        .into_result()?;
    Ok(Json(SetDeveloperCredentialResponse {
        id: event.user_id,
        client_secret: plaintext,
    }))
}

/// Revoke a developer credential
#[utoipa::path(
    delete,
    path = "/{id}/developer-credential",
    tag = "principals",
    operation_id = "revokePrincipalDeveloperCredential",
    params(("id" = String, Path, description = "Principal ID")),
    responses((status = 204, description = "Revoked")),
    security(("bearer_auth" = []))
)]
pub async fn revoke_developer_credential(
    State(state): State<DeveloperCredentialsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<StatusCode, PlatformError> {
    // The coarse gate before any load (Go `requireDeveloperCredentialAccess`):
    // your own credential needs the self-service permission; someone else's
    // needs the user-admin write permission.
    if auth.0.principal_id == id {
        checks::require_permission(
            &auth.0,
            crate::role::entity::permissions::developer::API_CREDENTIAL_MANAGE,
        )?;
    } else {
        checks::can_write_principals(&auth.0)?;
    }
    let target = load_target(&state, &auth.0, &id).await?;
    state
        .revoke_use_case
        .run(
            RevokeDeveloperCredentialCommand {
                principal_id: target.id,
            },
            ExecutionContext::from_auth(&auth.0),
        )
        .await
        .into_result()?;
    Ok(StatusCode::NO_CONTENT)
}

/// Nested at `/api/principals` beside the principal routes.
pub fn developer_credentials_router(state: DeveloperCredentialsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(list_developer_users))
        .routes(routes!(
            set_developer_credential,
            revoke_developer_credential
        ))
        .with_state(state)
}
