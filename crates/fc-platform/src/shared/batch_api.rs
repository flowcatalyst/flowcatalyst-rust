//! SDK Batch APIs — batch event and dispatch job ingest

use axum::{
    routing::post,
    extract::State,
    Json, Router,
};
use utoipa::ToSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::event::entity::Event;
use crate::event::repository::EventRepository;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;

// ── Batch Events ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BatchEventItem {
    pub spec_version: Option<String>,
    pub r#type: String,
    pub source: Option<String>,
    pub subject: Option<String>,
    pub data: Option<serde_json::Value>,
    pub correlation_id: Option<String>,
    pub causation_id: Option<String>,
    pub deduplication_id: Option<String>,
    pub message_group: Option<String>,
    pub client_id: Option<String>,
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
}

async fn batch_events(
    State(state): State<SdkEventsState>,
    _auth: Authenticated,
    Json(req): Json<BatchEventsRequest>,
) -> Result<Json<BatchResponse>, PlatformError> {
    if req.items.len() > 100 {
        return Err(PlatformError::validation("Maximum 100 items per batch"));
    }

    let mut results = Vec::with_capacity(req.items.len());
    for item in req.items {
        let mut event = Event::new(
            item.r#type,
            item.source.unwrap_or_default(),
            item.data.unwrap_or(serde_json::Value::Null),
        );
        event.subject = item.subject;
        let id = event.id.clone();
        state.event_repo.insert(&event).await?;
        results.push(BatchResultItem { id, status: "SUCCESS".to_string() });
    }

    Ok(Json(BatchResponse { results }))
}

pub fn sdk_events_batch_router(state: SdkEventsState) -> Router {
    Router::new()
        .route("/batch", post(batch_events))
        .with_state(state)
}
