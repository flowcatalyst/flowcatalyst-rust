//! A small client for the platform API, identical for both sides.
//!
//! Every call goes through `call`, which records the request and response
//! in the side's setup log (so a setup failure is legible in the report)
//! and never retries on its own.

use std::time::Duration;

use anyhow::{anyhow, bail, Context};
use reqwest::Method;
use serde_json::{json, Value};

#[derive(Clone)]
pub struct Api {
    pub base: String,
    http: reqwest::Client,
    auth: Auth,
}

#[derive(Clone)]
enum Auth {
    None,
    Cookie(String),
    Bearer(String),
}

#[derive(Debug, Clone)]
pub struct ServiceAccount {
    pub id: String,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub signing_secret: Option<String>,
}

pub struct Response {
    pub status: u16,
    pub body: Value,
    pub text: String,
}

impl Api {
    pub fn new(base: &str) -> Api {
        Api {
            base: base.trim_end_matches('/').to_string(),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("reqwest client"),
            auth: Auth::None,
        }
    }

    pub fn with_bearer(&self, token: &str) -> Api {
        Api {
            auth: Auth::Bearer(token.to_string()),
            ..self.clone()
        }
    }

    pub fn bearer_token(&self) -> Option<String> {
        match &self.auth {
            Auth::Bearer(t) => Some(t.clone()),
            _ => None,
        }
    }

    pub async fn call(&self, method: Method, path: &str, body: Option<Value>) -> Response {
        let url = format!("{}{}", self.base, path);
        let mut req = self.http.request(method.clone(), &url);
        req = match &self.auth {
            Auth::None => req,
            Auth::Cookie(c) => req.header("cookie", c),
            Auth::Bearer(t) => req.bearer_auth(t),
        };
        if let Some(b) = &body {
            req = req.json(b);
        }
        match req.send().await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                let text = resp.text().await.unwrap_or_default();
                let body = serde_json::from_str(&text).unwrap_or(Value::Null);
                Response { status, body, text }
            }
            Err(e) => Response {
                status: 0,
                body: Value::Null,
                text: format!("transport error: {e}"),
            },
        }
    }

    /// `call` that insists on a 2xx.
    pub async fn ok(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> anyhow::Result<Value> {
        let m = method.clone();
        let r = self.call(method, path, body).await;
        if !(200..300).contains(&r.status) {
            bail!("{m} {path} -> {} {}", r.status, truncate(&r.text, 400));
        }
        Ok(r.body)
    }

    /// Log in with the bootstrap admin and keep the session cookie.
    ///
    /// Both sides set `fc_session` as a `Secure` cookie, which a jar would
    /// never send back over plain loopback HTTP; carry it by hand.
    pub async fn login(&self, email: &str, password: &str) -> anyhow::Result<Api> {
        let url = format!("{}/auth/login", self.base);
        let resp = self
            .http
            .post(&url)
            .json(&json!({"email": email, "password": password}))
            .send()
            .await
            .context("login transport")?;
        let status = resp.status().as_u16();
        let cookies: Vec<String> = resp
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .filter_map(|v| v.split(';').next())
            .map(str::to_string)
            .filter(|c| c.starts_with("fc_session="))
            .collect();
        let text = resp.text().await.unwrap_or_default();
        if !(200..300).contains(&status) || cookies.is_empty() {
            bail!("login -> {status} {}", truncate(&text, 400));
        }
        Ok(Api {
            auth: Auth::Cookie(cookies.join("; ")),
            ..self.clone()
        })
    }

    pub async fn create_service_account(
        &self,
        code: &str,
        name: &str,
    ) -> anyhow::Result<ServiceAccount> {
        let body = self
            .ok(
                Method::POST,
                "/api/service-accounts",
                Some(json!({"code": code, "name": name, "description": "delivery harness"})),
            )
            .await?;
        let id = str_at(&body, &["/serviceAccount/id", "/id", "/serviceAccountId"])
            .ok_or_else(|| anyhow!("service account id not found in {body}"))?;
        Ok(ServiceAccount {
            id,
            client_id: str_at(&body, &["/oauth/clientId", "/clientId"]),
            client_secret: str_at(&body, &["/oauth/clientSecret", "/clientSecret"]),
            signing_secret: str_at(&body, &["/webhook/signingSecret", "/signingSecret"]),
        })
    }

    pub async fn assign_roles(&self, sa_id: &str, roles: &[&str]) -> anyhow::Result<()> {
        let path = format!("/api/service-accounts/{sa_id}/roles");
        let body = json!({"roles": roles});
        let r = self.call(Method::PUT, &path, Some(body.clone())).await;
        if (200..300).contains(&r.status) {
            return Ok(());
        }
        let r2 = self.call(Method::POST, &path, Some(body)).await;
        if (200..300).contains(&r2.status) {
            return Ok(());
        }
        bail!(
            "assign roles {roles:?}: PUT -> {} {}; POST -> {} {}",
            r.status,
            truncate(&r.text, 300),
            r2.status,
            truncate(&r2.text, 300)
        )
    }

    pub async fn client_credentials_token(
        &self,
        client_id: &str,
        secret: &str,
    ) -> anyhow::Result<String> {
        let url = format!("{}/oauth/token", self.base);
        let resp = self
            .http
            .post(&url)
            .form(&[
                ("grant_type", "client_credentials"),
                ("client_id", client_id),
                ("client_secret", secret),
            ])
            .send()
            .await?;
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        match v.get("access_token").and_then(Value::as_str) {
            Some(t) if (200..300).contains(&status) => Ok(t.to_string()),
            _ => bail!("oauth/token -> {status} {}", truncate(&text, 400)),
        }
    }

    pub async fn create_pool(
        &self,
        code: &str,
        concurrency: u32,
        rate_limit: Option<u32>,
    ) -> anyhow::Result<String> {
        let body = self
            .ok(
                Method::POST,
                "/api/dispatch-pools",
                Some(json!({
                    "code": code,
                    "name": code,
                    "concurrency": concurrency,
                    "rateLimit": rate_limit,
                })),
            )
            .await?;
        str_at(&body, &["/id", "/dispatchPool/id"]).ok_or_else(|| anyhow!("pool id not in {body}"))
    }

    pub async fn create_event_type(&self, code: &str) -> anyhow::Result<()> {
        self.ok(
            Method::POST,
            "/api/event-types",
            Some(json!({"code": code, "name": code, "description": "delivery harness"})),
        )
        .await
        .map(|_| ())
    }

    pub async fn create_subscription(&self, body: Value) -> anyhow::Result<String> {
        let b = self
            .ok(Method::POST, "/api/subscriptions", Some(body))
            .await?;
        str_at(&b, &["/id", "/subscription/id"])
            .ok_or_else(|| anyhow!("subscription id not in {b}"))
    }
}

pub fn str_at(v: &Value, pointers: &[&str]) -> Option<String> {
    pointers
        .iter()
        .find_map(|p| v.pointer(p).and_then(Value::as_str).map(str::to_string))
}

pub fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        return s.to_string();
    }
    let mut end = n;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}
