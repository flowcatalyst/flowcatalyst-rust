//! OAuth2 Authorization Code Flow with PKCE
//!
//! Helpers for SDK applications that authenticate users via FlowCatalyst's
//! OIDC server using the OAuth2 authorization code grant with PKCE.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::AuthError;

/// OAuth2 client configuration for the authorization code flow.
///
/// # Example
///
/// ```
/// use fc_sdk::auth::OAuthConfig;
///
/// let config = OAuthConfig {
///     issuer_url: "https://auth.flowcatalyst.io".to_string(),
///     client_id: "my-app".to_string(),
///     client_secret: Some("secret".to_string()),
///     redirect_uri: "https://myapp.example.com/callback".to_string(),
///     scopes: vec!["openid".to_string(), "profile".to_string(), "email".to_string()],
/// };
/// ```
#[derive(Debug, Clone)]
pub struct OAuthConfig {
    /// FlowCatalyst OIDC server URL
    pub issuer_url: String,
    /// OAuth client ID (registered in FlowCatalyst)
    pub client_id: String,
    /// OAuth client secret (for confidential clients)
    pub client_secret: Option<String>,
    /// Your application's callback URL
    pub redirect_uri: String,
    /// Requested scopes (default: openid profile email)
    pub scopes: Vec<String>,
}

impl Default for OAuthConfig {
    fn default() -> Self {
        Self {
            issuer_url: String::new(),
            client_id: String::new(),
            client_secret: None,
            redirect_uri: String::new(),
            scopes: vec![
                "openid".to_string(),
                "profile".to_string(),
                "email".to_string(),
            ],
        }
    }
}

/// PKCE challenge pair for the authorization code flow.
#[derive(Debug, Clone)]
pub struct PkceChallenge {
    /// The code verifier (keep secret, send in token exchange)
    pub code_verifier: String,
    /// The code challenge (send in authorization request)
    pub code_challenge: String,
    /// Always "S256"
    pub code_challenge_method: String,
}

impl PkceChallenge {
    /// Generate a new PKCE challenge pair.
    pub fn generate() -> Self {
        let code_verifier = generate_random_string(64);

        let mut hasher = Sha256::new();
        hasher.update(code_verifier.as_bytes());
        let hash = hasher.finalize();
        let code_challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hash);

        Self {
            code_verifier,
            code_challenge,
            code_challenge_method: "S256".to_string(),
        }
    }
}

/// Parameters for building an authorization URL.
#[derive(Debug, Clone)]
pub struct AuthorizeParams {
    /// PKCE challenge
    pub pkce: PkceChallenge,
    /// State parameter for CSRF protection
    pub state: String,
    /// Nonce for replay protection
    pub nonce: String,
}

/// OAuth2 flow helper for the authorization code grant with PKCE.
///
/// # Example
///
/// ```ignore
/// use fc_sdk::auth::{OAuthClient, OAuthConfig};
///
/// let oauth = OAuthClient::new(OAuthConfig {
///     issuer_url: "https://auth.flowcatalyst.io".to_string(),
///     client_id: "my-app".to_string(),
///     redirect_uri: "https://myapp.example.com/callback".to_string(),
///     ..Default::default()
/// });
///
/// // 1. Generate authorization URL
/// let (url, params) = oauth.authorize_url();
/// // Redirect user to `url`, store `params` in session
///
/// // 2. Handle callback — exchange code for tokens
/// let tokens = oauth.exchange_code("auth-code", &params.pkce.code_verifier).await?;
///
/// // 3. Use access token
/// println!("Access token: {}", tokens.access_token);
///
/// // 4. Refresh when expired
/// let new_tokens = oauth.refresh_token(&tokens.refresh_token.unwrap()).await?;
/// ```
pub struct OAuthClient {
    config: OAuthConfig,
    http: reqwest::Client,
    /// Single-flight refresh: one exchange per refresh token, see
    /// [`OAuthClient::refresh_token`].
    refreshes: Mutex<HashMap<String, Arc<RefreshSlot>>>,
}

/// How long a completed refresh is reused for a caller that read the old
/// refresh token just after the exchange finished.
const REFRESH_MEMO: Duration = Duration::from_secs(10);

/// One refresh exchange, shared by every caller presenting the same refresh
/// token while it runs and for [`REFRESH_MEMO`] after it succeeds.
#[derive(Default)]
struct RefreshSlot {
    outcome: tokio::sync::OnceCell<(Result<TokenResponse, String>, Instant)>,
}

impl RefreshSlot {
    /// Settled and no longer reusable: a failure, or a success past the memo.
    fn is_spent(&self, now: Instant) -> bool {
        match self.outcome.get() {
            Some((Ok(_), at)) => now.duration_since(*at) >= REFRESH_MEMO,
            Some((Err(_), _)) => true,
            None => false,
        }
    }
}

impl OAuthClient {
    /// Create a new OAuth client.
    pub fn new(config: OAuthConfig) -> Self {
        Self {
            config,
            http: reqwest::Client::new(),
            refreshes: Mutex::new(HashMap::new()),
        }
    }

    /// Build an authorization URL with PKCE.
    ///
    /// Returns the URL to redirect the user to, and the parameters
    /// to store in the session for the callback.
    pub fn authorize_url(&self) -> (String, AuthorizeParams) {
        let pkce = PkceChallenge::generate();
        let state = generate_random_string(32);
        let nonce = generate_random_string(32);

        let scope = self.config.scopes.join(" ");
        let base = self.config.issuer_url.trim_end_matches('/');

        let url = format!(
            "{}/oauth/authorize?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&nonce={}&code_challenge={}&code_challenge_method=S256",
            base,
            urlencoded(&self.config.client_id),
            urlencoded(&self.config.redirect_uri),
            urlencoded(&scope),
            urlencoded(&state),
            urlencoded(&nonce),
            urlencoded(&pkce.code_challenge),
        );

        let params = AuthorizeParams { pkce, state, nonce };

        (url, params)
    }

    /// Exchange an authorization code for tokens.
    ///
    /// Call this in your callback handler after the user is redirected back.
    pub async fn exchange_code(
        &self,
        code: &str,
        code_verifier: &str,
    ) -> Result<TokenResponse, AuthError> {
        let base = self.config.issuer_url.trim_end_matches('/');
        let url = format!("{}/oauth/token", base);

        let mut form = vec![
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", &self.config.redirect_uri),
            ("client_id", &self.config.client_id),
            ("code_verifier", code_verifier),
        ];

        let secret_ref;
        if let Some(ref secret) = self.config.client_secret {
            secret_ref = secret.clone();
            form.push(("client_secret", &secret_ref));
        }

        let resp = self
            .http
            .post(&url)
            .form(&form)
            .send()
            .await
            .map_err(|e| AuthError::TokenExchange(format!("HTTP error: {}", e)))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AuthError::TokenExchange(format!(
                "Token exchange failed: {}",
                body
            )));
        }

        resp.json()
            .await
            .map_err(|e| AuthError::TokenExchange(format!("Failed to parse token response: {}", e)))
    }

    /// Refresh an access token using a refresh token.
    ///
    /// Single-flight (owner ruling 5 of 2026-09-25). The platform rotates
    /// refresh tokens and revokes the whole family when a rotated-out token is
    /// presented again (beyond a 10 s leeway kept for exactly this race), so
    /// concurrent requests of one session that each see the access token
    /// expire must not each spend it. Callers presenting the same refresh
    /// token join one in-flight exchange, and a successful result is reused
    /// for 10 s by a caller that read the old token just after. A failure is
    /// shared with the callers that joined it, never remembered. Per
    /// `OAuthClient` (share one per process): instances behind a load
    /// balancer still rely on the platform's leeway.
    pub async fn refresh_token(&self, refresh_token: &str) -> Result<TokenResponse, AuthError> {
        let slot = {
            let mut slots = self.refreshes.lock().unwrap_or_else(|e| e.into_inner());
            let now = Instant::now();
            slots.retain(|_, slot| !slot.is_spent(now));
            slots.entry(refresh_token.to_string()).or_default().clone()
        };
        let (outcome, _) = slot
            .outcome
            .get_or_init(|| async {
                (
                    self.exchange_refresh_token(refresh_token)
                        .await
                        .map_err(|e| match e {
                            AuthError::TokenExchange(message) => message,
                            other => other.to_string(),
                        }),
                    Instant::now(),
                )
            })
            .await;
        match outcome {
            Ok(tokens) => Ok(tokens.clone()),
            Err(message) => {
                let mut slots = self.refreshes.lock().unwrap_or_else(|e| e.into_inner());
                if slots
                    .get(refresh_token)
                    .is_some_and(|current| Arc::ptr_eq(current, &slot))
                {
                    slots.remove(refresh_token);
                }
                Err(AuthError::TokenExchange(message.clone()))
            }
        }
    }

    /// One `refresh_token` grant against `/oauth/token`.
    async fn exchange_refresh_token(
        &self,
        refresh_token: &str,
    ) -> Result<TokenResponse, AuthError> {
        let base = self.config.issuer_url.trim_end_matches('/');
        let url = format!("{}/oauth/token", base);

        let mut form = vec![
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", &self.config.client_id),
        ];

        let secret_ref;
        if let Some(ref secret) = self.config.client_secret {
            secret_ref = secret.clone();
            form.push(("client_secret", &secret_ref));
        }

        let resp = self
            .http
            .post(&url)
            .form(&form)
            .send()
            .await
            .map_err(|e| AuthError::TokenExchange(format!("HTTP error: {}", e)))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AuthError::TokenExchange(format!(
                "Token refresh failed: {}",
                body
            )));
        }

        resp.json()
            .await
            .map_err(|e| AuthError::TokenExchange(format!("Failed to parse token response: {}", e)))
    }

    /// Revoke a token (access or refresh).
    pub async fn revoke_token(&self, token: &str) -> Result<(), AuthError> {
        let base = self.config.issuer_url.trim_end_matches('/');
        let url = format!("{}/oauth/revoke", base);

        let mut form = vec![("token", token), ("client_id", &self.config.client_id)];

        let secret_ref;
        if let Some(ref secret) = self.config.client_secret {
            secret_ref = secret.clone();
            form.push(("client_secret", &secret_ref));
        }

        let resp = self
            .http
            .post(&url)
            .form(&form)
            .send()
            .await
            .map_err(|e| AuthError::TokenExchange(format!("HTTP error: {}", e)))?;

        // Revocation always returns 200 per RFC 7009
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AuthError::TokenExchange(format!(
                "Token revocation failed: {}",
                body
            )));
        }

        Ok(())
    }

    /// Introspect a token to check validity (RFC 7662).
    pub async fn introspect_token(&self, token: &str) -> Result<IntrospectionResponse, AuthError> {
        let base = self.config.issuer_url.trim_end_matches('/');
        let url = format!("{}/oauth/introspect", base);

        let mut form = vec![("token", token), ("client_id", &self.config.client_id)];

        let secret_ref;
        if let Some(ref secret) = self.config.client_secret {
            secret_ref = secret.clone();
            form.push(("client_secret", &secret_ref));
        }

        let resp = self
            .http
            .post(&url)
            .form(&form)
            .send()
            .await
            .map_err(|e| AuthError::TokenExchange(format!("HTTP error: {}", e)))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AuthError::TokenExchange(format!(
                "Introspection failed: {}",
                body
            )));
        }

        resp.json()
            .await
            .map_err(|e| AuthError::TokenExchange(format!("Failed to parse introspection: {}", e)))
    }

    /// Fetch user info from the `/oauth/userinfo` endpoint.
    pub async fn userinfo(&self, access_token: &str) -> Result<UserInfoResponse, AuthError> {
        let base = self.config.issuer_url.trim_end_matches('/');
        let url = format!("{}/oauth/userinfo", base);

        let resp = self
            .http
            .get(&url)
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|e| AuthError::TokenExchange(format!("HTTP error: {}", e)))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AuthError::TokenExchange(format!(
                "UserInfo failed: {}",
                body
            )));
        }

        resp.json()
            .await
            .map_err(|e| AuthError::TokenExchange(format!("Failed to parse userinfo: {}", e)))
    }

    /// Build the RP-Initiated Logout URL.
    ///
    /// Redirect the user to this URL to end their session at FlowCatalyst.
    ///
    /// When `post_logout_redirect_uri` is set, you must also pass the
    /// user's `id_token` as `id_token_hint` — FlowCatalyst uses its `aud`
    /// claim to identify the client and verify the URI against that
    /// client's registered `postLogoutRedirectUris` (OIDC RP-Initiated
    /// Logout 1.0 §2). Omitting the hint causes the OP to refuse the
    /// redirect.
    pub fn logout_url(
        &self,
        post_logout_redirect_uri: Option<&str>,
        id_token_hint: Option<&str>,
        state: Option<&str>,
    ) -> String {
        let base = self.config.issuer_url.trim_end_matches('/');
        let mut url = format!("{}/auth/oidc/session/end", base);

        let mut params = Vec::new();
        if let Some(uri) = post_logout_redirect_uri {
            params.push(format!("post_logout_redirect_uri={}", urlencoded(uri)));
        }
        if let Some(hint) = id_token_hint {
            params.push(format!("id_token_hint={}", urlencoded(hint)));
        }
        if let Some(s) = state {
            params.push(format!("state={}", urlencoded(s)));
        }

        if !params.is_empty() {
            url.push('?');
            url.push_str(&params.join("&"));
        }

        url
    }
}

// ─── Response Types ──────────────────────────────────────────────────────────

/// Token response from the `/oauth/token` endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

/// Token introspection response (RFC 7662).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntrospectionResponse {
    pub active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exp: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iat: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub: Option<String>,
}

/// UserInfo response from the `/oauth/userinfo` endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfoResponse {
    pub sub: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email_verified: Option<bool>,
    /// Additional claims (catch-all)
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Generate a random URL-safe string of the given length.
fn generate_random_string(len: usize) -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    let bytes: Vec<u8> = (0..len).map(|_| rng.random()).collect();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&bytes)
}

/// Percent-encode a string for URL query parameters.
fn urlencoded(s: &str) -> String {
    // Minimal encoding for query parameters
    s.replace('%', "%25")
        .replace(' ', "%20")
        .replace('&', "%26")
        .replace('=', "%3D")
        .replace('+', "%2B")
        .replace('#', "%23")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── OAuthConfig ────────────────────────────────────────────────────

    #[test]
    fn oauth_config_default_scopes() {
        let config = OAuthConfig::default();
        assert_eq!(config.scopes, vec!["openid", "profile", "email"]);
        assert!(config.issuer_url.is_empty());
        assert!(config.client_id.is_empty());
        assert!(config.client_secret.is_none());
        assert!(config.redirect_uri.is_empty());
    }

    // ─── PkceChallenge ──────────────────────────────────────────────────

    #[test]
    fn pkce_challenge_generates_values() {
        let pkce = PkceChallenge::generate();

        assert!(!pkce.code_verifier.is_empty());
        assert!(!pkce.code_challenge.is_empty());
        assert_eq!(pkce.code_challenge_method, "S256");
        // Verifier and challenge should be different
        assert_ne!(pkce.code_verifier, pkce.code_challenge);
    }

    #[test]
    fn pkce_challenge_is_sha256_of_verifier() {
        let pkce = PkceChallenge::generate();

        // Recompute the challenge from the verifier
        let mut hasher = sha2::Sha256::new();
        sha2::Digest::update(&mut hasher, pkce.code_verifier.as_bytes());
        let hash = hasher.finalize();
        let expected = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hash);

        assert_eq!(pkce.code_challenge, expected);
    }

    #[test]
    fn pkce_challenges_are_unique() {
        let a = PkceChallenge::generate();
        let b = PkceChallenge::generate();
        assert_ne!(a.code_verifier, b.code_verifier);
        assert_ne!(a.code_challenge, b.code_challenge);
    }

    // ─── OAuthClient::authorize_url ─────────────────────────────────────

    #[test]
    fn authorize_url_structure() {
        let client = OAuthClient::new(OAuthConfig {
            issuer_url: "https://auth.example.com".to_string(),
            client_id: "my-app".to_string(),
            client_secret: None,
            redirect_uri: "https://myapp.com/callback".to_string(),
            scopes: vec!["openid".to_string(), "profile".to_string()],
        });

        let (url, params) = client.authorize_url();

        assert!(url.starts_with("https://auth.example.com/oauth/authorize?"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("client_id=my-app"));
        assert!(url.contains("redirect_uri="));
        assert!(url.contains("scope=openid%20profile"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains(&format!(
            "code_challenge={}",
            urlencoded(&params.pkce.code_challenge)
        )));
        assert!(url.contains(&format!("state={}", urlencoded(&params.state))));
        assert!(url.contains(&format!("nonce={}", urlencoded(&params.nonce))));

        // Params are populated
        assert!(!params.state.is_empty());
        assert!(!params.nonce.is_empty());
        assert_eq!(params.pkce.code_challenge_method, "S256");
    }

    #[test]
    fn authorize_url_strips_trailing_slash() {
        let client = OAuthClient::new(OAuthConfig {
            issuer_url: "https://auth.example.com/".to_string(),
            client_id: "app".to_string(),
            redirect_uri: "https://cb.com".to_string(),
            ..OAuthConfig::default()
        });

        let (url, _) = client.authorize_url();
        assert!(url.starts_with("https://auth.example.com/oauth/authorize?"));
        assert!(!url.contains("//oauth"));
    }

    #[test]
    fn authorize_url_unique_per_call() {
        let client = OAuthClient::new(OAuthConfig {
            issuer_url: "https://auth.example.com".to_string(),
            client_id: "app".to_string(),
            redirect_uri: "https://cb.com".to_string(),
            ..OAuthConfig::default()
        });

        let (url1, params1) = client.authorize_url();
        let (url2, params2) = client.authorize_url();

        assert_ne!(url1, url2);
        assert_ne!(params1.state, params2.state);
        assert_ne!(params1.nonce, params2.nonce);
        assert_ne!(params1.pkce.code_verifier, params2.pkce.code_verifier);
    }

    // ─── OAuthClient::logout_url ────────────────────────────────────────

    #[test]
    fn logout_url_no_params() {
        let client = OAuthClient::new(OAuthConfig {
            issuer_url: "https://auth.example.com".to_string(),
            client_id: "app".to_string(),
            ..OAuthConfig::default()
        });

        let url = client.logout_url(None, None, None);
        assert_eq!(url, "https://auth.example.com/auth/oidc/session/end");
    }

    #[test]
    fn logout_url_with_redirect_and_hint() {
        let client = OAuthClient::new(OAuthConfig {
            issuer_url: "https://auth.example.com".to_string(),
            client_id: "app".to_string(),
            ..OAuthConfig::default()
        });

        let url = client.logout_url(Some("https://myapp.com"), Some("eyJ.hint.sig"), None);
        assert!(url.contains("post_logout_redirect_uri="));
        assert!(url.contains("myapp.com"));
        assert!(url.contains("id_token_hint="));
    }

    #[test]
    fn logout_url_with_state() {
        let client = OAuthClient::new(OAuthConfig {
            issuer_url: "https://auth.example.com".to_string(),
            client_id: "app".to_string(),
            ..OAuthConfig::default()
        });

        let url = client.logout_url(None, None, Some("my-state"));
        assert!(url.contains("state=my-state"));
    }

    #[test]
    fn logout_url_with_all_params() {
        let client = OAuthClient::new(OAuthConfig {
            issuer_url: "https://auth.example.com/".to_string(),
            client_id: "app".to_string(),
            ..OAuthConfig::default()
        });

        let url = client.logout_url(Some("https://myapp.com"), Some("eyJ.hint.sig"), Some("s1"));
        assert!(url.contains("post_logout_redirect_uri="));
        assert!(url.contains("id_token_hint="));
        assert!(url.contains("state=s1"));
        assert!(url.contains('&'));
    }

    // ─── single-flight refresh ──────────────────────────────────────────

    /// A token endpoint that counts requests and answers each after 50 ms
    /// with tokens numbered by the request (`at-N`, `rt-N`).
    async fn counting_token_endpoint() -> (String, Arc<std::sync::atomic::AtomicUsize>) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let served = count.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let served = served.clone();
                tokio::spawn(async move {
                    let mut request = Vec::new();
                    let mut buf = [0u8; 4096];
                    // Read the head, then the form body its Content-Length names.
                    loop {
                        let n = socket.read(&mut buf).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        request.extend_from_slice(&buf[..n]);
                        let text = String::from_utf8_lossy(&request).to_string();
                        if let Some(head_end) = text.find("\r\n\r\n") {
                            let length = text[..head_end]
                                .lines()
                                .find_map(|l| {
                                    let (k, v) = l.split_once(':')?;
                                    k.eq_ignore_ascii_case("content-length")
                                        .then(|| v.trim().parse::<usize>().ok())?
                                })
                                .unwrap_or(0);
                            if request.len() >= head_end + 4 + length {
                                break;
                            }
                        }
                    }
                    let n = served.fetch_add(1, Ordering::SeqCst) + 1;
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    let body = format!(
                        r#"{{"access_token":"at-{n}","token_type":"Bearer","expires_in":3600,"refresh_token":"rt-{n}"}}"#
                    );
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        });
        (format!("http://{addr}"), count)
    }

    #[tokio::test]
    async fn concurrent_and_just_after_refreshes_of_one_token_make_one_exchange() {
        use std::sync::atomic::Ordering;

        let (issuer_url, requests) = counting_token_endpoint().await;
        let client = OAuthClient::new(OAuthConfig {
            issuer_url,
            client_id: "app".to_string(),
            client_secret: Some("s".to_string()),
            ..OAuthConfig::default()
        });

        let (a, b) = tokio::join!(
            client.refresh_token("rt-old"),
            client.refresh_token("rt-old")
        );
        let (a, b) = (a.unwrap(), b.unwrap());
        assert_eq!(
            requests.load(Ordering::SeqCst),
            1,
            "joined the in-flight exchange"
        );
        assert_eq!(a.access_token, b.access_token);

        let c = client.refresh_token("rt-old").await.unwrap();
        assert_eq!(
            requests.load(Ordering::SeqCst),
            1,
            "reused the result just after"
        );
        assert_eq!(c.refresh_token, a.refresh_token);

        client.refresh_token("rt-other").await.unwrap();
        assert_eq!(
            requests.load(Ordering::SeqCst),
            2,
            "a different refresh token is its own exchange"
        );
    }

    #[tokio::test]
    async fn a_failed_refresh_is_not_remembered() {
        // Nothing listens on port 1: the exchange fails at connect.
        let client = OAuthClient::new(OAuthConfig {
            issuer_url: "http://127.0.0.1:1".to_string(),
            client_id: "app".to_string(),
            ..OAuthConfig::default()
        });
        assert!(client.refresh_token("rt").await.is_err());
        assert!(
            client.refreshes.lock().unwrap().is_empty(),
            "a failure must not be reused by the next caller"
        );
    }

    // ─── urlencoded helper ──────────────────────────────────────────────

    #[test]
    fn urlencoded_encodes_special_chars() {
        assert_eq!(urlencoded("hello world"), "hello%20world");
        assert_eq!(urlencoded("a&b"), "a%26b");
        assert_eq!(urlencoded("a=b"), "a%3Db");
        assert_eq!(urlencoded("a+b"), "a%2Bb");
        assert_eq!(urlencoded("a#b"), "a%23b");
        assert_eq!(urlencoded("100%"), "100%25");
    }

    #[test]
    fn urlencoded_passthrough_safe_chars() {
        assert_eq!(urlencoded("hello"), "hello");
        assert_eq!(urlencoded("abc123"), "abc123");
        assert_eq!(urlencoded("a-b_c.d~e"), "a-b_c.d~e");
    }

    // ─── Response DTOs serialization ────────────────────────────────────

    #[test]
    fn token_response_deserialization() {
        let json = r#"{
            "access_token": "eyJ...",
            "token_type": "Bearer",
            "expires_in": 3600,
            "refresh_token": "ref_123",
            "id_token": "id_456",
            "scope": "openid profile"
        }"#;
        let resp: TokenResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.access_token, "eyJ...");
        assert_eq!(resp.token_type, "Bearer");
        assert_eq!(resp.expires_in, 3600);
        assert_eq!(resp.refresh_token.as_deref(), Some("ref_123"));
        assert_eq!(resp.id_token.as_deref(), Some("id_456"));
        assert_eq!(resp.scope.as_deref(), Some("openid profile"));
    }

    #[test]
    fn token_response_minimal() {
        let json = r#"{
            "access_token": "tok",
            "token_type": "Bearer",
            "expires_in": 60
        }"#;
        let resp: TokenResponse = serde_json::from_str(json).unwrap();
        assert!(resp.refresh_token.is_none());
        assert!(resp.id_token.is_none());
        assert!(resp.scope.is_none());
    }

    #[test]
    fn token_response_skip_serializing_none() {
        let resp = TokenResponse {
            access_token: "tok".into(),
            token_type: "Bearer".into(),
            expires_in: 60,
            refresh_token: None,
            id_token: None,
            scope: None,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json.get("refresh_token").is_none());
        assert!(json.get("id_token").is_none());
        assert!(json.get("scope").is_none());
    }

    #[test]
    fn introspection_response_active() {
        let json = r#"{
            "active": true,
            "scope": "openid",
            "client_id": "my-app",
            "username": "user@example.com",
            "token_type": "access_token",
            "exp": 9999999999,
            "iat": 1000000000,
            "sub": "prn_123"
        }"#;
        let resp: IntrospectionResponse = serde_json::from_str(json).unwrap();
        assert!(resp.active);
        assert_eq!(resp.sub.as_deref(), Some("prn_123"));
        assert_eq!(resp.client_id.as_deref(), Some("my-app"));
    }

    #[test]
    fn introspection_response_inactive() {
        let json = r#"{"active": false}"#;
        let resp: IntrospectionResponse = serde_json::from_str(json).unwrap();
        assert!(!resp.active);
        assert!(resp.scope.is_none());
        assert!(resp.sub.is_none());
    }

    #[test]
    fn userinfo_response_with_extra_claims() {
        let json = r#"{
            "sub": "prn_123",
            "name": "Alice",
            "email": "alice@example.com",
            "email_verified": true,
            "custom_claim": "custom_value",
            "org_id": 42
        }"#;
        let resp: UserInfoResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.sub, "prn_123");
        assert_eq!(resp.name.as_deref(), Some("Alice"));
        assert_eq!(resp.email.as_deref(), Some("alice@example.com"));
        assert_eq!(resp.email_verified, Some(true));
        assert_eq!(resp.extra["custom_claim"], "custom_value");
        assert_eq!(resp.extra["org_id"], 42);
    }

    #[test]
    fn userinfo_response_minimal() {
        let json = r#"{"sub": "prn_456"}"#;
        let resp: UserInfoResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.sub, "prn_456");
        assert!(resp.name.is_none());
        assert!(resp.email.is_none());
        assert!(resp.email_verified.is_none());
        assert!(resp.extra.is_empty());
    }

    // ─── generate_random_string ─────────────────────────────────────────

    #[test]
    fn random_strings_are_nonempty_and_unique() {
        let a = generate_random_string(32);
        let b = generate_random_string(32);
        assert!(!a.is_empty());
        assert!(!b.is_empty());
        assert_ne!(a, b);
    }

    #[test]
    fn random_string_length_scales() {
        // base64 encoding of N bytes yields ceil(N*4/3) characters (no padding)
        let short = generate_random_string(8);
        let long = generate_random_string(64);
        assert!(long.len() > short.len());
    }
}
