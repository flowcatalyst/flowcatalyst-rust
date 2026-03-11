//! Event Type Operations
//!
//! Use cases for managing event types.

mod create;
mod update;
mod archive;
mod delete;
mod add_schema;
mod finalise_schema;
mod deprecate_schema;
mod sync;
mod events;

pub use create::{CreateEventTypeCommand, CreateEventTypeUseCase};
pub use update::{UpdateEventTypeCommand, UpdateEventTypeUseCase};
pub use archive::{ArchiveEventTypeCommand, ArchiveEventTypeUseCase};
pub use delete::{DeleteEventTypeCommand, DeleteEventTypeUseCase};
pub use add_schema::{AddSchemaCommand, AddSchemaUseCase};
pub use finalise_schema::{FinaliseSchemaCommand, FinaliseSchemaUseCase};
pub use deprecate_schema::{DeprecateSchemaCommand, DeprecateSchemaUseCase};
pub use sync::{SyncEventTypesCommand, SyncEventTypesUseCase, SyncEventTypeInput};
pub use events::{
    EventTypeCreated, EventTypeUpdated, EventTypeArchived, EventTypeDeleted,
    SchemaAdded, SchemaFinalised, SchemaDeprecated, EventTypesSynced,
};
