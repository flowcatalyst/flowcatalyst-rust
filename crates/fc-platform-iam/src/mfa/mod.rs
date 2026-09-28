//! Two-factor authentication: the state an administrator reads and resets.
//!
//! Enrolled methods, recovery codes, trusted devices (`entity`,
//! `repository`) and the notification e-mails (`notify`). The login,
//! self-service and admin flows are fc-platform-auth's `mfa`, which
//! re-exports this module.

pub mod entity;
pub mod notify;
pub mod repository;

pub use repository::MfaRepository;
