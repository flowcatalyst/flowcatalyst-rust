//! The platform's signing keys (Java `fnhost/http/JwksKeySource.java`).
//!
//! The issuer comes from `<platformUrl>/.well-known/openid-configuration`,
//! never from `platformUrl` itself: that is how the host reaches the
//! platform (a Service Connect alias, a loopback address), not what a
//! token's `iss` carries. The discovered `jwks_uri` is followed only when it
//! is on the same origin as `platformUrl`; otherwise keys come from
//! `<platformUrl>/.well-known/jwks.json`. Until discovery succeeds every
//! `platform` call is refused.
//!
//! An unknown `kid` refetches at most once per 30 s: a flood of bad tokens
//! must not become a flood of JWKS requests. A failed fetch keeps the keys
//! already held.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use base64::Engine;
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use rsa::{BigUint, RsaPublicKey};
use serde_json::Value;

use crate::clock::SharedClock;
use crate::java;

pub const REFETCH_FLOOR: chrono::Duration = chrono::Duration::seconds(30);
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// `Base64.getUrlDecoder()`: the URL alphabet, padding optional.
pub(crate) const BASE64_URL: GeneralPurpose = GeneralPurpose::new(
    &base64::alphabet::URL_SAFE,
    GeneralPurposeConfig::new()
        .with_decode_padding_mode(DecodePaddingMode::Indifferent)
        .with_decode_allow_trailing_bits(true),
);

#[derive(Default)]
struct Discovered {
    issuer: Option<String>,
    jwks_url: Option<String>,
}

pub struct JwksKeySource {
    http: reqwest::Client,
    platform_url: String,
    clock: SharedClock,
    keys: RwLock<HashMap<String, RsaPublicKey>>,
    discovered: RwLock<Discovered>,
    /// Serialises fetches; holds the time of the last one.
    last_fetch: tokio::sync::Mutex<Option<DateTime<Utc>>>,
    fetch_count: AtomicUsize,
}

impl JwksKeySource {
    pub fn new(http: reqwest::Client, platform_url: impl Into<String>, clock: SharedClock) -> Self {
        Self {
            http,
            platform_url: platform_url.into(),
            clock,
            keys: RwLock::new(HashMap::new()),
            discovered: RwLock::new(Discovered::default()),
            last_fetch: tokio::sync::Mutex::new(None),
            fetch_count: AtomicUsize::new(0),
        }
    }

    /// Every key held.
    pub fn keys(&self) -> Vec<RsaPublicKey> {
        self.keys.read().values().cloned().collect()
    }

    /// The discovered issuer, `None` until discovery has succeeded once.
    pub fn issuer(&self) -> Option<String> {
        self.discovered.read().issuer.clone()
    }

    /// How many JWKS fetches actually happened (discovery not counted).
    pub fn fetch_count(&self) -> usize {
        self.fetch_count.load(Ordering::SeqCst)
    }

    /// Refetches, subject to the 30 s floor, only when `kid` is not held;
    /// a missing `kid` counts as unknown. Whether it is held afterwards.
    pub async fn ensure_known(&self, kid: Option<&str>) -> bool {
        let held = |kid: Option<&str>| kid.is_some_and(|k| self.keys.read().contains_key(k));
        if held(kid) {
            return true;
        }
        let mut last_fetch = self.last_fetch.lock().await;
        if held(kid) {
            return true;
        }
        let now = self.clock.now();
        if last_fetch.is_some_and(|last| now - last < REFETCH_FLOOR) {
            return false;
        }
        *last_fetch = Some(now);
        self.fetch().await;
        held(kid)
    }

    async fn fetch(&self) {
        if self.issuer().is_none() && !self.discover().await {
            return;
        }
        let Some(url) = self.discovered.read().jwks_url.clone() else {
            return;
        };
        let response = match self.http.get(&url).timeout(FETCH_TIMEOUT).send().await {
            Ok(response) => response,
            Err(e) => {
                tracing::debug!(err = %e, url = %url, "JWKS fetch failed; keeping the keys already held");
                return;
            }
        };
        self.fetch_count.fetch_add(1, Ordering::SeqCst);
        if response.status() != reqwest::StatusCode::OK {
            return;
        }
        let Ok(body) = response.text().await else {
            return;
        };
        if let Some(keys) = parse_jwks(&body) {
            *self.keys.write() = keys;
        }
    }

    async fn discover(&self) -> bool {
        let url = format!("{}/.well-known/openid-configuration", self.platform_url);
        let response = match self.http.get(&url).timeout(FETCH_TIMEOUT).send().await {
            Ok(response) => response,
            Err(e) => {
                tracing::warn!(platform_url = %self.platform_url, err = %e,
                    "could not discover the platform's issuer via /.well-known/openid-configuration; bearer auth will reject platform tokens until this succeeds");
                return false;
            }
        };
        if response.status() != reqwest::StatusCode::OK {
            tracing::warn!(platform_url = %self.platform_url, status = response.status().as_u16(),
                "could not discover the platform's issuer: unexpected status");
            return false;
        }
        let root: Value = match response
            .text()
            .await
            .ok()
            .and_then(|b| serde_json::from_str(&b).ok())
        {
            Some(root) => root,
            None => {
                tracing::warn!(platform_url = %self.platform_url,
                    "could not discover the platform's issuer: the discovery document is not JSON");
                return false;
            }
        };
        let issuer = match java::as_string(root.get("issuer"), None) {
            Some(issuer) if !java::is_blank(&issuer) => issuer,
            _ => {
                tracing::warn!(platform_url = %self.platform_url, "platform discovery document has no issuer");
                return false;
            }
        };
        let default_jwks = format!("{}/.well-known/jwks.json", self.platform_url);
        let jwks_url = match java::as_string(root.get("jwks_uri"), None) {
            Some(uri) if same_origin(&uri, &self.platform_url) => uri,
            _ => default_jwks,
        };
        *self.discovered.write() = Discovered {
            issuer: Some(issuer),
            jwks_url: Some(jwks_url),
        };
        true
    }
}

/// Scheme, host and port (defaulted by scheme) all equal.
fn same_origin(a: &str, b: &str) -> bool {
    let origin = |raw: &str| {
        let url = url::Url::parse(raw).ok()?;
        let port = url
            .port()
            .unwrap_or(if url.scheme().eq_ignore_ascii_case("https") {
                443
            } else {
                80
            });
        Some((url.scheme().to_owned(), url.host_str()?.to_owned(), port))
    };
    matches!((origin(a), origin(b)), (Some(x), Some(y)) if x == y)
}

/// `None` when the document is not JSON; an unreadable key is dropped.
fn parse_jwks(body: &str) -> Option<HashMap<String, RsaPublicKey>> {
    let root: Value = serde_json::from_str(body).ok()?;
    let mut keys = HashMap::new();
    for key in java::elements(root.get("keys")) {
        let field = |name: &str| java::as_string(key.get(name), None);
        let (Some(kid), Some(n), Some(e)) = (field("kid"), field("n"), field("e")) else {
            continue;
        };
        if let Some(public) = rsa_key(&n, &e) {
            keys.insert(kid, public);
        }
    }
    Some(keys)
}

fn rsa_key(n: &str, e: &str) -> Option<RsaPublicKey> {
    let n = BigUint::from_bytes_be(&BASE64_URL.decode(n).ok()?);
    let e = BigUint::from_bytes_be(&BASE64_URL.decode(e).ok()?);
    RsaPublicKey::new(n, e).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_origin_defaults_ports_by_scheme() {
        assert!(same_origin("http://p:80/x", "http://p"));
        assert!(same_origin(
            "https://p/.well-known/jwks.json",
            "https://p:443"
        ));
        assert!(!same_origin("https://p/x", "http://p"));
        assert!(!same_origin("http://q/x", "http://p"));
        assert!(!same_origin("http://p:81/x", "http://p"));
        assert!(!same_origin("not a url", "http://p"));
    }

    #[test]
    fn an_unreadable_key_is_dropped_not_fatal() {
        let keys = parse_jwks(
            r#"{"keys":[{"kid":"a","n":"!!","e":"AQAB"},{"kid":"b"},{"n":"x","e":"AQAB"}]}"#,
        )
        .unwrap();
        assert!(keys.is_empty());
        assert!(parse_jwks("nope").is_none());
    }
}
