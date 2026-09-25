//! Client Domain Events
//!
//! Type, source, subject, message group and `data` are Go's
//! (`internal/platform/client/operations/events.go`): type
//! `platform:admin:client:*`, source `platform:admin`, subject
//! `platform.client.{id}`, group `platform:client:{id}`, and each payload
//! carries exactly Go's `ToDataJSON` fields, since subscribers read them.

use crate::impl_domain_event;
use crate::usecase::domain_event::EventMetadata;
use crate::usecase::ExecutionContext;
use serde::{Deserialize, Serialize};

const SPEC_VERSION: &str = "1.0";
const SOURCE: &str = "platform:admin";

fn metadata(ctx: &ExecutionContext, event_type: &str, client_id: &str) -> EventMetadata {
    EventMetadata::from_ctx(
        ctx,
        event_type,
        SPEC_VERSION,
        SOURCE,
        format!("platform.client.{}", client_id),
        format!("platform:client:{}", client_id),
    )
}

/// `{clientId, name, identifier}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientCreated {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub client_id: String,
    pub name: String,
    pub identifier: String,
}

impl_domain_event!(ClientCreated);

impl ClientCreated {
    pub const EVENT_TYPE: &'static str = "platform:admin:client:created";

    pub fn new(ctx: &ExecutionContext, client_id: &str, name: &str, identifier: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, client_id),
            client_id: client_id.to_string(),
            name: name.to_string(),
            identifier: identifier.to_string(),
        }
    }
}

/// `{clientId, name}`: the client's name after the update.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientUpdated {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub client_id: String,
    pub name: String,
}

impl_domain_event!(ClientUpdated);

impl ClientUpdated {
    pub const EVENT_TYPE: &'static str = "platform:admin:client:updated";

    pub fn new(ctx: &ExecutionContext, client_id: &str, name: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, client_id),
            client_id: client_id.to_string(),
            name: name.to_string(),
        }
    }
}

/// `{clientId}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientActivated {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub client_id: String,
}

impl_domain_event!(ClientActivated);

impl ClientActivated {
    pub const EVENT_TYPE: &'static str = "platform:admin:client:activated";

    pub fn new(ctx: &ExecutionContext, client_id: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, client_id),
            client_id: client_id.to_string(),
        }
    }
}

/// `{clientId, reason}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientSuspended {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub client_id: String,
    pub reason: String,
}

impl_domain_event!(ClientSuspended);

impl ClientSuspended {
    pub const EVENT_TYPE: &'static str = "platform:admin:client:suspended";

    pub fn new(ctx: &ExecutionContext, client_id: &str, reason: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, client_id),
            client_id: client_id.to_string(),
            reason: reason.to_string(),
        }
    }
}

/// `{clientId, identifier}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientDeleted {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub client_id: String,
    pub identifier: String,
}

impl_domain_event!(ClientDeleted);

impl ClientDeleted {
    pub const EVENT_TYPE: &'static str = "platform:admin:client:deleted";

    pub fn new(ctx: &ExecutionContext, client_id: &str, identifier: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, client_id),
            client_id: client_id.to_string(),
            identifier: identifier.to_string(),
        }
    }
}

/// `{clientId, category, text}`. The author is the event's principal, not a
/// payload field.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientNoteAdded {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub client_id: String,
    pub category: String,
    pub text: String,
}

impl_domain_event!(ClientNoteAdded);

impl ClientNoteAdded {
    pub const EVENT_TYPE: &'static str = "platform:admin:client:note-added";

    pub fn new(ctx: &ExecutionContext, client_id: &str, category: &str, text: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, client_id),
            client_id: client_id.to_string(),
            category: category.to_string(),
            text: text.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_created_event() {
        let ctx = ExecutionContext::create("user-123");
        let event = ClientCreated::new(&ctx, "client-1", "Acme Corp", "acme-corp");

        assert_eq!(event.metadata.event_type, "platform:admin:client:created");
        assert_eq!(event.metadata.source, "platform:admin");
        assert_eq!(event.metadata.subject, "platform.client.client-1");
        assert_eq!(event.metadata.message_group, "platform:client:client-1");
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            serde_json::json!({"clientId": "client-1", "name": "Acme Corp", "identifier": "acme-corp"})
        );
    }

    #[test]
    fn test_client_suspended_event() {
        let ctx = ExecutionContext::create("user-123");
        let event = ClientSuspended::new(&ctx, "client-1", "Payment overdue");

        assert_eq!(event.metadata.event_type, "platform:admin:client:suspended");
        assert_eq!(event.reason, "Payment overdue");
    }
}
