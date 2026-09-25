//! `/api/reset-approvals` (Go `resetapproval/api/api.go`): a client
//! administrator lists pending lost-device resets for their clients and
//! approves (the user is emailed a reset link that also clears their 2FA)
//! or denies them.

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    Json,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::reset_approval::{Decision, ResetApprovalRepository, ResetApprovalRequest};
use crate::auth::password_reset_api::{PasswordResetEmailer, ResetOptions};
use crate::principal::api::StatusChangeResponse;
use crate::shared::authorization_service::{checks, AuthContext};
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;
use crate::PrincipalRepository;

#[derive(Clone)]
pub struct ResetApprovalsState {
    pub approvals: Arc<ResetApprovalRepository>,
    pub principal_repo: Arc<PrincipalRepository>,
    pub emailer: Arc<PasswordResetEmailer>,
}

/// Go `RequestDTO`.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResetApprovalDto {
    pub id: String,
    pub principal_id: String,
    pub email: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ResetApprovalListResponse {
    pub requests: Vec<ResetApprovalDto>,
}

/// Go `RequireUserAdmin`: an anchor reaches every target but still needs a
/// user-write permission; anyone else only a user of a client they can
/// access.
fn can_write_principals_of_client(
    ctx: &AuthContext,
    client_id: Option<&str>,
) -> Result<(), PlatformError> {
    if !ctx.is_anchor() {
        let Some(client_id) = client_id else {
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
    checks::can_write_principals(ctx)
}

/// List pending lost-device reset requests for your client(s)
#[utoipa::path(
    get,
    path = "",
    tag = "reset-approvals",
    operation_id = "listResetApprovals",
    responses((status = 200, body = ResetApprovalListResponse)),
    security(("bearer_auth" = []))
)]
pub async fn list_reset_approvals(
    State(state): State<ResetApprovalsState>,
    auth: Authenticated,
) -> Result<Json<ResetApprovalListResponse>, PlatformError> {
    checks::can_write_principals(&auth.0)?;
    let scope = (!auth.0.is_anchor()).then_some(auth.0.accessible_clients.as_slice());
    let pending = state.approvals.list_pending(scope).await?;
    let ids: Vec<String> = pending.iter().map(|r| r.principal_id.clone()).collect();
    let people = state
        .principal_repo
        .find_names_and_emails_by_ids(&ids)
        .await?;
    let requests = pending
        .into_iter()
        .map(|r| {
            let (name, email) = people.get(&r.principal_id).cloned().unwrap_or_default();
            ResetApprovalDto {
                id: r.id,
                principal_id: r.principal_id,
                email,
                name,
                client_id: r.client_id,
                expires_at: r.expires_at,
                created_at: r.created_at,
            }
        })
        .collect();
    Ok(Json(ResetApprovalListResponse { requests }))
}

async fn load(
    state: &ResetApprovalsState,
    id: &str,
) -> Result<ResetApprovalRequest, PlatformError> {
    state
        .approvals
        .find_by_id(id)
        .await?
        .ok_or_else(|| PlatformError::not_found("ResetApprovalRequest", id))
}

fn already_decided() -> PlatformError {
    PlatformError::bad_request_code("ALREADY_DECIDED", "request is no longer pending")
}

/// Approve a lost-device reset (emails the user a reset link)
#[utoipa::path(
    post,
    path = "/{id}/approve",
    tag = "reset-approvals",
    operation_id = "approveResetApproval",
    params(("id" = String, Path, description = "Request ID")),
    responses((status = 200, body = StatusChangeResponse)),
    security(("bearer_auth" = []))
)]
pub async fn approve_reset_approval(
    State(state): State<ResetApprovalsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<StatusChangeResponse>, PlatformError> {
    let request = load(&state, &id).await?;
    can_write_principals_of_client(&auth.0, request.client_id.as_deref())?;
    if !state
        .approvals
        .decide(&request.id, Decision::Approved, &auth.0.principal_id)
        .await?
    {
        return Err(already_decided());
    }
    let principal = state
        .principal_repo
        .find_by_id(&request.principal_id)
        .await?
        .ok_or_else(|| PlatformError::not_found("Principal", &request.principal_id))?;
    if let Err(e) = state
        .emailer
        .send_reset_email_with(
            &principal,
            ResetOptions {
                reset_2fa: request.reset_2fa,
                ..Default::default()
            },
        )
        .await
    {
        tracing::warn!(principal_id = %principal.id, error = %e, "failed to send approved reset link");
    }
    Ok(Json(StatusChangeResponse {
        message: "Reset approved — the user has been emailed a link".to_string(),
    }))
}

/// Deny a lost-device reset request
#[utoipa::path(
    post,
    path = "/{id}/deny",
    tag = "reset-approvals",
    operation_id = "denyResetApproval",
    params(("id" = String, Path, description = "Request ID")),
    responses((status = 200, body = StatusChangeResponse)),
    security(("bearer_auth" = []))
)]
pub async fn deny_reset_approval(
    State(state): State<ResetApprovalsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<StatusChangeResponse>, PlatformError> {
    let request = load(&state, &id).await?;
    can_write_principals_of_client(&auth.0, request.client_id.as_deref())?;
    if !state
        .approvals
        .decide(&request.id, Decision::Denied, &auth.0.principal_id)
        .await?
    {
        return Err(already_decided());
    }
    Ok(Json(StatusChangeResponse {
        message: "Reset request denied".to_string(),
    }))
}

/// Nested at `/api/reset-approvals`.
pub fn reset_approvals_router(state: ResetApprovalsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(list_reset_approvals))
        .routes(routes!(approve_reset_approval))
        .routes(routes!(deny_reset_approval))
        .with_state(state)
}
