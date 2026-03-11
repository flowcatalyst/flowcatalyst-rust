//! Sync Subscriptions Use Case
//!
//! Bulk creates/updates/deletes anchor-level subscriptions from an application SDK.

use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::{Subscription, EventTypeBinding};
use crate::subscription::entity::SubscriptionSource;
use crate::SubscriptionRepository;
use crate::ConnectionRepository;
use crate::DispatchPoolRepository;
use crate::usecase::{
    ExecutionContext, UseCaseError, UseCaseResult,
};
use super::events::SubscriptionsSynced;
use super::create::EventTypeBindingInput;

/// A single subscription definition in the sync payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncSubscriptionInput {
    pub code: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub connection_id: String,
    pub event_types: Vec<EventTypeBindingInput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispatch_pool_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u32>,
    #[serde(default)]
    pub data_only: bool,
}

/// Command for syncing subscriptions from an application.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncSubscriptionsCommand {
    pub application_code: String,
    pub subscriptions: Vec<SyncSubscriptionInput>,
    #[serde(default)]
    pub remove_unlisted: bool,
}

pub struct SyncSubscriptionsUseCase {
    subscription_repo: Arc<SubscriptionRepository>,
    connection_repo: Arc<ConnectionRepository>,
    dispatch_pool_repo: Arc<DispatchPoolRepository>,
}

impl SyncSubscriptionsUseCase {
    pub fn new(
        subscription_repo: Arc<SubscriptionRepository>,
        connection_repo: Arc<ConnectionRepository>,
        dispatch_pool_repo: Arc<DispatchPoolRepository>,
    ) -> Self {
        Self { subscription_repo, connection_repo, dispatch_pool_repo }
    }

    pub async fn execute(
        &self,
        command: SyncSubscriptionsCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<SubscriptionsSynced> {
        if command.application_code.trim().is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "APPLICATION_CODE_REQUIRED", "Application code is required",
            ));
        }

        // Validate inputs
        for input in &command.subscriptions {
            if input.code.trim().is_empty() {
                return UseCaseResult::failure(UseCaseError::validation(
                    "CODE_REQUIRED", "Subscription code is required",
                ));
            }
            if input.name.trim().is_empty() {
                return UseCaseResult::failure(UseCaseError::validation(
                    "NAME_REQUIRED", "Subscription name is required",
                ));
            }
            if input.connection_id.trim().is_empty() {
                return UseCaseResult::failure(UseCaseError::validation(
                    "CONNECTION_ID_REQUIRED", "Connection ID is required",
                ));
            }
            if input.event_types.is_empty() {
                return UseCaseResult::failure(UseCaseError::validation(
                    "EVENT_TYPES_REQUIRED", "At least one event type is required",
                ));
            }
            // Validate connection exists
            match self.connection_repo.find_by_id(&input.connection_id).await {
                Ok(Some(_)) => {}
                Ok(None) => {
                    return UseCaseResult::failure(UseCaseError::not_found(
                        "CONNECTION_NOT_FOUND",
                        format!("Connection '{}' not found", input.connection_id),
                    ));
                }
                Err(e) => {
                    return UseCaseResult::failure(UseCaseError::commit(format!(
                        "Failed to validate connection: {}", e
                    )));
                }
            }
        }

        // Fetch existing anchor-level subscriptions for this application
        let existing = match self.subscription_repo.find_by_application_code(&command.application_code).await {
            Ok(list) => list,
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to fetch existing subscriptions: {}", e
                )));
            }
        };

        let mut created_count = 0u32;
        let mut updated_count = 0u32;
        let mut deleted_count = 0u32;
        let mut synced_codes: Vec<String> = Vec::new();

        for input in &command.subscriptions {
            synced_codes.push(input.code.clone());

            let bindings: Vec<EventTypeBinding> = input.event_types.iter()
                .map(|et| {
                    let mut b = EventTypeBinding::new(&et.event_type_code);
                    if let Some(ref f) = et.filter {
                        b = b.with_filter(f);
                    }
                    b
                })
                .collect();

            let existing_sub = existing.iter().find(|s| s.code == input.code);
            match existing_sub {
                Some(sub) => {
                    // Only update API-sourced subscriptions
                    if sub.source == SubscriptionSource::Api || sub.source == SubscriptionSource::Code {
                        let mut updated = sub.clone();
                        updated.name = input.name.clone();
                        updated.description = input.description.clone();
                        updated.connection_id = input.connection_id.clone();
                        updated.event_types = bindings;
                        updated.data_only = input.data_only;
                        if let Some(retries) = input.max_retries {
                            updated.max_retries = retries as i32;
                        }
                        if let Some(timeout) = input.timeout_seconds {
                            updated.timeout_seconds = timeout as i32;
                        }
                        // Resolve dispatch pool by code if provided
                        if let Some(ref pool_code) = input.dispatch_pool_code {
                            if let Ok(Some(pool)) = self.dispatch_pool_repo.find_by_code(pool_code, None).await {
                                updated.dispatch_pool_id = Some(pool.id);
                                updated.dispatch_pool_code = Some(pool.code);
                            }
                        }
                        updated.updated_at = chrono::Utc::now();
                        if let Err(e) = self.subscription_repo.update(&updated).await {
                            return UseCaseResult::failure(UseCaseError::commit(format!(
                                "Failed to update subscription '{}': {}", input.code, e
                            )));
                        }
                        updated_count += 1;
                    }
                }
                None => {
                    let mut sub = Subscription::new(&input.code, &input.name, &input.connection_id);
                    sub.application_code = Some(command.application_code.clone());
                    sub.source = SubscriptionSource::Api;
                    sub.description = input.description.clone();
                    sub.event_types = bindings;
                    sub.data_only = input.data_only;
                    sub.created_by = Some(ctx.principal_id.clone());
                    if let Some(retries) = input.max_retries {
                        sub.max_retries = retries as i32;
                    }
                    if let Some(timeout) = input.timeout_seconds {
                        sub.timeout_seconds = timeout as i32;
                    }
                    if let Some(ref pool_code) = input.dispatch_pool_code {
                        if let Ok(Some(pool)) = self.dispatch_pool_repo.find_by_code(pool_code, None).await {
                            sub.dispatch_pool_id = Some(pool.id);
                            sub.dispatch_pool_code = Some(pool.code);
                        }
                    }
                    if let Err(e) = self.subscription_repo.insert(&sub).await {
                        return UseCaseResult::failure(UseCaseError::commit(format!(
                            "Failed to create subscription '{}': {}", input.code, e
                        )));
                    }
                    created_count += 1;
                }
            }
        }

        // Remove unlisted API-sourced subscriptions
        if command.remove_unlisted {
            for sub in &existing {
                if (sub.source == SubscriptionSource::Api || sub.source == SubscriptionSource::Code)
                    && !synced_codes.contains(&sub.code)
                {
                    if let Err(e) = self.subscription_repo.delete(&sub.id).await {
                        return UseCaseResult::failure(UseCaseError::commit(format!(
                            "Failed to delete subscription '{}': {}", sub.code, e
                        )));
                    }
                    deleted_count += 1;
                }
            }
        }

        let event = SubscriptionsSynced::new(
            &ctx,
            &command.application_code,
            created_count,
            updated_count,
            deleted_count,
            synced_codes,
        );

        UseCaseResult::success(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = SyncSubscriptionsCommand {
            application_code: "orders".to_string(),
            subscriptions: vec![],
            remove_unlisted: false,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("orders"));
    }
}
