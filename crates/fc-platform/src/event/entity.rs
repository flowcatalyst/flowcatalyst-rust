//! Event Entity
//!
//! CloudEvents spec 1.0 compliant event storage.
//! Immutable once created.

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use bson::serde_helpers::chrono_datetime_as_bson_datetime;

/// CloudEvents spec version
pub const CLOUDEVENTS_SPEC_VERSION: &str = "1.0";

/// Event entity - immutable event storage
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    /// TSID as Crockford Base32 string
    #[serde(rename = "_id")]
    pub id: String,

    /// CloudEvents: Event type (e.g., "com.example.order.created")
    /// Format: {application}:{subdomain}:{aggregate}:{event}
    #[serde(rename = "type")]
    pub event_type: String,

    /// CloudEvents: Event source URI
    pub source: String,

    /// CloudEvents: Event subject (optional context)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,

    /// CloudEvents: Timestamp of event occurrence
    #[serde(with = "chrono_datetime_as_bson_datetime")]
    pub time: DateTime<Utc>,

    /// CloudEvents: Event payload data
    pub data: serde_json::Value,

    /// CloudEvents: Content type of data
    #[serde(default = "default_content_type")]
    pub data_content_type: String,

    /// CloudEvents spec version
    #[serde(default = "default_spec_version")]
    pub spec_version: String,

    /// Message group for FIFO ordering
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_group: Option<String>,

    /// Correlation ID for request tracing
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,

    /// Causation ID - the event that caused this event
    #[serde(skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<String>,

    /// Deduplication ID for exactly-once delivery
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deduplication_id: Option<String>,

    /// Multi-tenant: Client/organization ID (null = anchor-level)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,

    /// Context data for filtering/searching
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context_data: Vec<ContextData>,

    /// When the event was stored
    #[serde(with = "chrono_datetime_as_bson_datetime")]
    pub created_at: DateTime<Utc>,
}

fn default_content_type() -> String {
    "application/json".to_string()
}

fn default_spec_version() -> String {
    CLOUDEVENTS_SPEC_VERSION.to_string()
}

/// Context data for event filtering
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextData {
    pub key: String,
    pub value: String,
}

impl Event {
    /// Create a new event with generated ID
    pub fn new(
        event_type: impl Into<String>,
        source: impl Into<String>,
        data: serde_json::Value,
    ) -> Self {
        Self {
            id: crate::TsidGenerator::generate(),
            event_type: event_type.into(),
            source: source.into(),
            subject: None,
            time: Utc::now(),
            data,
            data_content_type: default_content_type(),
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

    /// Builder pattern methods
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

    /// Extract application code from event type
    /// Event type format: {application}:{subdomain}:{aggregate}:{event}
    pub fn application(&self) -> Option<&str> {
        self.event_type.split(':').next()
    }

    /// Extract subdomain from event type
    pub fn subdomain(&self) -> Option<&str> {
        self.event_type.split(':').nth(1)
    }

    /// Extract aggregate from event type
    pub fn aggregate(&self) -> Option<&str> {
        self.event_type.split(':').nth(2)
    }

    /// Extract event name from event type
    pub fn event_name(&self) -> Option<&str> {
        self.event_type.split(':').nth(3)
    }
}

/// Event read projection - optimized for queries
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventRead {
    #[serde(rename = "_id")]
    pub id: String,

    #[serde(rename = "type")]
    pub event_type: String,
    pub source: String,
    pub subject: Option<String>,
    #[serde(with = "chrono_datetime_as_bson_datetime")]
    pub time: DateTime<Utc>,

    /// Parsed from event_type for filtering
    pub application: Option<String>,
    pub subdomain: Option<String>,
    pub aggregate: Option<String>,
    pub event_name: Option<String>,

    pub message_group: Option<String>,
    pub correlation_id: Option<String>,
    pub client_id: Option<String>,

    /// Denormalized client name for display
    pub client_name: Option<String>,

    #[serde(with = "chrono_datetime_as_bson_datetime")]
    pub created_at: DateTime<Utc>,
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
            event_name: event.event_name().map(String::from),
            message_group: event.message_group.clone(),
            correlation_id: event.correlation_id.clone(),
            client_id: event.client_id.clone(),
            client_name: None, // Populated by projection
            created_at: event.created_at,
        }
    }
}
