//! Identity Provider Domain Events
//!
//! Type, source, subject, message group and `data` are Go's
//! (`internal/platform/identityprovider/operations/events.go`): type
//! `platform:admin:identity-provider:*`, source `platform:admin`, subject
//! `platform.identityprovider.{id}`, group `platform:identityprovider:{id}`,
//! and each payload is Go's `{identityProviderId, code}`. No payload carries
//! the client secret or its reference.

use crate::impl_domain_event;
use crate::usecase::domain_event::EventMetadata;
use crate::usecase::ExecutionContext;
use serde::{Deserialize, Serialize};

const SPEC_VERSION: &str = "1.0";
const SOURCE: &str = "platform:admin";

fn metadata(ctx: &ExecutionContext, event_type: &str, idp_id: &str) -> EventMetadata {
    EventMetadata::from_ctx(
        ctx,
        event_type,
        SPEC_VERSION,
        SOURCE,
        format!("platform.identityprovider.{}", idp_id),
        format!("platform:identityprovider:{}", idp_id),
    )
}

macro_rules! idp_event {
    ($(#[$doc:meta])* $name:ident, $event_type:literal) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub struct $name {
            #[serde(skip)]
            pub metadata: EventMetadata,
            pub identity_provider_id: String,
            pub code: String,
        }

        impl_domain_event!($name);

        impl $name {
            pub const EVENT_TYPE: &'static str = $event_type;

            pub fn new(ctx: &ExecutionContext, idp_id: &str, code: &str) -> Self {
                Self {
                    metadata: metadata(ctx, Self::EVENT_TYPE, idp_id),
                    identity_provider_id: idp_id.to_string(),
                    code: code.to_string(),
                }
            }
        }
    };
}

idp_event!(
    /// `{identityProviderId, code}`.
    IdentityProviderCreated,
    "platform:admin:identity-provider:created"
);
idp_event!(
    /// `{identityProviderId, code}`.
    IdentityProviderUpdated,
    "platform:admin:identity-provider:updated"
);
idp_event!(
    /// `{identityProviderId, code}`.
    IdentityProviderDeleted,
    "platform:admin:identity-provider:deleted"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_provider_events_are_go_shaped() {
        let ctx = ExecutionContext::create("prn_1");
        let e = IdentityProviderCreated::new(&ctx, "idp_1", "okta");
        assert_eq!(
            e.metadata.event_type,
            "platform:admin:identity-provider:created"
        );
        assert_eq!(e.metadata.subject, "platform.identityprovider.idp_1");
        assert_eq!(e.metadata.message_group, "platform:identityprovider:idp_1");
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            serde_json::json!({"identityProviderId": "idp_1", "code": "okta"})
        );
    }
}
