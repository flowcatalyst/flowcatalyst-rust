//! Email Domain Mapping Domain Events

use crate::impl_domain_event;
use crate::usecase::domain_event::EventMetadata;
use crate::usecase::ExecutionContext;
use serde::{Deserialize, Serialize};

/// Event emitted when a new email domain mapping is created.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailDomainMappingCreated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub mapping_id: String,
    pub email_domain: String,
    pub identity_provider_id: String,
    pub scope_type: String,
}

impl_domain_event!(EmailDomainMappingCreated);

impl EmailDomainMappingCreated {
    const EVENT_TYPE: &'static str = "platform:admin:edm:created";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:admin";

    pub fn new(
        ctx: &ExecutionContext,
        mapping_id: &str,
        email_domain: &str,
        identity_provider_id: &str,
        scope_type: &str,
    ) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.edm.{}", mapping_id),
                format!("platform:edm:{}", mapping_id),
            ),
            mapping_id: mapping_id.to_string(),
            email_domain: email_domain.to_string(),
            identity_provider_id: identity_provider_id.to_string(),
            scope_type: scope_type.to_string(),
        }
    }
}

/// Event emitted when an email domain mapping is updated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailDomainMappingUpdated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub mapping_id: String,
    pub email_domain: String,
}

impl_domain_event!(EmailDomainMappingUpdated);

impl EmailDomainMappingUpdated {
    const EVENT_TYPE: &'static str = "platform:admin:edm:updated";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:admin";

    pub fn new(ctx: &ExecutionContext, mapping_id: &str, email_domain: &str) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.edm.{}", mapping_id),
                format!("platform:edm:{}", mapping_id),
            ),
            mapping_id: mapping_id.to_string(),
            email_domain: email_domain.to_string(),
        }
    }
}

/// Event emitted when an email domain mapping is deleted.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailDomainMappingDeleted {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub mapping_id: String,
    pub email_domain: String,
}

impl_domain_event!(EmailDomainMappingDeleted);

impl EmailDomainMappingDeleted {
    const EVENT_TYPE: &'static str = "platform:admin:edm:deleted";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:admin";

    pub fn new(ctx: &ExecutionContext, mapping_id: &str, email_domain: &str) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.edm.{}", mapping_id),
                format!("platform:edm:{}", mapping_id),
            ),
            mapping_id: mapping_id.to_string(),
            email_domain: email_domain.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_email_domain_mapping_created_event() {
        let ctx = ExecutionContext::create("admin-123");
        let event =
            EmailDomainMappingCreated::new(&ctx, "edm-1", "example.com", "idp-456", "ANCHOR");

        assert_eq!(event.metadata.event_type, "platform:admin:edm:created");
        assert_eq!(event.mapping_id, "edm-1");
        assert_eq!(event.email_domain, "example.com");
        assert_eq!(event.identity_provider_id, "idp-456");
        assert_eq!(event.scope_type, "ANCHOR");
        assert_eq!(event.metadata.subject, "platform.edm.edm-1");
        assert_eq!(event.metadata.message_group, "platform:edm:edm-1");
        assert_eq!(event.metadata.principal_id, "admin-123");
    }

    #[test]
    fn test_email_domain_mapping_created_serialization() {
        let ctx = ExecutionContext::create("user-1");
        let event = EmailDomainMappingCreated::new(&ctx, "edm-2", "test.org", "idp-1", "CLIENT");

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("edm-2"));
        assert!(json.contains("test.org"));
        assert!(json.contains("idp-1"));
        assert!(json.contains("CLIENT"));
    }

    #[test]
    fn test_email_domain_mapping_updated_event() {
        let ctx = ExecutionContext::create("admin-456");
        let event = EmailDomainMappingUpdated::new(&ctx, "edm-2", "updated.com");

        assert_eq!(event.metadata.event_type, "platform:admin:edm:updated");
        assert_eq!(event.mapping_id, "edm-2");
        assert_eq!(event.email_domain, "updated.com");
        assert_eq!(event.metadata.subject, "platform.edm.edm-2");
        assert_eq!(event.metadata.message_group, "platform:edm:edm-2");
        assert_eq!(event.metadata.principal_id, "admin-456");
    }

    #[test]
    fn test_email_domain_mapping_deleted_event() {
        let ctx = ExecutionContext::create("admin-789");
        let event = EmailDomainMappingDeleted::new(&ctx, "edm-3", "deleted.com");

        assert_eq!(event.metadata.event_type, "platform:admin:edm:deleted");
        assert_eq!(event.mapping_id, "edm-3");
        assert_eq!(event.email_domain, "deleted.com");
        assert_eq!(event.metadata.subject, "platform.edm.edm-3");
        assert_eq!(event.metadata.message_group, "platform:edm:edm-3");
        assert_eq!(event.metadata.principal_id, "admin-789");
    }

    #[test]
    fn test_event_metadata_ids_are_unique() {
        let ctx = ExecutionContext::create("user-1");
        let event1 = EmailDomainMappingCreated::new(&ctx, "edm-1", "a.com", "idp-1", "ANCHOR");
        let event2 = EmailDomainMappingCreated::new(&ctx, "edm-2", "b.com", "idp-2", "CLIENT");

        // Each event should get a unique event_id
        assert_ne!(event1.metadata.event_id, event2.metadata.event_id);
    }
}
