//! Go's create-time rules for an email-domain mapping
//! (`emaildomainmapping/operations/create.go` Validate), kept apart from
//! `create.rs` so the 2FA additions there merge cleanly.

use crate::email_domain_mapping::entity::ScopeType;
use crate::usecase::UseCaseError;

/// Go's DNS-name check: a dot, and none of space, `/`, `@`.
pub fn validate_domain(domain: &str) -> Result<(), UseCaseError> {
    if !domain.contains('.') || domain.contains([' ', '/', '@']) {
        return Err(UseCaseError::validation(
            "INVALID_EMAIL_DOMAIN",
            "Email domain must be a valid DNS name (e.g. example.com)",
        ));
    }
    Ok(())
}

/// A PARTNER or CLIENT mapping names its primary client.
pub fn validate_scope(
    scope_type: ScopeType,
    primary_client_id: Option<&str>,
) -> Result<(), UseCaseError> {
    if matches!(scope_type, ScopeType::Partner | ScopeType::Client) && primary_client_id.is_none() {
        return Err(UseCaseError::validation(
            "PRIMARY_CLIENT_REQUIRED",
            "primaryClientId is required for PARTNER and CLIENT scope",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_domain_is_a_dns_name() {
        assert!(validate_domain("example.com").is_ok());
        assert!(validate_domain("nodot").is_err());
        assert!(validate_domain("a b.com").is_err());
        assert!(validate_domain("x@y.com").is_err());
    }

    #[test]
    fn partner_and_client_need_a_primary_client() {
        assert!(validate_scope(ScopeType::Anchor, None).is_ok());
        assert!(validate_scope(ScopeType::Client, None).is_err());
        assert!(validate_scope(ScopeType::Partner, Some("clt_1")).is_ok());
    }
}
