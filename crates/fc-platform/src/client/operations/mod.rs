//! Client Operations
//!
//! Use cases for client (tenant) management.

pub mod events;
pub mod create;
pub mod update;
pub mod activate;
pub mod suspend;
pub mod delete;
pub mod add_note;

pub use events::*;
pub use create::{CreateClientCommand, CreateClientUseCase};
pub use update::{UpdateClientCommand, UpdateClientUseCase};
pub use activate::{ActivateClientCommand, ActivateClientUseCase};
pub use suspend::{SuspendClientCommand, SuspendClientUseCase};
pub use delete::{DeleteClientCommand, DeleteClientUseCase};
pub use add_note::{AddClientNoteCommand, AddClientNoteUseCase};
