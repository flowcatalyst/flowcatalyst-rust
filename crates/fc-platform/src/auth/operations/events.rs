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

macro_rules! oauth_client_event {
    ($name:ident, $event_type:expr) => {
        #[derive(Debug, Clone, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub struct $name {
            #[serde(flatten)]
            pub metadata: EventMetadata,

            pub oauth_client_id: String,
            pub client_id: String,
        }

        impl_domain_event!($name);

        impl $name {
            const EVENT_TYPE: &'static str = $event_type;
            const SPEC_VERSION: &'static str = "1.0";
            const SOURCE: &'static str = "platform:iam";

            pub fn new(ctx: &ExecutionContext, id: &str, client_id: &str) -> Self {
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
                    client_id: client_id.to_string(),
                }
            }
        }
    };
}

oauth_client_event!(OAuthClientCreated, "platform:iam:oauth-client:created");
oauth_client_event!(OAuthClientUpdated, "platform:iam:oauth-client:updated");
oauth_client_event!(OAuthClientDeleted, "platform:iam:oauth-client:deleted");
oauth_client_event!(OAuthClientActivated, "platform:iam:oauth-client:activated");
oauth_client_event!(
    OAuthClientDeactivated,
    "platform:iam:oauth-client:deactivated"
);
oauth_client_event!(
    OAuthClientPreviousSecretRevoked,
    "platform:iam:oauth-client:previous-secret-revoked"
);

/// A client's secret was rotated. `previous_secret_expires_at` is when the
/// superseded secret stops being accepted; absent when the rotation was an
/// immediate cutover (Go's `OAuthClientSecretRotated`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthClientSecretRotated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub oauth_client_id: String,
    pub client_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_secret_expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl_domain_event!(OAuthClientSecretRotated);

impl OAuthClientSecretRotated {
    const EVENT_TYPE: &'static str = "platform:iam:oauth-client:secret-rotated";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:iam";

    pub fn new(ctx: &ExecutionContext, id: &str, client_id: &str) -> Self {
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
            client_id: client_id.to_string(),
            previous_secret_expires_at: None,
        }
    }
}
