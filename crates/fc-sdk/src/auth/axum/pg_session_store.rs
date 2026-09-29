//! Postgres-backed session store. Cookie holds an opaque session id
//! (32B random, base64url-encoded); payload lives in `fc_sessions`.
//!
//! Mirrors `@flowcatalyst/sdk/fastify`'s `PgSessionStore`. Compared to the
//! default cookie store, the Postgres backend:
//!   - keeps the cookie small (just the sid) — no ~4KB cap on `sessionData`
//!   - lets you revoke a session server-side without touching the browser
//!   - costs one DB round-trip per request
//!
//! Run [`init_session_schema`] once at startup, or fold the SQL into your
//! migration tool.

use async_trait::async_trait;
use axum::http::header::{HeaderMap, SET_COOKIE};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngCore;
use sqlx::types::chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::auth::AuthError;

use super::session::{CookieAttrs, SessionPayload, SessionStore};

const SID_LEN: usize = 32;

pub struct PgSessionStore {
    pool: PgPool,
    table: String,
    attrs: CookieAttrs,
}

impl PgSessionStore {
    /// Build with the default table name (`fc_sessions`).
    pub fn new(pool: PgPool, attrs: CookieAttrs) -> Self {
        Self {
            pool,
            table: "fc_sessions".to_string(),
            attrs,
        }
    }

    /// Build with a custom table name. Useful when an app already owns
    /// `fc_sessions` for a different purpose.
    pub fn with_table(pool: PgPool, table: impl Into<String>, attrs: CookieAttrs) -> Self {
        Self {
            pool,
            table: table.into(),
            attrs,
        }
    }

    /// Delete rows whose TTL has elapsed. Returns the number of rows removed.
    /// Call this from a periodic task — rows are invisible past `expires_at`
    /// anyway (read() filters), but reaping keeps the table small.
    pub async fn reap_expired(&self) -> Result<u64, AuthError> {
        let sql = format!("DELETE FROM {} WHERE expires_at <= NOW()", self.table);
        let res = sqlx::query(&sql)
            .execute(&self.pool)
            .await
            .map_err(|e| AuthError::Config(format!("reap session rows: {e}")))?;
        Ok(res.rows_affected())
    }
}

#[async_trait]
impl SessionStore for PgSessionStore {
    async fn read(&self, headers: &HeaderMap) -> Option<SessionPayload> {
        let jar = CookieJar::from_headers(headers);
        let sid = jar.get(&self.attrs.name)?.value().to_owned();
        let sql = format!(
            "SELECT payload FROM {} WHERE sid = $1 AND expires_at > NOW()",
            self.table,
        );
        let row: Option<(serde_json::Value,)> = sqlx::query_as(&sql)
            .bind(&sid)
            .fetch_optional(&self.pool)
            .await
            .ok()
            .flatten();
        let (payload,) = row?;
        serde_json::from_value(payload).ok()
    }

    async fn write(
        &self,
        session: &SessionPayload,
        headers: &mut HeaderMap,
    ) -> Result<(), AuthError> {
        let sid = generate_sid();
        let payload = serde_json::to_value(session)
            .map_err(|e| AuthError::Crypto(format!("serialize session: {e}")))?;
        let expires_at =
            DateTime::<Utc>::from_timestamp_millis(session.expires_at).unwrap_or_else(Utc::now);
        let sql = format!(
            "INSERT INTO {0} (sid, payload, expires_at) VALUES ($1, $2, $3) \
             ON CONFLICT (sid) DO UPDATE SET payload = EXCLUDED.payload, \
             expires_at = EXCLUDED.expires_at",
            self.table,
        );
        sqlx::query(&sql)
            .bind(&sid)
            .bind(payload)
            .bind(expires_at)
            .execute(&self.pool)
            .await
            .map_err(|e| AuthError::Config(format!("write session row: {e}")))?;
        append_cookie(headers, &self.attrs, sid, false);
        Ok(())
    }

    async fn clear(&self, headers: &mut HeaderMap) -> Result<(), AuthError> {
        let jar = CookieJar::from_headers(headers);
        if let Some(sid) = jar.get(&self.attrs.name).map(|c| c.value().to_owned()) {
            let sql = format!("DELETE FROM {} WHERE sid = $1", self.table);
            let _ = sqlx::query(&sql).bind(&sid).execute(&self.pool).await;
        }
        append_cookie(headers, &self.attrs, String::new(), true);
        Ok(())
    }
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

/// Build the `CREATE TABLE` SQL for the session backing table.
pub fn create_session_table_sql(table: &str) -> String {
    format!(
        "CREATE TABLE IF NOT EXISTS {0} (\
            sid         TEXT PRIMARY KEY,\
            payload     JSONB NOT NULL,\
            expires_at  TIMESTAMPTZ NOT NULL\
         );\
         CREATE INDEX IF NOT EXISTS {0}_expires_at_idx ON {0} (expires_at);",
        table,
    )
}

/// Run [`create_session_table_sql`] against the pool. Idempotent.
pub async fn init_session_schema(pool: &PgPool, table: &str) -> Result<(), AuthError> {
    sqlx::query(&create_session_table_sql(table))
        .execute(pool)
        .await
        .map_err(|e| AuthError::Config(format!("init session schema: {e}")))?;
    Ok(())
}

// Hint to rustc: SameSite is used via the builder; keep import live.
#[allow(dead_code)]
const _SAMESITE_USE: SameSite = SameSite::Lax;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_sql_uses_supplied_table_name() {
        let sql = create_session_table_sql("custom_sessions");
        assert!(sql.contains("CREATE TABLE IF NOT EXISTS custom_sessions"));
        assert!(sql.contains("sid         TEXT PRIMARY KEY"));
        assert!(sql.contains("expires_at  TIMESTAMPTZ NOT NULL"));
        assert!(sql.contains("custom_sessions_expires_at_idx"));
    }
}
