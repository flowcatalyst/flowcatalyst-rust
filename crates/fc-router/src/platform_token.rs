//! The router's credential for its own platform (Go `internal/oauthtoken`).
//!
//! A deployed router fetches the platform's `GET /api/dispatch/router-config`
//! document, which is authenticated: the router's service account (the
//! built-in `platform:router` role, anchor scope) mints a
//! `client_credentials` token at `{FC_ROUTER_PLATFORM_URL}/oauth/token`.
//! [`PlatformTokenSource`] mints that token, caches it, and refreshes it 60s
//! before it expires, as Go's `oauthtoken.Manager` does. Which requests
//! carry it is decided by the caller on the URL's origin
//! ([`origin_of`]) — never positionally — so a comma-separated
//! `FLOWCATALYST_CONFIG_URL` that also lists third-party config services
//! (Integral's `/api/config`) never sees it.

use std::time::{Duration, Instant};

use serde::Deserialize;

/// How far ahead of expiry the cached token is replaced (Go
/// `tokenRefreshBuffer`): covers clock skew and in-flight request latency.
const TOKEN_REFRESH_BUFFER: Duration = Duration::from_secs(60);

/// The lifetime assumed when the token response carries no (or a zero)
/// `expires_in` — the platform's access-token TTL (Go: one hour). Without it
/// the token would count as already stale and be re-minted on every call.
const DEFAULT_TOKEN_TTL: Duration = Duration::from_secs(60 * 60);

/// Most of an error body kept for the error message (Go reads 64 KiB).
const MAX_ERROR_BODY: usize = 64 * 1024;

/// Why a token could not be minted.
#[derive(Debug, Clone, thiserror::Error)]
pub enum TokenError {
    #[error("token request: {0}")]
    Request(String),
    #[error("token endpoint returned {status}: {body}")]
    Status { status: u16, body: String },
    #[error("decode token response: {0}")]
    Decode(String),
    #[error("token endpoint returned no access_token")]
    NoAccessToken,
}

#[derive(Deserialize)]
struct TokenEndpointResponse {
    #[serde(default)]
    access_token: String,
    #[serde(default)]
    expires_in: i64,
}

struct CachedToken {
    access_token: String,
    expires_at: Instant,
}

/// Mints and caches an OAuth2 `client_credentials` access token for one
/// platform (Go `oauthtoken.Manager`). Safe for concurrent use: the cache
/// lock is held across a mint, so concurrent callers share one request.
pub struct PlatformTokenSource {
    token_url: String,
    client_id: String,
    client_secret: String,
    http: reqwest::Client,
    cached: tokio::sync::Mutex<Option<CachedToken>>,
}

impl PlatformTokenSource {
    /// A token source for the platform at `base_url` (its `/oauth/token`).
    pub fn new(
        base_url: &str,
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
        http: reqwest::Client,
    ) -> Self {
        Self {
            token_url: format!("{}/oauth/token", base_url.trim_end_matches('/')),
            client_id: client_id.into(),
            client_secret: client_secret.into(),
            http,
            cached: tokio::sync::Mutex::new(None),
        }
    }

    /// The token endpoint this source mints at.
    pub fn token_url(&self) -> &str {
        &self.token_url
    }

    /// The OAuth client id (never the secret).
    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    /// A valid bearer token (without the `Bearer ` prefix), minting a fresh
    /// one when none is cached or the cached one is within the refresh
    /// buffer of its expiry.
    pub async fn token(&self) -> Result<String, TokenError> {
        let mut cached = self.cached.lock().await;
        if let Some(tok) = cached.as_ref() {
            if Instant::now() + TOKEN_REFRESH_BUFFER < tok.expires_at {
                return Ok(tok.access_token.clone());
            }
        }
        let fresh = self.mint().await?;
        let token = fresh.access_token.clone();
        *cached = Some(fresh);
        Ok(token)
    }

    /// Drop the cached token so the next [`Self::token`] mints a new one —
    /// for a caller whose token was just rejected (401), where waiting for
    /// the refresh buffer would fail every attempt until expiry.
    pub async fn invalidate(&self) {
        *self.cached.lock().await = None;
    }

    async fn mint(&self) -> Result<CachedToken, TokenError> {
        let form = [
            ("grant_type", "client_credentials"),
            ("client_id", self.client_id.as_str()),
            ("client_secret", self.client_secret.as_str()),
        ];
        let response = self
            .http
            .post(&self.token_url)
            .header(reqwest::header::ACCEPT, "application/json")
            .form(&form)
            .send()
            .await
            .map_err(|e| TokenError::Request(e.to_string()))?;
        let status = response.status();
        let body = response
            .bytes()
            .await
            .map_err(|e| TokenError::Request(e.to_string()))?;
        let body = &body[..body.len().min(MAX_ERROR_BODY)];
        if !status.is_success() {
            return Err(TokenError::Status {
                status: status.as_u16(),
                body: String::from_utf8_lossy(body).trim().to_string(),
            });
        }
        let parsed: TokenEndpointResponse =
            serde_json::from_slice(body).map_err(|e| TokenError::Decode(e.to_string()))?;
        if parsed.access_token.is_empty() {
            return Err(TokenError::NoAccessToken);
        }
        let ttl = if parsed.expires_in > 0 {
            Duration::from_secs(parsed.expires_in as u64)
        } else {
            DEFAULT_TOKEN_TTL
        };
        Ok(CachedToken {
            access_token: parsed.access_token,
            expires_at: Instant::now() + ttl,
        })
    }
}

/// `scheme://host[:port]`, lower-cased — the unit "same platform" is
/// decided on (Go `originOf`). A path, query or trailing slash does not
/// change it; a different host, or an explicit port, does. `None` for
/// anything without a scheme and host.
pub fn origin_of(raw: &str) -> Option<String> {
    let (scheme, rest) = raw.trim().split_once("://")?;
    if scheme.is_empty() {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    if host.is_empty() {
        return None;
    }
    Some(format!("{scheme}://{host}").to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn origin_ignores_path_query_and_case_but_not_host_or_port() {
        let o = |s| origin_of(s);
        assert_eq!(
            o("http://fc-platform:8080/api/dispatch/router-config"),
            Some("http://fc-platform:8080".into())
        );
        assert_eq!(o("HTTP://FC-Platform:8080"), o("http://fc-platform:8080/"));
        assert_ne!(o("http://fc-platform:8080"), o("http://fc-platform:8081"));
        assert_ne!(
            o("http://fc-platform:8080"),
            o("https://amsa.inhanceapps.com/api/config")
        );
        assert_eq!(o("https://u:p@host/x?y"), Some("https://host".into()));
        assert_eq!(o("fc-platform:8080"), None);
        assert_eq!(o("http:///path"), None);
    }

    #[tokio::test]
    async fn mints_once_caches_and_reminted_after_invalidate() {
        let server = MockServer::start().await;
        let mints = Arc::new(AtomicU32::new(0));
        let counter = mints.clone();
        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .and(body_string_contains("grant_type=client_credentials"))
            .and(body_string_contains("client_id=rid"))
            .and(body_string_contains("client_secret=rsecret"))
            .respond_with(move |_: &wiremock::Request| {
                let n = counter.fetch_add(1, Ordering::SeqCst);
                ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "access_token": format!("tok-{n}"),
                    "token_type": "Bearer",
                    "expires_in": 3600,
                }))
            })
            .mount(&server)
            .await;

        let src = PlatformTokenSource::new(
            &format!("{}/", server.uri()),
            "rid",
            "rsecret",
            reqwest::Client::new(),
        );
        assert_eq!(src.token().await.unwrap(), "tok-0");
        assert_eq!(src.token().await.unwrap(), "tok-0", "cached");
        assert_eq!(mints.load(Ordering::SeqCst), 1);
        src.invalidate().await;
        assert_eq!(src.token().await.unwrap(), "tok-1");
    }

    #[tokio::test]
    async fn a_token_inside_the_refresh_buffer_is_replaced() {
        let server = MockServer::start().await;
        let mints = Arc::new(AtomicU32::new(0));
        let counter = mints.clone();
        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .respond_with(move |_: &wiremock::Request| {
                counter.fetch_add(1, Ordering::SeqCst);
                // 30s: already inside the 60s refresh buffer.
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"access_token": "t", "expires_in": 30}))
            })
            .mount(&server)
            .await;
        let src = PlatformTokenSource::new(&server.uri(), "a", "b", reqwest::Client::new());
        src.token().await.unwrap();
        src.token().await.unwrap();
        assert_eq!(mints.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn refusals_and_empty_tokens_are_errors() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(401).set_body_string("invalid_client"))
            .mount(&server)
            .await;
        let src = PlatformTokenSource::new(&server.uri(), "a", "b", reqwest::Client::new());
        match src.token().await {
            Err(TokenError::Status { status: 401, body }) => assert_eq!(body, "invalid_client"),
            other => panic!("expected a 401 refusal, got {other:?}"),
        }

        let empty = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&empty)
            .await;
        let src = PlatformTokenSource::new(&empty.uri(), "a", "b", reqwest::Client::new());
        assert!(matches!(src.token().await, Err(TokenError::NoAccessToken)));
    }
}
