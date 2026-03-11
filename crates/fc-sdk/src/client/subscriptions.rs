//! Subscription management operations.

use serde::{Deserialize, Serialize};
use super::{FlowCatalystClient, ClientError, ListResponse};

/// Request to create a subscription.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CreateSubscriptionRequest {
    /// Unique code for this subscription
    pub code: String,
    /// Human-readable name
    pub name: String,
    /// Connection ID (webhook endpoint)
    pub connection_id: String,
    /// Event type bindings (patterns with optional filters)
    pub event_types: Vec<EventTypeBinding>,
    /// Dispatch mode: "immediate" or "block_on_error"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// Dispatch pool for rate limiting
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispatch_pool_id: Option<String>,
    /// Service account for authentication
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_account_id: Option<String>,
    /// Webhook timeout in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u32>,
    /// Maximum retry attempts
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<u32>,
    /// Send raw data only (no envelope)
    #[serde(default)]
    pub data_only: bool,
    /// Client ID for multi-tenant scoping
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Message group for FIFO ordering
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_group: Option<String>,
}

/// Event type binding with wildcard pattern support.
///
/// Supports patterns like `"orders:*:*:*"` to match all events from the orders app.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventTypeBinding {
    /// Event type code or pattern (supports `*` wildcard per segment)
    pub event_type_code: String,
    /// Optional filter expression
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
}

/// Request to update a subscription.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateSubscriptionRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<u32>,
}

/// Subscription response from the platform API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionResponse {
    pub id: String,
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub connection_id: String,
    pub status: String,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub event_types: Vec<EventTypeBinding>,
    #[serde(default)]
    pub dispatch_pool_id: Option<String>,
    #[serde(default)]
    pub service_account_id: Option<String>,
    #[serde(default)]
    pub timeout_seconds: Option<u32>,
    #[serde(default)]
    pub max_retries: Option<u32>,
    #[serde(default)]
    pub data_only: bool,
    #[serde(default)]
    pub client_id: Option<String>,
}

impl FlowCatalystClient {
    /// Create a new subscription.
    pub async fn create_subscription(
        &self,
        req: &CreateSubscriptionRequest,
    ) -> Result<SubscriptionResponse, ClientError> {
        self.post("/api/subscriptions", req).await
    }

    /// Get a subscription by ID.
    pub async fn get_subscription(&self, id: &str) -> Result<SubscriptionResponse, ClientError> {
        self.get(&format!("/api/subscriptions/{}", id)).await
    }

    /// List subscriptions with optional filters.
    pub async fn list_subscriptions(
        &self,
        client_id: Option<&str>,
        status: Option<&str>,
    ) -> Result<ListResponse<SubscriptionResponse>, ClientError> {
        let mut params = Vec::new();
        if let Some(cid) = client_id {
            params.push(format!("client_id={}", cid));
        }
        if let Some(s) = status {
            params.push(format!("status={}", s));
        }

        let query = if params.is_empty() {
            String::new()
        } else {
            format!("?{}", params.join("&"))
        };

        self.get(&format!("/api/subscriptions{}", query)).await
    }

    /// Update a subscription.
    pub async fn update_subscription(
        &self,
        id: &str,
        req: &UpdateSubscriptionRequest,
    ) -> Result<SubscriptionResponse, ClientError> {
        self.put(&format!("/api/subscriptions/{}", id), req).await
    }

    /// Pause a subscription.
    pub async fn pause_subscription(&self, id: &str) -> Result<(), ClientError> {
        self.post_empty(&format!("/api/subscriptions/{}/pause", id))
            .await
    }

    /// Resume a subscription.
    pub async fn resume_subscription(&self, id: &str) -> Result<(), ClientError> {
        self.post_empty(&format!("/api/subscriptions/{}/resume", id))
            .await
    }

    /// Delete a subscription.
    pub async fn delete_subscription(&self, id: &str) -> Result<(), ClientError> {
        self.delete_req(&format!("/api/subscriptions/{}", id)).await
    }
}
