//! `auth: platform` and versioned calls: a bearer JWT verified locally
//! against the platform's keys. The verifier is
//! [`fc_platform_jwks::bearer`], shared with the router's API since owner
//! ruling 2 of 2026-09-25 (Java `eb21ed2a`); what stays here is the mapping
//! of its claims onto the function's [`Principal`].

use std::collections::BTreeSet;

use fc_function_abi::Principal;

use crate::clock::SharedClock;
pub use fc_platform_jwks::bearer::{BearerAuthenticator, TokenClaims, SCOPE_WILDCARD};
use std::sync::Arc;

/// Every claim onto the function's [`Principal`] (Java
/// `FnHttpServer.principalFrom`, spec `function-caller-claims.md` §3); an
/// absent `type` is `unknown`.
pub fn principal(claims: &TokenClaims) -> Principal {
    Principal {
        id: claims.subject.clone(),
        principal_type: claims
            .principal_type
            .clone()
            .unwrap_or_else(|| "unknown".to_owned()),
        tier: claims.tier.clone(),
        clients: claims.clients.clone(),
        roles: claims.roles.clone(),
        applications: claims.applications.clone(),
        all_applications: claims.all_applications,
        permissions: claims.permissions.iter().cloned().collect::<BTreeSet<_>>(),
    }
}

/// The host's clock as the verifier's (the same instant source, so a test's
/// `ManualClock` moves both).
pub fn verifier_clock(clock: &SharedClock) -> fc_platform_jwks::SharedClock {
    let clock = clock.clone();
    Arc::new(move || clock.now())
}
