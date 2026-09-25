//! JWT Claims and Auth Context
//!
//! Token claims matching FlowCatalyst's access token format,
//! plus a rich auth context for authorization checks.

use serde::{Deserialize, Serialize};

/// JWT claims for access tokens issued by FlowCatalyst.
///
/// These claims are embedded in every JWT issued by the platform's
/// `/oauth/token` endpoint. The platform's shape: `tier` is the tenancy
/// tier and `scope` the space-delimited granted permissions. Tokens from
/// older platforms carry the tier on `scope` and no `tier`; [`Self::tier`]
/// reads either.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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

    /// Tenancy tier: `"ANCHOR"`, `"PARTNER"`, or `"CLIENT"`. Absent on
    /// tokens from older platforms, which carried it on `scope`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,

    /// The granted permissions, space-delimited (the OAuth scope). On tokens
    /// from older platforms (no `tier`), the tier.
    #[serde(default)]
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

    /// Application codes derived from roles (e.g. `"operant"` from
    /// `"operant:admin"`). Always present on tokens issued by FC, but
    /// `#[serde(default)]` lets us deserialize older tokens too.
    #[serde(default)]
    pub applications: Vec<String>,

    /// Access to every application (deprecated companion of the `"*"`
    /// applications entry)
    #[serde(default)]
    pub all_applications: bool,

    /// The OAuth client the token was minted through, when there was one
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub azp: Option<String>,

    /// `"api"` (carries authority) or `"identity"` (interactive-login
    /// token, no authority); absent on older tokens
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_use: Option<String>,
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

    /// The tenancy tier: the `tier` claim, or on an older token its `scope`.
    pub fn tier(&self) -> &str {
        self.tier.as_deref().unwrap_or(&self.scope)
    }

    /// The permissions granted on `scope`. Empty on older tokens, whose
    /// `scope` was the tier.
    pub fn permissions(&self) -> Vec<&str> {
        if self.tier.is_none() {
            return Vec::new();
        }
        self.scope.split_whitespace().collect()
    }

    /// Check if this is an anchor user (full platform access).
    pub fn is_anchor(&self) -> bool {
        self.tier() == "ANCHOR"
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_claims(
        scope: &str,
        principal_type: &str,
        clients: Vec<&str>,
        roles: Vec<&str>,
    ) -> AccessTokenClaims {
        AccessTokenClaims {
            sub: "prn_test123".to_string(),
            iss: "flowcatalyst".to_string(),
            aud: "flowcatalyst".to_string(),
            exp: 9999999999,
            iat: 1000000000,
            nbf: 1000000000,
            jti: "jti_abc".to_string(),
            principal_type: principal_type.to_string(),
            scope: scope.to_string(),
            email: Some("user@example.com".to_string()),
            name: "Test User".to_string(),
            clients: clients.into_iter().map(String::from).collect(),
            roles: roles.into_iter().map(String::from).collect(),
            applications: vec![],
            ..Default::default()
        }
    }

    /// The platform's shape (Go `authservice.AccessTokenClaims`): `tier`
    /// is the tier and `scope` the granted permissions.
    #[test]
    fn platform_shape_reads_tier_and_permissions() {
        let json = r#"{
            "sub": "prn_1", "iss": "fc", "aud": "fc", "exp": 9999999999,
            "iat": 1000000000, "nbf": 1000000000, "jti": "j1",
            "type": "SERVICE", "tier": "ANCHOR",
            "scope": "platform:iam:user:view platform:messaging:event:view",
            "name": "Svc", "clients": ["*"], "roles": [],
            "applications": ["*"], "all_applications": true, "token_use": "api"
        }"#;
        let claims: AccessTokenClaims = serde_json::from_str(json).unwrap();
        assert!(claims.is_anchor());
        assert_eq!(claims.tier(), "ANCHOR");
        assert_eq!(
            claims.permissions(),
            vec!["platform:iam:user:view", "platform:messaging:event:view"]
        );
        assert!(claims.all_applications);
        assert_eq!(claims.token_use.as_deref(), Some("api"));

        // A token with no granted permissions omits `scope`.
        let json = r#"{
            "sub": "prn_1", "iss": "fc", "aud": "fc", "exp": 9999999999,
            "iat": 1000000000, "type": "USER", "tier": "CLIENT",
            "name": "U", "clients": ["clt_1:acme"], "roles": []
        }"#;
        let claims: AccessTokenClaims = serde_json::from_str(json).unwrap();
        assert_eq!(claims.tier(), "CLIENT");
        assert!(claims.permissions().is_empty());
    }

    /// Older tokens carried the tier on `scope`.
    #[test]
    fn legacy_shape_reads_the_tier_from_scope() {
        let claims = make_claims("ANCHOR", "USER", vec!["*"], vec![]);
        assert_eq!(claims.tier(), "ANCHOR");
        assert!(claims.permissions().is_empty());
    }

    // ─── AccessTokenClaims ──────────────────────────────────────────────

    #[test]
    fn principal_id_returns_sub() {
        let claims = make_claims("CLIENT", "USER", vec!["clt_1"], vec![]);
        assert_eq!(claims.principal_id(), "prn_test123");
    }

    #[test]
    fn is_anchor_true_for_anchor_scope() {
        let claims = make_claims("ANCHOR", "USER", vec!["*"], vec![]);
        assert!(claims.is_anchor());
    }

    #[test]
    fn is_anchor_false_for_client_scope() {
        let claims = make_claims("CLIENT", "USER", vec!["clt_1"], vec![]);
        assert!(!claims.is_anchor());
    }

    #[test]
    fn is_anchor_false_for_partner_scope() {
        let claims = make_claims("PARTNER", "USER", vec!["clt_1", "clt_2"], vec![]);
        assert!(!claims.is_anchor());
    }

    #[test]
    fn is_service_true() {
        let claims = make_claims("CLIENT", "SERVICE", vec!["clt_1"], vec![]);
        assert!(claims.is_service());
    }

    #[test]
    fn is_service_false_for_user() {
        let claims = make_claims("CLIENT", "USER", vec!["clt_1"], vec![]);
        assert!(!claims.is_service());
    }

    #[test]
    fn has_client_access_specific_client() {
        let claims = make_claims("CLIENT", "USER", vec!["clt_a", "clt_b"], vec![]);
        assert!(claims.has_client_access("clt_a"));
        assert!(claims.has_client_access("clt_b"));
        assert!(!claims.has_client_access("clt_c"));
    }

    #[test]
    fn has_client_access_wildcard() {
        let claims = make_claims("ANCHOR", "USER", vec!["*"], vec![]);
        assert!(claims.has_client_access("clt_anything"));
        assert!(claims.has_client_access("clt_other"));
    }

    #[test]
    fn has_client_access_empty_clients() {
        let claims = make_claims("CLIENT", "USER", vec![], vec![]);
        assert!(!claims.has_client_access("clt_1"));
    }

    #[test]
    fn has_role_present() {
        let claims = make_claims("CLIENT", "USER", vec!["clt_1"], vec!["admin", "editor"]);
        assert!(claims.has_role("admin"));
        assert!(claims.has_role("editor"));
    }

    #[test]
    fn has_role_absent() {
        let claims = make_claims("CLIENT", "USER", vec!["clt_1"], vec!["viewer"]);
        assert!(!claims.has_role("admin"));
    }

    #[test]
    fn has_role_empty() {
        let claims = make_claims("CLIENT", "USER", vec!["clt_1"], vec![]);
        assert!(!claims.has_role("anything"));
    }

    #[test]
    fn serialization_round_trip() {
        let claims = make_claims("ANCHOR", "USER", vec!["*"], vec!["admin"]);
        let json = serde_json::to_string(&claims).unwrap();
        let deserialized: AccessTokenClaims = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.sub, "prn_test123");
        assert_eq!(deserialized.principal_type, "USER");
        assert_eq!(deserialized.scope, "ANCHOR");
        assert_eq!(deserialized.clients, vec!["*"]);
        assert_eq!(deserialized.roles, vec!["admin"]);
        assert_eq!(deserialized.email.as_deref(), Some("user@example.com"));
    }

    #[test]
    fn serialization_type_field_rename() {
        let claims = make_claims("CLIENT", "SERVICE", vec![], vec![]);
        let json = serde_json::to_value(&claims).unwrap();
        // principal_type is serialized as "type"
        assert_eq!(json["type"], "SERVICE");
        assert!(json.get("principal_type").is_none());
    }

    #[test]
    fn deserialization_with_missing_optional_email() {
        let json = r#"{
            "sub": "prn_1",
            "iss": "fc",
            "aud": "fc",
            "exp": 9999999999,
            "iat": 1000000000,
            "nbf": 1000000000,
            "jti": "j1",
            "type": "SERVICE",
            "scope": "CLIENT",
            "name": "Service Account",
            "clients": ["clt_1"],
            "roles": []
        }"#;
        let claims: AccessTokenClaims = serde_json::from_str(json).unwrap();
        assert!(claims.email.is_none());
        assert_eq!(claims.principal_type, "SERVICE");
    }

    #[test]
    fn email_skipped_in_serialization_when_none() {
        let mut claims = make_claims("CLIENT", "SERVICE", vec![], vec![]);
        claims.email = None;
        let json = serde_json::to_value(&claims).unwrap();
        assert!(json.get("email").is_none());
    }

    // ─── AuthContext ────────────────────────────────────────────────────

    #[test]
    fn auth_context_delegates_to_claims() {
        let claims = make_claims("ANCHOR", "USER", vec!["*"], vec!["admin", "viewer"]);
        let ctx = AuthContext::new(claims, "eyJtoken".to_string());

        assert_eq!(ctx.principal_id(), "prn_test123");
        assert_eq!(ctx.email(), Some("user@example.com"));
        assert_eq!(ctx.name(), "Test User");
        assert!(ctx.is_anchor());
        assert!(!ctx.is_service());
        assert!(ctx.has_client_access("any_client"));
        assert!(ctx.has_role("admin"));
        assert!(!ctx.has_role("super_admin"));
        assert_eq!(ctx.client_ids(), &["*"]);
        assert_eq!(ctx.roles(), &["admin", "viewer"]);
        assert_eq!(ctx.bearer_token(), "eyJtoken");
    }

    #[test]
    fn auth_context_service_account() {
        let mut claims = make_claims("CLIENT", "SERVICE", vec!["clt_svc"], vec![]);
        claims.email = None;
        let ctx = AuthContext::new(claims, "svc-token".to_string());

        assert!(ctx.is_service());
        assert!(!ctx.is_anchor());
        assert!(ctx.email().is_none());
        assert!(ctx.has_client_access("clt_svc"));
        assert!(!ctx.has_client_access("clt_other"));
    }

    #[test]
    fn auth_context_clone() {
        let claims = make_claims("CLIENT", "USER", vec!["clt_1"], vec!["role1"]);
        let ctx = AuthContext::new(claims, "tok".to_string());
        let cloned = ctx.clone();

        assert_eq!(cloned.principal_id(), ctx.principal_id());
        assert_eq!(cloned.bearer_token(), ctx.bearer_token());
    }
}
