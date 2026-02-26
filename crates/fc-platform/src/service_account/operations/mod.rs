//! Service Account Operations
//!
//! Use cases for service account management following the Command pattern
//! with guaranteed event emission and audit logging through UnitOfWork.

pub mod events;
pub mod create;
pub mod update;
pub mod delete;
pub mod assign_roles;
pub mod regenerate_token;
pub mod regenerate_secret;

// Re-export events
pub use events::{
    ServiceAccountCreated,
    ServiceAccountUpdated,
    ServiceAccountDeleted,
    ServiceAccountRolesAssigned,
    ServiceAccountTokenRegenerated,
    ServiceAccountSecretRegenerated,
};

// Re-export commands and use cases
pub use create::{
    CreateServiceAccountCommand,
    CreateServiceAccountUseCase,
    CreateServiceAccountResult,
};

pub use update::{
    UpdateServiceAccountCommand,
    UpdateServiceAccountUseCase,
};

pub use delete::{
    DeleteServiceAccountCommand,
    DeleteServiceAccountUseCase,
};

pub use assign_roles::{
    AssignRolesCommand,
    AssignRolesUseCase,
};

pub use regenerate_token::{
    RegenerateAuthTokenCommand,
    RegenerateAuthTokenUseCase,
    RegenerateAuthTokenResult,
};

pub use regenerate_secret::{
    RegenerateSigningSecretCommand,
    RegenerateSigningSecretUseCase,
    RegenerateSigningSecretResult,
};
