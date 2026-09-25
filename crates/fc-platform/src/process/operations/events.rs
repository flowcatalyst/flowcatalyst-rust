//! Process Domain Events
//!
//! Type, source, subject, message group and `data` are Go's
//! (`internal/platform/process/operations/events.go`): type
//! `platform:admin:process:*`, source `platform:admin`, subject
//! `platform.process.{id}`, group `platform:process:{id}`, and each payload
//! carries exactly Go's `ToDataJSON` fields.

use crate::impl_domain_event;
use crate::usecase::domain_event::EventMetadata;
use crate::usecase::ExecutionContext;
use serde::{Deserialize, Serialize};

const SPEC_VERSION: &str = "1.0";
const SOURCE: &str = "platform:admin";

fn metadata(ctx: &ExecutionContext, event_type: &str, process_id: &str) -> EventMetadata {
    EventMetadata::from_ctx(
        ctx,
        event_type,
        SPEC_VERSION,
        SOURCE,
        format!("platform.process.{}", process_id),
        format!("platform:process:{}", process_id),
    )
}

/// `{processId, code, name}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessCreated {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub process_id: String,
    pub code: String,
    pub name: String,
}

impl_domain_event!(ProcessCreated);

impl ProcessCreated {
    pub const EVENT_TYPE: &'static str = "platform:admin:process:created";

    pub fn new(ctx: &ExecutionContext, process_id: &str, code: &str, name: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, process_id),
            process_id: process_id.to_string(),
            code: code.to_string(),
            name: name.to_string(),
        }
    }
}

/// `{processId, name}`: the process's name after the update.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessUpdated {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub process_id: String,
    pub name: String,
}

impl_domain_event!(ProcessUpdated);

impl ProcessUpdated {
    pub const EVENT_TYPE: &'static str = "platform:admin:process:updated";

    pub fn new(ctx: &ExecutionContext, process_id: &str, name: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, process_id),
            process_id: process_id.to_string(),
            name: name.to_string(),
        }
    }
}

macro_rules! process_code_event {
    ($(#[$doc:meta])* $name:ident, $event_type:literal) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub struct $name {
            #[serde(skip)]
            pub metadata: EventMetadata,
            pub process_id: String,
            pub code: String,
        }

        impl_domain_event!($name);

        impl $name {
            pub const EVENT_TYPE: &'static str = $event_type;

            pub fn new(ctx: &ExecutionContext, process_id: &str, code: &str) -> Self {
                Self {
                    metadata: metadata(ctx, Self::EVENT_TYPE, process_id),
                    process_id: process_id.to_string(),
                    code: code.to_string(),
                }
            }
        }
    };
}

process_code_event!(
    /// `{processId, code}`.
    ProcessArchived,
    "platform:admin:process:archived"
);
process_code_event!(
    /// `{processId, code}`.
    ProcessDeleted,
    "platform:admin:process:deleted"
);

/// The rollup of an SDK process sync:
/// `{applicationCode, created, updated, deleted, syncedCodes}` on subject
/// `platform.processes.{applicationCode}` and group
/// `platform:processes:{applicationCode}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessesSynced {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub application_code: String,
    pub created: u32,
    pub updated: u32,
    pub deleted: u32,
    pub synced_codes: Vec<String>,
}

impl_domain_event!(ProcessesSynced);

impl ProcessesSynced {
    pub const EVENT_TYPE: &'static str = "platform:admin:processes:synced";

    /// Metadata for this event, raised inside `ctx` for a sync of
    /// `application_code`.
    pub fn metadata_for(ctx: &ExecutionContext, application_code: &str) -> EventMetadata {
        let group = if application_code.is_empty() {
            "platform:processes".to_string()
        } else {
            format!("platform:processes:{}", application_code)
        };
        EventMetadata::from_ctx(
            ctx,
            Self::EVENT_TYPE,
            SPEC_VERSION,
            SOURCE,
            format!("platform.processes.{}", application_code),
            group,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_events_are_go_shaped() {
        let ctx = ExecutionContext::create("prn_1");
        let e = ProcessUpdated::new(&ctx, "prc_1", "Order flow");
        assert_eq!(e.metadata.event_type, "platform:admin:process:updated");
        assert_eq!(e.metadata.subject, "platform.process.prc_1");
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            serde_json::json!({"processId": "prc_1", "name": "Order flow"})
        );
        let meta = ProcessesSynced::metadata_for(&ctx, "orders");
        assert_eq!(meta.subject, "platform.processes.orders");
        assert_eq!(meta.message_group, "platform:processes:orders");
    }
}
