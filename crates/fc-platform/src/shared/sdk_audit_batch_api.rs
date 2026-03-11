//! SDK Batch Audit Logs API — batch audit log ingest

use axum::{
    routing::post,
    extract::State,
    Json, Router,
};
use utoipa::ToSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use chrono::{DateTime, Utc};
use tracing::warn;

use crate::audit::entity::AuditLog;
use crate::audit::repository::AuditLogRepository;
use crate::application::repository::ApplicationRepository;
use crate::client::repository::ClientRepository;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;

// ── Request / Response DTOs ─────────────────────────────────────────────

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BatchAuditLogItem {
    pub entity_type: String,
    pub entity_id: String,
    pub operation: String,
    pub operation_data: Option<serde_json::Value>,
    pub principal_id: Option<String>,
    pub performed_at: Option<String>,
    pub application_code: Option<String>,
    pub client_code: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BatchAuditLogRequest {
    pub items: Vec<BatchAuditLogItem>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BatchAuditLogResult {
    pub id: String,
    pub status: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BatchAuditLogResponse {
    pub results: Vec<BatchAuditLogResult>,
}

// ── State ───────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct SdkAuditBatchState {
    pub audit_log_repo: Arc<AuditLogRepository>,
    pub application_repo: Arc<ApplicationRepository>,
    pub client_repo: Arc<ClientRepository>,
}

// ── Handler ─────────────────────────────────────────────────────────────

async fn batch_audit_logs(
    State(state): State<SdkAuditBatchState>,
    auth: Authenticated,
    Json(req): Json<BatchAuditLogRequest>,
) -> Result<Json<BatchAuditLogResponse>, PlatformError> {
    if req.items.len() > 100 {
        return Err(PlatformError::validation("Maximum 100 items per batch"));
    }

    let mut results = Vec::with_capacity(req.items.len());

    for item in req.items {
        // Resolve application_code → application_id
        let application_id = if let Some(ref code) = item.application_code {
            match state.application_repo.find_by_code(code).await? {
                Some(app) => Some(app.id),
                None => {
                    warn!(application_code = %code, "Batch audit log: unknown application code, skipping");
                    results.push(BatchAuditLogResult {
                        id: String::new(),
                        status: "SKIPPED".to_string(),
                    });
                    continue;
                }
            }
        } else {
            None
        };

        // Resolve client_code → client_id
        let client_id = if let Some(ref code) = item.client_code {
            match state.client_repo.find_by_identifier(code).await? {
                Some(client) => Some(client.id),
                None => {
                    warn!(client_code = %code, "Batch audit log: unknown client code, skipping");
                    results.push(BatchAuditLogResult {
                        id: String::new(),
                        status: "SKIPPED".to_string(),
                    });
                    continue;
                }
            }
        } else {
            None
        };

        // Check client access when a client_id is resolved
        if let Some(ref cid) = client_id {
            if !auth.0.can_access_client(cid) {
                warn!(
                    client_id = %cid,
                    principal_id = %auth.0.principal_id,
                    "Batch audit log: principal cannot access client, skipping"
                );
                results.push(BatchAuditLogResult {
                    id: String::new(),
                    status: "SKIPPED".to_string(),
                });
                continue;
            }
        }

        // Parse performed_at or default to now
        let performed_at: DateTime<Utc> = item
            .performed_at
            .as_deref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(Utc::now);

        // Build audit log
        let mut log = AuditLog::new(
            &item.entity_type,
            &item.entity_id,
            &item.operation,
            item.operation_data,
            item.principal_id,
        );
        log.performed_at = performed_at;
        if let Some(app_id) = application_id {
            log = log.with_application_id(app_id);
        }
        if let Some(cid) = client_id {
            log = log.with_client_id(cid);
        }

        let id = log.id.clone();
        state.audit_log_repo.insert(&log).await?;

        results.push(BatchAuditLogResult {
            id,
            status: "SUCCESS".to_string(),
        });
    }

    Ok(Json(BatchAuditLogResponse { results }))
}

// ── Router ──────────────────────────────────────────────────────────────

pub fn sdk_audit_batch_router(state: SdkAuditBatchState) -> Router {
    Router::new()
        .route("/batch", post(batch_audit_logs))
        .with_state(state)
}
