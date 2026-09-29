//! Principal Operations
//!
//! Use cases for user (principal) management.

use fc_platform_core::shared::id::ClientId;
use fc_platform_core::usecase::UseCaseError;

/// A client id named in a command: a malformed one is the caller's mistake, a
/// 400 with its own code, not a database foreign-key failure.
pub(crate) fn parse_client_id(raw: &str) -> Result<ClientId, UseCaseError> {
    ClientId::parse(raw.trim())
        .map_err(|e| UseCaseError::validation("INVALID_CLIENT_ID", e.to_string()))
}

pub mod access;
pub mod activate;
pub mod assign_application_access;
pub mod assign_roles;
pub mod create;
pub mod deactivate;
pub mod delete;
pub mod events;
pub mod grant_client_access;
pub mod reset_password;
pub mod revoke_client_access;
pub mod set_client_association;
pub mod sync;
pub mod sync_users;
pub mod update;

pub use activate::{ActivateUserCommand, ActivateUserUseCase};
pub use assign_application_access::{
    AssignApplicationAccessCommand, AssignApplicationAccessUseCase,
};
pub use assign_roles::{AssignUserRolesCommand, AssignUserRolesUseCase};
pub use create::{CreateUserCommand, CreateUserUseCase};
pub use deactivate::{DeactivateUserCommand, DeactivateUserUseCase};
pub use delete::{DeleteUserCommand, DeleteUserUseCase};
pub use events::*;
pub use grant_client_access::{GrantClientAccessCommand, GrantClientAccessUseCase};
pub use reset_password::{ResetPasswordCommand, ResetPasswordUseCase};
pub use revoke_client_access::{RevokeClientAccessCommand, RevokeClientAccessUseCase};
pub use sync::{SyncPrincipalInput, SyncPrincipalsCommand, SyncPrincipalsUseCase};
pub use sync_users::{password_hashes_ignored, SyncUserInput, SyncUsersCommand, SyncUsersUseCase};
pub use update::{UpdateUserCommand, UpdateUserUseCase};
