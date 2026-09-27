//! Subscription management operations.

use super::applications::CreatedResponse;
use super::{ClientError, FlowCatalystClient};
use serde::{Deserialize, Serialize};

/// Paginated list of subscriptions — `GET /api/subscriptions`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionListResponse {
    pub subscriptions: Vec<SubscriptionResponse>,
    #[serde(default)]
    pub total: u64,
}

/// Custom config entry returned on subscription responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigEntry {
    pub key: String,
    pub value: String,
}

/// Request to create a subscription.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CreateSubscriptionRequest {
    /// Unique code for this subscription
    pub code: String,
    /// Human-readable name
    pub name: String,
    /// Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Webhook endpoint URL
    pub endpoint: String,
    /// Connection ID (references msg_connections, optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection_id: Option<String>,
    /// Event type bindings (patterns with optional filters)
    #[serde(default)]
    pub event_types: Vec<EventTypeBinding>,
    /// Client ID for multi-tenant scoping
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// Dispatch pool ID for rate limiting
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispatch_pool_id: Option<String>,
    /// Service account ID for authentication
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_account_id: Option<String>,
    /// Dispatch mode
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// Webhook timeout in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u32>,
    /// Maximum retry attempts
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<u32>,
    /// Send raw data only (no envelope)
    #[serde(default)]
    pub data_only: bool,
}

/// Event type binding with wildcard pattern support.
///
/// Supports patterns like `"orders:*:*:*"` to match all events from the orders app.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventTypeBinding {
    /// Event type code or pattern (supports `*` wildcard per segment)
    pub event_type_code: String,
    /// Optional filter expression
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
}

/// Request to update a subscription.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSubscriptionRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<u32>,
}

/// Subscription response from the platform API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionResponse {
    pub id: String,
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub client_identifier: Option<String>,
    #[serde(default)]
    pub event_types: Vec<EventTypeBinding>,
    pub endpoint: String,
    #[serde(default)]
    pub connection_id: Option<String>,
    #[serde(default)]
    pub queue: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    pub status: String,
    #[serde(default)]
    pub max_age_seconds: u32,
    #[serde(default)]
    pub dispatch_pool_id: Option<String>,
    #[serde(default)]
    pub dispatch_pool_code: Option<String>,
    #[serde(default)]
    pub delay_seconds: u32,
    #[serde(default)]
    pub sequence: i32,
    pub mode: String,
    #[serde(default)]
    pub timeout_seconds: u32,
    #[serde(default)]
    pub max_retries: u32,
    #[serde(default)]
    pub service_account_id: Option<String>,
    #[serde(default)]
    pub data_only: bool,
    #[serde(default)]
    pub application_code: Option<String>,
    #[serde(default)]
    pub client_scoped: bool,
    /// Per-subscription custom configuration entries (key/value pairs).
    /// Returned by the platform as part of every subscription response.
    #[serde(default)]
    pub custom_config: Vec<ConfigEntry>,
    pub created_at: String,
    pub updated_at: String,
}

/// Request body for the per-resource sync endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncSubscriptionsRequest {
    pub subscriptions: Vec<SyncSubscriptionItem>,
}

/// A subscription item for sync — matches platform's SyncSubscriptionInput.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncSubscriptionItem {
    pub code: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Webhook endpoint URL
    pub target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection_id: Option<String>,
    pub event_types: Vec<SyncEventTypeBinding>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispatch_pool_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u32>,
    #[serde(default)]
    pub data_only: bool,
}

/// Event type binding for subscription sync.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncEventTypeBinding {
    pub event_type_code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
}

/// Subscriptions resource accessor — created via [`FlowCatalystClient::subscriptions`].
pub struct Subscriptions<'a> {
    pub(crate) client: &'a FlowCatalystClient,
}

impl Subscriptions<'_> {
    /// Create a new subscription.
    ///
    /// Returns `{ id }` only. Call `get(&id)` if you need the full record.
    pub async fn create(
        &self,
        req: &CreateSubscriptionRequest,
    ) -> Result<CreatedResponse, ClientError> {
        self.client.post("/api/subscriptions", req).await
    }

    /// Get a subscription by ID.
    pub async fn get(&self, id: &str) -> Result<SubscriptionResponse, ClientError> {
        self.client.get(&format!("/api/subscriptions/{}", id)).await
    }

    /// List subscriptions with optional filters.
    pub async fn list(
        &self,
        client_id: Option<&str>,
        status: Option<&str>,
    ) -> Result<SubscriptionListResponse, ClientError> {
        let mut params = Vec::new();
        if let Some(s) = status {
            params.push(("status", s.to_string()));
        }
        if let Some(cid) = client_id {
            params.push(("clientId", cid.to_string()));
        }
        let query = FlowCatalystClient::query_string(&params);
        self.client
            .get(&format!("/api/subscriptions{}", query))
            .await
    }

    /// Update a subscription (204). Call `get(id)` for the updated record.
    pub async fn update(
        &self,
        id: &str,
        req: &UpdateSubscriptionRequest,
    ) -> Result<(), ClientError> {
        self.client
            .put_empty(&format!("/api/subscriptions/{}", id), req)
            .await
    }

    /// Pause a subscription.
    pub async fn pause(&self, id: &str) -> Result<(), ClientError> {
        self.client
            .post_empty(&format!("/api/subscriptions/{}/pause", id))
            .await
    }

    /// Resume a subscription.
    pub async fn resume(&self, id: &str) -> Result<(), ClientError> {
        self.client
            .post_empty(&format!("/api/subscriptions/{}/resume", id))
            .await
    }

    /// Delete a subscription.
    pub async fn delete(&self, id: &str) -> Result<(), ClientError> {
        self.client
            .delete_req(&format!("/api/subscriptions/{}", id))
            .await
    }

    /// Sync subscriptions for an application — declarative reconciliation
    /// against `POST /api/applications/{appCode}/subscriptions/sync`.
    pub async fn sync(
        &self,
        app_code: &str,
        req: &SyncSubscriptionsRequest,
        remove_unlisted: bool,
    ) -> Result<crate::client::SyncResult, ClientError> {
        let query = if remove_unlisted {
            "?removeUnlisted=true"
        } else {
            ""
        };
        self.client
            .post(
                &format!("/api/applications/{}/subscriptions/sync{}", app_code, query),
                req,
            )
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `source` is a plain `Option<String>`, not a closed enum — a
    /// `FUNCTION`-sourced subscription (function promotion) must deserialize
    /// the same as any other source value.
    #[test]
    fn deserializes_function_source() {
        let json = r#"{
            "id": "sub_1",
            "code": "orders-shipped",
            "name": "Orders Shipped",
            "endpoint": "https://example.com/webhook",
            "source": "FUNCTION",
            "status": "ACTIVE",
            "mode": "IMMEDIATE",
            "createdAt": "2026-01-01T00:00:00Z",
            "updatedAt": "2026-01-01T00:00:00Z"
        }"#;

        let sub: SubscriptionResponse = serde_json::from_str(json).unwrap();
        assert_eq!(sub.source.as_deref(), Some("FUNCTION"));
    }

    #[tokio::test]
    async fn create_reads_the_id_update_accepts_204_and_list_sends_client_id() {
        use crate::client::test_support::MockPlatform;
        let stub = MockPlatform::start(&[
            ("POST", "/api/subscriptions", 201, r#"{"id":"sub_1"}"#),
            ("PUT", "/api/subscriptions/sub_1", 204, ""),
            (
                "GET",
                "/api/subscriptions",
                200,
                r#"{"subscriptions":[],"total":0}"#,
            ),
        ])
        .await;
        let c = stub.client();
        let created = c
            .subscriptions()
            .create(&CreateSubscriptionRequest {
                code: "ship".into(),
                name: "Ship".into(),
                endpoint: "https://example.com/hook".into(),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(created.id, "sub_1");
        c.subscriptions()
            .update(
                "sub_1",
                &UpdateSubscriptionRequest {
                    max_retries: Some(3),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        c.subscriptions()
            .list(Some("clt_1"), Some("ACTIVE"))
            .await
            .unwrap();
        let reqs = stub.requests();
        assert_eq!(reqs[1].json(), serde_json::json!({"maxRetries": 3}));
        assert_eq!(
            reqs[2].query_pairs(),
            vec![
                ("status".to_string(), "ACTIVE".to_string()),
                ("clientId".to_string(), "clt_1".to_string())
            ]
        );
    }
}
