//! Warning management endpoints: list/filter, acknowledge, clear.

use super::AppState;
use crate::warning::parse_severity;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use fc_common::{Warning, WarningCategory};
use serde::{Deserialize, Serialize};
use tracing::debug;
use utoipa::ToSchema;

/// Query params for warnings endpoint
#[derive(Deserialize, Default, ToSchema)]
pub struct WarningsQuery {
    /// Filter by severity: INFO, WARN, ERROR, CRITICAL
    pub severity: Option<String>,
    /// Filter by category: ROUTING, PROCESSING, CONFIGURATION, etc.
    pub category: Option<String>,
    /// Filter by acknowledged status
    pub acknowledged: Option<bool>,
}

/// List warnings with optional filters
#[utoipa::path(
    get,
    path = "/warnings",
    tag = "warnings",
    params(
        ("severity" = Option<String>, Query, description = "Filter by severity"),
        ("category" = Option<String>, Query, description = "Filter by category"),
        ("acknowledged" = Option<bool>, Query, description = "Filter by acknowledged status")
    ),
    responses(
        (status = 200, description = "List of warnings", body = Vec<Warning>)
    )
)]
pub(crate) async fn list_warnings(
    State(state): State<AppState>,
    Query(query): Query<WarningsQuery>,
) -> Json<Vec<Warning>> {
    let mut warnings = if let Some(false) = query.acknowledged {
        state.warning_service.get_unacknowledged_warnings()
    } else {
        state.warning_service.get_all_warnings()
    };

    // Filter by severity if specified
    if let Some(ref sev_str) = query.severity {
        if let Some(sev) = parse_severity(sev_str) {
            warnings.retain(|w| w.severity == sev);
        }
    }

    // Filter by category if specified
    if let Some(ref cat_str) = query.category {
        let category = match cat_str.to_uppercase().as_str() {
            "ROUTING" => Some(WarningCategory::Routing),
            "PROCESSING" => Some(WarningCategory::Processing),
            "CONFIGURATION" => Some(WarningCategory::Configuration),
            "GROUPTHREADRESTART" => Some(WarningCategory::GroupThreadRestart),
            "RATELIMITING" => Some(WarningCategory::RateLimiting),
            "QUEUECONNECTIVITY" => Some(WarningCategory::QueueConnectivity),
            "POOLCAPACITY" => Some(WarningCategory::PoolCapacity),
            "CONSUMERHEALTH" => Some(WarningCategory::ConsumerHealth),
            "RESOURCE" => Some(WarningCategory::Resource),
            _ => None,
        };
        if let Some(cat) = category {
            warnings.retain(|w| w.category == cat);
        }
    }

    // Sort by created_at descending (newest first)
    warnings.sort_by_key(|w| std::cmp::Reverse(w.created_at));

    Json(warnings)
}

/// Acknowledge a warning
#[utoipa::path(
    post,
    path = "/warnings/{id}/acknowledge",
    tag = "warnings",
    params(
        ("id" = String, Path, description = "Warning ID to acknowledge")
    ),
    responses(
        (status = 200, description = "Warning acknowledged"),
        (status = 404, description = "Warning not found")
    )
)]
pub(crate) async fn acknowledge_warning(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    if state.warning_service.acknowledge_warning(&id) {
        debug!(id = %id, "Warning acknowledged");
        (
            StatusCode::OK,
            Json(serde_json::json!({ "acknowledged": true })),
        )
            .into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Warning not found" })),
        )
            .into_response()
    }
}

/// Acknowledge all warnings
#[utoipa::path(
    post,
    path = "/warnings/acknowledge-all",
    tag = "warnings",
    responses(
        (status = 200, description = "All warnings acknowledged")
    )
)]
pub(crate) async fn acknowledge_all_warnings(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let count = state.warning_service.acknowledge_matching(|_| true);
    debug!(count = count, "Acknowledged all warnings");
    Json(serde_json::json!({ "acknowledged": count }))
}

/// Get critical warnings
#[utoipa::path(
    get,
    path = "/warnings/critical",
    tag = "warnings",
    responses(
        (status = 200, description = "Critical warnings", body = Vec<Warning>)
    )
)]
pub(crate) async fn get_critical_warnings(State(state): State<AppState>) -> Json<Vec<Warning>> {
    Json(state.warning_service.get_critical_warnings())
}

/// Warning format for dashboard
#[derive(Serialize, ToSchema)]
pub(crate) struct DashboardWarning {
    id: String,
    timestamp: String,
    severity: String,
    category: String,
    source: String,
    message: String,
    acknowledged: bool,
}

/// Warnings endpoint for dashboard
#[utoipa::path(
    get,
    path = "/monitoring/warnings",
    tag = "monitoring",
    responses(
        (status = 200, description = "Warnings for dashboard", body = Vec<DashboardWarning>)
    )
)]
pub(crate) async fn dashboard_warnings_handler(
    State(state): State<AppState>,
) -> Json<Vec<DashboardWarning>> {
    let warnings = state.warning_service.get_all_warnings();

    let result: Vec<DashboardWarning> = warnings
        .into_iter()
        .map(|w| DashboardWarning {
            id: w.id,
            timestamp: w.created_at.to_rfc3339(),
            severity: format!("{:?}", w.severity).to_uppercase(),
            category: format!("{:?}", w.category).to_uppercase(),
            source: w.source,
            message: w.message,
            acknowledged: w.acknowledged,
        })
        .collect();

    Json(result)
}

/// Get unacknowledged warnings
#[utoipa::path(
    get,
    path = "/warnings/unacknowledged",
    tag = "warnings",
    responses(
        (status = 200, description = "Unacknowledged warnings", body = Vec<Warning>)
    )
)]
pub(crate) async fn get_unacknowledged_warnings(
    State(state): State<AppState>,
) -> Json<Vec<Warning>> {
    Json(state.warning_service.get_unacknowledged_warnings())
}

/// Get warnings by severity
#[utoipa::path(
    get,
    path = "/monitoring/warnings/severity/{severity}",
    tag = "warnings",
    params(
        ("severity" = String, Path, description = "Severity level: CRITICAL, ERROR, WARN, INFO")
    ),
    responses(
        (status = 200, description = "Warnings of specified severity", body = Vec<Warning>)
    )
)]
pub(crate) async fn get_warnings_by_severity(
    State(state): State<AppState>,
    Path(severity): Path<String>,
) -> Json<Vec<Warning>> {
    let warnings = match parse_severity(&severity) {
        Some(sev) => state.warning_service.get_warnings_by_severity(sev),
        None => vec![],
    };

    Json(warnings)
}

/// Acknowledge warning (monitoring path for Java compatibility)
#[utoipa::path(
    post,
    path = "/monitoring/warnings/{id}/acknowledge",
    tag = "warnings",
    params(
        ("id" = String, Path, description = "Warning ID to acknowledge")
    ),
    responses(
        (status = 200, description = "Warning acknowledged"),
        (status = 404, description = "Warning not found")
    )
)]
pub(crate) async fn monitoring_acknowledge_warning(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    if state.warning_service.acknowledge_warning(&id) {
        debug!(id = %id, "Warning acknowledged via monitoring endpoint");
        (
            StatusCode::OK,
            Json(serde_json::json!({ "status": "success" })),
        )
            .into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Warning not found" })),
        )
            .into_response()
    }
}

/// Query for clearing old warnings
#[derive(Deserialize, Default, ToSchema)]
pub(crate) struct ClearWarningsQuery {
    /// Hours old (default 24)
    hours: Option<i64>,
}

/// Clear all warnings
#[utoipa::path(
    delete,
    path = "/warnings",
    tag = "warnings",
    responses(
        (status = 200, description = "All warnings cleared")
    )
)]
pub(crate) async fn clear_all_warnings(State(state): State<AppState>) -> Json<serde_json::Value> {
    let count = state.warning_service.get_all_warnings().len();
    // Clear by acknowledging and then removing old
    state.warning_service.acknowledge_matching(|_| true);
    state.warning_service.clear_old_warnings(0);
    debug!(count = count, "Cleared all warnings");
    Json(serde_json::json!({ "status": "success", "cleared": count }))
}

/// Clear old warnings
#[utoipa::path(
    delete,
    path = "/warnings/old",
    tag = "warnings",
    params(
        ("hours" = Option<i64>, Query, description = "Clear warnings older than this many hours (default 24)")
    ),
    responses(
        (status = 200, description = "Old warnings cleared")
    )
)]
pub(crate) async fn clear_old_warnings(
    State(state): State<AppState>,
    Query(query): Query<ClearWarningsQuery>,
) -> Json<serde_json::Value> {
    let hours = query.hours.unwrap_or(24);
    let removed = state.warning_service.clear_old_warnings(hours);
    debug!(hours = hours, removed = removed, "Cleared old warnings");
    Json(serde_json::json!({ "status": "success", "removed": removed }))
}

#[cfg(test)]
mod tests {
    use super::parse_severity;
    use fc_common::WarningSeverity;

    #[test]
    fn test_severity_parsing() {
        let cases = [
            ("INFO", Some(WarningSeverity::Info)),
            ("WARN", Some(WarningSeverity::Warn)),
            ("WARNING", Some(WarningSeverity::Warn)),
            ("ERROR", Some(WarningSeverity::Error)),
            ("CRITICAL", Some(WarningSeverity::Critical)),
            ("warning", Some(WarningSeverity::Warn)),
            ("UNKNOWN", None),
        ];

        for (input, expected) in cases {
            assert_eq!(parse_severity(input), expected);
        }
    }
}
