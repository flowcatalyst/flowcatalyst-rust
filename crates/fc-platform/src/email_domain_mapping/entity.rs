//! EmailDomainMapping Entity

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ScopeType {
    Anchor,
    Partner,
    Client,
}

impl ScopeType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Anchor => "ANCHOR",
            Self::Partner => "PARTNER",
            Self::Client => "CLIENT",
        }
    }
    pub fn from_str(s: &str) -> Self {
        match s {
            "PARTNER" => Self::Partner,
            "CLIENT" => Self::Client,
            _ => Self::Anchor,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailDomainMapping {
    pub id: String,
    pub email_domain: String,
    pub identity_provider_id: String,
    pub scope_type: ScopeType,
    pub primary_client_id: Option<String>,
    pub additional_client_ids: Vec<String>,
    pub granted_client_ids: Vec<String>,
    pub required_oidc_tenant_id: Option<String>,
    pub allowed_role_ids: Vec<String>,
    pub sync_roles_from_idp: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl EmailDomainMapping {
    pub fn new(email_domain: impl Into<String>, identity_provider_id: impl Into<String>, scope_type: ScopeType) -> Self {
        let now = Utc::now();
        Self {
            id: crate::TsidGenerator::generate(crate::EntityType::EmailDomainMapping),
            email_domain: email_domain.into(),
            identity_provider_id: identity_provider_id.into(),
            scope_type,
            primary_client_id: None,
            additional_client_ids: Vec::new(),
            granted_client_ids: Vec::new(),
            required_oidc_tenant_id: None,
            allowed_role_ids: Vec::new(),
            sync_roles_from_idp: false,
            created_at: now,
            updated_at: now,
        }
    }
}

impl From<crate::entities::tnt_email_domain_mappings::Model> for EmailDomainMapping {
    fn from(m: crate::entities::tnt_email_domain_mappings::Model) -> Self {
        Self {
            id: m.id,
            email_domain: m.email_domain,
            identity_provider_id: m.identity_provider_id,
            scope_type: ScopeType::from_str(&m.scope_type),
            primary_client_id: m.primary_client_id,
            additional_client_ids: Vec::new(), // loaded separately
            granted_client_ids: Vec::new(),    // loaded separately
            required_oidc_tenant_id: m.required_oidc_tenant_id,
            allowed_role_ids: Vec::new(),      // loaded separately
            sync_roles_from_idp: m.sync_roles_from_idp,
            created_at: m.created_at.with_timezone(&Utc),
            updated_at: m.updated_at.with_timezone(&Utc),
        }
    }
}
