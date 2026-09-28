//! FlowCatalyst Platform core: the kernel every domain crate builds on.
//!
//! - [`usecase`]: the use-case contract, the sealed [`Committed`] proof and
//!   the unit of work that commits an aggregate with its event and audit row;
//! - [`shared`]: errors, typed ids, the authorization context
//!   ([`AuthContext`](shared::authorization_service::AuthContext)) and the
//!   [`checks`](shared::authorization_service::checks), the request
//!   extractors and auth layer, the database pool and migration runner,
//!   encryption, email, rate limiting;
//! - [`permissions`]: the permission catalogue;
//! - [`principal_kind`]: the principal type and client tier a context carries.
//!
//! `fc-platform` re-exports all of it at its historical paths
//! (`fc_platform::usecase`, `fc_platform::shared::error`, …).

pub mod permissions;
pub mod principal_kind;
pub mod shared;
pub mod usecase;

pub use shared::error::{PlatformError, Result};
pub use shared::tsid::EntityType;
pub use usecase::{
    Committed, DbTx, DomainEvent, ExecutionContext, HasId, Persist, PgUnitOfWork, UnitOfWork,
    UseCaseError, UseCaseResult,
};
