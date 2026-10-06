//! Platform bearer tokens minted on demand, for an outbox poller that
//! authenticates as a service account instead of with a pasted static
//! token (Go's outbox `TokenSource` and `fcdev outbox`'s
//! `clientCredentialsTokenSource`).
//!
//! The dispatcher asks for a token per request; a 401 invalidates it so the
//! next request mints a fresh one. A failed mint fails the batch as
//! `GATEWAY_ERROR` (retryable), as Go does.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::Deserialize;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use tokio::sync::Mutex;

/// Supplies the bearer token for each platform request.
#[async_trait]
pub trait TokenSource: Send + Sync {
    /// A valid token, minted or cached.
    async fn token(&self) -> anyhow::Result<String>;
    /// Drop any cached token (called after a 401) so the next call mints.
    fn invalidate(&self) {}
}

/// Re-mint this long before the cached token expires, so it never expires
/// between the check and the platform call (Go's `tokenRefreshSkew`).
const REFRESH_SKEW: Duration = Duration::from_secs(60);
/// The lifetime assumed when the token endpoint omits `expires_in`.
const DEFAULT_TTL: Duration = Duration::from_secs(300);
/// Go's token-request timeout.
const MINT_TIMEOUT: Duration = Duration::from_secs(15);

/// OAuth `client_credentials` against the platform's `/oauth/token`,
/// cached until [`REFRESH_SKEW`] before expiry.
pub struct ClientCredentialsTokenSource {
    token_url: String,
    client_id: String,
    client_secret: String,
    scope: Option<String>,
    client: reqwest::Client,
    cached: Mutex<Option<(String, Instant)>>,
    invalidated: AtomicBool,
}

impl ClientCredentialsTokenSource {
    /// `token_url` is the full token endpoint (`<platform>/oauth/token`);
    /// `scope`, when given, narrows the minted token.
    #[expect(
        clippy::expect_used,
        reason = "a reqwest client built from fixed options fails only if the TLS backend cannot initialise: a start-up failure, not a runtime one"
    )]
    pub fn new(
        token_url: impl Into<String>,
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
        scope: Option<String>,
    ) -> Self {
        Self {
            token_url: token_url.into(),
            client_id: client_id.into(),
            client_secret: client_secret.into(),
            scope: scope.filter(|s| !s.is_empty()),
            client: reqwest::Client::builder()
                .timeout(MINT_TIMEOUT)
                .build()
                .expect("a plain reqwest client builds"),
            cached: Mutex::new(None),
            invalidated: AtomicBool::new(false),
        }
    }

    async fn mint(&self) -> anyhow::Result<(String, Duration)> {
        #[derive(Deserialize)]
        struct TokenResponse {
            #[serde(default)]
            access_token: String,
            #[serde(default)]
            expires_in: u64,
        }
        let mut form = vec![
            ("grant_type", "client_credentials"),
            ("client_id", self.client_id.as_str()),
            ("client_secret", self.client_secret.as_str()),
        ];
        if let Some(scope) = &self.scope {
            form.push(("scope", scope.as_str()));
        }
        let response = self
            .client
            .post(&self.token_url)
            .header("accept", "application/json")
            .form(&form)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("token request: {e}"))?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            let body: String = body.chars().take(200).collect();
            anyhow::bail!("token endpoint {status}: {body}");
        }
        let parsed: TokenResponse = serde_json::from_str(&body)
            .map_err(|e| anyhow::anyhow!("decode token response: {e}"))?;
        if parsed.access_token.is_empty() {
            anyhow::bail!("token endpoint returned no access_token");
        }
        let ttl = match parsed.expires_in {
            0 => DEFAULT_TTL,
            secs => Duration::from_secs(secs),
        };
        Ok((parsed.access_token, ttl))
    }
}

#[async_trait]
impl TokenSource for ClientCredentialsTokenSource {
    async fn token(&self) -> anyhow::Result<String> {
        let mut cached = self.cached.lock().await;
        if self.invalidated.swap(false, Ordering::SeqCst) {
            *cached = None;
        }
        if let Some((token, expires)) = cached.as_ref() {
            if Instant::now() + REFRESH_SKEW < *expires {
                return Ok(token.clone());
            }
        }
        let (token, ttl) = self.mint().await?;
        *cached = Some((token.clone(), Instant::now() + ttl));
        Ok(token)
    }

    fn invalidate(&self) {
        self.invalidated.store(true, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::State;
    use axum::http::StatusCode;
    use axum::routing::post;
    use axum::{Form, Json, Router};
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::net::TcpListener;

    async fn token_endpoint(
        State(minted): State<Arc<AtomicUsize>>,
        Form(form): Form<HashMap<String, String>>,
    ) -> Result<Json<serde_json::Value>, StatusCode> {
        if form.get("client_secret").map(String::as_str) != Some("secret") {
            return Err(StatusCode::UNAUTHORIZED);
        }
        let n = minted.fetch_add(1, Ordering::SeqCst) + 1;
        Ok(Json(serde_json::json!({
            "access_token": format!("tok-{n}"),
            "expires_in": 3600,
            "scope": form.get("scope"),
        })))
    }

    async fn fake() -> (String, Arc<AtomicUsize>) {
        let minted = Arc::new(AtomicUsize::new(0));
        let app = Router::new()
            .route("/oauth/token", post(token_endpoint))
            .with_state(minted.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("http://{addr}/oauth/token"), minted)
    }

    #[tokio::test]
    async fn a_token_is_minted_once_cached_and_reminted_after_invalidate() {
        let (url, minted) = fake().await;
        let source = ClientCredentialsTokenSource::new(url, "id", "secret", None);
        assert_eq!(source.token().await.unwrap(), "tok-1");
        assert_eq!(source.token().await.unwrap(), "tok-1");
        assert_eq!(minted.load(Ordering::SeqCst), 1);
        source.invalidate();
        assert_eq!(source.token().await.unwrap(), "tok-2");
    }

    #[tokio::test]
    async fn a_refused_mint_is_an_error_naming_the_status() {
        let (url, _) = fake().await;
        let source = ClientCredentialsTokenSource::new(url, "id", "wrong", None);
        let err = source.token().await.unwrap_err().to_string();
        assert!(err.contains("401"), "{err}");
    }
}
