//! `fnr_config` + `fnr_secrets` (Java `function/FunctionSettingsRepository.java`):
//! per-function, per-key settings that outlive a deploy.
//!
//! **Secrets are encrypted at rest.** `fnr_secrets.value_ref` holds the
//! [`EncryptionService::encrypt_ref`] form (`encrypted:…`), never plaintext,
//! or an `aws-sm://` secret-manager reference the caller sent (owner
//! decision #54, [`classify_opaque_secret`]): the reference is resolved when
//! the secret is delivered, not here. With no app key configured, writing a
//! secret fails rather than falling back to plaintext; the API's
//! `503 ENCRYPTION_UNCONFIGURED` answers before a write gets here, and this
//! is the second line of defence. Reads never return a value, only keys and
//! metadata; [`FunctionSettingsRepository::open_secrets_each`] is for the
//! host control plane's desired state (P6).

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use sqlx::PgPool;

use super::entity::{FunctionConfig, FunctionSecret, SecretInfo};
use fc_platform_core::shared::encryption_service::EncryptionService;
use fc_platform_core::shared::error::{PlatformError, Result};
use fc_platform_core::shared::secret_ref::{classify_opaque_secret, OpaqueSecret};
use fc_platform_core::usecase::{DbTx, Persist};

/// A stored secret, opened as far as the repository can: an `encrypted:`
/// value decrypted, a secret-manager reference as stored. `Debug` never
/// shows a value.
#[derive(Clone, PartialEq, Eq)]
pub enum OpenedSecret {
    Value(String),
    Reference(String),
}

impl fmt::Debug for OpenedSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenedSecret::Value(_) => f.write_str("Value(***)"),
            OpenedSecret::Reference(r) => f.debug_tuple("Reference").field(r).finish(),
        }
    }
}

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
            sqlx::query_as("SELECT key, value FROM fnr_config WHERE function_id = $1")
                .bind(function_id)
                .fetch_all(&self.pool)
                .await?;
        Ok(rows.into_iter().collect())
    }

    /// Every function's config in `function_ids`, in one query, keyed by
    /// function (absent for a function with none).
    pub async fn config_maps(
        &self,
        function_ids: &[String],
    ) -> Result<HashMap<String, BTreeMap<String, String>>> {
        let mut out: HashMap<String, BTreeMap<String, String>> = HashMap::new();
        if function_ids.is_empty() {
            return Ok(out);
        }
        let rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT function_id, key, value FROM fnr_config WHERE function_id = ANY($1)",
        )
        .bind(function_ids)
        .fetch_all(&self.pool)
        .await?;
        for (function_id, key, value) in rows {
            out.entry(function_id).or_default().insert(key, value);
        }
        Ok(out)
    }

    // ── secrets ────────────────────────────────────────────────────────────

    /// Every secret's metadata, in key order. Never a value.
    pub async fn list_secrets(&self, function_id: &str) -> Result<Vec<SecretInfo>> {
        let rows: Vec<(String, DateTime<Utc>, String)> = sqlx::query_as(
            "SELECT key, updated_at, updated_by FROM fnr_secrets WHERE function_id = $1 \
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
            "SELECT EXISTS (SELECT 1 FROM fnr_secrets WHERE function_id = $1 AND key = $2)",
        )
        .bind(function_id)
        .bind(key)
        .fetch_one(&self.pool)
        .await?;
        Ok(exists)
    }

    /// Decrypts the function's secrets named in `keys`. A key with no row,
    /// whose value does not decrypt, or that holds a secret-manager
    /// reference, is absent; with no app key, nothing is read.
    pub async fn decrypt_secrets(
        &self,
        function_id: &str,
        keys: &[String],
    ) -> Result<BTreeMap<String, String>> {
        let mut opened = self
            .open_secrets_each(&[(function_id, keys.to_vec())])
            .await?;
        Ok(opened
            .pop()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(key, secret)| match secret {
                OpenedSecret::Value(value) => Some((key, value)),
                OpenedSecret::Reference(_) => None,
            })
            .collect())
    }

    /// Opens the secrets of many `(function_id, declared keys)` requests in
    /// one query, answered in request order. Only the keys a request names
    /// are opened for it: an `encrypted:` value decrypted, a reference as
    /// stored (the caller resolves it). A key with no row, or whose value
    /// does not decrypt, is absent (it then shows as a missing setting,
    /// never a 500); with no app key, nothing is read.
    pub async fn open_secrets_each(
        &self,
        requests: &[(&str, Vec<String>)],
    ) -> Result<Vec<BTreeMap<String, OpenedSecret>>> {
        let empty = || requests.iter().map(|_| BTreeMap::new()).collect();
        let Some(encryption) = &self.encryption else {
            return Ok(empty());
        };
        let function_ids: Vec<&str> = requests
            .iter()
            .filter(|(_, keys)| !keys.is_empty())
            .map(|(f, _)| *f)
            .collect();
        if function_ids.is_empty() {
            return Ok(empty());
        }
        let rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT function_id, key, value_ref FROM fnr_secrets WHERE function_id = ANY($1)",
        )
        .bind(&function_ids)
        .fetch_all(&self.pool)
        .await?;
        let mut refs: HashMap<(String, String), String> = HashMap::new();
        for (function_id, key, value_ref) in rows {
            refs.insert((function_id, key), value_ref);
        }
        Ok(requests
            .iter()
            .map(|(function_id, keys)| {
                keys.iter()
                    .filter_map(|key| {
                        let value_ref = refs.get(&(function_id.to_string(), key.clone()))?;
                        if let Ok(OpaqueSecret::Reference(reference)) =
                            classify_opaque_secret(value_ref)
                        {
                            return Some((key.clone(), OpenedSecret::Reference(reference.to_string())));
                        }
                        match encryption.decrypt_ref(value_ref) {
                            Ok(plaintext) => Some((key.clone(), OpenedSecret::Value(plaintext))),
                            Err(e) => {
                                tracing::warn!(function_id, key = %key, error = %e, "function secret did not decrypt");
                                None
                            }
                        }
                    })
                    .collect()
            })
            .collect())
    }

    /// Whether an `encrypted:` value opens with this platform's key (a
    /// value copied from another environment does not). With no app key,
    /// nothing opens.
    pub fn opens(&self, encrypted: &str) -> bool {
        self.encryption
            .as_ref()
            .is_some_and(|e| e.decrypt_ref(encrypted.trim()).is_ok())
    }

    /// The stored form of a value sent for a secret (owner decision #54): an
    /// `aws-sm://` reference or an `encrypted:` value as sent, anything else
    /// encrypted. The use case has already refused a malformed one.
    fn stored_ref(&self, value: &str) -> Result<String> {
        let encryption = self.encryption.as_ref().ok_or_else(|| {
            PlatformError::internal(
                "SECRET: FLOWCATALYST_APP_KEY not configured; cannot encrypt function secret",
            )
        })?;
        match classify_opaque_secret(value).map_err(|e| PlatformError::validation(e.0))? {
            OpaqueSecret::Reference(kept) | OpaqueSecret::Encrypted(kept) => Ok(kept.to_string()),
            OpaqueSecret::Plaintext(plaintext) => Ok(encryption.encrypt_ref(plaintext)?),
        }
    }
}

impl Persist<FunctionConfig> for FunctionSettingsRepository {
    /// A full replacement: every key not in `values` is deleted, every key in
    /// it is upserted.
    async fn persist(&self, config: &FunctionConfig, tx: &mut DbTx<'_>) -> Result<()> {
        let keys: Vec<&str> = config.values.keys().map(String::as_str).collect();
        sqlx::query("DELETE FROM fnr_config WHERE function_id = $1 AND NOT (key = ANY($2))")
            .bind(&config.function_id)
            .bind(&keys)
            .execute(&mut **tx.inner)
            .await?;
        if keys.is_empty() {
            return Ok(());
        }
        let values: Vec<&str> = config.values.values().map(String::as_str).collect();
        sqlx::query(
            "INSERT INTO fnr_config (function_id, key, value, updated_by, updated_at) \
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
        sqlx::query("DELETE FROM fnr_config WHERE function_id = $1")
            .bind(&config.function_id)
            .execute(&mut **tx.inner)
            .await?;
        Ok(())
    }
}

impl Persist<FunctionSecret> for FunctionSettingsRepository {
    /// Stores the value ([`Self::stored_ref`]) and upserts one row.
    async fn persist(&self, secret: &FunctionSecret, tx: &mut DbTx<'_>) -> Result<()> {
        let value_ref = self.stored_ref(secret.value.expose())?;
        sqlx::query(
            "INSERT INTO fnr_secrets (function_id, key, value_ref, updated_by, updated_at) \
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
        sqlx::query("DELETE FROM fnr_secrets WHERE function_id = $1 AND key = $2")
            .bind(&secret.function_id)
            .bind(&secret.key)
            .execute(&mut **tx.inner)
            .await?;
        Ok(())
    }
}
