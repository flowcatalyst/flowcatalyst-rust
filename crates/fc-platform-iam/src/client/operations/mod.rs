//! Client Operations
//!
//! Use cases for client (tenant) management.

pub mod activate;
pub mod add_note;
pub mod create;
pub mod delete;
pub mod events;
pub mod suspend;
pub mod update;

pub use activate::{ActivateClientCommand, ActivateClientUseCase};
pub use add_note::{AddClientNoteCommand, AddClientNoteUseCase};
pub use create::{CreateClientCommand, CreateClientUseCase};
pub use delete::{DeleteClientCommand, DeleteClientUseCase};
pub use events::*;
pub use suspend::{SuspendClientCommand, SuspendClientUseCase};
pub use update::{UpdateClientCommand, UpdateClientUseCase};
