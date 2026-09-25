//! A domain's second-factor stance (Go `auth/twofa/policy.go`), shared by
//! the login, self-service and password-reset flows so they agree.

use std::sync::Arc;

use crate::email_domain_mapping::entity::EmailDomainMapping;
use crate::identity_provider::entity::IdentityProviderType;
use crate::{EmailDomainMappingRepository, IdentityProviderRepository};

/// Resolves the email-domain mapping and its identity provider's type.
#[derive(Clone)]
pub struct TwoFactorPolicy {
    pub mappings: Arc<EmailDomainMappingRepository>,
    pub identity_providers: Arc<IdentityProviderRepository>,
}

/// The stance for one address's domain.
#[derive(Debug, Clone, Default)]
pub struct Eval {
    /// The mapping; none when the domain is unmapped.
    pub mapping: Option<EmailDomainMapping>,
    /// The domain authenticates internally (unmapped, or not an OIDC
    /// provider). 2FA only ever applies to internal domains.
    pub internal: bool,
}

impl TwoFactorPolicy {
    /// An unmapped domain (or a failed lookup) reads as internal with no
    /// policy, as Go's `Evaluate`.
    pub async fn evaluate(&self, email: &str) -> Eval {
        let Some(domain) = email
            .split_once('@')
            .map(|(_, d)| d.to_lowercase())
            .filter(|d| !d.is_empty())
        else {
            return Eval {
                mapping: None,
                internal: true,
            };
        };
        let Ok(Some(mapping)) = self.mappings.find_by_email_domain(&domain).await else {
            return Eval {
                mapping: None,
                internal: true,
            };
        };
        let internal = match self
            .identity_providers
            .find_by_id(&mapping.identity_provider_id)
            .await
        {
            Ok(Some(idp)) => idp.r#type != IdentityProviderType::Oidc,
            _ => true,
        };
        Eval {
            mapping: Some(mapping),
            internal,
        }
    }
}

impl Eval {
    /// The domain compels a second factor for password sign-in.
    pub fn requires_2fa(&self) -> bool {
        self.internal && self.mapping.as_ref().is_some_and(|m| m.require_2fa)
    }

    /// The permitted second factors (empty when unmapped).
    pub fn allowed_methods(&self) -> Vec<String> {
        self.mapping
            .as_ref()
            .map(|m| m.allowed_2fa_methods.clone())
            .unwrap_or_default()
    }

    /// The domain offers remember-this-device.
    pub fn remember_enabled(&self) -> bool {
        self.internal
            && self
                .mapping
                .as_ref()
                .is_some_and(|m| m.remember_device_enabled)
    }

    /// Remember-device lifetime in days (30 by default).
    pub fn remember_days(&self) -> i64 {
        match self.mapping.as_ref().map(|m| m.remember_device_days) {
            Some(d) if d > 0 => i64::from(d),
            _ => 30,
        }
    }

    /// Whether the user may use `method`: always, unless the domain
    /// requires 2FA with an allow-list (Go `methodAllowed`).
    pub fn method_allowed(&self, method: &str) -> bool {
        if !self.requires_2fa() {
            return true;
        }
        self.allowed_methods().iter().any(|m| m == method)
    }
}
