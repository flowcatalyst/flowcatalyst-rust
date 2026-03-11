//! JWT Claims and Auth Context
//!
//! Token claims matching FlowCatalyst's access token format,
//! plus a rich auth context for authorization checks.

use serde::{Deserialize, Serialize};

/// JWT claims for access tokens issued by FlowCatalyst.
///
/// These claims are embedded in every JWT issued by the platform's
/// `/oauth/token` endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessTokenClaims {
    /// Subject — principal ID (e.g., `"prn_0HZXEQ5Y8JY5Z"`)
    pub sub: String,

    /// Issuer (e.g., `"flowcatalyst"`)
    pub iss: String,

    /// Audience (e.g., `"flowcatalyst"`)
    pub aud: String,

    /// Expiration time (Unix timestamp)
    pub exp: i64,

    /// Issued at (Unix timestamp)
    pub iat: i64,

    /// Not before (Unix timestamp)
    pub nbf: i64,

    /// JWT ID (unique identifier)
    pub jti: String,

    /// Principal type: `"USER"` or `"SERVICE"`
    #[serde(rename = "type")]
    pub principal_type: String,

    /// User scope: `"ANCHOR"`, `"PARTNER"`, or `"CLIENT"`
    pub scope: String,

    /// User email (present for USER type, absent for SERVICE)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,

    /// Display name
    pub name: String,

    /// Client IDs this principal can access.
    /// `["*"]` for anchor users (access to all clients).
    pub clients: Vec<String>,

    /// Roles assigned to this principal
    #[serde(default)]
    pub roles: Vec<String>,
}

impl AccessTokenClaims {
    /// Check if this principal has access to a specific client.
    pub fn has_client_access(&self, client_id: &str) -> bool {
        self.clients.iter().any(|c| c == "*" || c == client_id)
    }

    /// Check if this principal has a specific role.
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }

    /// Check if this is an anchor user (full platform access).
    pub fn is_anchor(&self) -> bool {
        self.scope == "ANCHOR"
    }

    /// Check if this is a service account.
    pub fn is_service(&self) -> bool {
        self.principal_type == "SERVICE"
    }

    /// Get the principal ID.
    pub fn principal_id(&self) -> &str {
        &self.sub
    }
}

/// Rich authentication context built from validated token claims.
///
/// Provides convenient methods for authorization checks.
///
/// # Example
///
/// ```ignore
/// let ctx = token_validator.validate(&token).await?;
///
/// if ctx.is_anchor() {
///     // Full admin access
/// } else if ctx.has_client_access("clt_123") {
///     // Scoped to specific client
/// }
///
/// if ctx.has_role("admin") {
///     // Role-based access
/// }
/// ```
#[derive(Debug, Clone)]
pub struct AuthContext {
    /// The validated token claims
    pub claims: AccessTokenClaims,
    /// The raw JWT token string (for forwarding to downstream services)
    pub token: String,
}

impl AuthContext {
    pub fn new(claims: AccessTokenClaims, token: String) -> Self {
        Self { claims, token }
    }

    /// Principal ID from the token subject claim.
    pub fn principal_id(&self) -> &str {
        &self.claims.sub
    }

    /// User email (if present).
    pub fn email(&self) -> Option<&str> {
        self.claims.email.as_deref()
    }

    /// Display name.
    pub fn name(&self) -> &str {
        &self.claims.name
    }

    /// Whether this is an anchor user with full platform access.
    pub fn is_anchor(&self) -> bool {
        self.claims.is_anchor()
    }

    /// Whether this is a service account.
    pub fn is_service(&self) -> bool {
        self.claims.is_service()
    }

    /// Check if the principal has access to a specific client.
    pub fn has_client_access(&self, client_id: &str) -> bool {
        self.claims.has_client_access(client_id)
    }

    /// Check if the principal has a specific role.
    pub fn has_role(&self, role: &str) -> bool {
        self.claims.has_role(role)
    }

    /// Get the list of accessible client IDs.
    pub fn client_ids(&self) -> &[String] {
        &self.claims.clients
    }

    /// Get the list of assigned roles.
    pub fn roles(&self) -> &[String] {
        &self.claims.roles
    }

    /// Get the raw token for forwarding to downstream services.
    pub fn bearer_token(&self) -> &str {
        &self.token
    }
}
