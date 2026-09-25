//! Connection Domain Events
//!
//! Type, source, subject, message group and `data` are Go's
//! (`internal/platform/connection/operations/events.go`): type
//! `platform:admin:connection:*`, source `platform:admin`, subject
//! `platform.connection.{id}`, group `platform:connection:{id}`, and each
//! payload carries exactly Go's `ToDataJSON` fields.

use crate::impl_domain_event;
use crate::usecase::domain_event::EventMetadata;
use crate::usecase::ExecutionContext;
use serde::{Deserialize, Serialize};

const SPEC_VERSION: &str = "1.0";
const SOURCE: &str = "platform:admin";

fn metadata(ctx: &ExecutionContext, event_type: &str, connection_id: &str) -> EventMetadata {
    EventMetadata::from_ctx(
        ctx,
        event_type,
        SPEC_VERSION,
        SOURCE,
        format!("platform.connection.{}", connection_id),
        format!("platform:connection:{}", connection_id),
    )
}

/// `{connectionId, code, name}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionCreated {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub connection_id: String,
    pub code: String,
    pub name: String,
}

impl_domain_event!(ConnectionCreated);

impl ConnectionCreated {
    pub const EVENT_TYPE: &'static str = "platform:admin:connection:created";

    pub fn new(ctx: &ExecutionContext, connection_id: &str, code: &str, name: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, connection_id),
            connection_id: connection_id.to_string(),
            code: code.to_string(),
            name: name.to_string(),
        }
    }
}

/// `{connectionId, name}`: the connection's name after the update (a status
/// change is an update too, as Go's pause/activate).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionUpdated {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub connection_id: String,
    pub name: String,
}

impl_domain_event!(ConnectionUpdated);

impl ConnectionUpdated {
    pub const EVENT_TYPE: &'static str = "platform:admin:connection:updated";

    pub fn new(ctx: &ExecutionContext, connection_id: &str, name: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, connection_id),
            connection_id: connection_id.to_string(),
            name: name.to_string(),
        }
    }
}

/// `{connectionId, code}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionDeleted {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub connection_id: String,
    pub code: String,
}

impl_domain_event!(ConnectionDeleted);

impl ConnectionDeleted {
    pub const EVENT_TYPE: &'static str = "platform:admin:connection:deleted";

    pub fn new(ctx: &ExecutionContext, connection_id: &str, code: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, connection_id),
            connection_id: connection_id.to_string(),
            code: code.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_payloads_are_go_shaped() {
        let ctx = ExecutionContext::create("prn_1");
        let e = ConnectionCreated::new(&ctx, "con_1", "erp", "ERP");
        assert_eq!(e.metadata.subject, "platform.connection.con_1");
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            serde_json::json!({"connectionId": "con_1", "code": "erp", "name": "ERP"})
        );
        let u = ConnectionUpdated::new(&ctx, "con_1", "ERP");
        assert_eq!(
            serde_json::to_value(&u).unwrap(),
            serde_json::json!({"connectionId": "con_1", "name": "ERP"})
        );
    }
}
