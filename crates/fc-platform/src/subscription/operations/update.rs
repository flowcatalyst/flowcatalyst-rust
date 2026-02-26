//! Update Subscription Use Case

use std::sync::Arc;
use std::collections::HashSet;
use serde::{Deserialize, Serialize};

use crate::{EventTypeBinding, DispatchMode, SubscriptionStatus};
use crate::SubscriptionRepository;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::SubscriptionUpdated;
use super::create::EventTypeBindingInput;

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

    /// New target URL (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,

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
}

/// Use case for updating an existing subscription.
pub struct UpdateSubscriptionUseCase<U: UnitOfWork> {
    subscription_repo: Arc<SubscriptionRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> UpdateSubscriptionUseCase<U> {
    pub fn new(subscription_repo: Arc<SubscriptionRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            subscription_repo,
            unit_of_work,
        }
    }

    pub async fn execute(
        &self,
        command: UpdateSubscriptionCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<SubscriptionUpdated> {
        // Validation: subscription_id is required
        if command.subscription_id.trim().is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "SUBSCRIPTION_ID_REQUIRED",
                "Subscription ID is required",
            ));
        }

        // Validation: at least one field to update
        if command.name.is_none()
            && command.description.is_none()
            && command.target.is_none()
            && command.event_types.is_none()
            && command.dispatch_pool_id.is_none()
            && command.service_account_id.is_none()
            && command.mode.is_none()
            && command.max_retries.is_none()
            && command.timeout_seconds.is_none()
            && command.data_only.is_none()
        {
            return UseCaseResult::failure(UseCaseError::validation(
                "NO_UPDATES",
                "At least one field must be provided for update",
            ));
        }

        // Fetch existing subscription
        let mut subscription = match self.subscription_repo.find_by_id(&command.subscription_id).await {
            Ok(Some(s)) => s,
            Ok(None) => {
                return UseCaseResult::failure(UseCaseError::not_found(
                    "SUBSCRIPTION_NOT_FOUND",
                    format!("Subscription with ID '{}' not found", command.subscription_id),
                ));
            }
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to fetch subscription: {}",
                    e
                )));
            }
        };

        // Business rule: can only update active or paused subscriptions
        if subscription.status == SubscriptionStatus::Archived {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "CANNOT_UPDATE_ARCHIVED",
                "Cannot update an archived subscription",
            ));
        }

        // Track changes
        let mut updated_name: Option<&str> = None;
        let mut updated_target: Option<&str> = None;
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

        if let Some(ref target) = command.target {
            let target = target.trim();
            if target != subscription.target {
                subscription.target = target.to_string();
                updated_target = Some(target);
            }
        }

        if let Some(ref new_event_types) = command.event_types {
            let old_codes: HashSet<String> = subscription.event_types
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
            subscription.max_retries = retries;
        }

        if let Some(timeout) = command.timeout_seconds {
            subscription.timeout_seconds = timeout;
        }

        if let Some(data_only) = command.data_only {
            subscription.data_only = data_only;
        }

        subscription.updated_at = chrono::Utc::now();

        // Create domain event
        let event = SubscriptionUpdated::new(
            &ctx,
            &subscription.id,
            updated_name,
            updated_target,
            event_types_added,
            event_types_removed,
        );

        // Atomic commit
        self.unit_of_work.commit(&subscription, event, &command).await
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
            target: Some("https://new.example.com/webhook".to_string()),
            event_types: None,
            dispatch_pool_id: None,
            service_account_id: None,
            mode: None,
            max_retries: Some(10),
            timeout_seconds: None,
            data_only: None,
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("subscriptionId"));
        assert!(json.contains("New Name"));
    }
}
