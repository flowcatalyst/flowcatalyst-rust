//! Authorization Code Repository — PostgreSQL via SQLx
//!
//! Stores authorization codes in `oauth_oidc_payloads` (type = "AuthorizationCode")
//! for compatibility with the TypeScript oidc-provider implementation.

use crate::auth::authorization_code::Pkce;
use crate::shared::enum_str::corrupt_value;
use crate::shared::error::{PlatformError, Result};
use crate::AuthorizationCode;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::PgPool;
use tracing::debug;

const PAYLOAD_TYPE: &str = "AuthorizationCode";

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

impl TryFrom<PayloadRow> for AuthorizationCode {
    type Error = PlatformError;
    fn try_from(m: PayloadRow) -> Result<Self> {
        let p = &m.payload;
        let method = p.get("codeChallengeMethod").and_then(|v| v.as_str());
        let pkce = Pkce::from_parts(
            p.get("codeChallenge")
                .and_then(|v| v.as_str())
                .map(String::from),
            method,
        )
        .map_err(|_| {
            corrupt_value(
                "oauth_oidc_payloads",
                "payload.codeChallengeMethod",
                method.unwrap_or_default(),
                &m.id,
            )
        })?;
        let code =
            m.id.strip_prefix("AuthorizationCode:")
                .unwrap_or(&m.id)
                .to_string();
        let created_at = m.created_at;
        let expires_at = m
            .expires_at
            .unwrap_or_else(|| created_at + chrono::Duration::minutes(10));

        // consumed_at being set means the code was used
        let used = m.consumed_at.is_some();

        Ok(AuthorizationCode {
            code,
            client_id: p
                .get("clientId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            principal_id: p
                .get("accountId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            redirect_uri: p
                .get("redirectUri")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            scope: p.get("scope").and_then(|v| v.as_str()).map(String::from),
            pkce,
            nonce: p.get("nonce").and_then(|v| v.as_str()).map(String::from),
            state: p.get("state").and_then(|v| v.as_str()).map(String::from),
            context_client_id: p
                .get("contextClientId")
                .and_then(|v| v.as_str())
                .map(String::from),
            created_at,
            expires_at,
            used,
        })
    }
}

/// Repository for authorization codes via oauth_oidc_payloads.
pub struct AuthorizationCodeRepository {
    pool: PgPool,
}

impl AuthorizationCodeRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    /// Build the composite ID: "AuthorizationCode:{code}"
    fn make_id(code: &str) -> String {
        format!("{}:{}", PAYLOAD_TYPE, code)
    }

    /// Build JSONB payload from domain entity
    fn to_payload(code: &AuthorizationCode) -> Value {
        json!({
            "accountId": code.principal_id,
            "clientId": code.client_id,
            "redirectUri": code.redirect_uri,
            "scope": code.scope,
            "codeChallenge": code.pkce.as_ref().map(|p| &p.challenge),
            "codeChallengeMethod": code.pkce.as_ref().map(|p| p.method.as_str()),
            "nonce": code.nonce,
            "state": code.state,
            "contextClientId": code.context_client_id,
            "kind": PAYLOAD_TYPE,
            "iat": code.created_at.timestamp(),
            "exp": code.expires_at.timestamp(),
        })
    }

    /// Insert a new authorization code.
    pub async fn insert(&self, code: &AuthorizationCode) -> Result<()> {
        sqlx::query(
            r#"INSERT INTO oauth_oidc_payloads
                (id, type, payload, grant_id, user_code, uid, expires_at, consumed_at, created_at)
            VALUES ($1, $2, $3, NULL, NULL, NULL, $4, NULL, $5)
            ON CONFLICT (id) DO UPDATE SET payload = $3, expires_at = $4"#,
        )
        .bind(Self::make_id(&code.code))
        .bind(PAYLOAD_TYPE)
        .bind(Self::to_payload(code))
        .bind(Some(code.expires_at))
        .bind(code.created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Find an authorization code by its code value.
    pub async fn find_by_code(&self, code: &str) -> Result<Option<AuthorizationCode>> {
        let row =
            sqlx::query_as::<_, PayloadRow>("SELECT * FROM oauth_oidc_payloads WHERE id = $1")
                .bind(Self::make_id(code))
                .fetch_optional(&self.pool)
                .await?;
        row.map(AuthorizationCode::try_from).transpose()
    }

    /// Find a valid (not used, not expired) authorization code.
    pub async fn find_valid_code(&self, code: &str) -> Result<Option<AuthorizationCode>> {
        let row = sqlx::query_as::<_, PayloadRow>(
            r#"SELECT * FROM oauth_oidc_payloads
            WHERE id = $1
              AND consumed_at IS NULL
              AND expires_at > NOW()"#,
        )
        .bind(Self::make_id(code))
        .fetch_optional(&self.pool)
        .await?;
        row.map(AuthorizationCode::try_from).transpose()
    }

    /// Atomically find and consume a valid (not used, not expired) authorization code.
    ///
    /// Uses `UPDATE ... WHERE consumed_at IS NULL AND expires_at > NOW() RETURNING *`
    /// to prevent race conditions where two concurrent token requests could both
    /// consume the same authorization code. Returns None if the code doesn't exist,
    /// has expired, or was already consumed by another request.
    pub async fn find_and_consume(&self, code: &str) -> Result<Option<AuthorizationCode>> {
        let composite_id = Self::make_id(code);
        let row = sqlx::query_as::<_, PayloadRow>(
            r#"UPDATE oauth_oidc_payloads
            SET consumed_at = NOW()
            WHERE id = $1
              AND consumed_at IS NULL
              AND expires_at > NOW()
            RETURNING *"#,
        )
        .bind(&composite_id)
        .fetch_optional(&self.pool)
        .await?;

        if row.is_some() {
            debug!(
                code_prefix = &code[..code.len().min(8)],
                "Authorization code atomically consumed"
            );
        }

        row.map(AuthorizationCode::try_from).transpose()
    }

    /// Mark an authorization code as used (consumed).
    pub async fn mark_as_used(&self, code: &str) -> Result<bool> {
        let result = sqlx::query(
            r#"UPDATE oauth_oidc_payloads
            SET consumed_at = NOW()
            WHERE id = $1"#,
        )
        .bind(Self::make_id(code))
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Delete an authorization code.
    pub async fn delete(&self, code: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM oauth_oidc_payloads WHERE id = $1")
            .bind(Self::make_id(code))
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Delete all expired authorization codes.
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

    /// Delete all authorization codes for a principal.
    pub async fn delete_by_principal(&self, principal_id: &str) -> Result<u64> {
        let result = sqlx::query(
            r#"DELETE FROM oauth_oidc_payloads
            WHERE type = $1 AND payload->>'accountId' = $2"#,
        )
        .bind(PAYLOAD_TYPE)
        .bind(principal_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    /// Delete all authorization codes for a client.
    pub async fn delete_by_client(&self, client_id: &str) -> Result<u64> {
        let result = sqlx::query(
            r#"DELETE FROM oauth_oidc_payloads
            WHERE type = $1 AND payload->>'clientId' = $2"#,
        )
        .bind(PAYLOAD_TYPE)
        .bind(client_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    /// Count all authorization codes.
    pub async fn count(&self) -> Result<u64> {
        let (count,) =
            sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM oauth_oidc_payloads WHERE type = $1")
                .bind(PAYLOAD_TYPE)
                .fetch_one(&self.pool)
                .await?;
        Ok(count as u64)
    }

    /// Count valid (not consumed, not expired) authorization codes.
    pub async fn count_valid(&self) -> Result<u64> {
        let (count,) = sqlx::query_as::<_, (i64,)>(
            r#"SELECT COUNT(*) FROM oauth_oidc_payloads
            WHERE type = $1
              AND consumed_at IS NULL
              AND expires_at > NOW()"#,
        )
        .bind(PAYLOAD_TYPE)
        .fetch_one(&self.pool)
        .await?;
        Ok(count as u64)
    }
}
