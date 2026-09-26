//! Create Subscription Use Case

use async_trait::async_trait;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::SubscriptionCreated;
use crate::service_account::signing_reach::require_usable_signers;
use crate::shared::authorization_service::AuthContext;
use crate::shared::caller_reach::check_scope_access;
use crate::subscription::entity::{ConfigEntry, DispatchMode};
use crate::usecase::{ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult};
use crate::{ConnectionRepository, ServiceAccountRepository, SubscriptionRepository};
use crate::{EventTypeBinding, Subscription};

/// Subscription code pattern (Go `validate.CodePattern`): a lowercase
/// letter, then lowercase alphanumerics and hyphens.
fn code_pattern() -> &'static Regex {
    static PATTERN: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"^[a-z][a-z0-9-]*$").unwrap())
}

/// Go's delivery-target rule (`subscription/operations/create.go`):
/// `^https?://.+`, on the endpoint as sent.
pub(crate) fn is_http_url(endpoint: &str) -> bool {
    static PATTERN: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    PATTERN
        .get_or_init(|| Regex::new(r"^https?://.+").unwrap())
        .is_match(endpoint)
}

/// Go `dispatchqueue.Parse`: the dispatch priority in its canonical form,
/// matched ignoring case and surrounding space; blank is no priority
/// (`None`), anything else 400 `INVALID_QUEUE`.
pub(crate) fn parse_queue(raw: &str) -> Result<Option<String>, UseCaseError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    match trimmed.to_ascii_uppercase().as_str() {
        p @ ("DEFAULT" | "HIGH_PRIORITY") => Ok(Some(p.to_string())),
        _ => Err(UseCaseError::validation(
            "INVALID_QUEUE",
            "queue must be DEFAULT or HIGH_PRIORITY",
        )),
    }
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

    /// The event type's id, when the caller knows it (the SPA sends it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_type_id: Option<String>,

    /// The spec version bound to (the SPA sends the current one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec_version: Option<String>,
}

impl EventTypeBindingInput {
    /// The stored binding: code, id and spec version as sent (Go keeps
    /// them); the filter has no column.
    pub fn to_binding(&self) -> EventTypeBinding {
        let mut binding = EventTypeBinding::new(&self.event_type_code);
        binding.event_type_id = self.event_type_id.clone();
        binding.spec_version = self.spec_version.clone();
        if let Some(ref filter) = self.filter {
            binding = binding.with_filter(filter);
        }
        binding
    }
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

    /// Dispatch priority, DEFAULT or HIGH_PRIORITY (any case); blank or
    /// absent leaves it unset (Go `CreateCommand.Queue`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue: Option<String>,

    /// Delivery delay in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delay_seconds: Option<i32>,

    /// Maximum message age in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_age_seconds: Option<i32>,

    /// Custom configuration entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_config: Option<Vec<ConfigEntry>>,

    /// Who is creating it, for the scope check and the signing-reach check
    /// (never serialised, so never in the audit log). A named account or
    /// connection with no caller is refused.
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

    /// Go `CreateSubscription.Validate` (subscription/operations/create.go),
    /// its codes and messages.
    async fn validate(&self, command: &CreateSubscriptionCommand) -> Result<(), UseCaseError> {
        let code = command.code.trim().to_lowercase();
        if code.is_empty() {
            return Err(UseCaseError::validation(
                "CODE_REQUIRED",
                "code is required",
            ));
        }
        if !code_pattern().is_match(&code) {
            return Err(UseCaseError::validation(
                "INVALID_CODE_FORMAT",
                "code must start with a lowercase letter and contain only lowercase alphanumeric and hyphens",
            ));
        }
        if command.name.trim().is_empty() {
            return Err(UseCaseError::validation(
                "NAME_REQUIRED",
                "name is required",
            ));
        }
        if !is_http_url(&command.endpoint) {
            return Err(UseCaseError::validation(
                "INVALID_ENDPOINT",
                "endpoint must be a http(s) URL",
            ));
        }
        if command.event_types.is_empty() {
            return Err(UseCaseError::validation(
                "EVENT_TYPES_REQUIRED",
                "at least one event type binding is required",
            ));
        }
        if let Some(ref queue) = command.queue {
            parse_queue(queue)?;
        }
        Ok(())
    }

    /// Go `CheckScopeAccess` on the requested client (a platform-wide
    /// subscription needs anchor scope), then: a named service account or
    /// connection must exist and be one the caller may sign with (S7; Java
    /// `CreateSubscription`, b1ce6e55 and eac7ef57: no owning-application
    /// exemption).
    async fn authorize(
        &self,
        command: &CreateSubscriptionCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        if let Some(ref caller) = command.caller {
            check_scope_access(caller, command.client_id.as_deref())?;
        }
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

        // Business rule: the code is unique within (no application, this
        // client), the key a UI/API create writes (Go FindByCode).
        let existing = match self
            .subscription_repo
            .find_by_code_in_scope(&code, None, command.client_id.as_deref())
            .await
        {
            Ok(found) => found,
            Err(e) => return UseCaseResult::failure(e.into()),
        };

        if existing.is_some() {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "CODE_EXISTS",
                format!("Subscription with code '{}' already exists", code),
            ));
        }

        let bindings: Vec<EventTypeBinding> = command
            .event_types
            .iter()
            .map(EventTypeBindingInput::to_binding)
            .collect();

        // Create the subscription entity (the endpoint as sent, as Go).
        let mut subscription = Subscription::new(&code, name, &command.endpoint);
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
        if let Some(delay) = command.delay_seconds {
            subscription.delay_seconds = delay;
        }
        if let Some(max_age) = command.max_age_seconds {
            subscription.max_age_seconds = max_age;
        }
        if let Some(ref config) = command.custom_config {
            subscription.custom_config = config.clone();
        }
        // Stored canonically upper-case; validate rejected anything else.
        if let Some(ref queue) = command.queue {
            subscription.queue = parse_queue(queue).ok().flatten();
        }

        // Create domain event
        let event = SubscriptionCreated::new(
            &ctx,
            &subscription.id,
            &subscription.code,
            &subscription.name,
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
                event_type_id: None,
                spec_version: None,
            }],
            dispatch_pool_id: None,
            service_account_id: None,
            mode: None,
            max_retries: Some(5),
            timeout_seconds: Some(60),
            data_only: false,
            queue: None,
            delay_seconds: None,
            max_age_seconds: None,
            custom_config: None,
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
        assert!(pattern.is_match("a")); // Go's rule has no minimum
        assert!(!pattern.is_match("Order-Webhook")); // Uppercase
        assert!(!pattern.is_match("-order")); // Starts with hyphen
        assert!(!pattern.is_match("bad code"));
    }

    #[test]
    fn queue_and_endpoint_follow_go() {
        assert_eq!(
            parse_queue(" default ").unwrap().as_deref(),
            Some("DEFAULT")
        );
        assert_eq!(
            parse_queue("high_priority").unwrap().as_deref(),
            Some("HIGH_PRIORITY")
        );
        assert_eq!(parse_queue("  ").unwrap(), None);
        assert_eq!(
            parse_queue("workers-high").unwrap_err().code(),
            "INVALID_QUEUE"
        );
        assert!(is_http_url("https://x"));
        assert!(!is_http_url("not-a-url"));
        assert!(!is_http_url("https://"));
    }
}
