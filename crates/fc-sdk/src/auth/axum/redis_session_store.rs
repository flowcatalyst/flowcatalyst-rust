//! Redis-backed session store. Cookie holds an opaque 32-byte session id
//! (base64url); payload is stored in Redis under `${prefix}:{sid}` with TTL
//! enforced by Redis itself (no separate reaper needed).
//!
//! Mirrors `@flowcatalyst/sdk/fastify`'s `RedisSessionStore`. The lock
//! module's `RedisLockProvider` uses the same `redis::aio::ConnectionManager`
//! shape, so apps that already wire one can hand it to both.

use async_trait::async_trait;
use axum::http::header::{HeaderMap, SET_COOKIE};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngCore;
use redis::aio::ConnectionManager;
use redis::AsyncCommands;

use crate::auth::AuthError;

use super::session::{CookieAttrs, SessionPayload, SessionStore};
use std::cmp;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

const SID_LEN: usize = 32;

pub struct RedisSessionStore {
    conn: ConnectionManager,
    prefix: String,
    attrs: CookieAttrs,
}

impl RedisSessionStore {
    /// Build with the default key prefix (`fc:session`).
    pub fn new(conn: ConnectionManager, attrs: CookieAttrs) -> Self {
        Self {
            conn,
            prefix: "fc:session".to_string(),
            attrs,
        }
    }

    /// Build with a custom key prefix — useful when the Redis instance is
    /// shared across multiple apps.
    pub fn with_prefix(
        conn: ConnectionManager,
        prefix: impl Into<String>,
        attrs: CookieAttrs,
    ) -> Self {
        Self {
            conn,
            prefix: prefix.into(),
            attrs,
        }
    }

    fn key(&self, sid: &str) -> String {
        format!("{}:{}", self.prefix, sid)
    }
}

#[async_trait]
impl SessionStore for RedisSessionStore {
    async fn read(&self, headers: &HeaderMap) -> Option<SessionPayload> {
        let jar = CookieJar::from_headers(headers);
        let sid = jar.get(&self.attrs.name)?.value().to_owned();
        let mut conn = self.conn.clone();
        let raw: Option<String> = conn.get(self.key(&sid)).await.ok();
        let raw = raw?;
        serde_json::from_str(&raw).ok()
    }

    async fn write(
        &self,
        session: &SessionPayload,
        headers: &mut HeaderMap,
    ) -> Result<(), AuthError> {
        let sid = generate_sid();
        let body = serde_json::to_string(session)
            .map_err(|e| AuthError::Crypto(format!("serialize session: {e}")))?;
        let ttl_ms = cmp::max(1, session.expires_at - now_ms());
        let mut conn = self.conn.clone();
        let _: () = conn
            .pset_ex(self.key(&sid), body, ttl_ms as u64)
            .await
            .map_err(|e| AuthError::Config(format!("write session: {e}")))?;
        append_cookie(headers, &self.attrs, sid, false);
        Ok(())
    }

    async fn clear(&self, headers: &mut HeaderMap) -> Result<(), AuthError> {
        let jar = CookieJar::from_headers(headers);
        if let Some(sid) = jar.get(&self.attrs.name).map(|c| c.value().to_owned()) {
            let mut conn = self.conn.clone();
            let _: redis::RedisResult<i64> = conn.del(self.key(&sid)).await;
        }
        append_cookie(headers, &self.attrs, String::new(), true);
        Ok(())
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn generate_sid() -> String {
    let mut buf = [0u8; SID_LEN];
    rand::rng().fill_bytes(&mut buf);
    URL_SAFE_NO_PAD.encode(buf)
}

fn append_cookie(headers: &mut HeaderMap, attrs: &CookieAttrs, value: String, clear: bool) {
    let mut builder = Cookie::build((attrs.name.clone(), value))
        .path(attrs.path.clone())
        .http_only(attrs.http_only)
        .secure(attrs.secure)
        .same_site(attrs.same_site);
    if let Some(d) = &attrs.domain {
        builder = builder.domain(d.clone());
    }
    builder = if clear {
        builder.max_age(time::Duration::seconds(0))
    } else {
        builder.max_age(time::Duration::seconds(attrs.max_age_secs))
    };
    let cookie = builder.build();
    if let Ok(value) = cookie.to_string().parse() {
        headers.append(SET_COOKIE, value);
    }
}

#[allow(dead_code)]
const _SAMESITE_USE: SameSite = SameSite::Lax;
