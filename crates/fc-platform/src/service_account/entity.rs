//! Service Account Entity
//!
//! Machine-to-machine authentication for webhooks.

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

/// Webhook authentication type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WebhookAuthType {
    /// No authentication
    None,
    /// Bearer token in Authorization header
    BearerToken,
    /// Basic authentication
    BasicAuth,
    /// API key in header
    ApiKey,
    /// HMAC signature
    HmacSignature,
}

impl Default for WebhookAuthType {
    fn default() -> Self {
        Self::None
    }
}

impl WebhookAuthType {
    pub fn as_str(&self) -> &str {
        match self {
            Self::None => "NONE",
            Self::BearerToken => "BEARER_TOKEN",
            Self::BasicAuth => "BASIC_AUTH",
            Self::ApiKey => "API_KEY",
            Self::HmacSignature => "HMAC_SIGNATURE",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "BEARER_TOKEN" => Self::BearerToken,
            "BASIC_AUTH" => Self::BasicAuth,
            "API_KEY" => Self::ApiKey,
            "HMAC_SIGNATURE" => Self::HmacSignature,
            _ => Self::None,
        }
    }
}

/// Webhook credentials for service account
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookCredentials {
    /// Authentication type
    #[serde(default)]
    pub auth_type: WebhookAuthType,

    /// Bearer token or API key value
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,

    /// Username for basic auth
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,

    /// Password for basic auth
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,

    /// Header name for API key (default: X-Api-Key)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header_name: Option<String>,

    /// Secret for HMAC signature generation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signing_secret: Option<String>,

    /// HMAC algorithm (default: SHA256)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signing_algorithm: Option<String>,

    /// Header name for signature (default: X-Signature)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature_header: Option<String>,
}

impl WebhookCredentials {
    pub fn none() -> Self {
        Self {
            auth_type: WebhookAuthType::None,
            token: None,
            username: None,
            password: None,
            header_name: None,
            signing_secret: None,
            signing_algorithm: None,
            signature_header: None,
        }
    }

    pub fn bearer_token(token: impl Into<String>) -> Self {
        Self {
            auth_type: WebhookAuthType::BearerToken,
            token: Some(token.into()),
            ..Self::none()
        }
    }

    pub fn basic_auth(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            auth_type: WebhookAuthType::BasicAuth,
            username: Some(username.into()),
            password: Some(password.into()),
            ..Self::none()
        }
    }

    pub fn api_key(key: impl Into<String>, header_name: Option<String>) -> Self {
        Self {
            auth_type: WebhookAuthType::ApiKey,
            token: Some(key.into()),
            header_name: header_name.or(Some("X-Api-Key".to_string())),
            ..Self::none()
        }
    }

    pub fn hmac_signature(secret: impl Into<String>) -> Self {
        Self {
            auth_type: WebhookAuthType::HmacSignature,
            signing_secret: Some(secret.into()),
            signing_algorithm: Some("SHA256".to_string()),
            signature_header: Some("X-Signature".to_string()),
            ..Self::none()
        }
    }
}

impl Default for WebhookCredentials {
    fn default() -> Self {
        Self::none()
    }
}

/// Role assignment embedded in service account or principal
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleAssignment {
    /// Role name/code
    #[serde(rename = "roleName")]
    pub role: String,

    /// Client ID this role applies to (null = all clients)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,

    /// Source of this role assignment (e.g., "ADMIN", "IDP_SYNC")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignment_source: Option<String>,

    /// When the role was assigned
    pub assigned_at: DateTime<Utc>,

    /// Who assigned the role
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assigned_by: Option<String>,
}

impl RoleAssignment {
    pub fn new(role: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            client_id: None,
            assignment_source: None,
            assigned_at: Utc::now(),
            assigned_by: None,
        }
    }

    pub fn with_source(role: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            client_id: None,
            assignment_source: Some(source.into()),
            assigned_at: Utc::now(),
            assigned_by: None,
        }
    }

    pub fn for_client(role: impl Into<String>, client_id: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            client_id: Some(client_id.into()),
            assignment_source: None,
            assigned_at: Utc::now(),
            assigned_by: None,
        }
    }

    /// Check if this assignment is from IDP sync
    pub fn is_idp_sync(&self) -> bool {
        self.assignment_source.as_deref() == Some("IDP_SYNC")
    }
}

/// Service account entity
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccount {
    /// TSID as Crockford Base32 string
    pub id: String,

    /// Unique code
    pub code: String,

    /// Human-readable name
    pub name: String,

    /// Description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Whether the account is active
    #[serde(default = "default_active")]
    pub active: bool,

    /// Client IDs this service account can access
    /// Note: In PostgreSQL, this is stored via iam_client_access_grants on the principal
    #[serde(default)]
    pub client_ids: Vec<String>,

    /// Application ID (if created for an application)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application_id: Option<String>,

    /// Webhook credentials for outbound calls
    #[serde(default)]
    pub webhook_credentials: WebhookCredentials,

    /// Assigned roles (loaded from iam_principal_roles via the linked principal)
    #[serde(default)]
    pub roles: Vec<RoleAssignment>,

    /// Last time this account was used
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<DateTime<Utc>>,

    /// Audit fields
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

fn default_active() -> bool {
    true
}

impl ServiceAccount {
    pub fn new(code: impl Into<String>, name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: crate::TsidGenerator::generate(),
            code: code.into(),
            name: name.into(),
            description: None,
            active: true,
            client_ids: vec![],
            application_id: None,
            webhook_credentials: WebhookCredentials::none(),
            roles: vec![],
            last_used_at: None,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_client_id(mut self, client_id: impl Into<String>) -> Self {
        self.client_ids.push(client_id.into());
        self
    }

    pub fn with_application_id(mut self, application_id: impl Into<String>) -> Self {
        self.application_id = Some(application_id.into());
        self
    }

    pub fn with_credentials(mut self, credentials: WebhookCredentials) -> Self {
        self.webhook_credentials = credentials;
        self
    }

    pub fn assign_role(&mut self, role: impl Into<String>) {
        self.roles.push(RoleAssignment::new(role));
        self.updated_at = Utc::now();
    }

    pub fn assign_role_for_client(&mut self, role: impl Into<String>, client_id: impl Into<String>) {
        self.roles.push(RoleAssignment::for_client(role, client_id));
        self.updated_at = Utc::now();
    }

    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r.role == role)
    }

    pub fn has_client_access(&self, client_id: &str) -> bool {
        self.client_ids.is_empty() || self.client_ids.contains(&client_id.to_string())
    }

    pub fn deactivate(&mut self) {
        self.active = false;
        self.updated_at = Utc::now();
    }

    pub fn activate(&mut self) {
        self.active = true;
        self.updated_at = Utc::now();
    }

    pub fn record_usage(&mut self) {
        self.last_used_at = Some(Utc::now());
    }
}

/// Convert from SeaORM model to domain entity
/// Note: roles must be loaded separately from iam_principal_roles via the linked principal
impl From<crate::entities::iam_service_accounts::Model> for ServiceAccount {
    fn from(m: crate::entities::iam_service_accounts::Model) -> Self {
        let webhook_credentials = WebhookCredentials {
            auth_type: m.wh_auth_type.as_deref().map(WebhookAuthType::from_str).unwrap_or_default(),
            token: m.wh_auth_token_ref.clone(),
            username: None,
            password: None,
            header_name: None,
            signing_secret: m.wh_signing_secret_ref.clone(),
            signing_algorithm: m.wh_signing_algorithm.clone(),
            signature_header: None,
        };

        Self {
            id: m.id,
            code: m.code,
            name: m.name,
            description: m.description,
            active: m.active,
            client_ids: vec![], // Loaded via principal's client access grants
            application_id: m.application_id,
            webhook_credentials,
            roles: vec![], // Loaded via principal's role assignments
            last_used_at: m.last_used_at.map(|dt| dt.naive_utc().and_utc()),
            created_at: m.created_at.naive_utc().and_utc(),
            updated_at: m.updated_at.naive_utc().and_utc(),
        }
    }
}
