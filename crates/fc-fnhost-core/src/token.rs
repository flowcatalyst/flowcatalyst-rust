//! The host's bearer credential (Java `fnhost/reconcile/TokenSource.java`):
//! `client_credentials` against `<platform>/oauth/token`, cached until 60 s
//! before expiry, and re-minted on demand after a 401 from the control plane
//! itself. Neither the client secret nor a minted token ever appears in a
//! log field, an error message or a `Debug` rendering.

use std::fmt;
use std::time::Duration;

use chrono::{DateTime, Utc};
use reqwest::Client;
use serde_json::Value;

use crate::clock::SharedClock;
use crate::control_plane::{ControlPlaneError, ControlPlaneErrorReason};

/// A token is re-minted once it is within this margin of its expiry.
const EXPIRY_MARGIN: chrono::Duration = chrono::Duration::seconds(60);

/// Java mints over a client with no timeout at all; a hung token endpoint
/// would then hang every reconcile. The Rust host bounds it like every
/// other control-plane call.
const MINT_TIMEOUT: Duration = Duration::from_secs(30);

pub struct TokenSource {
    client: Client,
    platform_url: String,
    client_id: String,
    client_secret: String,
    clock: SharedClock,
    /// Held across a mint, so concurrent callers mint once.
    cached: tokio::sync::Mutex<Option<(String, DateTime<Utc>)>>,
}

impl TokenSource {
    pub fn new(
        client: Client,
        platform_url: impl Into<String>,
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
        clock: SharedClock,
    ) -> Self {
        Self {
            client,
            platform_url: platform_url.into(),
            client_id: client_id.into(),
            client_secret: client_secret.into(),
            clock,
            cached: tokio::sync::Mutex::new(None),
        }
    }

    /// The cached token, minting a fresh one when there is none or it is
    /// within 60 s of expiring.
    pub async fn token(&self) -> Result<String, ControlPlaneError> {
        let mut cached = self.cached.lock().await;
        if let Some((token, expiry)) = cached.as_ref() {
            if self.clock.now() < *expiry - EXPIRY_MARGIN {
                return Ok(token.clone());
            }
        }
        self.mint(&mut cached).await
    }

    /// Forces a fresh mint: called exactly once, after the control plane
    /// answers 401 to a request that carried the cached token.
    pub async fn refresh(&self) -> Result<String, ControlPlaneError> {
        let mut cached = self.cached.lock().await;
        self.mint(&mut cached).await
    }

    async fn mint(
        &self,
        cached: &mut Option<(String, DateTime<Utc>)>,
    ) -> Result<String, ControlPlaneError> {
        tracing::debug!(platform_url = %self.platform_url, client_id = %self.client_id, "minting a control-plane token");
        let form = format!(
            "grant_type=client_credentials&client_id={}&client_secret={}",
            encode(&self.client_id),
            encode(&self.client_secret)
        );
        let request = self
            .client
            .post(format!("{}/oauth/token", self.platform_url))
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .timeout(MINT_TIMEOUT)
            .body(form);
        let response = request.send().await.map_err(|_| {
            ControlPlaneError::new(
                ControlPlaneErrorReason::Unavailable,
                "minting a token failed",
            )
        })?;
        let status = response.status();
        if status != reqwest::StatusCode::OK {
            return Err(ControlPlaneError::new(
                ControlPlaneErrorReason::Unavailable,
                format!("token endpoint returned {}", status.as_u16()),
            ));
        }
        let body: Value = response.json().await.map_err(|_| {
            ControlPlaneError::new(
                ControlPlaneErrorReason::Unavailable,
                "token endpoint returned no access_token",
            )
        })?;
        let access_token = match body.get("access_token") {
            Some(Value::String(token)) if !crate::java::is_blank(token) => token.clone(),
            _ => {
                return Err(ControlPlaneError::new(
                    ControlPlaneErrorReason::Unavailable,
                    "token endpoint returned no access_token",
                ))
            }
        };
        // Jackson's asLong(0): a number, or a numeric string, else 0.
        let expires_in = match body.get("expires_in") {
            Some(Value::Number(n)) => n
                .as_i64()
                .or_else(|| n.as_f64().map(|f| f as i64))
                .unwrap_or(0),
            Some(Value::String(s)) => s.trim().parse::<i64>().unwrap_or(0),
            _ => 0,
        };
        let expiry = self.clock.now() + chrono::Duration::seconds(expires_in.max(0));
        *cached = Some((access_token.clone(), expiry));
        tracing::debug!(client_id = %self.client_id, expires_in, "minted a control-plane token");
        Ok(access_token)
    }
}

fn encode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

impl fmt::Debug for TokenSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "TokenSource[platformUrl={}, clientId={}]",
            self.platform_url, self.client_id
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::ManualClock;
    use axum::extract::State;
    use axum::routing::post;
    use axum::Router;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    type Bodies = Arc<parking_lot::Mutex<Vec<String>>>;

    async fn serve(expires_in: i64) -> (String, Arc<AtomicUsize>, Bodies) {
        let mints = Arc::new(AtomicUsize::new(0));
        let bodies = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let state = (mints.clone(), bodies.clone());
        let app = Router::new()
            .route(
                "/oauth/token",
                post(
                    move |State((mints, bodies)): State<(Arc<AtomicUsize>, Bodies)>,
                          body: String| async move {
                        let n = mints.fetch_add(1, Ordering::SeqCst);
                        bodies.lock().push(body);
                        axum::Json(serde_json::json!({"access_token": format!("tok-{n}"), "expires_in": expires_in}))
                    },
                ),
            )
            .with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (url, mints, bodies)
    }

    #[tokio::test]
    async fn caches_until_sixty_seconds_before_expiry() {
        let (url, mints, bodies) = serve(300).await;
        let clock = ManualClock::new(Utc::now());
        let source = TokenSource::new(Client::new(), url, "id", "s3cr&t", Arc::new(clock.clone()));
        assert_eq!(source.token().await.unwrap(), "tok-0");
        assert_eq!(source.token().await.unwrap(), "tok-0");
        clock.advance(chrono::Duration::seconds(239));
        assert_eq!(source.token().await.unwrap(), "tok-0");
        clock.advance(chrono::Duration::seconds(1));
        assert_eq!(source.token().await.unwrap(), "tok-1");
        assert_eq!(mints.load(Ordering::SeqCst), 2);
        assert_eq!(
            bodies.lock()[0],
            "grant_type=client_credentials&client_id=id&client_secret=s3cr%26t"
        );
    }

    #[tokio::test]
    async fn refresh_always_mints() {
        let (url, mints, _) = serve(3600).await;
        let source = TokenSource::new(
            Client::new(),
            url,
            "id",
            "secret",
            Arc::new(crate::clock::SystemClock),
        );
        source.token().await.unwrap();
        assert_eq!(source.refresh().await.unwrap(), "tok-1");
        assert_eq!(source.token().await.unwrap(), "tok-1");
        assert_eq!(mints.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn neither_secret_nor_token_appears_in_errors_or_debug() {
        let source = TokenSource::new(
            Client::new(),
            "http://127.0.0.1:1",
            "id",
            "super-secret",
            Arc::new(crate::clock::SystemClock),
        );
        let err = source.token().await.unwrap_err();
        assert!(!err.to_string().contains("super-secret"));
        assert!(!format!("{source:?}").contains("super-secret"));
    }
}
