//! Subscription Entity
//!
//! Links event types to target endpoints for dispatch.

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use bson::serde_helpers::chrono_datetime_as_bson_datetime;
use crate::dispatch_job::entity::DispatchMode;

/// Subscription status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SubscriptionStatus {
    Active,
    Paused,
    Archived,
}

impl Default for SubscriptionStatus {
    fn default() -> Self {
        Self::Active
    }
}

/// Event type binding in a subscription
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventTypeBinding {
    /// Event type code (full or with wildcards)
    /// Examples:
    /// - "orders:fulfillment:shipment:shipped" (exact)
    /// - "orders:fulfillment:*:*" (wildcard)
    /// - "orders:*:*:*" (application-level)
    pub event_type_code: String,

    /// Optional filter on event data (JSONPath or similar)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
}

impl EventTypeBinding {
    pub fn new(event_type_code: impl Into<String>) -> Self {
        Self {
            event_type_code: event_type_code.into(),
            filter: None,
        }
    }

    pub fn with_filter(mut self, filter: impl Into<String>) -> Self {
        self.filter = Some(filter.into());
        self
    }

    /// Check if this binding matches an event type code
    pub fn matches(&self, event_type_code: &str) -> bool {
        let pattern_parts: Vec<&str> = self.event_type_code.split(':').collect();
        let event_parts: Vec<&str> = event_type_code.split(':').collect();

        if pattern_parts.len() != event_parts.len() {
            return false;
        }

        pattern_parts.iter().zip(event_parts.iter()).all(|(pattern, event)| {
            *pattern == "*" || pattern == event
        })
    }
}

/// Custom configuration entry
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigEntry {
    pub key: String,
    pub value: String,
}

/// Subscription entity
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subscription {
    /// TSID as Crockford Base32 string
    #[serde(rename = "_id")]
    pub id: String,

    /// Unique code (unique per client_id)
    pub code: String,

    /// Human-readable name
    pub name: String,

    /// Description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Multi-tenant: Client ID (null = anchor-level/shared)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,

    /// Event types this subscription listens to
    #[serde(default)]
    pub event_types: Vec<EventTypeBinding>,

    /// Target URL for webhook delivery
    pub target: String,

    /// Queue name for dispatch (optional - uses default if not set)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue: Option<String>,

    /// Custom configuration passed to target
    #[serde(default)]
    pub custom_config: Vec<ConfigEntry>,

    /// Dispatch pool for rate limiting
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispatch_pool_id: Option<String>,

    /// Service account for webhook authentication
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_account_id: Option<String>,

    // === Dispatch behavior ===

    /// Dispatch mode for ordering
    #[serde(default)]
    pub mode: DispatchMode,

    /// Initial delay in seconds before dispatch
    #[serde(default)]
    pub delay_seconds: u32,

    /// Sequence number for ordering (lower = higher priority)
    #[serde(default = "default_sequence")]
    pub sequence: i32,

    /// Timeout in seconds for HTTP call
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u32,

    /// Maximum retry attempts
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,

    /// If true, send raw event data only (no envelope)
    #[serde(default)]
    pub data_only: bool,

    // === Status ===

    #[serde(default)]
    pub status: SubscriptionStatus,

    // === Audit ===

    #[serde(with = "chrono_datetime_as_bson_datetime")]
    pub created_at: DateTime<Utc>,
    #[serde(with = "chrono_datetime_as_bson_datetime")]
    pub updated_at: DateTime<Utc>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
}

fn default_sequence() -> i32 {
    99
}

fn default_timeout() -> u32 {
    30
}

fn default_max_retries() -> u32 {
    3
}

impl Subscription {
    pub fn new(code: impl Into<String>, name: impl Into<String>, target: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: crate::TsidGenerator::generate(),
            code: code.into(),
            name: name.into(),
            description: None,
            client_id: None,
            event_types: vec![],
            target: target.into(),
            queue: None,
            custom_config: vec![],
            dispatch_pool_id: None,
            service_account_id: None,
            mode: DispatchMode::Immediate,
            delay_seconds: 0,
            sequence: default_sequence(),
            timeout_seconds: default_timeout(),
            max_retries: default_max_retries(),
            data_only: false,
            status: SubscriptionStatus::Active,
            created_at: now,
            updated_at: now,
            created_by: None,
        }
    }

    pub fn with_event_type(mut self, event_type_code: impl Into<String>) -> Self {
        self.event_types.push(EventTypeBinding::new(event_type_code));
        self
    }

    pub fn with_event_type_binding(mut self, binding: EventTypeBinding) -> Self {
        self.event_types.push(binding);
        self
    }

    pub fn with_client_id(mut self, client_id: impl Into<String>) -> Self {
        self.client_id = Some(client_id.into());
        self
    }

    pub fn with_dispatch_pool_id(mut self, pool_id: impl Into<String>) -> Self {
        self.dispatch_pool_id = Some(pool_id.into());
        self
    }

    pub fn with_service_account_id(mut self, account_id: impl Into<String>) -> Self {
        self.service_account_id = Some(account_id.into());
        self
    }

    pub fn with_mode(mut self, mode: DispatchMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn with_data_only(mut self, data_only: bool) -> Self {
        self.data_only = data_only;
        self
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Check if this subscription matches an event type code
    pub fn matches_event_type(&self, event_type_code: &str) -> bool {
        self.event_types.iter().any(|binding| binding.matches(event_type_code))
    }

    /// Check if this subscription matches a client
    pub fn matches_client(&self, client_id: Option<&str>) -> bool {
        match (&self.client_id, client_id) {
            // Anchor-level subscription matches all clients
            (None, _) => true,
            // Client-specific subscription matches specific client
            (Some(sub_client), Some(event_client)) => sub_client == event_client,
            // Client-specific subscription doesn't match anchor-level event
            (Some(_), None) => false,
        }
    }

    pub fn pause(&mut self) {
        self.status = SubscriptionStatus::Paused;
        self.updated_at = Utc::now();
    }

    pub fn resume(&mut self) {
        self.status = SubscriptionStatus::Active;
        self.updated_at = Utc::now();
    }

    pub fn archive(&mut self) {
        self.status = SubscriptionStatus::Archived;
        self.updated_at = Utc::now();
    }

    pub fn is_active(&self) -> bool {
        self.status == SubscriptionStatus::Active
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_type_matching() {
        let binding = EventTypeBinding::new("orders:fulfillment:shipment:shipped");
        assert!(binding.matches("orders:fulfillment:shipment:shipped"));
        assert!(!binding.matches("orders:fulfillment:shipment:created"));
    }

    #[test]
    fn test_wildcard_matching() {
        let binding = EventTypeBinding::new("orders:fulfillment:*:*");
        assert!(binding.matches("orders:fulfillment:shipment:shipped"));
        assert!(binding.matches("orders:fulfillment:order:created"));
        assert!(!binding.matches("payments:fulfillment:order:created"));
    }

    #[test]
    fn test_subscription_client_matching() {
        // Anchor-level subscription
        let anchor_sub = Subscription::new("test", "Test", "http://example.com");
        assert!(anchor_sub.matches_client(Some("client1")));
        assert!(anchor_sub.matches_client(None));

        // Client-specific subscription
        let client_sub = Subscription::new("test", "Test", "http://example.com")
            .with_client_id("client1");
        assert!(client_sub.matches_client(Some("client1")));
        assert!(!client_sub.matches_client(Some("client2")));
        assert!(!client_sub.matches_client(None));
    }
}
