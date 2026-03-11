//! OAuth2 Authorization Code Flow with PKCE
//!
//! Helpers for SDK applications that authenticate users via FlowCatalyst's
//! OIDC server using the OAuth2 authorization code grant with PKCE.

use serde::{Deserialize, Serialize};
use sha2::{Sha256, Digest};
use base64::Engine;

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
}

impl OAuthClient {
    /// Create a new OAuth client.
    pub fn new(config: OAuthConfig) -> Self {
        Self {
            config,
            http: reqwest::Client::new(),
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

        let params = AuthorizeParams {
            pkce,
            state,
            nonce,
        };

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
            return Err(AuthError::TokenExchange(format!("Token exchange failed: {}", body)));
        }

        resp.json()
            .await
            .map_err(|e| AuthError::TokenExchange(format!("Failed to parse token response: {}", e)))
    }

    /// Refresh an access token using a refresh token.
    pub async fn refresh_token(&self, refresh_token: &str) -> Result<TokenResponse, AuthError> {
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
            return Err(AuthError::TokenExchange(format!("Token refresh failed: {}", body)));
        }

        resp.json()
            .await
            .map_err(|e| AuthError::TokenExchange(format!("Failed to parse token response: {}", e)))
    }

    /// Revoke a token (access or refresh).
    pub async fn revoke_token(&self, token: &str) -> Result<(), AuthError> {
        let base = self.config.issuer_url.trim_end_matches('/');
        let url = format!("{}/oauth/revoke", base);

        let mut form = vec![
            ("token", token),
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

        // Revocation always returns 200 per RFC 7009
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AuthError::TokenExchange(format!("Token revocation failed: {}", body)));
        }

        Ok(())
    }

    /// Introspect a token to check validity (RFC 7662).
    pub async fn introspect_token(
        &self,
        token: &str,
    ) -> Result<IntrospectionResponse, AuthError> {
        let base = self.config.issuer_url.trim_end_matches('/');
        let url = format!("{}/oauth/introspect", base);

        let mut form = vec![
            ("token", token),
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
            return Err(AuthError::TokenExchange(format!("Introspection failed: {}", body)));
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
            return Err(AuthError::TokenExchange(format!("UserInfo failed: {}", body)));
        }

        resp.json()
            .await
            .map_err(|e| AuthError::TokenExchange(format!("Failed to parse userinfo: {}", e)))
    }

    /// Build the RP-Initiated Logout URL.
    ///
    /// Redirect the user to this URL to end their session at FlowCatalyst.
    pub fn logout_url(&self, post_logout_redirect_uri: Option<&str>, state: Option<&str>) -> String {
        let base = self.config.issuer_url.trim_end_matches('/');
        let mut url = format!("{}/auth/oidc/session/end", base);

        let mut params = Vec::new();
        if let Some(uri) = post_logout_redirect_uri {
            params.push(format!("post_logout_redirect_uri={}", urlencoded(uri)));
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

use std::collections::HashMap;

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Generate a random URL-safe string of the given length.
fn generate_random_string(len: usize) -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..len).map(|_| rng.gen()).collect();
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
