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

use crate::platform_config::access_entity::PlatformConfigAccess;
use crate::platform_config::entity::PlatformConfig;
use fc_platform_core::impl_domain_event;
use fc_platform_core::shared::id::ClientId;
use fc_platform_core::shared::id::PlatformConfigAccessId;
use fc_platform_core::shared::id::PlatformConfigId;
use fc_platform_core::usecase::domain_event::EventMetadata;
use fc_platform_core::usecase::ExecutionContext;
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
    pub config_id: PlatformConfigId,
    pub application_code: String,
    pub section: String,
    pub property: String,
    #[serde(skip)]
    pub scope: String,
    #[serde(skip)]
    pub client_id: Option<ClientId>,
    #[serde(skip)]
    pub value_type: String,
    #[serde(skip)]
    pub was_created: bool,
}

impl_domain_event!(PlatformConfigPropertySet);

impl PlatformConfigPropertySet {
    pub const EVENT_TYPE: &'static str = "platform:admin:platform-config:property-set";

    /// The event for `config` as just set inside `ctx`; `was_created` when
    /// the set created it.
    pub fn new(ctx: &ExecutionContext, config: &PlatformConfig, was_created: bool) -> Self {
        Self {
            metadata: Self::metadata_for(ctx, &config.id),
            config_id: config.id.clone(),
            application_code: config.application_code.clone(),
            section: config.section.clone(),
            property: config.property.clone(),
            scope: config.scope.as_str().to_string(),
            client_id: config.client_id.clone(),
            value_type: config.value_type.as_str().to_string(),
            was_created,
        }
    }

    /// Metadata for this event, raised inside `ctx`.
    pub fn metadata_for(ctx: &ExecutionContext, config_id: &PlatformConfigId) -> EventMetadata {
        metadata(ctx, Self::EVENT_TYPE, config_id.as_str())
    }
}

/// `{accessId, applicationCode, roleCode, canWrite}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformConfigAccessGranted {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub access_id: PlatformConfigAccessId,
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

    /// The event for `access` as just granted inside `ctx`; `was_created`
    /// when the grant created it.
    pub fn new(ctx: &ExecutionContext, access: &PlatformConfigAccess, was_created: bool) -> Self {
        Self {
            metadata: Self::metadata_for(ctx, &access.id),
            access_id: access.id.clone(),
            application_code: access.application_code.clone(),
            role_code: access.role_code.clone(),
            can_read: access.can_read,
            can_write: access.can_write,
            was_created,
        }
    }

    /// Metadata for this event, raised inside `ctx`.
    pub fn metadata_for(
        ctx: &ExecutionContext,
        access_id: &PlatformConfigAccessId,
    ) -> EventMetadata {
        metadata(ctx, Self::EVENT_TYPE, access_id.as_str())
    }
}

/// `{accessId, applicationCode, roleCode}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformConfigAccessRevoked {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub access_id: PlatformConfigAccessId,
    pub application_code: String,
    pub role_code: String,
}

impl_domain_event!(PlatformConfigAccessRevoked);

impl PlatformConfigAccessRevoked {
    pub const EVENT_TYPE: &'static str = "platform:admin:platform-config:access-revoked";

    pub fn new(
        ctx: &ExecutionContext,
        access_id: &PlatformConfigAccessId,
        application_code: &str,
        role_code: &str,
    ) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, access_id.as_str()),
            access_id: access_id.clone(),
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
        let ctx = ExecutionContext::system("prn_1");
        let e = PlatformConfigAccessRevoked::new(
            &ctx,
            &PlatformConfigAccessId::from_wire("pca_1"),
            "orders",
            "orders:admin",
        );
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
