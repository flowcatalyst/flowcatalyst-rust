//! A service account's client reach: the clients it is linked to. Tokens are
//! built from the linked principal, so the reach has to land on the principal
//! (`iam_principals.scope` / `client_id` and `iam_client_access_grants`), not
//! just on the service account. The tier follows the links
//! ([`crate::service_account::entity::ServiceAccount::link_clients`], Go's `applyClientReach`).

use fc_platform_core::shared::id::ClientId;
use std::collections::HashSet;
use std::env;

use crate::client::repository::ClientRepository;
use fc_platform_core::usecase::UseCaseError;

/// Whether a stored `scope` that disagrees with the client links is refused
/// (`FC_SERVICE_ACCOUNT_SCOPE_REACH_STRICT`, default false). Go refuses it
/// (F28, `400 SCOPE_REACH_MISMATCH`); here it is off by default because the
/// vendored SPA's service-account drawer still sends the old links beside
/// `scope: ANCHOR`, and a client may send `CLIENT` with no links, both of
/// which are accepted today. Turn it on once the SPA is on Go's fix.
pub fn scope_reach_strict_from_env() -> bool {
    env::var("FC_SERVICE_ACCOUNT_SCOPE_REACH_STRICT").is_ok_and(|v| {
        matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

/// The tier a token for an account with `links` client links gets
/// (`applyClientReach`): none = ANCHOR, one = CLIENT, several = PARTNER.
fn derived_tier(links: usize) -> &'static str {
    match links {
        0 => "ANCHOR",
        1 => "CLIENT",
        _ => "PARTNER",
    }
}

/// Refuse a stored `scope` that disagrees with the tier the account's client
/// links give its token (Go F28 `requireScopeMatchesReach`). The scope field
/// is descriptive: tokens follow the links, so ANCHOR beside links is echoed
/// by the API while the token stays PARTNER/CLIENT, and CLIENT/PARTNER with
/// too few links yields a wider (ANCHOR) token than the field claims.
/// Refusing keeps the stored value and the token one fact and never widens
/// reach. An unset scope, or one that is not a tier, is left alone.
pub fn require_scope_matches_reach(scope: Option<&str>, links: usize) -> Result<(), UseCaseError> {
    let Some(scope) = scope else {
        return Ok(());
    };
    let want = scope.trim().to_ascii_uppercase();
    if !matches!(want.as_str(), "ANCHOR" | "CLIENT" | "PARTNER") {
        return Ok(());
    }
    let got = derived_tier(links);
    if got != want {
        return Err(UseCaseError::validation(
            "SCOPE_REACH_MISMATCH",
            format!(
                "scope {want} does not match the account's client links ({links} linked: tokens are ANCHOR with no clients, CLIENT with one, PARTNER with several, so these links give {got}). Change clientIds and scope together, or omit scope"
            ),
        ));
    }
    Ok(())
}

/// Drop repeated client ids, keeping the first occurrence's position.
pub fn dedupe_client_ids(client_ids: Vec<ClientId>) -> Vec<ClientId> {
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
    client_ids: &[ClientId],
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

    /// Go F28 `TestServiceAccountScope_MustMatchReach`, as a table.
    #[test]
    fn scope_must_match_the_tier_the_links_give() {
        let code = |r: Result<(), UseCaseError>| r.err().map(|e| e.code().to_string());
        // matching or unset or not a tier: accepted
        for (scope, links) in [
            (Some("ANCHOR"), 0),
            (Some("CLIENT"), 1),
            (Some("PARTNER"), 2),
            (Some("PARTNER"), 5),
            (Some("anchor"), 0),
            (Some(" client "), 1),
            (None, 0),
            (None, 3),
            (Some("WIDGET"), 3),
        ] {
            assert_eq!(
                code(require_scope_matches_reach(scope, links)),
                None,
                "{scope:?}/{links}"
            );
        }
        // disagreeing: refused
        for (scope, links) in [
            (Some("ANCHOR"), 1),
            (Some("ANCHOR"), 2),
            (Some("CLIENT"), 0),
            (Some("CLIENT"), 2),
            (Some("PARTNER"), 0),
            (Some("PARTNER"), 1),
        ] {
            assert_eq!(
                code(require_scope_matches_reach(scope, links)).as_deref(),
                Some("SCOPE_REACH_MISMATCH"),
                "{scope:?}/{links}"
            );
        }
    }

    #[test]
    fn dedupe_keeps_first_occurrence_order() {
        let id = |s: &str| ClientId::from_wire(s);
        let got = dedupe_client_ids(vec![id("b"), id("a"), id("b"), id("c")]);
        assert_eq!(got, vec![id("b"), id("a"), id("c")]);
    }
}
