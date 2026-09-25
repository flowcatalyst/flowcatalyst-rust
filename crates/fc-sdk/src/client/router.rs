//! Message-router monitoring endpoints.
//!
//! These call the **router** (a separate process from the platform) at the
//! `router_base_url` configured on the client via `.with_router_url(...)`.
//! If no router URL is configured, calls fall back to the platform's
//! `base_url`, which is correct only when the router and platform are
//! co-located (e.g. `fc-dev`).
//!
//! Designed for an external recovery / replay process that maintains its
//! own list of "messages that look stuck" and wants to confirm whether the
//! router is still actively processing each one before re-enqueueing.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{ClientError, FlowCatalystClient};

/// Response from `GET /monitoring/in-flight-messages/check`.
///
/// `inPipeline=true` → the router currently holds the message; the caller
/// should not re-enqueue. `inPipeline=false` → safe to resend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InPipelineCheckResponse {
    pub message_id: String,
    pub in_pipeline: bool,
    /// Populated only when `in_pipeline = true`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<InPipelineDetail>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InPipelineDetail {
    pub message_id: String,
    pub broker_message_id: Option<String>,
    pub queue_id: String,
    pub pool_code: String,
    pub elapsed_time_ms: u64,
    pub added_to_in_pipeline_at: String,
}

/// Body of `POST /monitoring/in-flight-messages/check-batch`.
///
/// Capped at 5000 ids per request server-side.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InPipelineBatchRequest {
    pub message_ids: Vec<String>,
}

/// Router monitoring accessor — created via [`FlowCatalystClient::router`].
pub struct Router<'a> {
    pub(crate) client: &'a FlowCatalystClient,
}

impl Router<'_> {
    fn router_url(&self, path: &str) -> String {
        let base = self
            .client
            .router_base_url
            .as_deref()
            .unwrap_or_else(|| {
                // No public accessor for base_url, so reuse the platform helper.
                // (router_url falls back to platform base_url when no override.)
                ""
            });
        if base.is_empty() {
            self.client.url(path)
        } else {
            format!("{}{}", base, path)
        }
    }

    /// Check whether a single application message ID is currently held in
    /// the router's in-pipeline map. O(1) on the server side.
    pub async fn in_pipeline(
        &self,
        message_id: &str,
    ) -> Result<InPipelineCheckResponse, ClientError> {
        let url = self.router_url("/monitoring/in-flight-messages/check");
        let resp = self
            .client
            .http
            .get(url)
            .headers(self.client.headers())
            .query(&[("messageId", message_id)])
            .send()
            .await
            .map_err(ClientError::Request)?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ClientError::Api {
                status: status.as_u16(),
                body,
            });
        }
        resp.json().await.map_err(ClientError::Request)
    }

    /// Batch-check whether each of the given application message IDs is
    /// currently held in the router's in-pipeline map. Returns
    /// `messageId → bool`. The server caps the batch at 5000 ids.
    pub async fn in_pipeline_batch(
        &self,
        message_ids: &[String],
    ) -> Result<HashMap<String, bool>, ClientError> {
        let url = self.router_url("/monitoring/in-flight-messages/check-batch");
        let body = InPipelineBatchRequest {
            message_ids: message_ids.to_vec(),
        };
        let resp = self
            .client
            .http
            .post(url)
            .headers(self.client.headers())
            .json(&body)
            .send()
            .await
            .map_err(ClientError::Request)?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ClientError::Api {
                status: status.as_u16(),
                body,
            });
        }
        resp.json().await.map_err(ClientError::Request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A router stub that records each request's `Authorization` header.
    async fn recording_router() -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let record = seen.clone();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let record = record.clone();
                tokio::spawn(async move {
                    let mut request = Vec::new();
                    let mut buf = [0u8; 4096];
                    loop {
                        let n = socket.read(&mut buf).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        request.extend_from_slice(&buf[..n]);
                        let text = String::from_utf8_lossy(&request).to_string();
                        let Some(head_end) = text.find("\r\n\r\n") else {
                            continue;
                        };
                        let header = |name: &str| {
                            text[..head_end].lines().find_map(|l| {
                                let (k, v) = l.split_once(':')?;
                                k.eq_ignore_ascii_case(name).then(|| v.trim().to_string())
                            })
                        };
                        let length: usize = header("content-length")
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(0);
                        if request.len() < head_end + 4 + length {
                            continue;
                        }
                        record
                            .lock()
                            .unwrap()
                            .push(header("authorization").unwrap_or_default());
                        let body = if text.starts_with("POST") {
                            r#"{"m1":true}"#
                        } else {
                            r#"{"messageId":"m1","inPipeline":false}"#
                        };
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        let _ = socket.write_all(response.as_bytes()).await;
                        return;
                    }
                });
            }
        });
        (format!("http://{addr}"), seen)
    }

    /// Owner ruling 2 of 2026-09-25 (Java 714f3f2d): the router will verify
    /// the platform bearer token, so both in-flight checks must carry the one
    /// the client uses for the platform.
    #[tokio::test]
    async fn in_flight_checks_send_the_platform_bearer_token() {
        let (router_url, seen) = recording_router().await;
        let client = FlowCatalystClient::new("http://127.0.0.1:1")
            .with_router_url(router_url)
            .with_token("tok-1");

        let single = client.router().in_pipeline("m1").await.unwrap();
        assert!(!single.in_pipeline);
        let batch = client
            .router()
            .in_pipeline_batch(&["m1".to_string()])
            .await
            .unwrap();
        assert_eq!(batch.get("m1"), Some(&true));

        assert_eq!(
            *seen.lock().unwrap(),
            vec!["Bearer tok-1".to_string(), "Bearer tok-1".to_string()]
        );
    }
}
