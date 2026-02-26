//! Keycloak Adapter
//!
//! Handles authentication with Keycloak identity provider.
//! Supports:
//! - OIDC discovery from Keycloak realm
//! - Realm role extraction
//! - Client role extraction
//! - Group claim extraction
//! - User attribute mapping

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::UserScope;
use crate::auth::oidc_service::IdTokenClaims;

use super::{IdpAdapter, IdpRoleMappingConfig, IdpUserInfo, apply_role_mappings};

/// Configuration for Keycloak adapter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeycloakConfig {
    /// Keycloak server URL (e.g., "https://keycloak.example.com")
    pub server_url: String,
    /// Keycloak realm name
    pub realm: String,
    /// Client ID for extracting client roles
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// Claim containing realm roles
    #[serde(default = "default_realm_access_claim")]
    pub realm_access_claim: String,
    /// Claim containing resource/client roles
    #[serde(default = "default_resource_access_claim")]
    pub resource_access_claim: String,
    /// Claim containing groups
    #[serde(default = "default_groups_claim")]
    pub groups_claim: String,
    /// Realm roles that indicate anchor (admin) scope
    #[serde(default)]
    pub anchor_roles: Vec<String>,
    /// Realm roles that indicate partner scope
    #[serde(default)]
    pub partner_roles: Vec<String>,
}

fn default_realm_access_claim() -> String {
    "realm_access".to_string()
}

fn default_resource_access_claim() -> String {
    "resource_access".to_string()
}

fn default_groups_claim() -> String {
    "groups".to_string()
}

impl KeycloakConfig {
    /// Create a new Keycloak config for a specific server and realm
    pub fn new(server_url: impl Into<String>, realm: impl Into<String>) -> Self {
        Self {
            server_url: server_url.into(),
            realm: realm.into(),
            client_id: None,
            realm_access_claim: default_realm_access_claim(),
            resource_access_claim: default_resource_access_claim(),
            groups_claim: default_groups_claim(),
            anchor_roles: vec![],
            partner_roles: vec![],
        }
    }

    /// Set the client ID for client role extraction
    pub fn with_client_id(mut self, client_id: impl Into<String>) -> Self {
        self.client_id = Some(client_id.into());
        self
    }

    /// Set anchor roles
    pub fn with_anchor_roles(mut self, roles: Vec<String>) -> Self {
        self.anchor_roles = roles;
        self
    }

    /// Set partner roles
    pub fn with_partner_roles(mut self, roles: Vec<String>) -> Self {
        self.partner_roles = roles;
        self
    }
}

/// Keycloak identity provider adapter
pub struct KeycloakAdapter {
    config: KeycloakConfig,
}

impl KeycloakAdapter {
    pub fn new(config: KeycloakConfig) -> Self {
        Self { config }
    }

    /// Create adapter for a specific server and realm
    pub fn for_realm(server_url: impl Into<String>, realm: impl Into<String>) -> Self {
        Self::new(KeycloakConfig::new(server_url, realm))
    }

    /// Extract realm roles from claims
    /// Keycloak puts realm roles in realm_access.roles
    fn extract_realm_roles(&self, claims: &IdTokenClaims) -> Vec<String> {
        // Keycloak includes realm_access as a nested object in ID token
        // For now, use the standard roles claim if present
        if let Some(ref roles) = claims.roles {
            return roles.clone();
        }
        vec![]
    }

    /// Extract groups from claims
    fn extract_groups(&self, claims: &IdTokenClaims) -> Vec<String> {
        if let Some(ref groups) = claims.groups {
            return groups.clone();
        }
        vec![]
    }

    /// Determine user scope based on realm role membership
    fn determine_scope(&self, roles: &[String]) -> Option<UserScope> {
        // Check anchor roles first (highest privilege)
        for anchor_role in &self.config.anchor_roles {
            if roles.iter().any(|r| r == anchor_role) {
                return Some(UserScope::Anchor);
            }
        }

        // Check partner roles
        for partner_role in &self.config.partner_roles {
            if roles.iter().any(|r| r == partner_role) {
                return Some(UserScope::Partner);
            }
        }

        // Default: no specific scope suggestion
        None
    }
}

#[async_trait]
impl IdpAdapter for KeycloakAdapter {
    fn provider_type(&self) -> &'static str {
        "keycloak"
    }

    fn discovery_url(&self) -> String {
        format!(
            "{}/realms/{}/.well-known/openid-configuration",
            self.config.server_url.trim_end_matches('/'),
            self.config.realm
        )
    }

    fn extract_user_info(&self, claims: &IdTokenClaims, role_config: &IdpRoleMappingConfig) -> IdpUserInfo {
        let roles = self.extract_realm_roles(claims);
        let groups = self.extract_groups(claims);

        debug!(
            sub = %claims.sub,
            email = ?claims.email,
            roles_count = roles.len(),
            groups_count = groups.len(),
            "Extracting Keycloak user info"
        );

        // Apply role mappings
        let fc_roles = apply_role_mappings(&roles, &groups, role_config);

        // Determine scope from roles
        let suggested_scope = self.determine_scope(&roles);

        IdpUserInfo {
            external_id: claims.sub.clone(),
            email: claims.email.clone(),
            email_verified: claims.email_verified.unwrap_or(false),
            first_name: claims.given_name.clone(),
            last_name: claims.family_name.clone(),
            display_name: claims.name.clone(),
            picture_url: claims.picture.clone(),
            groups,
            roles,
            fc_roles,
            suggested_scope,
        }
    }

    fn required_scopes(&self) -> Vec<String> {
        vec![
            "openid".to_string(),
            "profile".to_string(),
            "email".to_string(),
            // Keycloak uses 'roles' scope for role claims
            "roles".to_string(),
        ]
    }
}

/// Extended claims specific to Keycloak
/// These can be extracted from the ID/access token
#[derive(Debug, Clone, Deserialize)]
pub struct KeycloakExtendedClaims {
    /// Preferred username
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_username: Option<String>,
    /// Realm access containing realm roles
    #[serde(skip_serializing_if = "Option::is_none")]
    pub realm_access: Option<RealmAccess>,
    /// Resource/client access containing client roles
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_access: Option<serde_json::Value>,
    /// Group memberships
    #[serde(skip_serializing_if = "Option::is_none")]
    pub groups: Option<Vec<String>>,
    /// Session state
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_state: Option<String>,
    /// Authorized party (client ID that initiated auth)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub azp: Option<String>,
}

/// Keycloak realm access structure
#[derive(Debug, Clone, Deserialize)]
pub struct RealmAccess {
    /// Realm roles
    pub roles: Vec<String>,
}

/// Keycloak client/resource access structure
#[derive(Debug, Clone, Deserialize)]
pub struct ClientAccess {
    /// Client roles
    pub roles: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discovery_url() {
        let adapter = KeycloakAdapter::for_realm("https://keycloak.example.com", "my-realm");
        assert_eq!(
            adapter.discovery_url(),
            "https://keycloak.example.com/realms/my-realm/.well-known/openid-configuration"
        );
    }

    #[test]
    fn test_discovery_url_trailing_slash() {
        let adapter = KeycloakAdapter::for_realm("https://keycloak.example.com/", "my-realm");
        assert_eq!(
            adapter.discovery_url(),
            "https://keycloak.example.com/realms/my-realm/.well-known/openid-configuration"
        );
    }

    #[test]
    fn test_scope_determination() {
        let adapter = KeycloakAdapter::new(KeycloakConfig {
            server_url: "https://keycloak.example.com".to_string(),
            realm: "test".to_string(),
            anchor_roles: vec!["admin".to_string(), "super-admin".to_string()],
            partner_roles: vec!["partner".to_string()],
            ..KeycloakConfig::new("", "")
        });

        // Anchor role match
        let roles = vec!["admin".to_string(), "user".to_string()];
        assert_eq!(adapter.determine_scope(&roles), Some(UserScope::Anchor));

        // Partner role match
        let roles = vec!["partner".to_string(), "user".to_string()];
        assert_eq!(adapter.determine_scope(&roles), Some(UserScope::Partner));

        // No match
        let roles = vec!["user".to_string()];
        assert_eq!(adapter.determine_scope(&roles), None);
    }

    #[test]
    fn test_config_builder() {
        let config = KeycloakConfig::new("https://kc.example.com", "production")
            .with_client_id("my-app")
            .with_anchor_roles(vec!["admin".to_string()])
            .with_partner_roles(vec!["partner".to_string()]);

        assert_eq!(config.server_url, "https://kc.example.com");
        assert_eq!(config.realm, "production");
        assert_eq!(config.client_id, Some("my-app".to_string()));
        assert_eq!(config.anchor_roles, vec!["admin".to_string()]);
    }
}
