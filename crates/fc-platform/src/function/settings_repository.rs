//! `fn_config` + `fn_secrets` (Java `function/FunctionSettingsRepository.java`):
//! per-function, per-key settings that outlive a deploy.
//!
//! **Secrets are encrypted at rest.** `fn_secrets.value_ref` holds the
//! [`EncryptionService::encrypt_ref`] form (`encrypted:…`), never plaintext.
//! With no app key configured, writing a secret fails rather than falling
//! back to plaintext; the API's `503 ENCRYPTION_UNCONFIGURED` answers before
//! a write gets here, and this is the second line of defence. Reads never
//! return a value, only keys and metadata; [`FunctionSettingsRepository::decrypt_secrets`]
//! is for the host control plane's desired state (P6).

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

use super::entity::{FunctionConfig, FunctionSecret, SecretInfo};
use crate::shared::encryption_service::EncryptionService;
use crate::shared::error::{PlatformError, Result};
use crate::usecase::{DbTx, Persist};

pub struct FunctionSettingsRepository {
    pool: PgPool,
    encryption: Option<Arc<EncryptionService>>,
}

impl FunctionSettingsRepository {
    pub fn new(pool: &PgPool, encryption: Option<Arc<EncryptionService>>) -> Self {
        Self {
            pool: pool.clone(),
            encryption,
        }
    }

    /// Whether secrets can be written or read at all.
    pub fn encryption_configured(&self) -> bool {
        self.encryption.is_some()
    }

    // ── config ─────────────────────────────────────────────────────────────

    /// Every `(key, value)` of a function's config, in key order.
    pub async fn config_map(&self, function_id: &str) -> Result<BTreeMap<String, String>> {
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT key, value FROM fn_config WHERE function_id = $1")
                .bind(function_id)
                .fetch_all(&self.pool)
                .await?;
        Ok(rows.into_iter().collect())
    }

    // ── secrets ────────────────────────────────────────────────────────────

    /// Every secret's metadata, in key order. Never a value.
    pub async fn list_secrets(&self, function_id: &str) -> Result<Vec<SecretInfo>> {
        let rows: Vec<(String, DateTime<Utc>, String)> = sqlx::query_as(
            "SELECT key, updated_at, updated_by FROM fn_secrets WHERE function_id = $1 \
             ORDER BY key ASC",
        )
        .bind(function_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(key, updated_at, updated_by)| SecretInfo {
                key,
                updated_at,
                updated_by,
            })
            .collect())
    }

    /// Whether the function has a secret named `key`.
    pub async fn has_secret(&self, function_id: &str, key: &str) -> Result<bool> {
        let (exists,): (bool,) = sqlx::query_as(
            "SELECT EXISTS (SELECT 1 FROM fn_secrets WHERE function_id = $1 AND key = $2)",
        )
        .bind(function_id)
        .bind(key)
        .fetch_one(&self.pool)
        .await?;
        Ok(exists)
    }

    /// Decrypts the function's secrets named in `keys`. A key with no row,
    /// or whose value does not decrypt, is absent (it then shows as a
    /// missing setting, never a 500); with no app key, nothing is read.
    pub async fn decrypt_secrets(
        &self,
        function_id: &str,
        keys: &[String],
    ) -> Result<BTreeMap<String, String>> {
        let Some(encryption) = &self.encryption else {
            return Ok(BTreeMap::new());
        };
        if keys.is_empty() {
            return Ok(BTreeMap::new());
        }
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT key, value_ref FROM fn_secrets WHERE function_id = $1 AND key = ANY($2)",
        )
        .bind(function_id)
        .bind(keys)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .filter_map(|(key, value_ref)| match encryption.decrypt_ref(&value_ref) {
                Ok(plaintext) => Some((key, plaintext)),
                Err(e) => {
                    tracing::warn!(function_id, key = %key, error = %e, "function secret did not decrypt");
                    None
                }
            })
            .collect())
    }

    fn encrypted_ref(&self, plaintext: &str) -> Result<String> {
        let encryption = self.encryption.as_ref().ok_or_else(|| {
            PlatformError::internal(
                "SECRET: FLOWCATALYST_APP_KEY not configured; cannot encrypt function secret",
            )
        })?;
        Ok(encryption.encrypt_ref(plaintext)?)
    }
}

#[async_trait]
impl Persist<FunctionConfig> for FunctionSettingsRepository {
    /// A full replacement: every key not in `values` is deleted, every key in
    /// it is upserted.
    async fn persist(&self, config: &FunctionConfig, tx: &mut DbTx<'_>) -> Result<()> {
        let keys: Vec<&str> = config.values.keys().map(String::as_str).collect();
        sqlx::query("DELETE FROM fn_config WHERE function_id = $1 AND NOT (key = ANY($2))")
            .bind(&config.function_id)
            .bind(&keys)
            .execute(&mut **tx.inner)
            .await?;
        if keys.is_empty() {
            return Ok(());
        }
        let values: Vec<&str> = config.values.values().map(String::as_str).collect();
        sqlx::query(
            "INSERT INTO fn_config (function_id, key, value, updated_by, updated_at) \
             SELECT $1, k, v, $4, $5 FROM UNNEST($2::text[], $3::text[]) AS t(k, v) \
             ON CONFLICT (function_id, key) DO UPDATE SET \
                value = EXCLUDED.value, \
                updated_by = EXCLUDED.updated_by, \
                updated_at = EXCLUDED.updated_at",
        )
        .bind(&config.function_id)
        .bind(&keys)
        .bind(&values)
        .bind(&config.updated_by)
        .bind(config.updated_at)
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }

    async fn delete(&self, config: &FunctionConfig, tx: &mut DbTx<'_>) -> Result<()> {
        sqlx::query("DELETE FROM fn_config WHERE function_id = $1")
            .bind(&config.function_id)
            .execute(&mut **tx.inner)
            .await?;
        Ok(())
    }
}

#[async_trait]
impl Persist<FunctionSecret> for FunctionSettingsRepository {
    /// Encrypts the value and upserts one row.
    async fn persist(&self, secret: &FunctionSecret, tx: &mut DbTx<'_>) -> Result<()> {
        let value_ref = self.encrypted_ref(secret.value.expose())?;
        sqlx::query(
            "INSERT INTO fn_secrets (function_id, key, value_ref, updated_by, updated_at) \
             VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (function_id, key) DO UPDATE SET \
                value_ref = EXCLUDED.value_ref, \
                updated_by = EXCLUDED.updated_by, \
                updated_at = EXCLUDED.updated_at",
        )
        .bind(&secret.function_id)
        .bind(&secret.key)
        .bind(&value_ref)
        .bind(&secret.updated_by)
        .bind(secret.updated_at)
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }

    async fn delete(&self, secret: &FunctionSecret, tx: &mut DbTx<'_>) -> Result<()> {
        sqlx::query("DELETE FROM fn_secrets WHERE function_id = $1 AND key = $2")
            .bind(&secret.function_id)
            .bind(&secret.key)
            .execute(&mut **tx.inner)
            .await?;
        Ok(())
    }
}
