//! Sync Subscriptions Use Case
//!
//! Bulk creates/updates/deletes anchor-level subscriptions from an application SDK.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use super::create::EventTypeBindingInput;
use super::events::SubscriptionsSynced;
use crate::subscription::entity::SubscriptionSource;
use crate::usecase::{ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult};
use crate::ConnectionRepository;
use crate::DispatchPoolRepository;
use crate::SubscriptionRepository;
use crate::{DispatchPool, EventTypeBinding, Subscription};

/// A single subscription definition in the sync payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncSubscriptionInput {
    pub code: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection_id: Option<String>,
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

impl crate::usecase::AuditMasked for SyncSubscriptionsCommand {}

pub struct SyncSubscriptionsUseCase<U: UnitOfWork> {
    subscription_repo: Arc<SubscriptionRepository>,
    connection_repo: Arc<ConnectionRepository>,
    dispatch_pool_repo: Arc<DispatchPoolRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> SyncSubscriptionsUseCase<U> {
    pub fn new(
        subscription_repo: Arc<SubscriptionRepository>,
        connection_repo: Arc<ConnectionRepository>,
        dispatch_pool_repo: Arc<DispatchPoolRepository>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self {
            subscription_repo,
            connection_repo,
            dispatch_pool_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for SyncSubscriptionsUseCase<U> {
    type Command = SyncSubscriptionsCommand;
    type Event = SubscriptionsSynced;

    async fn validate(&self, command: &SyncSubscriptionsCommand) -> Result<(), UseCaseError> {
        if command.application_code.trim().is_empty() {
            return Err(UseCaseError::validation(
                "APPLICATION_CODE_REQUIRED",
                "Application code is required",
            ));
        }

        for input in &command.subscriptions {
            if input.code.trim().is_empty() {
                return Err(UseCaseError::validation(
                    "CODE_REQUIRED",
                    "Subscription code is required",
                ));
            }
            if input.name.trim().is_empty() {
                return Err(UseCaseError::validation(
                    "NAME_REQUIRED",
                    "Subscription name is required",
                ));
            }
            if input.target.trim().is_empty() {
                return Err(UseCaseError::validation(
                    "TARGET_REQUIRED",
                    "Target endpoint URL is required",
                ));
            }
            if input.event_types.is_empty() {
                return Err(UseCaseError::validation(
                    "EVENT_TYPES_REQUIRED",
                    "At least one event type is required",
                ));
            }
        }

        Ok(())
    }

    async fn authorize(
        &self,
        _command: &SyncSubscriptionsCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: SyncSubscriptionsCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<SubscriptionsSynced> {
        let event = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        self.unit_of_work.emit_event(event, &command).await
    }
}

impl<U: UnitOfWork> SyncSubscriptionsUseCase<U> {
    async fn prepare(
        &self,
        command: &SyncSubscriptionsCommand,
        ctx: &ExecutionContext,
    ) -> Result<SubscriptionsSynced, UseCaseError> {
        // Every named connection, in one query: it must exist, and its scope
        // must be consistent with the subscription's (Go
        // subscription/operations/sync.go:224-243, ruling 2026-09-21 #5).
        // An application's synced subscriptions are client-less here, so a
        // connection scoped to any client is a mismatch: a bare id could
        // otherwise borrow another tenant's connection and its account.
        let connection_ids: Vec<String> = command
            .subscriptions
            .iter()
            .filter_map(|i| i.connection_id.clone())
            .collect();
        let connections: HashMap<String, crate::Connection> = self
            .connection_repo
            .find_by_ids(&connection_ids)
            .await?
            .into_iter()
            .map(|c| (c.id.clone(), c))
            .collect();
        for input in &command.subscriptions {
            if let Some(ref conn_id) = input.connection_id {
                let connection = connections.get(conn_id).ok_or_else(|| {
                    // Go's own spelling here (subscription/operations/
                    // sync.go:223), not `Connection_NOT_FOUND`.
                    UseCaseError::not_found_verbatim(
                        "CONNECTION_NOT_FOUND",
                        format!("Connection '{}' not found", conn_id),
                    )
                })?;
                if connection.client_id.is_some() {
                    return Err(UseCaseError::validation(
                        "CONNECTION_SCOPE_MISMATCH",
                        format!(
                            "Subscription '{}': connection '{}' is scoped to a different client",
                            input.code, conn_id
                        ),
                    ));
                }
            }
        }

        // Resolve every referenced dispatch pool in one query, before any
        // write. An unknown code is a validation error naming it, rather
        // than a subscription silently created without its pool.
        let pool_codes = requested_pool_codes(&command.subscriptions);
        let pools: HashMap<String, DispatchPool> = self
            .dispatch_pool_repo
            .find_anchor_by_codes(&pool_codes)
            .await?
            .into_iter()
            .map(|p| (p.code.clone(), p))
            .collect();
        let missing = missing_pool_codes(&pool_codes, &pools);
        if !missing.is_empty() {
            return Err(UseCaseError::validation(
                "DISPATCH_POOL_NOT_FOUND",
                format!("Unknown dispatch pool code(s): {}", missing.join(", ")),
            ));
        }

        // Fetch existing anchor-level subscriptions for this application
        let existing = self
            .subscription_repo
            .find_by_application_code(&command.application_code)
            .await?;

        let mut created_count = 0u32;
        let mut updated_count = 0u32;
        let mut deleted_count = 0u32;
        let mut synced_codes: Vec<String> = Vec::new();

        for input in &command.subscriptions {
            synced_codes.push(input.code.clone());

            let bindings: Vec<EventTypeBinding> = input
                .event_types
                .iter()
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
                    if sub.source == SubscriptionSource::Api
                        || sub.source == SubscriptionSource::Code
                    {
                        let mut updated = sub.clone();
                        updated.name = input.name.clone();
                        updated.description = input.description.clone();
                        updated.endpoint = input.target.clone();
                        updated.connection_id = input.connection_id.clone();
                        updated.event_types = bindings;
                        updated.data_only = input.data_only;
                        if let Some(retries) = input.max_retries {
                            updated.max_retries = retries as i32;
                        }
                        if let Some(timeout) = input.timeout_seconds {
                            updated.timeout_seconds = timeout as i32;
                        }
                        // Pool codes were all resolved above.
                        if let Some(pool) = requested_pool_code(input).and_then(|c| pools.get(c)) {
                            updated.dispatch_pool_id = Some(pool.id.clone());
                            updated.dispatch_pool_code = Some(pool.code.clone());
                        }
                        updated.updated_at = chrono::Utc::now();
                        if let Err(e) = self.subscription_repo.update(&updated).await {
                            return Err(UseCaseError::commit(format!(
                                "Failed to update subscription '{}': {}",
                                input.code, e
                            )));
                        }
                        updated_count += 1;
                    }
                }
                None => {
                    let mut sub = Subscription::new(&input.code, &input.name, &input.target);
                    sub.connection_id = input.connection_id.clone();
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
                    // Ruling X-01: absent means NEXT_ON_ERROR, unknown means
                    // NEXT_ON_ERROR with a warning. An existing subscription's
                    // mode is left alone on update, as before.
                    sub.mode =
                        crate::dispatch_job::entity::parse_dispatch_mode(input.mode.as_deref());
                    if let Some(pool) = requested_pool_code(input).and_then(|c| pools.get(c)) {
                        sub.dispatch_pool_id = Some(pool.id.clone());
                        sub.dispatch_pool_code = Some(pool.code.clone());
                    }
                    if let Err(e) = self.subscription_repo.insert(&sub).await {
                        return Err(UseCaseError::commit(format!(
                            "Failed to create subscription '{}': {}",
                            input.code, e
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
                        return Err(UseCaseError::commit(format!(
                            "Failed to delete subscription '{}': {}",
                            sub.code, e
                        )));
                    }
                    deleted_count += 1;
                }
            }
        }

        let event = SubscriptionsSynced {
            metadata: SubscriptionsSynced::metadata_for(ctx, &command.application_code),
            application_code: command.application_code.clone(),
            created: created_count,
            updated: updated_count,
            deleted: deleted_count,
            synced_codes,
        };
        Ok(event)
    }
}

/// The dispatch pool code an input asks for. Blank means none, as in Go.
fn requested_pool_code(input: &SyncSubscriptionInput) -> Option<&str> {
    input
        .dispatch_pool_code
        .as_deref()
        .filter(|c| !c.trim().is_empty())
}

/// Every distinct pool code the payload references, sorted.
fn requested_pool_codes(inputs: &[SyncSubscriptionInput]) -> Vec<String> {
    let mut codes: Vec<String> = inputs
        .iter()
        .filter_map(requested_pool_code)
        .map(String::from)
        .collect();
    codes.sort();
    codes.dedup();
    codes
}

/// The requested codes with no pool, in request order.
fn missing_pool_codes<'a>(
    requested: &'a [String],
    found: &HashMap<String, DispatchPool>,
) -> Vec<&'a str> {
    requested
        .iter()
        .filter(|c| !found.contains_key(*c))
        .map(String::as_str)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(code: &str, pool: Option<&str>) -> SyncSubscriptionInput {
        SyncSubscriptionInput {
            code: code.to_string(),
            name: code.to_string(),
            description: None,
            target: "https://example.com/hook".to_string(),
            connection_id: None,
            event_types: vec![],
            dispatch_pool_code: pool.map(String::from),
            mode: None,
            max_retries: None,
            timeout_seconds: None,
            data_only: false,
        }
    }

    #[test]
    fn pool_codes_are_distinct_and_skip_blank() {
        let inputs = vec![
            input("a", Some("fast")),
            input("b", None),
            input("c", Some("  ")),
            input("d", Some("slow")),
            input("e", Some("fast")),
        ];
        assert_eq!(requested_pool_codes(&inputs), vec!["fast", "slow"]);
    }

    #[test]
    fn unknown_pool_codes_are_reported() {
        let requested = vec!["fast".to_string(), "nope".to_string(), "slow".to_string()];
        let mut found = HashMap::new();
        for code in ["fast", "slow"] {
            found.insert(code.to_string(), DispatchPool::new(code, code));
        }
        assert_eq!(missing_pool_codes(&requested, &found), vec!["nope"]);
        found.remove("slow");
        assert_eq!(missing_pool_codes(&requested, &found), vec!["nope", "slow"]);
    }

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
