//! Revoke Client Access Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ClientAccessRevoked;
use crate::principal::entity::{ClientAccessGrant, PrincipalType};
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::ClientAccessGrantRepository;
use crate::PrincipalRepository;

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
        Self {
            principal_repo,
            grant_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for RevokeClientAccessUseCase<U> {
    type Command = RevokeClientAccessCommand;
    type Event = ClientAccessRevoked;

    async fn validate(&self, command: &RevokeClientAccessCommand) -> Result<(), UseCaseError> {
        if command.user_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "USER_ID_REQUIRED",
                "User ID is required",
            ));
        }
        if command.client_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "CLIENT_ID_REQUIRED",
                "Client ID is required",
            ));
        }

        Ok(())
    }

    async fn authorize(
        &self,
        _command: &RevokeClientAccessCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: RevokeClientAccessCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ClientAccessRevoked> {
        let (grant, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        self.unit_of_work
            .commit_delete(&grant, &*self.grant_repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> RevokeClientAccessUseCase<U> {
    async fn prepare(
        &self,
        command: &RevokeClientAccessCommand,
        ctx: &ExecutionContext,
    ) -> Result<(ClientAccessGrant, ClientAccessRevoked), UseCaseError> {
        // Validate user exists and is a USER type
        let principal = self
            .principal_repo
            .find_by_id(&command.user_id)
            .await
            .or_not_found(
                "USER_NOT_FOUND",
                format!("User with ID '{}' not found", command.user_id),
            )?;
        if principal.principal_type != PrincipalType::User {
            return Err(UseCaseError::business_rule(
                "NOT_A_USER",
                "Client access can only be revoked from USER type principals",
            ));
        }

        // Find existing grant
        let grant = self
            .grant_repo
            .find_by_principal_and_client(&command.user_id, &command.client_id)
            .await
            .or_not_found(
                "GRANT_NOT_FOUND",
                "No access grant found for this user and client",
            )?;

        let event = ClientAccessRevoked::new(ctx, &command.user_id, &command.client_id);
        Ok((grant, event))
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
