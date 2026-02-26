//! Application Operations
//!
//! Use cases for application management following the Command pattern
//! with guaranteed event emission and audit logging through UnitOfWork.

pub mod events;
pub mod create;
pub mod update;
pub mod activate;
pub mod deactivate;

// Re-export events
pub use events::{
    ApplicationCreated,
    ApplicationUpdated,
    ApplicationActivated,
    ApplicationDeactivated,
    ApplicationServiceAccountProvisioned,
};

// Re-export commands and use cases
pub use create::{
    CreateApplicationCommand,
    CreateApplicationUseCase,
};

pub use update::{
    UpdateApplicationCommand,
    UpdateApplicationUseCase,
};

pub use activate::{
    ActivateApplicationCommand,
    ActivateApplicationUseCase,
};

pub use deactivate::{
    DeactivateApplicationCommand,
    DeactivateApplicationUseCase,
};
