//! Subscription Entity — matches TypeScript Subscription domain

use chrono::{DateTime, Utc};
pub use fc_common::DispatchMode;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[derive(Default)]
pub enum SubscriptionStatus {
    #[default]
    Active,
    Paused,
}

impl SubscriptionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::Paused => "PAUSED",
        }
    }
    pub fn from_str(s: &str) -> Self {
        match s {
            "PAUSED" => Self::Paused,
            _ => Self::Active,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[derive(Default)]
pub enum SubscriptionSource {
    Code,
    Api,
    #[default]
    Ui,
}

impl SubscriptionSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Code => "CODE",
            Self::Api => "API",
            Self::Ui => "UI",
        }
    }
    pub fn from_str(s: &str) -> Self {
        match s {
            "CODE" => Self::Code,
            "API" => Self::Api,
            _ => Self::Ui,
        }
    }
}

/// Event type binding stored in msg_subscription_event_types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventTypeBinding {
    pub event_type_id: Option<String>,
    pub event_type_code: String,
    pub spec_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
}

impl EventTypeBinding {
    pub fn new(event_type_code: impl Into<String>) -> Self {
        Self {
            event_type_id: None,
            event_type_code: event_type_code.into(),
            spec_version: None,
            filter: None,
        }
    }

    pub fn with_filter(mut self, filter: impl Into<String>) -> Self {
        self.filter = Some(filter.into());
        self
    }

    pub fn matches(&self, event_type_code: &str) -> bool {
        let pattern_parts: Vec<&str> = self.event_type_code.split(':').collect();
        let event_parts: Vec<&str> = event_type_code.split(':').collect();
        if pattern_parts.len() != event_parts.len() {
            return false;
        }
        pattern_parts
            .iter()
            .zip(event_parts.iter())
            .all(|(p, e)| *p == "*" || p == e)
    }
}

/// Custom configuration entry stored in msg_subscription_custom_configs
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigEntry {
    pub key: String,
    pub value: String,
}

/// Subscription domain entity — matches TypeScript Subscription interface
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subscription {
    pub id: String,
    pub code: String,
    pub application_code: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub client_id: Option<String>,
    pub client_identifier: Option<String>,
    pub client_scoped: bool,
    pub event_types: Vec<EventTypeBinding>,
    /// Connection ID — references msg_connections (optional, SDK subscriptions may not have one)
    pub connection_id: Option<String>,
    /// Webhook endpoint URL — the target URL for this subscription
    pub endpoint: String,
    pub queue: Option<String>,
    pub custom_config: Vec<ConfigEntry>,
    pub source: SubscriptionSource,
    pub status: SubscriptionStatus,
    pub max_age_seconds: i32,
    pub dispatch_pool_id: Option<String>,
    pub dispatch_pool_code: Option<String>,
    pub delay_seconds: i32,
    pub sequence: i32,
    pub mode: DispatchMode,
    pub timeout_seconds: i32,
    pub max_retries: i32,
    pub service_account_id: Option<String>,
    pub data_only: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Subscription {
    pub fn new(
        code: impl Into<String>,
        name: impl Into<String>,
        endpoint: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: crate::shared::tsid::generate(crate::EntityType::Subscription),
            code: code.into(),
            application_code: None,
            name: name.into(),
            description: None,
            client_id: None,
            client_identifier: None,
            client_scoped: false,
            event_types: vec![],
            connection_id: None,
            endpoint: endpoint.into(),
            queue: None,
            custom_config: vec![],
            source: SubscriptionSource::Ui,
            status: SubscriptionStatus::Active,
            max_age_seconds: 86400,
            dispatch_pool_id: None,
            dispatch_pool_code: None,
            delay_seconds: 0,
            sequence: 99,
            mode: DispatchMode::Immediate,
            timeout_seconds: 30,
            max_retries: 3,
            service_account_id: None,
            data_only: true,
            created_by: None,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn with_endpoint(mut self, ep: impl Into<String>) -> Self {
        self.endpoint = ep.into();
        self
    }
    pub fn with_connection_id(mut self, id: impl Into<String>) -> Self {
        self.connection_id = Some(id.into());
        self
    }
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }
    pub fn with_client_id(mut self, id: impl Into<String>) -> Self {
        self.client_id = Some(id.into());
        self
    }
    pub fn with_dispatch_pool_id(mut self, id: impl Into<String>) -> Self {
        self.dispatch_pool_id = Some(id.into());
        self
    }
    pub fn with_service_account_id(mut self, id: impl Into<String>) -> Self {
        self.service_account_id = Some(id.into());
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
    pub fn with_event_type_binding(mut self, binding: EventTypeBinding) -> Self {
        self.event_types.push(binding);
        self
    }

    pub fn matches_event_type(&self, event_type_code: &str) -> bool {
        self.event_types.iter().any(|b| b.matches(event_type_code))
    }

    pub fn matches_client(&self, client_id: Option<&str>) -> bool {
        match (&self.client_id, client_id) {
            (None, _) => true,
            (Some(sub_client), Some(event_client)) => sub_client == event_client,
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
    pub fn is_active(&self) -> bool {
        self.status == SubscriptionStatus::Active
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_subscription() {
        let sub = Subscription::new(
            "order-events",
            "Order Events",
            "https://example.com/webhook",
        );

        assert!(!sub.id.is_empty());
        assert!(
            sub.id.starts_with("sub_"),
            "ID should have sub_ prefix, got: {}",
            sub.id
        );
        assert_eq!(sub.code, "order-events");
        assert_eq!(sub.name, "Order Events");
        assert_eq!(sub.endpoint, "https://example.com/webhook");
        assert!(sub.application_code.is_none());
        assert!(sub.description.is_none());
        assert!(sub.client_id.is_none());
        assert!(sub.client_identifier.is_none());
        assert!(!sub.client_scoped);
        assert!(sub.event_types.is_empty());
        assert!(sub.connection_id.is_none());
        assert!(sub.queue.is_none());
        assert!(sub.custom_config.is_empty());
        assert_eq!(sub.source, SubscriptionSource::Ui);
        assert_eq!(sub.status, SubscriptionStatus::Active);
        assert_eq!(sub.max_age_seconds, 86400);
        assert!(sub.dispatch_pool_id.is_none());
        assert!(sub.dispatch_pool_code.is_none());
        assert_eq!(sub.delay_seconds, 0);
        assert_eq!(sub.sequence, 99);
        assert_eq!(sub.mode, DispatchMode::Immediate);
        assert_eq!(sub.timeout_seconds, 30);
        assert_eq!(sub.max_retries, 3);
        assert!(sub.service_account_id.is_none());
        assert!(sub.data_only);
        assert!(sub.created_by.is_none());
        assert_eq!(sub.created_at, sub.updated_at);
    }

    #[test]
    fn test_subscription_builder_methods() {
        let sub = Subscription::new("s1", "Sub 1", "https://a.com")
            .with_endpoint("https://b.com")
            .with_connection_id("conn-1")
            .with_description("Test subscription")
            .with_client_id("client-1")
            .with_dispatch_pool_id("pool-1")
            .with_service_account_id("sa-1")
            .with_mode(DispatchMode::BlockOnError)
            .with_data_only(false)
            .with_event_type_binding(EventTypeBinding::new("orders:*:*:*"));

        assert_eq!(sub.endpoint, "https://b.com");
        assert_eq!(sub.connection_id, Some("conn-1".to_string()));
        assert_eq!(sub.description, Some("Test subscription".to_string()));
        assert_eq!(sub.client_id, Some("client-1".to_string()));
        assert_eq!(sub.dispatch_pool_id, Some("pool-1".to_string()));
        assert_eq!(sub.service_account_id, Some("sa-1".to_string()));
        assert_eq!(sub.mode, DispatchMode::BlockOnError);
        assert!(!sub.data_only);
        assert_eq!(sub.event_types.len(), 1);
    }

    #[test]
    fn test_subscription_pause_and_resume() {
        let mut sub = Subscription::new("s1", "Sub 1", "https://a.com");
        assert!(sub.is_active());
        assert_eq!(sub.status, SubscriptionStatus::Active);

        sub.pause();
        assert!(!sub.is_active());
        assert_eq!(sub.status, SubscriptionStatus::Paused);

        sub.resume();
        assert!(sub.is_active());
        assert_eq!(sub.status, SubscriptionStatus::Active);
    }

    #[test]
    fn test_subscription_matches_event_type() {
        let sub = Subscription::new("s1", "Sub", "https://a.com")
            .with_event_type_binding(EventTypeBinding::new("orders:fulfillment:shipment:shipped"))
            .with_event_type_binding(EventTypeBinding::new("orders:*:*:*"));

        // Exact match
        assert!(sub.matches_event_type("orders:fulfillment:shipment:shipped"));
        // Wildcard match
        assert!(sub.matches_event_type("orders:billing:invoice:created"));
        // Non-match
        assert!(!sub.matches_event_type("payments:billing:invoice:created"));
    }

    #[test]
    fn test_subscription_matches_client() {
        let sub_no_client = Subscription::new("s1", "Sub", "https://a.com");
        // No client_id on subscription means it matches any client
        assert!(sub_no_client.matches_client(Some("client-1")));
        assert!(sub_no_client.matches_client(None));

        let sub_with_client =
            Subscription::new("s2", "Sub", "https://a.com").with_client_id("client-1");
        assert!(sub_with_client.matches_client(Some("client-1")));
        assert!(!sub_with_client.matches_client(Some("client-2")));
        assert!(!sub_with_client.matches_client(None));
    }

    #[test]
    fn test_event_type_binding_matches() {
        let exact = EventTypeBinding::new("orders:fulfillment:shipment:shipped");
        assert!(exact.matches("orders:fulfillment:shipment:shipped"));
        assert!(!exact.matches("orders:fulfillment:shipment:created"));

        let wildcard = EventTypeBinding::new("orders:*:*:*");
        assert!(wildcard.matches("orders:fulfillment:shipment:shipped"));
        assert!(wildcard.matches("orders:billing:invoice:created"));
        assert!(!wildcard.matches("payments:billing:invoice:created"));

        let partial_wildcard = EventTypeBinding::new("orders:fulfillment:*:*");
        assert!(partial_wildcard.matches("orders:fulfillment:shipment:shipped"));
        assert!(!partial_wildcard.matches("orders:billing:invoice:created"));

        // Wrong part count
        assert!(!exact.matches("orders:fulfillment:shipment"));
        assert!(!exact.matches("orders:fulfillment:shipment:shipped:extra"));
    }

    #[test]
    fn test_event_type_binding_with_filter() {
        let binding =
            EventTypeBinding::new("orders:*:*:*").with_filter("$.data.status == 'shipped'");

        assert_eq!(
            binding.filter,
            Some("$.data.status == 'shipped'".to_string())
        );
    }

    #[test]
    fn test_subscription_status_as_str() {
        assert_eq!(SubscriptionStatus::Active.as_str(), "ACTIVE");
        assert_eq!(SubscriptionStatus::Paused.as_str(), "PAUSED");
    }

    #[test]
    fn test_subscription_status_from_str() {
        assert_eq!(
            SubscriptionStatus::from_str("ACTIVE"),
            SubscriptionStatus::Active
        );
        assert_eq!(
            SubscriptionStatus::from_str("PAUSED"),
            SubscriptionStatus::Paused
        );
        assert_eq!(
            SubscriptionStatus::from_str("unknown"),
            SubscriptionStatus::Active
        );
    }

    #[test]
    fn test_subscription_status_default() {
        assert_eq!(SubscriptionStatus::default(), SubscriptionStatus::Active);
    }

    #[test]
    fn test_subscription_source_as_str() {
        assert_eq!(SubscriptionSource::Code.as_str(), "CODE");
        assert_eq!(SubscriptionSource::Api.as_str(), "API");
        assert_eq!(SubscriptionSource::Ui.as_str(), "UI");
    }

    #[test]
    fn test_subscription_source_from_str() {
        assert_eq!(
            SubscriptionSource::from_str("CODE"),
            SubscriptionSource::Code
        );
        assert_eq!(SubscriptionSource::from_str("API"), SubscriptionSource::Api);
        assert_eq!(SubscriptionSource::from_str("UI"), SubscriptionSource::Ui);
        assert_eq!(
            SubscriptionSource::from_str("unknown"),
            SubscriptionSource::Ui
        );
    }

    #[test]
    fn test_subscription_source_default() {
        assert_eq!(SubscriptionSource::default(), SubscriptionSource::Ui);
    }

    #[test]
    fn test_subscription_unique_ids() {
        let s1 = Subscription::new("a", "A", "https://a.com");
        let s2 = Subscription::new("b", "B", "https://b.com");
        assert_ne!(s1.id, s2.id);
    }

    // ── SubscriptionStatus transitions ────────────────────────────────────

    #[test]
    fn is_active_reflects_current_status() {
        let mut s = Subscription::new("a", "A", "https://a.com");
        assert!(s.is_active(), "new subscription is active by default");
        s.pause();
        assert!(!s.is_active());
        s.resume();
        assert!(s.is_active());
    }

    #[test]
    fn pause_bumps_updated_at() {
        let mut s = Subscription::new("a", "A", "https://a.com");
        let before = s.updated_at;
        std::thread::sleep(std::time::Duration::from_millis(2));
        s.pause();
        assert!(s.updated_at > before);
    }

    // ── EventTypeBinding::matches — wildcard edge cases ───────────────────

    #[test]
    fn all_wildcards_match_any_four_segment_code() {
        let b = EventTypeBinding::new("*:*:*:*");
        assert!(b.matches("orders:fulfillment:shipment:shipped"));
        assert!(b.matches("payments:billing:invoice:created"));
        assert!(b.matches("a:b:c:d"));
    }

    #[test]
    fn wildcard_does_not_match_different_segment_count() {
        // Matching requires same segment count; wildcards don't span segments.
        let three_seg = EventTypeBinding::new("*:*:*");
        assert!(!three_seg.matches("orders:fulfillment:shipment:shipped"));

        let five_seg = EventTypeBinding::new("*:*:*:*:*");
        assert!(!five_seg.matches("orders:fulfillment:shipment:shipped"));
    }

    #[test]
    fn mixed_literal_and_wildcard_segments() {
        let b = EventTypeBinding::new("orders:*:shipment:*");
        assert!(b.matches("orders:fulfillment:shipment:shipped"));
        assert!(b.matches("orders:billing:shipment:created"));
        assert!(
            !b.matches("orders:fulfillment:invoice:created"),
            "segment 3 must equal 'shipment'"
        );
        assert!(
            !b.matches("payments:fulfillment:shipment:shipped"),
            "segment 1 must equal 'orders'"
        );
    }

    #[test]
    fn exact_code_without_wildcards_is_strict_match() {
        let b = EventTypeBinding::new("orders:fulfillment:shipment:shipped");
        assert!(b.matches("orders:fulfillment:shipment:shipped"));
        assert!(!b.matches("orders:fulfillment:shipment:cancelled"));
        assert!(!b.matches("orders:fulfillment:shipment:shippe")); // off-by-one
    }

    // ── Subscription::matches_event_type with multiple bindings ───────────

    #[test]
    fn matches_event_type_succeeds_if_any_binding_matches() {
        let s = Subscription::new("multi", "Multi", "https://x.com")
            .with_event_type_binding(EventTypeBinding::new("orders:*:*:*"))
            .with_event_type_binding(EventTypeBinding::new("payments:billing:invoice:created"));

        assert!(s.matches_event_type("orders:fulfillment:shipment:shipped"));
        assert!(s.matches_event_type("payments:billing:invoice:created"));
        assert!(!s.matches_event_type("inventory:stock:item:reserved"));
    }

    #[test]
    fn matches_event_type_false_when_no_bindings() {
        let s = Subscription::new("empty", "Empty", "https://x.com");
        assert!(!s.matches_event_type("orders:fulfillment:shipment:shipped"));
    }

    // ── Subscription::matches_client — client scoping rules ───────────────

    #[test]
    fn global_subscription_matches_any_event_client() {
        // No client_id set on the subscription → matches all event clients.
        let s = Subscription::new("global", "Global", "https://x.com");
        assert!(s.matches_client(Some("clt_a")));
        assert!(s.matches_client(Some("clt_b")));
        assert!(s.matches_client(None));
    }

    #[test]
    fn client_scoped_subscription_matches_only_its_client() {
        let s = Subscription::new("scoped", "Scoped", "https://x.com").with_client_id("clt_a");
        assert!(s.matches_client(Some("clt_a")));
        assert!(!s.matches_client(Some("clt_b")));
        // Event without a client cannot match a client-scoped subscription.
        assert!(!s.matches_client(None));
    }

    #[test]
    fn subscription_status_from_str_falls_back_to_active() {
        assert_eq!(
            SubscriptionStatus::from_str("UNKNOWN"),
            SubscriptionStatus::Active
        );
        assert_eq!(SubscriptionStatus::from_str(""), SubscriptionStatus::Active);
    }
}
