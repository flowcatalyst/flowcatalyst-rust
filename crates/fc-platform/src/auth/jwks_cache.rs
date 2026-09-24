//! JWKS Cache — Fetches and caches JSON Web Key Sets per issuer URL
//!
//! Used during OIDC callback to validate ID token signatures from external IDPs.
//! Supports automatic discovery via `.well-known/openid-configuration` and
//! manual JWKS URI resolution. Cached entries expire after a configurable TTL.

use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, warn};

/// Why the JWKS for an issuer could not be obtained.
#[derive(Debug, thiserror::Error)]
pub enum JwksError {
    #[error("Failed to fetch OIDC discovery from {url}: {source}")]
    DiscoveryFetch { url: String, source: reqwest::Error },
    #[error("Failed to parse OIDC discovery: {0}")]
    DiscoveryParse(#[source] reqwest::Error),
    #[error("Failed to fetch JWKS from {url}: {source}")]
    JwksFetch { url: String, source: reqwest::Error },
    #[error("Failed to parse JWKS: {0}")]
    JwksParse(#[source] reqwest::Error),
}

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
    pub async fn get_jwks(&self, issuer_url: &str) -> Result<Jwks, JwksError> {
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
            cache.insert(
                issuer_url.to_string(),
                CachedJwks {
                    jwks: jwks.clone(),
                    fetched_at: Utc::now(),
                },
            );
        }

        Ok(jwks)
    }

    /// Fetch JWKS from the issuer's discovery endpoint
    async fn fetch_jwks(&self, issuer_url: &str) -> Result<Jwks, JwksError> {
        let base = issuer_url.trim_end_matches('/');
        let discovery_url = format!("{}/.well-known/openid-configuration", base);

        debug!(url = %discovery_url, "Fetching OIDC discovery document");

        let discovery: DiscoveryDoc = self
            .http_client
            .get(&discovery_url)
            .send()
            .await
            .map_err(|source| JwksError::DiscoveryFetch {
                url: discovery_url.clone(),
                source,
            })?
            .json()
            .await
            .map_err(JwksError::DiscoveryParse)?;

        debug!(jwks_uri = %discovery.jwks_uri, "Fetching JWKS");

        let jwks: Jwks = self
            .http_client
            .get(&discovery.jwks_uri)
            .send()
            .await
            .map_err(|source| JwksError::JwksFetch {
                url: discovery.jwks_uri.clone(),
                source,
            })?
            .json()
            .await
            .map_err(JwksError::JwksParse)?;

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
        Self::new(900) // 15 minute default TTL
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_creates_cache_with_specified_ttl() {
        let cache = JwksCache::new(300);
        assert_eq!(cache.ttl_secs, 300);
    }

    #[test]
    fn test_default_uses_15_minute_ttl() {
        let cache = JwksCache::default();
        assert_eq!(cache.ttl_secs, 900);
    }

    #[tokio::test]
    async fn test_cache_starts_empty() {
        let cache = JwksCache::new(60);
        let inner = cache.cache.read().await;
        assert!(inner.is_empty());
    }

    #[tokio::test]
    async fn test_cache_key_is_issuer_url() {
        // Verify that two different issuers produce separate cache entries
        let cache = JwksCache::new(60);
        let mut inner = cache.cache.write().await;

        let jwks = Jwks { keys: vec![] };

        inner.insert(
            "https://issuer-a.example.com".to_string(),
            CachedJwks {
                jwks: jwks.clone(),
                fetched_at: Utc::now(),
            },
        );
        inner.insert(
            "https://issuer-b.example.com".to_string(),
            CachedJwks {
                jwks: jwks.clone(),
                fetched_at: Utc::now(),
            },
        );

        assert_eq!(inner.len(), 2);
        assert!(inner.contains_key("https://issuer-a.example.com"));
        assert!(inner.contains_key("https://issuer-b.example.com"));
    }

    #[tokio::test]
    async fn test_same_issuer_same_cache_key() {
        let cache = JwksCache::new(60);
        let mut inner = cache.cache.write().await;

        let jwks = Jwks { keys: vec![] };
        inner.insert(
            "https://issuer.example.com".to_string(),
            CachedJwks {
                jwks: jwks.clone(),
                fetched_at: Utc::now(),
            },
        );
        // Inserting same key overwrites
        inner.insert(
            "https://issuer.example.com".to_string(),
            CachedJwks {
                jwks,
                fetched_at: Utc::now(),
            },
        );

        assert_eq!(inner.len(), 1);
    }

    #[tokio::test]
    async fn test_invalidate_removes_entry() {
        let cache = JwksCache::new(60);
        {
            let mut inner = cache.cache.write().await;
            inner.insert(
                "https://issuer.example.com".to_string(),
                CachedJwks {
                    jwks: Jwks { keys: vec![] },
                    fetched_at: Utc::now(),
                },
            );
        }

        cache.invalidate("https://issuer.example.com").await;

        let inner = cache.cache.read().await;
        assert!(!inner.contains_key("https://issuer.example.com"));
    }

    #[tokio::test]
    async fn test_invalidate_nonexistent_is_noop() {
        let cache = JwksCache::new(60);
        // Should not panic
        cache.invalidate("https://nonexistent.example.com").await;
        let inner = cache.cache.read().await;
        assert!(inner.is_empty());
    }

    #[tokio::test]
    async fn test_expired_entry_triggers_refetch() {
        let cache = JwksCache::new(1); // 1 second TTL
        {
            let mut inner = cache.cache.write().await;
            // Insert an entry with a timestamp 10 seconds in the past
            inner.insert(
                "https://expired.example.com".to_string(),
                CachedJwks {
                    jwks: Jwks { keys: vec![] },
                    fetched_at: Utc::now() - chrono::Duration::seconds(10),
                },
            );
        }

        // get_jwks should try to refetch (will fail since no server),
        // proving the cache considered the entry expired
        let result = cache.get_jwks("https://expired.example.com").await;
        assert!(result.is_err());
    }

    #[test]
    fn test_jwk_key_deserialization_rsa() {
        let json = r#"{
            "kty": "RSA",
            "use": "sig",
            "kid": "test-key-1",
            "alg": "RS256",
            "n": "0vx7agoebGcQSuuPiLJXZptN9nndrQmbXEps2aiAFbWhM78LhWx4cbbfAAtVT86zwu1RK7aPFFxuhDR1L6tSoc_BJECPebWKRXjBZCiFV4n3oknjhMstn64tZ_2W-5JsGY4Hc5n9yBXArwl93lqt7_RN5w6Cf0h4QyQ5v-65YGjQR0_FDW2QvzqY368QQMicAtaSqzs8KJZgnYb9c7d0zgdAZHzu6qMQvRL5hajrn1n91CbOpbISD08qNLyrdkt-bFTWhAI4vMQFh6WeZu0fM4lFd2NcRwr3XPksINHaQ-G_xBniIqbw0Ls1jF44-csFCur-kEgU8awapJzKnqDKgw",
            "e": "AQAB"
        }"#;

        let key: JwkKey = serde_json::from_str(json).unwrap();
        assert_eq!(key.kty, "RSA");
        assert_eq!(key.key_use, Some("sig".to_string()));
        assert_eq!(key.kid, Some("test-key-1".to_string()));
        assert_eq!(key.alg, Some("RS256".to_string()));
        assert!(key.n.is_some());
        assert!(key.e.is_some());
        // EC fields should be None
        assert!(key.x.is_none());
        assert!(key.y.is_none());
        assert!(key.crv.is_none());
    }

    #[test]
    fn test_jwk_key_deserialization_ec() {
        let json = r#"{
            "kty": "EC",
            "use": "sig",
            "kid": "ec-key-1",
            "crv": "P-256",
            "x": "f83OJ3D2xF1Bg8vub9tLe1gHMzV76e8Tus9uPHvRVEU",
            "y": "x_FEzRu9m36HLN_tue659LNpXW6pCyStikYjKIWI5a0"
        }"#;

        let key: JwkKey = serde_json::from_str(json).unwrap();
        assert_eq!(key.kty, "EC");
        assert_eq!(key.crv, Some("P-256".to_string()));
        assert!(key.x.is_some());
        assert!(key.y.is_some());
        // RSA fields should be None
        assert!(key.n.is_none());
        assert!(key.e.is_none());
    }

    #[test]
    fn test_jwks_deserialization() {
        let json = r#"{"keys": [
            {"kty": "RSA", "kid": "k1", "n": "abc", "e": "AQAB"},
            {"kty": "EC", "kid": "k2", "crv": "P-256", "x": "x1", "y": "y1"}
        ]}"#;

        let jwks: Jwks = serde_json::from_str(json).unwrap();
        assert_eq!(jwks.keys.len(), 2);
        assert_eq!(jwks.keys[0].kty, "RSA");
        assert_eq!(jwks.keys[1].kty, "EC");
    }

    #[test]
    fn test_jwks_empty_keys() {
        let json = r#"{"keys": []}"#;
        let jwks: Jwks = serde_json::from_str(json).unwrap();
        assert!(jwks.keys.is_empty());
    }

    #[test]
    fn test_jwk_key_minimal_fields() {
        // Only kty is required by spec
        let json = r#"{"kty": "oct"}"#;
        let key: JwkKey = serde_json::from_str(json).unwrap();
        assert_eq!(key.kty, "oct");
        assert!(key.key_use.is_none());
        assert!(key.kid.is_none());
        assert!(key.alg.is_none());
    }
}
