//! Requeue, cancel and complete (Go `dispatchjob/operations/{resend,
//! cancel,complete,shared}.go`), with Go's events.

use async_trait::async_trait;
use fc_platform_core::shared::id::ClientId;
use serde::de::IgnoredAny;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::repository::{DispatchJobActionsRepository, JobPayloadEdit, JobStatusFlip, JobsRequeue};
use crate::dispatch_job::entity::DispatchStatus;
use fc_common::dispatch_lifecycle::EDITABLE_STATUSES;
use fc_platform_core::impl_domain_event;
use fc_platform_core::permissions;
use fc_platform_core::shared::authorization_service::Authority;
use fc_platform_core::shared::error::PlatformError;
use fc_platform_core::usecase::domain_event::EventMetadata;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{Committed, ExecutionContext, UnitOfWork, UseCase, UseCaseError};
use std::slice;

const SOURCE: &str = "platform:messaging";

/// Go `DispatchJobsResent`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchJobsResent {
    #[serde(skip)]
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
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub dispatch_job_id: String,
}

impl_domain_event!(DispatchJobSettled);

pub const CANCELLED_EVENT: &str = "platform:messaging:dispatch-job:cancelled";
pub const COMPLETED_EVENT: &str = "platform:messaging:dispatch-job:completed";
pub const PAYLOAD_EDITED_EVENT: &str = "platform:messaging:dispatch-job:payload-edited";

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
    /// [`DispatchStatus::Cancelled`] or [`DispatchStatus::Completed`].
    #[serde(skip)]
    pub target: DispatchStatus,
    /// The stored spelling, as read: a legacy `ERROR` row is not `FAILED`
    /// here (it is refused), so it is not parsed into the enum, which reads
    /// `ERROR` as `Failed`.
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
        if command.current_status != DispatchStatus::Failed.as_str() {
            return Err(UseCaseError::business_rule(
                "NOT_FAILED",
                format!(
                    "dispatch job is not FAILED (current status: {}); only a FAILED job can be overridden",
                    command.current_status
                ),
            ));
        }
        let event_type = if command.target == DispatchStatus::Cancelled {
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

// ── Edit payload ─────────────────────────────────────────────────────────

/// The most an edited payload may hold: the same cap a function's
/// `fn.dispatch` applies to the jobs it creates (Go `MaxEditedPayloadBytes`).
pub const MAX_EDITED_PAYLOAD_BYTES: usize = 256 << 10;

/// Go `EditPayloadCommand`: rewrite one job's payload. `payload_content_type`
/// `None` keeps the job's content type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditPayloadCommand {
    pub id: String,
    pub payload: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_content_type: Option<String>,
}

/// The payload text stays out of `aud_logs.operation_json`, where the command
/// is recorded: the audit row holds who, when and the sizes and digests (see
/// [`DispatchJobPayloadEdited`]), not data that may be personal.
impl AuditMasked for EditPayloadCommand {
    fn audit_masked_fields(&self) -> &'static [&'static str] {
        &["payload"]
    }
}

impl EditPayloadCommand {
    /// Go `EditDispatchJobPayload`'s `Validate`, in Go's order. (A payload
    /// that is not UTF-8 cannot reach here: it is a `String`, and the JSON
    /// body that carried it would not have decoded, a 400 before this.)
    pub fn validate_shape(&self) -> Result<(), UseCaseError> {
        if self.id.trim().is_empty() {
            return Err(UseCaseError::validation("ID_REQUIRED", "id is required"));
        }
        if self.payload.is_empty() {
            return Err(UseCaseError::validation(
                "PAYLOAD_REQUIRED",
                "payload is required",
            ));
        }
        if self.payload.len() > MAX_EDITED_PAYLOAD_BYTES {
            return Err(UseCaseError::validation(
                "PAYLOAD_TOO_LARGE",
                "payload exceeds the 256 KiB limit",
            ));
        }
        if let Some(ct) = &self.payload_content_type {
            let ct = ct.trim();
            if ct.is_empty() || ct.len() > 100 || !ct.contains('/') {
                return Err(UseCaseError::validation(
                    "CONTENT_TYPE_INVALID",
                    "payloadContentType must be a media type such as application/json",
                ));
            }
        }
        Ok(())
    }
}

/// Go `DispatchJobPayloadEdited`: the event (and so the audit row) carries
/// sizes and SHA-256 digests of the old and new payload, never the payload
/// text.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchJobPayloadEdited {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub dispatch_job_id: String,
    /// The stored spelling of the status the job was in (`ERROR` stays `ERROR`).
    pub status: String,
    pub previous_size: usize,
    pub previous_sha256: String,
    pub size: usize,
    pub sha256: String,
    pub previous_content_type: String,
    pub content_type: String,
}

impl_domain_event!(DispatchJobPayloadEdited);

/// Whether a media type is JSON (`application/json` or a `+json` suffix
/// type), ignoring parameters and case (Go `isJSONType`).
pub fn is_json_type(content_type: &str) -> bool {
    let lower = content_type.to_lowercase();
    let media = lower.split(';').next().unwrap_or("").trim();
    media == "application/json" || media.ends_with("+json")
}

/// Hex SHA-256 of `text` (Go `digest`).
pub fn digest(text: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Go `EditDispatchJobPayload`: lets an operator correct a bad payload on a
/// job that has not been delivered (PENDING) or has failed (FAILED, legacy
/// ERROR), before retrying it. Any other status is a 409 `NOT_EDITABLE`: a
/// QUEUED or PROCESSING job is being delivered, and a settled COMPLETED,
/// CANCELLED or EXPIRED job is history. It changes the payload (and optionally
/// the content type) only: not the status, attempts or schedule; the operator
/// retries with the ordinary requeue.
pub struct EditDispatchJobPayloadUseCase<U: UnitOfWork> {
    repo: Arc<DispatchJobActionsRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> EditDispatchJobPayloadUseCase<U> {
    pub fn new(repo: Arc<DispatchJobActionsRepository>, unit_of_work: Arc<U>) -> Self {
        Self { repo, unit_of_work }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for EditDispatchJobPayloadUseCase<U> {
    type Command = EditPayloadCommand;
    type Event = DispatchJobPayloadEdited;

    async fn validate(&self, c: &EditPayloadCommand) -> Result<(), UseCaseError> {
        c.validate_shape()
    }

    /// Go `CheckScopeAccess` on the stored job (post-load): a client's job
    /// needs that client, a client-less one anchor scope or super-admin (403
    /// `SCOPE_FORBIDDEN`). A missing job passes here; `execute` answers 404.
    async fn authorize(
        &self,
        command: &EditPayloadCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        if let Some(job) = self.repo.edit_view(&command.id).await? {
            if !reaches(ctx.caller(), job.client_id.as_deref()) {
                return Err(scope_forbidden(job.client_id.as_deref()));
            }
        }
        Ok(())
    }

    async fn execute(
        &self,
        command: EditPayloadCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<DispatchJobPayloadEdited>, UseCaseError> {
        let job = self.repo.edit_view(&command.id).await?.ok_or_else(|| {
            UseCaseError::verbatim(PlatformError::not_found_code("DispatchJob", &command.id))
        })?;
        if !EDITABLE_STATUSES.contains(&job.status.as_str()) {
            return Err(UseCaseError::business_rule(
                "NOT_EDITABLE",
                format!(
                    "dispatch job is {}; a payload can be edited only while the job is PENDING or FAILED",
                    job.status
                ),
            ));
        }
        let content_type = command
            .payload_content_type
            .as_deref()
            .map(|c| c.trim().to_string())
            .unwrap_or_else(|| job.payload_content_type.clone());
        if is_json_type(&content_type)
            && serde_json::from_str::<IgnoredAny>(&command.payload).is_err()
        {
            return Err(UseCaseError::validation(
                "PAYLOAD_NOT_JSON",
                format!("the payload is not valid JSON for content type {content_type}"),
            ));
        }
        let previous = job.payload.clone().unwrap_or_default();
        let event = DispatchJobPayloadEdited {
            metadata: EventMetadata::from_ctx(
                &ctx,
                PAYLOAD_EDITED_EVENT,
                "1.0",
                SOURCE,
                format!("platform.dispatchjob.{}", job.id),
                format!("platform:dispatchjob:{}", job.id),
            ),
            dispatch_job_id: job.id.clone(),
            status: job.status.clone(),
            previous_size: previous.len(),
            previous_sha256: digest(&previous),
            size: command.payload.len(),
            sha256: digest(&command.payload),
            previous_content_type: job.payload_content_type.clone(),
            content_type: content_type.clone(),
        };
        let edit = JobPayloadEdit {
            id: job.id.clone(),
            created_at: job.created_at,
            payload: command.payload.clone(),
            // Go writes the content type back as resolved, trimmed.
            content_type: Some(content_type),
        };
        self.unit_of_work
            .commit(&edit, &*self.repo, event, &command)
            .await
    }
}
