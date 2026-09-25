//! Which clients a caller reaches, and under which client an ingested event
//! or dispatch job is written (owner decision #24; Java
//! `IngestApi.requireWritableClient`, security-fixes-2026-09-24 S3.2).
//!
//! The token's `clients` claim holds `*` for an anchor and `id:identifier`
//! pairs otherwise (Go's shape, owner decision #3), so a client id matches
//! an entry that is the id itself or the id followed by `:`.

use crate::shared::authorization_service::AuthContext;
use crate::shared::error::{PlatformError, Result};

/// The client id an entry of the `clients` claim names: the part before its
/// first `:` (a pair), or the whole entry. `*` names none.
fn entry_client_id(entry: &str) -> Option<&str> {
    let id = entry.split_once(':').map_or(entry, |(id, _)| id);
    (!id.is_empty() && id != "*").then_some(id)
}

/// The client ids the caller holds explicitly (never `*`), in claim order.
pub fn client_ids(ctx: &AuthContext) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for id in ctx
        .accessible_clients
        .iter()
        .filter_map(|c| entry_client_id(c))
    {
        if !ids.iter().any(|known| known == id) {
            ids.push(id.to_string());
        }
    }
    ids
}

/// Whether the caller may act within client `client_id`: an anchor always;
/// otherwise a `*` entry or an entry naming that client.
pub fn reaches_client(ctx: &AuthContext, client_id: &str) -> bool {
    ctx.is_anchor()
        || ctx
            .accessible_clients
            .iter()
            .any(|c| c == "*" || entry_client_id(c) == Some(client_id))
}

/// Whether the caller reaches a resource owned by `client_id`, where `None`
/// is the platform, which only an anchor reaches (Java
/// `Checks.canAccessScope`).
pub fn reaches_scope(ctx: &AuthContext, client_id: Option<&str>) -> bool {
    match client_id {
        Some(id) => reaches_client(ctx, id),
        None => ctx.is_anchor(),
    }
}

/// A blank client reference is no reference.
pub fn non_blank(value: Option<String>) -> Option<String> {
    value.filter(|v| !v.trim().is_empty())
}

/// The client an ingested row is written under (Java
/// `IngestApi.requireWritableClient`): a whole-request 403 before anything
/// is written when the caller may not write there.
///
/// No client (a platform-scoped row) is an anchor's alone: a platform-scoped
/// event matches platform-wide subscriptions and a platform-scoped job is
/// outside every tenant's view. For a caller confined to exactly one client
/// an absent client means that client, which is what an SDK outbox (which
/// sends no `clientId` on a dispatch job) needs under a client-scoped
/// credential; any other non-anchor must name one.
pub fn require_writable_client(
    ctx: &AuthContext,
    client_id: Option<String>,
) -> Result<Option<String>> {
    match non_blank(client_id) {
        None if ctx.is_anchor() => Ok(None),
        None => match client_ids(ctx).as_slice() {
            [only] if !ctx.accessible_clients.iter().any(|c| c == "*") => Ok(Some(only.clone())),
            _ => Err(PlatformError::forbidden_code(
                "FORBIDDEN",
                "clientId is required: a client-scoped caller cannot write platform-scoped rows",
            )),
        },
        Some(id) if reaches_client(ctx, &id) => Ok(Some(id)),
        Some(id) => Err(PlatformError::forbidden_code(
            "FORBIDDEN",
            format!("No access to client: {id}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PrincipalType, UserScope};
    use std::collections::HashSet;

    fn ctx(scope: UserScope, clients: &[&str]) -> AuthContext {
        AuthContext {
            principal_id: "prn_caller".into(),
            principal_type: PrincipalType::Service,
            scope,
            email: None,
            name: "caller".into(),
            accessible_clients: clients.iter().map(|c| c.to_string()).collect(),
            permissions: HashSet::new(),
            roles: vec![],
        }
    }

    fn refused(r: Result<Option<String>>) -> String {
        match r {
            Err(e) => e.to_string(),
            Ok(v) => panic!("expected a refusal, got {v:?}"),
        }
    }

    #[test]
    fn pairs_name_their_client() {
        let c = ctx(UserScope::Partner, &["clt_a:acme", "clt_b"]);
        assert_eq!(client_ids(&c), vec!["clt_a", "clt_b"]);
        assert!(reaches_client(&c, "clt_a"));
        assert!(reaches_client(&c, "clt_b"));
        assert!(!reaches_client(&c, "clt_ac"));
        assert!(!reaches_client(&c, "acme"));
        assert!(!reaches_scope(&c, None));
        assert!(reaches_scope(&ctx(UserScope::Anchor, &["*"]), None));
    }

    #[test]
    fn an_anchor_keeps_platform_scope_and_any_client() {
        let a = ctx(UserScope::Anchor, &["*"]);
        assert_eq!(require_writable_client(&a, None).unwrap(), None);
        assert_eq!(
            require_writable_client(&a, Some("  ".into())).unwrap(),
            None
        );
        assert_eq!(
            require_writable_client(&a, Some("clt_x".into())).unwrap(),
            Some("clt_x".into())
        );
    }

    #[test]
    fn a_single_client_caller_defaults_to_its_client() {
        let c = ctx(UserScope::Client, &["clt_a:acme"]);
        assert_eq!(
            require_writable_client(&c, None).unwrap(),
            Some("clt_a".into())
        );
        assert_eq!(
            require_writable_client(&c, Some("clt_a".into())).unwrap(),
            Some("clt_a".into())
        );
        assert_eq!(
            refused(require_writable_client(&c, Some("clt_b".into()))),
            "No access to client: clt_b"
        );
    }

    #[test]
    fn a_multi_client_caller_must_name_one() {
        let p = ctx(UserScope::Partner, &["clt_a", "clt_b"]);
        assert!(refused(require_writable_client(&p, None)).contains("clientId is required"));
        assert_eq!(
            require_writable_client(&p, Some("clt_b".into())).unwrap(),
            Some("clt_b".into())
        );
        // A non-anchor holding `*` reaches any client but still names one.
        let wild = ctx(UserScope::Partner, &["*"]);
        assert!(refused(require_writable_client(&wild, None)).contains("clientId is required"));
        // And one with no clients at all writes nowhere.
        let none = ctx(UserScope::Client, &[]);
        assert!(refused(require_writable_client(&none, None)).contains("clientId is required"));
    }
}
