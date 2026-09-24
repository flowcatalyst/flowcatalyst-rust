//! Domain Event Trait
//!
//! Base trait for all domain events. Events follow the CloudEvents specification
//! structure with additional fields for tracing and ordering.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::ExecutionContext;

/// Base trait for all domain events.
///
/// Domain events represent facts about what happened in the domain (past tense).
/// Each event has its own schema and is stored in the event store.
///
/// # Event Type Convention
///
/// Events are named in past tense describing what happened:
/// - `UserCreated` (not CreateUser)
/// - `SchemaFinalised` (not FinaliseSchema)
/// - `ApplicationActivated` (not ActivateApplication)
///
/// # Event Type Format
///
/// The event type follows the format: `{app}:{domain}:{aggregate}:{action}`
/// Example: `platform:iam:user:created`
///
/// # Subject Format
///
/// The subject is a qualified aggregate identifier: `{domain}.{aggregate}.{id}`
/// Example: `platform.user.0HZXEQ5Y8JY5Z`
///
/// # Message Group
///
/// Events in the same message group are processed in order.
/// Format: `{domain}:{aggregate}:{id}`
/// Example: `platform:user:0HZXEQ5Y8JY5Z`
pub trait DomainEvent: Send + Sync {
    /// Unique identifier for this event (TSID Crockford Base32 string).
    fn event_id(&self) -> &str;

    /// Event type code following the format: `{app}:{domain}:{aggregate}:{action}`
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
/// This struct holds the common CloudEvents fields and tracing context.
/// Event implementations include it as a `metadata` field and implement
/// [`DomainEvent`] with [`impl_domain_event!`](crate::impl_domain_event).
/// Build it with [`EventMetadata::from_ctx`].
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
    /// Metadata for a new event raised inside `ctx`.
    ///
    /// Generates a fresh event id, stamps `time` with now, and copies the
    /// execution, correlation, causation and principal ids from the context.
    pub fn from_ctx(
        ctx: &ExecutionContext,
        event_type: &str,
        spec_version: &str,
        source: &str,
        subject: impl Into<String>,
        message_group: impl Into<String>,
    ) -> Self {
        Self {
            event_id: crate::shared::tsid::TsidGenerator::generate_untyped(),
            event_type: event_type.to_string(),
            spec_version: spec_version.to_string(),
            source: source.to_string(),
            subject: subject.into(),
            time: Utc::now(),
            execution_id: ctx.execution_id.clone(),
            correlation_id: ctx.correlation_id.clone(),
            causation_id: ctx.causation_id.clone(),
            principal_id: ctx.principal_id.clone(),
            message_group: message_group.into(),
        }
    }
}

/// Helper macro for implementing the DomainEvent trait.
///
/// This macro generates the trait implementation by delegating to an
/// `EventMetadata` field named `metadata`.
///
/// # Example
///
/// ```ignore
/// use fc_platform::usecase::{DomainEvent, EventMetadata};
/// use fc_platform::impl_domain_event;
///
/// pub struct UserCreated {
///     metadata: EventMetadata,
///     pub user_id: String,
///     pub email: String,
/// }
///
/// impl_domain_event!(UserCreated);
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    #[derive(Debug, Clone, Serialize)]
    struct TestEvent {
        metadata: EventMetadata,
        pub test_field: String,
    }

    impl_domain_event!(TestEvent);

    fn sample_metadata() -> EventMetadata {
        EventMetadata {
            event_id: "evt-123".to_string(),
            event_type: "test:domain:entity:created".to_string(),
            spec_version: "1.0".to_string(),
            source: "test:domain".to_string(),
            subject: "domain.entity.123".to_string(),
            time: Utc::now(),
            execution_id: "exec-456".to_string(),
            correlation_id: "corr-789".to_string(),
            causation_id: None,
            principal_id: "principal-001".to_string(),
            message_group: "domain:entity:123".to_string(),
        }
    }

    #[test]
    fn from_ctx_copies_tracing_ids_and_generates_an_event_id() {
        let ctx = ExecutionContext::with_correlation("prn_test", "corr_from_ctx");
        let meta = EventMetadata::from_ctx(
            &ctx,
            "test.event",
            "1.0",
            "test",
            "sub.1",
            String::from("grp:1"),
        );

        assert!(!meta.event_id.is_empty());
        assert_eq!(meta.event_type, "test.event");
        assert_eq!(meta.spec_version, "1.0");
        assert_eq!(meta.source, "test");
        assert_eq!(meta.subject, "sub.1");
        assert_eq!(meta.message_group, "grp:1");
        assert_eq!(meta.execution_id, ctx.execution_id);
        assert_eq!(meta.correlation_id, "corr_from_ctx");
        assert_eq!(meta.principal_id, "prn_test");
        assert!(meta.causation_id.is_none());
    }

    #[test]
    fn from_ctx_carries_causation_and_unique_event_ids() {
        let mut ctx = ExecutionContext::create("prn");
        ctx.causation_id = Some("evt_parent".to_string());
        let a = EventMetadata::from_ctx(&ctx, "t", "1", "s", "sub", "grp");
        let b = EventMetadata::from_ctx(&ctx, "t", "1", "s", "sub", "grp");
        assert_eq!(a.causation_id.as_deref(), Some("evt_parent"));
        assert_ne!(a.event_id, b.event_id);
    }

    #[test]
    fn test_event_metadata() {
        let metadata = sample_metadata();

        let event = TestEvent {
            metadata,
            test_field: "test value".to_string(),
        };

        assert_eq!(event.event_id(), "evt-123");
        assert_eq!(event.event_type(), "test:domain:entity:created");
        assert_eq!(event.spec_version(), "1.0");
        assert_eq!(event.source(), "test:domain");
        assert_eq!(event.subject(), "domain.entity.123");
        assert_eq!(event.execution_id(), "exec-456");
        assert_eq!(event.correlation_id(), "corr-789");
        assert!(event.causation_id().is_none());
        assert_eq!(event.principal_id(), "principal-001");
        assert_eq!(event.message_group(), "domain:entity:123");
    }

    #[test]
    fn test_to_data_json() {
        let metadata = sample_metadata();

        let event = TestEvent {
            metadata,
            test_field: "test value".to_string(),
        };

        let json = event.to_data_json();
        assert!(json.contains("test_field"));
        assert!(json.contains("test value"));
    }
}
