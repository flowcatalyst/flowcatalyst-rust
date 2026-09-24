//! WebAuthn Domain Events — passkey registration, revocation, authentication.

use crate::impl_domain_event;
use crate::usecase::domain_event::EventMetadata;
use crate::usecase::ExecutionContext;
use serde::{Deserialize, Serialize};

const EVENT_SOURCE: &str = "platform:iam";

fn metadata_for(
    ctx: &ExecutionContext,
    event_type: &'static str,
    spec_version: &'static str,
    credential_id: &str,
) -> EventMetadata {
    EventMetadata::from_ctx(
        ctx,
        event_type,
        spec_version,
        EVENT_SOURCE,
        format!("platform.webauthncredential.{}", credential_id),
        format!("platform:webauthncredential:{}", credential_id),
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasskeyRegistered {
    #[serde(flatten)]
    pub metadata: EventMetadata,
    pub credential_id: String,
    pub principal_id: String,
    pub name: Option<String>,
}

impl_domain_event!(PasskeyRegistered);

impl PasskeyRegistered {
    const EVENT_TYPE: &'static str = "platform:iam:passkey:registered";
    const SPEC_VERSION: &'static str = "1.0";

    pub fn new(
        ctx: &ExecutionContext,
        credential_id: &str,
        principal_id: &str,
        name: Option<String>,
    ) -> Self {
        Self {
            metadata: metadata_for(ctx, Self::EVENT_TYPE, Self::SPEC_VERSION, credential_id),
            credential_id: credential_id.to_string(),
            principal_id: principal_id.to_string(),
            name,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasskeyRevoked {
    #[serde(flatten)]
    pub metadata: EventMetadata,
    pub credential_id: String,
    pub principal_id: String,
}

impl_domain_event!(PasskeyRevoked);

impl PasskeyRevoked {
    const EVENT_TYPE: &'static str = "platform:iam:passkey:revoked";
    const SPEC_VERSION: &'static str = "1.0";

    pub fn new(ctx: &ExecutionContext, credential_id: &str, principal_id: &str) -> Self {
        Self {
            metadata: metadata_for(ctx, Self::EVENT_TYPE, Self::SPEC_VERSION, credential_id),
            credential_id: credential_id.to_string(),
            principal_id: principal_id.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserLoggedInWithPasskey {
    #[serde(flatten)]
    pub metadata: EventMetadata,
    pub credential_id: String,
    pub principal_id: String,
}

impl_domain_event!(UserLoggedInWithPasskey);

impl UserLoggedInWithPasskey {
    const EVENT_TYPE: &'static str = "platform:iam:user:logged-in-with-passkey";
    const SPEC_VERSION: &'static str = "1.0";

    pub fn new(ctx: &ExecutionContext, credential_id: &str, principal_id: &str) -> Self {
        Self {
            metadata: metadata_for(ctx, Self::EVENT_TYPE, Self::SPEC_VERSION, credential_id),
            credential_id: credential_id.to_string(),
            principal_id: principal_id.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usecase::DomainEvent;

    fn ctx() -> ExecutionContext {
        ExecutionContext::create("prn_TESTPRINCIPAL")
    }

    #[test]
    fn registered_event_metadata_is_well_formed() {
        let event = PasskeyRegistered::new(&ctx(), "pkc_AAA", "prn_BBB", Some("MacBook".into()));
        assert_eq!(event.event_type(), "platform:iam:passkey:registered");
        assert_eq!(event.spec_version(), "1.0");
        assert_eq!(event.subject(), "platform.webauthncredential.pkc_AAA");
        assert_eq!(event.message_group(), "platform:webauthncredential:pkc_AAA");
        assert_eq!(event.credential_id, "pkc_AAA");
        assert_eq!(event.principal_id, "prn_BBB");
        assert_eq!(event.name.as_deref(), Some("MacBook"));
    }

    #[test]
    fn revoked_event_metadata_is_well_formed() {
        let event = PasskeyRevoked::new(&ctx(), "pkc_AAA", "prn_BBB");
        assert_eq!(event.event_type(), "platform:iam:passkey:revoked");
        assert_eq!(event.subject(), "platform.webauthncredential.pkc_AAA");
    }

    #[test]
    fn login_event_metadata_is_well_formed() {
        let event = UserLoggedInWithPasskey::new(&ctx(), "pkc_AAA", "prn_BBB");
        assert_eq!(
            event.event_type(),
            "platform:iam:user:logged-in-with-passkey"
        );
    }

    #[test]
    fn events_share_message_group_for_same_credential() {
        let r = PasskeyRegistered::new(&ctx(), "pkc_X", "prn_X", None);
        let v = PasskeyRevoked::new(&ctx(), "pkc_X", "prn_X");
        let l = UserLoggedInWithPasskey::new(&ctx(), "pkc_X", "prn_X");
        assert_eq!(r.message_group(), v.message_group());
        assert_eq!(v.message_group(), l.message_group());
    }
}
