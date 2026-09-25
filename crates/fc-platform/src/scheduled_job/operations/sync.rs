//! Sync ScheduledJobs use case.
//!
//! Bulk upsert of definitions from an SDK or admin caller, scoped to a
//! `(scope, client_id)` pair. Behaviour:
//!   * Each entry's `(client_id, code)` is matched against existing rows.
//!   * Missing → CREATE. Present + changed → UPDATE. Present + unchanged → no-op.
//!   * If `archive_unlisted = true`, ACTIVE jobs in the scope that aren't in
//!     the payload are ARCHIVED. Default is false (additive sync).
//!
//! Emits a single `ScheduledJobsSynced` summary event regardless of how many
//! rows changed.

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::events::ScheduledJobsSynced;
use crate::scheduled_job::entity::{ScheduledJob, ScheduledJobStatus};
use crate::scheduled_job::ScheduledJobRepository;
use crate::usecase::{ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledJobSyncEntry {
    pub code: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub crons: Vec<String>,
    #[serde(default = "default_timezone")]
    pub timezone: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
    #[serde(default)]
    pub concurrent: bool,
    #[serde(default)]
    pub tracks_completion: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<i32>,
    #[serde(default = "default_attempts")]
    pub delivery_max_attempts: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_url: Option<String>,
}
fn default_timezone() -> String {
    "UTC".into()
}
fn default_attempts() -> i32 {
    3
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncScheduledJobsCommand {
    /// Logical scope label for the sync run (typically the application code,
    /// or "platform" for anchor-driven syncs). Used in audit/log output.
    pub scope: String,
    /// None = platform-scoped jobs (anchor only); Some = client-scoped.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    pub jobs: Vec<ScheduledJobSyncEntry>,
    #[serde(default)]
    pub archive_unlisted: bool,
    /// Jobs this sync must leave alone: a function's own schedules (Java
    /// `protectedIds`, `function-invocation.md` §4.2). Neither updated when
    /// listed nor archived when unlisted.
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    pub protected_ids: std::collections::BTreeSet<String>,
}

impl crate::usecase::AuditMasked for SyncScheduledJobsCommand {}

pub struct SyncScheduledJobsUseCase<U: UnitOfWork> {
    repo: Arc<ScheduledJobRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> SyncScheduledJobsUseCase<U> {
    pub fn new(repo: Arc<ScheduledJobRepository>, unit_of_work: Arc<U>) -> Self {
        Self { repo, unit_of_work }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for SyncScheduledJobsUseCase<U> {
    type Command = SyncScheduledJobsCommand;
    type Event = ScheduledJobsSynced;

    async fn validate(&self, cmd: &Self::Command) -> Result<(), UseCaseError> {
        if cmd.scope.trim().is_empty() {
            return Err(UseCaseError::validation(
                "SCOPE_REQUIRED",
                "scope is required",
            ));
        }
        for j in &cmd.jobs {
            if j.code.trim().is_empty() || j.name.trim().is_empty() || j.crons.is_empty() {
                return Err(UseCaseError::validation(
                    "INVALID_ENTRY",
                    format!(
                        "Sync entry '{}' must have code, name, and at least one cron",
                        j.code
                    ),
                ));
            }
        }
        Ok(())
    }

    async fn authorize(&self, _: &Self::Command, _: &ExecutionContext) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        cmd: Self::Command,
        ctx: ExecutionContext,
    ) -> UseCaseResult<Self::Event> {
        let (to_persist, event) = match self.prepare(&cmd, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        if to_persist.is_empty() {
            // No changes to write; emit a summary event anyway so the audit
            // log records that a sync run occurred (matches event_type sync).
            return self.unit_of_work.emit_event(event, &cmd).await;
        }

        self.unit_of_work
            .commit_all(&to_persist, &*self.repo, event, &cmd)
            .await
    }
}

impl<U: UnitOfWork> SyncScheduledJobsUseCase<U> {
    /// Diff the payload against the stored jobs: the rows to write and the
    /// summary event.
    async fn prepare(
        &self,
        cmd: &SyncScheduledJobsCommand,
        ctx: &ExecutionContext,
    ) -> Result<(Vec<ScheduledJob>, ScheduledJobsSynced), UseCaseError> {
        let existing = match cmd.client_id.as_deref() {
            Some(cid) => self.repo.find_by_client(cid).await,
            None => {
                self.repo
                    .find_with_filters(Some(None), None, None, None, None)
                    .await
            }
        };
        let existing = existing?;

        let mut existing_by_code: std::collections::HashMap<String, ScheduledJob> =
            existing.into_iter().map(|j| (j.code.clone(), j)).collect();

        let mut created: Vec<String> = Vec::new();
        let mut updated: Vec<String> = Vec::new();
        let mut to_persist: Vec<ScheduledJob> = Vec::new();

        for entry in &cmd.jobs {
            match existing_by_code.remove(&entry.code) {
                Some(job) if cmd.protected_ids.contains(&job.id) => {}
                Some(mut job) => {
                    let mut changed = false;
                    if job.name != entry.name {
                        job.name = entry.name.clone();
                        changed = true;
                    }
                    if job.description.as_ref() != entry.description.as_ref() {
                        job.description = entry.description.clone();
                        changed = true;
                    }
                    if job.crons != entry.crons {
                        job.crons = entry.crons.clone();
                        changed = true;
                    }
                    if job.timezone != entry.timezone {
                        job.timezone = entry.timezone.clone();
                        changed = true;
                    }
                    if job.payload.as_ref() != entry.payload.as_ref() {
                        job.payload = entry.payload.clone();
                        changed = true;
                    }
                    if job.concurrent != entry.concurrent {
                        job.concurrent = entry.concurrent;
                        changed = true;
                    }
                    if job.tracks_completion != entry.tracks_completion {
                        job.tracks_completion = entry.tracks_completion;
                        changed = true;
                    }
                    if job.timeout_seconds != entry.timeout_seconds {
                        job.timeout_seconds = entry.timeout_seconds;
                        changed = true;
                    }
                    if job.delivery_max_attempts != entry.delivery_max_attempts {
                        job.delivery_max_attempts = entry.delivery_max_attempts;
                        changed = true;
                    }
                    if job.target_url.as_ref() != entry.target_url.as_ref() {
                        job.target_url = entry.target_url.clone();
                        changed = true;
                    }
                    // Sync re-activates archived/paused jobs that reappear in
                    // the payload — that's the contract.
                    if job.status != ScheduledJobStatus::Active {
                        job.status = ScheduledJobStatus::Active;
                        changed = true;
                    }
                    if changed {
                        job.record_update(Some(ctx.principal_id.clone()));
                        updated.push(job.id.clone());
                        to_persist.push(job);
                    }
                }
                None => {
                    let mut job = ScheduledJob::new(&entry.code, &entry.name, entry.crons.clone())
                        .with_timezone(entry.timezone.clone())
                        .with_concurrent(entry.concurrent)
                        .with_tracks_completion(entry.tracks_completion)
                        .with_delivery_max_attempts(entry.delivery_max_attempts)
                        .with_created_by(ctx.principal_id.clone());
                    if let Some(c) = &cmd.client_id {
                        job = job.with_client_id(c);
                    }
                    if let Some(d) = &entry.description {
                        job = job.with_description(d);
                    }
                    if let Some(p) = &entry.payload {
                        job = job.with_payload(p.clone());
                    }
                    if let Some(t) = entry.timeout_seconds {
                        job = job.with_timeout_seconds(t);
                    }
                    if let Some(u) = &entry.target_url {
                        job = job.with_target_url(u);
                    }
                    created.push(job.id.clone());
                    to_persist.push(job);
                }
            }
        }

        let mut archived: Vec<String> = Vec::new();
        if cmd.archive_unlisted {
            for (_, mut job) in existing_by_code.into_iter() {
                if job.status == ScheduledJobStatus::Active && !cmd.protected_ids.contains(&job.id)
                {
                    job.archive();
                    archived.push(job.id.clone());
                    to_persist.push(job);
                }
            }
        }

        let event = ScheduledJobsSynced::new(
            ctx,
            &cmd.scope,
            cmd.client_id.as_deref(),
            created.clone(),
            updated.clone(),
            archived.clone(),
        );
        Ok((to_persist, event))
    }
}
