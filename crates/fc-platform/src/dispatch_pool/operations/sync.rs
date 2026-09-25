//! Sync Dispatch Pools Use Case
//!
//! Bulk creates/updates/archives dispatch pools from an application SDK.

use async_trait::async_trait;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::{
    DispatchPoolArchived, DispatchPoolCreated, DispatchPoolUpdated, DispatchPoolsSynced,
};
use crate::usecase::{
    ExecutionContext, RecordedEvent, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::DispatchPool;
use crate::DispatchPoolRepository;

fn pool_code_pattern() -> &'static Regex {
    static PATTERN: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"^[a-z][a-z0-9_-]*$").unwrap())
}

/// A single dispatch pool definition in the sync payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncDispatchPoolInput {
    pub code: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// `None` means concurrency-only (router runs the pool with no rate limiter).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<u32>,
    #[serde(default = "default_concurrency")]
    pub concurrency: u32,
}

fn default_concurrency() -> u32 {
    10
}

/// Command for syncing dispatch pools from an application.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncDispatchPoolsCommand {
    pub application_code: String,
    pub pools: Vec<SyncDispatchPoolInput>,
    #[serde(default)]
    pub remove_unlisted: bool,
    /// Pools this sync must leave alone: a function's own pool (Java
    /// `protectedIds`, `function-invocation.md` §4.2). Neither updated when
    /// listed nor archived when unlisted, and neither counted.
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    pub protected_ids: std::collections::BTreeSet<String>,
}

impl crate::usecase::AuditMasked for SyncDispatchPoolsCommand {}

pub struct SyncDispatchPoolsUseCase<U: UnitOfWork> {
    dispatch_pool_repo: Arc<DispatchPoolRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> SyncDispatchPoolsUseCase<U> {
    pub fn new(dispatch_pool_repo: Arc<DispatchPoolRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            dispatch_pool_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for SyncDispatchPoolsUseCase<U> {
    type Command = SyncDispatchPoolsCommand;
    type Event = DispatchPoolsSynced;

    async fn validate(&self, command: &SyncDispatchPoolsCommand) -> Result<(), UseCaseError> {
        if command.application_code.trim().is_empty() {
            return Err(UseCaseError::validation(
                "APPLICATION_CODE_REQUIRED",
                "Application code is required",
            ));
        }

        let pattern = pool_code_pattern();
        for input in &command.pools {
            if input.code.trim().is_empty() || !pattern.is_match(&input.code) {
                return Err(UseCaseError::validation(
                    "INVALID_POOL_CODE",
                    format!("Pool code '{}' is invalid. Must start with lowercase letter, contain only lowercase alphanumeric, hyphens, underscores.", input.code),
                ));
            }
            if input.name.trim().is_empty() {
                return Err(UseCaseError::validation(
                    "NAME_REQUIRED",
                    "Pool name is required",
                ));
            }
            if let Some(rl) = input.rate_limit {
                if rl < 1 {
                    return Err(UseCaseError::validation(
                        "INVALID_RATE_LIMIT",
                        "Rate limit, when set, must be at least 1",
                    ));
                }
            }
            if input.concurrency < 1 {
                return Err(UseCaseError::validation(
                    "INVALID_CONCURRENCY",
                    "Concurrency must be at least 1",
                ));
            }
        }

        Ok(())
    }

    async fn authorize(
        &self,
        _command: &SyncDispatchPoolsCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: SyncDispatchPoolsCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<DispatchPoolsSynced> {
        let (rows, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        // Go's usecaseop.Sync: a created/updated/archived event per synced
        // pool, then the rollup.
        self.unit_of_work.emit_events(rows, event, &command).await
    }
}

impl<U: UnitOfWork> SyncDispatchPoolsUseCase<U> {
    async fn prepare(
        &self,
        command: &SyncDispatchPoolsCommand,
        ctx: &ExecutionContext,
    ) -> Result<(Vec<RecordedEvent>, DispatchPoolsSynced), UseCaseError> {
        // Fetch existing pools
        let existing = self.dispatch_pool_repo.find_all().await?;

        let mut created_count = 0u32;
        let mut updated_count = 0u32;
        let mut deleted_count = 0u32;
        let mut synced_codes: Vec<String> = Vec::new();
        let mut rows: Vec<RecordedEvent> = Vec::new();

        for input in &command.pools {
            synced_codes.push(input.code.clone());

            let existing_pool = existing.iter().find(|p| p.code == input.code);
            match existing_pool {
                Some(pool) if command.protected_ids.contains(&pool.id) => {}
                Some(pool) => {
                    let mut updated = pool.clone();
                    updated.name = input.name.clone();
                    updated.description = input.description.clone();
                    updated.rate_limit = input.rate_limit.map(|r| r as i32);
                    updated.concurrency = input.concurrency as i32;
                    updated.updated_at = chrono::Utc::now();
                    if let Err(e) = self.dispatch_pool_repo.update(&updated).await {
                        return Err(UseCaseError::commit(format!(
                            "Failed to update pool '{}': {}",
                            input.code, e
                        )));
                    }
                    rows.push(RecordedEvent::of(&DispatchPoolUpdated::new(
                        ctx,
                        &updated.id,
                        &updated.name,
                    ))?);
                    updated_count += 1;
                }
                None => {
                    let mut pool = DispatchPool::new(&input.code, &input.name);
                    pool.description = input.description.clone();
                    pool.rate_limit = input.rate_limit.map(|r| r as i32);
                    pool.concurrency = input.concurrency as i32;
                    if let Err(e) = self.dispatch_pool_repo.insert(&pool).await {
                        return Err(UseCaseError::commit(format!(
                            "Failed to create pool '{}': {}",
                            input.code, e
                        )));
                    }
                    rows.push(RecordedEvent::of(&DispatchPoolCreated::new(
                        ctx, &pool.id, &pool.code, &pool.name,
                    ))?);
                    created_count += 1;
                }
            }
        }

        // Archive unlisted pools (not hard delete)
        if command.remove_unlisted {
            for pool in &existing {
                if !synced_codes.contains(&pool.code)
                    && pool.status != crate::DispatchPoolStatus::Archived
                    && !command.protected_ids.contains(&pool.id)
                {
                    let mut archived = pool.clone();
                    archived.archive();
                    if let Err(e) = self.dispatch_pool_repo.update(&archived).await {
                        return Err(UseCaseError::commit(format!(
                            "Failed to archive pool '{}': {}",
                            pool.code, e
                        )));
                    }
                    rows.push(RecordedEvent::of(&DispatchPoolArchived::new(
                        ctx,
                        &archived.id,
                        &archived.code,
                    ))?);
                    deleted_count += 1;
                }
            }
        }

        let event = DispatchPoolsSynced {
            metadata: DispatchPoolsSynced::metadata_for(ctx, &command.application_code),
            application_code: command.application_code.clone(),
            created: created_count,
            updated: updated_count,
            deleted: deleted_count,
            synced_codes,
        };
        Ok((rows, event))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = SyncDispatchPoolsCommand {
            application_code: "orders".to_string(),
            pools: vec![],
            remove_unlisted: false,
            protected_ids: Default::default(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("orders"));
    }

    #[test]
    fn test_pool_code_pattern() {
        let pattern = pool_code_pattern();
        assert!(pattern.is_match("my-pool"));
        assert!(pattern.is_match("pool_1"));
        assert!(!pattern.is_match("My-Pool"));
        assert!(!pattern.is_match("-pool"));
    }
}
