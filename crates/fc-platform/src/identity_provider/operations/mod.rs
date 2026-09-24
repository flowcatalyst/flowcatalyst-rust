//! Identity Provider Operations
//!
//! Use cases for managing identity providers.

pub mod create;
pub mod delete;
pub mod events;
pub mod update;

pub use create::{CreateIdentityProviderCommand, CreateIdentityProviderUseCase};
pub use delete::{DeleteIdentityProviderCommand, DeleteIdentityProviderUseCase};
pub use events::{IdentityProviderCreated, IdentityProviderDeleted, IdentityProviderUpdated};
pub use update::{UpdateIdentityProviderCommand, UpdateIdentityProviderUseCase};

use crate::usecase::UseCaseError;

/// The command carries the client secret in its stored form: the handler
/// encrypts it before building the command, so a plaintext value here means a
/// caller skipped that step. Refuse it rather than store plaintext.
fn require_sealed_secret(secret_ref: Option<&str>) -> Result<(), UseCaseError> {
    match secret_ref {
        Some(s) if !crate::shared::encryption_service::is_encrypted_ref(s) => {
            Err(UseCaseError::validation(
                "CLIENT_SECRET_NOT_ENCRYPTED",
                "OIDC client secret must be encrypted before it is stored",
            ))
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::require_sealed_secret;

    #[test]
    fn plaintext_secret_in_command_is_refused() {
        let err = require_sealed_secret(Some("plain")).unwrap_err();
        assert_eq!(err.code(), "CLIENT_SECRET_NOT_ENCRYPTED");
        assert!(require_sealed_secret(Some("encrypted:abc")).is_ok());
        assert!(require_sealed_secret(None).is_ok());
    }
}
