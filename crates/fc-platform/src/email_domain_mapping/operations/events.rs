//! Email Domain Mapping Domain Events
//!
//! Type, source, subject, message group and `data` are Go's
//! (`internal/platform/emaildomainmapping/operations/events.go`): type
//! `platform:admin:email-domain-mapping:*`, source `platform:admin`, subject
//! `platform.emaildomainmapping.{id}`, group
//! `platform:emaildomainmapping:{id}`, and each payload is Go's
//! `{mappingId, emailDomain}`.

use crate::impl_domain_event;
use crate::usecase::domain_event::EventMetadata;
use crate::usecase::ExecutionContext;
use serde::{Deserialize, Serialize};

const SPEC_VERSION: &str = "1.0";
const SOURCE: &str = "platform:admin";

fn metadata(ctx: &ExecutionContext, event_type: &str, mapping_id: &str) -> EventMetadata {
    EventMetadata::from_ctx(
        ctx,
        event_type,
        SPEC_VERSION,
        SOURCE,
        format!("platform.emaildomainmapping.{}", mapping_id),
        format!("platform:emaildomainmapping:{}", mapping_id),
    )
}

macro_rules! mapping_event {
    ($(#[$doc:meta])* $name:ident, $event_type:literal) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub struct $name {
            #[serde(skip)]
            pub metadata: EventMetadata,
            pub mapping_id: String,
            pub email_domain: String,
        }

        impl_domain_event!($name);

        impl $name {
            pub const EVENT_TYPE: &'static str = $event_type;

            pub fn new(ctx: &ExecutionContext, mapping_id: &str, email_domain: &str) -> Self {
                Self {
                    metadata: metadata(ctx, Self::EVENT_TYPE, mapping_id),
                    mapping_id: mapping_id.to_string(),
                    email_domain: email_domain.to_string(),
                }
            }
        }
    };
}

mapping_event!(
    /// `{mappingId, emailDomain}`.
    EmailDomainMappingCreated,
    "platform:admin:email-domain-mapping:created"
);
mapping_event!(
    /// `{mappingId, emailDomain}`.
    EmailDomainMappingUpdated,
    "platform:admin:email-domain-mapping:updated"
);
mapping_event!(
    /// `{mappingId, emailDomain}`.
    EmailDomainMappingDeleted,
    "platform:admin:email-domain-mapping:deleted"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_email_domain_mapping_created_event() {
        let ctx = ExecutionContext::create("admin-123");
        let event = EmailDomainMappingCreated::new(&ctx, "edm-1", "example.com");

        assert_eq!(
            event.metadata.event_type,
            "platform:admin:email-domain-mapping:created"
        );
        assert_eq!(event.metadata.subject, "platform.emaildomainmapping.edm-1");
        assert_eq!(
            event.metadata.message_group,
            "platform:emaildomainmapping:edm-1"
        );
        assert_eq!(event.metadata.principal_id, "admin-123");
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            serde_json::json!({"mappingId": "edm-1", "emailDomain": "example.com"})
        );
    }

    #[test]
    fn test_email_domain_mapping_updated_and_deleted_events() {
        let ctx = ExecutionContext::create("admin-456");
        let updated = EmailDomainMappingUpdated::new(&ctx, "edm-2", "updated.com");
        assert_eq!(
            updated.metadata.event_type,
            "platform:admin:email-domain-mapping:updated"
        );
        let deleted = EmailDomainMappingDeleted::new(&ctx, "edm-3", "deleted.com");
        assert_eq!(
            deleted.metadata.event_type,
            "platform:admin:email-domain-mapping:deleted"
        );
        assert_eq!(deleted.email_domain, "deleted.com");
    }

    #[test]
    fn test_event_metadata_ids_are_unique() {
        let ctx = ExecutionContext::create("user-1");
        let event1 = EmailDomainMappingCreated::new(&ctx, "edm-1", "a.com");
        let event2 = EmailDomainMappingCreated::new(&ctx, "edm-2", "b.com");
        assert_ne!(event1.metadata.event_id, event2.metadata.event_id);
    }
}
