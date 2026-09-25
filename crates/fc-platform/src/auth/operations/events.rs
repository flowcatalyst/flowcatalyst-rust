//! Auth Domain Events — AnchorDomain, ClientAuthConfig, IdpRoleMapping and
//! OAuthClient.
//!
//! Type, source, subject, message group and `data` are Go's
//! (`internal/platform/auth/operations/events.go`): every event here has
//! source `platform:admin` and type `platform:admin:{aggregate}:{action}`,
//! and each payload carries exactly Go's `ToDataJSON` fields.

use crate::impl_domain_event;
use crate::usecase::domain_event::EventMetadata;
use crate::usecase::ExecutionContext;
use serde::{Deserialize, Serialize};

const SPEC_VERSION: &str = "1.0";
const SOURCE: &str = "platform:admin";

/// Metadata on subject `platform.{aggregate}.{id}`, group
/// `platform:{aggregate}:{id}`.
fn metadata(ctx: &ExecutionContext, event_type: &str, aggregate: &str, id: &str) -> EventMetadata {
    EventMetadata::from_ctx(
        ctx,
        event_type,
        SPEC_VERSION,
        SOURCE,
        format!("platform.{}.{}", aggregate, id),
        format!("platform:{}:{}", aggregate, id),
    )
}

// ── AnchorDomain Events ──────────────────────────────────────────────────────

macro_rules! anchor_domain_event {
    ($(#[$doc:meta])* $name:ident, $event_type:literal) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub struct $name {
            #[serde(skip)]
            pub metadata: EventMetadata,
            pub anchor_domain_id: String,
            pub domain: String,
        }

        impl_domain_event!($name);

        impl $name {
            pub const EVENT_TYPE: &'static str = $event_type;

            pub fn new(ctx: &ExecutionContext, id: &str, domain: &str) -> Self {
                Self {
                    metadata: metadata(ctx, Self::EVENT_TYPE, "anchordomain", id),
                    anchor_domain_id: id.to_string(),
                    domain: domain.to_string(),
                }
            }
        }
    };
}

anchor_domain_event!(
    /// `{anchorDomainId, domain}`.
    AnchorDomainCreated,
    "platform:admin:anchor-domain:created"
);
anchor_domain_event!(
    /// `{anchorDomainId, domain}`.
    AnchorDomainUpdated,
    "platform:admin:anchor-domain:updated"
);
anchor_domain_event!(
    /// `{anchorDomainId, domain}`.
    AnchorDomainDeleted,
    "platform:admin:anchor-domain:deleted"
);

// ── ClientAuthConfig Events ──────────────────────────────────────────────────

macro_rules! auth_config_event {
    ($(#[$doc:meta])* $name:ident, $event_type:literal) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub struct $name {
            #[serde(skip)]
            pub metadata: EventMetadata,
            pub auth_config_id: String,
            pub email_domain: String,
        }

        impl_domain_event!($name);

        impl $name {
            pub const EVENT_TYPE: &'static str = $event_type;

            pub fn new(ctx: &ExecutionContext, id: &str, email_domain: &str) -> Self {
                Self {
                    metadata: metadata(ctx, Self::EVENT_TYPE, "authconfig", id),
                    auth_config_id: id.to_string(),
                    email_domain: email_domain.to_string(),
                }
            }
        }
    };
}

auth_config_event!(
    /// `{authConfigId, emailDomain}`.
    AuthConfigCreated,
    "platform:admin:auth-config:created"
);
auth_config_event!(
    /// `{authConfigId, emailDomain}`.
    AuthConfigUpdated,
    "platform:admin:auth-config:updated"
);
auth_config_event!(
    /// `{authConfigId, emailDomain}`.
    AuthConfigDeleted,
    "platform:admin:auth-config:deleted"
);

// ── IdpRoleMapping events ────────────────────────────────────────────────────

/// `{mappingId, idpType, idpRoleName, platformRoleName}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdpRoleMappingCreated {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub mapping_id: String,
    pub idp_type: String,
    pub idp_role_name: String,
    pub platform_role_name: String,
}

impl_domain_event!(IdpRoleMappingCreated);

impl IdpRoleMappingCreated {
    pub const EVENT_TYPE: &'static str = "platform:admin:idp-role-mapping:created";

    pub fn new(
        ctx: &ExecutionContext,
        id: &str,
        idp_type: &str,
        idp_role_name: &str,
        platform_role_name: &str,
    ) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, "idprolemapping", id),
            mapping_id: id.to_string(),
            idp_type: idp_type.to_string(),
            idp_role_name: idp_role_name.to_string(),
            platform_role_name: platform_role_name.to_string(),
        }
    }
}

/// `{mappingId, idpRoleName}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdpRoleMappingDeleted {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub mapping_id: String,
    pub idp_role_name: String,
}

impl_domain_event!(IdpRoleMappingDeleted);

impl IdpRoleMappingDeleted {
    pub const EVENT_TYPE: &'static str = "platform:admin:idp-role-mapping:deleted";

    pub fn new(ctx: &ExecutionContext, id: &str, idp_role_name: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, "idprolemapping", id),
            mapping_id: id.to_string(),
            idp_role_name: idp_role_name.to_string(),
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
            #[serde(skip)]
            pub metadata: EventMetadata,

            pub oauth_client_id: String,
            $(pub $field: String,)*
        }

        impl_domain_event!($name);

        impl $name {
            pub const EVENT_TYPE: &'static str = $event_type;

            pub fn new(ctx: &ExecutionContext, id: &str $(, $field: &str)*) -> Self {
                Self {
                    metadata: metadata(ctx, Self::EVENT_TYPE, "oauthclient", id),
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
    #[serde(skip)]
    pub metadata: EventMetadata,

    pub oauth_client_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_secret_expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl_domain_event!(OAuthClientSecretRotated);

impl OAuthClientSecretRotated {
    pub const EVENT_TYPE: &'static str = "platform:admin:oauth-client:secret-rotated";

    pub fn new(ctx: &ExecutionContext, id: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, "oauthclient", id),
            oauth_client_id: id.to_string(),
            previous_secret_expires_at: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The keys of an event's `data` payload.
    fn payload_keys<T: Serialize>(event: &T) -> Vec<String> {
        let json = serde_json::to_value(event).unwrap();
        let mut keys: Vec<String> = json.as_object().unwrap().keys().cloned().collect();
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

    #[test]
    fn anchor_auth_config_and_mapping_events_are_go_shaped() {
        let ctx = ExecutionContext::create("prn_actor");
        let a = AnchorDomainCreated::new(&ctx, "anc_1", "acme.com");
        assert_eq!(
            a.metadata.event_type,
            "platform:admin:anchor-domain:created"
        );
        assert_eq!(a.metadata.subject, "platform.anchordomain.anc_1");
        assert_eq!(payload_keys(&a), ["anchorDomainId", "domain"]);
        let c = AuthConfigDeleted::new(&ctx, "cac_1", "acme.com");
        assert_eq!(c.metadata.event_type, "platform:admin:auth-config:deleted");
        assert_eq!(payload_keys(&c), ["authConfigId", "emailDomain"]);
        let m = IdpRoleMappingCreated::new(&ctx, "irm_1", "OIDC", "admins", "platform:admin");
        assert_eq!(
            payload_keys(&m),
            ["idpRoleName", "idpType", "mappingId", "platformRoleName"]
        );
        let d = IdpRoleMappingDeleted::new(&ctx, "irm_1", "admins");
        assert_eq!(payload_keys(&d), ["idpRoleName", "mappingId"]);
    }
}
