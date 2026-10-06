//! WebAuthn Credential Repository — PostgreSQL via SQLx.

use chrono::{DateTime, Utc};
use fc_platform_core::shared::id::PrincipalId;
use fc_platform_core::shared::id::WebauthnCredentialId;
use sqlx::PgPool;
use webauthn_rs::prelude::Passkey;

use super::entity::WebauthnCredential;
use fc_platform_core::shared::error::{PlatformError, Result};
use fc_platform_core::usecase::DbTx;
use fc_platform_core::usecase::HasId;
use fc_platform_core::usecase::Persist;

struct WebauthnCredentialRow {
    id: String,
    principal_id: PrincipalId,
    #[allow(dead_code)]
    credential_id: Vec<u8>,
    passkey_data: serde_json::Value,
    name: Option<String>,
    created_at: DateTime<Utc>,
    last_used_at: Option<DateTime<Utc>>,
}

impl TryFrom<WebauthnCredentialRow> for WebauthnCredential {
    type Error = PlatformError;
    fn try_from(r: WebauthnCredentialRow) -> Result<Self> {
        let passkey: Passkey = serde_json::from_value(r.passkey_data).map_err(|e| {
            PlatformError::internal(format!("corrupt passkey blob for {}: {}", r.id, e))
        })?;
        Ok(Self {
            id: WebauthnCredentialId::from_wire(r.id),
            principal_id: PrincipalId::from_wire(r.principal_id),
            passkey,
            name: r.name,
            created_at: r.created_at,
            last_used_at: r.last_used_at,
        })
    }
}

pub struct WebauthnCredentialRepository {
    pool: PgPool,
}

impl WebauthnCredentialRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<WebauthnCredential>> {
        let row = sqlx::query_as!(
            WebauthnCredentialRow,
            "SELECT id, principal_id AS \"principal_id: PrincipalId\", credential_id, \
                    passkey_data AS \"passkey_data: serde_json::Value\", name, \
                    created_at, last_used_at \
                    FROM webauthn_credentials WHERE id = $1",
            id
        )
        .fetch_optional(&self.pool)
        .await?;
        row.map(WebauthnCredential::try_from).transpose()
    }

    pub async fn find_by_credential_id(
        &self,
        credential_id: &[u8],
    ) -> Result<Option<WebauthnCredential>> {
        let row = sqlx::query_as!(
            WebauthnCredentialRow,
            "SELECT id, principal_id AS \"principal_id: PrincipalId\", credential_id, \
                    passkey_data AS \"passkey_data: serde_json::Value\", name, \
                    created_at, last_used_at \
                    FROM webauthn_credentials WHERE credential_id = $1",
            credential_id
        )
        .fetch_optional(&self.pool)
        .await?;
        row.map(WebauthnCredential::try_from).transpose()
    }

    pub async fn find_by_principal(
        &self,
        principal_id: &PrincipalId,
    ) -> Result<Vec<WebauthnCredential>> {
        let rows = sqlx::query_as!(
            WebauthnCredentialRow,
            "SELECT id, principal_id AS \"principal_id: PrincipalId\", credential_id, \
                    passkey_data AS \"passkey_data: serde_json::Value\", name, \
                    created_at, last_used_at \
                    FROM webauthn_credentials WHERE principal_id = $1 ORDER BY created_at DESC",
            principal_id as &PrincipalId
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(WebauthnCredential::try_from).collect()
    }

    pub async fn count_for_principal(&self, principal_id: &PrincipalId) -> Result<i64> {
        let count = sqlx::query_scalar!(
            "SELECT COUNT(*) AS \"count!\" FROM webauthn_credentials WHERE principal_id = $1",
            principal_id as &PrincipalId
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(count)
    }
}

impl HasId for WebauthnCredential {
    fn id(&self) -> &str {
        self.id.as_str()
    }
}

impl Persist<WebauthnCredential> for WebauthnCredentialRepository {
    async fn persist(&self, c: &WebauthnCredential, tx: &mut DbTx<'_>) -> Result<()> {
        let passkey_data = serde_json::to_value(&c.passkey)
            .map_err(|e| PlatformError::internal(format!("serialise passkey: {}", e)))?;
        sqlx::query!(
            "INSERT INTO webauthn_credentials
                (id, principal_id, credential_id, passkey_data, name, created_at, last_used_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (id) DO UPDATE SET
                passkey_data = EXCLUDED.passkey_data,
                name = EXCLUDED.name,
                last_used_at = EXCLUDED.last_used_at",
            &c.id as &WebauthnCredentialId,
            &c.principal_id as &PrincipalId,
            c.credential_id_bytes(),
            passkey_data,
            c.name.as_ref(),
            c.created_at,
            c.last_used_at
        )
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }

    async fn delete(&self, c: &WebauthnCredential, tx: &mut DbTx<'_>) -> Result<()> {
        sqlx::query!(
            "DELETE FROM webauthn_credentials WHERE id = $1",
            &c.id as &WebauthnCredentialId
        )
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }
}
