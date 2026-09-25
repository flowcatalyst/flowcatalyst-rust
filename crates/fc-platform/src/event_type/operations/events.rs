//! Event Type Domain Events
//!
//! Type, source, subject, message group and `data` are Go's
//! (`internal/platform/eventtype/operations/events.go`): type
//! `platform:admin:eventtype:*` (no hyphen in the aggregate), source
//! `platform:admin`, subject `platform.eventtype.{id}`, and each payload
//! carries exactly Go's `ToDataJSON` fields. Go leaves these events' message
//! group empty (the column is NULL); a schema version is `specVersion`.

use crate::impl_domain_event;
use crate::usecase::domain_event::EventMetadata;
use crate::usecase::ExecutionContext;
use serde::{Deserialize, Serialize};

const SPEC_VERSION: &str = "1.0";
const SOURCE: &str = "platform:admin";

/// Metadata on subject `platform.eventtype.{id}` with no message group.
fn metadata(ctx: &ExecutionContext, event_type: &str, event_type_id: &str) -> EventMetadata {
    EventMetadata::from_ctx(
        ctx,
        event_type,
        SPEC_VERSION,
        SOURCE,
        format!("platform.eventtype.{}", event_type_id),
        String::new(),
    )
}

/// `{eventTypeId, code, name, description?, application, subdomain,
/// aggregate, eventName, clientId?}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventTypeCreated {
    #[serde(skip)]
    pub metadata: EventMetadata,
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
    pub const EVENT_TYPE: &'static str = "platform:admin:eventtype:created";

    /// Metadata for this event, raised inside `ctx`.
    pub fn metadata_for(ctx: &ExecutionContext, event_type_id: &str) -> EventMetadata {
        metadata(ctx, Self::EVENT_TYPE, event_type_id)
    }
}

/// `{eventTypeId, name, description?}`: the event type's name and
/// description after the update.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventTypeUpdated {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub event_type_id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl_domain_event!(EventTypeUpdated);

impl EventTypeUpdated {
    pub const EVENT_TYPE: &'static str = "platform:admin:eventtype:updated";

    pub fn new(
        ctx: &ExecutionContext,
        event_type_id: &str,
        name: &str,
        description: Option<&str>,
    ) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, event_type_id),
            event_type_id: event_type_id.to_string(),
            name: name.to_string(),
            description: description.map(String::from),
        }
    }
}

macro_rules! code_event {
    ($(#[$doc:meta])* $name:ident, $event_type:literal) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub struct $name {
            #[serde(skip)]
            pub metadata: EventMetadata,
            pub event_type_id: String,
            pub code: String,
        }

        impl_domain_event!($name);

        impl $name {
            pub const EVENT_TYPE: &'static str = $event_type;

            pub fn new(ctx: &ExecutionContext, event_type_id: &str, code: &str) -> Self {
                Self {
                    metadata: metadata(ctx, Self::EVENT_TYPE, event_type_id),
                    event_type_id: event_type_id.to_string(),
                    code: code.to_string(),
                }
            }
        }
    };
}

code_event!(
    /// `{eventTypeId, code}`.
    EventTypeArchived,
    "platform:admin:eventtype:archived"
);
code_event!(
    /// `{eventTypeId, code}`.
    EventTypeDeleted,
    "platform:admin:eventtype:deleted"
);

/// `{eventTypeId, specVersion}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaAdded {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub event_type_id: String,
    #[serde(rename = "specVersion")]
    pub version: String,
}

impl_domain_event!(SchemaAdded);

impl SchemaAdded {
    pub const EVENT_TYPE: &'static str = "platform:admin:eventtype:schema-added";

    pub fn new(ctx: &ExecutionContext, event_type_id: &str, version: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, event_type_id),
            event_type_id: event_type_id.to_string(),
            version: version.to_string(),
        }
    }
}

/// `{eventTypeId, specVersion, deprecatedVersion?}`: `deprecatedVersion` is
/// the version finalising forced from CURRENT to DEPRECATED, if any.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaFinalised {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub event_type_id: String,
    #[serde(rename = "specVersion")]
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deprecated_version: Option<String>,
}

impl_domain_event!(SchemaFinalised);

impl SchemaFinalised {
    pub const EVENT_TYPE: &'static str = "platform:admin:eventtype:schema-finalised";

    pub fn new(
        ctx: &ExecutionContext,
        event_type_id: &str,
        version: &str,
        deprecated_version: Option<&str>,
    ) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, event_type_id),
            event_type_id: event_type_id.to_string(),
            version: version.to_string(),
            deprecated_version: deprecated_version.map(String::from),
        }
    }
}

/// `{eventTypeId, specVersion}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaDeprecated {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub event_type_id: String,
    #[serde(rename = "specVersion")]
    pub version: String,
}

impl_domain_event!(SchemaDeprecated);

impl SchemaDeprecated {
    pub const EVENT_TYPE: &'static str = "platform:admin:eventtype:schema-deprecated";

    pub fn new(ctx: &ExecutionContext, event_type_id: &str, version: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, event_type_id),
            event_type_id: event_type_id.to_string(),
            version: version.to_string(),
        }
    }
}

/// The rollup of an event-type sync:
/// `{applicationCode, created, updated, deleted, syncedCodes}` on subject
/// `platform.eventtypes.{applicationCode}` and group
/// `platform:eventtypes:{applicationCode}`. The schema tallies are for the
/// caller's response only; they are not part of the event's data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventTypesSynced {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub application_code: String,
    pub created: u32,
    pub updated: u32,
    pub deleted: u32,
    pub synced_codes: Vec<String>,
    #[serde(skip)]
    pub schemas_created: u32,
    #[serde(skip)]
    pub schemas_updated: u32,
    #[serde(skip)]
    pub schemas_unchanged: u32,
}

impl_domain_event!(EventTypesSynced);

impl EventTypesSynced {
    pub const EVENT_TYPE: &'static str = "platform:admin:eventtypes:synced";

    /// Metadata for this event, raised inside `ctx` for a sync of
    /// `application_code`.
    pub fn metadata_for(ctx: &ExecutionContext, application_code: &str) -> EventMetadata {
        EventMetadata::from_ctx(
            ctx,
            Self::EVENT_TYPE,
            SPEC_VERSION,
            SOURCE,
            format!("platform.eventtypes.{}", application_code),
            format!("platform:eventtypes:{}", application_code),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_type_created_metadata() {
        let ctx = ExecutionContext::create("user-123");
        let metadata = EventTypeCreated::metadata_for(&ctx, "0HZXEQ5Y8JY5Z");

        assert_eq!(metadata.event_type, "platform:admin:eventtype:created");
        assert_eq!(metadata.principal_id, "user-123");
        assert_eq!(metadata.subject, "platform.eventtype.0HZXEQ5Y8JY5Z");
        // Go leaves the event-type events' message group empty.
        assert_eq!(metadata.message_group, "");
    }

    #[test]
    fn test_event_type_updated() {
        let ctx = ExecutionContext::create("user-123");
        let event = EventTypeUpdated::new(&ctx, "et-123", "New Name", Some("New Description"));

        assert_eq!(
            event.metadata.event_type,
            "platform:admin:eventtype:updated"
        );
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            serde_json::json!({
                "eventTypeId": "et-123",
                "name": "New Name",
                "description": "New Description"
            })
        );
    }

    #[test]
    fn schema_events_carry_spec_version() {
        let ctx = ExecutionContext::create("user-123");
        let event = SchemaFinalised::new(&ctx, "et-123", "2.0", Some("1.0"));
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            serde_json::json!({
                "eventTypeId": "et-123",
                "specVersion": "2.0",
                "deprecatedVersion": "1.0"
            })
        );
    }

    #[test]
    fn test_event_type_archived() {
        let ctx = ExecutionContext::create("user-123");
        let event = EventTypeArchived::new(&ctx, "et-123", "orders:fulfillment:order:created");

        assert_eq!(
            event.metadata.event_type,
            "platform:admin:eventtype:archived"
        );
        assert_eq!(event.code, "orders:fulfillment:order:created");
    }
}
