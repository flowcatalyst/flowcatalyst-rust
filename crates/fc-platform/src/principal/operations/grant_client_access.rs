//! Grant Client Access Use Case

use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::principal::entity::{PrincipalType, UserScope, ClientAccessGrant};
use crate::PrincipalRepository;
use crate::ClientRepository;
use crate::ClientAccessGrantRepository;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::ClientAccessGranted;

/// Command for granting client access to a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantClientAccessCommand {
    pub user_id: String,
    pub client_id: String,
}

pub struct GrantClientAccessUseCase<U: UnitOfWork> {
    principal_repo: Arc<PrincipalRepository>,
    client_repo: Arc<ClientRepository>,
    grant_repo: Arc<ClientAccessGrantRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> GrantClientAccessUseCase<U> {
    pub fn new(
        principal_repo: Arc<PrincipalRepository>,
        client_repo: Arc<ClientRepository>,
        grant_repo: Arc<ClientAccessGrantRepository>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self { principal_repo, client_repo, grant_repo, unit_of_work }
    }

    pub async fn execute(
        &self,
        command: GrantClientAccessCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ClientAccessGranted> {
        if command.user_id.trim().is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "USER_ID_REQUIRED", "User ID is required",
            ));
        }
        if command.client_id.trim().is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "CLIENT_ID_REQUIRED", "Client ID is required",
            ));
        }

        let principal = match self.principal_repo.find_by_id(&command.user_id).await {
            Ok(Some(p)) => p,
            Ok(None) => {
                return UseCaseResult::failure(UseCaseError::not_found(
                    "USER_NOT_FOUND",
                    format!("User with ID '{}' not found", command.user_id),
                ));
            }
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to fetch user: {}", e
                )));
            }
        };

        if principal.principal_type != PrincipalType::User {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "NOT_A_USER", "Client access can only be granted to USER type principals",
            ));
        }

        // Business rule: PARTNER scope only
        if principal.scope != UserScope::Partner {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "NOT_PARTNER_SCOPE",
                "Client access grants are only for PARTNER scope users",
            ));
        }

        // Validate client exists
        match self.client_repo.find_by_id(&command.client_id).await {
            Ok(Some(_)) => {}
            Ok(None) => {
                return UseCaseResult::failure(UseCaseError::not_found(
                    "CLIENT_NOT_FOUND",
                    format!("Client with ID '{}' not found", command.client_id),
                ));
            }
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to validate client: {}", e
                )));
            }
        }

        // Check grant doesn't already exist
        match self.grant_repo.find_by_principal_and_client(&command.user_id, &command.client_id).await {
            Ok(Some(_)) => {
                return UseCaseResult::failure(UseCaseError::business_rule(
                    "GRANT_EXISTS",
                    "User already has access to this client",
                ));
            }
            Ok(None) => {}
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to check existing grant: {}", e
                )));
            }
        }

        let grant = ClientAccessGrant::new(
            &command.user_id,
            &command.client_id,
            &ctx.principal_id,
        );

        let event = ClientAccessGranted::new(
            &ctx,
            &principal.id,
            &command.client_id,
        );

        self.unit_of_work.commit(&grant, event, &command).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = GrantClientAccessCommand {
            user_id: "user-123".to_string(),
            client_id: "client-456".to_string(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("userId"));
        assert!(json.contains("clientId"));
    }
}
