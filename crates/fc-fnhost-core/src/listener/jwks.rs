//! The platform's signing keys: [`fc_platform_jwks::jwks`], shared with the
//! router's API since owner ruling 2 of 2026-09-25 (Java `eb21ed2a` moved
//! `JwksKeySource` out of the function host the same way).

pub use fc_platform_jwks::jwks::{JwksKeySource, REFETCH_FLOOR};
