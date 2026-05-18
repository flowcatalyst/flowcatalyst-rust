//! Login Attempts Admin API

use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::ToSchema;

use super::entity::LoginAttempt;
use super::repository::LoginAttemptRepository;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginAttemptsQuery {
    pub attempt_type: Option<String>,
    pub outcome: Option<String>,
    pub identifier: Option<String>,
    pub principal_id: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    /// Opaque cursor returned by a previous page's `nextCursor`. Omit for
    /// the first page.
    pub after: Option<String>,
    pub page_size: Option<u64>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoginAttemptResponse {
    pub id: String,
    pub attempt_type: String,
    pub outcome: String,
    pub failure_reason: Option<String>,
    pub identifier: Option<String>,
    pub principal_id: Option<String>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub attempted_at: String,
}

impl From<LoginAttempt> for LoginAttemptResponse {
    fn from(a: LoginAttempt) -> Self {
        Self {
            id: a.id,
            attempt_type: a.attempt_type.as_str().to_string(),
            outcome: a.outcome.as_str().to_string(),
            failure_reason: a.failure_reason,
            identifier: a.identifier,
            principal_id: a.principal_id,
            ip_address: a.ip_address,
            user_agent: a.user_agent,
            attempted_at: a.attempted_at.to_rfc3339(),
        }
    }
}

/// Cursor-paginated login attempts response. `iam_login_attempts` grows
/// unbounded so we never count.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoginAttemptsListResponse {
    pub items: Vec<LoginAttemptResponse>,
    pub has_more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

#[derive(Clone)]
pub struct LoginAttemptsState {
    pub login_attempt_repo: Arc<LoginAttemptRepository>,
}

/// List login attempts with optional filters and pagination
#[utoipa::path(
    get,
    path = "",
    tag = "login-attempts",
    operation_id = "getApiLoginAttempts",
    params(
        ("attempt_type" = Option<String>, Query, description = "Filter by attempt type"),
        ("outcome" = Option<String>, Query, description = "Filter by outcome"),
        ("identifier" = Option<String>, Query, description = "Filter by identifier"),
        ("principal_id" = Option<String>, Query, description = "Filter by principal ID"),
        ("date_from" = Option<String>, Query, description = "Filter from date"),
        ("date_to" = Option<String>, Query, description = "Filter to date"),
        ("page" = Option<u64>, Query, description = "Page number"),
        ("page_size" = Option<u64>, Query, description = "Page size"),
        ("sortField" = Option<String>, Query, description = "Sort field (attempted_at, identifier, outcome, attempt_type)"),
        ("sortOrder" = Option<String>, Query, description = "Sort order (asc or desc, default: desc)"),
    ),
    responses(
        (status = 200, description = "Login attempts list", body = LoginAttemptsListResponse),
    ),
    security(("bearer_auth" = []))
)]
async fn list_login_attempts(
    State(state): State<LoginAttemptsState>,
    _auth: Authenticated,
    Query(query): Query<LoginAttemptsQuery>,
) -> Result<Json<LoginAttemptsListResponse>, PlatformError> {
    use crate::shared::api_common::{decode_cursor, encode_cursor};

    let size = query.page_size.unwrap_or(50).clamp(1, 200) as usize;
    let cursor = match query.after.as_deref() {
        Some(c) => Some(decode_cursor(c).map_err(|_| PlatformError::validation("Invalid cursor"))?),
        None => None,
    };

    let mut items = state
        .login_attempt_repo
        .find_with_cursor(
            query.attempt_type.as_deref(),
            query.outcome.as_deref(),
            query.identifier.as_deref(),
            query.principal_id.as_deref(),
            query.date_from.as_deref(),
            query.date_to.as_deref(),
            cursor.as_ref(),
            (size as i64) + 1,
        )
        .await?;

    let has_more = items.len() > size;
    if has_more {
        items.truncate(size);
    }
    let next_cursor = if has_more {
        items.last().map(|a| encode_cursor(a.attempted_at, &a.id))
    } else {
        None
    };

    Ok(Json(LoginAttemptsListResponse {
        items: items.into_iter().map(|a| a.into()).collect(),
        has_more,
        next_cursor,
    }))
}

pub fn login_attempts_router(state: LoginAttemptsState) -> Router {
    Router::new()
        .route("/", get(list_login_attempts))
        .with_state(state)
}
