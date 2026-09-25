//! Auth Domain Events — AnchorDomain and ClientAuthConfig

use crate::impl_domain_event;
use crate::usecase::domain_event::EventMetadata;
use crate::usecase::ExecutionContext;
use serde::{Deserialize, Serialize};

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
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.anchordomain.{}", id),
                format!("platform:anchordomain:{}", id),
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
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.anchordomain.{}", id),
                format!("platform:anchordomain:{}", id),
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
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.authconfig.{}", id),
                format!("platform:authconfig:{}", id),
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
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.authconfig.{}", id),
                format!("platform:authconfig:{}", id),
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
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.authconfig.{}", id),
                format!("platform:authconfig:{}", id),
            ),
            auth_config_id: id.to_string(),
            email_domain: email_domain.to_string(),
        }
    }
}

// ── AnchorDomain update event ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnchorDomainUpdated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub anchor_domain_id: String,
    pub domain: String,
}

impl_domain_event!(AnchorDomainUpdated);

impl AnchorDomainUpdated {
    const EVENT_TYPE: &'static str = "platform:iam:anchor-domain:updated";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:iam";

    pub fn new(ctx: &ExecutionContext, id: &str, domain: &str) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.anchordomain.{}", id),
                format!("platform:anchordomain:{}", id),
            ),
            anchor_domain_id: id.to_string(),
            domain: domain.to_string(),
        }
    }
}

// ── IdpRoleMapping events ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdpRoleMappingCreated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub idp_role_mapping_id: String,
    pub idp_role: String,
    pub mapped_role: String,
}

impl_domain_event!(IdpRoleMappingCreated);

impl IdpRoleMappingCreated {
    const EVENT_TYPE: &'static str = "platform:iam:idp-role-mapping:created";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:iam";

    pub fn new(ctx: &ExecutionContext, id: &str, idp_role: &str, mapped_role: &str) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.idprolemapping.{}", id),
                format!("platform:idprolemapping:{}", id),
            ),
            idp_role_mapping_id: id.to_string(),
            idp_role: idp_role.to_string(),
            mapped_role: mapped_role.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdpRoleMappingDeleted {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub idp_role_mapping_id: String,
}

impl_domain_event!(IdpRoleMappingDeleted);

impl IdpRoleMappingDeleted {
    const EVENT_TYPE: &'static str = "platform:iam:idp-role-mapping:deleted";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:iam";

    pub fn new(ctx: &ExecutionContext, id: &str) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.idprolemapping.{}", id),
                format!("platform:idprolemapping:{}", id),
            ),
            idp_role_mapping_id: id.to_string(),
        }
    }
}

// ── OAuthClient events ───────────────────────────────────────────────────────
//
// Type, source, subject, message group and payload are Go's
// (flowcatalyst-go auth/operations/events.go:14-41 and the ToDataJSON of
// each event): source `platform:admin`, subject `platform.oauthclient.{id}`,
// group `platform:oauthclient:{id}`, and each payload carries exactly Go's
// fields, since subscribers consume them.

macro_rules! oauth_client_event {
    ($(#[$doc:meta])* $name:ident, $event_type:expr $(, $field:ident)*) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub struct $name {
            #[serde(flatten)]
            pub metadata: EventMetadata,

            pub oauth_client_id: String,
            $(pub $field: String,)*
        }

        impl_domain_event!($name);

        impl $name {
            const EVENT_TYPE: &'static str = $event_type;
            const SPEC_VERSION: &'static str = "1.0";
            const SOURCE: &'static str = "platform:admin";

            pub fn new(ctx: &ExecutionContext, id: &str $(, $field: &str)*) -> Self {
                Self {
                    metadata: EventMetadata::from_ctx(
                        ctx,
                        Self::EVENT_TYPE,
                        Self::SPEC_VERSION,
                        Self::SOURCE,
                        format!("platform.oauthclient.{}", id),
                        format!("platform:oauthclient:{}", id),
                    ),
                    oauth_client_id: id.to_string(),
                    $($field: $field.to_string(),)*
                }
            }
        }
    };
}

oauth_client_event!(
    /// `{oauthClientId, clientId, clientName}` (Go events.go:81-87).
    OAuthClientCreated,
    "platform:admin:oauth-client:created",
    client_id,
    client_name
);
oauth_client_event!(
    /// `{oauthClientId, clientName}` (Go events.go:106-111).
    OAuthClientUpdated,
    "platform:admin:oauth-client:updated",
    client_name
);
oauth_client_event!(
    /// `{oauthClientId, clientId}` (Go events.go:174-179).
    OAuthClientDeleted,
    "platform:admin:oauth-client:deleted",
    client_id
);
oauth_client_event!(
    /// `{oauthClientId}` (Go events.go:129-133).
    OAuthClientActivated,
    "platform:admin:oauth-client:activated"
);
oauth_client_event!(
    /// `{oauthClientId}` (Go events.go:151-155).
    OAuthClientDeactivated,
    "platform:admin:oauth-client:deactivated"
);
oauth_client_event!(
    /// `{oauthClientId}` (Go events.go:232-236).
    OAuthClientPreviousSecretRevoked,
    "platform:admin:oauth-client:previous-secret-revoked"
);

/// A client's secret was rotated: `{oauthClientId, previousSecretExpiresAt?}`
/// (Go events.go:201-206). `previous_secret_expires_at` is when the
/// superseded secret stops being accepted; absent when the rotation was an
/// immediate cutover.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthClientSecretRotated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub oauth_client_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_secret_expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl_domain_event!(OAuthClientSecretRotated);

impl OAuthClientSecretRotated {
    const EVENT_TYPE: &'static str = "platform:admin:oauth-client:secret-rotated";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:admin";

    pub fn new(ctx: &ExecutionContext, id: &str) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.oauthclient.{}", id),
                format!("platform:oauthclient:{}", id),
            ),
            oauth_client_id: id.to_string(),
            previous_secret_expires_at: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The payload keys an event serialises besides its flattened metadata.
    fn payload_keys<T: Serialize>(event: &T) -> Vec<String> {
        const METADATA: &[&str] = &[
            "event_id",
            "event_type",
            "spec_version",
            "source",
            "subject",
            "time",
            "execution_id",
            "correlation_id",
            "causation_id",
            "principal_id",
            "message_group",
        ];
        let json = serde_json::to_value(event).unwrap();
        let mut keys: Vec<String> = json
            .as_object()
            .unwrap()
            .keys()
            .filter(|k| !METADATA.contains(&k.as_str()))
            .cloned()
            .collect();
        keys.sort();
        keys
    }

    /// Payloads are Go's, key for key (auth/operations/events.go ToDataJSON).
    #[test]
    fn oauth_client_payloads_match_go() {
        let ctx = ExecutionContext::create("prn_actor");
        assert_eq!(
            payload_keys(&OAuthClientCreated::new(&ctx, "oac_1", "cid", "Name")),
            ["clientId", "clientName", "oauthClientId"]
        );
        assert_eq!(
            payload_keys(&OAuthClientUpdated::new(&ctx, "oac_1", "Name")),
            ["clientName", "oauthClientId"]
        );
        assert_eq!(
            payload_keys(&OAuthClientDeleted::new(&ctx, "oac_1", "cid")),
            ["clientId", "oauthClientId"]
        );
        for keys in [
            payload_keys(&OAuthClientActivated::new(&ctx, "oac_1")),
            payload_keys(&OAuthClientDeactivated::new(&ctx, "oac_1")),
            payload_keys(&OAuthClientPreviousSecretRevoked::new(&ctx, "oac_1")),
            payload_keys(&OAuthClientSecretRotated::new(&ctx, "oac_1")),
        ] {
            assert_eq!(keys, ["oauthClientId"]);
        }
        let mut rotated = OAuthClientSecretRotated::new(&ctx, "oac_1");
        rotated.previous_secret_expires_at = Some(chrono::Utc::now());
        assert_eq!(
            payload_keys(&rotated),
            ["oauthClientId", "previousSecretExpiresAt"]
        );

        let created = OAuthClientCreated::new(&ctx, "oac_1", "cid", "Name");
        assert_eq!(created.metadata.source, "platform:admin");
        assert_eq!(created.metadata.subject, "platform.oauthclient.oac_1");
        assert_eq!(created.metadata.message_group, "platform:oauthclient:oac_1");
    }
}
