//! The router half of ledger A-01: reporting BLOCK_ON_ERROR siblings to the
//! platform's settled-message hook, `POST /api/dispatch/settled` (Go
//! `internal/router/settled.go`).
//!
//! When the head of a BLOCK_ON_ERROR group fails terminally, the messages
//! buffered behind it must not be delivered past the failure. Go ACKs them
//! off the broker (a redelivery would make the first sibling the new head
//! and deliver it — the reordering the mode exists to prevent) and tells the
//! platform which dispatch jobs it just dropped, so the platform can put
//! those rows back to PENDING behind the failed one instead of leaving them
//! at QUEUED/PROCESSING. The platform's reaper is the backstop for a report
//! that never arrives, so the report is best-effort: fire-and-forget on its
//! own task and timeout, never blocking the drainer, no retries.
//!
//! Authentication is per job, not per router: each job travels with the
//! scheduler-signed HMAC token the router already holds for it
//! (`Message::auth_token`, the same token it forwards as `Authorization:
//! Bearer` to `/api/dispatch/process`), and the platform verifies each
//! `{id, token}` pair. A message without a token did not come from the
//! platform scheduler, so there is no job row to settle and it is left out.
//!
//! Wired only when `FC_ROUTER_PLATFORM_URL` names the platform
//! ([`crate::QueueManagerBuilder::settled_reporter`]). Without it Rust keeps
//! its earlier behaviour and hands the siblings back to the broker
//! (`docs/parity/router-deviations-from-go.md`, D2).

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use fc_common::Message;
use serde::Serialize;
use tracing::warn;

/// Appended to the platform base URL.
pub const SETTLED_PATH: &str = "/api/dispatch/settled";

/// Jobs per request (Go `settledChunkSize`): comfortably inside the
/// endpoint's 1 MiB body and 10,000-job caps. A larger group goes over
/// several sequential requests; the endpoint is idempotent, so overlapping
/// chunks are harmless.
pub const SETTLED_CHUNK_SIZE: usize = 1000;

/// Bound on one chunk's HTTP call (Go `defaultSettledTimeout`).
pub const DEFAULT_SETTLED_TIMEOUT: Duration = Duration::from_secs(5);

/// Bound on a whole background report (Go `settledReportTimeout`) — its
/// exit guarantee, since it is deliberately detached from the drainer.
pub const SETTLED_REPORT_TIMEOUT: Duration = Duration::from_secs(10);

/// One ACKed dispatch job and the scheduler-signed token for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SettledJob {
    pub id: String,
    pub token: String,
}

/// One settled group: the ids ACKed off the broker behind a failed
/// BLOCK_ON_ERROR head. Only `reason` and `jobs` cross the wire; the pool
/// and group are for logs.
#[derive(Debug, Clone)]
pub struct SettledReport {
    pub pool_code: String,
    pub group: String,
    pub reason: String,
    pub jobs: Vec<SettledJob>,
}

/// Reports a settled group to the platform. [`HttpSettledReporter`] in
/// production; a fake in tests.
#[async_trait]
pub trait SettledReporter: Send + Sync {
    async fn report_settled(&self, report: &SettledReport) -> Result<(), String>;
}

#[derive(Serialize)]
struct SettledRequest<'a> {
    reason: &'a str,
    jobs: &'a [SettledJob],
}

/// `POST {platform}/api/dispatch/settled`, chunked (Go
/// `HTTPSettledReporter`). Owns a small dedicated client: one known,
/// trusted, low-volume destination.
pub struct HttpSettledReporter {
    url: String,
    client: reqwest::Client,
}

impl HttpSettledReporter {
    /// A reporter for the platform at `platform_base_url`, each chunk
    /// bounded by `timeout` ([`DEFAULT_SETTLED_TIMEOUT`] when `None`).
    pub fn new(platform_base_url: &str, timeout: Option<Duration>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(timeout.unwrap_or(DEFAULT_SETTLED_TIMEOUT))
            .build()
            .expect("settled reporter HTTP client");
        Self {
            url: format!("{}{SETTLED_PATH}", platform_base_url.trim_end_matches('/')),
            client,
        }
    }

    /// The endpoint reports go to.
    pub fn url(&self) -> &str {
        &self.url
    }

    async fn post_chunk(&self, reason: &str, jobs: &[SettledJob]) -> Result<(), String> {
        let response = self
            .client
            .post(&self.url)
            .json(&SettledRequest { reason, jobs })
            .send()
            .await
            .map_err(|e| format!("do: {e}"))?;
        let status = response.status();
        if status != reqwest::StatusCode::OK {
            let body = response.text().await.unwrap_or_default();
            let body: String = body.chars().take(4096).collect();
            return Err(format!(
                "settled hook returned {}: {}",
                status.as_u16(),
                body.trim()
            ));
        }
        Ok(())
    }
}

#[async_trait]
impl SettledReporter for HttpSettledReporter {
    /// Every chunk is attempted even when an earlier one fails; the error
    /// names each chunk that failed.
    async fn report_settled(&self, report: &SettledReport) -> Result<(), String> {
        let mut errors = Vec::new();
        for (i, chunk) in report.jobs.chunks(SETTLED_CHUNK_SIZE).enumerate() {
            if let Err(e) = self.post_chunk(&report.reason, chunk).await {
                let start = i * SETTLED_CHUNK_SIZE;
                errors.push(format!(
                    "settled report chunk [{start}:{}]: {e}",
                    start + chunk.len()
                ));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
}

/// The `{id, token}` pair for a buffered message, or `None` when it carries
/// no scheduler-signed token (Go `dispatchJobFromMessage`).
pub fn settled_job_from_message(message: &Message) -> Option<SettledJob> {
    match message.auth_token.as_deref() {
        Some(token) if !token.is_empty() => Some(SettledJob {
            id: message.id.clone(),
            token: token.to_string(),
        }),
        _ => None,
    }
}

/// Send `report` on its own task, bounded by [`SETTLED_REPORT_TIMEOUT`],
/// logging a failure (Go `Pool.reportSettled`). Returns at once; a report
/// with no jobs is not sent.
pub fn spawn_report(reporter: Arc<dyn SettledReporter>, report: SettledReport) {
    if report.jobs.is_empty() {
        return;
    }
    tokio::spawn(async move {
        let outcome =
            tokio::time::timeout(SETTLED_REPORT_TIMEOUT, reporter.report_settled(&report)).await;
        let error = match outcome {
            Ok(Ok(())) => return,
            Ok(Err(e)) => e,
            Err(_) => format!("timed out after {SETTLED_REPORT_TIMEOUT:?}"),
        };
        warn!(
            pool = %report.pool_code,
            group = %report.group,
            jobs = report.jobs.len(),
            error = %error,
            "settled-message hook failed; the platform reaper is the backstop"
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn message(id: &str, token: Option<&str>) -> Message {
        Message {
            id: id.to_string(),
            pool_code: "P".to_string(),
            auth_token: token.map(str::to_string),
            signing_secret: None,
            mediation_type: fc_common::MediationType::HTTP,
            mediation_target: "http://t".to_string(),
            message_group_id: Some("g".to_string()),
            high_priority: false,
            dispatch_mode: fc_common::DispatchMode::BlockOnError,
            dispatch_mode_specified: true,
        }
    }

    #[test]
    fn only_messages_with_a_token_are_reported() {
        assert_eq!(
            settled_job_from_message(&message("a", Some("tok"))),
            Some(SettledJob {
                id: "a".into(),
                token: "tok".into()
            })
        );
        assert_eq!(settled_job_from_message(&message("b", None)), None);
        assert_eq!(settled_job_from_message(&message("c", Some(""))), None);
    }

    #[tokio::test]
    async fn posts_reason_and_jobs_in_chunks_without_a_router_credential() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(SETTLED_PATH))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"settled": 0})),
            )
            .mount(&server)
            .await;

        let reporter = HttpSettledReporter::new(&format!("{}/", server.uri()), None);
        assert_eq!(reporter.url(), format!("{}{SETTLED_PATH}", server.uri()));
        let jobs: Vec<SettledJob> = (0..SETTLED_CHUNK_SIZE + 1)
            .map(|i| SettledJob {
                id: format!("j{i}"),
                token: format!("t{i}"),
            })
            .collect();
        let report = SettledReport {
            pool_code: "P".into(),
            group: "g".into(),
            reason: "head failed under BLOCK_ON_ERROR".into(),
            jobs,
        };
        reporter.report_settled(&report).await.unwrap();

        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 2, "1001 jobs go in two chunks");
        let first: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(first["reason"], "head failed under BLOCK_ON_ERROR");
        assert_eq!(first["jobs"].as_array().unwrap().len(), SETTLED_CHUNK_SIZE);
        assert_eq!(
            first["jobs"][0],
            serde_json::json!({"id": "j0", "token": "t0"})
        );
        let second: serde_json::Value = serde_json::from_slice(&requests[1].body).unwrap();
        assert_eq!(second["jobs"].as_array().unwrap().len(), 1);
        assert!(requests[0].headers.get("authorization").is_none());
    }

    #[tokio::test]
    async fn a_refusal_is_an_error_naming_the_chunk() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(401).set_body_string("bad token"))
            .mount(&server)
            .await;
        let reporter = HttpSettledReporter::new(&server.uri(), None);
        let err = reporter
            .report_settled(&SettledReport {
                pool_code: "P".into(),
                group: "g".into(),
                reason: "r".into(),
                jobs: vec![SettledJob {
                    id: "a".into(),
                    token: "t".into(),
                }],
            })
            .await
            .unwrap_err();
        assert!(
            err.contains("[0:1]") && err.contains("401") && err.contains("bad token"),
            "{err}"
        );
    }
}
