//! Update ScheduledJob use case.
//!
//! All fields are optional in the patch — only Some(...) values are applied.
//! `code` and `client_id` are intentionally NOT updatable: changing the routing
//! key would break consumer SDK handlers, and re-scoping a job to a different
//! tenant changes its security boundary. To rename, archive + recreate.

use fc_platform_core::shared::id::ScheduledJobId;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::events::ScheduledJobUpdated;
use crate::scheduled_job::ScheduledJobRepository;
use fc_platform_core::shared::caller_reach;
use fc_platform_core::shared::id::OptionIdExt;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateScheduledJobCommand {
    pub scheduled_job_id: ScheduledJobId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crons: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concurrent: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tracks_completion: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery_max_attempts: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_url: Option<String>,
}

impl AuditMasked for UpdateScheduledJobCommand {}

pub struct UpdateScheduledJobUseCase<U: UnitOfWork> {
    repo: Arc<ScheduledJobRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> UpdateScheduledJobUseCase<U> {
    pub fn new(repo: Arc<ScheduledJobRepository>, unit_of_work: Arc<U>) -> Self {
        Self { repo, unit_of_work }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for UpdateScheduledJobUseCase<U> {
    type Command = UpdateScheduledJobCommand;
    type Event = ScheduledJobUpdated;

    async fn validate(&self, cmd: &Self::Command) -> Result<(), UseCaseError> {
        if cmd.scheduled_job_id.as_str().trim().is_empty() {
            return Err(UseCaseError::validation("ID_REQUIRED", "ID is required"));
        }
        // Go `UpdateScheduledJob.Validate` (scheduledjob/operations/ops.go).
        if cmd.name.as_deref().is_some_and(|n| n.trim().is_empty()) {
            return Err(UseCaseError::validation(
                "NAME_REQUIRED",
                "name cannot be empty",
            ));
        }
        if let Some(crons) = &cmd.crons {
            if crons.is_empty() {
                return Err(UseCaseError::validation(
                    "CRONS_REQUIRED",
                    "at least one cron expression is required",
                ));
            }
            for c in crons {
                super::create::validate_cron_shape(c)?;
            }
        }
        super::create::validate_target_url(cmd.target_url.as_deref())?;
        if let Some(d) = cmd.delivery_max_attempts {
            if !(1..=20).contains(&d) {
                return Err(UseCaseError::validation(
                    "INVALID_DELIVERY_ATTEMPTS",
                    "delivery_max_attempts must be between 1 and 20",
                ));
            }
        }
        Ok(())
    }

    /// Go `CheckScopeAccess` on the stored job (Go checks it post-load): a
    /// client's job needs that client, a platform one anchor scope (403
    /// `SCOPE_FORBIDDEN`). A missing job is `execute`'s 404.
    async fn authorize(
        &self,
        command: &UpdateScheduledJobCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        if let Some(job) = self.repo.find_by_id(&command.scheduled_job_id).await? {
            caller_reach::check_scope_access(ctx.caller(), job.client_id.as_id_str())?;
        }
        Ok(())
    }

    async fn execute(
        &self,
        cmd: Self::Command,
        ctx: ExecutionContext,
    ) -> Result<Committed<Self::Event>, UseCaseError> {
        let mut job = self
            .repo
            .find_by_id(&cmd.scheduled_job_id)
            .await
            .or_not_found(
                "SCHEDULED_JOB_NOT_FOUND",
                format!("ScheduledJob '{}' not found", cmd.scheduled_job_id),
            )?;

        let mut changed: Vec<String> = Vec::new();
        if let Some(v) = &cmd.name {
            let v = v.trim();
            if v != job.name {
                job.name = v.to_string();
                changed.push("name".into());
            }
        }
        if let Some(v) = &cmd.description {
            if Some(v) != job.description.as_ref() {
                job.description = Some(v.clone());
                changed.push("description".into());
            }
        }
        if let Some(v) = &cmd.crons {
            if v != &job.crons {
                job.crons = v.clone();
                changed.push("crons".into());
            }
        }
        if let Some(v) = &cmd.timezone {
            if v != &job.timezone {
                job.timezone = v.clone();
                changed.push("timezone".into());
            }
        }
        if let Some(v) = &cmd.payload {
            if Some(v) != job.payload.as_ref() {
                job.payload = Some(v.clone());
                changed.push("payload".into());
            }
        }
        if let Some(v) = cmd.concurrent {
            if v != job.concurrent {
                job.concurrent = v;
                changed.push("concurrent".into());
            }
        }
        if let Some(v) = cmd.tracks_completion {
            if v != job.tracks_completion {
                job.tracks_completion = v;
                changed.push("tracksCompletion".into());
            }
        }
        if let Some(v) = cmd.timeout_seconds {
            if Some(v) != job.timeout_seconds {
                job.timeout_seconds = Some(v);
                changed.push("timeoutSeconds".into());
            }
        }
        if let Some(v) = cmd.delivery_max_attempts {
            if v != job.delivery_max_attempts {
                job.delivery_max_attempts = v;
                changed.push("deliveryMaxAttempts".into());
            }
        }
        if let Some(v) = &cmd.target_url {
            if Some(v) != job.target_url.as_ref() {
                job.target_url = Some(v.clone());
                changed.push("targetUrl".into());
            }
        }

        if changed.is_empty() {
            return Err(UseCaseError::business_rule(
                "NO_CHANGES",
                "Update command did not change any fields",
            ));
        }

        job.record_update(Some(ctx.principal_id.clone()));

        let event = ScheduledJobUpdated::new(&ctx, &job.id, &job.code);

        self.unit_of_work
            .commit(&job, &*self.repo, event, &cmd)
            .await
    }
}
