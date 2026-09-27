//! Platform-issued bearer tokens, verified away from the platform (Java
//! `io.flowcatalyst.platform.shared.auth.jwks`, moved out of the function
//! host so the router could share it: Java `eb21ed2a`).
//!
//! - [`jwks::JwksKeySource`]: the platform's issuer and signing keys, found
//!   through `<platformUrl>/.well-known/openid-configuration`, cached, and
//!   refetched for an unknown `kid` at most once per 30 s.
//! - [`bearer::BearerAuthenticator`]: a `Bearer` JWT verified locally: RS256
//!   only, signature, `exp`/`nbf`, the discovered issuer, a platform
//!   audience, and identity tokens refused.
//! - [`permission::grants`]: the platform's permission match (an exact
//!   string, or the same segment count with `*` segments).
//!
//! The function host (`fc-fnhost-core`) and the router's API
//! (`fc-router`, owner ruling 2 of 2026-09-25) are the two callers.

pub mod bearer;
pub mod clock;
pub mod jwks;
pub mod permission;
#[cfg(feature = "test-support")]
pub mod testing;

pub use bearer::{BearerAuthenticator, TokenClaims, SCOPE_WILDCARD, TOKEN_USE_API};
pub use clock::{Clock, SharedClock, SystemClock};
pub use jwks::JwksKeySource;
