//! Role Domain Events

use crate::impl_domain_event;
use crate::usecase::domain_event::EventMetadata;
use crate::usecase::ExecutionContext;
use serde::{Deserialize, Serialize};

/// Event emitted when a new role is created.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleCreated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub role_id: String,
    pub code: String,
    pub display_name: String,
    pub application_code: String,
    pub permissions: Vec<String>,
}

impl_domain_event!(RoleCreated);

impl RoleCreated {
    const EVENT_TYPE: &'static str = "platform:iam:role:created";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:iam";

    pub fn new(
        ctx: &ExecutionContext,
        role_id: &str,
        code: &str,
        display_name: &str,
        application_code: &str,
        permissions: Vec<String>,
    ) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.role.{}", role_id),
                format!("platform:role:{}", role_id),
            ),
            role_id: role_id.to_string(),
            code: code.to_string(),
            display_name: display_name.to_string(),
            application_code: application_code.to_string(),
            permissions,
        }
    }
}

/// Event emitted when a role is updated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleUpdated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub role_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub permissions_added: Vec<String>,
    pub permissions_removed: Vec<String>,
}

impl_domain_event!(RoleUpdated);

impl RoleUpdated {
    const EVENT_TYPE: &'static str = "platform:iam:role:updated";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:iam";

    pub fn new(
        ctx: &ExecutionContext,
        role_id: &str,
        display_name: Option<&str>,
        description: Option<&str>,
        permissions_added: Vec<String>,
        permissions_removed: Vec<String>,
    ) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.role.{}", role_id),
                format!("platform:role:{}", role_id),
            ),
            role_id: role_id.to_string(),
            display_name: display_name.map(String::from),
            description: description.map(String::from),
            permissions_added,
            permissions_removed,
        }
    }
}

/// Event emitted when a role is deleted.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleDeleted {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub role_id: String,
    pub code: String,
}

impl_domain_event!(RoleDeleted);

impl RoleDeleted {
    const EVENT_TYPE: &'static str = "platform:iam:role:deleted";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:iam";

    pub fn new(ctx: &ExecutionContext, role_id: &str, code: &str) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.role.{}", role_id),
                format!("platform:role:{}", role_id),
            ),
            role_id: role_id.to_string(),
            code: code.to_string(),
        }
    }
}

/// Event emitted when roles are synced from an application SDK.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RolesSynced {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub application_code: String,
    pub created: u32,
    pub updated: u32,
    pub deleted: u32,
    pub synced_names: Vec<String>,
}

impl_domain_event!(RolesSynced);

impl RolesSynced {
    const EVENT_TYPE: &'static str = "platform:iam:roles:synced";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:iam";

    /// Metadata for this event, raised inside `ctx` for a sync of
    /// `application_code`.
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

    #[test]
    fn test_role_created_event() {
        let ctx = ExecutionContext::create("admin-123");
        let event = RoleCreated::new(
            &ctx,
            "role-1",
            "orders:admin",
            "Orders Admin",
            "orders",
            vec!["orders:read".to_string(), "orders:write".to_string()],
        );

        assert_eq!(event.metadata.event_type, "platform:iam:role:created");
        assert_eq!(event.role_id, "role-1");
        assert_eq!(event.code, "orders:admin");
    }

    #[test]
    fn test_role_deleted_event() {
        let ctx = ExecutionContext::create("admin-123");
        let event = RoleDeleted::new(&ctx, "role-1", "orders:admin");

        assert_eq!(event.metadata.event_type, "platform:iam:role:deleted");
        assert_eq!(event.code, "orders:admin");
    }
}
