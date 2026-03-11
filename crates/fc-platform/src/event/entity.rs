//! Event Entity — CloudEvents spec 1.0, matches msg_events PostgreSQL table

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

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
            id: crate::TsidGenerator::generate(crate::EntityType::Event),
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
        self.context_data.push(ContextData { key: key.into(), value: value.into() });
        self
    }

    pub fn with_context_data(mut self, data: Vec<ContextData>) -> Self {
        self.context_data = data;
        self
    }

    pub fn application(&self) -> Option<&str> { self.event_type.split(':').next() }
    pub fn subdomain(&self) -> Option<&str> { self.event_type.split(':').nth(1) }
    pub fn aggregate(&self) -> Option<&str> { self.event_type.split(':').nth(2) }
    pub fn event_name(&self) -> Option<&str> { self.event_type.split(':').nth(3) }
}

impl From<crate::entities::msg_events::Model> for Event {
    fn from(m: crate::entities::msg_events::Model) -> Self {
        let context_data: Vec<ContextData> = m.context_data
            .and_then(|v| serde_json::from_value(v.into()).ok())
            .unwrap_or_default();

        Self {
            id: m.id,
            event_type: m.event_type,
            source: m.source,
            subject: m.subject,
            time: m.time.with_timezone(&Utc),
            data: m.data.map(Into::into).unwrap_or(serde_json::Value::Null),
            spec_version: m.spec_version.unwrap_or_else(|| CLOUDEVENTS_SPEC_VERSION.to_string()),
            message_group: m.message_group,
            correlation_id: m.correlation_id,
            causation_id: m.causation_id,
            deduplication_id: m.deduplication_id,
            client_id: m.client_id,
            context_data,
            created_at: m.created_at.with_timezone(&Utc),
        }
    }
}

/// Event read projection — CQRS read model, matches msg_events_read table
#[derive(Debug, Clone, Serialize, Deserialize)]
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

impl From<crate::entities::msg_events_read::Model> for EventRead {
    fn from(m: crate::entities::msg_events_read::Model) -> Self {
        Self {
            id: m.id,
            event_type: m.event_type,
            source: m.source,
            subject: m.subject,
            time: m.time.with_timezone(&Utc),
            application: m.application,
            subdomain: m.subdomain,
            aggregate: m.aggregate,
            message_group: m.message_group,
            correlation_id: m.correlation_id,
            client_id: m.client_id,
            client_name: None,
            projected_at: m.projected_at.with_timezone(&Utc),
        }
    }
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
