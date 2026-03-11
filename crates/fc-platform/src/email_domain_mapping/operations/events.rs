//! Email Domain Mapping Domain Events

use serde::{Deserialize, Serialize};
use crate::usecase::ExecutionContext;
use crate::usecase::domain_event::EventMetadata;
use crate::TsidGenerator;
use crate::EntityType;
use crate::impl_domain_event;

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
        let event_id = TsidGenerator::generate(EntityType::Event);
        let subject = format!("platform.edm.{}", mapping_id);
        let message_group = format!("platform:edm:{}", mapping_id);

        Self {
            metadata: EventMetadata::new(
                event_id, Self::EVENT_TYPE, Self::SPEC_VERSION, Self::SOURCE,
                subject, message_group,
                ctx.execution_id.clone(), ctx.correlation_id.clone(),
                ctx.causation_id.clone(), ctx.principal_id.clone(),
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
        let event_id = TsidGenerator::generate(EntityType::Event);
        let subject = format!("platform.edm.{}", mapping_id);
        let message_group = format!("platform:edm:{}", mapping_id);

        Self {
            metadata: EventMetadata::new(
                event_id, Self::EVENT_TYPE, Self::SPEC_VERSION, Self::SOURCE,
                subject, message_group,
                ctx.execution_id.clone(), ctx.correlation_id.clone(),
                ctx.causation_id.clone(), ctx.principal_id.clone(),
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
        let event_id = TsidGenerator::generate(EntityType::Event);
        let subject = format!("platform.edm.{}", mapping_id);
        let message_group = format!("platform:edm:{}", mapping_id);

        Self {
            metadata: EventMetadata::new(
                event_id, Self::EVENT_TYPE, Self::SPEC_VERSION, Self::SOURCE,
                subject, message_group,
                ctx.execution_id.clone(), ctx.correlation_id.clone(),
                ctx.causation_id.clone(), ctx.principal_id.clone(),
            ),
            mapping_id: mapping_id.to_string(),
            email_domain: email_domain.to_string(),
        }
    }
}
