//! Login Attempts Admin API

use axum::{
    extract::{Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::ToSchema;

use super::entity::LoginAttempt;
use super::repository::{LoginAttemptFilter, LoginAttemptRepository};
use fc_platform_core::shared::authorization_service::checks;
use fc_platform_core::shared::enum_str::parse_opt;
use fc_platform_core::shared::error::PlatformError;
use fc_platform_core::shared::middleware::Authenticated;

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
    pub page_size: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoginAttemptResponse {
    pub id: String,
    pub attempt_type: String,
    pub outcome: String,
    #[schema(required = true)]
    pub failure_reason: Option<String>,
    #[schema(required = true)]
    pub identifier: Option<String>,
    #[schema(required = true)]
    pub principal_id: Option<String>,
    #[schema(required = true)]
    pub ip_address: Option<String>,
    #[schema(required = true)]
    pub user_agent: Option<String>,
    #[schema(format = DateTime)]
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
#[schema(as = LoginAttemptListResponse)]
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
    operation_id = "listLoginAttempts",
    params(
        ("attemptType" = Option<String>, Query),
        ("outcome" = Option<String>, Query),
        ("identifier" = Option<String>, Query),
        ("principalId" = Option<String>, Query),
        ("dateFrom" = Option<String>, Query),
        ("dateTo" = Option<String>, Query),
        ("after" = Option<String>, Query),
        ("pageSize" = Option<i64>, Query)
    ),
    responses(
        (status = 200, description = "Login attempts list", body = LoginAttemptsListResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_login_attempts(
    State(state): State<LoginAttemptsState>,
    auth: Authenticated,
    Query(query): Query<LoginAttemptsQuery>,
) -> Result<Json<LoginAttemptsListResponse>, PlatformError> {
    checks::can_read_login_attempts(&auth.0)?;

    use fc_platform_core::shared::api_common::{decode_cursor, encode_cursor};

    // Go (loginattempt/api/api.go `list`): a page size outside 1..=200 is
    // the default 50, and a cursor that does not decode is ignored.
    let size = match query.page_size {
        Some(s) if (1..=200).contains(&s) => s as usize,
        _ => 50,
    };
    let cursor = query.after.as_deref().and_then(|c| decode_cursor(c).ok());

    let mut items = state
        .login_attempt_repo
        .find_with_cursor(
            &LoginAttemptFilter {
                attempt_type: parse_opt(query.attempt_type.as_deref())?,
                outcome: parse_opt(query.outcome.as_deref())?,
                identifier: query.identifier.as_deref(),
                principal_id: query.principal_id.as_deref(),
                date_from: query.date_from.as_deref(),
                date_to: query.date_to.as_deref(),
            },
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
