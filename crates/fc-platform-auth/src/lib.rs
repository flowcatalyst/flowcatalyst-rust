//! FlowCatalyst Platform: the sign-in flows.
//!
//! OAuth and OIDC (the token endpoint, authorize, OIDC login, sessions,
//! refresh tokens, authorization codes, JWKS), password reset, two-factor
//! login and self-service (`mfa`), passkeys (`webauthn`), the portal login
//! plane (`portal`), client selection and `/api/me`. The model they work on
//! is fc-platform-iam's; `auth`, `mfa` and `portal` re-export its halves.
//!
//! Every module keeps its path from the single `fc-platform` crate this was
//! split from (docs/plans/build-speed-2026-09-28.md, section 6); `fc-platform`
//! re-exports them at `fc_platform::<module>`.

pub mod auth;
pub mod mfa;
pub mod portal;
pub mod shared;
pub mod webauthn;
