//! Update Subscription Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;

use super::create::EventTypeBindingInput;
use super::events::SubscriptionUpdated;
use crate::service_account::signing_reach::require_usable_signers;
use crate::shared::authorization_service::AuthContext;
use crate::shared::caller_reach::non_blank;
use crate::subscription::entity::DispatchMode;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::EventTypeBinding;
use crate::Subscription;
use crate::{ConnectionRepository, ServiceAccountRepository, SubscriptionRepository};

/// Command for updating an existing subscription.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSubscriptionCommand {
    /// Subscription ID to update
    pub subscription_id: String,

    /// New name (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// New description (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// New webhook endpoint URL (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,

    /// New connection ID (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection_id: Option<String>,

    /// New event types (replaces existing if provided)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_types: Option<Vec<EventTypeBindingInput>>,

    /// New dispatch pool ID (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispatch_pool_id: Option<String>,

    /// New service account ID (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_account_id: Option<String>,

    /// New dispatch mode (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<DispatchMode>,

    /// New max retries (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<u32>,

    /// New timeout in seconds (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u32>,

    /// New data_only setting (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_only: Option<bool>,

    /// Who is updating it, for the signing-reach check (never serialised,
    /// so never in the audit log).
    #[serde(skip)]
    pub caller: Option<AuthContext>,
}

impl crate::usecase::AuditMasked for UpdateSubscriptionCommand {}

/// Use case for updating an existing subscription.
pub struct UpdateSubscriptionUseCase<U: UnitOfWork> {
    subscription_repo: Arc<SubscriptionRepository>,
    service_account_repo: Arc<ServiceAccountRepository>,
    connection_repo: Arc<ConnectionRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> UpdateSubscriptionUseCase<U> {
    pub fn new(
        subscription_repo: Arc<SubscriptionRepository>,
        service_account_repo: Arc<ServiceAccountRepository>,
        connection_repo: Arc<ConnectionRepository>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self {
            subscription_repo,
            service_account_repo,
            connection_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for UpdateSubscriptionUseCase<U> {
    type Command = UpdateSubscriptionCommand;
    type Event = SubscriptionUpdated;

    async fn validate(&self, command: &UpdateSubscriptionCommand) -> Result<(), UseCaseError> {
        if command.subscription_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "SUBSCRIPTION_ID_REQUIRED",
                "Subscription ID is required",
            ));
        }

        if command.name.is_none()
            && command.description.is_none()
            && command.endpoint.is_none()
            && command.connection_id.is_none()
            && command.event_types.is_none()
            && command.dispatch_pool_id.is_none()
            && command.service_account_id.is_none()
            && command.mode.is_none()
            && command.max_retries.is_none()
            && command.timeout_seconds.is_none()
            && command.data_only.is_none()
        {
            return Err(UseCaseError::validation(
                "NO_UPDATES",
                "At least one field must be provided for update",
            ));
        }

        Ok(())
    }

    async fn authorize(
        &self,
        _command: &UpdateSubscriptionCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: UpdateSubscriptionCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<SubscriptionUpdated> {
        let (subscription, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        // Atomic commit
        self.unit_of_work
            .commit(&subscription, &*self.subscription_repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> UpdateSubscriptionUseCase<U> {
    async fn prepare(
        &self,
        command: &UpdateSubscriptionCommand,
        ctx: &ExecutionContext,
    ) -> Result<(Subscription, SubscriptionUpdated), UseCaseError> {
        // Fetch existing subscription
        let mut subscription = self
            .subscription_repo
            .find_by_id(&command.subscription_id)
            .await
            .or_not_found(
                "SUBSCRIPTION_NOT_FOUND",
                format!(
                    "Subscription with ID '{}' not found",
                    command.subscription_id
                ),
            )?;

        // Where deliveries go and who signs them, before the update.
        let account_before = non_blank(subscription.service_account_id.clone());
        let connection_before = non_blank(subscription.connection_id.clone());
        let endpoint_before = subscription.endpoint.clone();

        // Track changes
        let mut updated_name: Option<&str> = None;
        let mut event_types_added: Vec<String> = Vec::new();
        let mut event_types_removed: Vec<String> = Vec::new();

        // Apply updates
        if let Some(ref name) = command.name {
            let name = name.trim();
            if name != subscription.name {
                subscription.name = name.to_string();
                updated_name = Some(name);
            }
        }

        if let Some(ref desc) = command.description {
            let changed = subscription.description.as_deref() != Some(desc.as_str());
            if changed {
                subscription.description = Some(desc.clone());
            }
        }

        if let Some(ref ep) = command.endpoint {
            subscription.endpoint = ep.clone();
        }

        if let Some(ref conn_id) = command.connection_id {
            subscription.connection_id = Some(conn_id.clone());
        }

        if let Some(ref new_event_types) = command.event_types {
            let old_codes: HashSet<String> = subscription
                .event_types
                .iter()
                .map(|b| b.event_type_code.clone())
                .collect();

            let new_bindings: Vec<EventTypeBinding> = new_event_types
                .iter()
                .map(|input| {
                    let mut binding = EventTypeBinding::new(&input.event_type_code);
                    if let Some(ref filter) = input.filter {
                        binding = binding.with_filter(filter);
                    }
                    binding
                })
                .collect();

            let new_codes: HashSet<String> = new_bindings
                .iter()
                .map(|b| b.event_type_code.clone())
                .collect();

            // Calculate diff
            event_types_added = new_codes.difference(&old_codes).cloned().collect();
            event_types_removed = old_codes.difference(&new_codes).cloned().collect();

            if !event_types_added.is_empty() || !event_types_removed.is_empty() {
                subscription.event_types = new_bindings;
            }
        }

        if let Some(ref pool_id) = command.dispatch_pool_id {
            subscription.dispatch_pool_id = Some(pool_id.clone());
        }

        if let Some(ref account_id) = command.service_account_id {
            subscription.service_account_id = Some(account_id.clone());
        }

        if let Some(mode) = command.mode {
            subscription.mode = mode;
        }

        if let Some(retries) = command.max_retries {
            subscription.max_retries = retries as i32;
        }

        if let Some(timeout) = command.timeout_seconds {
            subscription.timeout_seconds = timeout as i32;
        }

        if let Some(data_only) = command.data_only {
            subscription.data_only = data_only;
        }

        // Whenever the update changes the endpoint, the account or the
        // connection, the resulting account and connection must be ones the
        // caller may sign with (S7; Java `UpdateSubscription`): re-pointing
        // the endpoint of a subscription signed by an account the caller
        // cannot reach would hand that account's credentials to the
        // caller's endpoint. Re-sending the current values (the SPA sends
        // the whole form) changes nothing and is not re-checked.
        let account_after = non_blank(subscription.service_account_id.clone());
        let connection_after = non_blank(subscription.connection_id.clone());
        let account_changed = account_before != account_after;
        let connection_changed = connection_before != connection_after;
        if account_changed || connection_changed || endpoint_before != subscription.endpoint {
            require_usable_signers(
                command.caller.as_ref(),
                &self.service_account_repo,
                &self.connection_repo,
                account_after.as_deref(),
                account_changed,
                connection_after.as_deref(),
                connection_changed,
            )
            .await?;
        }

        subscription.updated_at = chrono::Utc::now();

        // Create domain event
        let event = SubscriptionUpdated::new(
            ctx,
            &subscription.id,
            updated_name,
            event_types_added,
            event_types_removed,
        );
        Ok((subscription, event))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = UpdateSubscriptionCommand {
            subscription_id: "sub-123".to_string(),
            name: Some("New Name".to_string()),
            description: None,
            endpoint: None,
            connection_id: None,
            event_types: None,
            dispatch_pool_id: None,
            service_account_id: None,
            mode: None,
            max_retries: Some(10),
            timeout_seconds: None,
            data_only: None,
            caller: None,
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("subscriptionId"));
        assert!(json.contains("New Name"));
    }
}
