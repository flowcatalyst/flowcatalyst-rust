//! Use Case Infrastructure
//!
//! Foundational patterns for implementing domain-driven use cases:
//!
//! - [`UseCase`] — trait enforcing validate → authorize → execute pipeline
//! - [`UseCaseResult`] — sealed result type for use case outcomes
//! - [`UseCaseError`] — categorized error types (validation, not found, etc.)
//! - [`DomainEvent`] — trait for domain events with CloudEvents structure
//! - [`EventMetadata`] — common event metadata, built with [`EventMetadata::from_ctx`]
//! - [`ExecutionContext`] — tracing and principal context
//! - [`TracingContext`] — task-local correlation/causation propagation
//!
//! The [`UnitOfWork`](crate::outbox::UnitOfWork) trait and implementations
//! are in the [`outbox`](crate::outbox) module.

pub mod domain_event;
pub mod error;
pub mod execution_context;
pub mod result;
pub mod tracing_context;
pub mod use_case;

pub use domain_event::{DomainEvent, EventMetadata};
pub use error::UseCaseError;
pub use execution_context::ExecutionContext;
pub use result::UseCaseResult;
pub use tracing_context::TracingContext;
pub use use_case::UseCase;
