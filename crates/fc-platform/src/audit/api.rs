//! Audit Logs Admin API
//!
//! REST endpoints for viewing audit logs.

use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::audit::stored_redaction::redact_stored_document;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;
use crate::AuditLog;
use crate::AuditLogRepository;
use crate::PrincipalRepository;

/// One audit log, Go's `AuditLogResponse` (audit/api/dto.go): the list,
/// the by-entity and by-principal lists and the single read all use it.
/// Optional members are absent when unset (Go's `omitempty`);
/// `operationJson` is the command document as a compact JSON string, which
/// the SPA `JSON.parse`s, redacted on read ([`redact_stored_document`]).
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuditLogResponse {
    pub id: String,
    pub entity_type: String,
    pub entity_id: String,
    pub operation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_json: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub principal_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub principal_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    pub performed_at: String,
}

impl From<AuditLog> for AuditLogResponse {
    fn from(log: AuditLog) -> Self {
        let operation_json = log
            .operation_json
            .as_ref()
            .filter(|v| !v.is_null())
            .map(|v| {
                serde_json::to_string(&redact_stored_document(&log.operation, v))
                    .unwrap_or_default()
            });
        Self {
            id: log.id,
            entity_type: log.entity_type,
            entity_id: log.entity_id,
            operation: log.operation,
            operation_json,
            principal_id: log.principal_id,
            principal_name: log.principal_name,
            application_id: log.application_id,
            client_id: log.client_id,
            performed_at: log.performed_at.to_rfc3339(),
        }
    }
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

/// Redacted on read (Java b4a15fd8, S11): a row stored before source-side
/// redaction — every Go-era row, and rows from SDKs that predate it — is
/// never served with its secret, whether or not the sweep has run. The rule
/// is the sweep's own ([`redact_stored_document`]), so the two can't drift.
impl From<AuditLog> for AuditLogDetailResponse {
    fn from(log: AuditLog) -> Self {
        let entity_id_opt = if log.entity_id.is_empty() {
            None
        } else {
            Some(log.entity_id)
        };
        let op_json = log.operation_json.map(|v| {
            serde_json::to_string(&redact_stored_document(&log.operation, &v)).unwrap_or_default()
        });
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

/// Cursor-paginated audit logs response. `aud_logs` grows unbounded, so we
/// keyset-paginate on `(performed_at, id) DESC` and never count.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuditLogListResponse {
    pub audit_logs: Vec<AuditLogResponse>,
    pub has_more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
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

/// Query parameters for the audit log list (Go `listInput`,
/// audit/api/api.go): cursor-paginated, with the SPA's filters.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct AuditLogsQuery {
    /// Opaque cursor returned by a previous page's `nextCursor`. Omit for
    /// the first page.
    pub after: Option<String>,

    /// Page size (default 50; a value outside 1..=200 means 50).
    #[serde(default = "default_page_size")]
    pub page_size: i32,

    /// Filter by entity type
    pub entity_type: Option<String>,

    /// Filter by entity ID
    pub entity_id: Option<String>,

    /// Filter by operation (the command name)
    pub operation: Option<String>,

    /// Filter by principal ID
    pub principal_id: Option<String>,

    /// CSV of application ids
    pub application_ids: Option<String>,

    /// CSV of client ids
    pub client_ids: Option<String>,
}

/// A comma-separated query value, trimmed, blanks dropped (Go `csv`).
fn csv(value: Option<&str>) -> Vec<String> {
    value
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// An empty query value is no filter (Go `apicommon.OptStr`).
fn opt(value: &Option<String>) -> Option<&str> {
    value.as_deref().filter(|s| !s.is_empty())
}

fn default_page_size() -> i32 {
    50
}

/// Audit logs service state
#[derive(Clone)]
pub struct AuditLogsState {
    pub audit_log_repo: Arc<AuditLogRepository>,
    pub principal_repo: Arc<PrincipalRepository>,
}

/// Enrich audit logs with principal names from a batch lookup.
pub async fn enrich_principal_names(logs: &mut [AuditLog], principal_repo: &PrincipalRepository) {
    let principal_ids: Vec<String> = logs
        .iter()
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
pub async fn enrich_single_principal_name(
    log: &mut AuditLog,
    principal_repo: &PrincipalRepository,
) {
    if let Some(pid) = &log.principal_id {
        if let Ok(name_map) = principal_repo
            .find_names_by_ids(std::slice::from_ref(pid))
            .await
        {
            log.principal_name = name_map.get(pid).cloned();
        }
    }
}

#[allow(dead_code)]
fn parse_datetime(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

/// Get distinct entity types
#[utoipa::path(
    get,
    path = "/entity-types",
    tag = "audit-logs",
    operation_id = "getApiAuditLogsEntityTypes",
    responses(
        (status = 200, description = "List of distinct entity types", body = EntityTypesResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_entity_types(
    State(state): State<AuditLogsState>,
    auth: Authenticated,
) -> Result<Json<EntityTypesResponse>, PlatformError> {
    crate::checks::can_read_audit_logs(&auth.0)?;

    let entity_types = state.audit_log_repo.find_distinct_entity_types().await?;

    Ok(Json(EntityTypesResponse { entity_types }))
}

/// Get distinct operations
#[utoipa::path(
    get,
    path = "/operations",
    tag = "audit-logs",
    operation_id = "getApiAuditLogsOperations",
    responses(
        (status = 200, description = "List of distinct operations", body = OperationsResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_operations(
    State(state): State<AuditLogsState>,
    auth: Authenticated,
) -> Result<Json<OperationsResponse>, PlatformError> {
    crate::checks::can_read_audit_logs(&auth.0)?;

    let operations = state.audit_log_repo.find_distinct_operations().await?;

    Ok(Json(OperationsResponse { operations }))
}

/// Get audit log by ID
#[utoipa::path(
    get,
    path = "/{id}",
    tag = "audit-logs",
    operation_id = "getApiAuditLogsById",
    params(
        ("id" = String, Path, description = "Audit log ID")
    ),
    responses(
        (status = 200, description = "Audit log found", body = AuditLogResponse),
        (status = 404, description = "Audit log not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_audit_log(
    State(state): State<AuditLogsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<AuditLogResponse>, PlatformError> {
    crate::checks::can_read_audit_logs(&auth.0)?;

    let mut log = state
        .audit_log_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("AuditLog", &id))?;

    enrich_single_principal_name(&mut log, &state.principal_repo).await;

    Ok(Json(log.into()))
}

/// List audit logs with filters
#[utoipa::path(
    get,
    path = "",
    tag = "audit-logs",
    operation_id = "getApiAuditLogs",
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
    use crate::audit::repository::AuditCursorFilter;
    use crate::shared::api_common::{decode_cursor, encode_cursor};

    // Go gates every audit read on the permission alone: anchor scope is
    // reach, and the audit log is not client-scoped (audit/api/api.go).
    crate::checks::can_read_audit_logs(&auth.0)?;

    let size = if (1..=200).contains(&query.page_size) {
        query.page_size as usize
    } else {
        50
    };
    let cursor = match opt(&query.after) {
        Some(c) => Some(
            decode_cursor(c)
                .map_err(|_| PlatformError::bad_request_code("CURSOR", "invalid cursor"))?,
        ),
        None => None,
    };
    let application_ids = csv(query.application_ids.as_deref());
    let client_ids = csv(query.client_ids.as_deref());

    let mut logs = state
        .audit_log_repo
        .search_with_cursor_filtered(
            &AuditCursorFilter {
                entity_type: opt(&query.entity_type),
                entity_id: opt(&query.entity_id),
                principal_id: opt(&query.principal_id),
                operation: opt(&query.operation),
                application_ids: &application_ids,
                client_ids: &client_ids,
            },
            cursor.as_ref(),
            (size as i64) + 1,
        )
        .await?;

    let has_more = logs.len() > size;
    if has_more {
        logs.truncate(size);
    }
    let next_cursor = if has_more {
        logs.last().map(|l| encode_cursor(l.performed_at, &l.id))
    } else {
        None
    };

    enrich_principal_names(&mut logs, &state.principal_repo).await;

    let audit_logs: Vec<AuditLogResponse> = logs.into_iter().map(|l| l.into()).collect();

    Ok(Json(AuditLogListResponse {
        audit_logs,
        has_more,
        next_cursor,
    }))
}

/// A non-paginated audit list (by entity, by principal): Go's
/// `{auditLogs, hasMore: false}`, newest first, at most 500 rows.
async fn unpaged(state: &AuditLogsState, mut logs: Vec<AuditLog>) -> AuditLogListResponse {
    enrich_principal_names(&mut logs, &state.principal_repo).await;
    AuditLogListResponse {
        audit_logs: logs.into_iter().map(|l| l.into()).collect(),
        has_more: false,
        next_cursor: None,
    }
}

/// Get audit logs for a specific entity
#[utoipa::path(
    get,
    path = "/entity/{entityType}/{entityId}",
    tag = "audit-logs",
    operation_id = "getApiAuditLogsEntityByEntityTypeByEntityId",
    params(
        ("entityType" = String, Path, description = "Entity type"),
        ("entityId" = String, Path, description = "Entity ID")
    ),
    responses(
        (status = 200, description = "Audit logs for entity", body = AuditLogListResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_entity_audit_logs(
    State(state): State<AuditLogsState>,
    auth: Authenticated,
    Path((entity_type, entity_id)): Path<(String, String)>,
) -> Result<Json<AuditLogListResponse>, PlatformError> {
    crate::checks::can_read_audit_logs(&auth.0)?;

    let logs = state
        .audit_log_repo
        .find_by_entity(&entity_type, &entity_id, 500)
        .await?;

    Ok(Json(unpaged(&state, logs).await))
}

/// Get audit logs for a principal
#[utoipa::path(
    get,
    path = "/principal/{principalId}",
    tag = "audit-logs",
    operation_id = "getApiAuditLogsPrincipalByPrincipalId",
    params(
        ("principalId" = String, Path, description = "Principal ID")
    ),
    responses(
        (status = 200, description = "Audit logs for principal", body = AuditLogListResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_principal_audit_logs(
    State(state): State<AuditLogsState>,
    auth: Authenticated,
    Path(principal_id): Path<String>,
) -> Result<Json<AuditLogListResponse>, PlatformError> {
    crate::checks::can_read_audit_logs(&auth.0)?;

    let logs = state
        .audit_log_repo
        .find_by_principal(&principal_id, 500)
        .await?;

    Ok(Json(unpaged(&state, logs).await))
}

/// Recent audit logs: Go serves the list handler here too (an alias with
/// the same filters and cursor).
#[utoipa::path(
    get,
    path = "/recent",
    tag = "audit-logs",
    operation_id = "getApiAuditLogsRecent",
    params(AuditLogsQuery),
    responses(
        (status = 200, description = "Recent audit logs", body = AuditLogListResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_recent_audit_logs(
    state: State<AuditLogsState>,
    auth: Authenticated,
    query: Query<AuditLogsQuery>,
) -> Result<Json<AuditLogListResponse>, PlatformError> {
    list_audit_logs(state, auth, query).await
}

/// Get distinct application IDs
#[utoipa::path(
    get,
    path = "/application-ids",
    tag = "audit-logs",
    operation_id = "getApiAuditLogsApplicationIds",
    responses(
        (status = 200, description = "List of distinct application IDs", body = ApplicationIdsResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_application_ids(
    State(state): State<AuditLogsState>,
    auth: Authenticated,
) -> Result<Json<ApplicationIdsResponse>, PlatformError> {
    crate::checks::can_read_audit_logs(&auth.0)?;

    let application_ids = state.audit_log_repo.find_distinct_application_ids().await?;

    Ok(Json(ApplicationIdsResponse { application_ids }))
}

/// Get distinct client IDs
#[utoipa::path(
    get,
    path = "/client-ids",
    tag = "audit-logs",
    operation_id = "getApiAuditLogsClientIds",
    responses(
        (status = 200, description = "List of distinct client IDs", body = ClientIdsResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_client_ids(
    State(state): State<AuditLogsState>,
    auth: Authenticated,
) -> Result<Json<ClientIdsResponse>, PlatformError> {
    crate::checks::can_read_audit_logs(&auth.0)?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn detail(operation: &str, stored: Value) -> Value {
        let log = AuditLog::new("Thing", "thg_1", operation, Some(stored), None);
        let body = AuditLogDetailResponse::from(log)
            .operation_json
            .expect("operation json");
        serde_json::from_str(&body).expect("served json")
    }

    /// Every shared vector the name rule alone decides (`masked` empty) is
    /// served exactly as the spec expects, from a row stored unredacted.
    #[test]
    fn a_stored_row_is_served_redacted_per_the_shared_vectors() {
        let vectors: Vec<Value> = serde_json::from_str(include_str!(
            "../../../../docs/spec/audit-redaction-vectors.json"
        ))
        .unwrap();
        let mut checked = 0;
        for v in vectors {
            if !v["masked"].as_array().is_some_and(|m| m.is_empty()) {
                continue;
            }
            assert_eq!(
                detail("SomeCommand", v["input"].clone()),
                v["expected"],
                "{}",
                v["name"]
            );
            checked += 1;
        }
        assert!(checked >= 5, "only {checked} vectors checked");
    }

    /// A Go-era set-property row keeps its value only when it is `PLAIN`.
    #[test]
    fn a_stored_secret_config_value_is_served_masked() {
        let secret = detail(
            "SetPropertyCommand",
            json!({"property": "apiKey2", "value": "s3cr3t", "valueType": "SECRET"}),
        );
        assert_eq!(secret["value"], "***");
        let plain = detail(
            "SetPropertyCommand",
            json!({"property": "colour", "value": "blue", "valueType": "PLAIN"}),
        );
        assert_eq!(plain["value"], "blue");
    }

    #[test]
    fn a_row_without_json_is_served_without_json() {
        let log = AuditLog::new("Thing", "thg_1", "SomeCommand", None, None);
        assert!(AuditLogDetailResponse::from(log).operation_json.is_none());
    }
}
