//! FlowCatalyst Platform API Client
//!
//! HTTP client for the FlowCatalyst platform API. Operations are grouped
//! by resource and accessed via methods that return a borrowed accessor:
//!
//! ```ignore
//! use fc_sdk::client::FlowCatalystClient;
//!
//! let client = FlowCatalystClient::new("http://localhost:8080")
//!     .with_token("your-api-token");
//!
//! // Manage event types
//! let event_type = client.event_types().create(&req).await?;
//!
//! // Sync from an application manifest (per-resource)
//! let result = client.event_types().sync("orders", &sync_req, true).await?;
//!
//! // Or use the multi-resource orchestrator (sync module)
//! ```

pub mod applications;
pub mod audit_logs;
pub mod clients;
pub mod connections;
pub mod dispatch_pools;
pub mod event_types;
pub mod me;
pub mod permissions;
pub mod principals;
pub mod processes;
pub mod roles;
pub mod router;
pub mod scheduled_jobs;
pub mod subscriptions;

use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};

/// Result body returned by every per-resource sync endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncResult {
    pub application_code: String,
    pub created: u32,
    pub updated: u32,
    pub deleted: u32,
    pub synced_codes: Vec<String>,
}

// Re-export request/response DTOs at the module root so callers can write
// `use fc_sdk::client::{CreateApplicationRequest, ...}` without knowing
// which file each type lives in.
pub use applications::*;
pub use audit_logs::*;
pub use clients::*;
pub use connections::*;
pub use dispatch_pools::*;
pub use event_types::*;
pub use me::*;
pub use permissions::*;
pub use principals::*;
pub use processes::*;
pub use roles::*;
pub use router::*;
pub use scheduled_jobs::*;
pub use subscriptions::*;

/// HTTP client for the FlowCatalyst platform API.
///
/// Operations are exposed via resource-accessor methods — call
/// `client.applications().list().await?` rather than a free
/// `client.list_applications().await?`.
#[derive(Clone)]
pub struct FlowCatalystClient {
    base_url: String,
    pub(crate) http: reqwest::Client,
    token: Option<String>,
    /// Optional override base URL for the message router's monitoring
    /// endpoints. The router is a separate process from the platform and
    /// usually runs at its own host. If `None`, router methods fall back
    /// to `base_url`.
    pub(crate) router_base_url: Option<String>,
}

impl FlowCatalystClient {
    /// Create a new client with the given base URL.
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            http: reqwest::Client::new(),
            token: None,
            router_base_url: None,
        }
    }

    /// Set the bearer token for authentication.
    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    /// Set a custom reqwest client (e.g., with custom TLS config).
    pub fn with_http_client(mut self, client: reqwest::Client) -> Self {
        self.http = client;
        self
    }

    /// Set the message router's base URL for monitoring endpoints
    /// (`/monitoring/in-flight-messages/...`). The router runs at a
    /// different host than the platform; if you only configured `base_url`,
    /// router calls will hit the platform host instead.
    pub fn with_router_url(mut self, url: impl Into<String>) -> Self {
        self.router_base_url = Some(url.into().trim_end_matches('/').to_string());
        self
    }

    // ── Resource accessors ──────────────────────────────────────────────

    /// Applications — `/api/applications/*`.
    pub fn applications(&self) -> applications::Applications<'_> {
        applications::Applications { client: self }
    }

    /// Audit logs — `/api/audit-logs/*`.
    pub fn audit_logs(&self) -> audit_logs::AuditLogs<'_> {
        audit_logs::AuditLogs { client: self }
    }

    /// Clients (tenants) — `/api/clients/*`.
    pub fn clients(&self) -> clients::Clients<'_> {
        clients::Clients { client: self }
    }

    /// Connections — `/api/connections/*`.
    pub fn connections(&self) -> connections::Connections<'_> {
        connections::Connections { client: self }
    }

    /// Dispatch pools — `/api/dispatch-pools/*`.
    pub fn dispatch_pools(&self) -> dispatch_pools::DispatchPools<'_> {
        dispatch_pools::DispatchPools { client: self }
    }

    /// Event types — `/api/event-types/*`.
    pub fn event_types(&self) -> event_types::EventTypes<'_> {
        event_types::EventTypes { client: self }
    }

    /// Current user context — `/api/me/*`.
    pub fn me(&self) -> me::Me<'_> {
        me::Me { client: self }
    }

    /// Permissions catalogue — `/api/roles/permissions/*`.
    pub fn permissions(&self) -> permissions::Permissions<'_> {
        permissions::Permissions { client: self }
    }

    /// Principals (users + service accounts) — `/api/principals/*`.
    pub fn principals(&self) -> principals::Principals<'_> {
        principals::Principals { client: self }
    }

    /// Process documentation — `/api/processes/*`.
    pub fn processes(&self) -> processes::Processes<'_> {
        processes::Processes { client: self }
    }

    /// Roles — `/api/roles/*`.
    pub fn roles(&self) -> roles::Roles<'_> {
        roles::Roles { client: self }
    }

    /// Message-router monitoring — `/monitoring/in-flight-messages/*`.
    pub fn router(&self) -> router::Router<'_> {
        router::Router { client: self }
    }

    /// Scheduled jobs — `/api/scheduled-jobs/*`.
    pub fn scheduled_jobs(&self) -> scheduled_jobs::ScheduledJobs<'_> {
        scheduled_jobs::ScheduledJobs { client: self }
    }

    /// Subscriptions — `/api/subscriptions/*`.
    pub fn subscriptions(&self) -> subscriptions::Subscriptions<'_> {
        subscriptions::Subscriptions { client: self }
    }

    // ── Internal HTTP helpers ───────────────────────────────────────────

    pub(crate) fn headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        if let Some(ref token) = self.token {
            if let Ok(val) = HeaderValue::from_str(&format!("Bearer {}", token)) {
                headers.insert(AUTHORIZATION, val);
            }
        }
        headers
    }

    pub(crate) fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    pub(crate) async fn get<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<T, ClientError> {
        let resp = self
            .http
            .get(self.url(path))
            .headers(self.headers())
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

    pub(crate) async fn post<B: Serialize, T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ClientError> {
        let resp = self
            .http
            .post(self.url(path))
            .headers(self.headers())
            .json(body)
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

    pub(crate) async fn put<B: Serialize, T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ClientError> {
        let resp = self
            .http
            .put(self.url(path))
            .headers(self.headers())
            .json(body)
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

    pub(crate) async fn delete_req(&self, path: &str) -> Result<(), ClientError> {
        let resp = self
            .http
            .delete(self.url(path))
            .headers(self.headers())
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

        Ok(())
    }

    /// DELETE that returns a parsed response body (e.g. the updated resource).
    pub(crate) async fn delete_with_response<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<T, ClientError> {
        let resp = self
            .http
            .delete(self.url(path))
            .headers(self.headers())
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

    pub(crate) async fn post_action<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<T, ClientError> {
        let resp = self
            .http
            .post(self.url(path))
            .headers(self.headers())
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

    pub(crate) async fn post_empty(&self, path: &str) -> Result<(), ClientError> {
        let resp = self
            .http
            .post(self.url(path))
            .headers(self.headers())
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

        Ok(())
    }

    /// PUT with a JSON body, discarding the response body. For platform
    /// endpoints that return 204 No Content on success (e.g. update flows).
    pub(crate) async fn put_empty<B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<(), ClientError> {
        let resp = self
            .http
            .put(self.url(path))
            .headers(self.headers())
            .json(body)
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

        Ok(())
    }
}

/// Error type for client operations.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("API error (HTTP {status}): {body}")]
    Api { status: u16, body: String },
}
