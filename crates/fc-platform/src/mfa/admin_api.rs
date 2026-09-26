//! `POST /api/principals/{id}/reset-2fa` (Go principal/api/api.go
//! `resetTwoFactor`): an administrator clears a user's factors, recovery
//! codes, pending PINs and remembered devices; the user re-enrols at the
//! next sign-in if their domain requires 2FA. Mails the user and writes
//! Go's `2FA_RESET_BY_ADMIN` audit row.

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    Json,
};
use utoipa_axum::{router::OpenApiRouter, routes};

use super::login_api::{email_of, TwoFactorLogin};
use crate::principal::api::StatusChangeResponse;
use crate::shared::authorization_service::checks;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;
use crate::UserScope;

/// Clear a user's two-factor methods (forces re-enrollment)
#[utoipa::path(
    post,
    path = "/{id}/reset-2fa",
    tag = "principals",
    operation_id = "resetPrincipalTwoFactor",
    params(("id" = String, Path, description = "Principal ID")),
    responses(
        (status = 200, description = "Two-factor authentication reset", body = StatusChangeResponse),
        (status = 400, description = "NOT_USER"),
        (status = 403, description = "Insufficient permissions"),
        (status = 404, description = "Principal not found or out of scope")
    ),
    security(("bearer_auth" = []))
)]
pub async fn reset_two_factor(
    State(state): State<Arc<TwoFactorLogin>>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<StatusChangeResponse>, PlatformError> {
    // Coarse permission gate before any load (Go PR-3(a)).
    checks::can_write_principals(&auth.0)?;
    Ok(Json(reset_user_two_factor(&state, &auth.0, &id).await?))
}

/// The body of `POST /api/principals/{id}/reset-2fa`, shared with the
/// server-rendered `fc-web` UI.
pub async fn reset_user_two_factor(
    state: &TwoFactorLogin,
    ctx: &crate::AuthContext,
    id: &str,
) -> Result<StatusChangeResponse, PlatformError> {
    // Coarse permission gate before any load (Go PR-3(a)).
    checks::can_write_principals(ctx)?;
    let p = state
        .principal_repo
        .find_by_id(id)
        .await?
        .ok_or_else(|| PlatformError::not_found("Principal", id))?;
    // A non-anchor administrator reaches only CLIENT-scope users (Go
    // `blockNonClientTarget`), of a client it can access; out of scope is
    // the same 404 a missing id gets (Go `CanAccessScope`).
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
        return Err(PlatformError::not_found("Principal", id));
    }
    if !p.is_user() {
        return Err(PlatformError::bad_request_code(
            "NOT_USER",
            "Two-factor reset only applies to user accounts",
        ));
    }
    state
        .mfa
        .reset_all(&p.id)
        .await
        .map_err(|e| PlatformError::internal(format!("reset failed: {e}")))?;
    state.notifier.two_factor_reset(&email_of(&p)).await;
    super::audit::record(
        &state.audit_log_repo,
        &p.id,
        super::audit::RESET_BY_ADMIN,
        &ctx.principal_id,
    )
    .await;
    Ok(StatusChangeResponse {
        message: "Two-factor authentication reset".to_string(),
    })
}

/// Nested at `/api/principals`.
pub fn two_factor_admin_router(state: Arc<TwoFactorLogin>) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(reset_two_factor))
        .with_state(state)
}
