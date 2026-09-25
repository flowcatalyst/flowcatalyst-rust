//! ScheduledJob domain events.
//!
//! Emitted via UnitOfWork on every definition write. Per CLAUDE.md, instance
//! lifecycle (queued → delivered → completed) does NOT emit events — that path
//! is platform infrastructure and would saturate the event log.
//!
//! Type, source, subject, message group and `data` are Go's
//! (`internal/platform/scheduledjob/operations/events.go`): type
//! `platform:admin:scheduled-job:*`, source `platform:admin`, subject
//! `platform.scheduledjob.{id}`, group `platform:scheduledjob:{id}`, and
//! each payload is Go's `{scheduledJobId, code}` (plus `instanceId` for a
//! manual fire).

use crate::impl_domain_event;
use crate::usecase::domain_event::{null_if_empty, EventMetadata};
use crate::usecase::ExecutionContext;
use serde::{Deserialize, Serialize};

const SPEC: &str = "1.0";
const SOURCE: &str = "platform:admin";

fn meta(ctx: &ExecutionContext, event_type: &str, id: &str) -> EventMetadata {
    EventMetadata::from_ctx(
        ctx,
        event_type,
        SPEC,
        SOURCE,
        format!("platform.scheduledjob.{}", id),
        format!("platform:scheduledjob:{}", id),
    )
}

macro_rules! job_event {
    ($(#[$doc:meta])* $name:ident, $event_type:literal) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub struct $name {
            #[serde(skip)]
            pub metadata: EventMetadata,
            pub scheduled_job_id: String,
            pub code: String,
        }

        impl_domain_event!($name);

        impl $name {
            pub const EVENT_TYPE: &'static str = $event_type;

            pub fn new(ctx: &ExecutionContext, scheduled_job_id: &str, code: &str) -> Self {
                Self {
                    metadata: meta(ctx, Self::EVENT_TYPE, scheduled_job_id),
                    scheduled_job_id: scheduled_job_id.into(),
                    code: code.into(),
                }
            }
        }
    };
}

job_event!(
    /// `{scheduledJobId, code}`.
    ScheduledJobCreated,
    "platform:admin:scheduled-job:created"
);
job_event!(
    /// `{scheduledJobId, code}`.
    ScheduledJobUpdated,
    "platform:admin:scheduled-job:updated"
);
job_event!(
    /// `{scheduledJobId, code}`.
    ScheduledJobPaused,
    "platform:admin:scheduled-job:paused"
);
job_event!(
    /// `{scheduledJobId, code}`.
    ScheduledJobResumed,
    "platform:admin:scheduled-job:resumed"
);
job_event!(
    /// `{scheduledJobId, code}`.
    ScheduledJobArchived,
    "platform:admin:scheduled-job:archived"
);
job_event!(
    /// `{scheduledJobId, code}`.
    ScheduledJobDeleted,
    "platform:admin:scheduled-job:deleted"
);

/// A human fired a job outside its schedule: `{scheduledJobId, code,
/// instanceId}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledJobFiredManually {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub scheduled_job_id: String,
    pub code: String,
    pub instance_id: String,
}

impl_domain_event!(ScheduledJobFiredManually);

impl ScheduledJobFiredManually {
    pub const EVENT_TYPE: &'static str = "platform:admin:scheduled-job:fired-manually";

    pub fn new(
        ctx: &ExecutionContext,
        scheduled_job_id: &str,
        code: &str,
        instance_id: &str,
    ) -> Self {
        Self {
            metadata: meta(ctx, Self::EVENT_TYPE, scheduled_job_id),
            scheduled_job_id: scheduled_job_id.into(),
            code: code.into(),
            instance_id: instance_id.into(),
        }
    }
}

/// The rollup of a scheduled-job sync: `{applicationCode, created, updated,
/// archived}`, each list the affected job ids (`null` when empty: Go appends
/// to nil slices). Subject `platform.scheduledjobs.synced.{applicationCode}`,
/// group `platform:scheduledjobs:{applicationCode}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledJobsSynced {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub application_code: String,
    #[serde(serialize_with = "null_if_empty")]
    pub created: Vec<String>,
    #[serde(serialize_with = "null_if_empty")]
    pub updated: Vec<String>,
    #[serde(serialize_with = "null_if_empty")]
    pub archived: Vec<String>,
}

impl_domain_event!(ScheduledJobsSynced);

impl ScheduledJobsSynced {
    pub const EVENT_TYPE: &'static str = "platform:admin:scheduledjobs:synced";

    pub fn new(
        ctx: &ExecutionContext,
        application_code: &str,
        created: Vec<String>,
        updated: Vec<String>,
        archived: Vec<String>,
    ) -> Self {
        let group = if application_code.is_empty() {
            "platform:scheduledjobs:synced".to_string()
        } else {
            format!("platform:scheduledjobs:{}", application_code)
        };
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                SPEC,
                SOURCE,
                format!("platform.scheduledjobs.synced.{}", application_code),
                group,
            ),
            application_code: application_code.into(),
            created,
            updated,
            archived,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheduled_job_events_are_go_shaped() {
        let ctx = ExecutionContext::create("prn_1");
        let e = ScheduledJobFiredManually::new(&ctx, "sjb_1", "nightly", "sji_1");
        assert_eq!(
            e.metadata.event_type,
            "platform:admin:scheduled-job:fired-manually"
        );
        assert_eq!(e.metadata.subject, "platform.scheduledjob.sjb_1");
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            serde_json::json!({"scheduledJobId": "sjb_1", "code": "nightly", "instanceId": "sji_1"})
        );

        let s = ScheduledJobsSynced::new(&ctx, "orders", vec!["sjb_1".into()], vec![], vec![]);
        assert_eq!(s.metadata.subject, "platform.scheduledjobs.synced.orders");
        assert_eq!(s.metadata.message_group, "platform:scheduledjobs:orders");
        assert_eq!(
            serde_json::to_value(&s).unwrap(),
            serde_json::json!({
                "applicationCode": "orders",
                "created": ["sjb_1"],
                "updated": null,
                "archived": null
            })
        );
    }
}
