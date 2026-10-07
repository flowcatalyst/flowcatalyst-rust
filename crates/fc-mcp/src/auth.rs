use chrono::{DateTime, Duration, Utc};
use reqwest::Client;
use serde::Deserialize;
use tokio::sync::Mutex;

use crate::config::Config;
use crate::error::{Error, Result};

/// 60-second buffer before the actual expiry — covers clock skew and avoids
/// shipping a token that the platform will reject mid-request.
const REFRESH_BUFFER: Duration = Duration::seconds(60);

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: i64,
}

#[derive(Clone)]
struct CachedToken {
    token: String,
    expires_at: DateTime<Utc>,
}

pub struct TokenManager {
    http: Client,
    token_url: String,
    client_id: String,
    client_secret: String,
    cached: Mutex<Option<CachedToken>>,
}

impl TokenManager {
    pub fn new(config: &Config, http: Client) -> Self {
        Self {
            http,
            token_url: format!("{}/oauth/token", config.base_url),
            client_id: config.client_id.clone(),
            client_secret: config.client_secret.clone(),
            cached: Mutex::new(None),
        }
    }

    pub async fn get_access_token(&self) -> Result<String> {
        let mut guard = self.cached.lock().await;
        if let Some(cached) = guard.as_ref() {
            if cached.expires_at > Utc::now() + REFRESH_BUFFER {
                return Ok(cached.token.clone());
            }
        }

        let fresh = self.fetch().await?;
        let token = fresh.token.clone();
        *guard = Some(fresh);
        Ok(token)
    }

    async fn fetch(&self) -> Result<CachedToken> {
        let resp = self
            .http
            .post(&self.token_url)
            .form(&[
                ("grant_type", "client_credentials"),
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.as_str()),
            ])
            .send()
            .await
            .map_err(Error::TokenRequest)?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::TokenRejected { status, body });
        }

        let parsed: TokenResponse = resp.json().await?;
        Ok(CachedToken {
            token: parsed.access_token,
            expires_at: Utc::now() + Duration::seconds(parsed.expires_in),
        })
    }
}
