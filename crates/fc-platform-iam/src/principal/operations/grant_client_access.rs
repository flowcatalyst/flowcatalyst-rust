//! Grant Client Access Use Case

use async_trait::async_trait;
use fc_platform_core::shared::id::ClientId;
use fc_platform_core::shared::id::PrincipalId;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ClientAccessGranted;
use crate::auth::config_repository::ClientAccessGrantRepository;
use crate::client::repository::ClientRepository;
use crate::principal::entity::ClientAccessGrant;
use crate::principal::repository::PrincipalRepository;
use fc_platform_core::principal_kind::{PrincipalType, UserScope};
use fc_platform_core::shared::authorization_service::checks;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};

/// Command for granting client access to a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantClientAccessCommand {
    pub user_id: PrincipalId,
    pub client_id: ClientId,
}

impl AuditMasked for GrantClientAccessCommand {}

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
        if command.user_id.as_str().trim().is_empty() {
            return Err(UseCaseError::validation(
                "USER_ID_REQUIRED",
                "User ID is required",
            ));
        }
        if command.client_id.as_str().trim().is_empty() {
            return Err(UseCaseError::validation(
                "CLIENT_ID_REQUIRED",
                "Client ID is required",
            ));
        }

        Ok(())
    }

    /// Client-access grants are an anchor's (`can_grant_client_access`, checked
    /// with the permission by the handler before the body; owner decision #25).
    async fn authorize(
        &self,
        _command: &GrantClientAccessCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(checks::require_anchor_scope(ctx.caller())?)
    }

    async fn execute(
        &self,
        command: GrantClientAccessCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<ClientAccessGranted>, UseCaseError> {
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

        let grant = ClientAccessGrant::new(
            command.user_id.clone(),
            command.client_id.clone(),
            &ctx.principal_id,
        );

        let event = ClientAccessGranted::new(&ctx, &principal.id, &command.client_id);

        self.unit_of_work
            .commit(&grant, &*self.grant_repo, event, &command)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = GrantClientAccessCommand {
            user_id: PrincipalId::from_wire("user-123"),
            client_id: ClientId::from_wire("client-456"),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("userId"));
        assert!(json.contains("clientId"));
    }
}
