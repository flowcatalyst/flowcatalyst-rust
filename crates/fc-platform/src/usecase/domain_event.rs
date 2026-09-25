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
///
/// # Shape
///
/// A domain event is a serializable struct carrying an [`EventMetadata`]. The
/// envelope (id, type, subject, tracing ids, …) is read through
/// [`metadata`](DomainEvent::metadata); the event's persisted JSON body (the
/// `msg_events.data` column) is its `Serialize` output, which holds the
/// event's own fields only: the metadata field is `#[serde(skip)]`, as Go's
/// `ToDataJSON` carries no envelope fields. Use
/// [`impl_domain_event!`](crate::impl_domain_event) to implement it for a
/// struct with a `metadata: EventMetadata` field.
pub trait DomainEvent: Serialize + Send + Sync {
    /// The CloudEvents-style envelope fields of this event.
    fn metadata(&self) -> &EventMetadata;
}

/// Common metadata for domain events.
///
/// This struct holds the common CloudEvents fields and tracing context.
/// Event implementations include it as a `metadata` field and implement
/// [`DomainEvent`] with [`impl_domain_event!`](crate::impl_domain_event).
/// Build it with [`EventMetadata::from_ctx`]. `Default` exists only so an
/// event can derive `Deserialize` with its metadata `#[serde(skip)]`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
            event_id: crate::shared::tsid::generate_untyped(),
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

/// A domain event rendered to its envelope and `data` payload, so events of
/// different types can travel in one list: the per-row events of a sync,
/// written ahead of its rollup (Go's `usecaseop.Sync`). It persists exactly
/// as the event it was taken from.
#[derive(Debug, Clone)]
pub struct RecordedEvent {
    metadata: EventMetadata,
    data: serde_json::Value,
}

impl RecordedEvent {
    /// Render `event`. Fails only if the event does not serialize.
    pub fn of<E: DomainEvent>(event: &E) -> Result<Self, super::UseCaseError> {
        let data = serde_json::to_value(event).map_err(|e| {
            super::UseCaseError::commit(format!("Failed to serialize domain event: {e}"))
        })?;
        Ok(Self {
            metadata: event.metadata().clone(),
            data,
        })
    }
}

impl Serialize for RecordedEvent {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.data.serialize(s)
    }
}

impl DomainEvent for RecordedEvent {
    fn metadata(&self) -> &EventMetadata {
        &self.metadata
    }
}

/// Serializes an empty list as JSON `null`, a non-empty one as the array.
///
/// Go marshals a nil slice as `null`; an event field Go builds by appending
/// to a nil slice (never `make`) is `null` when nothing was appended. Use as
/// `#[serde(serialize_with = "crate::usecase::domain_event::null_if_empty")]`
/// on exactly those fields, so the persisted `data` matches Go's.
pub fn null_if_empty<S: serde::Serializer>(v: &[String], s: S) -> Result<S::Ok, S::Error> {
    if v.is_empty() {
        s.serialize_none()
    } else {
        s.collect_seq(v)
    }
}

/// Implements [`DomainEvent`] for a struct with a `metadata: EventMetadata` field.
///
/// # Example
///
/// ```ignore
/// use fc_platform::usecase::EventMetadata;
/// use fc_platform::impl_domain_event;
///
/// #[derive(Serialize)]
/// pub struct UserCreated {
///     #[serde(skip)]
///     pub metadata: EventMetadata,
///     pub user_id: String,
///     pub email: String,
/// }
///
/// impl_domain_event!(UserCreated);
/// ```
///
/// `impl_domain_event!(Wrapper => field)` implements it for a use-case result
/// that wraps an event in `field`, taking the metadata from that event. Such a
/// wrapper's `Serialize` output must be exactly the wrapped event's (flatten
/// the event and skip any other field), since that output is what a commit
/// would persist.
#[macro_export]
macro_rules! impl_domain_event {
    ($event_type:ty) => {
        impl $crate::usecase::DomainEvent for $event_type {
            fn metadata(&self) -> &$crate::usecase::EventMetadata {
                &self.metadata
            }
        }
    };
    ($wrapper:ty => $event:ident) => {
        impl $crate::usecase::DomainEvent for $wrapper {
            fn metadata(&self) -> &$crate::usecase::EventMetadata {
                $crate::usecase::DomainEvent::metadata(&self.$event)
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
        #[serde(skip)]
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
    fn impl_domain_event_exposes_metadata() {
        let event = TestEvent {
            metadata: sample_metadata(),
            test_field: "test value".to_string(),
        };

        let metadata = event.metadata();
        assert_eq!(metadata.event_id, "evt-123");
        assert_eq!(metadata.event_type, "test:domain:entity:created");
        assert_eq!(metadata.subject, "domain.entity.123");
        assert_eq!(metadata.principal_id, "principal-001");
        assert_eq!(metadata.message_group, "domain:entity:123");
    }

    #[derive(Serialize)]
    struct TestResult {
        #[serde(flatten)]
        event: TestEvent,
        #[serde(skip_serializing)]
        #[allow(dead_code)]
        secret: String,
    }

    impl_domain_event!(TestResult => event);

    #[test]
    fn impl_domain_event_delegates_through_a_wrapper() {
        let result = TestResult {
            event: TestEvent {
                metadata: sample_metadata(),
                test_field: "test value".to_string(),
            },
            secret: "s3cret".to_string(),
        };

        assert_eq!(result.metadata().event_id, "evt-123");
        assert_eq!(
            serde_json::to_value(&result).unwrap(),
            serde_json::to_value(&result.event).unwrap()
        );
    }
}
