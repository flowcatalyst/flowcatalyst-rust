//! Create ScheduledJob use case.

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::events::ScheduledJobCreated;
use crate::scheduled_job::entity::ScheduledJob;
use crate::scheduled_job::ScheduledJobRepository;
use crate::usecase::{ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateScheduledJobCommand {
    pub code: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// None = platform-scoped (anchor only); Some = client-scoped.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
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
    #[serde(default = "default_delivery_attempts")]
    pub delivery_max_attempts: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_url: Option<String>,
}

impl crate::usecase::AuditMasked for CreateScheduledJobCommand {}

fn default_timezone() -> String {
    "UTC".into()
}
fn default_delivery_attempts() -> i32 {
    3
}

pub struct CreateScheduledJobUseCase<U: UnitOfWork> {
    repo: Arc<ScheduledJobRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> CreateScheduledJobUseCase<U> {
    pub fn new(repo: Arc<ScheduledJobRepository>, unit_of_work: Arc<U>) -> Self {
        Self { repo, unit_of_work }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for CreateScheduledJobUseCase<U> {
    type Command = CreateScheduledJobCommand;
    type Event = ScheduledJobCreated;

    async fn validate(&self, cmd: &Self::Command) -> Result<(), UseCaseError> {
        if cmd.code.trim().is_empty() {
            return Err(UseCaseError::validation(
                "CODE_REQUIRED",
                "Code is required",
            ));
        }
        if cmd.name.trim().is_empty() {
            return Err(UseCaseError::validation(
                "NAME_REQUIRED",
                "Name is required",
            ));
        }
        if cmd.crons.is_empty() {
            return Err(UseCaseError::validation(
                "CRONS_REQUIRED",
                "At least one cron expression is required",
            ));
        }
        for c in &cmd.crons {
            validate_cron_shape(c)?;
        }
        if cmd.delivery_max_attempts < 1 || cmd.delivery_max_attempts > 20 {
            return Err(UseCaseError::validation(
                "INVALID_DELIVERY_ATTEMPTS",
                "delivery_max_attempts must be between 1 and 20",
            ));
        }
        if cmd.timezone.trim().is_empty() {
            return Err(UseCaseError::validation("TZ_REQUIRED", "Timezone required"));
        }
        Ok(())
    }

    async fn authorize(
        &self,
        _cmd: &Self::Command,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        // Resource-level authorization (anchor for platform-scoped, client
        // membership for client-scoped) is enforced at the handler layer
        // before run() is called.
        Ok(())
    }

    async fn execute(
        &self,
        cmd: Self::Command,
        ctx: ExecutionContext,
    ) -> UseCaseResult<Self::Event> {
        let existing = match self
            .repo
            .find_by_code(cmd.client_id.as_deref(), &cmd.code)
            .await
        {
            Ok(found) => found,
            Err(e) => return UseCaseResult::failure(e.into()),
        };
        if existing.is_some() {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "CODE_EXISTS",
                format!("ScheduledJob '{}' already exists in this scope", cmd.code),
            ));
        }

        let mut job = ScheduledJob::new(&cmd.code, &cmd.name, cmd.crons.clone())
            .with_timezone(cmd.timezone.clone())
            .with_concurrent(cmd.concurrent)
            .with_tracks_completion(cmd.tracks_completion)
            .with_delivery_max_attempts(cmd.delivery_max_attempts)
            .with_created_by(ctx.principal_id.clone());

        if let Some(c) = &cmd.client_id {
            job = job.with_client_id(c);
        }
        if let Some(d) = &cmd.description {
            job = job.with_description(d);
        }
        if let Some(p) = &cmd.payload {
            job = job.with_payload(p.clone());
        }
        if let Some(t) = cmd.timeout_seconds {
            job = job.with_timeout_seconds(t);
        }
        if let Some(u) = &cmd.target_url {
            job = job.with_target_url(u);
        }

        let event = ScheduledJobCreated {
            metadata: ScheduledJobCreated::metadata_for(&ctx, &job.id),
            scheduled_job_id: job.id.clone(),
            client_id: job.client_id.clone(),
            code: job.code.clone(),
            name: job.name.clone(),
            crons: job.crons.clone(),
            timezone: job.timezone.clone(),
            concurrent: job.concurrent,
            tracks_completion: job.tracks_completion,
        };

        self.unit_of_work
            .commit(&job, &*self.repo, event, &cmd)
            .await
    }
}

/// Lightweight cron shape check. Full cron parsing happens in the poller;
/// we just want to fail obviously broken input early at the API boundary.
fn validate_cron_shape(expr: &str) -> Result<(), UseCaseError> {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        return Err(UseCaseError::validation(
            "CRON_EMPTY",
            "Cron expression cannot be empty",
        ));
    }
    let fields = trimmed.split_whitespace().count();
    if !(5..=7).contains(&fields) {
        return Err(UseCaseError::validation(
            "CRON_INVALID_SHAPE",
            format!(
                "Cron expression must have 5-7 whitespace-separated fields, got {}: '{}'",
                fields, trimmed
            ),
        ));
    }
    Ok(())
}
