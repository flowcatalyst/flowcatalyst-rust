//! Message publishing and dev-mode seeding.

use super::model::{PublishMessageRequest, PublishMessageResponse};
use super::{AppState, SimpleState};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use fc_common::{MediationType, Message};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

/// Publish a message
#[utoipa::path(
    post,
    path = "/messages",
    tag = "messages",
    request_body = PublishMessageRequest,
    responses(
        (status = 200, description = "Message published", body = PublishMessageResponse),
        (status = 500, description = "Failed to publish")
    )
)]
pub(crate) async fn publish_message(
    State(state): State<AppState>,
    Json(req): Json<PublishMessageRequest>,
) -> Response {
    let message_id = Uuid::new_v4().to_string();

    let message = Message {
        id: message_id.clone(),
        pool_code: req.pool_code.unwrap_or_else(|| "DEFAULT".to_string()),
        auth_token: None,
        signing_secret: None,
        mediation_type: MediationType::HTTP,
        mediation_target: req
            .mediation_target
            .unwrap_or_else(|| "http://localhost:8080/echo".to_string()),
        message_group_id: req.message_group_id,
        high_priority: false,
        dispatch_mode: fc_common::DispatchMode::default(),
        dispatch_mode_specified: true,
    };

    match state.publisher.publish(message).await {
        Ok(_) => (
            StatusCode::OK,
            Json(PublishMessageResponse {
                message_id,
                status: "ACCEPTED".to_string(),
            }),
        )
            .into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "Failed to publish message" })),
        )
            .into_response(),
    }
}

/// Simple publish message (for simple router)
pub(crate) async fn simple_publish_message(
    State(state): State<SimpleState>,
    Json(req): Json<PublishMessageRequest>,
) -> Response {
    let message_id = Uuid::new_v4().to_string();

    let message = Message {
        id: message_id.clone(),
        pool_code: req.pool_code.unwrap_or_else(|| "DEFAULT".to_string()),
        auth_token: None,
        signing_secret: None,
        mediation_type: MediationType::HTTP,
        mediation_target: req
            .mediation_target
            .unwrap_or_else(|| "http://localhost:8080/echo".to_string()),
        message_group_id: req.message_group_id,
        high_priority: false,
        dispatch_mode: fc_common::DispatchMode::default(),
        dispatch_mode_specified: true,
    };

    match state.publisher.publish(message).await {
        Ok(_) => (
            StatusCode::OK,
            Json(PublishMessageResponse {
                message_id,
                status: "ACCEPTED".to_string(),
            }),
        )
            .into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "Failed to publish message" })),
        )
            .into_response(),
    }
}

/// Message seed request (matches Java format)
#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct SeedMessageRequest {
    count: Option<u32>,
    queue: Option<String>,
    endpoint: Option<String>,
    #[serde(rename = "messageGroupMode")]
    message_group_mode: Option<String>,
}

/// Message seed response
#[derive(Serialize, ToSchema)]
pub(crate) struct SeedMessageResponse {
    status: String,
    #[serde(rename = "messagesSent", skip_serializing_if = "Option::is_none")]
    messages_sent: Option<u32>,
    #[serde(rename = "totalRequested", skip_serializing_if = "Option::is_none")]
    total_requested: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

/// Seed test messages
#[utoipa::path(
    post,
    path = "/api/seed/messages",
    tag = "test",
    request_body = SeedMessageRequest,
    responses(
        (status = 200, description = "Messages seeded", body = SeedMessageResponse)
    )
)]
pub(crate) async fn seed_messages(
    State(state): State<AppState>,
    Json(req): Json<SeedMessageRequest>,
) -> Json<SeedMessageResponse> {
    let count = req.count.unwrap_or(10).min(1000);
    let endpoint = req.endpoint.unwrap_or_else(|| "fast".to_string());
    let _queue = req.queue.unwrap_or_else(|| "high".to_string());
    let message_group_mode = req.message_group_mode.unwrap_or_else(|| "1of8".to_string()); // Java default

    // Resolve endpoint
    let target = match endpoint.as_str() {
        "fast" => "http://localhost:8080/api/test/fast",
        "slow" => "http://localhost:8080/api/test/slow",
        "faulty" => "http://localhost:8080/api/test/faulty",
        "fail" => "http://localhost:8080/api/test/fail",
        "random" => "http://localhost:8080/api/test/faulty",
        other if other.starts_with("http") => other,
        _ => "http://localhost:8080/api/test/fast",
    };

    let mut sent = 0u32;
    for i in 0..count {
        let message_group_id = match message_group_mode.as_str() {
            "unique" => Some(format!("unique-{}", Uuid::new_v4())),
            "1of8" => Some(format!("group-{}", i % 8)),
            "single" => Some("single-group".to_string()),
            _ => None,
        };

        let message = Message {
            id: Uuid::new_v4().to_string(),
            pool_code: "DEFAULT".to_string(),
            auth_token: None,
            signing_secret: None,
            mediation_type: MediationType::HTTP,
            mediation_target: target.to_string(),
            message_group_id,
            high_priority: false,
            dispatch_mode: fc_common::DispatchMode::default(),
            dispatch_mode_specified: true,
        };

        if state.publisher.publish(message).await.is_ok() {
            sent += 1;
        }
    }

    Json(SeedMessageResponse {
        status: "success".to_string(),
        messages_sent: Some(sent),
        total_requested: Some(count),
        message: None,
    })
}
