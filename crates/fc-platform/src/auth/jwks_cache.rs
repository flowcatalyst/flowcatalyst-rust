//! JWKS Cache — Fetches and caches JSON Web Key Sets per issuer URL
//!
//! Used during OIDC callback to validate ID token signatures from external IDPs.
//! Supports automatic discovery via `.well-known/openid-configuration` and
//! manual JWKS URI resolution. Cached entries expire after a configurable TTL.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use tracing::{debug, warn};

/// Cached JWKS entry for a single issuer
struct CachedJwks {
    jwks: Jwks,
    fetched_at: DateTime<Utc>,
}

/// JWKS response
#[derive(Debug, Clone, Deserialize)]
pub struct Jwks {
    pub keys: Vec<JwkKey>,
}

/// Individual JWK key
#[derive(Debug, Clone, Deserialize)]
pub struct JwkKey {
    pub kty: String,
    #[serde(rename = "use")]
    pub key_use: Option<String>,
    pub kid: Option<String>,
    pub alg: Option<String>,
    pub n: Option<String>,
    pub e: Option<String>,
    pub x: Option<String>,
    pub y: Option<String>,
    pub crv: Option<String>,
}

/// Partial OIDC discovery document (only what we need)
#[derive(Debug, Deserialize)]
struct DiscoveryDoc {
    jwks_uri: String,
}

/// JWKS Cache with per-issuer TTL
pub struct JwksCache {
    cache: Arc<RwLock<HashMap<String, CachedJwks>>>,
    http_client: reqwest::Client,
    ttl_secs: i64,
}

impl JwksCache {
    /// Create a new JWKS cache with the given TTL (in seconds)
    pub fn new(ttl_secs: i64) -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            http_client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_default(),
            ttl_secs,
        }
    }

    /// Get JWKS for an issuer, fetching from the network if not cached or expired.
    pub async fn get_jwks(&self, issuer_url: &str) -> Result<Jwks, String> {
        // Check cache first
        {
            let cache = self.cache.read().await;
            if let Some(entry) = cache.get(issuer_url) {
                let age = (Utc::now() - entry.fetched_at).num_seconds();
                if age < self.ttl_secs {
                    debug!(issuer = %issuer_url, age_secs = age, "JWKS cache hit");
                    return Ok(entry.jwks.clone());
                }
            }
        }

        // Fetch fresh JWKS
        let jwks = self.fetch_jwks(issuer_url).await?;

        // Store in cache
        {
            let mut cache = self.cache.write().await;
            cache.insert(issuer_url.to_string(), CachedJwks {
                jwks: jwks.clone(),
                fetched_at: Utc::now(),
            });
        }

        Ok(jwks)
    }

    /// Fetch JWKS from the issuer's discovery endpoint
    async fn fetch_jwks(&self, issuer_url: &str) -> Result<Jwks, String> {
        let base = issuer_url.trim_end_matches('/');
        let discovery_url = format!("{}/.well-known/openid-configuration", base);

        debug!(url = %discovery_url, "Fetching OIDC discovery document");

        let discovery: DiscoveryDoc = self.http_client
            .get(&discovery_url)
            .send()
            .await
            .map_err(|e| format!("Failed to fetch OIDC discovery from {}: {}", discovery_url, e))?
            .json()
            .await
            .map_err(|e| format!("Failed to parse OIDC discovery: {}", e))?;

        debug!(jwks_uri = %discovery.jwks_uri, "Fetching JWKS");

        let jwks: Jwks = self.http_client
            .get(&discovery.jwks_uri)
            .send()
            .await
            .map_err(|e| format!("Failed to fetch JWKS from {}: {}", discovery.jwks_uri, e))?
            .json()
            .await
            .map_err(|e| format!("Failed to parse JWKS: {}", e))?;

        if jwks.keys.is_empty() {
            warn!(issuer = %issuer_url, "JWKS contains no keys");
        }

        debug!(issuer = %issuer_url, key_count = jwks.keys.len(), "JWKS fetched successfully");
        Ok(jwks)
    }

    /// Invalidate cached JWKS for a specific issuer (force re-fetch on next use)
    #[allow(dead_code)]
    pub async fn invalidate(&self, issuer_url: &str) {
        let mut cache = self.cache.write().await;
        cache.remove(issuer_url);
    }
}

impl Default for JwksCache {
    fn default() -> Self {
        Self::new(3600) // 1 hour default TTL
    }
}
