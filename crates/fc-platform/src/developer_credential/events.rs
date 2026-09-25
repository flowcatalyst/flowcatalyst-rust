//! Developer-credential events, as Go's (principal/operations/events.go:
//! `platform:iam:user:developer-credential-set` / `-revoked`, source
//! `platform:iam`, subject `platform.principal.{id}`, group
//! `platform:principal:{id}`, data `{userId}`). Never the secret.

use serde::{Deserialize, Serialize};

use crate::impl_domain_event;
use crate::usecase::{EventMetadata, ExecutionContext};

const SOURCE: &str = "platform:iam";
const SPEC_VERSION: &str = "1.0";

fn metadata(ctx: &ExecutionContext, event_type: &str, user_id: &str) -> EventMetadata {
    EventMetadata::from_ctx(
        ctx,
        event_type,
        SPEC_VERSION,
        SOURCE,
        format!("platform.principal.{user_id}"),
        format!("platform:principal:{user_id}"),
    )
}

/// A user's developer secret was created or rotated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeveloperCredentialSet {
    #[serde(flatten)]
    pub metadata: EventMetadata,
    pub user_id: String,
}

impl_domain_event!(DeveloperCredentialSet);

impl DeveloperCredentialSet {
    pub const EVENT_TYPE: &'static str = "platform:iam:user:developer-credential-set";

    pub fn new(ctx: &ExecutionContext, user_id: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, user_id),
            user_id: user_id.to_string(),
        }
    }
}

/// A user's developer secret was cleared (the role is untouched).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeveloperCredentialRevoked {
    #[serde(flatten)]
    pub metadata: EventMetadata,
    pub user_id: String,
}

impl_domain_event!(DeveloperCredentialRevoked);

impl DeveloperCredentialRevoked {
    pub const EVENT_TYPE: &'static str = "platform:iam:user:developer-credential-revoked";

    pub fn new(ctx: &ExecutionContext, user_id: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, user_id),
            user_id: user_id.to_string(),
        }
    }
}
