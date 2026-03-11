//! Revoke Client Access Use Case

use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::principal::entity::PrincipalType;
use crate::PrincipalRepository;
use crate::ClientAccessGrantRepository;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::ClientAccessRevoked;

/// Command for revoking client access from a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevokeClientAccessCommand {
    pub user_id: String,
    pub client_id: String,
}

pub struct RevokeClientAccessUseCase<U: UnitOfWork> {
    principal_repo: Arc<PrincipalRepository>,
    grant_repo: Arc<ClientAccessGrantRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> RevokeClientAccessUseCase<U> {
    pub fn new(
        principal_repo: Arc<PrincipalRepository>,
        grant_repo: Arc<ClientAccessGrantRepository>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self { principal_repo, grant_repo, unit_of_work }
    }

    pub async fn execute(
        &self,
        command: RevokeClientAccessCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ClientAccessRevoked> {
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

        // Validate user exists and is a USER type
        let _principal = match self.principal_repo.find_by_id(&command.user_id).await {
            Ok(Some(p)) => {
                if p.principal_type != PrincipalType::User {
                    return UseCaseResult::failure(UseCaseError::business_rule(
                        "NOT_A_USER", "Client access can only be revoked from USER type principals",
                    ));
                }
                p
            }
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

        // Find existing grant
        let grant = match self.grant_repo.find_by_principal_and_client(&command.user_id, &command.client_id).await {
            Ok(Some(g)) => g,
            Ok(None) => {
                return UseCaseResult::failure(UseCaseError::not_found(
                    "GRANT_NOT_FOUND",
                    "No access grant found for this user and client",
                ));
            }
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to fetch grant: {}", e
                )));
            }
        };

        let event = ClientAccessRevoked::new(
            &ctx,
            &command.user_id,
            &command.client_id,
        );

        self.unit_of_work.commit_delete(&grant, event, &command).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = RevokeClientAccessCommand {
            user_id: "user-123".to_string(),
            client_id: "client-456".to_string(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("userId"));
    }
}
