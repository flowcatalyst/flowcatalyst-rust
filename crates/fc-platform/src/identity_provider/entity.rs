//! IdentityProvider Entity

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IdentityProviderType {
    Internal,
    Oidc,
}

impl IdentityProviderType {
    pub fn as_str(&self) -> &'static str {
        match self { Self::Internal => "INTERNAL", Self::Oidc => "OIDC" }
    }
    pub fn from_str(s: &str) -> Self {
        match s { "OIDC" => Self::Oidc, _ => Self::Internal }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityProvider {
    pub id: String,
    pub code: String,
    pub name: String,
    pub r#type: IdentityProviderType,
    pub oidc_issuer_url: Option<String>,
    pub oidc_client_id: Option<String>,
    pub oidc_client_secret_ref: Option<String>,
    pub oidc_multi_tenant: bool,
    pub oidc_issuer_pattern: Option<String>,
    pub allowed_email_domains: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl IdentityProvider {
    pub fn new(code: impl Into<String>, name: impl Into<String>, idp_type: IdentityProviderType) -> Self {
        let now = Utc::now();
        Self {
            id: crate::TsidGenerator::generate(crate::EntityType::IdentityProvider),
            code: code.into(),
            name: name.into(),
            r#type: idp_type,
            oidc_issuer_url: None,
            oidc_client_id: None,
            oidc_client_secret_ref: None,
            oidc_multi_tenant: false,
            oidc_issuer_pattern: None,
            allowed_email_domains: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }

    pub fn has_client_secret(&self) -> bool {
        self.oidc_client_secret_ref.is_some()
    }
}

impl From<crate::entities::oauth_identity_providers::Model> for IdentityProvider {
    fn from(m: crate::entities::oauth_identity_providers::Model) -> Self {
        Self {
            id: m.id,
            code: m.code,
            name: m.name,
            r#type: IdentityProviderType::from_str(&m.r#type),
            oidc_issuer_url: m.oidc_issuer_url,
            oidc_client_id: m.oidc_client_id,
            oidc_client_secret_ref: m.oidc_client_secret_ref,
            oidc_multi_tenant: m.oidc_multi_tenant,
            oidc_issuer_pattern: m.oidc_issuer_pattern,
            allowed_email_domains: Vec::new(), // loaded separately
            created_at: m.created_at.with_timezone(&Utc),
            updated_at: m.updated_at.with_timezone(&Utc),
        }
    }
}
