//! HTTP Dispatcher for FlowCatalyst API
//!
//! Sends outbox items to the FlowCatalyst REST API endpoints.
//! Matches the Java FlowCatalystApiClient behavior.
//!
//! Routes items to the correct endpoint based on type:
//! - `/api/events/batch` for EVENT items
//! - `/api/dispatch/jobs/batch` for DISPATCH_JOB items
//! - `/api/audit/logs/batch` for AUDIT_LOG items

use std::sync::Arc;
use std::time::Duration;
use async_trait::async_trait;
use fc_common::{OutboxItem, OutboxItemType, OutboxStatus};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, warn};

use crate::message_group_processor::{
    DispatchResult, MessageDispatcher, BatchMessageDispatcher,
    BatchDispatchResult, BatchItemResult,
};

/// HTTP dispatcher configuration
#[derive(Debug, Clone)]
pub struct HttpDispatcherConfig {
    /// FlowCatalyst API base URL
    pub api_base_url: String,
    /// Optional Bearer token for authentication
    pub api_token: Option<String>,
    /// Connect timeout
    pub connect_timeout: Duration,
    /// Request timeout
    pub request_timeout: Duration,
}

impl Default for HttpDispatcherConfig {
    fn default() -> Self {
        Self {
            api_base_url: "http://localhost:8080".to_string(),
            api_token: None,
            connect_timeout: Duration::from_secs(10),
            request_timeout: Duration::from_secs(30),
        }
    }
}

/// Batch request payload (matches Java structure)
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchRequest {
    pub items: Vec<BatchItem>,
}

/// Single item in a batch request
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchItem {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_group: Option<String>,
    pub payload: serde_json::Value,
}

/// Batch response from the API
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchResponse {
    pub results: Vec<ItemResult>,
}

/// Result for a single item
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemResult {
    pub id: String,
    pub status: ItemStatus,
    #[serde(default)]
    pub error: Option<String>,
}

/// Item status from API response
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ItemStatus {
    Success,
    BadRequest,
    InternalError,
    Unauthorized,
    Forbidden,
    GatewayError,
}

impl ItemStatus {
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            ItemStatus::InternalError | ItemStatus::Unauthorized | ItemStatus::GatewayError
        )
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            ItemStatus::Success | ItemStatus::BadRequest | ItemStatus::Forbidden
        )
    }

    /// Convert to OutboxStatus for database storage
    pub fn to_outbox_status(&self) -> OutboxStatus {
        match self {
            ItemStatus::Success => OutboxStatus::SUCCESS,
            ItemStatus::BadRequest => OutboxStatus::BAD_REQUEST,
            ItemStatus::InternalError => OutboxStatus::INTERNAL_ERROR,
            ItemStatus::Unauthorized => OutboxStatus::UNAUTHORIZED,
            ItemStatus::Forbidden => OutboxStatus::FORBIDDEN,
            ItemStatus::GatewayError => OutboxStatus::GATEWAY_ERROR,
        }
    }
}

/// Result for a dispatched outbox item
#[derive(Debug, Clone)]
pub struct OutboxDispatchResult {
    pub id: String,
    pub status: OutboxStatus,
    pub error_message: Option<String>,
}

/// HTTP dispatcher that sends outbox items to FlowCatalyst API
pub struct HttpDispatcher {
    config: HttpDispatcherConfig,
    client: reqwest::Client,
}

impl HttpDispatcher {
    pub fn new(config: HttpDispatcherConfig) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .connect_timeout(config.connect_timeout)
            .timeout(config.request_timeout)
            .build()?;

        Ok(Self { config, client })
    }

    /// Get the API endpoint for a given item type
    fn endpoint_for_type(&self, item_type: OutboxItemType) -> String {
        format!("{}{}", self.config.api_base_url, item_type.api_path())
    }

    /// Send a batch of OutboxItems to the appropriate API endpoint
    pub async fn send_outbox_batch(&self, items: &[OutboxItem]) -> Vec<OutboxDispatchResult> {
        if items.is_empty() {
            return Vec::new();
        }

        // All items in a batch should have the same type (enforced by processor)
        let item_type = items[0].item_type;
        let url = self.endpoint_for_type(item_type);

        let batch_request = BatchRequest {
            items: items.iter().map(|item| BatchItem {
                id: item.id.clone(),
                message_group: item.message_group.clone(),
                payload: item.payload.clone(),
            }).collect(),
        };

        debug!("Sending batch of {} {} items to {}", items.len(), item_type, url);

        let mut request = self.client.post(&url).json(&batch_request);

        if let Some(ref token) = self.config.api_token {
            request = request.header("Authorization", format!("Bearer {}", token));
        }

        match request.send().await {
            Ok(response) => {
                let status = response.status();
                if status.is_success() {
                    match response.json::<BatchResponse>().await {
                        Ok(batch_response) => {
                            batch_response.results.into_iter().map(|r| OutboxDispatchResult {
                                id: r.id,
                                status: r.status.to_outbox_status(),
                                error_message: r.error,
                            }).collect()
                        }
                        Err(e) => {
                            error!("Failed to parse batch response: {}", e);
                            items.iter().map(|item| OutboxDispatchResult {
                                id: item.id.clone(),
                                status: OutboxStatus::INTERNAL_ERROR,
                                error_message: Some(format!("Parse error: {}", e)),
                            }).collect()
                        }
                    }
                } else {
                    let outbox_status = match status.as_u16() {
                        400 => OutboxStatus::BAD_REQUEST,
                        401 => OutboxStatus::UNAUTHORIZED,
                        403 => OutboxStatus::FORBIDDEN,
                        500 => OutboxStatus::INTERNAL_ERROR,
                        502 | 503 | 504 => OutboxStatus::GATEWAY_ERROR,
                        _ => OutboxStatus::INTERNAL_ERROR,
                    };

                    let error_body = response.text().await.unwrap_or_default();
                    warn!("Batch request failed with status {}: {}", status, error_body);

                    items.iter().map(|item| OutboxDispatchResult {
                        id: item.id.clone(),
                        status: outbox_status,
                        error_message: Some(format!("HTTP {}: {}", status, error_body)),
                    }).collect()
                }
            }
            Err(e) => {
                error!("HTTP request failed: {}", e);
                let error_msg = e.to_string();
                items.iter().map(|item| OutboxDispatchResult {
                    id: item.id.clone(),
                    status: OutboxStatus::GATEWAY_ERROR,
                    error_message: Some(error_msg.clone()),
                }).collect()
            }
        }
    }
}

#[async_trait]
impl MessageDispatcher for HttpDispatcher {
    async fn dispatch(&self, item: &OutboxItem) -> DispatchResult {
        let results = self.send_outbox_batch(&[item.clone()]).await;

        match results.first() {
            Some(result) => {
                if matches!(result.status, OutboxStatus::SUCCESS) {
                    DispatchResult::Success
                } else {
                    DispatchResult::Failure {
                        error: result.error_message.clone().unwrap_or_else(|| "Unknown error".to_string()),
                        retryable: result.status.is_retryable(),
                    }
                }
            }
            None => DispatchResult::Failure {
                error: "No result returned".to_string(),
                retryable: true,
            },
        }
    }
}

#[async_trait]
impl BatchMessageDispatcher for HttpDispatcher {
    async fn dispatch_batch(&self, items: &[OutboxItem]) -> BatchDispatchResult {
        let api_results = self.send_outbox_batch(items).await;

        let results = api_results.into_iter().map(|r| {
            let result = if matches!(r.status, OutboxStatus::SUCCESS) {
                DispatchResult::Success
            } else {
                DispatchResult::Failure {
                    error: r.error_message.clone().unwrap_or_else(|| "Unknown error".to_string()),
                    retryable: r.status.is_retryable(),
                }
            };
            BatchItemResult {
                item_id: r.id,
                result,
            }
        }).collect();

        BatchDispatchResult { results }
    }
}

/// Batch dispatcher for efficient bulk sending
pub struct BatchHttpDispatcher {
    dispatcher: Arc<HttpDispatcher>,
}

impl BatchHttpDispatcher {
    pub fn new(dispatcher: Arc<HttpDispatcher>) -> Self {
        Self { dispatcher }
    }

    /// Dispatch a batch of outbox items and return results
    pub async fn dispatch_batch(&self, items: &[OutboxItem]) -> Vec<OutboxDispatchResult> {
        self.dispatcher.send_outbox_batch(items).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_item_status_retryable() {
        assert!(ItemStatus::InternalError.is_retryable());
        assert!(ItemStatus::Unauthorized.is_retryable());
        assert!(ItemStatus::GatewayError.is_retryable());
        assert!(!ItemStatus::Success.is_retryable());
        assert!(!ItemStatus::BadRequest.is_retryable());
        assert!(!ItemStatus::Forbidden.is_retryable());
    }

    #[test]
    fn test_item_status_terminal() {
        assert!(ItemStatus::Success.is_terminal());
        assert!(ItemStatus::BadRequest.is_terminal());
        assert!(ItemStatus::Forbidden.is_terminal());
        assert!(!ItemStatus::InternalError.is_terminal());
        assert!(!ItemStatus::Unauthorized.is_terminal());
        assert!(!ItemStatus::GatewayError.is_terminal());
    }

    #[test]
    fn test_batch_item_from_outbox_item() {
        let item = OutboxItem {
            id: "test-1".to_string(),
            item_type: OutboxItemType::EVENT,
            message_group: Some("group-1".to_string()),
            payload: serde_json::json!({"key": "value"}),
            status: OutboxStatus::PENDING,
            retry_count: 0,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            error_message: None,
            client_id: None,
            payload_size: None,
            headers: None,
        };

        let batch_item = BatchItem {
            id: item.id.clone(),
            message_group: item.message_group.clone(),
            payload: item.payload.clone(),
        };

        assert_eq!(batch_item.id, "test-1");
        assert_eq!(batch_item.message_group, Some("group-1".to_string()));
        assert_eq!(batch_item.payload, serde_json::json!({"key": "value"}));
    }
}
