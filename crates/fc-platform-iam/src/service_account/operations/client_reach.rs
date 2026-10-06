//! A service account's client reach: the clients it is linked to. Tokens are
//! built from the linked principal, so the reach has to land on the principal
//! (`iam_principals.scope` / `client_id` and `iam_client_access_grants`), not
//! just on the service account. The tier follows the links
//! ([`crate::service_account::entity::ServiceAccount::link_clients`], Go's `applyClientReach`).

use fc_platform_core::shared::id::ClientId;
use std::collections::HashSet;

use crate::client::repository::ClientRepository;
use fc_platform_core::usecase::UseCaseError;

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

    #[test]
    fn dedupe_keeps_first_occurrence_order() {
        let id = |s: &str| ClientId::from_wire(s);
        let got = dedupe_client_ids(vec![id("b"), id("a"), id("b"), id("c")]);
        assert_eq!(got, vec![id("b"), id("a"), id("c")]);
    }
}
