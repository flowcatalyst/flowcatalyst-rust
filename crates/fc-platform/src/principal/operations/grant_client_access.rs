//! Grant Client Access Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ClientAccessGranted;
use crate::principal::entity::{ClientAccessGrant, PrincipalType, UserScope};
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::ClientAccessGrantRepository;
use crate::ClientRepository;
use crate::PrincipalRepository;

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
        Self {
            principal_repo,
            client_repo,
            grant_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for GrantClientAccessUseCase<U> {
    type Command = GrantClientAccessCommand;
    type Event = ClientAccessGranted;

    async fn validate(&self, command: &GrantClientAccessCommand) -> Result<(), UseCaseError> {
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
        _command: &GrantClientAccessCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: GrantClientAccessCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ClientAccessGranted> {
        let (grant, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        self.unit_of_work
            .commit(&grant, &*self.grant_repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> GrantClientAccessUseCase<U> {
    async fn prepare(
        &self,
        command: &GrantClientAccessCommand,
        ctx: &ExecutionContext,
    ) -> Result<(ClientAccessGrant, ClientAccessGranted), UseCaseError> {
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
                "Client access can only be granted to USER type principals",
            ));
        }

        // Business rule: PARTNER scope only
        if principal.scope != UserScope::Partner {
            return Err(UseCaseError::business_rule(
                "NOT_PARTNER_SCOPE",
                "Client access grants are only for PARTNER scope users",
            ));
        }

        // Validate client exists
        self.client_repo
            .find_by_id(&command.client_id)
            .await
            .or_not_found(
                "CLIENT_NOT_FOUND",
                format!("Client with ID '{}' not found", command.client_id),
            )?;

        // Check grant doesn't already exist
        if self
            .grant_repo
            .find_by_principal_and_client(&command.user_id, &command.client_id)
            .await?
            .is_some()
        {
            return Err(UseCaseError::business_rule(
                "GRANT_EXISTS",
                "User already has access to this client",
            ));
        }

        let grant = ClientAccessGrant::new(&command.user_id, &command.client_id, &ctx.principal_id);

        let event = ClientAccessGranted::new(ctx, &principal.id, &command.client_id);
        Ok((grant, event))
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
