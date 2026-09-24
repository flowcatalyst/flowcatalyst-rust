//! Update Service Account Use Case

use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;

use super::events::ServiceAccountUpdated;
use crate::service_account::ServiceAccount;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::ServiceAccountRepository;

/// Command for updating a service account.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateServiceAccountCommand {
    /// Service account ID
    pub id: String,

    /// Updated name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Updated description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Updated client IDs
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_ids: Option<Vec<String>>,
}

/// Use case for updating a service account.
pub struct UpdateServiceAccountUseCase<U: UnitOfWork> {
    service_account_repo: Arc<ServiceAccountRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> UpdateServiceAccountUseCase<U> {
    pub fn new(service_account_repo: Arc<ServiceAccountRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            service_account_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for UpdateServiceAccountUseCase<U> {
    type Command = UpdateServiceAccountCommand;
    type Event = ServiceAccountUpdated;

    async fn validate(&self, _command: &UpdateServiceAccountCommand) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn authorize(
        &self,
        _command: &UpdateServiceAccountCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: UpdateServiceAccountCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ServiceAccountUpdated> {
        let (service_account, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        // Atomic commit
        self.unit_of_work
            .commit(
                &service_account,
                &*self.service_account_repo,
                event,
                &command,
            )
            .await
    }
}

impl<U: UnitOfWork> UpdateServiceAccountUseCase<U> {
    async fn prepare(
        &self,
        command: &UpdateServiceAccountCommand,
        ctx: &ExecutionContext,
    ) -> Result<(ServiceAccount, ServiceAccountUpdated), UseCaseError> {
        // Find the service account
        let mut service_account = self
            .service_account_repo
            .find_by_id(&command.id)
            .await
            .or_not_found(
                "SERVICE_ACCOUNT_NOT_FOUND",
                format!("Service account with ID '{}' not found", command.id),
            )?;

        // Track changes for event
        let mut updated_name: Option<String> = None;
        let mut updated_description: Option<String> = None;
        let mut client_ids_added: Vec<String> = Vec::new();
        let mut client_ids_removed: Vec<String> = Vec::new();

        // Apply name update
        if let Some(ref name) = command.name {
            let name = name.trim();
            if name.is_empty() || name.len() > 100 {
                return Err(UseCaseError::validation(
                    "INVALID_NAME",
                    "Name must be 1-100 characters",
                ));
            }
            if service_account.name != name {
                service_account.name = name.to_string();
                updated_name = Some(name.to_string());
            }
        }

        // Apply description update
        if let Some(ref description) = command.description {
            if description.len() > 500 {
                return Err(UseCaseError::validation(
                    "INVALID_DESCRIPTION",
                    "Description must be max 500 characters",
                ));
            }
            service_account.description = Some(description.clone());
            updated_description = Some(description.clone());
        }

        // Apply client_ids update
        if let Some(ref new_client_ids) = command.client_ids {
            let current_set: HashSet<String> = service_account.client_ids.iter().cloned().collect();
            let new_set: HashSet<String> = new_client_ids.iter().cloned().collect();

            client_ids_added = new_set.difference(&current_set).cloned().collect();
            client_ids_removed = current_set.difference(&new_set).cloned().collect();

            service_account.client_ids = new_client_ids.clone();
        }

        service_account.updated_at = Utc::now();

        // Create domain event
        let event = ServiceAccountUpdated::new(
            ctx,
            &service_account.id,
            updated_name.as_deref(),
            updated_description.as_deref(),
            client_ids_added,
            client_ids_removed,
        );
        Ok((service_account, event))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = UpdateServiceAccountCommand {
            id: "sa-123".to_string(),
            name: Some("Updated Name".to_string()),
            description: None,
            client_ids: Some(vec!["client-1".to_string(), "client-2".to_string()]),
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("sa-123"));
        assert!(json.contains("Updated Name"));
    }
}
