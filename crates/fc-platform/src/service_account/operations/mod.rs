//! Service Account Operations
//!
//! Use cases for service account management following the Command pattern
//! with guaranteed event emission and audit logging through UnitOfWork.

pub mod assign_roles;
pub mod client_reach;
pub mod create;
pub mod deactivate;
pub mod delete;
pub mod events;
pub mod mint_token;
pub mod regenerate_secret;
pub mod regenerate_token;
pub mod update;

// Re-export events
pub use events::{
    ServiceAccountCreated, ServiceAccountDeactivated, ServiceAccountDeleted,
    ServiceAccountRolesAssigned, ServiceAccountSecretRegenerated, ServiceAccountTokenRegenerated,
    ServiceAccountUpdated,
};

// Re-export commands and use cases
pub use create::{
    CreateServiceAccountCommand, CreateServiceAccountResult, CreateServiceAccountUseCase,
};

pub use update::{UpdateServiceAccountCommand, UpdateServiceAccountUseCase};

pub use deactivate::{DeactivateServiceAccountCommand, DeactivateServiceAccountUseCase};

pub use delete::{DeleteServiceAccountCommand, DeleteServiceAccountUseCase};

pub use assign_roles::{AssignRolesCommand, AssignRolesUseCase};

pub use regenerate_token::{
    RegenerateAuthTokenCommand, RegenerateAuthTokenResult, RegenerateAuthTokenUseCase,
};

pub use regenerate_secret::{
    RegenerateSigningSecretCommand, RegenerateSigningSecretResult, RegenerateSigningSecretUseCase,
};
