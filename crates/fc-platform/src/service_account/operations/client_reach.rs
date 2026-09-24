//! A service account's client reach: its scope plus the clients it is linked
//! to. Tokens are built from the linked principal, so the reach has to land
//! on the principal (`iam_principals.scope` / `client_id` and
//! `iam_client_access_grants`), not just on the service account.

use std::collections::HashSet;

use crate::principal::entity::UserScope;
use crate::usecase::UseCaseError;
use crate::ClientRepository;

/// Decide the scope for a set of client links.
///
/// With no explicit scope the scope follows the links, as Go's
/// `applyClientReach` does: none → ANCHOR, one → CLIENT, several → PARTNER.
/// An explicit scope must agree with the links: ANCHOR takes none, CLIENT
/// exactly one, PARTNER at least one. Go silently re-derives a disagreeing
/// scope from the links (so `CLIENT` with no clients became ANCHOR); here it
/// is a 400 instead.
pub fn resolve_client_reach(
    scope: Option<UserScope>,
    client_ids: &[String],
) -> Result<UserScope, UseCaseError> {
    match (scope, client_ids.len()) {
        (None, 0) => Ok(UserScope::Anchor),
        (None, 1) => Ok(UserScope::Client),
        (None, _) => Ok(UserScope::Partner),
        (Some(UserScope::Anchor), 0) => Ok(UserScope::Anchor),
        (Some(UserScope::Anchor), _) => Err(UseCaseError::validation(
            "ANCHOR_SCOPE_TAKES_NO_CLIENTS",
            "An ANCHOR service account reaches every client; clientIds must be empty",
        )),
        (Some(UserScope::Client), 1) => Ok(UserScope::Client),
        (Some(UserScope::Client), _) => Err(UseCaseError::validation(
            "CLIENT_SCOPE_NEEDS_ONE_CLIENT",
            "A CLIENT service account needs exactly one client in clientIds",
        )),
        (Some(UserScope::Partner), 0) => Err(UseCaseError::validation(
            "PARTNER_SCOPE_NEEDS_CLIENTS",
            "A PARTNER service account needs at least one client in clientIds",
        )),
        (Some(UserScope::Partner), _) => Ok(UserScope::Partner),
    }
}

/// Drop repeated client ids, keeping the first occurrence's position.
pub fn dedupe_client_ids(client_ids: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    client_ids
        .into_iter()
        .filter(|id| seen.insert(id.clone()))
        .collect()
}

/// Refuse a link to a client that doesn't exist, naming it, rather than
/// confine the account to a client that isn't there (Go's
/// `requireClientsExist`).
pub async fn require_clients_exist(
    clients: &ClientRepository,
    client_ids: &[String],
) -> Result<(), UseCaseError> {
    if client_ids.is_empty() {
        return Ok(());
    }
    let found = clients.find_by_ids(client_ids).await?;
    let known: HashSet<&str> = found.iter().map(|c| c.id.as_str()).collect();
    match client_ids.iter().find(|id| !known.contains(id.as_str())) {
        Some(missing) => Err(UseCaseError::not_found(
            "CLIENT_NOT_FOUND",
            format!("Client not found: {}", missing),
        )),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("clt_{i}")).collect()
    }

    fn code(r: Result<UserScope, UseCaseError>) -> String {
        r.unwrap_err().code().to_string()
    }

    #[test]
    fn absent_scope_follows_the_links_like_go() {
        assert_eq!(
            resolve_client_reach(None, &ids(0)).unwrap(),
            UserScope::Anchor
        );
        assert_eq!(
            resolve_client_reach(None, &ids(1)).unwrap(),
            UserScope::Client
        );
        assert_eq!(
            resolve_client_reach(None, &ids(3)).unwrap(),
            UserScope::Partner
        );
    }

    #[test]
    fn explicit_scope_is_honoured_when_the_links_agree() {
        let anchor = Some(UserScope::Anchor);
        let client = Some(UserScope::Client);
        let partner = Some(UserScope::Partner);
        assert_eq!(
            resolve_client_reach(anchor, &ids(0)).unwrap(),
            UserScope::Anchor
        );
        assert_eq!(
            resolve_client_reach(client, &ids(1)).unwrap(),
            UserScope::Client
        );
        assert_eq!(
            resolve_client_reach(partner, &ids(1)).unwrap(),
            UserScope::Partner
        );
        assert_eq!(
            resolve_client_reach(partner, &ids(2)).unwrap(),
            UserScope::Partner
        );
    }

    #[test]
    fn explicit_scope_that_disagrees_with_the_links_is_rejected() {
        assert_eq!(
            code(resolve_client_reach(Some(UserScope::Anchor), &ids(1))),
            "ANCHOR_SCOPE_TAKES_NO_CLIENTS"
        );
        // Go reads this as ANCHOR; here it's an error, never an escalation.
        assert_eq!(
            code(resolve_client_reach(Some(UserScope::Client), &ids(0))),
            "CLIENT_SCOPE_NEEDS_ONE_CLIENT"
        );
        assert_eq!(
            code(resolve_client_reach(Some(UserScope::Client), &ids(2))),
            "CLIENT_SCOPE_NEEDS_ONE_CLIENT"
        );
        assert_eq!(
            code(resolve_client_reach(Some(UserScope::Partner), &ids(0))),
            "PARTNER_SCOPE_NEEDS_CLIENTS"
        );
    }

    #[test]
    fn dedupe_keeps_first_occurrence_order() {
        let got = dedupe_client_ids(vec!["b".into(), "a".into(), "b".into(), "c".into()]);
        assert_eq!(got, vec!["b", "a", "c"]);
    }
}
