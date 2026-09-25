//! WebAuthn Domain Events — passkey registration, revocation, authentication.
//!
//! Type, source, subject, message group and `data` are Go's
//! (`internal/platform/webauthn/operations/events.go`): type
//! `platform:admin:passkey:*`, source `platform:admin`, subject
//! `platform.passkey.{credentialId}`, group `platform:passkey:{credentialId}`,
//! and each payload carries exactly Go's `ToDataJSON` fields (the owner is
//! `userId`).

use crate::impl_domain_event;
use crate::usecase::domain_event::EventMetadata;
use crate::usecase::ExecutionContext;
use serde::{Deserialize, Serialize};

const SPEC_VERSION: &str = "1.0";
const SOURCE: &str = "platform:admin";

fn metadata_for(ctx: &ExecutionContext, event_type: &str, credential_id: &str) -> EventMetadata {
    EventMetadata::from_ctx(
        ctx,
        event_type,
        SPEC_VERSION,
        SOURCE,
        format!("platform.passkey.{}", credential_id),
        format!("platform:passkey:{}", credential_id),
    )
}

/// `{credentialId, userId, name?}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasskeyRegistered {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub credential_id: String,
    pub user_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl_domain_event!(PasskeyRegistered);

impl PasskeyRegistered {
    pub const EVENT_TYPE: &'static str = "platform:admin:passkey:registered";

    pub fn new(
        ctx: &ExecutionContext,
        credential_id: &str,
        user_id: &str,
        name: Option<String>,
    ) -> Self {
        Self {
            metadata: metadata_for(ctx, Self::EVENT_TYPE, credential_id),
            credential_id: credential_id.to_string(),
            user_id: user_id.to_string(),
            name,
        }
    }
}

/// `{credentialId, userId}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasskeyRevoked {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub credential_id: String,
    pub user_id: String,
}

impl_domain_event!(PasskeyRevoked);

impl PasskeyRevoked {
    pub const EVENT_TYPE: &'static str = "platform:admin:passkey:revoked";

    pub fn new(ctx: &ExecutionContext, credential_id: &str, user_id: &str) -> Self {
        Self {
            metadata: metadata_for(ctx, Self::EVENT_TYPE, credential_id),
            credential_id: credential_id.to_string(),
            user_id: user_id.to_string(),
        }
    }
}

/// A user signed in with a passkey: `{credentialId, userId}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasskeyAuthenticated {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub credential_id: String,
    pub user_id: String,
}

impl_domain_event!(PasskeyAuthenticated);

impl PasskeyAuthenticated {
    pub const EVENT_TYPE: &'static str = "platform:admin:passkey:authenticated";

    pub fn new(ctx: &ExecutionContext, credential_id: &str, user_id: &str) -> Self {
        Self {
            metadata: metadata_for(ctx, Self::EVENT_TYPE, credential_id),
            credential_id: credential_id.to_string(),
            user_id: user_id.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ExecutionContext {
        ExecutionContext::create("prn_TESTPRINCIPAL")
    }

    #[test]
    fn registered_event_is_go_shaped() {
        let event = PasskeyRegistered::new(&ctx(), "pkc_AAA", "prn_BBB", Some("MacBook".into()));
        assert_eq!(
            event.metadata.event_type,
            "platform:admin:passkey:registered"
        );
        assert_eq!(event.metadata.source, "platform:admin");
        assert_eq!(event.metadata.spec_version, "1.0");
        assert_eq!(event.metadata.subject, "platform.passkey.pkc_AAA");
        assert_eq!(event.metadata.message_group, "platform:passkey:pkc_AAA");
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            serde_json::json!({"credentialId": "pkc_AAA", "userId": "prn_BBB", "name": "MacBook"})
        );
    }

    #[test]
    fn revoked_and_authenticated_events_are_go_shaped() {
        let event = PasskeyRevoked::new(&ctx(), "pkc_AAA", "prn_BBB");
        assert_eq!(event.metadata.event_type, "platform:admin:passkey:revoked");
        let event = PasskeyAuthenticated::new(&ctx(), "pkc_AAA", "prn_BBB");
        assert_eq!(
            event.metadata.event_type,
            "platform:admin:passkey:authenticated"
        );
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            serde_json::json!({"credentialId": "pkc_AAA", "userId": "prn_BBB"})
        );
    }

    #[test]
    fn events_share_message_group_for_same_credential() {
        let r = PasskeyRegistered::new(&ctx(), "pkc_X", "prn_X", None);
        let v = PasskeyRevoked::new(&ctx(), "pkc_X", "prn_X");
        let l = PasskeyAuthenticated::new(&ctx(), "pkc_X", "prn_X");
        assert_eq!(r.metadata.message_group, v.metadata.message_group);
        assert_eq!(v.metadata.message_group, l.metadata.message_group);
    }
}
