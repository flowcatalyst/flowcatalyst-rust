//! The dispatch-job lifecycle: the one owner of `msg_dispatch_jobs.status`.
//!
//! The statements live in [`fc_common::dispatch_lifecycle`] (the lowest crate
//! both this crate and `fc-stream`'s fan-out can see); this module re-exports
//! it and adapts the entity to its insert row. Nothing outside the lifecycle
//! file writes the table: `tests/lifecycle_enforcement.rs` fails if it does.

pub use fc_common::dispatch_lifecycle::*;

use crate::dispatch_job::entity::DispatchJob;

/// The insert row for `job`, columns as the entity carries them.
pub fn new_job(job: &DispatchJob) -> NewJob {
    NewJob {
        id: job.id.clone(),
        external_id: job.external_id.clone(),
        source: job.source.clone(),
        kind: job.kind.as_str().to_string(),
        code: job.code.clone(),
        subject: job.subject.clone(),
        event_id: job.event_id.clone(),
        correlation_id: job.correlation_id.clone(),
        metadata: serde_json::to_value(&job.metadata).unwrap_or_default(),
        target_url: job.target_url.clone(),
        protocol: job.protocol.as_str().to_string(),
        payload: job.payload.clone(),
        payload_content_type: job.payload_content_type.clone(),
        data_only: job.data_only,
        service_account_id: job.service_account_id.clone(),
        client_id: job.client_id.clone(),
        subscription_id: job.subscription_id.clone(),
        mode: job.mode.as_str().to_string(),
        dispatch_pool_id: job.dispatch_pool_id.clone(),
        message_group: job.message_group.clone(),
        sequence: job.sequence,
        timeout_seconds: job.timeout_seconds as i32,
        schema_id: job.schema_id.clone(),
        status: job.status.as_str().to_string(),
        max_retries: job.max_retries as i32,
        retry_strategy: job.retry_strategy.as_str().to_string(),
        scheduled_for: job.scheduled_for,
        expires_at: job.expires_at,
        attempt_count: job.attempt_count as i32,
        last_attempt_at: job.last_attempt_at,
        completed_at: job.completed_at,
        duration_millis: job.duration_millis,
        last_error: job.last_error.clone(),
        idempotency_key: job.idempotency_key.clone(),
        created_at: job.created_at,
        updated_at: job.updated_at,
        descriptor: job.descriptor.clone(),
        queue: job.queue.clone(),
    }
}
