//! The one place `fc-dev fn` talks HTTP (Java `FnClient`): JSON calls to
//! the platform with a cached `client_credentials` token, where a non-2xx
//! answer becomes [`CliError::Platform`] carrying the platform's
//! `{error, message, details}`; and raw calls to the function host, where
//! the answer, whatever its status, is the result.

use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use reqwest::{Method, StatusCode};
use serde_json::Value;
use tokio::sync::Mutex;

use super::credentials::Credentials;
use super::CliError;

pub struct FnClient {
    base: String,
    http: reqwest::Client,
    credentials: Credentials,
    token: Mutex<Option<String>>,
}

/// A raw call's answer.
pub struct RawResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl RawResponse {
    pub fn body_text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(300))
        .build()
        .expect("a plain reqwest client builds")
}

impl FnClient {
    pub fn new(credentials: Credentials) -> Self {
        Self {
            base: credentials.platform_url.clone(),
            http: http_client(),
            credentials,
            token: Mutex::new(None),
        }
    }

    /// The client-credentials token, fetched once per process.
    pub async fn bearer_token(&self) -> Result<String, CliError> {
        let mut cached = self.token.lock().await;
        if let Some(token) = cached.as_ref() {
            return Ok(token.clone());
        }
        let url = format!("{}/oauth/token", self.base);
        let response = self
            .http
            .post(&url)
            .form(&[
                ("grant_type", "client_credentials"),
                ("client_id", self.credentials.client_id.as_str()),
                ("client_secret", self.credentials.client_secret.as_str()),
            ])
            .send()
            .await
            .map_err(|e| network(&url, e))?;
        let status = response.status();
        let body: Value = response.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            let code = body["error"]
                .as_str()
                .unwrap_or("TOKEN_REFUSED")
                .to_string();
            let message = body["error_description"]
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| {
                    format!("the platform refused client {}", self.credentials.client_id)
                });
            return Err(CliError::Platform {
                code,
                message,
                status: status.as_u16(),
                details: Value::Null,
            });
        }
        let token = body["access_token"]
            .as_str()
            .ok_or_else(|| CliError::Other(format!("{url} answered no access_token")))?
            .to_string();
        *cached = Some(token.clone());
        Ok(token)
    }

    pub async fn get(&self, path: &str) -> Result<Option<Value>, CliError> {
        self.send(Method::GET, path, None).await
    }

    pub async fn post(&self, path: &str, body: Value) -> Result<Option<Value>, CliError> {
        self.send(Method::POST, path, Some(body)).await
    }

    pub async fn put(&self, path: &str, body: Value) -> Result<Option<Value>, CliError> {
        self.send(Method::PUT, path, Some(body)).await
    }

    async fn send(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Option<Value>, CliError> {
        let url = format!("{}{path}", self.base);
        let mut request = self
            .http
            .request(method, &url)
            .bearer_auth(self.bearer_token().await?);
        if let Some(body) = body {
            request = request
                .header(CONTENT_TYPE, "application/json")
                .body(body.to_string());
        }
        let response = request.send().await.map_err(|e| network(&url, e))?;
        read(response).await
    }

    /// `PUT` of raw bytes (`application/octet-stream`): the artifact upload.
    pub async fn put_bytes(&self, path: &str, bytes: Vec<u8>) -> Result<Option<Value>, CliError> {
        let url = format!("{}{path}", self.base);
        let response = self
            .http
            .put(&url)
            .bearer_auth(self.bearer_token().await?)
            .header(CONTENT_TYPE, "application/octet-stream")
            .body(bytes)
            .send()
            .await
            .map_err(|e| network(&url, e))?;
        read(response).await
    }
}

/// One call to an arbitrary URL; never fails on the answer's status.
pub async fn raw(
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: Vec<u8>,
) -> Result<RawResponse, CliError> {
    let method = Method::from_bytes(method.to_ascii_uppercase().as_bytes())
        .map_err(|_| CliError::Usage(format!("not an HTTP method: {method}")))?;
    let mut map = HeaderMap::new();
    for (name, value) in headers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| CliError::Usage(format!("not a header name: {name}")))?;
        let value = HeaderValue::from_str(value)
            .map_err(|_| CliError::Usage(format!("not a header value: {value}")))?;
        map.append(name, value);
    }
    let response = http_client()
        .request(method, url)
        .headers(map)
        .body(body)
        .send()
        .await
        .map_err(|e| network(url, e))?;
    let status = response.status().as_u16();
    let headers = response
        .headers()
        .iter()
        .map(|(k, v)| {
            (
                k.to_string(),
                String::from_utf8_lossy(v.as_bytes()).into_owned(),
            )
        })
        .collect();
    let body = response
        .bytes()
        .await
        .map_err(|e| network(url, e))?
        .to_vec();
    Ok(RawResponse {
        status,
        headers,
        body,
    })
}

/// `Authorization: Bearer …` for a raw call.
pub fn bearer_header(token: &str) -> (String, String) {
    (AUTHORIZATION.to_string(), format!("Bearer {token}"))
}

async fn read(response: reqwest::Response) -> Result<Option<Value>, CliError> {
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if status.is_success() {
        if text.trim().is_empty() {
            return Ok(None);
        }
        return serde_json::from_str(&text)
            .map(Some)
            .map_err(|e| CliError::Other(format!("the platform answered invalid JSON: {e}")));
    }
    Err(platform_error(status, &text))
}

fn platform_error(status: StatusCode, text: &str) -> CliError {
    if let Ok(body) = serde_json::from_str::<Value>(text) {
        if let Some(code) = body["error"]
            .as_str()
            .or_else(|| body["code"].as_str())
            .filter(|c| !c.trim().is_empty())
        {
            return CliError::Platform {
                code: code.to_string(),
                message: body["message"].as_str().unwrap_or("").to_string(),
                status: status.as_u16(),
                details: body.get("details").cloned().unwrap_or(Value::Null),
            };
        }
    }
    CliError::Platform {
        code: format!("HTTP_{}", status.as_u16()),
        message: format!("request failed with status {}", status.as_u16()),
        status: status.as_u16(),
        details: Value::Null,
    }
}

fn network(target: &str, e: reqwest::Error) -> CliError {
    CliError::Platform {
        code: "NETWORK_ERROR".to_string(),
        message: format!("could not reach {target}: {e}"),
        status: 0,
        details: Value::Null,
    }
}
