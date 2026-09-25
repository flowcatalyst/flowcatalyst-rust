//! SDK Batch Audit Logs API — batch audit log ingest
//!
//! `POST /api/audit-logs/batch`, the SDK/outbox audit-ingest endpoint. It
//! matches the Go platform (`internal/platform/shared/sdk/audit_batch.go`):
//!
//! - authentication only; each item whose client the caller cannot access is
//!   `SKIPPED`, as is an item naming an unknown application or client code;
//! - an item without a `principalId` is refused in its own slot
//!   (`BAD_REQUEST`, `principalId is required`) and the rest still land;
//! - `performedAt` is RFC 3339 and falls back to now when absent or
//!   unparseable;
//! - `200 {results:[{id,status,error?}]}`, one result per item in order.
//!
//! Platform infrastructure ingest (CLAUDE.md, "Exceptions: Platform
//! Infrastructure Processing"): rows are written directly, not through a use
//! case. The whole batch costs one lookup query per code kind and one insert.

use std::collections::{BTreeSet, HashMap};

use axum::{
    body::Bytes,
    extract::State,
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use std::sync::Arc;
use tracing::warn;
use utoipa::ToSchema;

use crate::application::repository::ApplicationRepository;
use crate::audit::entity::AuditLog;
use crate::audit::repository::AuditLogRepository;
use crate::client::repository::ClientRepository;
use crate::shared::authorization_service::AuthContext;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;

/// Largest batch accepted, as in Go.
const MAX_BATCH: usize = 100;

// ── Request / Response DTOs ─────────────────────────────────────────────

/// One inbound audit row. Missing or `null` strings decode as empty, as Go's
/// decoder leaves them.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BatchAuditLogItem {
    #[serde(default, deserialize_with = "null_as_empty")]
    pub entity_type: String,
    #[serde(default, deserialize_with = "null_as_empty")]
    pub entity_id: String,
    #[serde(default, deserialize_with = "null_as_empty")]
    pub operation: String,
    pub operation_data: Option<serde_json::Value>,
    /// Required per item: an entry without an actor is refused, never
    /// attributed to the caller.
    pub principal_id: Option<String>,
    pub performed_at: Option<String>,
    pub application_code: Option<String>,
    pub client_code: Option<String>,
}

fn null_as_empty<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(Option::<String>::deserialize(d)?.unwrap_or_default())
}

#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BatchAuditLogRequest {
    #[serde(default)]
    pub items: Vec<BatchAuditLogItem>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BatchAuditLogResult {
    /// The stored row's id; empty for a `SKIPPED` or `BAD_REQUEST` item.
    pub id: String,
    /// `SUCCESS`, `SKIPPED` or `BAD_REQUEST`.
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl BatchAuditLogResult {
    fn success(id: String) -> Self {
        Self {
            id,
            status: "SUCCESS".to_string(),
            error: None,
        }
    }

    fn skipped() -> Self {
        Self {
            id: String::new(),
            status: "SKIPPED".to_string(),
            error: None,
        }
    }

    fn bad_request(error: &str) -> Self {
        Self {
            id: String::new(),
            status: "BAD_REQUEST".to_string(),
            error: Some(error.to_string()),
        }
    }
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

/// A 400 with a specific error code (Go's `httperror.BadRequest`, Java's
/// `HttpError.badRequest`).
fn bad_request(code: &str, message: impl Into<String>) -> Response {
    PlatformError::bad_request_code(code, message).into_response()
}

/// Decode the body the way Go's `json.NewDecoder(r.Body).Decode` does: the
/// first JSON value, whatever the content type; `null` is an empty batch.
fn decode_request(body: &[u8]) -> Result<BatchAuditLogRequest, String> {
    match serde_json::Deserializer::from_slice(body)
        .into_iter::<Option<BatchAuditLogRequest>>()
        .next()
    {
        Some(Ok(req)) => Ok(req.unwrap_or_default()),
        Some(Err(e)) => Err(e.to_string()),
        None => Err("EOF".to_string()),
    }
}

/// The distinct non-empty codes in `codes`, for one `ANY($1)` lookup.
fn distinct_codes<'a>(codes: impl Iterator<Item = Option<&'a String>>) -> Vec<String> {
    codes
        .flatten()
        .filter(|c| !c.is_empty())
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Turn the items into rows to insert and one result per item, given the
/// resolved `code -> id` maps. No I/O: every per-item decision (unknown code,
/// client access, principal, `performedAt`, redaction) happens here, in Go's
/// order (`audit_batch.go:85-178`).
fn plan_batch(
    items: Vec<BatchAuditLogItem>,
    app_ids: &HashMap<String, String>,
    client_ids: &HashMap<String, String>,
    auth: &AuthContext,
    now: DateTime<Utc>,
) -> (Vec<AuditLog>, Vec<BatchAuditLogResult>) {
    let mut logs = Vec::with_capacity(items.len());
    let mut results = Vec::with_capacity(items.len());

    for item in items {
        // An empty code is no code, as in Go.
        let application_id = match item.application_code.as_deref() {
            Some(code) if !code.is_empty() => match app_ids.get(code) {
                Some(id) => Some(id.clone()),
                None => {
                    warn!(application_code = %code, "Batch audit log: unknown application code, skipping");
                    results.push(BatchAuditLogResult::skipped());
                    continue;
                }
            },
            _ => None,
        };

        let client_id = match item.client_code.as_deref() {
            Some(code) if !code.is_empty() => match client_ids.get(code) {
                Some(id) => Some(id.clone()),
                None => {
                    warn!(client_code = %code, "Batch audit log: unknown client code, skipping");
                    results.push(BatchAuditLogResult::skipped());
                    continue;
                }
            },
            _ => None,
        };

        if let Some(cid) = client_id.as_deref() {
            if !auth.can_access_client(cid) {
                warn!(
                    client_id = %cid,
                    principal_id = %auth.principal_id,
                    "Batch audit log: principal cannot access client, skipping"
                );
                results.push(BatchAuditLogResult::skipped());
                continue;
            }
        }

        let performed_at = item
            .performed_at
            .as_deref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or(now);

        let principal_id = item
            .principal_id
            .as_deref()
            .map(str::trim)
            .unwrap_or_default();
        if principal_id.is_empty() {
            results.push(BatchAuditLogResult::bad_request("principalId is required"));
            continue;
        }

        // Backstop (owner spec docs/spec/audit-redaction.md, Java repo): SDKs
        // redact at the source, but SDK versions that predate that and apps
        // writing their own outbox rows do not, so the ingested document is
        // redacted by the name rule before it is stored. There is no live
        // command here to declare masked fields. The SDK DTOs send
        // `operationData` as a JSON-encoded string, which redact_document
        // reaches into as well.
        let operation_data = item
            .operation_data
            .map(|data| fc_common::audit_redaction::redact_document(&data, &[]));

        let mut log = AuditLog::new(
            item.entity_type,
            item.entity_id,
            item.operation,
            operation_data,
            Some(principal_id.to_string()),
        );
        log.performed_at = performed_at;
        log.application_id = application_id;
        log.client_id = client_id;

        results.push(BatchAuditLogResult::success(log.id.clone()));
        logs.push(log);
    }

    (logs, results)
}

async fn batch_audit_logs(
    State(state): State<SdkAuditBatchState>,
    auth: Authenticated,
    body: Bytes,
) -> Result<Response, PlatformError> {
    let req = match decode_request(&body) {
        Ok(req) => req,
        Err(e) => return Ok(bad_request("INVALID_JSON", e)),
    };
    if req.items.len() > MAX_BATCH {
        return Ok(bad_request(
            "BATCH_TOO_LARGE",
            format!("Maximum {MAX_BATCH} items per batch"),
        ));
    }

    let app_codes = distinct_codes(req.items.iter().map(|i| i.application_code.as_ref()));
    let client_codes = distinct_codes(req.items.iter().map(|i| i.client_code.as_ref()));
    let (app_ids, client_ids) = tokio::try_join!(
        state.application_repo.find_ids_by_codes(&app_codes),
        state.client_repo.find_ids_by_identifiers(&client_codes),
    )?;

    let (logs, results) = plan_batch(req.items, &app_ids, &client_ids, &auth.0, Utc::now());

    // One statement: the batch lands or fails as a whole.
    state.audit_log_repo.insert_batch(&logs).await?;

    Ok(Json(BatchAuditLogResponse { results }).into_response())
}

// ── Router ──────────────────────────────────────────────────────────────

pub fn sdk_audit_batch_router(state: SdkAuditBatchState) -> Router {
    Router::new()
        .route("/batch", post(batch_audit_logs))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::principal::entity::{PrincipalType, UserScope};
    use serde_json::json;
    use std::collections::HashSet;

    fn ctx(scope: UserScope, clients: &[&str]) -> AuthContext {
        AuthContext {
            principal_id: "prn_caller".to_string(),
            principal_type: PrincipalType::Service,
            scope,
            email: None,
            name: "caller".to_string(),
            accessible_clients: clients.iter().map(|c| c.to_string()).collect(),
            permissions: HashSet::new(),
            roles: Vec::new(),
            credential: crate::shared::authorization_service::Credential::BearerToken,
        }
    }

    fn items(value: serde_json::Value) -> Vec<BatchAuditLogItem> {
        decode_request(value.to_string().as_bytes())
            .expect("decode")
            .items
    }

    fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn statuses(results: &[BatchAuditLogResult]) -> Vec<&str> {
        results.iter().map(|r| r.status.as_str()).collect()
    }

    #[test]
    fn an_inaccessible_client_is_skipped_and_the_rest_are_kept() {
        let batch = items(json!({ "items": [
            { "entityType": "Order", "entityId": "o1", "operation": "CREATE",
              "principalId": "prn_a", "clientCode": "mine" },
            { "entityType": "Order", "entityId": "o2", "operation": "CREATE",
              "principalId": "prn_a", "clientCode": "theirs" },
            { "entityType": "Order", "entityId": "o3", "operation": "CREATE",
              "principalId": "prn_a" },
        ]}));
        let clients = map(&[("mine", "clt_mine"), ("theirs", "clt_theirs")]);
        let (logs, results) = plan_batch(
            batch,
            &HashMap::new(),
            &clients,
            &ctx(UserScope::Client, &["clt_mine"]),
            Utc::now(),
        );

        assert_eq!(statuses(&results), ["SUCCESS", "SKIPPED", "SUCCESS"]);
        assert_eq!(results[1].id, "");
        assert!(results[1].error.is_none());
        let stored: Vec<&str> = logs.iter().map(|l| l.entity_id.as_str()).collect();
        assert_eq!(stored, ["o1", "o3"]);
        assert_eq!(logs[0].client_id.as_deref(), Some("clt_mine"));
        assert_eq!(results[0].id, logs[0].id);
    }

    #[test]
    fn unknown_codes_are_skipped_and_empty_codes_are_no_code() {
        let batch = items(json!({ "items": [
            { "entityType": "Order", "entityId": "o1", "operation": "CREATE",
              "principalId": "prn_a", "applicationCode": "nope" },
            { "entityType": "Order", "entityId": "o2", "operation": "CREATE",
              "principalId": "prn_a", "clientCode": "nope" },
            { "entityType": "Order", "entityId": "o3", "operation": "CREATE",
              "principalId": "prn_a", "applicationCode": "", "clientCode": "" },
            { "entityType": "Order", "entityId": "o4", "operation": "CREATE",
              "principalId": "prn_a", "applicationCode": "orders" },
        ]}));
        let (logs, results) = plan_batch(
            batch,
            &map(&[("orders", "app_orders")]),
            &HashMap::new(),
            &ctx(UserScope::Anchor, &["*"]),
            Utc::now(),
        );

        assert_eq!(
            statuses(&results),
            ["SKIPPED", "SKIPPED", "SUCCESS", "SUCCESS"]
        );
        assert_eq!(logs[0].application_id, None);
        assert_eq!(logs[0].client_id, None);
        assert_eq!(logs[1].application_id.as_deref(), Some("app_orders"));
    }

    #[test]
    fn performed_at_is_honoured_and_a_bad_value_falls_back_to_now() {
        let batch = items(json!({ "items": [
            { "entityType": "Order", "entityId": "o1", "operation": "CREATE",
              "principalId": "prn_a", "performedAt": "2025-03-04T05:06:07.123+02:00" },
            { "entityType": "Order", "entityId": "o2", "operation": "CREATE",
              "principalId": "prn_a", "performedAt": "yesterday" },
            { "entityType": "Order", "entityId": "o3", "operation": "CREATE",
              "principalId": "prn_a" },
        ]}));
        let now = Utc::now();
        let (logs, _) = plan_batch(
            batch,
            &HashMap::new(),
            &HashMap::new(),
            &ctx(UserScope::Anchor, &["*"]),
            now,
        );

        let expected: DateTime<Utc> = "2025-03-04T03:06:07.123Z".parse().unwrap();
        assert_eq!(logs[0].performed_at, expected);
        assert_eq!(logs[1].performed_at, now);
        assert_eq!(logs[2].performed_at, now);
    }

    #[test]
    fn a_missing_principal_is_refused_in_its_own_slot() {
        let batch = items(json!({ "items": [
            { "entityType": "Order", "entityId": "o1", "operation": "CREATE" },
            { "entityType": "Order", "entityId": "o2", "operation": "CREATE",
              "principalId": "   " },
            { "entityType": "Order", "entityId": "o3", "operation": "CREATE",
              "principalId": " prn_actor " },
        ]}));
        let (logs, results) = plan_batch(
            batch,
            &HashMap::new(),
            &HashMap::new(),
            &ctx(UserScope::Anchor, &["*"]),
            Utc::now(),
        );

        assert_eq!(
            statuses(&results),
            ["BAD_REQUEST", "BAD_REQUEST", "SUCCESS"]
        );
        assert_eq!(results[0].error.as_deref(), Some("principalId is required"));
        assert_eq!(results[0].id, "");
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].principal_id.as_deref(), Some("prn_actor"));
    }

    #[test]
    fn operation_data_is_redacted() {
        let batch = items(json!({ "items": [
            { "entityType": "Order", "entityId": "o1", "operation": "CREATE",
              "principalId": "prn_a",
              "operationData": { "password": "x", "name": "kept" } },
        ]}));
        let (logs, _) = plan_batch(
            batch,
            &HashMap::new(),
            &HashMap::new(),
            &ctx(UserScope::Anchor, &["*"]),
            Utc::now(),
        );
        assert_eq!(
            logs[0].operation_json,
            Some(json!({ "password": "***", "name": "kept" }))
        );
    }

    #[test]
    fn the_body_decodes_like_go() {
        // Missing and null strings are empty; a null body or missing items
        // is an empty batch; malformed JSON is an error.
        let req = decode_request(br#"{"items":[{"entityType":null}]}"#).unwrap();
        assert_eq!(req.items[0].entity_type, "");
        assert_eq!(req.items[0].entity_id, "");
        assert!(decode_request(b"null").unwrap().items.is_empty());
        assert!(decode_request(b"{}").unwrap().items.is_empty());
        assert!(decode_request(b"").is_err());
        assert!(decode_request(b"{\"items\":").is_err());
        assert!(decode_request(br#"{"items":[{"principalId":5}]}"#).is_err());
    }

    #[test]
    fn codes_are_looked_up_once_each() {
        let batch = items(json!({ "items": [
            { "clientCode": "a" }, { "clientCode": "b" }, { "clientCode": "a" },
            { "clientCode": "" }, {},
        ]}));
        let codes = distinct_codes(batch.iter().map(|i| i.client_code.as_ref()));
        assert_eq!(codes, ["a", "b"]);
    }
}
