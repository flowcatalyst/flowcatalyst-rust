//! SDK Batch APIs — batch event ingest.
//!
//! The handler durably stores events; fan-out (subscription matching →
//! dispatch jobs → queue) runs out-of-band in the stream processor's
//! `EventFanOutService` (fc-stream). The request returns as soon as the
//! events are committed; the fan-out service picks them up off the
//! partial `idx_msg_events_unfanned` index.

use axum::{
    extract::{DefaultBodyLimit, State},
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::ToSchema;

use crate::client::repository::ClientRepository;
use crate::event::entity::{ContextData, Event};
use crate::event::repository::EventRepository;
use crate::permissions;
use crate::shared::authorization_service::checks;
use crate::shared::caller_reach;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;

// ── Caller-supplied ids ─────────────────────────────────────────────────

/// Width of `msg_events.id` and `msg_dispatch_jobs.id` (`VARCHAR(13)`).
pub const MESSAGE_ID_MAX_LEN: usize = 13;

/// A caller-supplied event or dispatch-job id, honoured as Go honours it
/// (owner decision #24). Blank means none, and the platform mints one.
/// Otherwise it must fit the column, 1 to 13 ASCII letters, digits, `_` or
/// `-` (an SDK sends a 13-character TSID); anything else would fail the
/// whole insert with a 500, so it is a 400 `INVALID_ID` instead.
pub fn supplied_id(raw: Option<&str>) -> Result<Option<String>, PlatformError> {
    let Some(raw) = raw.filter(|r| !r.trim().is_empty()) else {
        return Ok(None);
    };
    let valid = raw.len() <= MESSAGE_ID_MAX_LEN
        && raw
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    if !valid {
        let shown: String = raw.chars().take(40).collect();
        return Err(PlatformError::bad_request_code(
            "INVALID_ID",
            format!("id '{shown}' must be 1 to {MESSAGE_ID_MAX_LEN} letters, digits, '_' or '-'"),
        ));
    }
    Ok(Some(raw.to_string()))
}

/// 409 `DUPLICATE_ID` (Java ruling 17c, security-fixes-2026-09-24 S3.3).
pub fn duplicate_id(message: String) -> PlatformError {
    PlatformError::Coded {
        status: axum::http::StatusCode::CONFLICT,
        code: "DUPLICATE_ID".to_string(),
        message,
        details: Default::default(),
    }
}

/// The dispatch-job ids a batch supplies: each valid ([`supplied_id`]) and
/// named once. A repeat within the batch refuses the whole batch 409
/// `DUPLICATE_ID`, as does (at insert) an id that already names a job.
#[derive(Default)]
pub struct SuppliedJobIds {
    seen: std::collections::HashSet<String>,
    ids: Vec<String>,
}

impl SuppliedJobIds {
    /// The id `raw` supplies, if any, claimed for this batch.
    pub fn claim(&mut self, raw: Option<&str>) -> Result<Option<String>, PlatformError> {
        let Some(id) = supplied_id(raw)? else {
            return Ok(None);
        };
        if !self.seen.insert(id.clone()) {
            return Err(duplicate_id(format!(
                "dispatch job id '{id}' appears more than once in this batch"
            )));
        }
        self.ids.push(id.clone());
        Ok(Some(id))
    }

    pub fn ids(&self) -> &[String] {
        &self.ids
    }
}

/// The refusal for supplied ids that already name a job.
pub fn job_ids_taken(taken: &[String]) -> PlatformError {
    duplicate_id(format!(
        "dispatch job id already exists: {}",
        taken.join(", ")
    ))
}

// ── Batch Events ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BatchEventItem {
    /// A caller-supplied event id (Go `BatchEventItem.ID`); minted when
    /// absent. See [`supplied_id`].
    pub id: Option<String>,
    pub spec_version: Option<String>,
    /// Event type — accepts both `type` (camelCase API) and `event_type` (SDK outbox payload).
    #[serde(alias = "event_type")]
    pub r#type: String,
    pub source: Option<String>,
    pub subject: Option<String>,
    pub data: Option<serde_json::Value>,
    #[serde(alias = "correlation_id")]
    pub correlation_id: Option<String>,
    #[serde(alias = "causation_id")]
    pub causation_id: Option<String>,
    #[serde(alias = "deduplication_id")]
    pub deduplication_id: Option<String>,
    #[serde(alias = "message_group")]
    pub message_group: Option<String>,
    #[serde(alias = "client_id")]
    pub client_id: Option<String>,
    /// The client's identifier, resolved to its id when `clientId` is absent
    /// (Go event/api/api.go:168-185). The Laravel SDK's outbox sends it.
    #[serde(alias = "client_code")]
    pub client_code: Option<String>,
    #[serde(alias = "context_data")]
    pub context_data: Option<serde_json::Value>,
}

/// `contextData` as `[{key, value}]`; a null or non-string value reads as
/// its text (null as empty), entries without a key are dropped.
fn context_entries(value: Option<serde_json::Value>) -> Vec<ContextData> {
    let Some(serde_json::Value::Array(items)) = value else {
        return Vec::new();
    };
    items
        .into_iter()
        .filter_map(|item| {
            let key = item.get("key")?.as_str()?.to_string();
            let value = match item.get("value") {
                None | Some(serde_json::Value::Null) => String::new(),
                Some(serde_json::Value::String(v)) => v.clone(),
                Some(other) => other.to_string(),
            };
            Some(ContextData { key, value })
        })
        .collect()
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BatchEventsRequest {
    pub items: Vec<BatchEventItem>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BatchResultItem {
    pub id: String,
    pub status: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BatchResponse {
    pub results: Vec<BatchResultItem>,
}

#[derive(Clone)]
pub struct SdkEventsState {
    pub event_repo: Arc<EventRepository>,
    /// Resolves `clientCode` to a client id
    pub client_repo: Arc<ClientRepository>,
}

async fn batch_events(
    State(state): State<SdkEventsState>,
    auth: Authenticated,
    Json(req): Json<BatchEventsRequest>,
) -> Result<Json<BatchResponse>, PlatformError> {
    // Go event/api/api.go:137: the batch-write permission, checked before
    // anything is read.
    checks::require_permission(&auth.0, permissions::admin::BATCH_EVENTS_WRITE)?;
    if req.items.len() > 1000 {
        return Err(PlatformError::validation("Maximum 1000 items per batch"));
    }

    // Every distinct `clientCode` on items without a `clientId`, resolved in
    // one query. For an anchor an unknown code leaves the event unlinked
    // rather than failing the batch: the event is a fact (Go
    // event/api/api.go:151-185). A non-anchor never writes an unlinked row
    // (owner decision #24), so for it an unknown code is refused like one
    // naming another tenant.
    let mut codes: Vec<String> = req
        .items
        .iter()
        .filter(|i| caller_reach::non_blank(i.client_id.clone()).is_none())
        .filter_map(|i| caller_reach::non_blank(i.client_code.clone()))
        .collect();
    codes.sort();
    codes.dedup();
    let client_ids_by_code = state.client_repo.find_ids_by_identifiers(&codes).await?;

    // Supplied ids that already name a stored event: the event was ingested
    // before (an outbox retrying a batch it never saw acknowledged), so it
    // is acknowledged and not written again (owner decisions #18, #24).
    let supplied = req
        .items
        .iter()
        .map(|i| supplied_id(i.id.as_deref()))
        .collect::<Result<Vec<_>, _>>()?;
    let supplied_ids: Vec<String> = supplied.iter().flatten().cloned().collect();
    let stored_ids = state.event_repo.find_existing_ids(&supplied_ids).await?;

    let mut inserted_events = Vec::with_capacity(req.items.len());
    let mut results = Vec::with_capacity(req.items.len());

    // The whole batch is checked before anything is written: one item the
    // caller may not write refuses the request.
    for (item, supplied) in req.items.into_iter().zip(supplied) {
        let client_id = match caller_reach::non_blank(item.client_id) {
            Some(id) => Some(id),
            None => match caller_reach::non_blank(item.client_code) {
                Some(code) => match client_ids_by_code.get(&code) {
                    Some(id) => Some(id.clone()),
                    None if auth.0.is_anchor() => None,
                    None => {
                        return Err(PlatformError::forbidden_code(
                            "FORBIDDEN",
                            format!("No access to client: {code}"),
                        ))
                    }
                },
                None => None,
            },
        };
        let client_id = caller_reach::require_writable_client(&auth.0, client_id)?;

        let mut event = Event::new(
            item.r#type,
            item.source.unwrap_or_default(),
            item.data.unwrap_or(serde_json::Value::Null),
        );
        // A supplied id is the event's, set before the default
        // deduplication id (`<type>-<id>`) derives from it: a re-sent event
        // without a deduplication id of its own is then still recognised.
        if let Some(id) = supplied {
            if stored_ids.contains(&id) {
                results.push(BatchResultItem {
                    id,
                    status: "SUCCESS".to_string(),
                });
                continue;
            }
            event.id = id;
        }
        if let Some(spec_version) = item.spec_version.filter(|v| !v.is_empty()) {
            event.spec_version = spec_version;
        }
        event.subject = item.subject;
        event.correlation_id = item.correlation_id;
        event.causation_id = item.causation_id;
        // Go's event.New gives every event a deduplication id,
        // `<type>-<tsid>` (event/entity.go:70); a sent one wins.
        event.deduplication_id = Some(
            item.deduplication_id
                .filter(|d| !d.is_empty())
                .unwrap_or_else(|| format!("{}-{}", event.event_type, event.id)),
        );
        event.message_group = item.message_group;
        event.client_id = client_id;
        event.context_data = context_entries(item.context_data);

        results.push(BatchResultItem {
            id: event.id.clone(),
            status: "SUCCESS".to_string(),
        });
        inserted_events.push(event);
    }

    // Idempotent on the deduplication id (EventRepository::insert_many): a
    // duplicate is dropped and still reported SUCCESS, the outcome the
    // sender wants acknowledged (Go event/api/api.go:197-205).
    state.event_repo.insert_many(&inserted_events).await?;

    Ok(Json(BatchResponse { results }))
}

pub fn sdk_events_batch_router(state: SdkEventsState) -> Router {
    Router::new()
        .route("/batch", post(batch_events))
        .layer(DefaultBodyLimit::max(32 * 1024 * 1024))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_supplied_id_must_fit_the_column() {
        assert_eq!(supplied_id(None).unwrap(), None);
        assert_eq!(supplied_id(Some("  ")).unwrap(), None);
        assert_eq!(
            supplied_id(Some("0HZXEQ5Y8JY5Z")).unwrap().as_deref(),
            Some("0HZXEQ5Y8JY5Z")
        );
        assert_eq!(
            supplied_id(Some("a_b-c")).unwrap().as_deref(),
            Some("a_b-c")
        );
        for bad in ["0HZXEQ5Y8JY5ZX", "has space", "x'; drop", "ünï"] {
            let err = supplied_id(Some(bad)).unwrap_err();
            assert!(
                matches!(&err, PlatformError::Coded { code, .. } if code == "INVALID_ID"),
                "{bad}: {err:?}"
            );
        }
    }

    #[test]
    fn a_batch_names_each_supplied_id_once() {
        let mut ids = SuppliedJobIds::default();
        assert_eq!(ids.claim(None).unwrap(), None);
        assert_eq!(ids.claim(Some("A1")).unwrap().as_deref(), Some("A1"));
        assert_eq!(ids.claim(Some("B2")).unwrap().as_deref(), Some("B2"));
        let err = ids.claim(Some("A1")).unwrap_err();
        assert!(
            matches!(&err, PlatformError::Coded { status, code, .. }
                if *status == axum::http::StatusCode::CONFLICT && code == "DUPLICATE_ID"),
            "{err:?}"
        );
        assert_eq!(ids.ids(), ["A1".to_string(), "B2".to_string()]);
    }
}
