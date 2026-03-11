//! Principal Operations
//!
//! Use cases for user (principal) management.

pub mod events;
pub mod create;
pub mod update;
pub mod activate;
pub mod deactivate;
pub mod delete;
pub mod assign_roles;
pub mod grant_client_access;
pub mod revoke_client_access;
pub mod sync;
pub mod assign_application_access;

pub use events::*;
pub use create::{CreateUserCommand, CreateUserUseCase};
pub use update::{UpdateUserCommand, UpdateUserUseCase};
pub use activate::{ActivateUserCommand, ActivateUserUseCase};
pub use deactivate::{DeactivateUserCommand, DeactivateUserUseCase};
pub use delete::{DeleteUserCommand, DeleteUserUseCase};
pub use assign_roles::{AssignUserRolesCommand, AssignUserRolesUseCase};
pub use grant_client_access::{GrantClientAccessCommand, GrantClientAccessUseCase};
pub use revoke_client_access::{RevokeClientAccessCommand, RevokeClientAccessUseCase};
pub use sync::{SyncPrincipalsCommand, SyncPrincipalsUseCase, SyncPrincipalInput};
pub use assign_application_access::{AssignApplicationAccessCommand, AssignApplicationAccessUseCase};
