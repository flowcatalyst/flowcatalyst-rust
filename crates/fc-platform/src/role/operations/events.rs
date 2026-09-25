//! Role Domain Events
//!
//! Type, source, subject, message group and `data` are Go's
//! (`internal/platform/role/operations/events.go`): type
//! `platform:admin:role:*`, source `platform:admin`, subject
//! `platform.role.{id}`, group `platform:role:{id}`, and each payload carries
//! exactly Go's `ToDataJSON` fields. `name` is the role's full name
//! (`{application}:{role}`).

use crate::impl_domain_event;
use crate::usecase::domain_event::EventMetadata;
use crate::usecase::ExecutionContext;
use serde::{Deserialize, Serialize};

const SPEC_VERSION: &str = "1.0";
const SOURCE: &str = "platform:admin";

fn metadata(ctx: &ExecutionContext, event_type: &str, role_id: &str) -> EventMetadata {
    EventMetadata::from_ctx(
        ctx,
        event_type,
        SPEC_VERSION,
        SOURCE,
        format!("platform.role.{}", role_id),
        format!("platform:role:{}", role_id),
    )
}

macro_rules! role_event {
    ($(#[$doc:meta])* $name:ident, $event_type:literal) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub struct $name {
            #[serde(skip)]
            pub metadata: EventMetadata,
            pub role_id: String,
            pub name: String,
        }

        impl_domain_event!($name);

        impl $name {
            pub const EVENT_TYPE: &'static str = $event_type;

            pub fn new(ctx: &ExecutionContext, role_id: &str, name: &str) -> Self {
                Self {
                    metadata: metadata(ctx, Self::EVENT_TYPE, role_id),
                    role_id: role_id.to_string(),
                    name: name.to_string(),
                }
            }
        }
    };
}

role_event!(
    /// `{roleId, name}`.
    RoleCreated,
    "platform:admin:role:created"
);
role_event!(
    /// `{roleId, name}`.
    RoleUpdated,
    "platform:admin:role:updated"
);
role_event!(
    /// `{roleId, name}`.
    RoleDeleted,
    "platform:admin:role:deleted"
);

/// `{roleId, roleName, permission}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RolePermissionGranted {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub role_id: String,
    pub role_name: String,
    pub permission: String,
}

impl_domain_event!(RolePermissionGranted);

impl RolePermissionGranted {
    pub const EVENT_TYPE: &'static str = "platform:admin:role:permission-granted";

    pub fn new(ctx: &ExecutionContext, role_id: &str, role_name: &str, permission: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, role_id),
            role_id: role_id.to_string(),
            role_name: role_name.to_string(),
            permission: permission.to_string(),
        }
    }
}

/// `{roleId, roleName, permission}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RolePermissionRevoked {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub role_id: String,
    pub role_name: String,
    pub permission: String,
}

impl_domain_event!(RolePermissionRevoked);

impl RolePermissionRevoked {
    pub const EVENT_TYPE: &'static str = "platform:admin:role:permission-revoked";

    pub fn new(ctx: &ExecutionContext, role_id: &str, role_name: &str, permission: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, role_id),
            role_id: role_id.to_string(),
            role_name: role_name.to_string(),
            permission: permission.to_string(),
        }
    }
}

/// The rollup of an application's SDK role sync:
/// `{created, updated, removed, total, applicationCode, syncedCodes}`.
/// `total` is the number of roles in the payload; `applicationCode` and
/// `syncedCodes` are omitted when empty. Subject `platform.roles` (Go's
/// `Subject()` is fixed), group `platform:roles:{applicationCode}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RolesSynced {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub created: u32,
    pub updated: u32,
    pub removed: u32,
    pub total: u32,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub application_code: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub synced_codes: Vec<String>,
}

impl_domain_event!(RolesSynced);

impl RolesSynced {
    pub const EVENT_TYPE: &'static str = "platform:admin:roles:synced";

    /// Metadata for this event, raised inside `ctx` for a sync of
    /// `application_code`.
    pub fn metadata_for(ctx: &ExecutionContext, application_code: &str) -> EventMetadata {
        let group = if application_code.is_empty() {
            "platform:roles".to_string()
        } else {
            format!("platform:roles:{}", application_code)
        };
        EventMetadata::from_ctx(
            ctx,
            Self::EVENT_TYPE,
            SPEC_VERSION,
            SOURCE,
            "platform.roles",
            group,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_created_event() {
        let ctx = ExecutionContext::create("admin-123");
        let event = RoleCreated::new(&ctx, "role-1", "orders:admin");

        assert_eq!(event.metadata.event_type, "platform:admin:role:created");
        assert_eq!(event.metadata.source, "platform:admin");
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            serde_json::json!({"roleId": "role-1", "name": "orders:admin"})
        );
    }

    #[test]
    fn test_role_deleted_event() {
        let ctx = ExecutionContext::create("admin-123");
        let event = RoleDeleted::new(&ctx, "role-1", "orders:admin");

        assert_eq!(event.metadata.event_type, "platform:admin:role:deleted");
        assert_eq!(event.name, "orders:admin");
    }
}
