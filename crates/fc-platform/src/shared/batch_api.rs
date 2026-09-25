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
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;

// ── Batch Events ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BatchEventItem {
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
    _auth: Authenticated,
    Json(req): Json<BatchEventsRequest>,
) -> Result<Json<BatchResponse>, PlatformError> {
    if req.items.len() > 1000 {
        return Err(PlatformError::validation("Maximum 1000 items per batch"));
    }

    // Every distinct `clientCode` on items without a `clientId`, resolved in
    // one query. An unknown code leaves the event unlinked rather than
    // failing the batch: the event is a fact (Go event/api/api.go:151-185).
    let mut codes: Vec<String> = req
        .items
        .iter()
        .filter(|i| i.client_id.is_none())
        .filter_map(|i| i.client_code.clone().filter(|c| !c.is_empty()))
        .collect();
    codes.sort();
    codes.dedup();
    let client_ids_by_code = state.client_repo.find_ids_by_identifiers(&codes).await?;

    let mut inserted_events = Vec::with_capacity(req.items.len());

    for item in req.items {
        let mut event = Event::new(
            item.r#type,
            item.source.unwrap_or_default(),
            item.data.unwrap_or(serde_json::Value::Null),
        );
        if let Some(spec_version) = item.spec_version.filter(|v| !v.is_empty()) {
            event.spec_version = spec_version;
        }
        event.subject = item.subject;
        event.correlation_id = item.correlation_id;
        event.causation_id = item.causation_id;
        event.deduplication_id = item.deduplication_id;
        event.message_group = item.message_group;
        event.client_id = item.client_id.or_else(|| {
            item.client_code
                .as_deref()
                .and_then(|code| client_ids_by_code.get(code).cloned())
        });
        event.context_data = context_entries(item.context_data);

        inserted_events.push(event);
    }

    state.event_repo.insert_many(&inserted_events).await?;

    let results: Vec<BatchResultItem> = inserted_events
        .iter()
        .map(|e| BatchResultItem {
            id: e.id.clone(),
            status: "SUCCESS".to_string(),
        })
        .collect();

    Ok(Json(BatchResponse { results }))
}

pub fn sdk_events_batch_router(state: SdkEventsState) -> Router {
    Router::new()
        .route("/batch", post(batch_events))
        .layer(DefaultBodyLimit::max(32 * 1024 * 1024))
        .with_state(state)
}
