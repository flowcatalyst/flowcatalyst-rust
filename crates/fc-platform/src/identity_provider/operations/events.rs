//! Identity Provider Domain Events

use serde::{Deserialize, Serialize};
use crate::usecase::ExecutionContext;
use crate::usecase::domain_event::EventMetadata;
use crate::TsidGenerator;
use crate::EntityType;
use crate::impl_domain_event;

/// Event emitted when a new identity provider is created.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityProviderCreated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub idp_id: String,
    pub code: String,
    pub name: String,
    pub idp_type: String,
}

impl_domain_event!(IdentityProviderCreated);

impl IdentityProviderCreated {
    const EVENT_TYPE: &'static str = "platform:admin:idp:created";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:admin";

    pub fn new(
        ctx: &ExecutionContext,
        idp_id: &str,
        code: &str,
        name: &str,
        idp_type: &str,
    ) -> Self {
        let event_id = TsidGenerator::generate(EntityType::Event);
        let subject = format!("platform.idp.{}", idp_id);
        let message_group = format!("platform:idp:{}", idp_id);

        Self {
            metadata: EventMetadata::new(
                event_id, Self::EVENT_TYPE, Self::SPEC_VERSION, Self::SOURCE,
                subject, message_group,
                ctx.execution_id.clone(), ctx.correlation_id.clone(),
                ctx.causation_id.clone(), ctx.principal_id.clone(),
            ),
            idp_id: idp_id.to_string(),
            code: code.to_string(),
            name: name.to_string(),
            idp_type: idp_type.to_string(),
        }
    }
}

/// Event emitted when an identity provider is updated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityProviderUpdated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub idp_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl_domain_event!(IdentityProviderUpdated);

impl IdentityProviderUpdated {
    const EVENT_TYPE: &'static str = "platform:admin:idp:updated";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:admin";

    pub fn new(
        ctx: &ExecutionContext,
        idp_id: &str,
        name: Option<&str>,
    ) -> Self {
        let event_id = TsidGenerator::generate(EntityType::Event);
        let subject = format!("platform.idp.{}", idp_id);
        let message_group = format!("platform:idp:{}", idp_id);

        Self {
            metadata: EventMetadata::new(
                event_id, Self::EVENT_TYPE, Self::SPEC_VERSION, Self::SOURCE,
                subject, message_group,
                ctx.execution_id.clone(), ctx.correlation_id.clone(),
                ctx.causation_id.clone(), ctx.principal_id.clone(),
            ),
            idp_id: idp_id.to_string(),
            name: name.map(String::from),
        }
    }
}

/// Event emitted when an identity provider is deleted.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityProviderDeleted {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub idp_id: String,
    pub code: String,
}

impl_domain_event!(IdentityProviderDeleted);

impl IdentityProviderDeleted {
    const EVENT_TYPE: &'static str = "platform:admin:idp:deleted";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:admin";

    pub fn new(ctx: &ExecutionContext, idp_id: &str, code: &str) -> Self {
        let event_id = TsidGenerator::generate(EntityType::Event);
        let subject = format!("platform.idp.{}", idp_id);
        let message_group = format!("platform:idp:{}", idp_id);

        Self {
            metadata: EventMetadata::new(
                event_id, Self::EVENT_TYPE, Self::SPEC_VERSION, Self::SOURCE,
                subject, message_group,
                ctx.execution_id.clone(), ctx.correlation_id.clone(),
                ctx.causation_id.clone(), ctx.principal_id.clone(),
            ),
            idp_id: idp_id.to_string(),
            code: code.to_string(),
        }
    }
}
