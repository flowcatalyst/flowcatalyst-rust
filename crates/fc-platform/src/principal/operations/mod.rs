//! Principal Operations
//!
//! Use cases for user (principal) management.

pub mod events;
pub mod create;
pub mod update;
pub mod activate;
pub mod deactivate;
pub mod delete;

pub use events::*;
pub use create::{CreateUserCommand, CreateUserUseCase};
pub use update::{UpdateUserCommand, UpdateUserUseCase};
pub use activate::{ActivateUserCommand, ActivateUserUseCase};
pub use deactivate::{DeactivateUserCommand, DeactivateUserUseCase};
pub use delete::{DeleteUserCommand, DeleteUserUseCase};
