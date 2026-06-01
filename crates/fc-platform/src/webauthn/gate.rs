//! Principal gate — passkeys are only available for internal-auth principals.
//!
//! A principal is "internal-auth" iff they have a local password hash AND no
//! external IdP subject (`user_identity.password_hash.is_some()` AND
//! `user_identity.external_id.is_none()`). The check is per-principal, NOT
//! per-domain — a single email domain can have a mix of local accounts and
//! federated accounts (the domain mapping drives where to send *new* users
//! who don't have a local row, but it doesn't determine eligibility for an
//! existing principal).
//!
//! Federated principals MUST NOT be issued passkey challenges or have
//! credentials returned to them — the IdP owns identity. The check is
//! enforced at both `begin` handlers (refuse to issue a challenge) and
//! inside `AuthenticatePasskeyUseCase` (hard cutover at auth time, in case
//! a principal flipped from internal to federated after registering a
//! passkey).
//!
//! Enumeration safety: the callers turn `Err(_)` from this gate into the
//! same "no credentials available" envelope as a genuinely unknown user
//! (see `resolve_real_credentials` in `api.rs`), so the exact error string
//! returned here never reaches the wire.

use crate::principal::repository::PrincipalRepository;
use crate::shared::error::{PlatformError, Result};

/// Returns `Ok(())` iff the principal identified by `email` exists, is
/// active, has a local password hash, and is NOT linked to an external IdP.
/// Any other state returns `BadRequest` with a generic message.
pub async fn ensure_internal_principal(email: &str, repo: &PrincipalRepository) -> Result<()> {
    let Some(principal) = repo.find_by_email(email).await? else {
        // Unknown email — same envelope as the federated case so callers
        // can't distinguish "no such user" from "federated user" from
        // "user has no passkey". The `authenticate/begin` flow turns this
        // into a synthesized random challenge for enumeration safety.
        return Err(PlatformError::bad_request(
            "passkeys are not available for this account".to_string(),
        ));
    };
    let Some(identity) = principal.user_identity.as_ref() else {
        // Service-account or otherwise non-user principal — no passkey path.
        return Err(PlatformError::bad_request(
            "passkeys are not available for this account".to_string(),
        ));
    };
    if identity.external_id.is_some() || identity.password_hash.is_none() {
        return Err(PlatformError::bad_request(
            "passkeys are not available for this account".to_string(),
        ));
    }
    Ok(())
}
