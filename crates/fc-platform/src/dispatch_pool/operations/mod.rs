//! Dispatch Pool Operations
//!
//! Use cases for dispatch pool management following the Command pattern
//! with guaranteed event emission and audit logging through UnitOfWork.

pub mod archive;
pub mod create;
pub mod delete;
pub mod events;
pub mod status;
pub mod sync;
pub mod update;

// Re-export events
pub use events::{
    DispatchPoolActivated, DispatchPoolArchived, DispatchPoolCreated, DispatchPoolDeleted,
    DispatchPoolSuspended, DispatchPoolUpdated, DispatchPoolsSynced,
};

// Re-export commands and use cases
pub use create::{CreateDispatchPoolCommand, CreateDispatchPoolUseCase};

pub use update::{UpdateDispatchPoolCommand, UpdateDispatchPoolUseCase};

pub use archive::{ArchiveDispatchPoolCommand, ArchiveDispatchPoolUseCase};

pub use delete::{DeleteDispatchPoolCommand, DeleteDispatchPoolUseCase};

pub use status::{
    ActivateDispatchPoolCommand, ActivateDispatchPoolUseCase, SuspendDispatchPoolCommand,
    SuspendDispatchPoolUseCase,
};

pub use sync::{SyncDispatchPoolInput, SyncDispatchPoolsCommand, SyncDispatchPoolsUseCase};
