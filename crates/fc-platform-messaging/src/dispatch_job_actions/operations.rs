//! Requeue, cancel and complete (Go `dispatchjob/operations/{resend,
//! cancel,complete,shared}.go`), with Go's events.

use async_trait::async_trait;
use fc_platform_core::shared::id::ClientId;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::repository::{DispatchJobActionsRepository, JobStatusFlip, JobsRequeue};
use fc_platform_core::impl_domain_event;
use fc_platform_core::permissions;
use fc_platform_core::shared::authorization_service::Authority;
use fc_platform_core::usecase::domain_event::EventMetadata;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{Committed, ExecutionContext, UnitOfWork, UseCase, UseCaseError};
use std::slice;

const SOURCE: &str = "platform:messaging";

/// Go `DispatchJobsResent`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchJobsResent {
    #[serde(flatten)]
    pub metadata: EventMetadata,
    pub ids: Vec<String>,
}

impl_domain_event!(DispatchJobsResent);

impl DispatchJobsResent {
    pub const EVENT_TYPE: &'static str = "platform:messaging:dispatch-jobs:resent";
}

/// Go `DispatchJobCancelled` / `DispatchJobCompleted`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchJobSettled {
    #[serde(flatten)]
    pub metadata: EventMetadata,
    pub dispatch_job_id: String,
}

impl_domain_event!(DispatchJobSettled);

pub const CANCELLED_EVENT: &str = "platform:messaging:dispatch-job:cancelled";
pub const COMPLETED_EVENT: &str = "platform:messaging:dispatch-job:completed";

/// Go `CanAccessScope`: a client's job needs that client; a client-less
/// one anchor or super-admin.
pub fn reaches(caller: &impl Authority, client_id: Option<&str>) -> bool {
    match client_id {
        Some(c) => caller.can_access_client(&ClientId::from_wire(c)),
        None => caller.is_anchor() || caller.has_permission(permissions::ADMIN_ALL),
    }
}

/// Go `CheckScopeAccess`'s refusal: 403 `SCOPE_FORBIDDEN`.
pub fn scope_forbidden(client_id: Option<&str>) -> UseCaseError {
    UseCaseError::forbidden(
        "SCOPE_FORBIDDEN",
        if client_id.is_some() {
            "no access to this resource's client"
        } else {
            "anchor scope required for this resource"
        },
    )
}

/// Go `ResendCommand`: the jobs the caller may reach, already resolved.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResendCommand {
    pub ids: Vec<String>,
    #[serde(skip)]
    pub jobs: Vec<(String, chrono::DateTime<chrono::Utc>)>,
}

impl AuditMasked for ResendCommand {}

pub struct RequeueDispatchJobsUseCase<U: UnitOfWork> {
    repo: Arc<DispatchJobActionsRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> RequeueDispatchJobsUseCase<U> {
    pub fn new(repo: Arc<DispatchJobActionsRepository>, unit_of_work: Arc<U>) -> Self {
        Self { repo, unit_of_work }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for RequeueDispatchJobsUseCase<U> {
    type Command = ResendCommand;
    type Event = DispatchJobsResent;

    async fn validate(&self, _c: &ResendCommand) -> Result<(), UseCaseError> {
        Ok(())
    }

    /// Every job must be one the caller reaches (Go `CanAccessScope`). The
    /// handler drops unreachable and unknown ids silently before building the
    /// command (Go's bulk contract, and the ids the audit row records), so this
    /// never refuses an HTTP request; it holds the rule for any other caller.
    async fn authorize(
        &self,
        command: &ResendCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        for head in self.repo.heads(&command.ids).await? {
            if !reaches(ctx.caller(), head.client_id.as_deref()) {
                return Err(scope_forbidden(head.client_id.as_deref()));
            }
        }
        Ok(())
    }

    async fn execute(
        &self,
        command: ResendCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<DispatchJobsResent>, UseCaseError> {
        let event = DispatchJobsResent {
            metadata: EventMetadata::from_ctx(
                &ctx,
                DispatchJobsResent::EVENT_TYPE,
                "1.0",
                SOURCE,
                "platform.dispatchjobs.resent".to_string(),
                "platform:dispatchjobs:resent".to_string(),
            ),
            ids: command.ids.clone(),
        };
        let requeue = JobsRequeue {
            jobs: command.jobs.clone(),
        };
        self.unit_of_work
            .commit(&requeue, &*self.repo, event, &command)
            .await
    }
}

/// Go `StatusFlipCommand`: settle one FAILED job by hand.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusFlipCommand {
    pub id: String,
    #[serde(skip)]
    pub target: &'static str,
    #[serde(skip)]
    pub current_status: String,
    #[serde(skip)]
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl AuditMasked for StatusFlipCommand {}

pub struct SettleDispatchJobUseCase<U: UnitOfWork> {
    repo: Arc<DispatchJobActionsRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> SettleDispatchJobUseCase<U> {
    pub fn new(repo: Arc<DispatchJobActionsRepository>, unit_of_work: Arc<U>) -> Self {
        Self { repo, unit_of_work }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for SettleDispatchJobUseCase<U> {
    type Command = StatusFlipCommand;
    type Event = DispatchJobSettled;

    async fn validate(&self, c: &StatusFlipCommand) -> Result<(), UseCaseError> {
        if c.id.trim().is_empty() {
            return Err(UseCaseError::validation("ID_REQUIRED", "id is required"));
        }
        Ok(())
    }

    /// Go `CheckScopeAccess` on the stored job (post-load): a client's job needs
    /// that client, a client-less one anchor scope or super-admin (403
    /// `SCOPE_FORBIDDEN`). The handler loads the job first (404) to build the
    /// command.
    async fn authorize(
        &self,
        command: &StatusFlipCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        if let Some(head) = self
            .repo
            .heads(slice::from_ref(&command.id))
            .await?
            .into_iter()
            .next()
        {
            if !reaches(ctx.caller(), head.client_id.as_deref()) {
                return Err(scope_forbidden(head.client_id.as_deref()));
            }
        }
        Ok(())
    }

    async fn execute(
        &self,
        command: StatusFlipCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<DispatchJobSettled>, UseCaseError> {
        if command.current_status != "FAILED" {
            return Err(UseCaseError::business_rule(
                "NOT_FAILED",
                format!(
                    "dispatch job is not FAILED (current status: {}); only a FAILED job can be overridden",
                    command.current_status
                ),
            ));
        }
        let event_type = if command.target == "CANCELLED" {
            CANCELLED_EVENT
        } else {
            COMPLETED_EVENT
        };
        let event = DispatchJobSettled {
            metadata: EventMetadata::from_ctx(
                &ctx,
                event_type,
                "1.0",
                SOURCE,
                format!("platform.dispatchjob.{}", command.id),
                format!("platform:dispatchjob:{}", command.id),
            ),
            dispatch_job_id: command.id.clone(),
        };
        let flip = JobStatusFlip {
            id: command.id.clone(),
            created_at: command.created_at,
            status: command.target,
        };
        self.unit_of_work
            .commit(&flip, &*self.repo, event, &command)
            .await
    }
}
