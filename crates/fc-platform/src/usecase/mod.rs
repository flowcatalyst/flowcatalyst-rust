//! Use Case Infrastructure
//!
//! Provides the foundational patterns for implementing use cases:
//! - `Result<T>` - sealed result type for use case outcomes
//! - `UseCaseError` - categorized error types for consistent handling
//! - `DomainEvent` - trait for domain events with CloudEvents structure
//! - `ExecutionContext` - tracing and principal context for use case execution
//! - `TracingContext` - distributed tracing context propagation
//! - `UnitOfWork` - atomic commit of entity + event + audit log

pub mod result;
pub mod error;
pub mod domain_event;
pub mod execution_context;
pub mod tracing_context;
pub mod unit_of_work;

pub use result::UseCaseResult;
pub use error::UseCaseError;
pub use domain_event::{DomainEvent, EventMetadata, EventMetadataBuilder};
pub use execution_context::ExecutionContext;
pub use tracing_context::TracingContext;
pub use unit_of_work::{UnitOfWork, MongoUnitOfWork};
