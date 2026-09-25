//! Email Domain Mapping Operations
//!
//! Use cases for managing email domain mappings.

pub mod create;
pub mod create_rules;
pub mod delete;
pub mod events;
pub mod move_provider;
pub mod update;

pub use create::{CreateEmailDomainMappingCommand, CreateEmailDomainMappingUseCase};
pub use delete::{DeleteEmailDomainMappingCommand, DeleteEmailDomainMappingUseCase};
pub use events::*;
pub use update::{UpdateEmailDomainMappingCommand, UpdateEmailDomainMappingUseCase};

use crate::usecase::UseCaseError;

/// Owner ruling 2026-09-25 (item 3; Java ecb622fe): a mapping routed to a
/// multi-tenant OIDC provider must pin the tenant. Entra's multi-tenant keys
/// sign tokens for any tenant and its `email` claim is settable by any
/// tenant admin, so an unpinned mapping lets any tenant sign in as an
/// existing user of the domain. Single-tenant providers need no pin.
pub fn require_tenant_pin(
    idp_multi_tenant: bool,
    mapping: &crate::email_domain_mapping::entity::EmailDomainMapping,
) -> Result<(), UseCaseError> {
    if idp_multi_tenant && !mapping.is_tenant_pinned() {
        return Err(UseCaseError::validation(
            "TENANT_PIN_REQUIRED",
            format!(
                "Email domain '{}' routes to a multi-tenant identity provider and must set requiredOidcTenantId",
                mapping.email_domain
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::email_domain_mapping::entity::{EmailDomainMapping, ScopeType};

    #[test]
    fn a_multi_tenant_mapping_must_pin_the_tenant() {
        let mut m = EmailDomainMapping::new("acme.test", "idp_1", ScopeType::Client);
        assert!(
            require_tenant_pin(false, &m).is_ok(),
            "single-tenant needs none"
        );
        for blank in [None, Some(""), Some("   ")] {
            m.required_oidc_tenant_id = blank.map(String::from);
            let err = require_tenant_pin(true, &m).unwrap_err();
            assert_eq!(err.code(), "TENANT_PIN_REQUIRED", "{blank:?}");
        }
        m.required_oidc_tenant_id = Some("tenant-a".to_string());
        assert!(require_tenant_pin(true, &m).is_ok());
    }
}
