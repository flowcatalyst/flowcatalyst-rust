//! Event Entity — CloudEvents spec 1.0, matches msg_events PostgreSQL table

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// CloudEvents spec version
pub const CLOUDEVENTS_SPEC_VERSION: &str = "1.0";

/// Context data for event filtering/searching
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextData {
    pub key: String,
    pub value: String,
}

/// Event entity — write model, immutable once created
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    /// TSID as Crockford Base32 string (VARCHAR(13))
    pub id: String,

    /// CloudEvents: Event type e.g. "orders:fulfillment:shipment:shipped"
    #[serde(rename = "type")]
    pub event_type: String,

    /// CloudEvents: Event source URI
    pub source: String,

    /// CloudEvents: Event subject (optional context)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,

    /// CloudEvents: Timestamp of event occurrence
    pub time: DateTime<Utc>,

    /// CloudEvents: Event payload data
    pub data: serde_json::Value,

    /// CloudEvents spec version
    #[serde(default = "default_spec_version")]
    pub spec_version: String,

    /// Message group for FIFO ordering
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_group: Option<String>,

    /// Correlation ID for request tracing
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,

    /// Causation ID — the event that caused this event
    #[serde(skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<String>,

    /// Deduplication ID for exactly-once delivery
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deduplication_id: Option<String>,

    /// Multi-tenant: Client/organization ID (null = anchor-level)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,

    /// Context data for filtering/searching (stored as JSONB)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context_data: Vec<ContextData>,

    /// When the event was stored
    pub created_at: DateTime<Utc>,
}

fn default_spec_version() -> String {
    CLOUDEVENTS_SPEC_VERSION.to_string()
}

impl Event {
    pub fn new(
        event_type: impl Into<String>,
        source: impl Into<String>,
        data: serde_json::Value,
    ) -> Self {
        Self {
            id: crate::shared::tsid::generate_untyped(),
            event_type: event_type.into(),
            source: source.into(),
            subject: None,
            time: Utc::now(),
            data,
            spec_version: default_spec_version(),
            message_group: None,
            correlation_id: None,
            causation_id: None,
            deduplication_id: None,
            client_id: None,
            context_data: vec![],
            created_at: Utc::now(),
        }
    }

    pub fn with_subject(mut self, subject: impl Into<String>) -> Self {
        self.subject = Some(subject.into());
        self
    }

    pub fn with_message_group(mut self, group: impl Into<String>) -> Self {
        self.message_group = Some(group.into());
        self
    }

    pub fn with_correlation_id(mut self, id: impl Into<String>) -> Self {
        self.correlation_id = Some(id.into());
        self
    }

    pub fn with_causation_id(mut self, id: impl Into<String>) -> Self {
        self.causation_id = Some(id.into());
        self
    }

    pub fn with_client_id(mut self, id: impl Into<String>) -> Self {
        self.client_id = Some(id.into());
        self
    }

    pub fn with_deduplication_id(mut self, id: impl Into<String>) -> Self {
        self.deduplication_id = Some(id.into());
        self
    }

    pub fn with_context(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.context_data.push(ContextData {
            key: key.into(),
            value: value.into(),
        });
        self
    }

    pub fn with_context_data(mut self, data: Vec<ContextData>) -> Self {
        self.context_data = data;
        self
    }

    pub fn application(&self) -> Option<&str> {
        self.event_type.split(':').next()
    }
    pub fn subdomain(&self) -> Option<&str> {
        self.event_type.split(':').nth(1)
    }
    pub fn aggregate(&self) -> Option<&str> {
        self.event_type.split(':').nth(2)
    }
    pub fn event_name(&self) -> Option<&str> {
        self.event_type.split(':').nth(3)
    }
}

/// Event read projection — CQRS read model, matches msg_events_read table
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EventRead {
    pub id: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub source: String,
    pub subject: Option<String>,
    pub time: DateTime<Utc>,
    pub application: Option<String>,
    pub subdomain: Option<String>,
    pub aggregate: Option<String>,
    pub message_group: Option<String>,
    pub correlation_id: Option<String>,
    pub client_id: Option<String>,
    /// Denormalized client name for display
    pub client_name: Option<String>,
    pub projected_at: DateTime<Utc>,
}

/// Filter options for the events read model (cascading filters).
/// Clients are served by the canonical `/bff/filter-options/clients` endpoint,
/// so they aren't duplicated here.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EventFilterOptions {
    pub applications: Vec<String>,
    pub subdomains: Vec<String>,
    pub aggregates: Vec<String>,
    pub types: Vec<String>,
}

impl From<&Event> for EventRead {
    fn from(event: &Event) -> Self {
        Self {
            id: event.id.clone(),
            event_type: event.event_type.clone(),
            source: event.source.clone(),
            subject: event.subject.clone(),
            time: event.time,
            application: event.application().map(String::from),
            subdomain: event.subdomain().map(String::from),
            aggregate: event.aggregate().map(String::from),
            message_group: event.message_group.clone(),
            correlation_id: event.correlation_id.clone(),
            client_id: event.client_id.clone(),
            client_name: None,
            projected_at: event.created_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_event() {
        let data = serde_json::json!({"orderId": "123"});
        let event = Event::new(
            "orders:fulfillment:shipment:shipped",
            "my-app",
            data.clone(),
        );

        assert!(!event.id.is_empty());
        assert_eq!(
            event.id.len(),
            13,
            "Untyped ID should be 13 chars, got: {}",
            event.id.len()
        );
        assert!(
            !event.id.contains('_'),
            "Untyped ID should not have prefix underscore"
        );
        assert_eq!(event.event_type, "orders:fulfillment:shipment:shipped");
        assert_eq!(event.source, "my-app");
        assert!(event.subject.is_none());
        assert_eq!(event.data, data);
        assert_eq!(event.spec_version, "1.0");
        assert!(event.message_group.is_none());
        assert!(event.correlation_id.is_none());
        assert!(event.causation_id.is_none());
        assert!(event.deduplication_id.is_none());
        assert!(event.client_id.is_none());
        assert!(event.context_data.is_empty());
    }

    #[test]
    fn test_event_unique_ids() {
        let e1 = Event::new("t1", "s", serde_json::json!({}));
        let e2 = Event::new("t2", "s", serde_json::json!({}));
        assert_ne!(e1.id, e2.id);
    }

    #[test]
    fn test_event_builder_methods() {
        let event = Event::new("t", "s", serde_json::json!({}))
            .with_subject("order-123")
            .with_message_group("group-1")
            .with_correlation_id("corr-1")
            .with_causation_id("cause-1")
            .with_client_id("client-1")
            .with_deduplication_id("dedup-1")
            .with_context("key1", "value1");

        assert_eq!(event.subject, Some("order-123".to_string()));
        assert_eq!(event.message_group, Some("group-1".to_string()));
        assert_eq!(event.correlation_id, Some("corr-1".to_string()));
        assert_eq!(event.causation_id, Some("cause-1".to_string()));
        assert_eq!(event.client_id, Some("client-1".to_string()));
        assert_eq!(event.deduplication_id, Some("dedup-1".to_string()));
        assert_eq!(event.context_data.len(), 1);
        assert_eq!(event.context_data[0].key, "key1");
        assert_eq!(event.context_data[0].value, "value1");
    }

    #[test]
    fn test_event_with_context_data() {
        let ctx = vec![
            ContextData {
                key: "a".to_string(),
                value: "1".to_string(),
            },
            ContextData {
                key: "b".to_string(),
                value: "2".to_string(),
            },
        ];
        let event = Event::new("t", "s", serde_json::json!({})).with_context_data(ctx);

        assert_eq!(event.context_data.len(), 2);
    }

    #[test]
    fn test_event_code_parsing() {
        let event = Event::new(
            "orders:fulfillment:shipment:shipped",
            "app",
            serde_json::json!({}),
        );
        assert_eq!(event.application(), Some("orders"));
        assert_eq!(event.subdomain(), Some("fulfillment"));
        assert_eq!(event.aggregate(), Some("shipment"));
        assert_eq!(event.event_name(), Some("shipped"));
    }

    #[test]
    fn test_event_code_parsing_simple() {
        let event = Event::new("simple-event", "app", serde_json::json!({}));
        assert_eq!(event.application(), Some("simple-event"));
        assert!(event.subdomain().is_none());
        assert!(event.aggregate().is_none());
        assert!(event.event_name().is_none());
    }

    #[test]
    fn test_event_spec_version_constant() {
        assert_eq!(CLOUDEVENTS_SPEC_VERSION, "1.0");
    }

    // --- EventRead projection ---

    #[test]
    fn test_event_read_from_event() {
        let event = Event::new(
            "orders:billing:invoice:created",
            "my-app",
            serde_json::json!({}),
        )
        .with_client_id("c1")
        .with_message_group("g1")
        .with_correlation_id("corr1");

        let read = EventRead::from(&event);
        assert_eq!(read.id, event.id);
        assert_eq!(read.event_type, event.event_type);
        assert_eq!(read.source, event.source);
        assert_eq!(read.application, Some("orders".to_string()));
        assert_eq!(read.subdomain, Some("billing".to_string()));
        assert_eq!(read.aggregate, Some("invoice".to_string()));
        assert_eq!(read.client_id, Some("c1".to_string()));
        assert_eq!(read.message_group, Some("g1".to_string()));
        assert_eq!(read.correlation_id, Some("corr1".to_string()));
        assert!(
            read.client_name.is_none(),
            "client_name is not populated from Event"
        );
    }
}
