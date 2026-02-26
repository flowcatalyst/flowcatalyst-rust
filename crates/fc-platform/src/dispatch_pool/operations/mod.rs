//! Dispatch Pool Operations
//!
//! Use cases for dispatch pool management following the Command pattern
//! with guaranteed event emission and audit logging through UnitOfWork.

pub mod events;
pub mod create;
pub mod update;
pub mod archive;
pub mod delete;

// Re-export events
pub use events::{
    DispatchPoolCreated,
    DispatchPoolUpdated,
    DispatchPoolArchived,
    DispatchPoolDeleted,
};

// Re-export commands and use cases
pub use create::{
    CreateDispatchPoolCommand,
    CreateDispatchPoolUseCase,
};

pub use update::{
    UpdateDispatchPoolCommand,
    UpdateDispatchPoolUseCase,
};

pub use archive::{
    ArchiveDispatchPoolCommand,
    ArchiveDispatchPoolUseCase,
};

pub use delete::{
    DeleteDispatchPoolCommand,
    DeleteDispatchPoolUseCase,
};
