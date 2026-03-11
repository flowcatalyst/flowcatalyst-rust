//! Domain Event Trait
//!
//! Base trait for all domain events. Events follow the CloudEvents specification
//! with additional fields for distributed tracing and message ordering.
//!
//! # Event Type Format
//!
//! `{app}:{domain}:{aggregate}:{action}` — e.g., `orders:fulfillment:shipment:shipped`
//!
//! # Subject Format
//!
//! `{domain}.{aggregate}.{id}` — e.g., `fulfillment.shipment.0HZXEQ5Y8JY5Z`
//!
//! # Message Group
//!
//! `{domain}:{aggregate}:{id}` — events in the same group are processed in order.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Base trait for all domain events.
///
/// Implement this trait for each domain event in your application.
/// Use the [`impl_domain_event!`](crate::impl_domain_event) macro to
/// delegate to an `EventMetadata` field.
pub trait DomainEvent: Send + Sync {
    /// Unique identifier for this event (TSID Crockford Base32 string).
    fn event_id(&self) -> &str;
    /// Event type code: `{app}:{domain}:{aggregate}:{action}`
    fn event_type(&self) -> &str;
    /// Schema version of this event type (e.g., "1.0").
    fn spec_version(&self) -> &str;
    /// Source system that generated this event.
    fn source(&self) -> &str;
    /// Qualified aggregate identifier: `{domain}.{aggregate}.{id}`
    fn subject(&self) -> &str;
    /// When the event occurred.
    fn time(&self) -> DateTime<Utc>;
    /// Execution ID for tracking a single use case execution.
    fn execution_id(&self) -> &str;
    /// Correlation ID for distributed tracing.
    fn correlation_id(&self) -> &str;
    /// ID of the event that caused this event (if any).
    fn causation_id(&self) -> Option<&str>;
    /// Principal who initiated the action that produced this event.
    fn principal_id(&self) -> &str;
    /// Message group for ordering guarantees.
    fn message_group(&self) -> &str;
    /// Serialize the event-specific data payload to JSON.
    fn to_data_json(&self) -> String;
}

/// Common metadata for domain events.
///
/// Include this as a `metadata` field in your event structs and use
/// [`impl_domain_event!`](crate::impl_domain_event) to auto-implement the trait.
///
/// # Example
///
/// ```
/// use fc_sdk::usecase::EventMetadata;
/// use serde::Serialize;
///
/// #[derive(Serialize)]
/// pub struct OrderCreated {
///     pub metadata: EventMetadata,
///     pub order_id: String,
///     pub customer_id: String,
///     pub total: f64,
/// }
///
/// fc_sdk::impl_domain_event!(OrderCreated);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventMetadata {
    pub event_id: String,
    pub event_type: String,
    pub spec_version: String,
    pub source: String,
    pub subject: String,
    pub time: DateTime<Utc>,
    pub execution_id: String,
    pub correlation_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<String>,
    pub principal_id: String,
    pub message_group: String,
}

impl EventMetadata {
    pub fn new(
        event_id: String,
        event_type: &str,
        spec_version: &str,
        source: &str,
        subject: String,
        message_group: String,
        execution_id: String,
        correlation_id: String,
        causation_id: Option<String>,
        principal_id: String,
    ) -> Self {
        Self {
            event_id,
            event_type: event_type.to_string(),
            spec_version: spec_version.to_string(),
            source: source.to_string(),
            subject,
            time: Utc::now(),
            execution_id,
            correlation_id,
            causation_id,
            principal_id,
            message_group,
        }
    }

    /// Create a builder for event metadata.
    pub fn builder() -> EventMetadataBuilder {
        EventMetadataBuilder::new()
    }
}

/// Fluent builder for [`EventMetadata`].
///
/// # Example
///
/// ```ignore
/// let metadata = EventMetadata::builder()
///     .from(&ctx)
///     .event_type("orders:fulfillment:shipment:shipped")
///     .spec_version("1.0")
///     .source("orders:fulfillment")
///     .subject(format!("fulfillment.shipment.{}", shipment_id))
///     .message_group(format!("fulfillment:shipment:{}", shipment_id))
///     .build();
/// ```
#[derive(Default)]
pub struct EventMetadataBuilder {
    event_id: Option<String>,
    event_type: Option<String>,
    spec_version: Option<String>,
    source: Option<String>,
    subject: Option<String>,
    message_group: Option<String>,
    execution_id: Option<String>,
    correlation_id: Option<String>,
    causation_id: Option<String>,
    principal_id: Option<String>,
}

impl EventMetadataBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Copy tracing metadata from an [`ExecutionContext`](super::ExecutionContext).
    ///
    /// Sets event_id (new TSID), execution_id, correlation_id, causation_id,
    /// and principal_id from the context.
    pub fn from(mut self, ctx: &super::ExecutionContext) -> Self {
        self.event_id = Some(crate::tsid::TsidGenerator::generate(crate::tsid::EntityType::Event));
        self.execution_id = Some(ctx.execution_id.clone());
        self.correlation_id = Some(ctx.correlation_id.clone());
        self.causation_id = ctx.causation_id.clone();
        self.principal_id = Some(ctx.principal_id.clone());
        self
    }

    pub fn event_id(mut self, id: impl Into<String>) -> Self {
        self.event_id = Some(id.into());
        self
    }

    pub fn event_type(mut self, event_type: impl Into<String>) -> Self {
        self.event_type = Some(event_type.into());
        self
    }

    pub fn spec_version(mut self, version: impl Into<String>) -> Self {
        self.spec_version = Some(version.into());
        self
    }

    pub fn source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    pub fn subject(mut self, subject: impl Into<String>) -> Self {
        self.subject = Some(subject.into());
        self
    }

    pub fn message_group(mut self, group: impl Into<String>) -> Self {
        self.message_group = Some(group.into());
        self
    }

    pub fn execution_id(mut self, id: impl Into<String>) -> Self {
        self.execution_id = Some(id.into());
        self
    }

    pub fn correlation_id(mut self, id: impl Into<String>) -> Self {
        self.correlation_id = Some(id.into());
        self
    }

    pub fn causation_id(mut self, id: impl Into<String>) -> Self {
        self.causation_id = Some(id.into());
        self
    }

    pub fn principal_id(mut self, id: impl Into<String>) -> Self {
        self.principal_id = Some(id.into());
        self
    }

    /// Build the EventMetadata.
    ///
    /// # Panics
    ///
    /// Panics if required fields are missing: event_type, spec_version,
    /// source, subject, message_group, execution_id, correlation_id, principal_id.
    pub fn build(self) -> EventMetadata {
        EventMetadata {
            event_id: self.event_id.unwrap_or_else(|| {
                crate::tsid::TsidGenerator::generate(crate::tsid::EntityType::Event)
            }),
            event_type: self.event_type.expect("event_type is required"),
            spec_version: self.spec_version.expect("spec_version is required"),
            source: self.source.expect("source is required"),
            subject: self.subject.expect("subject is required"),
            time: Utc::now(),
            execution_id: self
                .execution_id
                .expect("execution_id is required (use .from(ctx))"),
            correlation_id: self
                .correlation_id
                .expect("correlation_id is required (use .from(ctx))"),
            causation_id: self.causation_id,
            principal_id: self
                .principal_id
                .expect("principal_id is required (use .from(ctx))"),
            message_group: self.message_group.expect("message_group is required"),
        }
    }

    /// Try to build the EventMetadata, returning an error if fields are missing.
    pub fn try_build(self) -> Result<EventMetadata, &'static str> {
        Ok(EventMetadata {
            event_id: self.event_id.unwrap_or_else(|| {
                crate::tsid::TsidGenerator::generate(crate::tsid::EntityType::Event)
            }),
            event_type: self.event_type.ok_or("event_type is required")?,
            spec_version: self.spec_version.ok_or("spec_version is required")?,
            source: self.source.ok_or("source is required")?,
            subject: self.subject.ok_or("subject is required")?,
            time: Utc::now(),
            execution_id: self.execution_id.ok_or("execution_id is required")?,
            correlation_id: self.correlation_id.ok_or("correlation_id is required")?,
            causation_id: self.causation_id,
            principal_id: self.principal_id.ok_or("principal_id is required")?,
            message_group: self.message_group.ok_or("message_group is required")?,
        })
    }
}

/// Macro for implementing the [`DomainEvent`] trait.
///
/// Delegates all trait methods to a field named `metadata` of type [`EventMetadata`].
///
/// # Example
///
/// ```
/// use fc_sdk::usecase::{DomainEvent, EventMetadata};
/// use serde::Serialize;
///
/// #[derive(Debug, Clone, Serialize)]
/// pub struct OrderShipped {
///     pub metadata: EventMetadata,
///     pub order_id: String,
///     pub tracking_number: String,
/// }
///
/// fc_sdk::impl_domain_event!(OrderShipped);
/// ```
#[macro_export]
macro_rules! impl_domain_event {
    ($event_type:ty) => {
        impl $crate::usecase::DomainEvent for $event_type {
            fn event_id(&self) -> &str {
                &self.metadata.event_id
            }

            fn event_type(&self) -> &str {
                &self.metadata.event_type
            }

            fn spec_version(&self) -> &str {
                &self.metadata.spec_version
            }

            fn source(&self) -> &str {
                &self.metadata.source
            }

            fn subject(&self) -> &str {
                &self.metadata.subject
            }

            fn time(&self) -> chrono::DateTime<chrono::Utc> {
                self.metadata.time
            }

            fn execution_id(&self) -> &str {
                &self.metadata.execution_id
            }

            fn correlation_id(&self) -> &str {
                &self.metadata.correlation_id
            }

            fn causation_id(&self) -> Option<&str> {
                self.metadata.causation_id.as_deref()
            }

            fn principal_id(&self) -> &str {
                &self.metadata.principal_id
            }

            fn message_group(&self) -> &str {
                &self.metadata.message_group
            }

            fn to_data_json(&self) -> String {
                serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
            }
        }
    };
}
