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
    #[serde(default)]
    pub application_id: Option<String>,
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
    #[serde(default)]
    pub scheduled_job_id: Option<String>,
    #[serde(default)]
    pub client_id: Option<String>,
    pub level: String,
    pub message: String,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
    pub created_at: String,
}

/// One page of scheduled jobs (Go's `OffsetPageScheduledJobResponse`).
/// Go spells the page count `total_pages`; `totalPages` is still read.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledJobListResponse {
    pub data: Vec<ScheduledJobResponse>,
    pub page: u32,
    pub size: u32,
    pub total: u64,
    #[serde(rename = "total_pages", alias = "totalPages")]
    pub total_pages: u32,
}

/// One page of instances (Go's `OffsetPageScheduledJobInstanceResponse`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledJobInstanceListResponse {
    pub data: Vec<ScheduledJobInstanceResponse>,
    pub page: u32,
    pub size: u32,
    pub total: u64,
    #[serde(rename = "total_pages", alias = "totalPages")]
    pub total_pages: u32,
}

/// Response from a manual fire (Go's `FireNowResponse`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FireResponse {
    pub instance_id: String,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub scheduled_job_id: Option<String>,
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

/// Filters for listing instances of a job (Go's `status`, `page`, `size`).
#[derive(Debug, Clone, Default)]
pub struct InstanceFilters {
    pub status: Option<String>,
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
        if let Some(ref s) = filters.status {
            params.push(("status", s.clone()));
        }
        if let Some(ref c) = filters.client_id {
            params.push(("clientId", c.clone()));
        }
        if let Some(ref q) = filters.search {
            params.push(("search", q.clone()));
        }
        if let Some(p) = filters.page {
            params.push(("page", p.to_string()));
        }
        if let Some(s) = filters.size {
            params.push(("size", s.to_string()));
        }
        let query = FlowCatalystClient::query_string(&params);
        self.client
            .get(&format!("/api/scheduled-jobs{}", query))
            .await
    }

    /// Get a scheduled job by ID.
    pub async fn get(&self, id: &str) -> Result<ScheduledJobResponse, ClientError> {
        self.client
            .get(&format!("/api/scheduled-jobs/{}", id))
            .await
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

    /// Update a scheduled job (204). Call `get(id)` for the updated record.
    pub async fn update(
        &self,
        id: &str,
        req: &UpdateScheduledJobRequest,
    ) -> Result<(), ClientError> {
        self.client
            .put_empty(&format!("/api/scheduled-jobs/{}", id), req)
            .await
    }

    /// Pause a scheduled job (204).
    pub async fn pause(&self, id: &str) -> Result<(), ClientError> {
        self.client
            .post_empty(&format!("/api/scheduled-jobs/{}/pause", id))
            .await
    }

    /// Resume a paused scheduled job (204).
    pub async fn resume(&self, id: &str) -> Result<(), ClientError> {
        self.client
            .post_empty(&format!("/api/scheduled-jobs/{}/resume", id))
            .await
    }

    /// Archive (soft-delete) a scheduled job (204). Distinct from `delete` —
    /// archived jobs are kept for audit.
    pub async fn archive(&self, id: &str) -> Result<(), ClientError> {
        self.client
            .post_empty(&format!("/api/scheduled-jobs/{}/archive", id))
            .await
    }

    /// Hard-delete a scheduled job.
    pub async fn delete(&self, id: &str) -> Result<(), ClientError> {
        self.client
            .delete_req(&format!("/api/scheduled-jobs/{}", id))
            .await
    }

    /// Manually fire a scheduled job. Returns the new instance ID.
    pub async fn fire(&self, id: &str, req: &FireRequest) -> Result<FireResponse, ClientError> {
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
            params.push(("status", s.clone()));
        }
        if let Some(p) = filters.page {
            params.push(("page", p.to_string()));
        }
        if let Some(s) = filters.size {
            params.push(("size", s.to_string()));
        }
        let query = FlowCatalystClient::query_string(&params);
        self.client
            .get(&format!(
                "/api/scheduled-jobs/{}/instances{}",
                job_id, query
            ))
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

    /// List logs for an instance. The platform answers a bare array.
    pub async fn list_instance_logs(
        &self,
        instance_id: &str,
    ) -> Result<Vec<InstanceLogResponse>, ClientError> {
        self.client
            .get(&format!(
                "/api/scheduled-jobs/instances/{}/logs",
                instance_id
            ))
            .await
    }

    /// SDK callback — append a log entry to a running instance (204).
    pub async fn log_for_instance(
        &self,
        instance_id: &str,
        req: &InstanceLogRequest,
    ) -> Result<(), ClientError> {
        self.client
            .post_no_content(
                &format!("/api/scheduled-jobs/instances/{}/log", instance_id),
                req,
            )
            .await
    }

    /// SDK callback — mark an instance complete with the given status (204).
    pub async fn complete_instance(
        &self,
        instance_id: &str,
        req: &InstanceCompleteRequest,
    ) -> Result<(), ClientError> {
        self.client
            .post_no_content(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::test_support::MockPlatform;

    const JOB: &str = r#"{"id":"sj_1","code":"nightly","name":"Nightly","status":"ACTIVE",
        "crons":["0 0 * * *"],"timezone":"UTC","concurrent":false,"tracksCompletion":true,
        "deliveryMaxAttempts":3,"hasActiveInstance":false,"version":1,
        "createdAt":"t","updatedAt":"t"}"#;

    #[tokio::test]
    async fn list_reads_gos_pagination_members() {
        let page = format!(r#"{{"data":[{JOB}],"page":0,"size":20,"total":1,"total_pages":1}}"#);
        let stub = MockPlatform::start(&[("GET", "/api/scheduled-jobs", 200, &page)]).await;
        let resp = stub
            .client()
            .scheduled_jobs()
            .list(&ScheduledJobFilters {
                status: Some("ACTIVE".into()),
                size: Some(20),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(resp.total_pages, 1);
        assert_eq!(resp.data[0].code, "nightly");
        assert_eq!(
            stub.single().query_pairs(),
            vec![
                ("status".to_string(), "ACTIVE".to_string()),
                ("size".to_string(), "20".to_string())
            ]
        );
    }

    #[tokio::test]
    async fn list_instances_sends_only_gos_filters() {
        let stub = MockPlatform::start(&[(
            "GET",
            "/api/scheduled-jobs/sj_1/instances",
            200,
            r#"{"data":[],"page":1,"size":10,"total":0,"total_pages":0}"#,
        )])
        .await;
        stub.client()
            .scheduled_jobs()
            .list_instances(
                "sj_1",
                &InstanceFilters {
                    status: Some("FAILED".into()),
                    page: Some(1),
                    size: Some(10),
                },
            )
            .await
            .unwrap();
        assert_eq!(
            stub.single().query_pairs(),
            vec![
                ("status".to_string(), "FAILED".to_string()),
                ("page".to_string(), "1".to_string()),
                ("size".to_string(), "10".to_string())
            ]
        );
    }

    #[tokio::test]
    async fn no_content_writes_accept_204() {
        let stub = MockPlatform::start(&[
            ("PUT", "/api/scheduled-jobs/sj_1", 204, ""),
            ("POST", "/api/scheduled-jobs/sj_1/pause", 204, ""),
            ("POST", "/api/scheduled-jobs/sj_1/resume", 204, ""),
            ("POST", "/api/scheduled-jobs/sj_1/archive", 204, ""),
            ("POST", "/api/scheduled-jobs/instances/in_1/log", 204, ""),
            (
                "POST",
                "/api/scheduled-jobs/instances/in_1/complete",
                204,
                "",
            ),
        ])
        .await;
        let c = stub.client();
        let jobs = c.scheduled_jobs();
        jobs.update(
            "sj_1",
            &UpdateScheduledJobRequest {
                name: Some("Nightly".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        jobs.pause("sj_1").await.unwrap();
        jobs.resume("sj_1").await.unwrap();
        jobs.archive("sj_1").await.unwrap();
        jobs.log_for_instance(
            "in_1",
            &InstanceLogRequest {
                message: "hello".into(),
                level: LogLevel::default(),
                metadata: None,
            },
        )
        .await
        .unwrap();
        jobs.complete_instance(
            "in_1",
            &InstanceCompleteRequest {
                status: CompletionStatus::Success,
                result: None,
            },
        )
        .await
        .unwrap();
        let reqs = stub.requests();
        assert_eq!(reqs.len(), 6);
        assert_eq!(
            reqs[4].json(),
            serde_json::json!({"message": "hello", "level": "INFO"})
        );
        assert_eq!(reqs[5].json(), serde_json::json!({"status": "SUCCESS"}));
    }

    #[tokio::test]
    async fn list_instance_logs_reads_a_bare_array_and_fire_reads_202() {
        let stub = MockPlatform::start(&[
            (
                "GET",
                "/api/scheduled-jobs/instances/in_1/logs",
                200,
                r#"[{"id":"log_1","instanceId":"in_1","scheduledJobId":"sj_1","level":"INFO",
                    "message":"m","createdAt":"t"}]"#,
            ),
            (
                "POST",
                "/api/scheduled-jobs/sj_1/fire",
                202,
                r#"{"id":"in_2","instanceId":"in_2","scheduledJobId":"sj_1"}"#,
            ),
        ])
        .await;
        let c = stub.client();
        let logs = c.scheduled_jobs().list_instance_logs("in_1").await.unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].scheduled_job_id.as_deref(), Some("sj_1"));
        let fired = c
            .scheduled_jobs()
            .fire("sj_1", &FireRequest::default())
            .await
            .unwrap();
        assert_eq!(fired.instance_id, "in_2");
    }
}
