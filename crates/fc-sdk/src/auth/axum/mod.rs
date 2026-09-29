//! Axum integration for FlowCatalyst OIDC + Bearer authentication.
//!
//! Rust analogue of `@flowcatalyst/sdk/fastify`. Gated behind the `axum`
//! feature. Adds:
//!
//!   - [`Principal`] — extractor exposing `has_role/roles/any_role`,
//!     `has_permission_to/any_permission_to`, and client/scope helpers.
//!   - [`RbacBuilder`] / [`RbacCatalogue`] — local role→permission map
//!     (declared in code) with `:` segment wildcards.
//!   - [`SessionStore`] / [`CookieSessionStore`] — AES-256-GCM encrypted
//!     cookie sessions as the default backend; trait is pluggable for
//!     Postgres/Redis follow-ups.
//!   - [`FlowcatalystAuthBuilder`] — produces an `(AuthState, Router)`
//!     pair that the consumer app merges into theirs.
//!   - Three guard-style extractors: [`RequireSession`], [`RequireBearer`],
//!     [`RequireAuth`] — wrap [`Principal`] with redirect/401 behaviour.
//!
//! See the crate root README for a worked example.

mod crypto;
mod extractor;
mod middleware;
mod oidc;
mod principal;
mod rbac;
mod router;
mod session;
mod state;

#[cfg(feature = "axum-session-postgres")]
mod pg_session_store;
#[cfg(feature = "axum-session-redis")]
mod redis_session_store;

pub use crypto::{generate_session_secret, SessionCrypto};
pub use extractor::{AuthRejection, Principal, RequireAuth, RequireBearer, RequireSession};
pub use middleware::fc_auth_middleware;
pub use principal::{AuthMechanism, Principal as PrincipalData};
pub use rbac::{RbacBuilder, RbacCatalogue, RoleScope};
pub use router::FlowcatalystAuthBuilder;
pub use session::{
    CookieAttrs, CookieSessionStore, PrincipalSnapshot, SessionPayload, SessionStore,
    SessionTokens, SharedSessionStore,
};
pub use state::{AuthRoutes, AuthState};

#[cfg(feature = "axum-session-postgres")]
pub use pg_session_store::{create_session_table_sql, init_session_schema, PgSessionStore};

#[cfg(feature = "axum-session-redis")]
pub use redis_session_store::RedisSessionStore;
