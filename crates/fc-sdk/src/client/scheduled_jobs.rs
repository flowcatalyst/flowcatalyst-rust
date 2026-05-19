//! Scheduled-job management operations.
//!
//! A `ScheduledJob` is a cron-driven (or manually-fired) job definition
//! that the platform's scheduler fires into a webhook target URL. Each
//! firing produces a `ScheduledJobInstance` which the SDK callback path
//! can log against and mark complete.

use super::applications::CreatedResponse;
use super::{ClientError, FlowCatalystClient};
use serde::{Deserialize, Serialize};

// ── Request DTOs ──────────────────────────────────────────────────────────

/// Request to create a scheduled job.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CreateScheduledJobRequest {
    pub code: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// `None` = platform-scoped (anchor only); `Some` = client-scoped.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    pub crons: Vec<String>,
    /// Defaults to `UTC` server-side.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
    #[serde(default)]
    pub concurrent: bool,
    #[serde(default)]
    pub tracks_completion: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery_max_attempts: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_url: Option<String>,
}

/// Request to update a scheduled job. All fields optional.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateScheduledJobRequest {
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

/// Request body for a manual fire.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FireRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
}

/// Log entry to append to a running instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceLogRequest {
    pub message: String,
    #[serde(default)]
    pub level: LogLevel,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum LogLevel {
    Debug,
    #[default]
    Info,
    Warn,
    Error,
}

/// Mark an instance as complete.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceCompleteRequest {
    pub status: CompletionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum CompletionStatus {
    Success,
    Failure,
}

// ── Response DTOs ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledJobResponse {
    pub id: String,
    #[serde(default)]
    pub client_id: Option<String>,
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub status: String,
    pub crons: Vec<String>,
    pub timezone: String,
    #[serde(default)]
    pub payload: Option<serde_json::Value>,
    pub concurrent: bool,
    pub tracks_completion: bool,
    #[serde(default)]
    pub timeout_seconds: Option<i32>,
    pub delivery_max_attempts: i32,
    #[serde(default)]
    pub target_url: Option<String>,
    #[serde(default)]
    pub last_fired_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub created_by: Option<String>,
    #[serde(default)]
    pub updated_by: Option<String>,
    pub version: i32,
    /// Computed: true if any non-terminal instance currently exists.
    #[serde(default)]
    pub has_active_instance: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledJobInstanceResponse {
    pub id: String,
    pub scheduled_job_id: String,
    #[serde(default)]
    pub client_id: Option<String>,
    pub job_code: String,
    pub trigger_kind: String,
    #[serde(default)]
    pub scheduled_for: Option<String>,
    pub fired_at: String,
    #[serde(default)]
    pub delivered_at: Option<String>,
    #[serde(default)]
    pub completed_at: Option<String>,
    pub status: String,
    pub delivery_attempts: i32,
    #[serde(default)]
    pub delivery_error: Option<String>,
    #[serde(default)]
    pub completion_status: Option<String>,
    #[serde(default)]
    pub completion_result: Option<serde_json::Value>,
    #[serde(default)]
    pub correlation_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceLogResponse {
    pub id: String,
    pub instance_id: String,
    pub level: String,
    pub message: String,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
    pub created_at: String,
}

/// Paginated list wrapper used by `list` and `list_instances`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledJobListResponse {
    pub data: Vec<ScheduledJobResponse>,
    pub page: u32,
    pub size: u32,
    pub total: u64,
    pub total_pages: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledJobInstanceListResponse {
    pub data: Vec<ScheduledJobInstanceResponse>,
    pub page: u32,
    pub size: u32,
    pub total: u64,
    pub total_pages: u32,
}

/// List of instance logs — `GET /api/scheduled-jobs/instances/{id}/logs`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceLogListResponse {
    pub logs: Vec<InstanceLogResponse>,
    #[serde(default)]
    pub total: Option<u64>,
}

/// Response from a manual fire — returns the new instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FireResponse {
    pub instance_id: String,
}

// ── Filters ───────────────────────────────────────────────────────────────

/// Filters for listing scheduled jobs.
#[derive(Debug, Clone, Default)]
pub struct ScheduledJobFilters {
    /// Pass the literal `"platform"` to filter platform-scoped only.
    pub client_id: Option<String>,
    pub status: Option<String>,
    pub search: Option<String>,
    pub page: Option<u32>,
    pub size: Option<u32>,
}

/// Filters for listing instances of a job.
#[derive(Debug, Clone, Default)]
pub struct InstanceFilters {
    pub status: Option<String>,
    pub trigger_kind: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub page: Option<u32>,
    pub size: Option<u32>,
}

// ── Sync DTOs ─────────────────────────────────────────────────────────────

/// Request body for the per-resource sync endpoint.
///
/// `client_id = None` syncs platform-scoped jobs (anchor only). `archive_unlisted`
/// archives jobs not present in the list (versus the standard `remove_unlisted`
/// query-param convention used by other sync endpoints).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncScheduledJobsRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    pub jobs: Vec<SyncScheduledJobItem>,
    #[serde(default)]
    pub archive_unlisted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncScheduledJobItem {
    pub code: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub crons: Vec<String>,
    /// Defaults to `UTC` server-side.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
    #[serde(default)]
    pub concurrent: bool,
    #[serde(default)]
    pub tracks_completion: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<i32>,
    /// Defaults to `3` server-side.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery_max_attempts: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_url: Option<String>,
}

/// Result of a scheduled-jobs sync — distinct shape from the other syncs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncScheduledJobsResult {
    pub application_code: String,
    pub created: Vec<String>,
    pub updated: Vec<String>,
    pub archived: Vec<String>,
}

// ── Accessor ──────────────────────────────────────────────────────────────

/// Scheduled-jobs resource accessor — created via [`FlowCatalystClient::scheduled_jobs`].
pub struct ScheduledJobs<'a> {
    pub(crate) client: &'a FlowCatalystClient,
}

impl ScheduledJobs<'_> {
    /// Create a new scheduled job.
    ///
    /// Returns `{ id }` only. Call `get(&id)` if you need the full record.
    pub async fn create(
        &self,
        req: &CreateScheduledJobRequest,
    ) -> Result<CreatedResponse, ClientError> {
        self.client.post("/api/scheduled-jobs", req).await
    }

    /// List scheduled jobs with optional filters and pagination.
    pub async fn list(
        &self,
        filters: &ScheduledJobFilters,
    ) -> Result<ScheduledJobListResponse, ClientError> {
        let mut params = Vec::new();
        if let Some(ref c) = filters.client_id {
            params.push(format!("clientId={}", c));
        }
        if let Some(ref s) = filters.status {
            params.push(format!("status={}", s));
        }
        if let Some(ref q) = filters.search {
            params.push(format!("search={}", q));
        }
        if let Some(p) = filters.page {
            params.push(format!("page={}", p));
        }
        if let Some(s) = filters.size {
            params.push(format!("size={}", s));
        }
        let query = if params.is_empty() {
            String::new()
        } else {
            format!("?{}", params.join("&"))
        };
        self.client
            .get(&format!("/api/scheduled-jobs{}", query))
            .await
    }

    /// Get a scheduled job by ID.
    pub async fn get(&self, id: &str) -> Result<ScheduledJobResponse, ClientError> {
        self.client.get(&format!("/api/scheduled-jobs/{}", id)).await
    }

    /// Get a scheduled job by code. Optionally scope to a single client.
    pub async fn get_by_code(
        &self,
        code: &str,
        client_id: Option<&str>,
    ) -> Result<ScheduledJobResponse, ClientError> {
        let query = match client_id {
            Some(c) => format!("?clientId={}", c),
            None => String::new(),
        };
        self.client
            .get(&format!("/api/scheduled-jobs/by-code/{}{}", code, query))
            .await
    }

    /// Update a scheduled job.
    pub async fn update(
        &self,
        id: &str,
        req: &UpdateScheduledJobRequest,
    ) -> Result<ScheduledJobResponse, ClientError> {
        self.client
            .put(&format!("/api/scheduled-jobs/{}", id), req)
            .await
    }

    /// Pause a scheduled job.
    pub async fn pause(&self, id: &str) -> Result<ScheduledJobResponse, ClientError> {
        self.client
            .post_action(&format!("/api/scheduled-jobs/{}/pause", id))
            .await
    }

    /// Resume a paused scheduled job.
    pub async fn resume(&self, id: &str) -> Result<ScheduledJobResponse, ClientError> {
        self.client
            .post_action(&format!("/api/scheduled-jobs/{}/resume", id))
            .await
    }

    /// Archive (soft-delete) a scheduled job. Distinct from `delete` —
    /// archived jobs are kept for audit.
    pub async fn archive(&self, id: &str) -> Result<ScheduledJobResponse, ClientError> {
        self.client
            .post_action(&format!("/api/scheduled-jobs/{}/archive", id))
            .await
    }

    /// Hard-delete a scheduled job.
    pub async fn delete(&self, id: &str) -> Result<(), ClientError> {
        self.client
            .delete_req(&format!("/api/scheduled-jobs/{}", id))
            .await
    }

    /// Manually fire a scheduled job. Returns the new instance ID.
    pub async fn fire(
        &self,
        id: &str,
        req: &FireRequest,
    ) -> Result<FireResponse, ClientError> {
        self.client
            .post(&format!("/api/scheduled-jobs/{}/fire", id), req)
            .await
    }

    /// List instances for a scheduled job with optional filters.
    pub async fn list_instances(
        &self,
        job_id: &str,
        filters: &InstanceFilters,
    ) -> Result<ScheduledJobInstanceListResponse, ClientError> {
        let mut params = Vec::new();
        if let Some(ref s) = filters.status {
            params.push(format!("status={}", s));
        }
        if let Some(ref t) = filters.trigger_kind {
            params.push(format!("triggerKind={}", t));
        }
        if let Some(ref f) = filters.from {
            params.push(format!("from={}", f));
        }
        if let Some(ref t) = filters.to {
            params.push(format!("to={}", t));
        }
        if let Some(p) = filters.page {
            params.push(format!("page={}", p));
        }
        if let Some(s) = filters.size {
            params.push(format!("size={}", s));
        }
        let query = if params.is_empty() {
            String::new()
        } else {
            format!("?{}", params.join("&"))
        };
        self.client
            .get(&format!("/api/scheduled-jobs/{}/instances{}", job_id, query))
            .await
    }

    /// Get a single scheduled-job instance.
    pub async fn get_instance(
        &self,
        instance_id: &str,
    ) -> Result<ScheduledJobInstanceResponse, ClientError> {
        self.client
            .get(&format!("/api/scheduled-jobs/instances/{}", instance_id))
            .await
    }

    /// List logs for an instance.
    pub async fn list_instance_logs(
        &self,
        instance_id: &str,
    ) -> Result<InstanceLogListResponse, ClientError> {
        self.client
            .get(&format!(
                "/api/scheduled-jobs/instances/{}/logs",
                instance_id
            ))
            .await
    }

    /// SDK callback — append a log entry to a running instance.
    pub async fn log_for_instance(
        &self,
        instance_id: &str,
        req: &InstanceLogRequest,
    ) -> Result<InstanceLogResponse, ClientError> {
        self.client
            .post(
                &format!("/api/scheduled-jobs/instances/{}/log", instance_id),
                req,
            )
            .await
    }

    /// SDK callback — mark an instance complete with the given status.
    pub async fn complete_instance(
        &self,
        instance_id: &str,
        req: &InstanceCompleteRequest,
    ) -> Result<ScheduledJobInstanceResponse, ClientError> {
        self.client
            .post(
                &format!("/api/scheduled-jobs/instances/{}/complete", instance_id),
                req,
            )
            .await
    }

    /// Sync scheduled jobs for an application — declarative reconciliation
    /// against `POST /api/applications/{appCode}/scheduled-jobs/sync`.
    ///
    /// Unlike the other syncs, scheduled-jobs sync uses `archiveUnlisted` in
    /// the body (not `removeUnlisted` in the query) and returns a distinct
    /// `{ applicationCode, created, updated, archived }` shape with per-code
    /// vectors rather than counts.
    pub async fn sync(
        &self,
        app_code: &str,
        req: &SyncScheduledJobsRequest,
    ) -> Result<SyncScheduledJobsResult, ClientError> {
        self.client
            .post(
                &format!("/api/applications/{}/scheduled-jobs/sync", app_code),
                req,
            )
            .await
    }
}
