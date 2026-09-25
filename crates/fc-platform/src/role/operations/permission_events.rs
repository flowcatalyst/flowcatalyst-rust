//! Domain events for per-role permission grants and the permission catalogue.
//!
//! The grant/revoke types, source, subject and message group are Go's
//! (`role/operations/events.go`, `permissions.go`). Go writes the catalogue
//! (`iam_permissions`) without an event; Rust routes every write through a
//! unit of work, so the catalogue writes carry the two `platform:admin:
//! permission:*` events below (a Rust addition, no Go counterpart).

use crate::impl_domain_event;
use crate::usecase::domain_event::EventMetadata;
use crate::usecase::ExecutionContext;
use serde::{Deserialize, Serialize};

const SPEC_VERSION: &str = "1.0";
const SOURCE: &str = "platform:admin";

// The grant/revoke events are `super::events::{RolePermissionGranted,
// RolePermissionRevoked}` (Go's shape).

fn permission_metadata(ctx: &ExecutionContext, event_type: &str, id: &str) -> EventMetadata {
    EventMetadata::from_ctx(
        ctx,
        event_type,
        SPEC_VERSION,
        SOURCE,
        format!("platform.permission.{}", id),
        format!("platform:permission:{}", id),
    )
}

/// A permission was defined (or redefined) in the catalogue.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionDefined {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub permission_id: String,
    pub permission: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl_domain_event!(PermissionDefined);

impl PermissionDefined {
    pub const EVENT_TYPE: &'static str = "platform:admin:permission:defined";

    pub fn new(
        ctx: &ExecutionContext,
        id: &str,
        permission: &str,
        description: Option<&str>,
    ) -> Self {
        Self {
            metadata: permission_metadata(ctx, Self::EVENT_TYPE, id),
            permission_id: id.to_string(),
            permission: permission.to_string(),
            description: description.map(str::to_string),
        }
    }
}

/// A permission was removed from the catalogue.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionDeleted {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub permission_id: String,
    pub permission: String,
}

impl_domain_event!(PermissionDeleted);

impl PermissionDeleted {
    pub const EVENT_TYPE: &'static str = "platform:admin:permission:deleted";

    pub fn new(ctx: &ExecutionContext, id: &str, permission: &str) -> Self {
        Self {
            metadata: permission_metadata(ctx, Self::EVENT_TYPE, id),
            permission_id: id.to_string(),
            permission: permission.to_string(),
        }
    }
}
