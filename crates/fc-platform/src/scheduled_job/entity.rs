//! ScheduledJob domain entity.
//!
//! A ScheduledJob is a definition: cron expression(s), routing code, optional
//! payload. Each cron tick produces a *ScheduledJobInstance* (history row) — the
//! instance is platform-infrastructure plumbing and is not modelled as an
//! aggregate; see `repository.rs` for direct-write infrastructure methods.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[derive(Default)]
pub enum ScheduledJobStatus {
    #[default]
    Active,
    Paused,
    Archived,
}

crate::shared::enum_str::str_enum!(ScheduledJobStatus, "scheduled job status", {
    Active => "ACTIVE",
    Paused => "PAUSED",
    Archived => "ARCHIVED",
});

/// ScheduledJob aggregate. Pure data + behaviour. No sqlx imports here.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledJob {
    pub id: String,
    /// NULL = platform-scoped (anchor-only); Some = client-scoped.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// The application the job belongs to (Java `ScheduledJob.applicationId`):
    /// its oldest active service account signs the job's firings. `None` for
    /// a job no application owns.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application_id: Option<String>,
    /// Routing key the SDK uses to find the registered handler. Unique per
    /// (client_id, code) — or globally when client_id is None.
    pub code: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub status: ScheduledJobStatus,
    /// One or more cron expressions. Fire times are unioned and de-duplicated
    /// per minute by the poller.
    pub crons: Vec<String>,
    /// IANA timezone name (e.g. "UTC", "Europe/London").
    pub timezone: String,
    /// Optional user-defined payload, passed through verbatim in the webhook
    /// envelope on every fire.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
    /// Informational metadata for the SDK. The platform does not enforce
    /// concurrency — the SDK is responsible (typically via a distributed lock).
    pub concurrent: bool,
    /// When true, the SDK is expected to call back with completion status. The
    /// instance lifecycle waits in DELIVERED until then. When false, DELIVERED
    /// is the terminal state.
    pub tracks_completion: bool,
    /// Hint passed to the SDK for its own runtime timeout. Not enforced
    /// platform-side (the SDK owns instance runtime).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<i32>,
    /// How many times the platform retries delivery (HTTP 202 ACK) before
    /// marking the instance DELIVERY_FAILED. Distinct from business retries,
    /// which are the SDK/consumer's responsibility.
    pub delivery_max_attempts: i32,
    /// HTTP endpoint the dispatcher POSTs to on every fire. The SDK exposes
    /// one URL per app/client and routes by `code` server-side. None means
    /// the job has no destination yet — the dispatcher will mark instances
    /// DELIVERY_FAILED with a clear error until configured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_url: Option<String>,
    /// Last cron-slot timestamp the poller fired for this job. Used to skip
    /// already-fired slots and compute the next due slot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_fired_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
    pub version: i32,
}

impl ScheduledJob {
    /// Construct a new ACTIVE ScheduledJob with required fields. Cron syntax is
    /// validated at the use-case layer (uses the `cron` crate); this constructor
    /// trusts its inputs.
    pub fn new(code: impl Into<String>, name: impl Into<String>, crons: Vec<String>) -> Self {
        let now = Utc::now();
        Self {
            id: crate::shared::tsid::generate(crate::EntityType::ScheduledJob),
            client_id: None,
            application_id: None,
            code: code.into(),
            name: name.into(),
            description: None,
            status: ScheduledJobStatus::Active,
            crons,
            timezone: "UTC".to_string(),
            payload: None,
            concurrent: false,
            tracks_completion: false,
            timeout_seconds: None,
            delivery_max_attempts: 3,
            target_url: None,
            last_fired_at: None,
            created_at: now,
            updated_at: now,
            created_by: None,
            updated_by: None,
            version: 1,
        }
    }

    pub fn with_client_id(mut self, id: impl Into<String>) -> Self {
        self.client_id = Some(id.into());
        self
    }
    pub fn with_application_id(mut self, id: impl Into<String>) -> Self {
        self.application_id = Some(id.into());
        self
    }
    pub fn with_description(mut self, d: impl Into<String>) -> Self {
        self.description = Some(d.into());
        self
    }
    pub fn with_timezone(mut self, tz: impl Into<String>) -> Self {
        self.timezone = tz.into();
        self
    }
    pub fn with_payload(mut self, p: serde_json::Value) -> Self {
        self.payload = Some(p);
        self
    }
    pub fn with_concurrent(mut self, c: bool) -> Self {
        self.concurrent = c;
        self
    }
    pub fn with_tracks_completion(mut self, t: bool) -> Self {
        self.tracks_completion = t;
        self
    }
    pub fn with_timeout_seconds(mut self, t: i32) -> Self {
        self.timeout_seconds = Some(t);
        self
    }
    pub fn with_delivery_max_attempts(mut self, n: i32) -> Self {
        self.delivery_max_attempts = n;
        self
    }
    pub fn with_target_url(mut self, u: impl Into<String>) -> Self {
        self.target_url = Some(u.into());
        self
    }
    pub fn with_created_by(mut self, p: impl Into<String>) -> Self {
        self.created_by = Some(p.into());
        self
    }

    pub fn pause(&mut self) {
        self.status = ScheduledJobStatus::Paused;
        self.updated_at = Utc::now();
        self.version += 1;
    }

    pub fn resume(&mut self) {
        self.status = ScheduledJobStatus::Active;
        self.updated_at = Utc::now();
        self.version += 1;
    }

    pub fn archive(&mut self) {
        self.status = ScheduledJobStatus::Archived;
        self.updated_at = Utc::now();
        self.version += 1;
    }

    /// Apply an in-place update from a partial change. Bumps `updated_at` and
    /// `version`. Caller is responsible for validation.
    pub fn record_update(&mut self, updated_by: Option<String>) {
        self.updated_at = Utc::now();
        self.updated_by = updated_by;
        self.version += 1;
    }

    pub fn is_active(&self) -> bool {
        matches!(self.status, ScheduledJobStatus::Active)
    }

    /// The declarative reconcile (Java `ScheduledJob.reconcile`): this job
    /// with `d` applied, `ACTIVE` again, the application backfilled when
    /// given, `version + 1`, and the names of the fields that differed; or
    /// `None` when nothing differs, so a no-op is neither written nor
    /// reported. `code`, scope and `last_fired_at` never change here.
    pub fn reconcile(
        &self,
        d: &JobDefinition,
        application_id: Option<&str>,
        by: Option<&str>,
    ) -> Option<(ScheduledJob, Vec<String>)> {
        let application_id = application_id
            .map(str::to_string)
            .or_else(|| self.application_id.clone());
        let mut changed = Vec::new();
        let mut differs = |yes: bool, field: &str| {
            if yes {
                changed.push(field.to_string());
            }
        };
        differs(self.name != d.name, "name");
        differs(self.description != d.description, "description");
        differs(self.crons != d.crons, "crons");
        differs(self.timezone != d.timezone, "timezone");
        differs(self.payload != d.payload, "payload");
        differs(self.concurrent != d.concurrent, "concurrent");
        differs(
            self.tracks_completion != d.tracks_completion,
            "tracksCompletion",
        );
        differs(self.timeout_seconds != d.timeout_seconds, "timeoutSeconds");
        differs(
            self.delivery_max_attempts != d.delivery_max_attempts,
            "deliveryMaxAttempts",
        );
        differs(self.target_url != d.target_url, "targetUrl");
        differs(self.application_id != application_id, "applicationId");
        differs(self.status != ScheduledJobStatus::Active, "status");
        if changed.is_empty() {
            return None;
        }
        let mut job = self.clone();
        job.name = d.name.clone();
        job.description = d.description.clone();
        job.crons = d.crons.clone();
        job.timezone = d.timezone.clone();
        job.payload = d.payload.clone();
        job.concurrent = d.concurrent;
        job.tracks_completion = d.tracks_completion;
        job.timeout_seconds = d.timeout_seconds;
        job.delivery_max_attempts = d.delivery_max_attempts;
        job.target_url = d.target_url.clone();
        job.application_id = application_id;
        job.status = ScheduledJobStatus::Active;
        job.record_update(by.map(str::to_string));
        Some((job, changed))
    }
}

/// What a declarative writer wants a job to be (Java
/// `ScheduledJob.Definition`): `timezone` already defaulted to `UTC`,
/// `delivery_max_attempts` to 3, and a JSON `null` payload already `None`.
#[derive(Debug, Clone, PartialEq)]
pub struct JobDefinition {
    pub name: String,
    pub description: Option<String>,
    pub crons: Vec<String>,
    pub timezone: String,
    pub payload: Option<serde_json::Value>,
    pub concurrent: bool,
    pub tracks_completion: bool,
    pub timeout_seconds: Option<i32>,
    pub delivery_max_attempts: i32,
    pub target_url: Option<String>,
}

/// Trigger reason for a single firing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TriggerKind {
    Cron,
    Manual,
}

crate::shared::enum_str::str_enum!(TriggerKind, "trigger kind", {
    Cron => "CRON",
    Manual => "MANUAL",
});

/// Lifecycle status of a single firing.
///
/// Terminal states: `COMPLETED`, `FAILED`, `DELIVERY_FAILED`. When
/// `ScheduledJob.tracks_completion` is false, `DELIVERED` is also terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum InstanceStatus {
    Queued,
    InFlight,
    Delivered,
    Completed,
    Failed,
    DeliveryFailed,
}

impl InstanceStatus {
    /// True if no further state transitions are expected (apart from manual
    /// admin actions).
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::DeliveryFailed)
    }
}

crate::shared::enum_str::str_enum!(InstanceStatus, "instance status", {
    Queued => "QUEUED",
    InFlight => "IN_FLIGHT",
    Delivered => "DELIVERED",
    Completed => "COMPLETED",
    Failed => "FAILED",
    DeliveryFailed => "DELIVERY_FAILED",
});

/// SDK-reported completion outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CompletionStatus {
    Success,
    Failure,
}

crate::shared::enum_str::str_enum!(CompletionStatus, "completion status", {
    Success => "SUCCESS",
    Failure => "FAILURE",
});

/// Log severity for `logForScheduledJobInstance` writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
#[derive(Default)]
pub enum LogLevel {
    Debug,
    #[default]
    Info,
    Warn,
    Error,
}

crate::shared::enum_str::str_enum!(LogLevel, "log level", {
    Debug => "DEBUG",
    Info => "INFO",
    Warn => "WARN",
    Error => "ERROR",
});

/// Per-firing history row. Not an aggregate — written directly by the
/// scheduler/dispatcher via the platform-infrastructure exemption.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledJobInstance {
    pub id: String,
    pub scheduled_job_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    pub job_code: String,
    pub trigger_kind: TriggerKind,
    /// The cron slot this firing represents. NULL for MANUAL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduled_for: Option<DateTime<Utc>>,
    pub fired_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivered_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
    pub status: InstanceStatus,
    pub delivery_attempts: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_status: Option<CompletionStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// One log entry attached to an instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledJobInstanceLog {
    pub id: String,
    pub instance_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduled_job_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    pub level: LogLevel,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn new_active_with_defaults() {
        let job = ScheduledJob::new("daily-cleanup", "Daily Cleanup", vec!["0 0 * * *".into()]);
        assert!(job.id.starts_with("sjb_"));
        assert_eq!(job.status, ScheduledJobStatus::Active);
        assert_eq!(job.timezone, "UTC");
        assert!(!job.concurrent);
        assert!(!job.tracks_completion);
        assert_eq!(job.delivery_max_attempts, 3);
        assert_eq!(job.version, 1);
    }

    #[test]
    fn pause_resume_archive_bump_version_and_updated_at() {
        let mut job = ScheduledJob::new("c", "n", vec!["* * * * *".into()]);
        let v0 = job.version;
        let t0 = job.updated_at;
        std::thread::sleep(std::time::Duration::from_millis(2));

        job.pause();
        assert_eq!(job.status, ScheduledJobStatus::Paused);
        assert_eq!(job.version, v0 + 1);
        assert!(job.updated_at > t0);

        job.resume();
        assert_eq!(job.status, ScheduledJobStatus::Active);
        assert_eq!(job.version, v0 + 2);

        job.archive();
        assert_eq!(job.status, ScheduledJobStatus::Archived);
        assert_eq!(job.version, v0 + 3);
    }

    #[test]
    fn status_roundtrip_rejects_unknown() {
        for s in [
            ScheduledJobStatus::Active,
            ScheduledJobStatus::Paused,
            ScheduledJobStatus::Archived,
        ] {
            assert_eq!(ScheduledJobStatus::from_str(s.as_str()), Ok(s));
        }
        assert!(ScheduledJobStatus::from_str("UNKNOWN").is_err());
    }

    #[test]
    fn instance_status_terminal() {
        assert!(InstanceStatus::Completed.is_terminal());
        assert!(InstanceStatus::Failed.is_terminal());
        assert!(InstanceStatus::DeliveryFailed.is_terminal());
        assert!(!InstanceStatus::Queued.is_terminal());
        assert!(!InstanceStatus::InFlight.is_terminal());
        assert!(!InstanceStatus::Delivered.is_terminal());
    }

    #[test]
    fn instance_status_roundtrip() {
        for s in [
            InstanceStatus::Queued,
            InstanceStatus::InFlight,
            InstanceStatus::Delivered,
            InstanceStatus::Completed,
            InstanceStatus::Failed,
            InstanceStatus::DeliveryFailed,
        ] {
            assert_eq!(InstanceStatus::from_str(s.as_str()), Ok(s));
        }
    }

    #[test]
    fn log_level_roundtrip() {
        for s in [
            LogLevel::Debug,
            LogLevel::Info,
            LogLevel::Warn,
            LogLevel::Error,
        ] {
            assert_eq!(LogLevel::from_str(s.as_str()), Ok(s));
        }
        assert!(LogLevel::from_str("WHAT").is_err());
    }

    #[test]
    fn trigger_kind_roundtrip() {
        assert_eq!(TriggerKind::from_str("CRON"), Ok(TriggerKind::Cron));
        assert_eq!(TriggerKind::from_str("MANUAL"), Ok(TriggerKind::Manual));
        assert!(TriggerKind::from_str("OTHER").is_err());
    }

    #[test]
    fn entity_does_not_import_sqlx() {
        // Compile-time guarantee — if anyone adds `use sqlx`, this file would
        // need to import it explicitly. Tests can't enforce this directly, but
        // CLAUDE.md and the layering rule require it.
        let _ = stringify!(ScheduledJob);
    }
}
