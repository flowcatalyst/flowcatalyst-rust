//! CORS Domain Events

use fc_platform_core::impl_domain_event;
use fc_platform_core::shared::id::CorsOriginId;
use fc_platform_core::usecase::domain_event::EventMetadata;
use fc_platform_core::usecase::ExecutionContext;
use serde::{Deserialize, Serialize};

/// Event emitted when a CORS origin is added.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CorsOriginAdded {
    #[serde(skip)]
    pub metadata: EventMetadata,

    pub origin_id: CorsOriginId,
    pub origin: String,
}

impl_domain_event!(CorsOriginAdded);

impl CorsOriginAdded {
    const EVENT_TYPE: &'static str = "platform:admin:cors:origin-added";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:admin";

    pub fn new(ctx: &ExecutionContext, origin_id: &CorsOriginId, origin: &str) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.cors.{}", origin_id),
                format!("platform:cors:{}", origin_id),
            ),
            origin_id: origin_id.clone(),
            origin: origin.to_string(),
        }
    }
}

/// Event emitted when a CORS origin is deleted.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CorsOriginDeleted {
    #[serde(skip)]
    pub metadata: EventMetadata,

    pub origin_id: CorsOriginId,
    pub origin: String,
}

impl_domain_event!(CorsOriginDeleted);

impl CorsOriginDeleted {
    const EVENT_TYPE: &'static str = "platform:admin:cors:origin-deleted";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:admin";

    pub fn new(ctx: &ExecutionContext, origin_id: &CorsOriginId, origin: &str) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.cors.{}", origin_id),
                format!("platform:cors:{}", origin_id),
            ),
            origin_id: origin_id.clone(),
            origin: origin.to_string(),
        }
    }
}
