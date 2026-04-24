//! PlatformConfig Domain Events

use serde::{Deserialize, Serialize};
use crate::usecase::ExecutionContext;
use crate::usecase::domain_event::EventMetadata;
use crate::TsidGenerator;
use crate::impl_domain_event;

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

    pub fn new(
        ctx: &ExecutionContext,
        config_id: &str,
        application_code: &str,
        section: &str,
        property: &str,
        scope: &str,
        client_id: Option<&str>,
        value_type: &str,
        was_created: bool,
    ) -> Self {
        let event_id = TsidGenerator::generate_untyped();
        let subject = format!("platform.platformconfig.{}", config_id);
        let message_group = format!("platform:platformconfig:{}", config_id);
        Self {
            metadata: EventMetadata::new(
                event_id, Self::EVENT_TYPE, Self::SPEC_VERSION, Self::SOURCE,
                subject, message_group,
                ctx.execution_id.clone(), ctx.correlation_id.clone(),
                ctx.causation_id.clone(), ctx.principal_id.clone(),
            ),
            config_id: config_id.to_string(),
            application_code: application_code.to_string(),
            section: section.to_string(),
            property: property.to_string(),
            scope: scope.to_string(),
            client_id: client_id.map(String::from),
            value_type: value_type.to_string(),
            was_created,
        }
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

    pub fn new(
        ctx: &ExecutionContext,
        access_id: &str,
        application_code: &str,
        role_code: &str,
        can_read: bool,
        can_write: bool,
        was_created: bool,
    ) -> Self {
        let event_id = TsidGenerator::generate_untyped();
        let subject = format!("platform.platformconfigaccess.{}", access_id);
        let message_group = format!("platform:platformconfigaccess:{}", access_id);
        Self {
            metadata: EventMetadata::new(
                event_id, Self::EVENT_TYPE, Self::SPEC_VERSION, Self::SOURCE,
                subject, message_group,
                ctx.execution_id.clone(), ctx.correlation_id.clone(),
                ctx.causation_id.clone(), ctx.principal_id.clone(),
            ),
            access_id: access_id.to_string(),
            application_code: application_code.to_string(),
            role_code: role_code.to_string(),
            can_read,
            can_write,
            was_created,
        }
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
        let event_id = TsidGenerator::generate_untyped();
        let subject = format!("platform.platformconfigaccess.{}", access_id);
        let message_group = format!("platform:platformconfigaccess:{}", access_id);
        Self {
            metadata: EventMetadata::new(
                event_id, Self::EVENT_TYPE, Self::SPEC_VERSION, Self::SOURCE,
                subject, message_group,
                ctx.execution_id.clone(), ctx.correlation_id.clone(),
                ctx.causation_id.clone(), ctx.principal_id.clone(),
            ),
            access_id: access_id.to_string(),
            application_code: application_code.to_string(),
            role_code: role_code.to_string(),
        }
    }
}
