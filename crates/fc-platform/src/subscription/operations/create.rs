//! Create Subscription Use Case

use async_trait::async_trait;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::SubscriptionCreated;
use crate::service_account::signing_reach::require_usable_signers;
use crate::shared::authorization_service::AuthContext;
use crate::subscription::entity::DispatchMode;
use crate::usecase::{ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult};
use crate::{ConnectionRepository, ServiceAccountRepository, SubscriptionRepository};
use crate::{EventTypeBinding, Subscription};

/// Subscription code pattern: lowercase alphanumeric with hyphens
fn code_pattern() -> &'static Regex {
    static PATTERN: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"^[a-z][a-z0-9-]*[a-z0-9]$").unwrap())
}

/// Event type binding input for command
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventTypeBindingInput {
    /// Event type code (full or with wildcards)
    pub event_type_code: String,

    /// Optional filter expression
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
}

/// Command for creating a new subscription.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSubscriptionCommand {
    /// Unique code (lowercase alphanumeric with hyphens)
    pub code: String,

    /// Human-readable name
    pub name: String,

    /// Description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Client ID (optional - null for anchor-level subscriptions)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,

    /// Webhook endpoint URL
    pub endpoint: String,

    /// Connection ID (references msg_connections, optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection_id: Option<String>,

    /// Event types to subscribe to
    pub event_types: Vec<EventTypeBindingInput>,

    /// Dispatch pool ID (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispatch_pool_id: Option<String>,

    /// Service account ID for authentication (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_account_id: Option<String>,

    /// Dispatch mode
    #[serde(default)]
    pub mode: Option<DispatchMode>,

    /// Maximum retry attempts
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<u32>,

    /// Timeout in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u32>,

    /// Send raw event data only (no envelope)
    #[serde(default)]
    pub data_only: bool,

    /// Who is creating it, for the signing-reach check (never serialised,
    /// so never in the audit log). A named account or connection with no
    /// caller is refused.
    #[serde(skip)]
    pub caller: Option<AuthContext>,
}

impl crate::usecase::AuditMasked for CreateSubscriptionCommand {}

/// Use case for creating a new subscription.
pub struct CreateSubscriptionUseCase<U: UnitOfWork> {
    subscription_repo: Arc<SubscriptionRepository>,
    service_account_repo: Arc<ServiceAccountRepository>,
    connection_repo: Arc<ConnectionRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> CreateSubscriptionUseCase<U> {
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
impl<U: UnitOfWork> UseCase for CreateSubscriptionUseCase<U> {
    type Command = CreateSubscriptionCommand;
    type Event = SubscriptionCreated;

    async fn validate(&self, command: &CreateSubscriptionCommand) -> Result<(), UseCaseError> {
        let code = command.code.trim().to_lowercase();
        if code.is_empty() {
            return Err(UseCaseError::validation(
                "CODE_REQUIRED",
                "Subscription code is required",
            ));
        }

        if code.len() < 2 || !code_pattern().is_match(&code) {
            return Err(UseCaseError::validation(
                "INVALID_CODE_FORMAT",
                "Subscription code must be lowercase alphanumeric with hyphens (min 2 chars)",
            ));
        }

        let name = command.name.trim();
        if name.is_empty() {
            return Err(UseCaseError::validation(
                "NAME_REQUIRED",
                "Subscription name is required",
            ));
        }

        let endpoint = command.endpoint.trim();
        if endpoint.is_empty() {
            return Err(UseCaseError::validation(
                "ENDPOINT_REQUIRED",
                "Endpoint URL is required",
            ));
        }

        if command.event_types.is_empty() {
            return Err(UseCaseError::validation(
                "EVENT_TYPES_REQUIRED",
                "At least one event type is required",
            ));
        }

        Ok(())
    }

    /// A named service account or connection must exist and be one the
    /// caller may sign with (S7; Java `CreateSubscription`, b1ce6e55 and
    /// eac7ef57: no owning-application exemption).
    async fn authorize(
        &self,
        command: &CreateSubscriptionCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        require_usable_signers(
            command.caller.as_ref(),
            &self.service_account_repo,
            &self.connection_repo,
            command.service_account_id.as_deref(),
            true,
            command.connection_id.as_deref(),
            true,
        )
        .await
    }

    async fn execute(
        &self,
        command: CreateSubscriptionCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<SubscriptionCreated> {
        let code = command.code.trim().to_lowercase();
        let name = command.name.trim();
        let endpoint = command.endpoint.trim();

        // Business rule: code must be unique within client scope
        let existing = match self
            .subscription_repo
            .find_by_code_and_client(&code, command.client_id.as_deref())
            .await
        {
            Ok(found) => found,
            Err(e) => return UseCaseResult::failure(e.into()),
        };

        if existing.is_some() {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "SUBSCRIPTION_CODE_EXISTS",
                format!("A subscription with code '{}' already exists", code),
            ));
        }

        // Build event type bindings
        let bindings: Vec<EventTypeBinding> = command
            .event_types
            .iter()
            .map(|input| {
                let mut binding = EventTypeBinding::new(&input.event_type_code);
                if let Some(ref filter) = input.filter {
                    binding = binding.with_filter(filter);
                }
                binding
            })
            .collect();

        // Create the subscription entity
        let mut subscription = Subscription::new(&code, name, endpoint);
        subscription.connection_id = command.connection_id.clone();

        subscription.description = command.description.clone();
        subscription.client_id = command.client_id.clone();
        subscription.event_types = bindings;
        subscription.dispatch_pool_id = command.dispatch_pool_id.clone();
        subscription.service_account_id = command.service_account_id.clone();
        subscription.data_only = command.data_only;
        subscription.created_by = Some(ctx.principal_id.clone());

        if let Some(mode) = command.mode {
            subscription.mode = mode;
        }
        if let Some(retries) = command.max_retries {
            subscription.max_retries = retries as i32;
        }
        if let Some(timeout) = command.timeout_seconds {
            subscription.timeout_seconds = timeout as i32;
        }

        // Create domain event
        let event_type_codes: Vec<String> = subscription
            .event_types
            .iter()
            .map(|b| b.event_type_code.clone())
            .collect();

        let event = SubscriptionCreated::new(
            &ctx,
            &subscription.id,
            &subscription.code,
            &subscription.name,
            &subscription.endpoint,
            event_type_codes,
            subscription.client_id.as_deref(),
        );

        // Atomic commit
        self.unit_of_work
            .commit(&subscription, &*self.subscription_repo, event, &command)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usecase::unit_of_work::HasId;

    #[test]
    fn test_command_serialization() {
        let cmd = CreateSubscriptionCommand {
            code: "order-webhook".to_string(),
            name: "Order Webhook".to_string(),
            description: Some("Receives order events".to_string()),
            client_id: Some("client-123".to_string()),
            endpoint: "https://example.com/webhook".to_string(),
            connection_id: Some("conn-123".to_string()),
            event_types: vec![EventTypeBindingInput {
                event_type_code: "orders:*:*:*".to_string(),
                filter: None,
            }],
            dispatch_pool_id: None,
            service_account_id: None,
            mode: None,
            max_retries: Some(5),
            timeout_seconds: Some(60),
            data_only: false,
            caller: None,
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("order-webhook"));
        assert!(json.contains("Order Webhook"));
    }

    #[test]
    fn test_subscription_has_id() {
        let subscription = Subscription::new("test", "Test", "https://example.com/test");
        assert!(!subscription.id().is_empty());
    }

    #[test]
    fn test_code_pattern() {
        let pattern = code_pattern();
        assert!(pattern.is_match("order-webhook"));
        assert!(pattern.is_match("my-sub-1"));
        assert!(pattern.is_match("ab"));
        assert!(!pattern.is_match("a")); // Too short for pattern
        assert!(!pattern.is_match("Order-Webhook")); // Uppercase
        assert!(!pattern.is_match("-order")); // Starts with hyphen
    }
}
