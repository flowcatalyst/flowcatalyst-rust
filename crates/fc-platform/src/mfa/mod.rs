//! Two-factor authentication for internal (password) users, as Go serves it
//! (`internal/platform/mfa`, `auth/login/twofactor*.go`, `auth/twofa`,
//! `auth/mfatoken`, `notify`).
//!
//! - A password sign-in whose user has a confirmed factor (or whose domain
//!   requires one) answers `mfa_required` / `enrollment_required` with a
//!   short-lived step token instead of a session; the token-gated
//!   `/auth/2fa/*` routes then finish it ([`login_api`]).
//! - Signed-in users manage their factors, recovery codes and remembered
//!   devices under `/auth/2fa/*` ([`self_service_api`]).
//! - Passkey and OIDC sign-ins never challenge.
//!
//! Factor, code, PIN and device rows are authentication state, written
//! directly like Go's (and like the token rows CLAUDE.md lists as
//! infrastructure): Go emits no domain events for them, only the audit rows
//! (`2FA_TOTP_ENROLLED`, `2FA_EMAIL_ENROLLED`, `2FA_METHOD_REMOVED`,
//! `2FA_RECOVERY_REGENERATED`, `2FA_RESET_BY_ADMIN`) that [`audit`] writes.

pub mod audit;
pub mod crypto;
pub mod entity;
pub mod login_api;
pub mod notify;
pub mod policy;
pub mod repository;
pub mod self_service_api;
pub mod service;
pub mod token;

pub use login_api::{two_factor_login_router, TwoFactorLogin};
pub use policy::TwoFactorPolicy;
pub use repository::MfaRepository;
pub use self_service_api::two_factor_self_service_router;
pub use service::MfaService;
pub use token::MfaTokenIssuer;
