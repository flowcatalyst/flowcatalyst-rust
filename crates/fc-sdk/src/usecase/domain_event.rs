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

use super::ExecutionContext;

/// A domain event: a serializable struct carrying an [`EventMetadata`].
///
/// The metadata (id, type, subject, tracing ids, …) is read through
/// [`metadata`](DomainEvent::metadata); the event's JSON body is its `Serialize`
/// output. Use [`impl_domain_event!`](crate::impl_domain_event) to implement it
/// for a struct with a `metadata: EventMetadata` field.
pub trait DomainEvent: Serialize + Send + Sync {
    /// The CloudEvents-style envelope fields of this event.
    fn metadata(&self) -> &EventMetadata;
}

/// Common metadata for domain events.
///
/// Include this as a `metadata` field in your event structs and use
/// [`impl_domain_event!`](crate::impl_domain_event) to implement
/// [`DomainEvent`]. Build it with [`EventMetadata::from_ctx`], or with a
/// struct literal when you need to set every field yourself.
///
/// # Example
///
/// ```
/// use fc_sdk::usecase::{EventMetadata, ExecutionContext};
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
///
/// let ctx = ExecutionContext::create("user-123");
/// let event = OrderCreated {
///     metadata: EventMetadata::from_ctx(
///         &ctx,
///         "shop:orders:order:created",
///         "1.0",
///         "shop:orders",
///         "orders.order.42",
///         "orders:order:42",
///     ),
///     order_id: "42".into(),
///     customer_id: "c-1".into(),
///     total: 9.99,
/// };
/// # let _ = event;
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
            event_id: crate::tsid::TsidGenerator::generate_untyped(),
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

/// Implements [`DomainEvent`] for a struct with a `metadata: EventMetadata` field.
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
            fn metadata(&self) -> &$crate::usecase::EventMetadata {
                &self.metadata
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_metadata(event_id: &str, causation_id: Option<&str>) -> EventMetadata {
        EventMetadata {
            event_id: event_id.into(),
            event_type: "orders:order:created".into(),
            spec_version: "1.0".into(),
            source: "shop:orders".into(),
            subject: "orders.order.42".into(),
            time: Utc::now(),
            execution_id: "exec-1".into(),
            correlation_id: "corr-1".into(),
            causation_id: causation_id.map(Into::into),
            principal_id: "prn_user".into(),
            message_group: "orders:order:42".into(),
        }
    }

    // ─── EventMetadata ──────────────────────────────────────────────────

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
        assert!(meta.time <= Utc::now());
    }

    #[test]
    fn from_ctx_carries_causation() {
        let ctx = ExecutionContext::with_correlation("prn", "corr").with_causation("evt_parent");
        let meta = EventMetadata::from_ctx(&ctx, "t", "1", "s", "sub", "grp");
        assert_eq!(meta.causation_id.as_deref(), Some("evt_parent"));
    }

    #[test]
    fn from_ctx_event_ids_are_unique() {
        let ctx = ExecutionContext::with_correlation("prn", "corr");
        let a = EventMetadata::from_ctx(&ctx, "t", "1", "s", "sub", "grp");
        let b = EventMetadata::from_ctx(&ctx, "t", "1", "s", "sub", "grp");
        assert_ne!(a.event_id, b.event_id);
    }

    #[test]
    fn event_metadata_serialization_round_trip() {
        let meta = sample_metadata("evt_rt", Some("cause-rt"));
        let json = serde_json::to_string(&meta).unwrap();
        let deserialized: EventMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(meta, deserialized);
    }

    #[test]
    fn event_metadata_causation_id_skipped_when_none() {
        let json = serde_json::to_string(&sample_metadata("e", None)).unwrap();
        assert!(!json.contains("causation_id"));
    }

    // ─── impl_domain_event! macro ───────────────────────────────────────

    #[derive(Debug, Clone, Serialize)]
    struct TestEvent {
        pub metadata: EventMetadata,
        pub order_id: String,
        pub amount: f64,
    }

    crate::impl_domain_event!(TestEvent);

    #[test]
    fn impl_domain_event_exposes_metadata() {
        let meta = sample_metadata("evt_macro", Some("cause-m"));
        let event = TestEvent {
            metadata: meta.clone(),
            order_id: "ord_1".into(),
            amount: 99.99,
        };
        assert_eq!(event.metadata(), &meta);
    }

    /// The outbox stores the event's full `Serialize` output as `data`; pin
    /// the exact encoding so trait changes can't alter it.
    #[test]
    fn event_json_is_the_struct_serialization() {
        let mut meta = sample_metadata("e", None);
        meta.time = "2026-01-02T03:04:05Z".parse().unwrap();
        let event = TestEvent {
            metadata: meta,
            order_id: "ord_42".into(),
            amount: 123.45,
        };

        let json = serde_json::to_string(&event).unwrap();
        assert_eq!(
            json,
            r#"{"metadata":{"event_id":"e","event_type":"orders:order:created","spec_version":"1.0","source":"shop:orders","subject":"orders.order.42","time":"2026-01-02T03:04:05Z","execution_id":"exec-1","correlation_id":"corr-1","principal_id":"prn_user","message_group":"orders:order:42"},"order_id":"ord_42","amount":123.45}"#
        );
    }
}
