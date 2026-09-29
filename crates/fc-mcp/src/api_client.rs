use std::sync::Arc;

use anyhow::{anyhow, Result};
use reqwest::Client;
use serde_json::Value;

use crate::auth::TokenManager;
use crate::config::Config;

/// Thin HTTP wrapper. Responses come back as `serde_json::Value` so the MCP
/// tools can round-trip them as JSON text without re-modelling every DTO; the
/// platform's `EventTypeResponse` / `SubscriptionResponse` evolve, and we don't
/// want the MCP layer to be a second source of truth for those shapes.
pub struct ApiClient {
    http: Client,
    /// `{base}/api` — the bearer-token programmable surface.
    base_api: String,
    /// `{base}` — needed because the OpenAPI doc is currently served by a
    /// `/bff/*` route. BFF is path convention, not auth boundary: bearer
    /// tokens authenticate there too.
    base_url: String,
    tokens: Arc<TokenManager>,
}

#[derive(Default, Debug)]
pub struct ListEventTypesFilters<'a> {
    pub status: Option<&'a str>,
    pub application: Option<&'a str>,
    pub client_id: Option<&'a str>,
    pub subdomain: Option<&'a str>,
    pub aggregate: Option<&'a str>,
}

impl ApiClient {
    pub fn new(config: &Config, http: Client, tokens: Arc<TokenManager>) -> Self {
        Self {
            base_api: format!("{}/api", config.base_url),
            base_url: config.base_url.clone(),
            http,
            tokens,
        }
    }

    pub async fn list_event_types(&self, filters: &ListEventTypesFilters<'_>) -> Result<Value> {
        let mut req = self.http.get(format!("{}/event-types", self.base_api));
        let mut query: Vec<(&str, &str)> = Vec::new();
        if let Some(s) = filters.status {
            query.push(("status", s));
        }
        if let Some(a) = filters.application {
            query.push(("application", a));
        }
        if let Some(c) = filters.client_id {
            query.push(("clientId", c));
        }
        if let Some(sd) = filters.subdomain {
            query.push(("subdomain", sd));
        }
        if let Some(ag) = filters.aggregate {
            query.push(("aggregate", ag));
        }
        if !query.is_empty() {
            req = req.query(&query);
        }
        self.send(req).await
    }

    pub async fn get_event_type(&self, id: &str) -> Result<Value> {
        let url = format!("{}/event-types/{}", self.base_api, urlencode(id));
        self.send(self.http.get(url)).await
    }

    pub async fn list_subscriptions(&self, client_id: Option<&str>) -> Result<Value> {
        let mut req = self.http.get(format!("{}/subscriptions", self.base_api));
        if let Some(c) = client_id {
            req = req.query(&[("clientId", c)]);
        }
        self.send(req).await
    }

    pub async fn get_subscription(&self, id: &str) -> Result<Value> {
        let url = format!("{}/subscriptions/{}", self.base_api, urlencode(id));
        self.send(self.http.get(url)).await
    }

    pub async fn list_applications(&self, active: Option<bool>) -> Result<Value> {
        let mut req = self.http.get(format!("{}/applications", self.base_api));
        if let Some(a) = active {
            req = req.query(&[("active", a.to_string())]);
        }
        self.send(req).await
    }

    pub async fn get_application_by_code(&self, code: &str) -> Result<Value> {
        let url = format!("{}/applications/by-code/{}", self.base_api, urlencode(code));
        self.send(self.http.get(url)).await
    }

    pub async fn list_roles(&self, source: Option<&str>) -> Result<Value> {
        let mut req = self.http.get(format!("{}/roles", self.base_api));
        if let Some(s) = source {
            req = req.query(&[("source", s)]);
        }
        self.send(req).await
    }

    pub async fn get_role(&self, id: &str) -> Result<Value> {
        let url = format!("{}/roles/{}", self.base_api, urlencode(id));
        self.send(self.http.get(url)).await
    }

    /// Fetch the CURRENT OpenAPI document for an application by code.
    /// Two-hop: resolve code → id, then read the spec. The OpenAPI
    /// endpoint lives under `/bff/developer` today; bearer tokens work
    /// there because `/bff` is a path convention, not an auth boundary.
    pub async fn get_openapi(&self, application_code: &str) -> Result<Value> {
        let app = self.get_application_by_code(application_code).await?;
        let app_id = app
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("application '{application_code}' has no id"))?
            .to_string();
        let url = format!(
            "{}/bff/developer/applications/{}/openapi/current",
            self.base_url,
            urlencode(&app_id),
        );
        self.send(self.http.get(url)).await
    }

    /// Fetch the calling principal — id, type, scope, roles, and the
    /// clients + applications it can act against.
    pub async fn whoami(&self) -> Result<Value> {
        self.send(self.http.get(format!("{}/me", self.base_api)))
            .await
    }

    /// List applications the calling principal has access to.
    pub async fn list_my_applications(&self) -> Result<Value> {
        self.send(self.http.get(format!("{}/me/applications", self.base_api)))
            .await
    }

    /// Get roles assignable to a given application (by application id).
    pub async fn list_roles_by_application(&self, app_id: &str) -> Result<Value> {
        let url = format!(
            "{}/roles/by-application/{}",
            self.base_api,
            urlencode(app_id),
        );
        self.send(self.http.get(url)).await
    }

    /// Compose an "everything about this app" view for an agent.
    /// Includes app metadata, default base URL, CURRENT OpenAPI doc (if
    /// synced), assignable roles, and event types. Individual sub-calls
    /// that 404 are tolerated — apps without a synced OpenAPI just get
    /// `openapi: null`, ditto for missing roles/event types.
    pub async fn get_application_capabilities(&self, application_code: &str) -> Result<Value> {
        let app = self.get_application_by_code(application_code).await?;
        let app_id = app
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("application '{application_code}' has no id"))?
            .to_string();

        let openapi_url = format!(
            "{}/bff/developer/applications/{}/openapi/current",
            self.base_url,
            urlencode(&app_id),
        );
        let openapi = self.send_optional(self.http.get(openapi_url)).await;

        let roles = self
            .send_optional(self.http.get(format!(
                "{}/roles/by-application/{}",
                self.base_api,
                urlencode(&app_id)
            )))
            .await;

        let event_types = self
            .send_optional(
                self.http
                    .get(format!("{}/event-types", self.base_api))
                    .query(&[("application", application_code), ("status", "CURRENT")]),
            )
            .await;

        Ok(serde_json::json!({
            "application": app,
            "openapi": openapi,
            "assignableRoles": roles,
            "eventTypes": event_types,
        }))
    }

    /// Same as `send` but treats 404 / failure as `null` instead of an
    /// error. Used by `get_application_capabilities` to assemble a partial
    /// view when some sub-resources haven't been synced yet.
    async fn send_optional(&self, req: reqwest::RequestBuilder) -> Value {
        match self.send(req).await {
            Ok(v) => v,
            Err(_) => Value::Null,
        }
    }

    async fn send(&self, req: reqwest::RequestBuilder) -> Result<Value> {
        let token = self.tokens.get_access_token().await?;
        let resp = req
            .header("Accept", "application/json")
            .bearer_auth(token)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("platform request failed ({status}): {body}"));
        }
        Ok(resp.json::<Value>().await?)
    }
}

fn urlencode(s: &str) -> String {
    // Only encodes path-segment-dangerous characters. IDs are TSIDs so this is
    // effectively a passthrough, but covers operators who paste a code with a
    // colon or slash.
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}
