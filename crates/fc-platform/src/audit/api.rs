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
use crate::PrincipalRepository;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;

/// Audit log response DTO (matches Java AuditLogDto)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuditLogResponse {
    pub id: String,
    pub operation: String,
    pub entity_type: String,
    pub entity_id: Option<String>,
    pub principal_id: Option<String>,
    pub principal_name: Option<String>,
    pub application_id: Option<String>,
    pub client_id: Option<String>,
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
    pub operation_json: Option<String>,
    pub principal_id: Option<String>,
    pub principal_name: Option<String>,
    pub application_id: Option<String>,
    pub client_id: Option<String>,
    pub performed_at: String,
}

impl From<AuditLog> for AuditLogResponse {
    fn from(log: AuditLog) -> Self {
        let entity_id_opt = if log.entity_id.is_empty() { None } else { Some(log.entity_id) };
        Self {
            id: log.id,
            operation: log.operation,
            entity_type: log.entity_type,
            entity_id: entity_id_opt,
            principal_id: log.principal_id,
            principal_name: log.principal_name,
            application_id: log.application_id,
            client_id: log.client_id,
            performed_at: log.performed_at.to_rfc3339(),
        }
    }
}

impl From<AuditLog> for AuditLogDetailResponse {
    fn from(log: AuditLog) -> Self {
        let entity_id_opt = if log.entity_id.is_empty() { None } else { Some(log.entity_id) };
        let op_json = log.operation_json.map(|v| serde_json::to_string(&v).unwrap_or_default());
        Self {
            id: log.id,
            operation: log.operation,
            entity_type: log.entity_type,
            entity_id: entity_id_opt,
            operation_json: op_json,
            principal_id: log.principal_id,
            principal_name: log.principal_name,
            application_id: log.application_id,
            client_id: log.client_id,
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

/// Application IDs response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationIdsResponse {
    pub application_ids: Vec<String>,
}

/// Client IDs response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClientIdsResponse {
    pub client_ids: Vec<String>,
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
    pub principal_repo: Arc<PrincipalRepository>,
}


/// Enrich audit logs with principal names from a batch lookup.
async fn enrich_principal_names(
    logs: &mut [AuditLog],
    principal_repo: &PrincipalRepository,
) {
    let principal_ids: Vec<String> = logs.iter()
        .filter_map(|l| l.principal_id.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    if principal_ids.is_empty() {
        return;
    }

    if let Ok(name_map) = principal_repo.find_names_by_ids(&principal_ids).await {
        for log in logs.iter_mut() {
            if let Some(pid) = &log.principal_id {
                log.principal_name = name_map.get(pid).cloned();
            }
        }
    }
}

/// Enrich a single audit log with principal name.
async fn enrich_single_principal_name(
    log: &mut AuditLog,
    principal_repo: &PrincipalRepository,
) {
    if let Some(pid) = &log.principal_id {
        if let Ok(name_map) = principal_repo.find_names_by_ids(&[pid.clone()]).await {
            log.principal_name = name_map.get(pid).cloned();
        }
    }
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
    operation_id = "getApiAdminAuditLogsEntityTypes",
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
    operation_id = "getApiAdminAuditLogsOperations",
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
    operation_id = "getApiAdminAuditLogsById",
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

    let mut log = state.audit_log_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("AuditLog", &id))?;

    enrich_single_principal_name(&mut log, &state.principal_repo).await;

    Ok(Json(log.into()))
}

/// List audit logs with filters (matches Java AuditLogAdminResource)
#[utoipa::path(
    get,
    path = "",
    tag = "audit-logs",
    operation_id = "getApiAdminAuditLogs",
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

    let mut logs = state.audit_log_repo.search(
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

    enrich_principal_names(&mut logs, &state.principal_repo).await;

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
    operation_id = "getApiAdminAuditLogsEntityByEntityTypeByEntityId",
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

    let mut logs = state.audit_log_repo.find_by_entity(&entity_type, &entity_id, 1000).await?;
    let total = logs.len() as i64;

    enrich_principal_names(&mut logs, &state.principal_repo).await;

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
    operation_id = "getApiAdminAuditLogsPrincipalByPrincipalId",
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

    let mut logs = state.audit_log_repo.find_by_principal(&principal_id, 1000).await?;

    enrich_principal_names(&mut logs, &state.principal_repo).await;

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
    operation_id = "getApiAdminAuditLogsRecent",
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

    let mut logs = state.audit_log_repo.find_recent(100).await?;

    enrich_principal_names(&mut logs, &state.principal_repo).await;

    let response: Vec<AuditLogResponse> = logs.into_iter()
        .map(|l| l.into())
        .collect();

    Ok(Json(response))
}

/// Get distinct application IDs
#[utoipa::path(
    get,
    path = "/application-ids",
    tag = "audit-logs",
    operation_id = "getApiAdminAuditLogsApplicationIds",
    responses(
        (status = 200, description = "List of distinct application IDs", body = ApplicationIdsResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_application_ids(
    State(state): State<AuditLogsState>,
    auth: Authenticated,
) -> Result<Json<ApplicationIdsResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    let application_ids = state.audit_log_repo.find_distinct_application_ids().await?;

    Ok(Json(ApplicationIdsResponse { application_ids }))
}

/// Get distinct client IDs
#[utoipa::path(
    get,
    path = "/client-ids",
    tag = "audit-logs",
    operation_id = "getApiAdminAuditLogsClientIds",
    responses(
        (status = 200, description = "List of distinct client IDs", body = ClientIdsResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_client_ids(
    State(state): State<AuditLogsState>,
    auth: Authenticated,
) -> Result<Json<ClientIdsResponse>, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;

    let client_ids = state.audit_log_repo.find_distinct_client_ids().await?;

    Ok(Json(ClientIdsResponse { client_ids }))
}

/// Create audit logs router
pub fn audit_logs_router(state: AuditLogsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(list_audit_logs))
        .routes(routes!(get_entity_types))
        .routes(routes!(get_operations))
        .routes(routes!(get_application_ids))
        .routes(routes!(get_client_ids))
        .routes(routes!(get_recent_audit_logs))
        .routes(routes!(get_audit_log))
        .routes(routes!(get_entity_audit_logs))
        .routes(routes!(get_principal_audit_logs))
        .with_state(state)
}
