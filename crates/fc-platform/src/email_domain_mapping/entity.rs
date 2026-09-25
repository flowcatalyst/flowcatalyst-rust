//! EmailDomainMapping Entity

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ScopeType {
    Anchor,
    Partner,
    Client,
}

crate::shared::enum_str::str_enum!(ScopeType, "scope type", {
    Anchor => "ANCHOR",
    Partner => "PARTNER",
    Client => "CLIENT",
});

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
    pub fn new(
        email_domain: impl Into<String>,
        identity_provider_id: impl Into<String>,
        scope_type: ScopeType,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: crate::shared::tsid::generate(crate::EntityType::EmailDomainMapping),
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

    /// Whether the mapping pins the OIDC tenant (`tid`) a login must come
    /// from. A blank value pins nothing.
    pub fn is_tenant_pinned(&self) -> bool {
        self.required_oidc_tenant_id
            .as_deref()
            .is_some_and(|t| !t.trim().is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_new_email_domain_mapping() {
        let edm = EmailDomainMapping::new("example.com", "idp-123", ScopeType::Anchor);

        assert!(!edm.id.is_empty());
        assert!(
            edm.id.starts_with("edm_"),
            "ID should have edm_ prefix, got: {}",
            edm.id
        );
        assert_eq!(edm.email_domain, "example.com");
        assert_eq!(edm.identity_provider_id, "idp-123");
        assert_eq!(edm.scope_type, ScopeType::Anchor);
        assert!(edm.primary_client_id.is_none());
        assert!(edm.additional_client_ids.is_empty());
        assert!(edm.granted_client_ids.is_empty());
        assert!(edm.required_oidc_tenant_id.is_none());
        assert!(edm.allowed_role_ids.is_empty());
        assert!(!edm.sync_roles_from_idp);
        assert_eq!(edm.created_at, edm.updated_at);
    }

    #[test]
    fn test_scope_type_as_str() {
        assert_eq!(ScopeType::Anchor.as_str(), "ANCHOR");
        assert_eq!(ScopeType::Partner.as_str(), "PARTNER");
        assert_eq!(ScopeType::Client.as_str(), "CLIENT");
    }

    #[test]
    fn test_scope_type_from_str() {
        assert_eq!(ScopeType::from_str("PARTNER"), Ok(ScopeType::Partner));
        assert_eq!(ScopeType::from_str("CLIENT"), Ok(ScopeType::Client));
        // Unknown values are rejected (X-06)
        assert_eq!(ScopeType::from_str("ANCHOR"), Ok(ScopeType::Anchor));
        assert!(ScopeType::from_str("unknown").is_err());
        assert!(ScopeType::from_str("").is_err());
    }

    #[test]
    fn test_scope_type_serialization() {
        let json = serde_json::to_string(&ScopeType::Anchor).unwrap();
        assert_eq!(json, "\"ANCHOR\"");

        let json = serde_json::to_string(&ScopeType::Partner).unwrap();
        assert_eq!(json, "\"PARTNER\"");

        let json = serde_json::to_string(&ScopeType::Client).unwrap();
        assert_eq!(json, "\"CLIENT\"");
    }

    #[test]
    fn test_scope_type_deserialization() {
        let anchor: ScopeType = serde_json::from_str("\"ANCHOR\"").unwrap();
        assert_eq!(anchor, ScopeType::Anchor);

        let partner: ScopeType = serde_json::from_str("\"PARTNER\"").unwrap();
        assert_eq!(partner, ScopeType::Partner);

        let client: ScopeType = serde_json::from_str("\"CLIENT\"").unwrap();
        assert_eq!(client, ScopeType::Client);
    }

    #[test]
    fn test_email_domain_mapping_unique_ids() {
        let edm1 = EmailDomainMapping::new("a.com", "idp-1", ScopeType::Anchor);
        let edm2 = EmailDomainMapping::new("b.com", "idp-2", ScopeType::Client);
        assert_ne!(edm1.id, edm2.id);
    }

    #[test]
    fn test_email_domain_mapping_serialization() {
        let edm = EmailDomainMapping::new("test.org", "idp-1", ScopeType::Partner);

        let json = serde_json::to_string(&edm).unwrap();
        assert!(json.contains("emailDomain"));
        assert!(json.contains("test.org"));
        assert!(json.contains("identityProviderId"));
        assert!(json.contains("idp-1"));
        assert!(json.contains("PARTNER"));
        assert!(json.contains("syncRolesFromIdp"));
    }
}
