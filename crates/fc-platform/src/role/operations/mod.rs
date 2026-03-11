//! Role Operations
//!
//! Use cases for role management.

pub mod events;
pub mod create;
pub mod update;
pub mod delete;
pub mod sync;

pub use events::*;
pub use create::{CreateRoleCommand, CreateRoleUseCase};
pub use update::{UpdateRoleCommand, UpdateRoleUseCase};
pub use delete::{DeleteRoleCommand, DeleteRoleUseCase};
pub use sync::{SyncRolesCommand, SyncRolesUseCase, SyncRoleInput};
