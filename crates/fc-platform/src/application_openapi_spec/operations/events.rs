//! OpenAPI spec domain events.

use serde::{Deserialize, Serialize};

use crate::impl_domain_event;
use crate::usecase::domain_event::EventMetadata;
use crate::usecase::ExecutionContext;

/// Emitted when an application syncs a new OpenAPI document, whether the
/// content was new (versionDelta=true) or byte-identical to the prior CURRENT
/// (versionDelta=false). The audit log keeps both cases for completeness.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationOpenApiSpecSynced {
    #[serde(skip)]
    pub metadata: EventMetadata,

    pub application_id: String,
    pub application_code: String,
    pub spec_id: String,
    pub version: String,
    pub spec_hash: String,
    /// Some when a prior CURRENT was archived in this sync.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived_prior_version: Option<String>,
    /// True if the diff includes removed paths/schemas/verbs.
    pub has_breaking: bool,
    /// True if the incoming spec was byte-identical to the existing CURRENT
    /// (no new row was inserted).
    pub unchanged: bool,
}

impl_domain_event!(ApplicationOpenApiSpecSynced);

impl ApplicationOpenApiSpecSynced {
    const EVENT_TYPE: &'static str = "platform:developer:application-openapi:synced";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:developer";

    /// Metadata for this event, raised inside `ctx`.
    pub fn metadata_for(
        ctx: &ExecutionContext,
        application_id: &str,
        spec_id: &str,
    ) -> EventMetadata {
        EventMetadata::from_ctx(
            ctx,
            Self::EVENT_TYPE,
            Self::SPEC_VERSION,
            Self::SOURCE,
            format!("platform.application-openapi.{}", spec_id),
            format!("platform:application-openapi:{}", application_id),
        )
    }
}
