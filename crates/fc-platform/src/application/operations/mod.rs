//! Application Operations
//!
//! Use cases for application management following the Command pattern
//! with guaranteed event emission and audit logging through UnitOfWork.

pub mod events;
pub mod create;
pub mod update;
pub mod activate;
pub mod deactivate;
pub mod delete;
pub mod enable_for_client;
pub mod disable_for_client;
pub mod update_client_config;
pub mod attach_service_account;

// Re-export events
pub use events::{
    ApplicationCreated,
    ApplicationUpdated,
    ApplicationActivated,
    ApplicationDeactivated,
    ApplicationDeleted,
    ApplicationEnabledForClient,
    ApplicationDisabledForClient,
    ApplicationClientConfigUpdated,
    ApplicationServiceAccountProvisioned,
};

// Re-export commands and use cases
pub use create::{CreateApplicationCommand, CreateApplicationUseCase};
pub use update::{UpdateApplicationCommand, UpdateApplicationUseCase};
pub use activate::{ActivateApplicationCommand, ActivateApplicationUseCase};
pub use deactivate::{DeactivateApplicationCommand, DeactivateApplicationUseCase};
pub use delete::{DeleteApplicationCommand, DeleteApplicationUseCase};
pub use enable_for_client::{EnableApplicationForClientCommand, EnableApplicationForClientUseCase};
pub use disable_for_client::{DisableApplicationForClientCommand, DisableApplicationForClientUseCase};
pub use update_client_config::{UpdateApplicationClientConfigCommand, UpdateApplicationClientConfigUseCase};
pub use attach_service_account::{AttachServiceAccountToApplicationCommand, AttachServiceAccountToApplicationUseCase};
