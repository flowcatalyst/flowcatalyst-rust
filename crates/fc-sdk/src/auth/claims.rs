//! JWT Claims and Auth Context
//!
//! Token claims matching FlowCatalyst's access token format,
//! plus a rich auth context for authorization checks.
//!
//! Claim shape (the Go platform's, which the Rust platform issues too):
//! `tier` is the tenancy tier (`ANCHOR` | `PARTNER` | `CLIENT`); `scope` is
//! the granted permissions as a space-delimited string (absent when there
//! are none); `token_use` is `api` or `identity`; `clients` holds `"*"` or
//! `"{id}:{identifier}"` entries; `applications` holds `"*"` or
//! `"{id}:{code}"` entries, with `all_applications` alongside. Bare ids (the
//! older form) are still accepted in both lists, and a token that predates
//! `tier` (tier carried in `scope`) still reads its tier correctly.

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

    /// Tenancy tier: `"ANCHOR"`, `"PARTNER"`, or `"CLIENT"`. Empty on a
    /// token that predates the claim — use [`Self::tenancy_tier`], which
    /// falls back to the legacy `scope`-as-tier form.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tier: String,

    /// Granted permissions, space-delimited (the OAuth `scope` claim);
    /// empty when the token carries none. See
    /// [`Self::granted_permissions`]. Tokens minted before `tier` existed
    /// carried the tenancy tier here instead.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub scope: String,

    /// User email (present for USER type, absent for SERVICE)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,

    /// Display name
    pub name: String,

    /// Clients this principal can access: `"{id}:{identifier}"` entries
    /// (bare ids on older tokens), or `["*"]` for anchor users.
    #[serde(default)]
    pub clients: Vec<String>,

    /// Roles assigned to this principal
    #[serde(default)]
    pub roles: Vec<String>,

    /// Applications this principal can access: `"{id}:{code}"` entries
    /// (bare ids on older tokens), or `["*"]` for every application.
    #[serde(default)]
    pub applications: Vec<String>,

    /// Access to every application, present and future (the application
    /// analogue of the anchor tier). When true, `applications` is not a
    /// restriction.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub all_applications: bool,

    /// Access-token class: `"api"` (carries authority, valid as a platform
    /// API bearer) or `"identity"` (interactive login; carries no roles,
    /// clients, applications or scope). `None` on tokens that predate it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_use: Option<String>,
}

const TIERS: [&str; 3] = ["ANCHOR", "PARTNER", "CLIENT"];

/// The id part of a `"{id}:{label}"` claim entry (the whole entry when it
/// has no label).
fn entry_id(entry: &str) -> &str {
    entry.split_once(':').map_or(entry, |(id, _)| id)
}

impl AccessTokenClaims {
    /// Check if this principal has access to a specific client (by id).
    pub fn has_client_access(&self, client_id: &str) -> bool {
        self.clients
            .iter()
            .any(|c| c == "*" || c == client_id || entry_id(c) == client_id)
    }

    /// The tenancy tier: the `tier` claim, or — on a token that predates it
    /// — a tier value carried in `scope`. Empty when neither is present.
    pub fn tenancy_tier(&self) -> &str {
        if !self.tier.is_empty() {
            &self.tier
        } else if TIERS.contains(&self.scope.as_str()) {
            &self.scope
        } else {
            ""
        }
    }

    /// The granted permissions from the space-delimited `scope` claim.
    /// Empty when the token carries none, and for a legacy token whose
    /// `scope` held the tier.
    pub fn granted_permissions(&self) -> Vec<&str> {
        if self.tier.is_empty() && TIERS.contains(&self.scope.as_str()) {
            return Vec::new();
        }
        self.scope.split_whitespace().collect()
    }

    /// The client ids this principal can access (`"*"` for all), with the
    /// identifier part of each `"{id}:{identifier}"` entry dropped.
    pub fn client_id_list(&self) -> Vec<&str> {
        self.clients.iter().map(|c| entry_id(c)).collect()
    }

    /// The application ids this principal can access, with the code part of
    /// each `"{id}:{code}"` entry dropped. Empty when
    /// [`Self::has_all_applications`] is true.
    pub fn application_ids(&self) -> Vec<&str> {
        if self.has_all_applications() {
            return Vec::new();
        }
        self.applications.iter().map(|a| entry_id(a)).collect()
    }

    /// Whether this principal reaches every application: the `"*"` entry or
    /// the `all_applications` claim.
    pub fn has_all_applications(&self) -> bool {
        self.all_applications || self.applications.iter().any(|a| a == "*")
    }

    /// Check if this principal can access an application (by id).
    pub fn has_application_access(&self, application_id: &str) -> bool {
        self.has_all_applications()
            || self
                .applications
                .iter()
                .any(|a| a == application_id || entry_id(a) == application_id)
    }

    /// Whether this is an identity-only access token (`token_use =
    /// "identity"`), which carries no authority.
    pub fn is_identity_token(&self) -> bool {
        self.token_use.as_deref() == Some("identity")
    }

    /// Check if this principal has a specific role.
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }

    /// Check if this is an anchor user (full platform access): the
    /// `ANCHOR` tier, or a `"*"` entry in `clients`.
    pub fn is_anchor(&self) -> bool {
        self.tenancy_tier() == "ANCHOR" || self.clients.iter().any(|c| c == "*")
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
            tier: scope.to_string(),
            scope: String::new(),
            email: Some("user@example.com".to_string()),
            name: "Test User".to_string(),
            clients: clients.into_iter().map(String::from).collect(),
            roles: roles.into_iter().map(String::from).collect(),
            applications: vec![],
            all_applications: false,
            token_use: None,
        }
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
        assert_eq!(deserialized.tier, "ANCHOR");
        assert_eq!(deserialized.tenancy_tier(), "ANCHOR");
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

    // ─── Go's claim shape ───────────────────────────────────────────────

    /// An `api` access token as the platform mints it (Go's shape).
    fn go_api_token() -> AccessTokenClaims {
        serde_json::from_value(serde_json::json!({
            "iss": "https://fc.example.com", "sub": "prn_1", "aud": "flowcatalyst",
            "exp": 9999999999i64, "iat": 1000000000, "nbf": 1000000000, "jti": "j1",
            "type": "USER", "tier": "CLIENT",
            "scope": "orders:order:read  orders:order:write",
            "email": "u@example.com", "name": "U",
            "clients": ["clt_a:acme", "clt_b"],
            "roles": ["orders:viewer"],
            "applications": ["app_1:orders", "app_2"],
            "all_applications": false,
            "token_use": "api"
        }))
        .expect("Go-shaped access token deserialises")
    }

    #[test]
    fn go_shape_tier_and_scope_are_read_apart() {
        let c = go_api_token();
        assert_eq!(c.tenancy_tier(), "CLIENT");
        assert!(!c.is_anchor());
        assert_eq!(
            c.granted_permissions(),
            vec!["orders:order:read", "orders:order:write"]
        );
        assert!(!c.is_identity_token());
    }

    #[test]
    fn go_shape_client_and_application_pairs_match_by_id() {
        let c = go_api_token();
        assert!(c.has_client_access("clt_a"));
        assert!(c.has_client_access("clt_b"));
        assert!(!c.has_client_access("acme"));
        assert!(!c.has_client_access("clt_c"));
        assert_eq!(c.client_id_list(), vec!["clt_a", "clt_b"]);
        assert!(c.has_application_access("app_1"));
        assert!(c.has_application_access("app_2"));
        assert!(!c.has_application_access("app_3"));
        assert_eq!(c.application_ids(), vec!["app_1", "app_2"]);
        assert!(!c.has_all_applications());
    }

    #[test]
    fn go_shape_anchor_tier_and_all_applications() {
        let c: AccessTokenClaims = serde_json::from_value(serde_json::json!({
            "iss": "i", "sub": "prn_1", "aud": "a", "exp": 1, "iat": 1, "nbf": 1, "jti": "j",
            "type": "USER", "tier": "ANCHOR", "name": "A",
            "clients": ["*"], "roles": [], "applications": ["*"], "all_applications": true
        }))
        .unwrap();
        assert!(c.is_anchor());
        assert!(c.granted_permissions().is_empty());
        assert!(c.has_all_applications());
        assert!(c.has_application_access("app_anything"));
        assert!(c.application_ids().is_empty());
    }

    #[test]
    fn go_shape_identity_token_without_scope_or_authority_deserialises() {
        // authorization_code logins mint an identity-class token: no scope
        // claim at all, empty authority lists.
        let c: AccessTokenClaims = serde_json::from_value(serde_json::json!({
            "iss": "i", "sub": "prn_1", "aud": "a", "exp": 1, "iat": 1, "nbf": 1, "jti": "j",
            "type": "USER", "tier": "PARTNER", "name": "P",
            "clients": [], "roles": [], "applications": [], "all_applications": false,
            "token_use": "identity"
        }))
        .unwrap();
        assert!(c.is_identity_token());
        assert_eq!(c.tenancy_tier(), "PARTNER");
        assert!(c.scope.is_empty());
        assert!(c.granted_permissions().is_empty());
    }

    #[test]
    fn legacy_scope_as_tier_token_still_reads_its_tier() {
        let c: AccessTokenClaims = serde_json::from_value(serde_json::json!({
            "iss": "i", "sub": "prn_1", "aud": "a", "exp": 1, "iat": 1, "nbf": 1, "jti": "j",
            "type": "USER", "scope": "ANCHOR", "name": "L", "clients": ["*"]
        }))
        .unwrap();
        assert_eq!(c.tenancy_tier(), "ANCHOR");
        assert!(c.is_anchor());
        assert!(c.granted_permissions().is_empty());
    }

    #[test]
    fn serialises_back_to_go_shape() {
        let json = serde_json::to_value(go_api_token()).unwrap();
        assert_eq!(json["tier"], "CLIENT");
        assert_eq!(json["scope"], "orders:order:read  orders:order:write");
        assert_eq!(json["token_use"], "api");
        assert_eq!(json["type"], "USER");
    }
}
