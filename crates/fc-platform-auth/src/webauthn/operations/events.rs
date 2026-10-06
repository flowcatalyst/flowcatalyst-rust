//! WebAuthn Domain Events — passkey registration, revocation, authentication.
//!
//! Type, source, subject, message group and `data` are Go's
//! (`internal/platform/webauthn/operations/events.go`): type
//! `platform:admin:passkey:*`, source `platform:admin`, subject
//! `platform.passkey.{credentialId}`, group `platform:passkey:{credentialId}`,
//! and each payload carries exactly Go's `ToDataJSON` fields (the owner is
//! `userId`).

use fc_platform_core::impl_domain_event;
use fc_platform_core::shared::id::PrincipalId;
use fc_platform_core::shared::id::WebauthnCredentialId;
use fc_platform_core::usecase::domain_event::EventMetadata;
use fc_platform_core::usecase::ExecutionContext;
use serde::{Deserialize, Serialize};

const SPEC_VERSION: &str = "1.0";
const SOURCE: &str = "platform:admin";

fn metadata_for(
    ctx: &ExecutionContext,
    event_type: &str,
    credential_id: &WebauthnCredentialId,
) -> EventMetadata {
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
    pub credential_id: WebauthnCredentialId,
    pub user_id: PrincipalId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl_domain_event!(PasskeyRegistered);

impl PasskeyRegistered {
    pub const EVENT_TYPE: &'static str = "platform:admin:passkey:registered";

    pub fn new(
        ctx: &ExecutionContext,
        credential_id: &WebauthnCredentialId,
        user_id: &PrincipalId,
        name: Option<String>,
    ) -> Self {
        Self {
            metadata: metadata_for(ctx, Self::EVENT_TYPE, credential_id),
            credential_id: credential_id.clone(),
            user_id: user_id.clone(),
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
    pub credential_id: WebauthnCredentialId,
    pub user_id: PrincipalId,
}

impl_domain_event!(PasskeyRevoked);

impl PasskeyRevoked {
    pub const EVENT_TYPE: &'static str = "platform:admin:passkey:revoked";

    pub fn new(
        ctx: &ExecutionContext,
        credential_id: &WebauthnCredentialId,
        user_id: &PrincipalId,
    ) -> Self {
        Self {
            metadata: metadata_for(ctx, Self::EVENT_TYPE, credential_id),
            credential_id: credential_id.clone(),
            user_id: user_id.clone(),
        }
    }
}

/// A user signed in with a passkey: `{credentialId, userId}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasskeyAuthenticated {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub credential_id: WebauthnCredentialId,
    pub user_id: PrincipalId,
}

impl_domain_event!(PasskeyAuthenticated);

impl PasskeyAuthenticated {
    pub const EVENT_TYPE: &'static str = "platform:admin:passkey:authenticated";

    pub fn new(
        ctx: &ExecutionContext,
        credential_id: &WebauthnCredentialId,
        user_id: &PrincipalId,
    ) -> Self {
        Self {
            metadata: metadata_for(ctx, Self::EVENT_TYPE, credential_id),
            credential_id: credential_id.clone(),
            user_id: user_id.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ExecutionContext {
        ExecutionContext::system("prn_TESTPRINCIPAL")
    }

    #[test]
    fn registered_event_is_go_shaped() {
        let event = PasskeyRegistered::new(
            &ctx(),
            &WebauthnCredentialId::parse("pkc_AAA").unwrap(),
            &PrincipalId::parse("prn_BBB").unwrap(),
            Some("MacBook".into()),
        );
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
        let event = PasskeyRevoked::new(
            &ctx(),
            &WebauthnCredentialId::parse("pkc_AAA").unwrap(),
            &PrincipalId::parse("prn_BBB").unwrap(),
        );
        assert_eq!(event.metadata.event_type, "platform:admin:passkey:revoked");
        let event = PasskeyAuthenticated::new(
            &ctx(),
            &WebauthnCredentialId::parse("pkc_AAA").unwrap(),
            &PrincipalId::parse("prn_BBB").unwrap(),
        );
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
        let r = PasskeyRegistered::new(
            &ctx(),
            &WebauthnCredentialId::parse("pkc_X").unwrap(),
            &PrincipalId::parse("prn_X").unwrap(),
            None,
        );
        let v = PasskeyRevoked::new(
            &ctx(),
            &WebauthnCredentialId::parse("pkc_X").unwrap(),
            &PrincipalId::parse("prn_X").unwrap(),
        );
        let l = PasskeyAuthenticated::new(
            &ctx(),
            &WebauthnCredentialId::parse("pkc_X").unwrap(),
            &PrincipalId::parse("prn_X").unwrap(),
        );
        assert_eq!(r.metadata.message_group, v.metadata.message_group);
        assert_eq!(v.metadata.message_group, l.metadata.message_group);
    }
}
