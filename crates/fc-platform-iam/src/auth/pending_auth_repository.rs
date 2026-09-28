//! Pending Auth State Repository — PostgreSQL via SQLx
//!
//! Stores OAuth pending authorization states in `oauth_oidc_payloads` (type = "PendingAuth")
//! to survive server restarts. Replaces the in-memory HashMap that was used previously.

use chrono::{DateTime, Duration, Utc};
use fc_platform_core::shared::error::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;

const PAYLOAD_TYPE: &str = "PendingAuth";
/// Pending auth states expire after 10 minutes
const EXPIRY_SECONDS: i64 = 600;

/// Row struct matching the `oauth_oidc_payloads` table
#[derive(Debug, sqlx::FromRow)]
#[allow(dead_code)]
struct PayloadRow {
    pub id: String,
    #[sqlx(rename = "type")]
    pub r#type: String,
    pub payload: serde_json::Value,
    pub grant_id: Option<String>,
    pub user_code: Option<String>,
    pub uid: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub consumed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Pending authorization state (between /authorize and callback)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingAuth {
    pub client_id: String,
    pub redirect_uri: String,
    pub scope: Option<String>,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
    pub nonce: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Repository for pending OAuth authorization states.
pub struct PendingAuthRepository {
    pool: PgPool,
}

impl PendingAuthRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    /// Build the composite ID: "PendingAuth:{state}"
    fn make_id(state: &str) -> String {
        format!("{}:{}", PAYLOAD_TYPE, state)
    }

    /// Store a pending auth state keyed by the state parameter.
    pub async fn insert(&self, state_param: &str, pending: &PendingAuth) -> Result<()> {
        let now = Utc::now();
        let expires_at = now + Duration::seconds(EXPIRY_SECONDS);

        let payload = json!({
            "clientId": pending.client_id,
            "redirectUri": pending.redirect_uri,
            "scope": pending.scope,
            "codeChallenge": pending.code_challenge,
            "codeChallengeMethod": pending.code_challenge_method,
            "nonce": pending.nonce,
            "createdAt": pending.created_at.to_rfc3339(),
        });

        sqlx::query(
            r#"INSERT INTO oauth_oidc_payloads (id, type, payload, expires_at, created_at)
               VALUES ($1, $2, $3, $4, $5)
               ON CONFLICT (id) DO UPDATE SET payload = $3, expires_at = $4"#,
        )
        .bind(Self::make_id(state_param))
        .bind(PAYLOAD_TYPE)
        .bind(&payload)
        .bind(expires_at)
        .bind(now)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Find and remove a pending auth state atomically (single-use).
    /// Returns None if state doesn't exist or has expired.
    pub async fn find_and_consume(&self, state_param: &str) -> Result<Option<PendingAuth>> {
        let composite_id = Self::make_id(state_param);

        let row = sqlx::query_as::<_, PayloadRow>(
            r#"DELETE FROM oauth_oidc_payloads
               WHERE id = $1 AND consumed_at IS NULL AND expires_at > NOW()
               RETURNING *"#,
        )
        .bind(&composite_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| Self::from_payload(&r.payload)))
    }

    fn from_payload(p: &serde_json::Value) -> PendingAuth {
        let created_at = p
            .get("createdAt")
            .and_then(|v| v.as_str())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(Utc::now);

        PendingAuth {
            client_id: p
                .get("clientId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            redirect_uri: p
                .get("redirectUri")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            scope: p.get("scope").and_then(|v| v.as_str()).map(String::from),
            code_challenge: p
                .get("codeChallenge")
                .and_then(|v| v.as_str())
                .map(String::from),
            code_challenge_method: p
                .get("codeChallengeMethod")
                .and_then(|v| v.as_str())
                .map(String::from),
            nonce: p.get("nonce").and_then(|v| v.as_str()).map(String::from),
            created_at,
        }
    }

    /// Delete all expired pending auth states (cleanup).
    pub async fn delete_expired(&self) -> Result<u64> {
        let result =
            sqlx::query("DELETE FROM oauth_oidc_payloads WHERE type = $1 AND expires_at < NOW()")
                .bind(PAYLOAD_TYPE)
                .execute(&self.pool)
                .await?;

        Ok(result.rows_affected())
    }
}
