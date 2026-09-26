//! Use Case Infrastructure
//!
//! Provides the foundational patterns for implementing use cases:
//! - `Result<T>` - sealed result type for use case outcomes
//! - `UseCaseError` - categorized error types for consistent handling
//! - `DomainEvent` - trait for domain events with CloudEvents structure
//! - `ExecutionContext` - tracing and principal context for use case execution
//! - `UnitOfWork` - atomic commit of entity + event + audit log

pub mod audit_operation;
pub mod domain_event;
pub mod error;
pub mod execution_context;
pub mod result;
pub mod unit_of_work;
pub mod use_case;

#[cfg(test)]
mod event_persistence_snapshot_tests;

pub use domain_event::{DomainEvent, EventMetadata, RecordedEvent};
pub use error::{ErrorKind, OrNotFound, UseCaseError};
pub use execution_context::ExecutionContext;
pub use result::UseCaseResult;
pub use unit_of_work::{
    DbTx, HasId, LockedRead, Persist, PgUnitOfWork, TxScopedUnitOfWork, UnitOfWork,
};
pub use use_case::UseCase;

/// A command's declared audit-masked fields (see `fc_common::audit_redaction`).
/// Every command a unit of work audits implements it; most declare none.
pub use fc_common::audit_redaction::AuditMasked;
