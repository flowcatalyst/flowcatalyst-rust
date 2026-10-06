//! WebAuthn ceremony state — short-lived, single-use challenge persistence.
//!
//! Stored in `oauth_oidc_payloads` (the same JSONB store the OIDC provider
//! uses) with type discriminants:
//! - `WebauthnRegistration:{state_id}` — `PasskeyRegistration` blob + principal_id
//! - `WebauthnAuthentication:{state_id}` — `PasskeyAuthentication` blob + optional principal_id
//!
//! `consume_*` uses `DELETE ... RETURNING` so a successful read also marks
//! the state as used in a single round-trip — race-free and replay-safe.

use chrono::{DateTime, Duration, Utc};
use fc_platform_core::shared::id::PrincipalId;
use serde_json::json;
use sqlx::PgPool;
use webauthn_rs::prelude::{PasskeyAuthentication, PasskeyRegistration};

use fc_platform_core::shared::error::{PlatformError, Result};

const REGISTRATION_TYPE: &str = "WebauthnRegistration";
const AUTHENTICATION_TYPE: &str = "WebauthnAuthentication";
const DEFAULT_TTL_SECS: i64 = 600;

fn make_id(kind: &str, state_id: &str) -> String {
    format!("{}:{}", kind, state_id)
}

pub struct WebauthnCeremonyRepository {
    pool: PgPool,
}

pub struct ConsumedRegistration {
    pub principal_id: PrincipalId,
    pub state: PasskeyRegistration,
    pub display_name: Option<String>,
}

pub struct ConsumedAuthentication {
    pub principal_id: Option<PrincipalId>,
    pub state: PasskeyAuthentication,
}

impl WebauthnCeremonyRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn store_registration(
        &self,
        state_id: &str,
        principal_id: &PrincipalId,
        state: &PasskeyRegistration,
        display_name: Option<&str>,
    ) -> Result<()> {
        let payload = json!({
            "principalId": principal_id,
            "displayName": display_name,
            "state": serde_json::to_value(state)
                .map_err(|e| PlatformError::internal(format!("serialise PasskeyRegistration: {}", e)))?,
        });
        let expires_at = Utc::now() + Duration::seconds(DEFAULT_TTL_SECS);
        sqlx::query!(
            "INSERT INTO oauth_oidc_payloads (id, type, payload, expires_at, created_at)
             VALUES ($1, $2, $3, $4, NOW())
             ON CONFLICT (id) DO UPDATE SET payload = EXCLUDED.payload, expires_at = EXCLUDED.expires_at",
            make_id(REGISTRATION_TYPE, state_id),
            REGISTRATION_TYPE,
            payload,
            expires_at
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn consume_registration(
        &self,
        state_id: &str,
    ) -> Result<Option<ConsumedRegistration>> {
        let payload = sqlx::query_scalar!(
            "DELETE FROM oauth_oidc_payloads
             WHERE id = $1 AND (expires_at IS NULL OR expires_at > NOW())
             RETURNING payload",
            make_id(REGISTRATION_TYPE, state_id)
        )
        .fetch_optional(&self.pool)
        .await?;

        let Some(payload) = payload else {
            return Ok(None);
        };
        let principal_id = payload
            .get("principalId")
            .and_then(|v| v.as_str())
            .map(PrincipalId::from_wire)
            .ok_or_else(|| PlatformError::internal("ceremony payload missing principalId"))?;
        let display_name = payload
            .get("displayName")
            .and_then(|v| v.as_str())
            .map(String::from);
        let state: PasskeyRegistration = serde_json::from_value(
            payload
                .get("state")
                .cloned()
                .ok_or_else(|| PlatformError::internal("ceremony payload missing state"))?,
        )
        .map_err(|e| PlatformError::internal(format!("deserialise PasskeyRegistration: {}", e)))?;

        Ok(Some(ConsumedRegistration {
            principal_id,
            state,
            display_name,
        }))
    }

    pub async fn store_authentication(
        &self,
        state_id: &str,
        principal_id: Option<&PrincipalId>,
        state: &PasskeyAuthentication,
    ) -> Result<()> {
        let payload = json!({
            "principalId": principal_id,
            "state": serde_json::to_value(state)
                .map_err(|e| PlatformError::internal(format!("serialise PasskeyAuthentication: {}", e)))?,
        });
        let expires_at = Utc::now() + Duration::seconds(DEFAULT_TTL_SECS);
        sqlx::query!(
            "INSERT INTO oauth_oidc_payloads (id, type, payload, expires_at, created_at)
             VALUES ($1, $2, $3, $4, NOW())
             ON CONFLICT (id) DO UPDATE SET payload = EXCLUDED.payload, expires_at = EXCLUDED.expires_at",
            make_id(AUTHENTICATION_TYPE, state_id),
            AUTHENTICATION_TYPE,
            payload,
            expires_at
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn consume_authentication(
        &self,
        state_id: &str,
    ) -> Result<Option<ConsumedAuthentication>> {
        let payload = sqlx::query_scalar!(
            "DELETE FROM oauth_oidc_payloads
             WHERE id = $1 AND (expires_at IS NULL OR expires_at > NOW())
             RETURNING payload",
            make_id(AUTHENTICATION_TYPE, state_id)
        )
        .fetch_optional(&self.pool)
        .await?;

        let Some(payload) = payload else {
            return Ok(None);
        };
        let principal_id = payload
            .get("principalId")
            .and_then(|v| v.as_str())
            .map(PrincipalId::from_wire);
        let state: PasskeyAuthentication = serde_json::from_value(
            payload
                .get("state")
                .cloned()
                .ok_or_else(|| PlatformError::internal("ceremony payload missing state"))?,
        )
        .map_err(|e| {
            PlatformError::internal(format!("deserialise PasskeyAuthentication: {}", e))
        })?;

        Ok(Some(ConsumedAuthentication {
            principal_id,
            state,
        }))
    }

    pub async fn purge_expired(&self) -> Result<u64> {
        let res = sqlx::query!(
            "DELETE FROM oauth_oidc_payloads
             WHERE type IN ($1, $2) AND expires_at IS NOT NULL AND expires_at <= NOW()",
            REGISTRATION_TYPE,
            AUTHENTICATION_TYPE
        )
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected())
    }

    pub fn registration_ttl_seconds(&self) -> DateTime<Utc> {
        Utc::now() + Duration::seconds(DEFAULT_TTL_SECS)
    }
}
