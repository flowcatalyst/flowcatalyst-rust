//! Sync Dispatch Pools Use Case
//!
//! Bulk creates/updates/archives dispatch pools from an application SDK.

use std::sync::Arc;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::DispatchPool;
use crate::DispatchPoolRepository;
use crate::usecase::{
    ExecutionContext, UseCaseError, UseCaseResult,
};
use super::events::DispatchPoolsSynced;

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
    #[serde(default = "default_rate_limit")]
    pub rate_limit: u32,
    #[serde(default = "default_concurrency")]
    pub concurrency: u32,
}

fn default_rate_limit() -> u32 { 100 }
fn default_concurrency() -> u32 { 10 }

/// Command for syncing dispatch pools from an application.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncDispatchPoolsCommand {
    pub application_code: String,
    pub pools: Vec<SyncDispatchPoolInput>,
    #[serde(default)]
    pub remove_unlisted: bool,
}

pub struct SyncDispatchPoolsUseCase {
    dispatch_pool_repo: Arc<DispatchPoolRepository>,
}

impl SyncDispatchPoolsUseCase {
    pub fn new(dispatch_pool_repo: Arc<DispatchPoolRepository>) -> Self {
        Self { dispatch_pool_repo }
    }

    pub async fn execute(
        &self,
        command: SyncDispatchPoolsCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<DispatchPoolsSynced> {
        if command.application_code.trim().is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "APPLICATION_CODE_REQUIRED", "Application code is required",
            ));
        }

        // Validate inputs
        let pattern = pool_code_pattern();
        for input in &command.pools {
            if input.code.trim().is_empty() || !pattern.is_match(&input.code) {
                return UseCaseResult::failure(UseCaseError::validation(
                    "INVALID_POOL_CODE",
                    format!("Pool code '{}' is invalid. Must start with lowercase letter, contain only lowercase alphanumeric, hyphens, underscores.", input.code),
                ));
            }
            if input.name.trim().is_empty() {
                return UseCaseResult::failure(UseCaseError::validation(
                    "NAME_REQUIRED", "Pool name is required",
                ));
            }
            if input.rate_limit < 1 {
                return UseCaseResult::failure(UseCaseError::validation(
                    "INVALID_RATE_LIMIT", "Rate limit must be at least 1",
                ));
            }
            if input.concurrency < 1 {
                return UseCaseResult::failure(UseCaseError::validation(
                    "INVALID_CONCURRENCY", "Concurrency must be at least 1",
                ));
            }
        }

        // Fetch existing pools
        let existing = match self.dispatch_pool_repo.find_all().await {
            Ok(list) => list,
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to fetch existing pools: {}", e
                )));
            }
        };

        let mut created_count = 0u32;
        let mut updated_count = 0u32;
        let mut deleted_count = 0u32;
        let mut synced_codes: Vec<String> = Vec::new();

        for input in &command.pools {
            synced_codes.push(input.code.clone());

            let existing_pool = existing.iter().find(|p| p.code == input.code);
            match existing_pool {
                Some(pool) => {
                    let mut updated = pool.clone();
                    updated.name = input.name.clone();
                    updated.description = input.description.clone();
                    updated.rate_limit = input.rate_limit as i32;
                    updated.concurrency = input.concurrency as i32;
                    updated.updated_at = chrono::Utc::now();
                    if let Err(e) = self.dispatch_pool_repo.update(&updated).await {
                        return UseCaseResult::failure(UseCaseError::commit(format!(
                            "Failed to update pool '{}': {}", input.code, e
                        )));
                    }
                    updated_count += 1;
                }
                None => {
                    let mut pool = DispatchPool::new(&input.code, &input.name);
                    pool.description = input.description.clone();
                    pool.rate_limit = input.rate_limit as i32;
                    pool.concurrency = input.concurrency as i32;
                    if let Err(e) = self.dispatch_pool_repo.insert(&pool).await {
                        return UseCaseResult::failure(UseCaseError::commit(format!(
                            "Failed to create pool '{}': {}", input.code, e
                        )));
                    }
                    created_count += 1;
                }
            }
        }

        // Archive unlisted pools (not hard delete)
        if command.remove_unlisted {
            for pool in &existing {
                if !synced_codes.contains(&pool.code)
                    && pool.status != crate::DispatchPoolStatus::Archived
                {
                    let mut archived = pool.clone();
                    archived.archive();
                    if let Err(e) = self.dispatch_pool_repo.update(&archived).await {
                        return UseCaseResult::failure(UseCaseError::commit(format!(
                            "Failed to archive pool '{}': {}", pool.code, e
                        )));
                    }
                    deleted_count += 1;
                }
            }
        }

        let event = DispatchPoolsSynced::new(
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
        let cmd = SyncDispatchPoolsCommand {
            application_code: "orders".to_string(),
            pools: vec![],
            remove_unlisted: false,
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
