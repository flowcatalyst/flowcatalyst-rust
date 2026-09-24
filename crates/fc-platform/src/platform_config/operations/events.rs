//! PlatformConfig Domain Events

use crate::impl_domain_event;
use crate::usecase::domain_event::EventMetadata;
use crate::usecase::ExecutionContext;
use serde::{Deserialize, Serialize};

/// Emitted when a platform config property is created or updated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformConfigPropertySet {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub config_id: String,
    pub application_code: String,
    pub section: String,
    pub property: String,
    pub scope: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    pub value_type: String,
    /// Whether this set created a new row vs updated an existing one.
    pub was_created: bool,
}

impl_domain_event!(PlatformConfigPropertySet);

impl PlatformConfigPropertySet {
    const EVENT_TYPE: &'static str = "platform:admin:config:property-set";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:admin";

    /// Metadata for this event, raised inside `ctx`.
    pub fn metadata_for(ctx: &ExecutionContext, config_id: &str) -> EventMetadata {
        EventMetadata::from_ctx(
            ctx,
            Self::EVENT_TYPE,
            Self::SPEC_VERSION,
            Self::SOURCE,
            format!("platform.platformconfig.{}", config_id),
            format!("platform:platformconfig:{}", config_id),
        )
    }
}

/// Emitted when a role is granted (or its grant is updated) on a config app.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformConfigAccessGranted {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub access_id: String,
    pub application_code: String,
    pub role_code: String,
    pub can_read: bool,
    pub can_write: bool,
    pub was_created: bool,
}

impl_domain_event!(PlatformConfigAccessGranted);

impl PlatformConfigAccessGranted {
    const EVENT_TYPE: &'static str = "platform:admin:config-access:granted";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:admin";

    /// Metadata for this event, raised inside `ctx`.
    pub fn metadata_for(ctx: &ExecutionContext, access_id: &str) -> EventMetadata {
        EventMetadata::from_ctx(
            ctx,
            Self::EVENT_TYPE,
            Self::SPEC_VERSION,
            Self::SOURCE,
            format!("platform.platformconfigaccess.{}", access_id),
            format!("platform:platformconfigaccess:{}", access_id),
        )
    }
}

/// Emitted when a role's access on a config app is revoked.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformConfigAccessRevoked {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub access_id: String,
    pub application_code: String,
    pub role_code: String,
}

impl_domain_event!(PlatformConfigAccessRevoked);

impl PlatformConfigAccessRevoked {
    const EVENT_TYPE: &'static str = "platform:admin:config-access:revoked";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:admin";

    pub fn new(
        ctx: &ExecutionContext,
        access_id: &str,
        application_code: &str,
        role_code: &str,
    ) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.platformconfigaccess.{}", access_id),
                format!("platform:platformconfigaccess:{}", access_id),
            ),
            access_id: access_id.to_string(),
            application_code: application_code.to_string(),
            role_code: role_code.to_string(),
        }
    }
}
