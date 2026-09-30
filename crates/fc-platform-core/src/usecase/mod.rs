//! Use Case Infrastructure
//!
//! Provides the foundational patterns for implementing use cases:
//! - `Committed<T>` - sealed proof of a unit-of-work commit; `UseCaseResult<T>` - a use case's outcome
//! - `UseCaseError` - categorized error types for consistent handling
//! - `DomainEvent` - trait for domain events with CloudEvents structure
//! - `ExecutionContext` - tracing and principal context for use case execution
//! - `Caller` - the authority a use case's `authorize` checks
//! - `UnitOfWork` - atomic commit of entity + event + audit log

pub mod audit_operation;
pub mod caller;
pub mod domain_event;
pub mod error;
pub mod execution_context;
pub mod result;
pub mod unit_of_work;
pub mod use_case;

pub use caller::{Caller, CallerCredential};
pub use domain_event::{DomainEvent, EventMetadata, RecordedEvent};
pub use error::{ErrorKind, OrNotFound, UseCaseError};
pub use execution_context::ExecutionContext;
pub use result::{Committed, UseCaseResult};
pub use unit_of_work::{
    DbTx, HasId, LockedRead, Persist, PgUnitOfWork, TxScopedUnitOfWork, UnitOfWork,
};
pub use use_case::UseCase;

use crate::shared::id::ClientId;
use crate::PlatformError;
use fc_common::netguard::default_policy;

/// A client id named in a command: a malformed one is the caller's mistake, a
/// 400 with its own code, not a database foreign-key failure.
pub fn parse_client_id(raw: &str) -> Result<ClientId, UseCaseError> {
    ClientId::parse(raw.trim())
        .map_err(|e| UseCaseError::validation("INVALID_CLIENT_ID", e.to_string()))
}

/// [`parse_client_id`] for an optional command field.
pub fn parse_client_id_opt(raw: Option<&str>) -> Result<Option<ClientId>, UseCaseError> {
    raw.map(parse_client_id).transpose()
}

/// The delivery policy (`fc_common::netguard`) as a validation step: a URL the
/// platform will POST to is customer input, so a loopback, link-local or
/// private target is refused when it is written (Go: `netguard.Default.
/// ValidateURL` in each use case's `Validate`). `what` names the field in the
/// message, as Go's `"endpoint " + err` / `"targetUrl " + err`.
pub fn validate_delivery_url(
    code: &'static str,
    what: &str,
    url: &str,
) -> Result<(), UseCaseError> {
    default_policy()
        .validate_url(url)
        .map_err(|e| UseCaseError::validation(code, format!("{what} {e}")))
}

/// [`validate_delivery_url`] for a handler that answers with a `PlatformError`
/// (the dispatch job ingest endpoints: a 400 `INVALID_TARGET_URL`).
pub fn check_target_url(url: &str) -> Result<(), PlatformError> {
    default_policy().validate_url(url).map_err(|e| {
        PlatformError::bad_request_code("INVALID_TARGET_URL", format!("targetUrl {e}"))
    })
}

/// A command's declared audit-masked fields (see `fc_common::audit_redaction`).
/// Every command a unit of work audits implements it; most declare none.
pub use fc_common::audit_redaction::AuditMasked;
