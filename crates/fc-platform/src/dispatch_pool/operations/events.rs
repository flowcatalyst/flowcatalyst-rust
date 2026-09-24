//! Dispatch Pool Domain Events

use crate::impl_domain_event;
use crate::usecase::domain_event::EventMetadata;
use crate::usecase::ExecutionContext;
use serde::{Deserialize, Serialize};

/// Event emitted when a new dispatch pool is created.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchPoolCreated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub dispatch_pool_id: String,
    pub code: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
}

impl_domain_event!(DispatchPoolCreated);

impl DispatchPoolCreated {
    const EVENT_TYPE: &'static str = "platform:admin:dispatch-pool:created";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:admin";

    pub fn new(
        ctx: &ExecutionContext,
        dispatch_pool_id: &str,
        code: &str,
        name: &str,
        client_id: Option<&str>,
    ) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.dispatchpool.{}", dispatch_pool_id),
                format!("platform:dispatchpool:{}", dispatch_pool_id),
            ),
            dispatch_pool_id: dispatch_pool_id.to_string(),
            code: code.to_string(),
            name: name.to_string(),
            client_id: client_id.map(String::from),
        }
    }
}

/// Event emitted when a dispatch pool is updated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchPoolUpdated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub dispatch_pool_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concurrency: Option<u32>,
}

impl_domain_event!(DispatchPoolUpdated);

impl DispatchPoolUpdated {
    const EVENT_TYPE: &'static str = "platform:admin:dispatch-pool:updated";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:admin";

    pub fn new(
        ctx: &ExecutionContext,
        dispatch_pool_id: &str,
        name: Option<&str>,
        rate_limit: Option<u32>,
        concurrency: Option<u32>,
    ) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.dispatchpool.{}", dispatch_pool_id),
                format!("platform:dispatchpool:{}", dispatch_pool_id),
            ),
            dispatch_pool_id: dispatch_pool_id.to_string(),
            name: name.map(String::from),
            rate_limit,
            concurrency,
        }
    }
}

/// Event emitted when a dispatch pool is archived.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchPoolArchived {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub dispatch_pool_id: String,
    pub code: String,
}

impl_domain_event!(DispatchPoolArchived);

impl DispatchPoolArchived {
    const EVENT_TYPE: &'static str = "platform:admin:dispatch-pool:archived";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:admin";

    pub fn new(ctx: &ExecutionContext, dispatch_pool_id: &str, code: &str) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.dispatchpool.{}", dispatch_pool_id),
                format!("platform:dispatchpool:{}", dispatch_pool_id),
            ),
            dispatch_pool_id: dispatch_pool_id.to_string(),
            code: code.to_string(),
        }
    }
}

/// Event emitted when a dispatch pool is deleted.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchPoolDeleted {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub dispatch_pool_id: String,
    pub code: String,
}

impl_domain_event!(DispatchPoolDeleted);

impl DispatchPoolDeleted {
    const EVENT_TYPE: &'static str = "platform:admin:dispatch-pool:deleted";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:admin";

    pub fn new(ctx: &ExecutionContext, dispatch_pool_id: &str, code: &str) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.dispatchpool.{}", dispatch_pool_id),
                format!("platform:dispatchpool:{}", dispatch_pool_id),
            ),
            dispatch_pool_id: dispatch_pool_id.to_string(),
            code: code.to_string(),
        }
    }
}

/// Event emitted when dispatch pools are synced from an application SDK.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchPoolsSynced {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub application_code: String,
    pub created: u32,
    pub updated: u32,
    pub deleted: u32,
    pub synced_codes: Vec<String>,
}

impl_domain_event!(DispatchPoolsSynced);

impl DispatchPoolsSynced {
    const EVENT_TYPE: &'static str = "platform:admin:dispatch-pools:synced";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:admin";

    pub fn new(
        ctx: &ExecutionContext,
        application_code: &str,
        created: u32,
        updated: u32,
        deleted: u32,
        synced_codes: Vec<String>,
    ) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.application.{}", application_code),
                format!("platform:application:{}", application_code),
            ),
            application_code: application_code.to_string(),
            created,
            updated,
            deleted,
            synced_codes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dispatch_pool_created_event() {
        let ctx = ExecutionContext::create("admin-123");
        let event =
            DispatchPoolCreated::new(&ctx, "dp-1", "main-pool", "Main Pool", Some("client-1"));

        assert_eq!(
            event.metadata.event_type,
            "platform:admin:dispatch-pool:created"
        );
        assert_eq!(event.dispatch_pool_id, "dp-1");
        assert_eq!(event.code, "main-pool");
    }

    #[test]
    fn test_dispatch_pool_archived_event() {
        let ctx = ExecutionContext::create("admin-123");
        let event = DispatchPoolArchived::new(&ctx, "dp-1", "main-pool");

        assert_eq!(
            event.metadata.event_type,
            "platform:admin:dispatch-pool:archived"
        );
    }
}
