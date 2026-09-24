//! IdentityProvider Entity

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IdentityProviderType {
    Internal,
    Oidc,
}

crate::shared::enum_str::str_enum!(IdentityProviderType, "identity provider type", {
    Internal => "INTERNAL",
    Oidc => "OIDC",
});

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
    pub fn new(
        code: impl Into<String>,
        name: impl Into<String>,
        idp_type: IdentityProviderType,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: crate::shared::tsid::generate(crate::EntityType::IdentityProvider),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_new_internal_identity_provider() {
        let idp = IdentityProvider::new(
            "internal",
            "Internal Provider",
            IdentityProviderType::Internal,
        );

        assert!(!idp.id.is_empty());
        assert!(
            idp.id.starts_with("idp_"),
            "ID should have idp_ prefix, got: {}",
            idp.id
        );
        assert_eq!(idp.code, "internal");
        assert_eq!(idp.name, "Internal Provider");
        assert_eq!(idp.r#type, IdentityProviderType::Internal);
        assert!(idp.oidc_issuer_url.is_none());
        assert!(idp.oidc_client_id.is_none());
        assert!(idp.oidc_client_secret_ref.is_none());
        assert!(!idp.oidc_multi_tenant);
        assert!(idp.oidc_issuer_pattern.is_none());
        assert!(idp.allowed_email_domains.is_empty());
        assert_eq!(idp.created_at, idp.updated_at);
    }

    #[test]
    fn test_new_oidc_identity_provider() {
        let idp = IdentityProvider::new("azure-ad", "Azure AD", IdentityProviderType::Oidc);

        assert_eq!(idp.code, "azure-ad");
        assert_eq!(idp.name, "Azure AD");
        assert_eq!(idp.r#type, IdentityProviderType::Oidc);
    }

    #[test]
    fn test_has_client_secret() {
        let mut idp = IdentityProvider::new("test", "Test", IdentityProviderType::Oidc);
        assert!(!idp.has_client_secret());

        idp.oidc_client_secret_ref = Some("secret-ref-123".to_string());
        assert!(idp.has_client_secret());
    }

    #[test]
    fn test_identity_provider_type_as_str() {
        assert_eq!(IdentityProviderType::Internal.as_str(), "INTERNAL");
        assert_eq!(IdentityProviderType::Oidc.as_str(), "OIDC");
    }

    #[test]
    fn test_identity_provider_type_from_str() {
        assert_eq!(
            IdentityProviderType::from_str("OIDC"),
            Ok(IdentityProviderType::Oidc)
        );
        assert_eq!(
            IdentityProviderType::from_str("INTERNAL"),
            Ok(IdentityProviderType::Internal)
        );
        // Unknown values are rejected (X-06)
        assert!(IdentityProviderType::from_str("unknown").is_err());
        assert!(IdentityProviderType::from_str("").is_err());
    }

    #[test]
    fn test_identity_provider_type_serialization() {
        let json = serde_json::to_string(&IdentityProviderType::Internal).unwrap();
        assert_eq!(json, "\"INTERNAL\"");

        let json = serde_json::to_string(&IdentityProviderType::Oidc).unwrap();
        assert_eq!(json, "\"OIDC\"");
    }

    #[test]
    fn test_identity_provider_unique_ids() {
        let idp1 = IdentityProvider::new("a", "A", IdentityProviderType::Internal);
        let idp2 = IdentityProvider::new("b", "B", IdentityProviderType::Oidc);
        assert_ne!(idp1.id, idp2.id);
    }

    #[test]
    fn test_identity_provider_serialization() {
        let idp = IdentityProvider::new("okta", "Okta SSO", IdentityProviderType::Oidc);

        let json = serde_json::to_string(&idp).unwrap();
        assert!(json.contains("\"code\":\"okta\""));
        assert!(json.contains("Okta SSO"));
        assert!(json.contains("OIDC"));
        assert!(json.contains("oidcMultiTenant"));
    }
}
