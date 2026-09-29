//! Application Operations
//!
//! Use cases for application management following the Command pattern
//! with guaranteed event emission and audit logging through UnitOfWork.

pub mod activate;
pub mod attach_service_account;
pub mod create;
pub mod deactivate;
pub mod delete;
pub mod disable_for_client;
pub mod enable_for_client;
pub mod events;
pub mod provision_service_account;
pub mod update;
pub mod update_client_applications;
pub mod update_client_config;

// Re-export events
pub use events::{
    ApplicationActivated, ApplicationClientConfigUpdated, ApplicationCreated,
    ApplicationDeactivated, ApplicationDeleted, ApplicationDisabledForClient,
    ApplicationEnabledForClient, ApplicationServiceAccountProvisioned, ApplicationUpdated,
    ClientApplicationsUpdated,
};

// Re-export commands and use cases
pub use activate::{ActivateApplicationCommand, ActivateApplicationUseCase};
pub use attach_service_account::{
    AttachServiceAccountToApplicationCommand, AttachServiceAccountToApplicationUseCase,
};
pub use create::{CreateApplicationCommand, CreateApplicationUseCase};
pub use deactivate::{DeactivateApplicationCommand, DeactivateApplicationUseCase};
pub use delete::{DeleteApplicationCommand, DeleteApplicationUseCase};
pub use disable_for_client::{
    DisableApplicationForClientCommand, DisableApplicationForClientUseCase,
};
pub use enable_for_client::{EnableApplicationForClientCommand, EnableApplicationForClientUseCase};
pub use provision_service_account::ProvisionServiceAccountCommand;
pub use update::{UpdateApplicationCommand, UpdateApplicationUseCase};
pub use update_client_applications::{
    UpdateClientApplicationsCommand, UpdateClientApplicationsUseCase,
};
pub use update_client_config::{
    UpdateApplicationClientConfigCommand, UpdateApplicationClientConfigUseCase,
};
