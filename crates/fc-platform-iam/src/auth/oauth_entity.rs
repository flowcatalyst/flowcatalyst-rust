//! OAuth Client Entity
//!
//! Represents OAuth 2.0 client registrations for external applications.

use chrono::{DateTime, Utc};
use fc_platform_core::shared::id::ApplicationId;
use fc_platform_core::shared::id::ClientId;
use fc_platform_core::shared::id::OAuthClientId;
use fc_platform_core::shared::id::PortalAppId;
use fc_platform_core::shared::id::PrincipalId;
use serde::{Deserialize, Serialize};

/// OAuth client type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[derive(Default)]
pub enum OAuthClientType {
    /// Public client (SPA, mobile app) - cannot keep secrets
    #[default]
    Public,
    /// Confidential client (server-side) - can keep secrets
    Confidential,
}

fc_platform_core::shared::enum_str::str_enum!(OAuthClientType, "OAuth client type", {
    Public => "PUBLIC",
    Confidential => "CONFIDENTIAL",
});

/// OAuth grant type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantType {
    AuthorizationCode,
    ClientCredentials,
    RefreshToken,
    Password,
}

fc_platform_core::shared::enum_str::str_enum!(GrantType, "grant type", {
    AuthorizationCode => "authorization_code",
    ClientCredentials => "client_credentials",
    RefreshToken => "refresh_token",
    Password => "password",
});

/// OAuth client entity
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthClient {
    /// TSID as Crockford Base32 string
    pub id: OAuthClientId,

    /// OAuth client_id (public identifier)
    pub client_id: String,

    /// Human-readable name
    pub client_name: String,

    /// Client type
    #[serde(default)]
    pub client_type: OAuthClientType,

    /// Reference to client secret (encrypted or in secret manager)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret_ref: Option<String>,

    /// The immediately-prior secret, kept acceptable until
    /// `previous_secret_expires_at` so a rotation doesn't cut off every
    /// service still holding the old value (Go's rotation overlap). Read it
    /// through [`OAuthClient::usable_previous_secret_ref`], which enforces
    /// the expiry.
    #[serde(skip)]
    pub previous_secret_ref: Option<String>,

    /// When the previous secret stops being accepted.
    #[serde(skip)]
    pub previous_secret_expires_at: Option<DateTime<Utc>>,

    /// When the previous secret was last accepted; `None` means nobody has
    /// used it since the rotation.
    #[serde(skip)]
    pub previous_secret_last_used_at: Option<DateTime<Utc>>,

    /// Allowed redirect URIs
    #[serde(default)]
    pub redirect_uris: Vec<String>,

    /// Allowed post-logout redirect URIs (OIDC RP-Initiated Logout 1.0).
    /// Validated against the same `matches_redirect_uri` matcher as
    /// `redirect_uris` — exact match or single-segment `*` wildcard.
    #[serde(default)]
    pub post_logout_redirect_uris: Vec<String>,

    /// Allowed grant types
    #[serde(default)]
    pub grant_types: Vec<GrantType>,

    /// Default scopes
    #[serde(default)]
    pub default_scopes: Vec<String>,

    /// Whether PKCE is required
    #[serde(default)]
    pub pkce_required: bool,

    /// Application IDs this client can access
    #[serde(default)]
    pub application_ids: Vec<ApplicationId>,

    /// Allowed CORS origins
    #[serde(default)]
    pub allowed_origins: Vec<String>,

    /// Service account principal ID (for client_credentials grant)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_account_principal_id: Option<PrincipalId>,

    /// Whether the client is active
    #[serde(default = "default_true")]
    pub active: bool,

    /// A trusted first-party client whose interactive logins receive an
    /// authority-bearing access token (`token_use: api`), narrowed to the
    /// client's applications; every other client's logins receive an
    /// identity-only token (Go `OAuthClient.APIAccess`, migration 042 /
    /// Rust 043). Read here; set through the database.
    #[serde(default)]
    pub api_access: bool,

    /// Audit fields
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,

    /// Portal identity plane (Go `OAuthClient.PortalClientID`): when set,
    /// this client is a portal entry point owned by that tenant client — it
    /// must enter through `/portal/authorize` and its codes carry portal
    /// identity subjects.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub portal_client_id: Option<ClientId>,

    /// The portal app this portal client fronts (Go `PortalAppID`); `None`
    /// on a portal client is a legacy client-wide portal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub portal_app_id: Option<PortalAppId>,
}

fn default_true() -> bool {
    true
}

#[bon::bon]
impl OAuthClient {
    /// A new client from its public id and name, with every other field at
    /// its default unless set: a fresh id, PUBLIC, the authorization-code
    /// grant, PKCE required, active, no secret, no redirect URIs, scopes,
    /// applications or origins, no API access.
    #[builder(
        start_fn(name = builder, vis = "pub"),
        finish_fn(name = build, vis = "pub"),
        builder_type(name = OAuthClientBuilder, vis = "pub")
    )]
    fn from_parts(
        #[builder(into)] client_id: String,
        #[builder(into)] client_name: String,
        #[builder(default = OAuthClientId::generate())] id: OAuthClientId,
        #[builder(default = OAuthClientType::Public)] client_type: OAuthClientType,
        client_secret_ref: Option<String>,
        #[builder(default)] redirect_uris: Vec<String>,
        #[builder(default)] post_logout_redirect_uris: Vec<String>,
        #[builder(default = vec![GrantType::AuthorizationCode])] grant_types: Vec<GrantType>,
        #[builder(default)] default_scopes: Vec<String>,
        #[builder(default = true)] pkce_required: bool,
        #[builder(default)] application_ids: Vec<ApplicationId>,
        #[builder(default)] allowed_origins: Vec<String>,
        service_account_principal_id: Option<PrincipalId>,
        #[builder(default)] api_access: bool,
        created_by: Option<String>,
        portal_client_id: Option<ClientId>,
        portal_app_id: Option<PortalAppId>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id,
            client_id,
            client_name,
            client_type,
            client_secret_ref,
            previous_secret_ref: None,
            previous_secret_expires_at: None,
            previous_secret_last_used_at: None,
            redirect_uris,
            post_logout_redirect_uris,
            grant_types,
            default_scopes,
            pkce_required,
            application_ids,
            allowed_origins,
            service_account_principal_id,
            active: true,
            api_access,
            created_at: now,
            updated_at: now,
            created_by,
            portal_client_id,
            portal_app_id,
        }
    }

    /// [`OAuthClient::builder`] with only the public id and name set.
    pub fn new(client_id: impl Into<String>, client_name: impl Into<String>) -> Self {
        Self::builder()
            .client_id(client_id)
            .client_name(client_name)
            .build()
    }

    pub fn confidential(client_id: impl Into<String>, client_name: impl Into<String>) -> Self {
        let mut client = Self::new(client_id, client_name);
        client.client_type = OAuthClientType::Confidential;
        client.pkce_required = false;
        client.grant_types = vec![GrantType::ClientCredentials];
        client
    }

    pub fn with_redirect_uri(mut self, uri: impl Into<String>) -> Self {
        self.redirect_uris.push(uri.into());
        self
    }

    pub fn with_grant_type(mut self, grant_type: GrantType) -> Self {
        if !self.grant_types.contains(&grant_type) {
            self.grant_types.push(grant_type);
        }
        self
    }

    pub fn with_secret_ref(mut self, secret_ref: impl Into<String>) -> Self {
        self.client_secret_ref = Some(secret_ref.into());
        self
    }

    pub fn with_service_account(mut self, principal_id: PrincipalId) -> Self {
        self.service_account_principal_id = Some(principal_id);
        self
    }

    pub fn deactivate(&mut self) {
        self.active = false;
        self.updated_at = Utc::now();
    }

    /// Install `secret_ref` with no overlap window, dropping any in-flight
    /// previous secret (Go's `SetSecretRef`, auth/entity.go:302-308): the
    /// provisioning path and the immediate cutover for a compromised secret.
    pub fn set_secret_ref(&mut self, secret_ref: impl Into<String>) {
        self.client_secret_ref = Some(secret_ref.into());
        self.previous_secret_ref = None;
        self.previous_secret_expires_at = None;
        self.previous_secret_last_used_at = None;
        self.updated_at = Utc::now();
    }

    /// Install `secret_ref` as the current secret and keep the outgoing one
    /// acceptable for `grace`, so a fleet can be rolled gradually (Go's
    /// `RotateSecretRef`, auth/entity.go:316-331). A zero grace, or no
    /// current secret to demote, is a hard cutover. Exactly one previous
    /// secret is honoured: rotating twice inside a window retires the older
    /// one at once. Returns when the outgoing secret lapses, if one was kept.
    pub fn rotate_secret_ref(
        &mut self,
        secret_ref: impl Into<String>,
        grace: chrono::Duration,
    ) -> Option<DateTime<Utc>> {
        if grace <= chrono::Duration::zero() || self.client_secret_ref.is_none() {
            self.set_secret_ref(secret_ref);
            return None;
        }
        let now = Utc::now();
        let expires = now + grace;
        self.previous_secret_ref = self.client_secret_ref.take();
        self.previous_secret_expires_at = Some(expires);
        self.previous_secret_last_used_at = None;
        self.client_secret_ref = Some(secret_ref.into());
        self.updated_at = now;
        Some(expires)
    }

    /// End an in-flight overlap now (Go's `RevokePreviousSecret`). Reports
    /// whether a previous secret was dropped.
    pub fn revoke_previous_secret(&mut self) -> bool {
        if self.previous_secret_ref.is_none() {
            return false;
        }
        self.previous_secret_ref = None;
        self.previous_secret_expires_at = None;
        self.previous_secret_last_used_at = None;
        self.updated_at = Utc::now();
        true
    }

    /// The previous secret while its overlap window is open, else `None`
    /// (Go's `UsablePreviousSecretRef`). Verification must use this: an
    /// expired ref stays in the row until the purger clears it.
    pub fn usable_previous_secret_ref(&self) -> Option<&str> {
        match (&self.previous_secret_ref, self.previous_secret_expires_at) {
            (Some(prev), Some(expires)) if Utc::now() < expires => Some(prev),
            _ => None,
        }
    }

    pub fn is_public(&self) -> bool {
        self.client_type == OAuthClientType::Public
    }

    pub fn is_confidential(&self) -> bool {
        self.client_type == OAuthClientType::Confidential
    }

    pub fn supports_grant(&self, grant: GrantType) -> bool {
        self.grant_types.contains(&grant)
    }

    pub fn is_redirect_uri_allowed(&self, uri: &str) -> bool {
        self.redirect_uris.iter().any(|allowed| {
            // Exact match or pattern match (for localhost with varying ports)
            allowed == uri || (allowed.contains("*") && uri.starts_with(&allowed.replace("*", "")))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn confidential_with(secret: &str) -> OAuthClient {
        OAuthClient::confidential("cid", "Client").with_secret_ref(secret)
    }

    /// The defaults `new` (the builder with only the id and name) gives.
    #[test]
    fn new_has_the_defaults() {
        let c = OAuthClient::new("cid", "Client");
        assert!(c.id.as_str().starts_with("oac_"), "{}", c.id);
        assert_eq!(
            (c.client_id.as_str(), c.client_name.as_str()),
            ("cid", "Client")
        );
        assert_eq!(c.client_type, OAuthClientType::Public);
        assert!(c.client_secret_ref.is_none() && c.previous_secret_ref.is_none());
        assert!(c.previous_secret_expires_at.is_none());
        assert!(c.previous_secret_last_used_at.is_none());
        assert!(c.redirect_uris.is_empty() && c.post_logout_redirect_uris.is_empty());
        assert_eq!(c.grant_types, vec![GrantType::AuthorizationCode]);
        assert!(c.default_scopes.is_empty());
        assert!(c.pkce_required && c.active && !c.api_access);
        assert!(c.application_ids.is_empty() && c.allowed_origins.is_empty());
        assert!(c.service_account_principal_id.is_none() && c.created_by.is_none());
        assert!(c.portal_client_id.is_none() && c.portal_app_id.is_none());
        assert_eq!(c.created_at, c.updated_at);
    }

    #[test]
    fn the_builder_keeps_a_given_id() {
        let c = OAuthClient::builder()
            .client_id("cid")
            .client_name("Client")
            .id(OAuthClientId::parse("oac_given").unwrap())
            .build();
        assert_eq!(c.id, OAuthClientId::parse("oac_given").unwrap());
    }

    #[test]
    fn rotation_keeps_the_outgoing_secret_for_the_grace_window() {
        let mut c = confidential_with("old");
        let expires = c
            .rotate_secret_ref("new", chrono::Duration::hours(24))
            .expect("an overlap");
        assert_eq!(c.client_secret_ref.as_deref(), Some("new"));
        assert_eq!(c.usable_previous_secret_ref(), Some("old"));
        assert!(expires > Utc::now() + chrono::Duration::hours(23));
        assert!(c.previous_secret_last_used_at.is_none());

        // A second rotation inside the window retires the older secret.
        c.previous_secret_last_used_at = Some(Utc::now());
        c.rotate_secret_ref("newer", chrono::Duration::hours(1));
        assert_eq!(c.usable_previous_secret_ref(), Some("new"));
        assert!(c.previous_secret_last_used_at.is_none());
    }

    #[test]
    fn zero_grace_or_no_current_secret_is_a_hard_cutover() {
        let mut c = confidential_with("old");
        c.rotate_secret_ref("mid", chrono::Duration::hours(1));
        assert_eq!(c.rotate_secret_ref("new", chrono::Duration::zero()), None);
        assert_eq!(c.client_secret_ref.as_deref(), Some("new"));
        assert!(c.previous_secret_ref.is_none());
        assert!(c.previous_secret_expires_at.is_none());

        let mut fresh = OAuthClient::confidential("cid", "Client");
        assert_eq!(
            fresh.rotate_secret_ref("first", chrono::Duration::hours(1)),
            None
        );
        assert!(fresh.previous_secret_ref.is_none());
    }

    #[test]
    fn an_expired_previous_secret_is_not_usable() {
        let mut c = confidential_with("new");
        c.previous_secret_ref = Some("old".to_string());
        c.previous_secret_expires_at = Some(Utc::now() - chrono::Duration::seconds(1));
        assert_eq!(c.usable_previous_secret_ref(), None);
    }

    #[test]
    fn revoke_drops_the_previous_secret_and_is_idempotent() {
        let mut c = confidential_with("old");
        c.rotate_secret_ref("new", chrono::Duration::hours(1));
        assert!(c.revoke_previous_secret());
        assert_eq!(c.usable_previous_secret_ref(), None);
        assert!(c.previous_secret_expires_at.is_none());
        assert!(!c.revoke_previous_secret());
        assert_eq!(c.client_secret_ref.as_deref(), Some("new"));
    }
}
