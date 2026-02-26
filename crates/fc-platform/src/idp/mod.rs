//! Identity Provider Adapters
//!
//! Specialized adapters for different OIDC identity providers.
//! Each adapter handles provider-specific claim mapping and role extraction.

pub mod entra;
pub mod keycloak;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::UserScope;
use crate::auth::oidc_service::IdTokenClaims;

/// Role mapping from IDP to FlowCatalyst
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleMapping {
    /// IDP role/group name or pattern
    pub idp_role: String,
    /// FlowCatalyst role to assign
    pub fc_role: String,
    /// Optional client ID for client-scoped roles
    pub client_id: Option<String>,
}

/// Configuration for IDP role mapping
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdpRoleMappingConfig {
    /// Role mappings from IDP to FlowCatalyst
    pub role_mappings: Vec<RoleMapping>,
    /// Default FlowCatalyst role if no mapping matches
    pub default_role: Option<String>,
    /// Whether to sync all IDP roles (create FC roles if they don't exist)
    pub auto_create_roles: bool,
}

impl Default for IdpRoleMappingConfig {
    fn default() -> Self {
        Self {
            role_mappings: vec![],
            default_role: None,
            auto_create_roles: false,
        }
    }
}

/// Extracted user information from IDP token
#[derive(Debug, Clone)]
pub struct IdpUserInfo {
    /// External subject ID from IDP
    pub external_id: String,
    /// Email address
    pub email: Option<String>,
    /// Email verified
    pub email_verified: bool,
    /// First/given name
    pub first_name: Option<String>,
    /// Last/family name
    pub last_name: Option<String>,
    /// Display name
    pub display_name: Option<String>,
    /// Picture URL
    pub picture_url: Option<String>,
    /// Raw groups from IDP
    pub groups: Vec<String>,
    /// Raw roles from IDP
    pub roles: Vec<String>,
    /// Mapped FlowCatalyst roles
    pub fc_roles: Vec<MappedRole>,
    /// Suggested user scope
    pub suggested_scope: Option<UserScope>,
}

/// A mapped FlowCatalyst role
#[derive(Debug, Clone)]
pub struct MappedRole {
    /// Role code
    pub role: String,
    /// Optional client ID for client-scoped roles
    pub client_id: Option<String>,
    /// Source of the role assignment (e.g., "IDP_SYNC")
    pub source: String,
}

/// Trait for IDP-specific adapters
#[async_trait]
pub trait IdpAdapter: Send + Sync {
    /// Get the provider type identifier
    fn provider_type(&self) -> &'static str;

    /// Get the discovery URL for this provider
    fn discovery_url(&self) -> String;

    /// Extract user information from ID token claims
    fn extract_user_info(&self, claims: &IdTokenClaims, config: &IdpRoleMappingConfig) -> IdpUserInfo;

    /// Get additional scopes required by this provider
    fn required_scopes(&self) -> Vec<String> {
        vec!["openid".to_string(), "profile".to_string(), "email".to_string()]
    }
}

/// Apply role mappings to extract FlowCatalyst roles
pub fn apply_role_mappings(
    idp_roles: &[String],
    idp_groups: &[String],
    config: &IdpRoleMappingConfig,
) -> Vec<MappedRole> {
    let mut mapped_roles = Vec::new();

    // Combine roles and groups for matching
    let all_idp_items: Vec<&str> = idp_roles.iter()
        .chain(idp_groups.iter())
        .map(|s| s.as_str())
        .collect();

    for mapping in &config.role_mappings {
        // Check if any IDP role/group matches
        let matches = all_idp_items.iter().any(|item| {
            if mapping.idp_role.contains('*') {
                // Wildcard matching
                let pattern = mapping.idp_role.replace('*', "");
                item.contains(&pattern)
            } else {
                // Exact match
                *item == mapping.idp_role
            }
        });

        if matches {
            mapped_roles.push(MappedRole {
                role: mapping.fc_role.clone(),
                client_id: mapping.client_id.clone(),
                source: "IDP_SYNC".to_string(),
            });
        }
    }

    // Add default role if no mappings matched
    if mapped_roles.is_empty() {
        if let Some(ref default_role) = config.default_role {
            mapped_roles.push(MappedRole {
                role: default_role.clone(),
                client_id: None,
                source: "IDP_SYNC".to_string(),
            });
        }
    }

    mapped_roles
}
