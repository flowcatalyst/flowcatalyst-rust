//! Update Connection Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ConnectionUpdated;
use crate::connection::entity::ConnectionStatus;
use crate::connection::repository::ConnectionRepository;
use fc_platform_core::shared::caller_reach;
use fc_platform_core::shared::id::OptionIdExt;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};

/// Command for updating a connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateConnectionCommand {
    pub connection_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<ConnectionStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_account_id: Option<String>,
    /// A new owning application (set-if-provided; the handler has resolved
    /// it within the caller's application scope).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub application_code: Option<String>,
    /// Go's PUT: the name is required and description and externalId are
    /// replaced as sent (absent clears them). A status flip sets it false.
    #[serde(default)]
    pub replace_details: bool,
}

impl AuditMasked for UpdateConnectionCommand {}

pub struct UpdateConnectionUseCase<U: UnitOfWork> {
    connection_repo: Arc<ConnectionRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> UpdateConnectionUseCase<U> {
    pub fn new(connection_repo: Arc<ConnectionRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            connection_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for UpdateConnectionUseCase<U> {
    type Command = UpdateConnectionCommand;
    type Event = ConnectionUpdated;

    async fn validate(&self, command: &UpdateConnectionCommand) -> Result<(), UseCaseError> {
        if command.connection_id.trim().is_empty() {
            return Err(UseCaseError::validation("ID_REQUIRED", "id is required"));
        }
        if command.replace_details && command.name.as_deref().is_none_or(|n| n.trim().is_empty()) {
            return Err(UseCaseError::validation(
                "NAME_REQUIRED",
                "Connection name is required",
            ));
        }
        Ok(())
    }

    /// Go `CheckScopeAccess` on the stored connection (Go checks it post-load): a
    /// client's connection needs that client, a platform one anchor scope (403
    /// `SCOPE_FORBIDDEN`). A missing connection is `execute`'s 404.
    async fn authorize(
        &self,
        command: &UpdateConnectionCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        if let Some(target) = self
            .connection_repo
            .find_by_id(&command.connection_id)
            .await?
        {
            caller_reach::check_scope_access(ctx.caller(), target.client_id.as_id_str())?;
        }
        Ok(())
    }

    async fn execute(
        &self,
        command: UpdateConnectionCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<ConnectionUpdated>, UseCaseError> {
        let mut connection = self
            .connection_repo
            .find_by_id(&command.connection_id)
            .await
            .or_not_found(
                "CONNECTION_NOT_FOUND",
                format!("Connection with ID '{}' not found", command.connection_id),
            )?;
        if let Some(ref name) = command.name {
            connection.name = name.trim().to_string();
        }
        if command.replace_details {
            connection.description = command.description.clone();
            connection.external_id = command.external_id.clone();
        } else {
            if let Some(ref desc) = command.description {
                connection.description = Some(desc.clone());
            }
            if let Some(ref ext_id) = command.external_id {
                connection.external_id = Some(ext_id.clone());
            }
        }
        // A new application must not collide within its key (Go).
        if let Some(ref app_code) = command.application_code {
            if connection.application_code.as_deref() != Some(app_code.as_str()) {
                let dup = self
                    .connection_repo
                    .find_by_code_in_scope(
                        &connection.code,
                        Some(app_code),
                        connection.client_id.as_id_str(),
                    )
                    .await?;
                if dup.is_some_and(|d| d.id != connection.id) {
                    return Err(UseCaseError::business_rule(
                        "CODE_EXISTS",
                        format!("Connection with code '{}' already exists", connection.code),
                    ));
                }
                connection.application_code = Some(app_code.clone());
            }
        }
        if let Some(ref sa_id) = command.service_account_id {
            connection.service_account_id = sa_id.clone();
        }
        match command.status {
            Some(ConnectionStatus::Active) => connection.activate(),
            Some(ConnectionStatus::Paused) => connection.pause(),
            None => {}
        }
        connection.updated_at = chrono::Utc::now();

        let event = ConnectionUpdated::new(&ctx, connection.id.as_str(), &connection.name);

        self.unit_of_work
            .commit(&connection, &*self.connection_repo, event, &command)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = UpdateConnectionCommand {
            connection_id: "conn-123".to_string(),
            name: Some("Updated Name".to_string()),
            description: None,
            external_id: None,
            status: None,
            service_account_id: None,
            application_code: None,
            replace_details: false,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("connectionId"));
    }
}
