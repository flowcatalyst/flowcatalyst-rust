//! Audit Logs Admin API
//!
//! REST endpoints for viewing audit logs.

use axum::{
    extract::{State, Path, Query},
    Json,
};
use utoipa_axum::{router::OpenApiRouter, routes};
use utoipa::{ToSchema, IntoParams};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use chrono::{DateTime, Utc};

use crate::AuditLog;
use crate::AuditLogRepository;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;

/// Audit log response DTO (matches Java AuditLogDto)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuditLogResponse {
    pub id: String,
    /// Operation name (Java calls this "operation")
    pub operation: String,
    pub entity_type: String,
    pub entity_id: Option<String>,
    pub principal_id: Option<String>,
    /// Principal name (resolved from principal entity)
    pub principal_name: Option<String>,
    pub performed_at: String,
}

/// Audit log detail response (includes operation JSON)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuditLogDetailResponse {
    pub id: String,
    pub operation: String,
    pub entity_type: String,
    pub entity_id: Option<String>,
    /// Full operation payload as JSON string
    pub operation_json: Option<String>,
    pub principal_id: Option<String>,
    pub principal_name: Option<String>,
    pub performed_at: String,
}

impl From<AuditLog> for AuditLogResponse {
    fn from(log: AuditLog) -> Self {
        Self {
            id: log.id,
            operation: log.operation,
            entity_type: log.entity_type,
            entity_id: log.entity_id,
            principal_id: log.principal_id.clone(),
            principal_name: log.principal_id.clone(),
            performed_at: log.performed_at.to_rfc3339(),
        }
    }
}

impl From<AuditLog> for AuditLogDetailResponse {
    fn from(log: AuditLog) -> Self {
        Self {
            id: log.id,
            operation: log.operation,
            entity_type: log.entity_type,
            entity_id: log.entity_id,
            operation_json: log.operation_json,
            principal_id: log.principal_id.clone(),
            principal_name: log.principal_id.clone(),
            performed_at: log.performed_at.to_rfc3339(),
        }
    }
}

/// Audit logs list response (matches Java AuditLogListResponse)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuditLogListResponse {
    pub audit_logs: Vec<AuditLogResponse>,
    pub total: i64,
    pub page: i32,
    pub page_size: i32,
}

/// Entity types response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EntityTypesResponse {
    pub entity_types: Vec<String>,
}

/// Operations response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OperationsResponse {
    pub operations: Vec<String>,
}

/// Entity audit logs response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EntityAuditLogsResponse {
    pub audit_logs: Vec<AuditLogResponse>,
    pub total: i64,
    pub entity_type: String,
    pub entity_id: String,
}

/// Query parameters for audit logs (matches Java query params)
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct AuditLogsQuery {
    /// Page number (0-based, matches Java)
    #[serde(default)]
    pub page: i32,

    /// Page size (default 50)
    #[serde(default = "default_page_size")]
    pub page_size: i32,

    /// Filter by entity type
    pub entity_type: Option<String>,

    /// Filter by entity ID
    pub entity_id: Option<String>,

    /// Filter by operation (Java calls this "operation", maps to action internally)
    pub operation: Option<String>,

    /// Filter by principal ID
    pub principal_id: Option<String>,
}

fn default_page_size() -> i32 { 50 }

/// Audit logs service state
#[derive(Clone)]
pub struct AuditLogsState {
    pub audit_log_repo: Arc<AuditLogRepository>,
}


#[allow(dead_code)]
fn parse_datetime(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s).ok().map(|dt| dt.with_timezone(&Utc))
}

/// Get distinct entity types
#[utoipa::path(
    get,
    path = "/entity-types",
    tag = "audit-logs",
    operation_id = "getApiAdminPlatformAuditLogsEntityTypes",
    responses(
        (status = 200, description = "List of distinct entity types", body = EntityTypesResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_entity_types(
    State(state): State<AuditLogsState>,
    auth: Authenticated,
) -> Result<Json<EntityTypesResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    let entity_types = state.audit_log_repo.find_distinct_entity_types().await?;

    Ok(Json(EntityTypesResponse { entity_types }))
}

/// Get distinct operations
#[utoipa::path(
    get,
    path = "/operations",
    tag = "audit-logs",
    operation_id = "getApiAdminPlatformAuditLogsOperations",
    responses(
        (status = 200, description = "List of distinct operations", body = OperationsResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_operations(
    State(state): State<AuditLogsState>,
    auth: Authenticated,
) -> Result<Json<OperationsResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    let operations = state.audit_log_repo.find_distinct_operations().await?;

    Ok(Json(OperationsResponse { operations }))
}

/// Get audit log by ID
#[utoipa::path(
    get,
    path = "/{id}",
    tag = "audit-logs",
    operation_id = "getApiAdminPlatformAuditLogsById",
    params(
        ("id" = String, Path, description = "Audit log ID")
    ),
    responses(
        (status = 200, description = "Audit log found", body = AuditLogDetailResponse),
        (status = 404, description = "Audit log not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_audit_log(
    State(state): State<AuditLogsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<AuditLogDetailResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    let log = state.audit_log_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("AuditLog", &id))?;

    Ok(Json(log.into()))
}

/// List audit logs with filters (matches Java AuditLogAdminResource)
#[utoipa::path(
    get,
    path = "",
    tag = "audit-logs",
    operation_id = "getApiAdminPlatformAuditLogs",
    params(AuditLogsQuery),
    responses(
        (status = 200, description = "List of audit logs", body = AuditLogListResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_audit_logs(
    State(state): State<AuditLogsState>,
    auth: Authenticated,
    Query(query): Query<AuditLogsQuery>,
) -> Result<Json<AuditLogListResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    let page = query.page;
    let page_size = query.page_size;
    let skip = (page * page_size) as u64;
    let limit = page_size as i64;

    let logs = state.audit_log_repo.search(
        query.entity_type.as_deref(),
        query.entity_id.as_deref(),
        query.operation.as_deref(),
        query.principal_id.as_deref(),
        skip,
        limit,
    ).await?;

    // Get total count for pagination
    let total = state.audit_log_repo.count_with_filters(
        query.entity_type.as_deref(),
        query.entity_id.as_deref(),
        query.operation.as_deref(),
        query.principal_id.as_deref(),
    ).await?;

    let audit_logs: Vec<AuditLogResponse> = logs.into_iter()
        .map(|l| l.into())
        .collect();

    Ok(Json(AuditLogListResponse {
        audit_logs,
        total,
        page,
        page_size,
    }))
}

/// Get audit logs for a specific entity
#[utoipa::path(
    get,
    path = "/entity/{entity_type}/{entity_id}",
    tag = "audit-logs",
    operation_id = "getApiAdminPlatformAuditLogsEntityByEntityTypeByEntityId",
    params(
        ("entity_type" = String, Path, description = "Entity type"),
        ("entity_id" = String, Path, description = "Entity ID")
    ),
    responses(
        (status = 200, description = "Audit logs for entity", body = EntityAuditLogsResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_entity_audit_logs(
    State(state): State<AuditLogsState>,
    auth: Authenticated,
    Path((entity_type, entity_id)): Path<(String, String)>,
) -> Result<Json<EntityAuditLogsResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    let logs = state.audit_log_repo.find_by_entity(&entity_type, &entity_id, 1000).await?;
    let total = logs.len() as i64;

    let audit_logs: Vec<AuditLogResponse> = logs.into_iter()
        .map(|l| l.into())
        .collect();

    Ok(Json(EntityAuditLogsResponse {
        audit_logs,
        total,
        entity_type,
        entity_id,
    }))
}

/// Get audit logs for a principal
#[utoipa::path(
    get,
    path = "/principal/{principal_id}",
    tag = "audit-logs",
    operation_id = "getApiAdminPlatformAuditLogsPrincipalByPrincipalId",
    params(
        ("principal_id" = String, Path, description = "Principal ID")
    ),
    responses(
        (status = 200, description = "Audit logs for principal", body = Vec<AuditLogResponse>)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_principal_audit_logs(
    State(state): State<AuditLogsState>,
    auth: Authenticated,
    Path(principal_id): Path<String>,
) -> Result<Json<Vec<AuditLogResponse>>, PlatformError> {
    // Allow principals to view their own audit logs
    if !auth.0.is_anchor() && auth.0.principal_id != principal_id {
        return Err(PlatformError::forbidden("Cannot view other principal's audit logs"));
    }

    let logs = state.audit_log_repo.find_by_principal(&principal_id, 1000).await?;

    let response: Vec<AuditLogResponse> = logs.into_iter()
        .map(|l| l.into())
        .collect();

    Ok(Json(response))
}

/// Get recent audit logs
#[utoipa::path(
    get,
    path = "/recent",
    tag = "audit-logs",
    operation_id = "getApiAdminPlatformAuditLogsRecent",
    responses(
        (status = 200, description = "Recent audit logs", body = Vec<AuditLogResponse>)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_recent_audit_logs(
    State(state): State<AuditLogsState>,
    auth: Authenticated,
) -> Result<Json<Vec<AuditLogResponse>>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    let logs = state.audit_log_repo.find_recent(100).await?;

    let response: Vec<AuditLogResponse> = logs.into_iter()
        .map(|l| l.into())
        .collect();

    Ok(Json(response))
}

/// Create audit logs router
pub fn audit_logs_router(state: AuditLogsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(list_audit_logs))
        .routes(routes!(get_entity_types))
        .routes(routes!(get_operations))
        .routes(routes!(get_recent_audit_logs))
        .routes(routes!(get_audit_log))
        .routes(routes!(get_entity_audit_logs))
        .routes(routes!(get_principal_audit_logs))
        .with_state(state)
}
