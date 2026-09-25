//! Refresh Token Repository — PostgreSQL via SQLx
//!
//! Stores refresh tokens in `oauth_oidc_payloads` (type = "RefreshToken")
//! for compatibility with the TypeScript oidc-provider implementation.

use crate::shared::error::Result;
use crate::RefreshToken;
use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Value};
use sqlx::PgPool;

const PAYLOAD_TYPE: &str = "RefreshToken";

/// Row struct matching oauth_oidc_payloads columns
#[derive(sqlx::FromRow)]
#[allow(dead_code)]
struct PayloadRow {
    id: String,
    #[sqlx(rename = "type")]
    r#type: String,
    payload: Value,
    grant_id: Option<String>,
    user_code: Option<String>,
    uid: Option<String>,
    expires_at: Option<DateTime<Utc>>,
    consumed_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

impl From<PayloadRow> for RefreshToken {
    fn from(m: PayloadRow) -> Self {
        let p = &m.payload;
        let id =
            m.id.strip_prefix("RefreshToken:")
                .unwrap_or(&m.id)
                .to_string();

        let scopes: Vec<String> = p
            .get("scope")
            .and_then(|v| v.as_str())
            .map(|s| {
                s.split_whitespace()
                    .filter(|v| !v.is_empty())
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();

        let accessible_clients: Vec<String> = p
            .get("accessibleClients")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let revoked = p.get("revoked").and_then(|v| v.as_bool()).unwrap_or(false);

        // `revokedAt` is RFC 3339 text (Go's storage contract); rows Rust
        // revoked before it wrote that form carry Postgres' timestamp text,
        // so the consumed_at column — stamped in the same statement —
        // stands in.
        let revoked_at = p
            .get("revokedAt")
            .and_then(|v| v.as_str())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc))
            .or(if revoked { m.consumed_at } else { None });

        let token_family = p
            .get("tokenFamily")
            .and_then(|v| v.as_str())
            .map(String::from);
        let replaced_by = p
            .get("replacedBy")
            .and_then(|v| v.as_str())
            .map(String::from);

        let last_used_at = p
            .get("lastUsedAt")
            .and_then(|v| v.as_str())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc));

        let created_from_ip = p
            .get("createdFromIp")
            .and_then(|v| v.as_str())
            .map(String::from);
        let user_agent = p
            .get("userAgent")
            .and_then(|v| v.as_str())
            .map(String::from);

        let created_at = m.created_at;
        let expires_at = m
            .expires_at
            .unwrap_or_else(|| created_at + Duration::days(30));

        RefreshToken {
            id,
            token_hash: p
                .get("tokenHash")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            principal_id: p
                .get("accountId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            oauth_client_id: p.get("clientId").and_then(|v| v.as_str()).map(String::from),
            scopes,
            accessible_clients,
            revoked,
            revoked_at,
            token_family,
            replaced_by,
            created_at,
            expires_at,
            last_used_at,
            created_from_ip,
            user_agent,
        }
    }
}

/// Repository for refresh token management via oauth_oidc_payloads
pub struct RefreshTokenRepository {
    pool: PgPool,
}

impl RefreshTokenRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    /// Build the composite ID: "RefreshToken:{id}"
    fn make_id(id: &str) -> String {
        format!("{}:{}", PAYLOAD_TYPE, id)
    }

    /// Build JSONB payload from domain entity
    fn to_payload(token: &RefreshToken) -> Value {
        json!({
            "accountId": token.principal_id,
            "clientId": token.oauth_client_id,
            "tokenHash": token.token_hash,
            "scope": token.scopes.join(" "),
            "accessibleClients": token.accessible_clients,
            "revoked": token.revoked,
            "revokedAt": token.revoked_at.map(|dt| dt.to_rfc3339()),
            "tokenFamily": token.token_family,
            "replacedBy": token.replaced_by,
            "lastUsedAt": token.last_used_at.map(|dt| dt.to_rfc3339()),
            "createdFromIp": token.created_from_ip,
            "userAgent": token.user_agent,
            "iat": token.created_at.timestamp(),
            "exp": token.expires_at.timestamp(),
            "kind": PAYLOAD_TYPE,
        })
    }

    /// Insert a new refresh token
    pub async fn insert(&self, token: &RefreshToken) -> Result<()> {
        sqlx::query(
            r#"INSERT INTO oauth_oidc_payloads
                (id, type, payload, grant_id, user_code, uid, expires_at, consumed_at, created_at)
            VALUES ($1, $2, $3, $4, NULL, NULL, $5, NULL, $6)
            ON CONFLICT (id) DO UPDATE SET payload = $3, grant_id = $4, expires_at = $5"#,
        )
        .bind(Self::make_id(&token.id))
        .bind(PAYLOAD_TYPE)
        .bind(Self::to_payload(token))
        .bind(&token.token_family)
        .bind(Some(token.expires_at))
        .bind(token.created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Rotate atomically (Java `GrantStore.consumeValidByHash` + insert +
    /// `markReplaced` in one transaction, 6a06a7f0 S2.5): consume the
    /// presented token — revoked, `replacedBy` the replacement, filed in
    /// `family` if it had none — only if it is still valid and unconsumed,
    /// then insert the replacement. Of two concurrent rotations exactly one
    /// consumes: the other's UPDATE waits on the row lock and, re-checking
    /// `consumed_at IS NULL` against the committed row, matches nothing.
    /// Returns whether this call consumed the token (nothing is written
    /// when it did not).
    pub async fn consume_and_replace(
        &self,
        token_hash: &str,
        family: &str,
        replacement: &RefreshToken,
    ) -> Result<bool> {
        let now = Utc::now();
        let mut tx = self.pool.begin().await?;
        let consumed = sqlx::query_scalar::<_, String>(
            r#"UPDATE oauth_oidc_payloads
            SET payload = jsonb_set(
                    jsonb_set(
                        jsonb_set(
                            jsonb_set(payload, '{revoked}', 'true'::jsonb),
                            '{revokedAt}', to_jsonb($4::text)
                        ),
                        '{replacedBy}', to_jsonb($5::text)
                    ),
                    '{tokenFamily}',
                    COALESCE(NULLIF(payload->'tokenFamily', 'null'::jsonb), to_jsonb($6::text))
                ),
                grant_id = COALESCE(grant_id, $6),
                consumed_at = $3
            WHERE type = $1
              AND payload->>'tokenHash' = $2
              AND consumed_at IS NULL
              AND expires_at > NOW()
              AND (payload->>'revoked' IS NULL OR payload->>'revoked' = 'false')
            RETURNING id"#,
        )
        .bind(PAYLOAD_TYPE)
        .bind(token_hash)
        .bind(now)
        .bind(now.to_rfc3339())
        .bind(&replacement.token_hash)
        .bind(family)
        .fetch_optional(&mut *tx)
        .await?;
        if consumed.is_none() {
            tx.rollback().await?;
            return Ok(false);
        }
        sqlx::query(
            r#"INSERT INTO oauth_oidc_payloads
                (id, type, payload, grant_id, user_code, uid, expires_at, consumed_at, created_at)
            VALUES ($1, $2, $3, $4, NULL, NULL, $5, NULL, $6)"#,
        )
        .bind(Self::make_id(&replacement.id))
        .bind(PAYLOAD_TYPE)
        .bind(Self::to_payload(replacement))
        .bind(&replacement.token_family)
        .bind(Some(replacement.expires_at))
        .bind(replacement.created_at)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(true)
    }

    /// Find a refresh token by its hash
    pub async fn find_by_hash(&self, token_hash: &str) -> Result<Option<RefreshToken>> {
        let row = sqlx::query_as::<_, PayloadRow>(
            r#"SELECT * FROM oauth_oidc_payloads
            WHERE type = $1 AND payload->>'tokenHash' = $2"#,
        )
        .bind(PAYLOAD_TYPE)
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(RefreshToken::from))
    }

    /// Find a valid (non-expired, non-revoked) refresh token by its hash
    pub async fn find_valid_by_hash(&self, token_hash: &str) -> Result<Option<RefreshToken>> {
        let row = sqlx::query_as::<_, PayloadRow>(
            r#"SELECT * FROM oauth_oidc_payloads
            WHERE type = $1
              AND payload->>'tokenHash' = $2
              AND expires_at > NOW()
              AND consumed_at IS NULL"#,
        )
        .bind(PAYLOAD_TYPE)
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;
        match row {
            Some(m) => {
                let token = RefreshToken::from(m);
                if token.revoked {
                    Ok(None)
                } else {
                    Ok(Some(token))
                }
            }
            None => Ok(None),
        }
    }

    /// Find all tokens for a principal
    pub async fn find_by_principal(&self, principal_id: &str) -> Result<Vec<RefreshToken>> {
        let rows = sqlx::query_as::<_, PayloadRow>(
            r#"SELECT * FROM oauth_oidc_payloads
            WHERE type = $1 AND payload->>'accountId' = $2"#,
        )
        .bind(PAYLOAD_TYPE)
        .bind(principal_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(RefreshToken::from).collect())
    }

    /// Find all active tokens for a principal
    pub async fn find_active_by_principal(&self, principal_id: &str) -> Result<Vec<RefreshToken>> {
        let rows = sqlx::query_as::<_, PayloadRow>(
            r#"SELECT * FROM oauth_oidc_payloads
            WHERE type = $1
              AND payload->>'accountId' = $2
              AND expires_at > NOW()
              AND consumed_at IS NULL"#,
        )
        .bind(PAYLOAD_TYPE)
        .bind(principal_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(RefreshToken::from)
            .filter(|t| !t.revoked)
            .collect())
    }

    /// Revoke a token by its ID.
    ///
    /// Atomically sets the revoked flag in the JSONB payload and marks consumed_at.
    pub async fn revoke_by_id(&self, id: &str) -> Result<bool> {
        let composite_id = Self::make_id(id);
        let now = Utc::now();
        let result = sqlx::query(
            r#"UPDATE oauth_oidc_payloads
            SET payload = jsonb_set(
                jsonb_set(payload, '{revoked}', 'true'::jsonb),
                '{revokedAt}', to_jsonb($3::text)
            ),
            consumed_at = $2
            WHERE id = $1"#,
        )
        .bind(&composite_id)
        .bind(now)
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Revoke a token by its hash
    pub async fn revoke_by_hash(&self, token_hash: &str) -> Result<bool> {
        let now = Utc::now();
        let result = sqlx::query(
            r#"UPDATE oauth_oidc_payloads
            SET payload = jsonb_set(
                jsonb_set(payload, '{revoked}', 'true'::jsonb),
                '{revokedAt}', to_jsonb($4::text)
            ),
            consumed_at = $3
            WHERE type = $1 AND payload->>'tokenHash' = $2"#,
        )
        .bind(PAYLOAD_TYPE)
        .bind(token_hash)
        .bind(now)
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Revoke all tokens for a principal (logout all devices).
    ///
    /// Single UPDATE instead of N individual revocations.
    pub async fn revoke_all_for_principal(&self, principal_id: &str) -> Result<u64> {
        let now = Utc::now();
        let result = sqlx::query(
            r#"UPDATE oauth_oidc_payloads
            SET payload = jsonb_set(
                jsonb_set(payload, '{revoked}', 'true'::jsonb),
                '{revokedAt}', to_jsonb($4::text)
            ),
            consumed_at = $3
            WHERE type = $1
              AND payload->>'accountId' = $2
              AND consumed_at IS NULL
              AND expires_at > NOW()
              AND (payload->>'revoked' IS NULL OR payload->>'revoked' = 'false')"#,
        )
        .bind(PAYLOAD_TYPE)
        .bind(principal_id)
        .bind(now)
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    /// Find all tokens in a token family (by grant_id)
    pub async fn find_by_family(&self, family_id: &str) -> Result<Vec<RefreshToken>> {
        let rows = sqlx::query_as::<_, PayloadRow>(
            r#"SELECT * FROM oauth_oidc_payloads
            WHERE type = $1 AND grant_id = $2"#,
        )
        .bind(PAYLOAD_TYPE)
        .bind(family_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(RefreshToken::from).collect())
    }

    /// Revoke all tokens in a token family.
    ///
    /// Single UPDATE instead of N individual revocations.
    pub async fn revoke_all_in_family(&self, family_id: &str) -> Result<u64> {
        let now = Utc::now();
        let result = sqlx::query(
            r#"UPDATE oauth_oidc_payloads
            SET payload = jsonb_set(
                jsonb_set(payload, '{revoked}', 'true'::jsonb),
                '{revokedAt}', to_jsonb($4::text)
            ),
            consumed_at = $3
            WHERE type = $1
              AND grant_id = $2
              AND (payload->>'revoked' IS NULL OR payload->>'revoked' = 'false')"#,
        )
        .bind(PAYLOAD_TYPE)
        .bind(family_id)
        .bind(now)
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    /// Mark a token as replaced during token rotation.
    ///
    /// Atomically sets the replacedBy field in the JSONB payload.
    pub async fn mark_as_replaced(&self, token_hash: &str, new_token_hash: &str) -> Result<bool> {
        let result = sqlx::query(
            r#"UPDATE oauth_oidc_payloads
            SET payload = jsonb_set(payload, '{replacedBy}', to_jsonb($3::text))
            WHERE type = $1 AND payload->>'tokenHash' = $2"#,
        )
        .bind(PAYLOAD_TYPE)
        .bind(token_hash)
        .bind(new_token_hash)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Check if a token was replaced
    pub async fn was_replaced(&self, token_hash: &str) -> Result<bool> {
        let (exists,) = sqlx::query_as::<_, (bool,)>(
            r#"SELECT EXISTS(
                SELECT 1 FROM oauth_oidc_payloads
                WHERE type = $1
                  AND payload->>'tokenHash' = $2
                  AND payload->>'replacedBy' IS NOT NULL
            )"#,
        )
        .bind(PAYLOAD_TYPE)
        .bind(token_hash)
        .fetch_one(&self.pool)
        .await?;
        Ok(exists)
    }

    /// Update last used timestamp.
    ///
    /// Atomically sets the lastUsedAt field in the JSONB payload.
    pub async fn update_last_used(&self, id: &str) -> Result<bool> {
        let now = Utc::now();
        let result = sqlx::query(
            r#"UPDATE oauth_oidc_payloads
            SET payload = jsonb_set(payload, '{lastUsedAt}', to_jsonb($2::text))
            WHERE id = $1"#,
        )
        .bind(Self::make_id(id))
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Delete expired tokens (cleanup job)
    pub async fn delete_expired(&self) -> Result<u64> {
        let result = sqlx::query(
            r#"DELETE FROM oauth_oidc_payloads
            WHERE type = $1 AND expires_at < NOW()"#,
        )
        .bind(PAYLOAD_TYPE)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    /// Delete revoked tokens older than a given date (cleanup job)
    pub async fn delete_revoked_before(&self, cutoff: DateTime<Utc>) -> Result<u64> {
        let result = sqlx::query(
            r#"DELETE FROM oauth_oidc_payloads
            WHERE type = $1
              AND consumed_at IS NOT NULL
              AND created_at < $2"#,
        )
        .bind(PAYLOAD_TYPE)
        .bind(cutoff)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    /// Count active tokens for a principal.
    ///
    /// Uses a single COUNT query instead of loading all tokens into memory.
    pub async fn count_active_for_principal(&self, principal_id: &str) -> Result<u64> {
        let (count,) = sqlx::query_as::<_, (i64,)>(
            r#"SELECT COUNT(*) FROM oauth_oidc_payloads
            WHERE type = $1
              AND payload->>'accountId' = $2
              AND expires_at > NOW()
              AND consumed_at IS NULL
              AND (payload->>'revoked' IS NULL OR payload->>'revoked' = 'false')"#,
        )
        .bind(PAYLOAD_TYPE)
        .bind(principal_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(count as u64)
    }

    /// Count all refresh token payloads
    pub async fn count(&self) -> Result<u64> {
        let (count,) =
            sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM oauth_oidc_payloads WHERE type = $1")
                .bind(PAYLOAD_TYPE)
                .fetch_one(&self.pool)
                .await?;
        Ok(count as u64)
    }

    /// Count expired refresh tokens
    pub async fn count_expired(&self) -> Result<u64> {
        let (count,) = sqlx::query_as::<_, (i64,)>(
            r#"SELECT COUNT(*) FROM oauth_oidc_payloads
            WHERE type = $1 AND expires_at < NOW()"#,
        )
        .bind(PAYLOAD_TYPE)
        .fetch_one(&self.pool)
        .await?;
        Ok(count as u64)
    }
}

#[cfg(test)]
mod tests {
    // Repository tests require a PostgreSQL connection
    // These would typically be integration tests
}
