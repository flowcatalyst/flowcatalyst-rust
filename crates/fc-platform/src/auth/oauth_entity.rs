//! OAuth Client Entity
//!
//! Represents OAuth 2.0 client registrations for external applications.

use chrono::{DateTime, Utc};
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

impl OAuthClientType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Public => "PUBLIC",
            Self::Confidential => "CONFIDENTIAL",
        }
    }
    pub fn from_str(s: &str) -> Self {
        match s {
            "CONFIDENTIAL" => Self::Confidential,
            _ => Self::Public,
        }
    }
}

/// OAuth grant type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantType {
    AuthorizationCode,
    ClientCredentials,
    RefreshToken,
    Password,
}

impl GrantType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AuthorizationCode => "authorization_code",
            Self::ClientCredentials => "client_credentials",
            Self::RefreshToken => "refresh_token",
            Self::Password => "password",
        }
    }
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "authorization_code" => Some(Self::AuthorizationCode),
            "client_credentials" => Some(Self::ClientCredentials),
            "refresh_token" => Some(Self::RefreshToken),
            "password" => Some(Self::Password),
            _ => None,
        }
    }
}

/// OAuth client entity
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthClient {
    /// TSID as Crockford Base32 string
    pub id: String,

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
    pub application_ids: Vec<String>,

    /// Allowed CORS origins
    #[serde(default)]
    pub allowed_origins: Vec<String>,

    /// Service account principal ID (for client_credentials grant)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_account_principal_id: Option<String>,

    /// Whether the client is active
    #[serde(default = "default_true")]
    pub active: bool,

    /// Audit fields
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
}

fn default_true() -> bool {
    true
}

impl OAuthClient {
    pub fn new(client_id: impl Into<String>, client_name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: crate::shared::tsid::generate(crate::EntityType::OAuthClient),
            client_id: client_id.into(),
            client_name: client_name.into(),
            client_type: OAuthClientType::Public,
            client_secret_ref: None,
            redirect_uris: vec![],
            post_logout_redirect_uris: vec![],
            grant_types: vec![GrantType::AuthorizationCode],
            default_scopes: vec![],
            pkce_required: true,
            application_ids: vec![],
            allowed_origins: vec![],
            service_account_principal_id: None,
            active: true,
            created_at: now,
            updated_at: now,
            created_by: None,
        }
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

    pub fn with_service_account(mut self, principal_id: impl Into<String>) -> Self {
        self.service_account_principal_id = Some(principal_id.into());
        self
    }

    pub fn deactivate(&mut self) {
        self.active = false;
        self.updated_at = Utc::now();
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
