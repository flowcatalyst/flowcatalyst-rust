//! Domain gate — passkeys are only available for internal-auth principals.
//!
//! A domain is federated when it is mapped to an OIDC (external) identity
//! provider. A mapping to an INTERNAL provider does not federate it: Go's
//! `fcdev init` maps the anchor domain to an INTERNAL provider, and Go never
//! refuses those users passkeys (Go gates password login only on OIDC
//! providers, auth/login/endpoint.go `SSO_REQUIRED`). Federated
//! domains MUST NOT be issued passkey challenges or have credentials
//! returned to them — the IdP owns identity. The check is enforced both at
//! the `begin` handlers (refuse to issue a challenge) and inside the
//! `AuthenticatePasskeyUseCase` (hard cutover at auth time).

use crate::email_domain_mapping::repository::EmailDomainMappingRepository;
use crate::shared::error::{PlatformError, Result};

pub fn extract_domain(email: &str) -> Result<String> {
    email
        .split('@')
        .nth(1)
        .filter(|d| !d.is_empty())
        .map(|d| d.to_lowercase())
        .ok_or_else(|| {
            PlatformError::bad_request("email is not in a valid 'local@domain' format".to_string())
        })
}

/// Returns `Ok(())` if the domain is internal (unmapped, or mapped to an
/// INTERNAL provider). Returns `BadRequest` if the domain maps to an OIDC
/// IdP — callers should
/// surface a generic enumeration-safe response, not the underlying reason
/// (see `enumeration_defence.rs`).
pub async fn ensure_internal_auth(email: &str, repo: &EmailDomainMappingRepository) -> Result<()> {
    let domain = extract_domain(email)?;
    if repo.is_federated_domain(&domain).await? {
        return Err(PlatformError::bad_request(
            "passkeys are not available for this domain".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_domain_lowercases() {
        assert_eq!(extract_domain("Alice@Example.COM").unwrap(), "example.com");
    }

    #[test]
    fn extract_domain_rejects_no_at() {
        assert!(extract_domain("alice").is_err());
    }

    #[test]
    fn extract_domain_rejects_empty_domain() {
        assert!(extract_domain("alice@").is_err());
    }

    #[test]
    fn extract_domain_takes_first_at_split() {
        // RFC-illegal but defensive: only the part after the first '@' counts.
        assert_eq!(extract_domain("alice@example.com").unwrap(), "example.com");
    }
}
