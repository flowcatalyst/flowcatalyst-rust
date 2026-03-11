//! Login Attempts Admin API

use axum::{
    routing::get,
    extract::{State, Query},
    Json, Router,
};
use utoipa::ToSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

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
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    #[serde(rename = "sortField")]
    pub sort_field: Option<String>,
    #[serde(rename = "sortOrder")]
    pub sort_order: Option<String>,
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

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoginAttemptsListResponse {
    pub items: Vec<LoginAttemptResponse>,
    pub total: u64,
    pub page: u64,
    pub page_size: u64,
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
    operation_id = "getApiAdminLoginAttempts",
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
    let page = query.page.unwrap_or(0);
    let page_size = query.page_size.unwrap_or(100).min(500);

    let (items, total) = state.login_attempt_repo.find_paged(
        query.attempt_type.as_deref(),
        query.outcome.as_deref(),
        query.identifier.as_deref(),
        query.principal_id.as_deref(),
        query.date_from.as_deref(),
        query.date_to.as_deref(),
        page,
        page_size,
        query.sort_field.as_deref(),
        query.sort_order.as_deref(),
    ).await?;

    Ok(Json(LoginAttemptsListResponse {
        items: items.into_iter().map(|a| a.into()).collect(),
        total,
        page,
        page_size,
    }))
}

pub fn login_attempts_router(state: LoginAttemptsState) -> Router {
    Router::new()
        .route("/", get(list_login_attempts))
        .with_state(state)
}
