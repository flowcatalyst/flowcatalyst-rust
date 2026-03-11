//! FlowCatalyst Platform API Client
//!
//! HTTP client for managing event types, subscriptions, connections,
//! and performing sync operations against the FlowCatalyst platform API.
//!
//! # Example
//!
//! ```ignore
//! use fc_sdk::client::FlowCatalystClient;
//!
//! let client = FlowCatalystClient::new("http://localhost:8080")
//!     .with_token("your-api-token");
//!
//! // Manage event types
//! let event_type = client.create_event_type(&CreateEventTypeRequest {
//!     code: "orders:fulfillment:shipment:shipped".to_string(),
//!     name: "Shipment Shipped".to_string(),
//!     ..Default::default()
//! }).await?;
//!
//! // Sync from application manifest
//! let result = client.sync_event_types("orders", &sync_req, true).await?;
//! ```

pub mod applications;
pub mod clients;
pub mod event_types;
pub mod subscriptions;
pub mod connections;
pub mod sync;

use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};

pub use applications::*;
pub use clients::*;
pub use event_types::*;
pub use subscriptions::*;
pub use connections::*;
pub use sync::*;

/// HTTP client for the FlowCatalyst platform API.
#[derive(Clone)]
pub struct FlowCatalystClient {
    base_url: String,
    http: reqwest::Client,
    token: Option<String>,
}

impl FlowCatalystClient {
    /// Create a new client with the given base URL.
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            http: reqwest::Client::new(),
            token: None,
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

    fn headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        if let Some(ref token) = self.token {
            if let Ok(val) = HeaderValue::from_str(&format!("Bearer {}", token)) {
                headers.insert(AUTHORIZATION, val);
            }
        }
        headers
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, ClientError> {
        let resp = self
            .http
            .get(&self.url(path))
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

    async fn post<B: Serialize, T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ClientError> {
        let resp = self
            .http
            .post(&self.url(path))
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

    async fn put<B: Serialize, T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ClientError> {
        let resp = self
            .http
            .put(&self.url(path))
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

    async fn delete_req(&self, path: &str) -> Result<(), ClientError> {
        let resp = self
            .http
            .delete(&self.url(path))
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

    async fn post_action<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<T, ClientError> {
        let resp = self
            .http
            .post(&self.url(path))
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

    async fn post_empty(&self, path: &str) -> Result<(), ClientError> {
        let resp = self
            .http
            .post(&self.url(path))
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
}

/// Error type for client operations.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("API error (HTTP {status}): {body}")]
    Api { status: u16, body: String },
}

/// Paginated list response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListResponse<T> {
    pub data: Vec<T>,
    #[serde(default)]
    pub total: Option<u64>,
    #[serde(default)]
    pub page: Option<u32>,
    #[serde(default)]
    pub page_size: Option<u32>,
}
