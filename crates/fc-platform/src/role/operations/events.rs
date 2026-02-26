//! Role Domain Events

use serde::{Deserialize, Serialize};
use crate::usecase::ExecutionContext;
use crate::usecase::domain_event::EventMetadata;
use crate::TsidGenerator;
use crate::impl_domain_event;

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
        let event_id = TsidGenerator::generate();
        let subject = format!("platform.role.{}", role_id);
        let message_group = format!("platform:role:{}", role_id);

        Self {
            metadata: EventMetadata::new(
                event_id,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                subject,
                message_group,
                ctx.execution_id.clone(),
                ctx.correlation_id.clone(),
                ctx.causation_id.clone(),
                ctx.principal_id.clone(),
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
        let event_id = TsidGenerator::generate();
        let subject = format!("platform.role.{}", role_id);
        let message_group = format!("platform:role:{}", role_id);

        Self {
            metadata: EventMetadata::new(
                event_id,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                subject,
                message_group,
                ctx.execution_id.clone(),
                ctx.correlation_id.clone(),
                ctx.causation_id.clone(),
                ctx.principal_id.clone(),
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
        let event_id = TsidGenerator::generate();
        let subject = format!("platform.role.{}", role_id);
        let message_group = format!("platform:role:{}", role_id);

        Self {
            metadata: EventMetadata::new(
                event_id,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                subject,
                message_group,
                ctx.execution_id.clone(),
                ctx.correlation_id.clone(),
                ctx.causation_id.clone(),
                ctx.principal_id.clone(),
            ),
            role_id: role_id.to_string(),
            code: code.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usecase::DomainEvent;

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

        assert_eq!(event.event_type(), "platform:iam:role:created");
        assert_eq!(event.role_id, "role-1");
        assert_eq!(event.code, "orders:admin");
    }

    #[test]
    fn test_role_deleted_event() {
        let ctx = ExecutionContext::create("admin-123");
        let event = RoleDeleted::new(&ctx, "role-1", "orders:admin");

        assert_eq!(event.event_type(), "platform:iam:role:deleted");
        assert_eq!(event.code, "orders:admin");
    }
}
