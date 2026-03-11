//! Use Case Infrastructure
//!
//! Foundational patterns for implementing domain-driven use cases:
//!
//! - [`UseCaseResult`] — sealed result type for use case outcomes
//! - [`UseCaseError`] — categorized error types (validation, not found, etc.)
//! - [`DomainEvent`] — trait for domain events with CloudEvents structure
//! - [`EventMetadata`] — common metadata with builder pattern
//! - [`ExecutionContext`] — tracing and principal context
//! - [`TracingContext`] — distributed tracing propagation
//!
//! The [`UnitOfWork`](crate::outbox::UnitOfWork) trait and implementations
//! are in the [`outbox`](crate::outbox) module.

pub mod result;
pub mod error;
pub mod domain_event;
pub mod execution_context;
pub mod tracing_context;

pub use result::UseCaseResult;
pub use error::UseCaseError;
pub use domain_event::{DomainEvent, EventMetadata, EventMetadataBuilder};
pub use execution_context::ExecutionContext;
pub use tracing_context::TracingContext;
