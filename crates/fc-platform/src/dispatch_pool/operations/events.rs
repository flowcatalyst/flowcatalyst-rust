//! Dispatch Pool Domain Events
//!
//! Type, source, subject, message group and `data` are Go's
//! (`internal/platform/dispatchpool/operations/events.go`): type
//! `platform:admin:dispatch-pool:*`, source `platform:admin`, subject
//! `platform.dispatchpool.{id}`, group `platform:dispatchpool:{id}`, and each
//! payload carries exactly Go's `ToDataJSON` fields (the id is `poolId`).

use crate::impl_domain_event;
use crate::usecase::domain_event::EventMetadata;
use crate::usecase::ExecutionContext;
use serde::{Deserialize, Serialize};

const SPEC_VERSION: &str = "1.0";
const SOURCE: &str = "platform:admin";

fn metadata(ctx: &ExecutionContext, event_type: &str, pool_id: &str) -> EventMetadata {
    EventMetadata::from_ctx(
        ctx,
        event_type,
        SPEC_VERSION,
        SOURCE,
        format!("platform.dispatchpool.{}", pool_id),
        format!("platform:dispatchpool:{}", pool_id),
    )
}

/// `{poolId, code, name}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchPoolCreated {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub pool_id: String,
    pub code: String,
    pub name: String,
}

impl_domain_event!(DispatchPoolCreated);

impl DispatchPoolCreated {
    pub const EVENT_TYPE: &'static str = "platform:admin:dispatch-pool:created";

    pub fn new(ctx: &ExecutionContext, pool_id: &str, code: &str, name: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, pool_id),
            pool_id: pool_id.to_string(),
            code: code.to_string(),
            name: name.to_string(),
        }
    }
}

/// `{poolId, name}`: the pool's name after the update.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchPoolUpdated {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub pool_id: String,
    pub name: String,
}

impl_domain_event!(DispatchPoolUpdated);

impl DispatchPoolUpdated {
    pub const EVENT_TYPE: &'static str = "platform:admin:dispatch-pool:updated";

    pub fn new(ctx: &ExecutionContext, pool_id: &str, name: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, pool_id),
            pool_id: pool_id.to_string(),
            name: name.to_string(),
        }
    }
}

macro_rules! pool_code_event {
    ($(#[$doc:meta])* $name:ident, $event_type:literal) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub struct $name {
            #[serde(skip)]
            pub metadata: EventMetadata,
            pub pool_id: String,
            pub code: String,
        }

        impl_domain_event!($name);

        impl $name {
            pub const EVENT_TYPE: &'static str = $event_type;

            pub fn new(ctx: &ExecutionContext, pool_id: &str, code: &str) -> Self {
                Self {
                    metadata: metadata(ctx, Self::EVENT_TYPE, pool_id),
                    pool_id: pool_id.to_string(),
                    code: code.to_string(),
                }
            }
        }
    };
}

pool_code_event!(
    /// `{poolId, code}`.
    DispatchPoolArchived,
    "platform:admin:dispatch-pool:archived"
);
pool_code_event!(
    /// `{poolId, code}`.
    DispatchPoolDeleted,
    "platform:admin:dispatch-pool:deleted"
);

/// The rollup of an SDK dispatch-pool sync:
/// `{applicationCode, created, updated, deleted, syncedCodes}` on subject
/// `platform.dispatchpools.{applicationCode}` and group
/// `platform:dispatchpools:{applicationCode}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchPoolsSynced {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub application_code: String,
    pub created: u32,
    pub updated: u32,
    pub deleted: u32,
    pub synced_codes: Vec<String>,
}

impl_domain_event!(DispatchPoolsSynced);

impl DispatchPoolsSynced {
    pub const EVENT_TYPE: &'static str = "platform:admin:dispatch-pools:synced";

    /// Metadata for this event, raised inside `ctx` for a sync of
    /// `application_code`.
    pub fn metadata_for(ctx: &ExecutionContext, application_code: &str) -> EventMetadata {
        let group = if application_code.is_empty() {
            "platform:dispatchpools".to_string()
        } else {
            format!("platform:dispatchpools:{}", application_code)
        };
        EventMetadata::from_ctx(
            ctx,
            Self::EVENT_TYPE,
            SPEC_VERSION,
            SOURCE,
            format!("platform.dispatchpools.{}", application_code),
            group,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dispatch_pool_created_event() {
        let ctx = ExecutionContext::create("admin-123");
        let event = DispatchPoolCreated::new(&ctx, "dp-1", "main-pool", "Main Pool");

        assert_eq!(
            event.metadata.event_type,
            "platform:admin:dispatch-pool:created"
        );
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            serde_json::json!({"poolId": "dp-1", "code": "main-pool", "name": "Main Pool"})
        );
    }

    #[test]
    fn test_dispatch_pool_archived_event() {
        let ctx = ExecutionContext::create("admin-123");
        let event = DispatchPoolArchived::new(&ctx, "dp-1", "main-pool");

        assert_eq!(
            event.metadata.event_type,
            "platform:admin:dispatch-pool:archived"
        );
        assert_eq!(event.metadata.subject, "platform.dispatchpool.dp-1");
    }
}
