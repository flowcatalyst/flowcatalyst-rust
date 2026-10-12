//! Which clients a caller reaches, and under which client an ingested event
//! or dispatch job is written (owner decision #24; Java
//! `IngestApi.requireWritableClient`, security-fixes-2026-09-24 S3.2).

use crate::permissions;
use crate::shared::authorization_service::Authority;
use crate::shared::error::{PlatformError, Result};
use crate::shared::id::ClientId;
use crate::shared::id::Id;
use crate::usecase::UseCaseError;
use std::collections::HashMap;
use std::result;

/// The client ids the caller holds explicitly (never `*`), in claim order.
pub fn client_ids(ctx: &impl Authority) -> Vec<String> {
    ctx.accessible_clients()
        .iter()
        .filter(|c| c.as_str() != "*")
        .cloned()
        .collect()
}

/// Whether the caller may act within client `client_id`: an anchor always,
/// otherwise as [`Authority::can_access_client`] says (Java
/// `AuthContext.canAccessClient`).
pub fn reaches_client(ctx: &impl Authority, client_id: &ClientId) -> bool {
    ctx.is_anchor() || ctx.can_access_client(client_id)
}

/// Whether the caller reaches a resource owned by `client_id`, where `None`
/// is the platform, which only an anchor reaches (Java
/// `Checks.canAccessScope`).
pub fn reaches_scope(ctx: &impl Authority, client_id: Option<&ClientId>) -> bool {
    match client_id {
        Some(id) => reaches_client(ctx, id),
        None => ctx.is_anchor(),
    }
}

/// A blank client reference is no reference.
pub fn non_blank(value: Option<String>) -> Option<String> {
    value.filter(|v| !v.trim().is_empty())
}

/// [`non_blank`] for an id a client may have sent blank.
pub fn non_blank_id<K>(value: Option<Id<K>>) -> Option<Id<K>> {
    value.filter(|v| !v.as_str().trim().is_empty())
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
    ctx: &impl Authority,
    client_id: Option<String>,
) -> Result<Option<String>> {
    match non_blank(client_id) {
        None if ctx.is_anchor() => Ok(None),
        None => match client_ids(ctx).as_slice() {
            [only] if !ctx.accessible_clients().iter().any(|c| c == "*") => Ok(Some(only.clone())),
            _ => Err(PlatformError::forbidden_code(
                "FORBIDDEN",
                "clientId is required: a client-scoped caller cannot write platform-scoped rows",
            )),
        },
        Some(id) if reaches_client(ctx, &ClientId::from_wire(id.as_str())) => Ok(Some(id)),
        Some(id) => Err(PlatformError::forbidden_code(
            "FORBIDDEN",
            format!("No access to client: {id}"),
        )),
    }
}

/// The distinct non-blank `clientCode`s of the items that name no
/// non-blank `clientId` (`(clientId, clientCode)` per item): the codes a
/// request has to look up, in one query.
pub fn codes_to_resolve<'a>(
    items: impl IntoIterator<Item = (Option<&'a str>, Option<&'a str>)>,
) -> Vec<String> {
    let mut codes: Vec<String> = items
        .into_iter()
        .filter(|(id, _)| id.is_none_or(|id| id.trim().is_empty()))
        .filter_map(|(_, code)| code.filter(|c| !c.trim().is_empty()).map(str::to_string))
        .collect();
    codes.sort();
    codes.dedup();
    codes
}

/// The refusal of a `clientCode` the caller may not write under, naming the
/// code: one answer whether it names another client or none, so it neither
/// confirms that a code exists nor reveals another tenant's client id.
fn no_access_to_code(code: &str) -> PlatformError {
    PlatformError::forbidden_code("FORBIDDEN", format!("No access to client: {code}"))
}

/// The client a directly created dispatch job is written under when it may
/// name it by `clientCode` (Go `ResolveJobClient`, Java
/// `IngestClients.resolveJobClient`; owner ruling 2026-10-12, F14), given
/// the request's resolved codes (`ids_by_code`, see [`codes_to_resolve`]):
///
/// 1. a non-blank `clientId` wins (the code is ignored) and goes through
///    [`require_writable_client`]: 403 "No access to client: <clientId>";
/// 2. else a code naming a client the caller reaches resolves to it;
/// 3. a code naming another client, or none, from a non-anchor: 403
///    `FORBIDDEN` "No access to client: <clientCode>";
/// 4. a code naming no client from an anchor: 400 `CLIENT_NOT_FOUND`
///    "Client not found: <clientCode>" (a job is never written unscoped
///    because of an unknown code);
/// 5. neither: [`require_writable_client`] with no client, unchanged (Go
///    writes a non-anchor's job platform-scoped here; Java and Rust refuse).
///
/// A blank `clientId` or `clientCode` counts as absent. The code is not
/// stored.
pub fn resolve_job_client(
    ctx: &impl Authority,
    client_id: Option<String>,
    client_code: Option<String>,
    ids_by_code: &HashMap<String, ClientId>,
) -> Result<Option<String>> {
    if let Some(id) = non_blank(client_id) {
        return require_writable_client(ctx, Some(id));
    }
    let Some(code) = non_blank(client_code) else {
        return require_writable_client(ctx, None);
    };
    match ids_by_code.get(&code) {
        Some(id) if reaches_client(ctx, id) => Ok(Some(id.to_string())),
        None if ctx.is_anchor() => Err(PlatformError::bad_request_code(
            "CLIENT_NOT_FOUND",
            format!("Client not found: {code}"),
        )),
        _ => Err(no_access_to_code(&code)),
    }
}

/// The client an ingested event is written under when it may name it by
/// `clientCode` (Go `ResolveEventClient`, Java
/// `IngestClients.resolveEventClient`; owner ruling 2026-10-12, F14), given
/// the request's resolved codes (`ids_by_code`, see [`codes_to_resolve`]):
/// a non-blank `clientId` wins and goes through [`require_writable_client`];
/// else a code naming a client the caller reaches resolves to it; an
/// anchor's unknown code leaves the event platform-scoped (the event is a
/// fact); any other code is 403 `FORBIDDEN` "No access to client:
/// <clientCode>", whether it names another client or none (never the
/// other client's id); neither goes through [`require_writable_client`].
pub fn resolve_event_client(
    ctx: &impl Authority,
    client_id: Option<String>,
    client_code: Option<String>,
    ids_by_code: &HashMap<String, ClientId>,
) -> Result<Option<String>> {
    if let Some(id) = non_blank(client_id) {
        return require_writable_client(ctx, Some(id));
    }
    let Some(code) = non_blank(client_code) else {
        return require_writable_client(ctx, None);
    };
    match ids_by_code.get(&code) {
        Some(id) if reaches_client(ctx, id) => Ok(Some(id.to_string())),
        None if ctx.is_anchor() => Ok(None),
        _ => Err(no_access_to_code(&code)),
    }
}

/// The client filter of a read-model list (events, dispatch jobs), shared
/// by the `/api` list handlers and the server-rendered `fc-web` UI:
/// requested clients must each be reachable (else 403 `No access to
/// client: …`); with none requested an anchor sees everything (an empty
/// filter) and anyone else their explicit clients. `Ok(None)` means the
/// caller reaches no client, so the list is empty.
pub fn read_client_filter(
    ctx: &impl Authority,
    requested: Vec<String>,
) -> Result<Option<Vec<String>>> {
    if !requested.is_empty() {
        for cid in &requested {
            if !ctx.can_access_client(&ClientId::from_wire(cid.as_str())) {
                return Err(PlatformError::forbidden(format!(
                    "No access to client: {}",
                    cid
                )));
            }
        }
        return Ok(Some(requested));
    }
    if ctx.is_anchor() {
        return Ok(Some(requested));
    }
    let own = client_ids(ctx);
    Ok((!own.is_empty()).then_some(own))
}

/// A single event or dispatch job is visible when it is platform-scoped or
/// the caller can access its client; `what` names it in the refusal ("No
/// access to this event").
pub fn ensure_row_visible(
    ctx: &impl Authority,
    client_id: Option<&ClientId>,
    what: &str,
) -> Result<()> {
    match client_id {
        Some(cid) if !ctx.can_access_client(cid) => Err(PlatformError::forbidden(format!(
            "No access to this {what}"
        ))),
        _ => Ok(()),
    }
}

/// Go `auth.CanAccessScope` (shared/auth/auth.go:400): a client's resource
/// needs that client; a platform resource (`None`) needs anchor scope or
/// the super-admin wildcard.
pub fn can_access_scope(ctx: &impl Authority, client_id: Option<&ClientId>) -> bool {
    match client_id {
        Some(id) => reaches_client(ctx, id),
        None => ctx.is_anchor() || ctx.has_permission(permissions::ADMIN_ALL),
    }
}

/// Go `auth.CheckScopeAccess` (shared/auth/auth.go:433), the per-resource
/// scope check under a coarse permission check: 403 `SCOPE_FORBIDDEN`
/// with Go's two messages.
pub fn check_scope_access(
    ctx: &impl Authority,
    client_id: Option<&ClientId>,
) -> result::Result<(), UseCaseError> {
    if can_access_scope(ctx, client_id) {
        return Ok(());
    }
    Err(UseCaseError::forbidden(
        "SCOPE_FORBIDDEN",
        if client_id.is_some() {
            "no access to this resource's client"
        } else {
            "anchor scope required for this resource"
        },
    ))
}

/// [`check_scope_access`] for a handler.
pub fn require_scope_access(ctx: &impl Authority, client_id: Option<&ClientId>) -> Result<()> {
    check_scope_access(ctx, client_id).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::principal_kind::{PrincipalType, UserScope};
    use crate::shared::authorization_service::{AuthContext, Credential};
    use crate::shared::id::PrincipalId;
    use axum::http::StatusCode;
    use std::collections::HashSet;

    fn caller(scope: UserScope, clients: &[&str], perms: &[&str]) -> AuthContext {
        AuthContext {
            principal_id: PrincipalId::parse("prn_caller").unwrap(),
            principal_type: PrincipalType::Service,
            scope,
            email: None,
            name: "caller".into(),
            accessible_clients: clients.iter().map(|c| c.to_string()).collect(),
            permissions: perms.iter().map(|p| p.to_string()).collect::<HashSet<_>>(),
            roles: vec![],
            credential: Credential::BearerToken,
        }
    }

    fn ctx(scope: UserScope, clients: &[&str]) -> AuthContext {
        caller(scope, clients, &[])
    }

    fn refused(r: Result<Option<String>>) -> String {
        match r {
            Err(e) => e.to_string(),
            Ok(v) => panic!("expected a refusal, got {v:?}"),
        }
    }

    #[test]
    fn read_lists_filter_by_the_callers_clients() {
        let anchor = ctx(UserScope::Anchor, &["*"]);
        assert_eq!(read_client_filter(&anchor, vec![]).unwrap(), Some(vec![]));
        let c = ctx(UserScope::Client, &["clt_a"]);
        assert_eq!(
            read_client_filter(&c, vec![]).unwrap(),
            Some(vec!["clt_a".to_string()])
        );
        assert_eq!(
            read_client_filter(&c, vec!["clt_a".into()]).unwrap(),
            Some(vec!["clt_a".to_string()])
        );
        assert!(read_client_filter(&c, vec!["clt_b".into()])
            .unwrap_err()
            .to_string()
            .contains("No access to client: clt_b"));
        let none = ctx(UserScope::Client, &[]);
        assert_eq!(read_client_filter(&none, vec![]).unwrap(), None);
        assert!(ensure_row_visible(&c, Some(&ClientId::parse("clt_b").unwrap()), "event").is_err());
        assert!(ensure_row_visible(&c, Some(&ClientId::parse("clt_a").unwrap()), "event").is_ok());
        assert!(ensure_row_visible(&c, None, "event").is_ok());
    }

    #[test]
    fn only_an_anchor_reaches_the_platform() {
        let c = ctx(UserScope::Partner, &["clt_a", "clt_b"]);
        assert_eq!(client_ids(&c), vec!["clt_a", "clt_b"]);
        assert!(reaches_client(&c, &ClientId::parse("clt_a").unwrap()));
        assert!(!reaches_client(&c, &ClientId::parse("clt_c").unwrap()));
        assert!(!reaches_scope(&c, None));
        assert!(reaches_scope(&ctx(UserScope::Anchor, &["*"]), None));
        assert!(reaches_scope(
            &ctx(UserScope::Anchor, &["*"]),
            Some(&ClientId::parse("clt_c").unwrap())
        ));
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
        let c = ctx(UserScope::Client, &["clt_a"]);
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

    fn codes() -> HashMap<String, ClientId> {
        HashMap::from([
            ("acme".to_string(), ClientId::parse("clt_a").unwrap()),
            ("beta".to_string(), ClientId::parse("clt_b").unwrap()),
        ])
    }

    fn some(s: &str) -> Option<String> {
        Some(s.to_string())
    }

    #[test]
    fn only_codes_of_items_without_a_client_id_are_looked_up() {
        let items = [
            (None, Some("beta")),
            (Some("clt_x"), Some("acme")),
            (Some("  "), Some("acme")),
            (None, Some(" ")),
            (None, None),
            (None, Some("beta")),
        ];
        assert_eq!(codes_to_resolve(items), ["acme", "beta"]);
    }

    #[test]
    fn a_job_code_resolves_to_a_client_the_caller_reaches() {
        let c = ctx(UserScope::Client, &["clt_a"]);
        let anchor = ctx(UserScope::Anchor, &["*"]);
        let p = ctx(UserScope::Partner, &["clt_a", "clt_b"]);
        let codes = codes();
        // A code naming a reached client resolves.
        for caller in [&c, &anchor] {
            assert_eq!(
                resolve_job_client(caller, None, some("acme"), &codes).unwrap(),
                some("clt_a")
            );
        }
        assert_eq!(
            resolve_job_client(&p, None, some("beta"), &codes).unwrap(),
            some("clt_b")
        );
        // A clientId wins, even over another client's code; a blank one is
        // absent.
        assert_eq!(
            resolve_job_client(&c, some("clt_a"), some("beta"), &codes).unwrap(),
            some("clt_a")
        );
        assert_eq!(
            resolve_job_client(&c, some(" "), some("acme"), &codes).unwrap(),
            some("clt_a")
        );
        assert_eq!(
            refused(resolve_job_client(&c, some("clt_b"), some("acme"), &codes)),
            "No access to client: clt_b"
        );
        // Neither: the writable-client rule, unchanged.
        assert_eq!(
            resolve_job_client(&anchor, None, some("  "), &codes).unwrap(),
            None
        );
        assert_eq!(
            resolve_job_client(&c, None, None, &codes).unwrap(),
            some("clt_a")
        );
        assert!(
            refused(resolve_job_client(&p, None, None, &codes)).contains("clientId is required")
        );
    }

    #[test]
    fn a_job_code_the_caller_cannot_use_is_refused_naming_the_code() {
        let c = ctx(UserScope::Client, &["clt_a"]);
        let codes = codes();
        for code in ["beta", "nosuch"] {
            let err = resolve_job_client(&c, None, some(code), &codes).unwrap_err();
            assert!(
                matches!(&err, PlatformError::Coded { status, code: c, .. }
                    if *status == StatusCode::FORBIDDEN && c == "FORBIDDEN"),
                "{err:?}"
            );
            assert_eq!(err.to_string(), format!("No access to client: {code}"));
        }
        let anchor = ctx(UserScope::Anchor, &["*"]);
        let err = resolve_job_client(&anchor, None, some("nosuch"), &codes).unwrap_err();
        assert!(
            matches!(&err, PlatformError::Coded { status, code, .. }
                if *status == StatusCode::BAD_REQUEST && code == "CLIENT_NOT_FOUND"),
            "{err:?}"
        );
        assert_eq!(err.to_string(), "Client not found: nosuch");
    }

    #[test]
    fn an_event_code_refusal_names_the_code() {
        let c = ctx(UserScope::Client, &["clt_a"]);
        let anchor = ctx(UserScope::Anchor, &["*"]);
        let codes = codes();
        // Another client's code and an unknown code: the same refusal,
        // naming the code, never `clt_b`.
        for code in ["beta", "nosuch"] {
            assert_eq!(
                refused(resolve_event_client(&c, None, some(code), &codes)),
                format!("No access to client: {code}")
            );
            assert_eq!(
                refused(resolve_event_client(&c, some("  "), some(code), &codes)),
                format!("No access to client: {code}")
            );
        }
        // An own code resolves, an explicit id wins, a blank one is absent.
        assert_eq!(
            resolve_event_client(&c, None, some("acme"), &codes).unwrap(),
            some("clt_a")
        );
        assert_eq!(
            resolve_event_client(&c, some("clt_a"), some("beta"), &codes).unwrap(),
            some("clt_a")
        );
        assert_eq!(
            resolve_event_client(&c, some(" "), None, &codes).unwrap(),
            some("clt_a")
        );
        // An anchor's unknown code leaves the event platform-scoped.
        assert_eq!(
            resolve_event_client(&anchor, None, some("nosuch"), &codes).unwrap(),
            None
        );
        assert_eq!(
            resolve_event_client(&anchor, some(""), some("beta"), &codes).unwrap(),
            some("clt_b")
        );
        let p = ctx(UserScope::Partner, &["clt_a", "clt_b"]);
        assert!(
            refused(resolve_event_client(&p, None, None, &codes)).contains("clientId is required")
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
