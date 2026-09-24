//! Authentication Configuration Entities — matches TypeScript domain

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[derive(Default)]
pub enum AuthProvider {
    #[default]
    Internal,
    Oidc,
}

impl AuthProvider {
    pub fn from_str(s: &str) -> Self {
        match s {
            "OIDC" => Self::Oidc,
            _ => Self::Internal,
        }
    }
}

crate::shared::enum_str::str_enum!(AuthProvider, "auth provider", {
    Internal => "INTERNAL",
    Oidc => "OIDC",
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[derive(Default)]
pub enum AuthConfigType {
    Anchor,
    Partner,
    #[default]
    Client,
}

impl AuthConfigType {
    pub fn from_str(s: &str) -> Self {
        match s {
            "ANCHOR" => Self::Anchor,
            "PARTNER" => Self::Partner,
            _ => Self::Client,
        }
    }
}

crate::shared::enum_str::str_enum!(AuthConfigType, "auth config type", {
    Anchor => "ANCHOR",
    Partner => "PARTNER",
    Client => "CLIENT",
});

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
            id: crate::shared::tsid::generate(crate::EntityType::AnchorDomain),
            domain: domain.into().to_lowercase(),
            created_at: now,
            updated_at: now,
        }
    }

    pub fn matches_email(&self, email: &str) -> bool {
        email.to_lowercase().ends_with(&format!("@{}", self.domain))
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
    pub fn new_internal(email_domain: impl Into<String>, config_type: AuthConfigType) -> Self {
        let now = Utc::now();
        Self {
            id: crate::shared::tsid::generate(crate::EntityType::ClientAuthConfig),
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

    pub fn with_oidc(
        mut self,
        issuer_url: impl Into<String>,
        client_id: impl Into<String>,
    ) -> Self {
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
            id: crate::shared::tsid::generate(crate::EntityType::IdpRoleMapping),
            idp_type: idp_type.into(),
            idp_role_name: idp_role_name.into(),
            platform_role_name: platform_role_name.into(),
            created_at: now,
            updated_at: now,
        }
    }
}
