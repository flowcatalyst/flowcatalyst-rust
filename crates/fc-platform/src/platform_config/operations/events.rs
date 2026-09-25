//! PlatformConfig Domain Events
//!
//! Type, source, subject, message group and `data` are Go's
//! (`internal/platform/platformconfig/operations/events.go`): type
//! `platform:admin:platform-config:*`, source `platform:admin`, subject
//! `platform.platformconfig.{id}` and group `platform:platformconfig:{id}`
//! (the property's id, or the access grant's id), and each payload carries
//! exactly Go's `ToDataJSON` fields. A property's value never appears.
//!
//! Fields Go's payload lacks are kept for the use case's caller and marked
//! `#[serde(skip)]`, so they are not part of the event's data.

use crate::impl_domain_event;
use crate::usecase::domain_event::EventMetadata;
use crate::usecase::ExecutionContext;
use serde::{Deserialize, Serialize};

const SPEC_VERSION: &str = "1.0";
const SOURCE: &str = "platform:admin";

fn metadata(ctx: &ExecutionContext, event_type: &str, id: &str) -> EventMetadata {
    EventMetadata::from_ctx(
        ctx,
        event_type,
        SPEC_VERSION,
        SOURCE,
        format!("platform.platformconfig.{}", id),
        format!("platform:platformconfig:{}", id),
    )
}

/// `{configId, applicationCode, section, property}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformConfigPropertySet {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub config_id: String,
    pub application_code: String,
    pub section: String,
    pub property: String,
    #[serde(skip)]
    pub scope: String,
    #[serde(skip)]
    pub client_id: Option<String>,
    #[serde(skip)]
    pub value_type: String,
    #[serde(skip)]
    pub was_created: bool,
}

impl_domain_event!(PlatformConfigPropertySet);

impl PlatformConfigPropertySet {
    pub const EVENT_TYPE: &'static str = "platform:admin:platform-config:property-set";

    /// Metadata for this event, raised inside `ctx`.
    pub fn metadata_for(ctx: &ExecutionContext, config_id: &str) -> EventMetadata {
        metadata(ctx, Self::EVENT_TYPE, config_id)
    }
}

/// `{accessId, applicationCode, roleCode, canWrite}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformConfigAccessGranted {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub access_id: String,
    pub application_code: String,
    pub role_code: String,
    #[serde(skip)]
    pub can_read: bool,
    pub can_write: bool,
    #[serde(skip)]
    pub was_created: bool,
}

impl_domain_event!(PlatformConfigAccessGranted);

impl PlatformConfigAccessGranted {
    pub const EVENT_TYPE: &'static str = "platform:admin:platform-config:access-granted";

    /// Metadata for this event, raised inside `ctx`.
    pub fn metadata_for(ctx: &ExecutionContext, access_id: &str) -> EventMetadata {
        metadata(ctx, Self::EVENT_TYPE, access_id)
    }
}

/// `{accessId, applicationCode, roleCode}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformConfigAccessRevoked {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub access_id: String,
    pub application_code: String,
    pub role_code: String,
}

impl_domain_event!(PlatformConfigAccessRevoked);

impl PlatformConfigAccessRevoked {
    pub const EVENT_TYPE: &'static str = "platform:admin:platform-config:access-revoked";

    pub fn new(
        ctx: &ExecutionContext,
        access_id: &str,
        application_code: &str,
        role_code: &str,
    ) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, access_id),
            access_id: access_id.to_string(),
            application_code: application_code.to_string(),
            role_code: role_code.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_revoked_is_go_shaped() {
        let ctx = ExecutionContext::create("prn_1");
        let e = PlatformConfigAccessRevoked::new(&ctx, "pca_1", "orders", "orders:admin");
        assert_eq!(
            e.metadata.event_type,
            "platform:admin:platform-config:access-revoked"
        );
        assert_eq!(e.metadata.subject, "platform.platformconfig.pca_1");
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            serde_json::json!({
                "accessId": "pca_1",
                "applicationCode": "orders",
                "roleCode": "orders:admin"
            })
        );
    }
}
