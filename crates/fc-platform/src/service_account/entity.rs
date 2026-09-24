//! Service Account Entity
//!
//! Machine-to-machine authentication for webhooks.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Webhook authentication type — matches TypeScript WebhookAuthType
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[derive(Default)]
pub enum WebhookAuthType {
    /// No authentication
    #[default]
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

impl WebhookAuthType {
    pub fn as_str(&self) -> &'static str {
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

    /// Scope (ANCHOR, PARTNER, CLIENT)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,

    /// Application ID (if created for an application)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application_id: Option<String>,

    /// Webhook credentials for outbound calls
    #[serde(default)]
    pub webhook_credentials: WebhookCredentials,

    /// The iam_service_accounts.id (separate from the principal ID which is self.id)
    #[serde(skip)]
    pub service_account_table_id: Option<String>,

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
            id: crate::TsidGenerator::generate(crate::EntityType::ServiceAccount),
            code: code.into(),
            name: name.into(),
            description: None,
            active: true,
            client_ids: vec![],
            scope: None,
            application_id: None,
            webhook_credentials: WebhookCredentials::none(),
            service_account_table_id: None,
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

    pub fn assign_role_for_client(
        &mut self,
        role: impl Into<String>,
        client_id: impl Into<String>,
    ) {
        self.roles.push(RoleAssignment::for_client(role, client_id));
        self.updated_at = Utc::now();
    }

    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r.role == role)
    }

    pub fn has_client_access(&self, client_id: &str) -> bool {
        self.client_ids.is_empty() || self.client_ids.iter().any(|c| c == client_id)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_service_account() {
        let sa = ServiceAccount::new("app:my-app", "My App Service Account");

        assert!(!sa.id.is_empty());
        assert!(
            sa.id.starts_with("sac_"),
            "ID should have sac_ prefix, got: {}",
            sa.id
        );
        assert_eq!(
            sa.id.len(),
            17,
            "Typed ID should be 17 chars, got: {}",
            sa.id.len()
        );
        assert_eq!(sa.code, "app:my-app");
        assert_eq!(sa.name, "My App Service Account");
        assert!(sa.description.is_none());
        assert!(sa.active);
        assert!(sa.client_ids.is_empty());
        assert!(sa.scope.is_none());
        assert!(sa.application_id.is_none());
        assert_eq!(sa.webhook_credentials.auth_type, WebhookAuthType::None);
        assert!(sa.roles.is_empty());
        assert!(sa.last_used_at.is_none());
        assert_eq!(sa.created_at, sa.updated_at);
    }

    #[test]
    fn test_service_account_unique_ids() {
        let sa1 = ServiceAccount::new("a", "A");
        let sa2 = ServiceAccount::new("b", "B");
        assert_ne!(sa1.id, sa2.id);
    }

    #[test]
    fn test_service_account_builder_methods() {
        let sa = ServiceAccount::new("sa", "SA")
            .with_description("A test service account")
            .with_client_id("client-1")
            .with_application_id("app-1")
            .with_credentials(WebhookCredentials::bearer_token("my-token"));

        assert_eq!(sa.description, Some("A test service account".to_string()));
        assert_eq!(sa.client_ids, vec!["client-1".to_string()]);
        assert_eq!(sa.application_id, Some("app-1".to_string()));
        assert_eq!(
            sa.webhook_credentials.auth_type,
            WebhookAuthType::BearerToken
        );
        assert_eq!(sa.webhook_credentials.token, Some("my-token".to_string()));
    }

    #[test]
    fn test_service_account_activate_deactivate() {
        let mut sa = ServiceAccount::new("sa", "SA");
        assert!(sa.active);

        sa.deactivate();
        assert!(!sa.active);

        sa.activate();
        assert!(sa.active);
    }

    #[test]
    fn test_service_account_assign_role() {
        let mut sa = ServiceAccount::new("sa", "SA");
        assert!(sa.roles.is_empty());

        sa.assign_role("admin");
        assert_eq!(sa.roles.len(), 1);
        assert_eq!(sa.roles[0].role, "admin");
    }

    #[test]
    fn test_service_account_assign_role_for_client() {
        let mut sa = ServiceAccount::new("sa", "SA");
        sa.assign_role_for_client("viewer", "client-1");

        assert_eq!(sa.roles.len(), 1);
        assert_eq!(sa.roles[0].role, "viewer");
        assert_eq!(sa.roles[0].client_id, Some("client-1".to_string()));
    }

    #[test]
    fn test_service_account_has_role() {
        let mut sa = ServiceAccount::new("sa", "SA");
        sa.assign_role("admin");
        sa.assign_role("viewer");

        assert!(sa.has_role("admin"));
        assert!(sa.has_role("viewer"));
        assert!(!sa.has_role("editor"));
    }

    #[test]
    fn test_service_account_has_client_access() {
        let sa_no_clients = ServiceAccount::new("sa", "SA");
        // Empty client_ids means access to all
        assert!(sa_no_clients.has_client_access("any-client"));

        let sa_with_clients = ServiceAccount::new("sa", "SA").with_client_id("client-1");
        assert!(sa_with_clients.has_client_access("client-1"));
        assert!(!sa_with_clients.has_client_access("client-2"));
    }

    #[test]
    fn test_service_account_record_usage() {
        let mut sa = ServiceAccount::new("sa", "SA");
        assert!(sa.last_used_at.is_none());

        sa.record_usage();
        assert!(sa.last_used_at.is_some());
    }

    // --- WebhookAuthType ---

    #[test]
    fn test_webhook_auth_type_as_str() {
        assert_eq!(WebhookAuthType::None.as_str(), "NONE");
        assert_eq!(WebhookAuthType::BearerToken.as_str(), "BEARER_TOKEN");
        assert_eq!(WebhookAuthType::BasicAuth.as_str(), "BASIC_AUTH");
        assert_eq!(WebhookAuthType::ApiKey.as_str(), "API_KEY");
        assert_eq!(WebhookAuthType::HmacSignature.as_str(), "HMAC_SIGNATURE");
    }

    #[test]
    fn test_webhook_auth_type_from_str() {
        assert_eq!(WebhookAuthType::from_str("NONE"), WebhookAuthType::None);
        assert_eq!(
            WebhookAuthType::from_str("BEARER_TOKEN"),
            WebhookAuthType::BearerToken
        );
        assert_eq!(
            WebhookAuthType::from_str("BASIC_AUTH"),
            WebhookAuthType::BasicAuth
        );
        assert_eq!(
            WebhookAuthType::from_str("API_KEY"),
            WebhookAuthType::ApiKey
        );
        assert_eq!(
            WebhookAuthType::from_str("HMAC_SIGNATURE"),
            WebhookAuthType::HmacSignature
        );
        assert_eq!(WebhookAuthType::from_str("unknown"), WebhookAuthType::None);
    }

    #[test]
    fn test_webhook_auth_type_default() {
        assert_eq!(WebhookAuthType::default(), WebhookAuthType::None);
    }

    #[test]
    fn test_webhook_auth_type_roundtrip() {
        for t in [
            WebhookAuthType::None,
            WebhookAuthType::BearerToken,
            WebhookAuthType::BasicAuth,
            WebhookAuthType::ApiKey,
            WebhookAuthType::HmacSignature,
        ] {
            assert_eq!(
                WebhookAuthType::from_str(t.as_str()),
                t,
                "Roundtrip failed for {:?}",
                t
            );
        }
    }

    // --- WebhookCredentials constructors ---

    #[test]
    fn test_webhook_credentials_none() {
        let creds = WebhookCredentials::none();
        assert_eq!(creds.auth_type, WebhookAuthType::None);
        assert!(creds.token.is_none());
        assert!(creds.username.is_none());
        assert!(creds.password.is_none());
    }

    #[test]
    fn test_webhook_credentials_bearer_token() {
        let creds = WebhookCredentials::bearer_token("my-secret-token");
        assert_eq!(creds.auth_type, WebhookAuthType::BearerToken);
        assert_eq!(creds.token, Some("my-secret-token".to_string()));
    }

    #[test]
    fn test_webhook_credentials_basic_auth() {
        let creds = WebhookCredentials::basic_auth("user", "pass");
        assert_eq!(creds.auth_type, WebhookAuthType::BasicAuth);
        assert_eq!(creds.username, Some("user".to_string()));
        assert_eq!(creds.password, Some("pass".to_string()));
    }

    #[test]
    fn test_webhook_credentials_api_key() {
        let creds = WebhookCredentials::api_key("key-value", None);
        assert_eq!(creds.auth_type, WebhookAuthType::ApiKey);
        assert_eq!(creds.token, Some("key-value".to_string()));
        assert_eq!(creds.header_name, Some("X-Api-Key".to_string()));

        let creds_custom = WebhookCredentials::api_key("key", Some("X-Custom-Key".to_string()));
        assert_eq!(creds_custom.header_name, Some("X-Custom-Key".to_string()));
    }

    #[test]
    fn test_webhook_credentials_hmac_signature() {
        let creds = WebhookCredentials::hmac_signature("my-signing-secret");
        assert_eq!(creds.auth_type, WebhookAuthType::HmacSignature);
        assert_eq!(creds.signing_secret, Some("my-signing-secret".to_string()));
        assert_eq!(creds.signing_algorithm, Some("SHA256".to_string()));
        assert_eq!(creds.signature_header, Some("X-Signature".to_string()));
    }

    #[test]
    fn test_webhook_credentials_default() {
        let creds = WebhookCredentials::default();
        assert_eq!(creds.auth_type, WebhookAuthType::None);
    }

    // --- RoleAssignment ---

    #[test]
    fn test_role_assignment_new() {
        let ra = RoleAssignment::new("admin");
        assert_eq!(ra.role, "admin");
        assert!(ra.client_id.is_none());
        assert!(ra.assignment_source.is_none());
        assert!(ra.assigned_by.is_none());
    }

    #[test]
    fn test_role_assignment_with_source() {
        let ra = RoleAssignment::with_source("admin", "IDP_SYNC");
        assert_eq!(ra.role, "admin");
        assert_eq!(ra.assignment_source, Some("IDP_SYNC".to_string()));
        assert!(ra.is_idp_sync());
    }

    #[test]
    fn test_role_assignment_for_client() {
        let ra = RoleAssignment::for_client("viewer", "client-1");
        assert_eq!(ra.role, "viewer");
        assert_eq!(ra.client_id, Some("client-1".to_string()));
    }

    #[test]
    fn test_role_assignment_is_idp_sync() {
        let ra_sync = RoleAssignment::with_source("admin", "IDP_SYNC");
        assert!(ra_sync.is_idp_sync());

        let ra_not_sync = RoleAssignment::with_source("admin", "ADMIN");
        assert!(!ra_not_sync.is_idp_sync());

        let ra_no_source = RoleAssignment::new("admin");
        assert!(!ra_no_source.is_idp_sync());
    }
}
