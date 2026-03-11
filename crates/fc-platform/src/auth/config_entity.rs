//! Authentication Configuration Entities — matches TypeScript domain

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AuthProvider {
    Internal,
    Oidc,
}

impl Default for AuthProvider {
    fn default() -> Self { Self::Internal }
}

impl AuthProvider {
    pub fn as_str(&self) -> &'static str {
        match self { Self::Internal => "INTERNAL", Self::Oidc => "OIDC" }
    }
    pub fn from_str(s: &str) -> Self {
        match s { "OIDC" => Self::Oidc, _ => Self::Internal }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AuthConfigType {
    Anchor,
    Partner,
    Client,
}

impl Default for AuthConfigType {
    fn default() -> Self { Self::Client }
}

impl AuthConfigType {
    pub fn as_str(&self) -> &'static str {
        match self { Self::Anchor => "ANCHOR", Self::Partner => "PARTNER", Self::Client => "CLIENT" }
    }
    pub fn from_str(s: &str) -> Self {
        match s { "ANCHOR" => Self::Anchor, "PARTNER" => Self::Partner, _ => Self::Client }
    }
}

/// AnchorDomain — matches TypeScript AnchorDomain interface
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnchorDomain {
    pub id: String,
    pub domain: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl AnchorDomain {
    pub fn new(domain: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: crate::TsidGenerator::generate(crate::EntityType::AnchorDomain),
            domain: domain.into().to_lowercase(),
            created_at: now,
            updated_at: now,
        }
    }

    pub fn matches_email(&self, email: &str) -> bool {
        email.to_lowercase().ends_with(&format!("@{}", self.domain))
    }
}

impl From<crate::entities::tnt_anchor_domains::Model> for AnchorDomain {
    fn from(m: crate::entities::tnt_anchor_domains::Model) -> Self {
        Self {
            id: m.id,
            domain: m.domain,
            created_at: m.created_at.with_timezone(&Utc),
            updated_at: m.updated_at.with_timezone(&Utc),
        }
    }
}

/// ClientAuthConfig — matches TypeScript ClientAuthConfig interface
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientAuthConfig {
    pub id: String,
    pub email_domain: String,
    pub config_type: AuthConfigType,
    pub primary_client_id: Option<String>,
    pub additional_client_ids: Vec<String>,
    pub granted_client_ids: Vec<String>,
    pub auth_provider: AuthProvider,
    pub oidc_issuer_url: Option<String>,
    pub oidc_client_id: Option<String>,
    pub oidc_multi_tenant: bool,
    pub oidc_issuer_pattern: Option<String>,
    pub oidc_client_secret_ref: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ClientAuthConfig {
    pub fn new_internal(
        email_domain: impl Into<String>,
        config_type: AuthConfigType,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: crate::TsidGenerator::generate(crate::EntityType::ClientAuthConfig),
            email_domain: email_domain.into().to_lowercase(),
            config_type,
            primary_client_id: None,
            additional_client_ids: vec![],
            granted_client_ids: vec![],
            auth_provider: AuthProvider::Internal,
            oidc_issuer_url: None,
            oidc_client_id: None,
            oidc_multi_tenant: false,
            oidc_issuer_pattern: None,
            oidc_client_secret_ref: None,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn new_partner(email_domain: impl Into<String>) -> Self {
        Self::new_internal(email_domain, AuthConfigType::Partner)
    }

    pub fn new_client(email_domain: impl Into<String>, client_id: impl Into<String>) -> Self {
        let mut config = Self::new_internal(email_domain, AuthConfigType::Client);
        config.primary_client_id = Some(client_id.into());
        config
    }

    pub fn with_oidc(mut self, issuer_url: impl Into<String>, client_id: impl Into<String>) -> Self {
        self.auth_provider = AuthProvider::Oidc;
        self.oidc_issuer_url = Some(issuer_url.into());
        self.oidc_client_id = Some(client_id.into());
        self
    }

    /// Check if the given OIDC issuer URL is valid for this config
    pub fn is_valid_issuer(&self, issuer: &str) -> bool {
        if let Some(ref issuer_url) = self.oidc_issuer_url {
            if issuer_url == issuer {
                return true;
            }
        }
        if let Some(ref pattern) = self.oidc_issuer_pattern {
            if let Ok(re) = regex::Regex::new(pattern) {
                return re.is_match(issuer);
            }
        }
        false
    }

    /// Get all accessible client IDs for this config
    pub fn accessible_clients(&self) -> Vec<String> {
        let mut clients = Vec::new();
        if let Some(ref primary) = self.primary_client_id {
            clients.push(primary.clone());
        }
        clients.extend(self.additional_client_ids.iter().cloned());
        clients.extend(self.granted_client_ids.iter().cloned());
        clients
    }
}

/// IDP Role Mapping — maps external IDP role names to internal platform roles
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdpRoleMapping {
    pub id: String,
    pub idp_type: String,
    pub idp_role_name: String,
    pub platform_role_name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl IdpRoleMapping {
    pub fn new(
        idp_type: impl Into<String>,
        idp_role_name: impl Into<String>,
        platform_role_name: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: crate::TsidGenerator::generate(crate::EntityType::IdpRoleMapping),
            idp_type: idp_type.into(),
            idp_role_name: idp_role_name.into(),
            platform_role_name: platform_role_name.into(),
            created_at: now,
            updated_at: now,
        }
    }
}

impl From<crate::entities::oauth_idp_role_mappings::Model> for IdpRoleMapping {
    fn from(m: crate::entities::oauth_idp_role_mappings::Model) -> Self {
        Self {
            id: m.id,
            idp_type: "OIDC".to_string(), // DB table doesn't store idp_type separately
            idp_role_name: m.idp_role_name,
            platform_role_name: m.internal_role_name,
            created_at: m.created_at.with_timezone(&Utc),
            updated_at: m.updated_at.with_timezone(&Utc),
        }
    }
}

impl From<crate::entities::tnt_client_auth_configs::Model> for ClientAuthConfig {
    fn from(m: crate::entities::tnt_client_auth_configs::Model) -> Self {
        let additional_client_ids: Vec<String> =
            serde_json::from_value(m.additional_client_ids.into()).unwrap_or_default();
        let granted_client_ids: Vec<String> =
            serde_json::from_value(m.granted_client_ids.into()).unwrap_or_default();
        Self {
            id: m.id,
            email_domain: m.email_domain,
            config_type: AuthConfigType::from_str(&m.config_type),
            primary_client_id: m.primary_client_id,
            additional_client_ids,
            granted_client_ids,
            auth_provider: AuthProvider::from_str(&m.auth_provider),
            oidc_issuer_url: m.oidc_issuer_url,
            oidc_client_id: m.oidc_client_id,
            oidc_multi_tenant: m.oidc_multi_tenant,
            oidc_issuer_pattern: m.oidc_issuer_pattern,
            oidc_client_secret_ref: m.oidc_client_secret_ref,
            created_at: m.created_at.with_timezone(&Utc),
            updated_at: m.updated_at.with_timezone(&Utc),
        }
    }
}
