//! CORS Domain Events

use serde::{Deserialize, Serialize};
use crate::usecase::ExecutionContext;
use crate::usecase::domain_event::EventMetadata;
use crate::TsidGenerator;
use crate::EntityType;
use crate::impl_domain_event;

/// Event emitted when a CORS origin is added.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CorsOriginAdded {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub origin_id: String,
    pub origin: String,
}

impl_domain_event!(CorsOriginAdded);

impl CorsOriginAdded {
    const EVENT_TYPE: &'static str = "platform:admin:cors:origin-added";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:admin";

    pub fn new(ctx: &ExecutionContext, origin_id: &str, origin: &str) -> Self {
        let event_id = TsidGenerator::generate(EntityType::Event);
        let subject = format!("platform.cors.{}", origin_id);
        let message_group = format!("platform:cors:{}", origin_id);

        Self {
            metadata: EventMetadata::new(
                event_id, Self::EVENT_TYPE, Self::SPEC_VERSION, Self::SOURCE,
                subject, message_group,
                ctx.execution_id.clone(), ctx.correlation_id.clone(),
                ctx.causation_id.clone(), ctx.principal_id.clone(),
            ),
            origin_id: origin_id.to_string(),
            origin: origin.to_string(),
        }
    }
}

/// Event emitted when a CORS origin is deleted.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CorsOriginDeleted {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub origin_id: String,
    pub origin: String,
}

impl_domain_event!(CorsOriginDeleted);

impl CorsOriginDeleted {
    const EVENT_TYPE: &'static str = "platform:admin:cors:origin-deleted";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:admin";

    pub fn new(ctx: &ExecutionContext, origin_id: &str, origin: &str) -> Self {
        let event_id = TsidGenerator::generate(EntityType::Event);
        let subject = format!("platform.cors.{}", origin_id);
        let message_group = format!("platform:cors:{}", origin_id);

        Self {
            metadata: EventMetadata::new(
                event_id, Self::EVENT_TYPE, Self::SPEC_VERSION, Self::SOURCE,
                subject, message_group,
                ctx.execution_id.clone(), ctx.correlation_id.clone(),
                ctx.causation_id.clone(), ctx.principal_id.clone(),
            ),
            origin_id: origin_id.to_string(),
            origin: origin.to_string(),
        }
    }
}
