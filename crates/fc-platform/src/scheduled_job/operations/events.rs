//! ScheduledJob domain events.
//!
//! Emitted via UnitOfWork on every definition write. Per CLAUDE.md, instance
//! lifecycle (queued → delivered → completed) does NOT emit events — that path
//! is platform infrastructure and would saturate the event log.

use serde::{Deserialize, Serialize};

use crate::impl_domain_event;
use crate::usecase::domain_event::EventMetadata;
use crate::usecase::ExecutionContext;

const SPEC: &str = "1.0";
const SOURCE: &str = "platform:admin";

fn subject_for(id: &str) -> String {
    format!("platform.scheduledjob.{}", id)
}
fn group_for(id: &str) -> String {
    format!("platform:scheduledjob:{}", id)
}

fn meta(ctx: &ExecutionContext, event_type: &str, id: &str) -> EventMetadata {
    EventMetadata::from_ctx(
        ctx,
        event_type,
        SPEC,
        SOURCE,
        subject_for(id),
        group_for(id),
    )
}

// ── Created ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledJobCreated {
    #[serde(flatten)]
    pub metadata: EventMetadata,
    pub scheduled_job_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    pub code: String,
    pub name: String,
    pub crons: Vec<String>,
    pub timezone: String,
    pub concurrent: bool,
    pub tracks_completion: bool,
}
impl_domain_event!(ScheduledJobCreated);

impl ScheduledJobCreated {
    pub const EVENT_TYPE: &'static str = "platform:admin:scheduledjob:created";

    /// Metadata for this event, raised inside `ctx`.
    pub fn metadata_for(ctx: &ExecutionContext, scheduled_job_id: &str) -> EventMetadata {
        meta(ctx, Self::EVENT_TYPE, scheduled_job_id)
    }
}

// ── Updated ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledJobUpdated {
    #[serde(flatten)]
    pub metadata: EventMetadata,
    pub scheduled_job_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    pub code: String,
    /// Field names that changed in this update — useful for audit consumers
    /// who don't want to diff old vs new payloads.
    pub changed_fields: Vec<String>,
    pub version: i32,
}
impl_domain_event!(ScheduledJobUpdated);

impl ScheduledJobUpdated {
    pub const EVENT_TYPE: &'static str = "platform:admin:scheduledjob:updated";

    pub fn new(
        ctx: &ExecutionContext,
        scheduled_job_id: &str,
        client_id: Option<&str>,
        code: &str,
        changed_fields: Vec<String>,
        version: i32,
    ) -> Self {
        Self {
            metadata: meta(ctx, Self::EVENT_TYPE, scheduled_job_id),
            scheduled_job_id: scheduled_job_id.into(),
            client_id: client_id.map(String::from),
            code: code.into(),
            changed_fields,
            version,
        }
    }
}

// ── Status transitions: Paused / Resumed / Archived ────────────────────────

macro_rules! status_event {
    ($name:ident, $event_type:literal) => {
        #[derive(Debug, Clone, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub struct $name {
            #[serde(flatten)]
            pub metadata: EventMetadata,
            pub scheduled_job_id: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            pub client_id: Option<String>,
            pub code: String,
        }
        impl_domain_event!($name);
        impl $name {
            pub const EVENT_TYPE: &'static str = $event_type;
            pub fn new(
                ctx: &ExecutionContext,
                scheduled_job_id: &str,
                client_id: Option<&str>,
                code: &str,
            ) -> Self {
                Self {
                    metadata: meta(ctx, Self::EVENT_TYPE, scheduled_job_id),
                    scheduled_job_id: scheduled_job_id.into(),
                    client_id: client_id.map(String::from),
                    code: code.into(),
                }
            }
        }
    };
}

status_event!(ScheduledJobPaused, "platform:admin:scheduledjob:paused");
status_event!(ScheduledJobResumed, "platform:admin:scheduledjob:resumed");
status_event!(ScheduledJobArchived, "platform:admin:scheduledjob:archived");
status_event!(ScheduledJobDeleted, "platform:admin:scheduledjob:deleted");

// ── Manual fire ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledJobFiredManually {
    #[serde(flatten)]
    pub metadata: EventMetadata,
    pub scheduled_job_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    pub code: String,
    pub instance_id: String,
}
impl_domain_event!(ScheduledJobFiredManually);

impl ScheduledJobFiredManually {
    pub const EVENT_TYPE: &'static str = "platform:admin:scheduledjob:firedManually";

    pub fn new(
        ctx: &ExecutionContext,
        scheduled_job_id: &str,
        client_id: Option<&str>,
        code: &str,
        instance_id: &str,
    ) -> Self {
        Self {
            metadata: meta(ctx, Self::EVENT_TYPE, scheduled_job_id),
            scheduled_job_id: scheduled_job_id.into(),
            client_id: client_id.map(String::from),
            code: code.into(),
            instance_id: instance_id.into(),
        }
    }
}

// ── Sync (one summary event per call, not per row) ─────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledJobsSynced {
    #[serde(flatten)]
    pub metadata: EventMetadata,
    /// Application code or "platform" — scopes the sync run.
    pub scope: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    pub created: Vec<String>,
    pub updated: Vec<String>,
    pub archived: Vec<String>,
}
impl_domain_event!(ScheduledJobsSynced);

impl ScheduledJobsSynced {
    pub const EVENT_TYPE: &'static str = "platform:admin:scheduledjobs:synced";

    pub fn new(
        ctx: &ExecutionContext,
        scope: &str,
        client_id: Option<&str>,
        created: Vec<String>,
        updated: Vec<String>,
        archived: Vec<String>,
    ) -> Self {
        let key = format!("sync:{}:{}", scope, client_id.unwrap_or("platform"));
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                SPEC,
                SOURCE,
                format!("platform.scheduledjobs.synced.{}", key),
                format!("platform:scheduledjobs:synced:{}", key),
            ),
            scope: scope.into(),
            client_id: client_id.map(String::from),
            created,
            updated,
            archived,
        }
    }
}
