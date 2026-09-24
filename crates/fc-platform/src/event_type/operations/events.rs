//! Event Type Domain Events

use crate::impl_domain_event;
use crate::usecase::domain_event::EventMetadata;
use crate::usecase::ExecutionContext;
use serde::{Deserialize, Serialize};

/// Event emitted when a new event type is created.
///
/// Event type: `platform:admin:eventtype:created`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventTypeCreated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    // Event-specific data
    pub event_type_id: String,
    pub code: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub application: String,
    pub subdomain: String,
    pub aggregate: String,
    pub event_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
}

impl_domain_event!(EventTypeCreated);

impl EventTypeCreated {
    const EVENT_TYPE: &'static str = "platform:admin:eventtype:created";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:admin";

    /// Metadata for this event, raised inside `ctx`.
    pub fn metadata_for(ctx: &ExecutionContext, event_type_id: &str) -> EventMetadata {
        EventMetadata::from_ctx(
            ctx,
            Self::EVENT_TYPE,
            Self::SPEC_VERSION,
            Self::SOURCE,
            format!("platform.eventtype.{}", event_type_id),
            format!("platform:eventtype:{}", event_type_id),
        )
    }
}

/// Event emitted when an event type is updated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventTypeUpdated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub event_type_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl_domain_event!(EventTypeUpdated);

impl EventTypeUpdated {
    const EVENT_TYPE: &'static str = "platform:admin:eventtype:updated";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:admin";

    pub fn new(
        ctx: &ExecutionContext,
        event_type_id: &str,
        name: Option<&str>,
        description: Option<&str>,
    ) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.eventtype.{}", event_type_id),
                format!("platform:eventtype:{}", event_type_id),
            ),
            event_type_id: event_type_id.to_string(),
            name: name.map(String::from),
            description: description.map(String::from),
        }
    }
}

/// Event emitted when an event type is archived.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventTypeArchived {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub event_type_id: String,
    pub code: String,
}

impl_domain_event!(EventTypeArchived);

impl EventTypeArchived {
    const EVENT_TYPE: &'static str = "platform:admin:eventtype:archived";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:admin";

    pub fn new(ctx: &ExecutionContext, event_type_id: &str, code: &str) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.eventtype.{}", event_type_id),
                format!("platform:eventtype:{}", event_type_id),
            ),
            event_type_id: event_type_id.to_string(),
            code: code.to_string(),
        }
    }
}

/// Event emitted when a schema version is added to an event type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaAdded {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub event_type_id: String,
    pub version: String,
    pub mime_type: String,
    pub schema_type: String,
}

impl_domain_event!(SchemaAdded);

impl SchemaAdded {
    const EVENT_TYPE: &'static str = "platform:admin:eventtype:schema-added";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:admin";

    pub fn new(
        ctx: &ExecutionContext,
        event_type_id: &str,
        version: &str,
        mime_type: &str,
        schema_type: &str,
    ) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.eventtype.{}", event_type_id),
                format!("platform:eventtype:{}", event_type_id),
            ),
            event_type_id: event_type_id.to_string(),
            version: version.to_string(),
            mime_type: mime_type.to_string(),
            schema_type: schema_type.to_string(),
        }
    }
}

/// Event emitted when a schema version is finalised.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaFinalised {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub event_type_id: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deprecated_version: Option<String>,
}

impl_domain_event!(SchemaFinalised);

impl SchemaFinalised {
    const EVENT_TYPE: &'static str = "platform:admin:eventtype:schema-finalised";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:admin";

    pub fn new(
        ctx: &ExecutionContext,
        event_type_id: &str,
        version: &str,
        deprecated_version: Option<&str>,
    ) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.eventtype.{}", event_type_id),
                format!("platform:eventtype:{}", event_type_id),
            ),
            event_type_id: event_type_id.to_string(),
            version: version.to_string(),
            deprecated_version: deprecated_version.map(String::from),
        }
    }
}

/// Event emitted when a schema version is deprecated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaDeprecated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub event_type_id: String,
    pub version: String,
}

impl_domain_event!(SchemaDeprecated);

impl SchemaDeprecated {
    const EVENT_TYPE: &'static str = "platform:admin:eventtype:schema-deprecated";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:admin";

    pub fn new(ctx: &ExecutionContext, event_type_id: &str, version: &str) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.eventtype.{}", event_type_id),
                format!("platform:eventtype:{}", event_type_id),
            ),
            event_type_id: event_type_id.to_string(),
            version: version.to_string(),
        }
    }
}

/// Event emitted when an event type is deleted.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventTypeDeleted {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub event_type_id: String,
    pub code: String,
}

impl_domain_event!(EventTypeDeleted);

impl EventTypeDeleted {
    const EVENT_TYPE: &'static str = "platform:admin:eventtype:deleted";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:admin";

    pub fn new(ctx: &ExecutionContext, event_type_id: &str, code: &str) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.eventtype.{}", event_type_id),
                format!("platform:eventtype:{}", event_type_id),
            ),
            event_type_id: event_type_id.to_string(),
            code: code.to_string(),
        }
    }
}

/// Event emitted when event types are synced from an application SDK.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventTypesSynced {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub application_code: String,
    pub created: u32,
    pub updated: u32,
    pub deleted: u32,
    pub synced_codes: Vec<String>,
    #[serde(default)]
    pub schemas_created: u32,
    #[serde(default)]
    pub schemas_updated: u32,
    #[serde(default)]
    pub schemas_unchanged: u32,
}

impl_domain_event!(EventTypesSynced);

impl EventTypesSynced {
    const EVENT_TYPE: &'static str = "platform:admin:eventtypes:synced";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:admin";

    /// Metadata for this event, raised inside `ctx`.
    pub fn metadata_for(ctx: &ExecutionContext, application_code: &str) -> EventMetadata {
        EventMetadata::from_ctx(
            ctx,
            Self::EVENT_TYPE,
            Self::SPEC_VERSION,
            Self::SOURCE,
            format!("platform.application.{}", application_code),
            format!("platform:application:{}", application_code),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usecase::DomainEvent;

    #[test]
    fn test_event_type_created_metadata() {
        let ctx = ExecutionContext::create("user-123");
        let metadata = EventTypeCreated::metadata_for(&ctx, "0HZXEQ5Y8JY5Z");

        assert_eq!(metadata.event_type, "platform:admin:eventtype:created");
        assert_eq!(metadata.principal_id, "user-123");
        assert_eq!(metadata.subject, "platform.eventtype.0HZXEQ5Y8JY5Z");
        assert_eq!(metadata.message_group, "platform:eventtype:0HZXEQ5Y8JY5Z");
    }

    #[test]
    fn test_event_type_updated() {
        let ctx = ExecutionContext::create("user-123");
        let event =
            EventTypeUpdated::new(&ctx, "et-123", Some("New Name"), Some("New Description"));

        assert_eq!(event.event_type(), "platform:admin:eventtype:updated");
        assert_eq!(event.event_type_id, "et-123");
        assert_eq!(event.name, Some("New Name".to_string()));
    }

    #[test]
    fn test_event_type_archived() {
        let ctx = ExecutionContext::create("user-123");
        let event = EventTypeArchived::new(&ctx, "et-123", "orders:fulfillment:order:created");

        assert_eq!(event.event_type(), "platform:admin:eventtype:archived");
        assert_eq!(event.code, "orders:fulfillment:order:created");
    }
}
