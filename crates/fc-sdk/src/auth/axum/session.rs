//! Session payload + pluggable storage backends.
//!
//! Default backend is [`CookieSessionStore`] — the encrypted cookie IS the
//! session, no infra required. Apps that need server-side revocation or
//! large `session_data` can implement the [`SessionStore`] trait against
//! their own store (Postgres, Redis, etc.). The cache backends in
//! `fc_sdk::cache` are a useful starting point.

use std::sync::Arc;

use async_trait::async_trait;
use axum::http::header::{HeaderMap, SET_COOKIE};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use serde::{Deserialize, Serialize};

use crate::auth::AuthError;

use super::crypto::SessionCrypto;
use super::principal::AuthMechanism;

/// Pruned snapshot of the principal carried inside a session. Identical
/// shape to what [`Principal`](super::Principal) is rebuilt from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrincipalSnapshot {
    pub id: String,
    pub principal_type: String,
    /// Tenancy tier. Absent from snapshots written before it existed, whose
    /// `scope` then holds the tier (the claims' legacy fallback reads it).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tier: String,
    /// The token's space-delimited granted permissions.
    pub scope: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    pub clients: Vec<String>,
    pub roles: Vec<String>,
    #[serde(default)]
    pub applications: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub all_applications: bool,
}

/// Tokens persisted in the session so the plugin can transparently refresh
/// access tokens mid-session without re-prompting the user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTokens {
    pub access_token: String,
    /// Unix-milliseconds when `access_token` expires.
    pub access_token_expires_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
}

/// The full session payload stored by a [`SessionStore`].
///
/// `session_data` is the consumer app's bag — arbitrary JSON the app can
/// stash per-user state in. Keep it small; the default cookie backend is
/// capped by browser cookie limits (~4KB).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionPayload {
    pub principal: PrincipalSnapshot,
    pub tokens: SessionTokens,
    #[serde(default)]
    pub session_data: serde_json::Value,
    /// Unix-milliseconds when the session itself expires.
    pub expires_at: i64,
    /// Mechanism this session was established under (always `Session`,
    /// kept here for symmetry with the TS payload).
    #[serde(default = "default_mechanism")]
    pub mechanism: AuthMechanism,
}

impl Serialize for AuthMechanism {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(match self {
            AuthMechanism::Bearer => "bearer",
            AuthMechanism::Session => "session",
        })
    }
}

impl<'de> Deserialize<'de> for AuthMechanism {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        match s.as_str() {
            "bearer" => Ok(AuthMechanism::Bearer),
            "session" => Ok(AuthMechanism::Session),
            other => Err(serde::de::Error::custom(format!(
                "unknown auth mechanism {other:?}"
            ))),
        }
    }
}

fn default_mechanism() -> AuthMechanism {
    AuthMechanism::Session
}

/// Cookie attributes applied when a session store writes its cookie.
#[derive(Debug, Clone)]
pub struct CookieAttrs {
    pub name: String,
    pub path: String,
    pub domain: Option<String>,
    pub http_only: bool,
    pub secure: bool,
    pub same_site: SameSite,
    /// Max-age in seconds.
    pub max_age_secs: i64,
}

impl Default for CookieAttrs {
    fn default() -> Self {
        Self {
            name: "fc_session".into(),
            path: "/".into(),
            domain: None,
            http_only: true,
            secure: true,
            same_site: SameSite::Lax,
            max_age_secs: 60 * 60 * 8,
        }
    }
}

/// Server-agnostic session backend. Implementations are responsible for
/// reading the session from the incoming request, persisting it on the
/// outgoing response (cookie), and clearing it on logout.
#[async_trait]
pub trait SessionStore: Send + Sync + 'static {
    async fn read(&self, headers: &HeaderMap) -> Option<SessionPayload>;
    async fn write(
        &self,
        session: &SessionPayload,
        headers: &mut HeaderMap,
    ) -> Result<(), AuthError>;
    async fn clear(&self, headers: &mut HeaderMap) -> Result<(), AuthError>;
}

/// Default session backend: AES-256-GCM encrypted cookie. The full payload
/// is round-tripped through the cookie value, so no server storage is
/// needed. Cookie size is capped by the browser (~4KB).
pub struct CookieSessionStore {
    crypto: SessionCrypto,
    attrs: CookieAttrs,
}

impl CookieSessionStore {
    pub fn new(crypto: SessionCrypto, attrs: CookieAttrs) -> Self {
        Self { crypto, attrs }
    }

    pub fn attrs(&self) -> &CookieAttrs {
        &self.attrs
    }
}

#[async_trait]
impl SessionStore for CookieSessionStore {
    async fn read(&self, headers: &HeaderMap) -> Option<SessionPayload> {
        let jar = jar_from(headers);
        let envelope = jar.get(&self.attrs.name)?.value().to_owned();
        let bytes = self.crypto.decrypt(&envelope)?;
        let payload: SessionPayload = serde_json::from_slice(&bytes).ok()?;
        let now_ms = now_ms();
        if payload.expires_at <= now_ms {
            return None;
        }
        Some(payload)
    }

    async fn write(
        &self,
        session: &SessionPayload,
        headers: &mut HeaderMap,
    ) -> Result<(), AuthError> {
        let body = serde_json::to_vec(session)
            .map_err(|e| AuthError::Crypto(format!("serialize session: {e}")))?;
        let envelope = self.crypto.encrypt(&body)?;
        let cookie = build_cookie(&self.attrs, envelope, false);
        append_set_cookie(headers, &cookie);
        Ok(())
    }

    async fn clear(&self, headers: &mut HeaderMap) -> Result<(), AuthError> {
        let cookie = build_cookie(&self.attrs, String::new(), true);
        append_set_cookie(headers, &cookie);
        Ok(())
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn jar_from(headers: &HeaderMap) -> CookieJar {
    CookieJar::from_headers(headers)
}

fn build_cookie<'a>(attrs: &CookieAttrs, value: String, clear: bool) -> Cookie<'a> {
    let mut builder = Cookie::build((attrs.name.clone(), value))
        .path(attrs.path.clone())
        .http_only(attrs.http_only)
        .secure(attrs.secure)
        .same_site(attrs.same_site);
    if let Some(d) = &attrs.domain {
        builder = builder.domain(d.clone());
    }
    if clear {
        builder = builder.max_age(time::Duration::seconds(0));
    } else {
        builder = builder.max_age(time::Duration::seconds(attrs.max_age_secs));
    }
    builder.build()
}

fn append_set_cookie(headers: &mut HeaderMap, cookie: &Cookie<'_>) {
    if let Ok(value) = cookie.to_string().parse() {
        headers.append(SET_COOKIE, value);
    }
}

/// Sharable handle — auth middleware holds `Arc<dyn SessionStore>` so the
/// concrete store type is opaque past plugin registration.
pub type SharedSessionStore = Arc<dyn SessionStore>;


#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::header::COOKIE;

    fn sample_session() -> SessionPayload {
        SessionPayload {
            principal: PrincipalSnapshot {
                id: "prn_x".into(),
                principal_type: "USER".into(),
                tier: "CLIENT".into(),
                scope: String::new(),
                name: "Test".into(),
                email: None,
                clients: vec!["clt_a".into()],
                roles: vec!["r".into()],
                applications: vec![],
                all_applications: false,
            },
            tokens: SessionTokens {
                access_token: "at".into(),
                access_token_expires_at: now_ms() + 60_000,
                refresh_token: Some("rt".into()),
            },
            session_data: serde_json::json!({ "foo": "bar" }),
            expires_at: now_ms() + 3_600_000,
            mechanism: AuthMechanism::Session,
        }
    }

    #[tokio::test]
    async fn cookie_store_round_trip() {
        let crypto = SessionCrypto::new([super::super::crypto::generate_session_secret()]).unwrap();
        let store = CookieSessionStore::new(
            crypto,
            CookieAttrs {
                secure: false,
                ..Default::default()
            },
        );
        let mut headers = HeaderMap::new();
        let session = sample_session();
        store.write(&session, &mut headers).await.unwrap();

        // Move Set-Cookie → Cookie to simulate a follow-up request.
        let set = headers.remove(SET_COOKIE).expect("set-cookie");
        let cookie_value = set.to_str().unwrap().split(';').next().unwrap();
        let mut req_headers = HeaderMap::new();
        req_headers.insert(COOKIE, cookie_value.parse().unwrap());

        let read = store.read(&req_headers).await.expect("session read");
        assert_eq!(read.principal.id, "prn_x");
        assert_eq!(read.session_data, serde_json::json!({ "foo": "bar" }));
    }

    #[tokio::test]
    async fn cookie_store_returns_none_when_expired() {
        let crypto = SessionCrypto::new([super::super::crypto::generate_session_secret()]).unwrap();
        let store = CookieSessionStore::new(
            crypto,
            CookieAttrs {
                secure: false,
                ..Default::default()
            },
        );
        let mut headers = HeaderMap::new();
        let mut s = sample_session();
        s.expires_at = 0;
        store.write(&s, &mut headers).await.unwrap();
        let set = headers.remove(SET_COOKIE).unwrap();
        let cookie_value = set.to_str().unwrap().split(';').next().unwrap();
        let mut req_headers = HeaderMap::new();
        req_headers.insert(COOKIE, cookie_value.parse().unwrap());
        assert!(store.read(&req_headers).await.is_none());
    }
}
