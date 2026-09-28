//! Event Type Operations
//!
//! Use cases for managing event types.

mod add_schema;
mod archive;
mod create;
mod delete;
mod deprecate_schema;
mod events;
mod finalise_schema;
mod sync;
mod update;

pub use add_schema::{AddSchemaCommand, AddSchemaUseCase};
pub use archive::{ArchiveEventTypeCommand, ArchiveEventTypeUseCase};
pub use create::{CreateEventTypeCommand, CreateEventTypeUseCase};
pub use delete::{DeleteEventTypeCommand, DeleteEventTypeUseCase};
pub use deprecate_schema::{DeprecateSchemaCommand, DeprecateSchemaUseCase};
pub use events::{
    EventTypeArchived, EventTypeCreated, EventTypeDeleted, EventTypeUpdated, EventTypesSynced,
    SchemaAdded, SchemaDeprecated, SchemaFinalised,
};
pub use finalise_schema::{FinaliseSchemaCommand, FinaliseSchemaUseCase};
pub use sync::{SyncEventTypeInput, SyncEventTypesCommand, SyncEventTypesUseCase};
pub use update::{UpdateEventTypeCommand, UpdateEventTypeUseCase};
