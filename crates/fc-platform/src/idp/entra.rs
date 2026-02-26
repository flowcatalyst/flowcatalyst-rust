//! Microsoft Entra ID (Azure AD) Adapter
//!
//! Handles authentication with Microsoft Entra ID (formerly Azure Active Directory).
//! Supports:
//! - OIDC discovery from Azure tenant
//! - Group claim extraction
//! - Role claim extraction from app roles
//! - User provisioning from ID token claims

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::UserScope;
use crate::auth::oidc_service::IdTokenClaims;

use super::{IdpAdapter, IdpRoleMappingConfig, IdpUserInfo, apply_role_mappings};

/// Azure cloud environment
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AzureCloud {
    /// Azure Public Cloud (default)
    Public,
    /// Azure Government Cloud
    Government,
    /// Azure China Cloud
    China,
    /// Azure Germany Cloud (legacy)
    Germany,
}

impl Default for AzureCloud {
    fn default() -> Self {
        Self::Public
    }
}

impl AzureCloud {
    /// Get the base URL for this Azure cloud
    pub fn base_url(&self) -> &'static str {
        match self {
            Self::Public => "https://login.microsoftonline.com",
            Self::Government => "https://login.microsoftonline.us",
            Self::China => "https://login.chinacloudapi.cn",
            Self::Germany => "https://login.microsoftonline.de",
        }
    }

    /// Get the Graph API base URL for this Azure cloud
    pub fn graph_url(&self) -> &'static str {
        match self {
            Self::Public => "https://graph.microsoft.com",
            Self::Government => "https://graph.microsoft.us",
            Self::China => "https://microsoftgraph.chinacloudapi.cn",
            Self::Germany => "https://graph.microsoft.de",
        }
    }
}

/// Configuration for Entra ID adapter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntraIdConfig {
    /// Azure tenant ID (GUID or domain)
    pub tenant_id: String,
    /// Azure cloud environment
    #[serde(default)]
    pub cloud: AzureCloud,
    /// Claim containing group memberships
    #[serde(default = "default_groups_claim")]
    pub groups_claim: String,
    /// Claim containing app roles
    #[serde(default = "default_roles_claim")]
    pub roles_claim: String,
    /// Whether to use group names instead of GUIDs (requires Graph API)
    #[serde(default)]
    pub resolve_group_names: bool,
    /// Groups that indicate anchor (admin) scope
    #[serde(default)]
    pub anchor_groups: Vec<String>,
    /// Groups that indicate partner scope
    #[serde(default)]
    pub partner_groups: Vec<String>,
}

fn default_groups_claim() -> String {
    "groups".to_string()
}

fn default_roles_claim() -> String {
    "roles".to_string()
}

impl Default for EntraIdConfig {
    fn default() -> Self {
        Self {
            tenant_id: "common".to_string(),
            cloud: AzureCloud::Public,
            groups_claim: default_groups_claim(),
            roles_claim: default_roles_claim(),
            resolve_group_names: false,
            anchor_groups: vec![],
            partner_groups: vec![],
        }
    }
}

/// Entra ID (Azure AD) identity provider adapter
pub struct EntraIdAdapter {
    config: EntraIdConfig,
}

impl EntraIdAdapter {
    pub fn new(config: EntraIdConfig) -> Self {
        Self { config }
    }

    /// Create adapter for a specific tenant
    pub fn for_tenant(tenant_id: impl Into<String>) -> Self {
        Self::new(EntraIdConfig {
            tenant_id: tenant_id.into(),
            ..Default::default()
        })
    }

    /// Extract groups from claims
    /// Azure puts groups in either 'groups' claim (as GUIDs) or custom claim
    fn extract_groups(&self, claims: &IdTokenClaims) -> Vec<String> {
        // First try the standard groups claim
        if let Some(ref groups) = claims.groups {
            return groups.clone();
        }

        // Azure ID tokens may have groups as a custom claim
        // For now, return empty if not present
        vec![]
    }

    /// Extract roles from claims
    /// Azure app roles come in the 'roles' claim
    fn extract_roles(&self, claims: &IdTokenClaims) -> Vec<String> {
        if let Some(ref roles) = claims.roles {
            return roles.clone();
        }
        vec![]
    }

    /// Determine user scope based on group membership
    fn determine_scope(&self, groups: &[String]) -> Option<UserScope> {
        // Check anchor groups first (highest privilege)
        for anchor_group in &self.config.anchor_groups {
            if groups.iter().any(|g| g == anchor_group || g.contains(anchor_group)) {
                return Some(UserScope::Anchor);
            }
        }

        // Check partner groups
        for partner_group in &self.config.partner_groups {
            if groups.iter().any(|g| g == partner_group || g.contains(partner_group)) {
                return Some(UserScope::Partner);
            }
        }

        // Default: no specific scope suggestion (will default to CLIENT)
        None
    }
}

#[async_trait]
impl IdpAdapter for EntraIdAdapter {
    fn provider_type(&self) -> &'static str {
        "entra_id"
    }

    fn discovery_url(&self) -> String {
        format!(
            "{}/{}/v2.0/.well-known/openid-configuration",
            self.config.cloud.base_url(),
            self.config.tenant_id
        )
    }

    fn extract_user_info(&self, claims: &IdTokenClaims, role_config: &IdpRoleMappingConfig) -> IdpUserInfo {
        let groups = self.extract_groups(claims);
        let roles = self.extract_roles(claims);

        debug!(
            sub = %claims.sub,
            email = ?claims.email,
            groups_count = groups.len(),
            roles_count = roles.len(),
            "Extracting Entra ID user info"
        );

        // Apply role mappings
        let fc_roles = apply_role_mappings(&roles, &groups, role_config);

        // Determine scope
        let suggested_scope = self.determine_scope(&groups);

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
            // Include offline_access for refresh tokens
            "offline_access".to_string(),
        ]
    }
}

/// Extended claims specific to Azure AD
/// These can be extracted from the ID token if the application is configured to include them
#[derive(Debug, Clone, Deserialize)]
pub struct EntraIdExtendedClaims {
    /// User principal name (typically email format)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upn: Option<String>,
    /// Preferred username
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_username: Option<String>,
    /// Tenant ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tid: Option<String>,
    /// Object ID (user's unique ID in Azure AD)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oid: Option<String>,
    /// Group memberships (as GUIDs)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub groups: Option<Vec<String>>,
    /// App roles assigned to the user
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roles: Option<Vec<String>>,
    /// Indicates if groups claim is overage (too many groups)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "_claim_names")]
    pub claim_names: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discovery_url() {
        let adapter = EntraIdAdapter::for_tenant("my-tenant-id");
        assert_eq!(
            adapter.discovery_url(),
            "https://login.microsoftonline.com/my-tenant-id/v2.0/.well-known/openid-configuration"
        );
    }

    #[test]
    fn test_discovery_url_government() {
        let adapter = EntraIdAdapter::new(EntraIdConfig {
            tenant_id: "gov-tenant".to_string(),
            cloud: AzureCloud::Government,
            ..Default::default()
        });
        assert_eq!(
            adapter.discovery_url(),
            "https://login.microsoftonline.us/gov-tenant/v2.0/.well-known/openid-configuration"
        );
    }

    #[test]
    fn test_scope_determination() {
        let adapter = EntraIdAdapter::new(EntraIdConfig {
            tenant_id: "test".to_string(),
            anchor_groups: vec!["GlobalAdmins".to_string()],
            partner_groups: vec!["Partners".to_string()],
            ..Default::default()
        });

        // Anchor group match
        let groups = vec!["GlobalAdmins".to_string(), "Users".to_string()];
        assert_eq!(adapter.determine_scope(&groups), Some(UserScope::Anchor));

        // Partner group match
        let groups = vec!["Partners".to_string(), "Users".to_string()];
        assert_eq!(adapter.determine_scope(&groups), Some(UserScope::Partner));

        // No match
        let groups = vec!["Users".to_string()];
        assert_eq!(adapter.determine_scope(&groups), None);
    }
}
