//! Auth Domain Events — AnchorDomain and ClientAuthConfig

use serde::{Deserialize, Serialize};
use crate::usecase::ExecutionContext;
use crate::usecase::domain_event::EventMetadata;
use crate::TsidGenerator;
use crate::EntityType;
use crate::impl_domain_event;

// ── AnchorDomain Events ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnchorDomainCreated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub anchor_domain_id: String,
    pub domain: String,
}

impl_domain_event!(AnchorDomainCreated);

impl AnchorDomainCreated {
    const EVENT_TYPE: &'static str = "platform:iam:anchor-domain:created";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:iam";

    pub fn new(ctx: &ExecutionContext, id: &str, domain: &str) -> Self {
        let event_id = TsidGenerator::generate(EntityType::Event);
        let subject = format!("platform.anchordomain.{}", id);
        let message_group = format!("platform:anchordomain:{}", id);

        Self {
            metadata: EventMetadata::new(
                event_id, Self::EVENT_TYPE, Self::SPEC_VERSION, Self::SOURCE,
                subject, message_group,
                ctx.execution_id.clone(), ctx.correlation_id.clone(),
                ctx.causation_id.clone(), ctx.principal_id.clone(),
            ),
            anchor_domain_id: id.to_string(),
            domain: domain.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnchorDomainDeleted {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub anchor_domain_id: String,
    pub domain: String,
}

impl_domain_event!(AnchorDomainDeleted);

impl AnchorDomainDeleted {
    const EVENT_TYPE: &'static str = "platform:iam:anchor-domain:deleted";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:iam";

    pub fn new(ctx: &ExecutionContext, id: &str, domain: &str) -> Self {
        let event_id = TsidGenerator::generate(EntityType::Event);
        let subject = format!("platform.anchordomain.{}", id);
        let message_group = format!("platform:anchordomain:{}", id);

        Self {
            metadata: EventMetadata::new(
                event_id, Self::EVENT_TYPE, Self::SPEC_VERSION, Self::SOURCE,
                subject, message_group,
                ctx.execution_id.clone(), ctx.correlation_id.clone(),
                ctx.causation_id.clone(), ctx.principal_id.clone(),
            ),
            anchor_domain_id: id.to_string(),
            domain: domain.to_string(),
        }
    }
}

// ── ClientAuthConfig Events ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthConfigCreated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub auth_config_id: String,
    pub email_domain: String,
    pub config_type: String,
}

impl_domain_event!(AuthConfigCreated);

impl AuthConfigCreated {
    const EVENT_TYPE: &'static str = "platform:iam:auth-config:created";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:iam";

    pub fn new(ctx: &ExecutionContext, id: &str, email_domain: &str, config_type: &str) -> Self {
        let event_id = TsidGenerator::generate(EntityType::Event);
        let subject = format!("platform.authconfig.{}", id);
        let message_group = format!("platform:authconfig:{}", id);

        Self {
            metadata: EventMetadata::new(
                event_id, Self::EVENT_TYPE, Self::SPEC_VERSION, Self::SOURCE,
                subject, message_group,
                ctx.execution_id.clone(), ctx.correlation_id.clone(),
                ctx.causation_id.clone(), ctx.principal_id.clone(),
            ),
            auth_config_id: id.to_string(),
            email_domain: email_domain.to_string(),
            config_type: config_type.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthConfigUpdated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub auth_config_id: String,
    pub email_domain: String,
}

impl_domain_event!(AuthConfigUpdated);

impl AuthConfigUpdated {
    const EVENT_TYPE: &'static str = "platform:iam:auth-config:updated";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:iam";

    pub fn new(ctx: &ExecutionContext, id: &str, email_domain: &str) -> Self {
        let event_id = TsidGenerator::generate(EntityType::Event);
        let subject = format!("platform.authconfig.{}", id);
        let message_group = format!("platform:authconfig:{}", id);

        Self {
            metadata: EventMetadata::new(
                event_id, Self::EVENT_TYPE, Self::SPEC_VERSION, Self::SOURCE,
                subject, message_group,
                ctx.execution_id.clone(), ctx.correlation_id.clone(),
                ctx.causation_id.clone(), ctx.principal_id.clone(),
            ),
            auth_config_id: id.to_string(),
            email_domain: email_domain.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthConfigDeleted {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub auth_config_id: String,
    pub email_domain: String,
}

impl_domain_event!(AuthConfigDeleted);

impl AuthConfigDeleted {
    const EVENT_TYPE: &'static str = "platform:iam:auth-config:deleted";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:iam";

    pub fn new(ctx: &ExecutionContext, id: &str, email_domain: &str) -> Self {
        let event_id = TsidGenerator::generate(EntityType::Event);
        let subject = format!("platform.authconfig.{}", id);
        let message_group = format!("platform:authconfig:{}", id);

        Self {
            metadata: EventMetadata::new(
                event_id, Self::EVENT_TYPE, Self::SPEC_VERSION, Self::SOURCE,
                subject, message_group,
                ctx.execution_id.clone(), ctx.correlation_id.clone(),
                ctx.causation_id.clone(), ctx.principal_id.clone(),
            ),
            auth_config_id: id.to_string(),
            email_domain: email_domain.to_string(),
        }
    }
}
