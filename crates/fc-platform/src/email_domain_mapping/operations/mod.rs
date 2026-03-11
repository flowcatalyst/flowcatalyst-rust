//! Email Domain Mapping Operations
//!
//! Use cases for managing email domain mappings.

pub mod events;
pub mod create;
pub mod update;
pub mod delete;

pub use events::*;
pub use create::{CreateEmailDomainMappingCommand, CreateEmailDomainMappingUseCase};
pub use update::{UpdateEmailDomainMappingCommand, UpdateEmailDomainMappingUseCase};
pub use delete::{DeleteEmailDomainMappingCommand, DeleteEmailDomainMappingUseCase};
