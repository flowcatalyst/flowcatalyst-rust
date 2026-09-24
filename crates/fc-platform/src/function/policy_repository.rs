//! `fn_client_policies` (Java `function/ClientPolicyRepository.java`).
//! Natural key: [`FunctionOwner::key`], so the platform's own policy is the
//! row `PLATFORM`. `signers` is read tolerantly: a rule with a blank issuer
//! or subject is dropped, and so is an unknown runtime within a rule.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::PgPool;

use super::entity::{ClientPolicy, SignerRule};
use super::{FunctionOwner, Runtime};
use crate::shared::enum_str::corrupt_value;
use crate::shared::error::Result;
use crate::usecase::{DbTx, Persist};

#[derive(sqlx::FromRow)]
struct PolicyRow {
    client_id: String,
    signers: Value,
    max_duration_ms: Option<i32>,
    max_concurrency: Option<i32>,
    max_wasm_memory_mb: Option<i32>,
    max_db_pool_size: Option<i32>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

const COLUMNS: &str = "client_id, signers, max_duration_ms, max_concurrency, max_wasm_memory_mb, \
                       max_db_pool_size, created_at, updated_at";

pub struct ClientPolicyRepository {
    pool: PgPool,
}

impl ClientPolicyRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn find_by_owner(&self, owner: &FunctionOwner) -> Result<Option<ClientPolicy>> {
        let row = sqlx::query_as::<_, PolicyRow>(&format!(
            "SELECT {COLUMNS} FROM fn_client_policies WHERE client_id = $1"
        ))
        .bind(owner.key())
        .fetch_optional(&self.pool)
        .await?;
        row.map(to_entity).transpose()
    }

    /// Every stored row: the platform's first, then client ids ascending.
    pub async fn list_all(&self) -> Result<Vec<ClientPolicy>> {
        let rows = sqlx::query_as::<_, PolicyRow>(&format!(
            "SELECT {COLUMNS} FROM fn_client_policies \
             ORDER BY CASE WHEN client_id = $1 THEN 0 ELSE 1 END, client_id ASC"
        ))
        .bind(FunctionOwner::PLATFORM_KEY)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(to_entity).collect()
    }
}

fn to_entity(row: PolicyRow) -> Result<ClientPolicy> {
    let owner = FunctionOwner::from_key(&row.client_id).map_err(|_| {
        corrupt_value(
            "fn_client_policies",
            "client_id",
            &row.client_id,
            &row.client_id,
        )
    })?;
    Ok(ClientPolicy {
        owner,
        signers: read_signers(&row.signers),
        max_duration_ms: row.max_duration_ms,
        max_concurrency: row.max_concurrency,
        max_wasm_memory_mb: row.max_wasm_memory_mb,
        max_db_pool_size: row.max_db_pool_size,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

/// The stored shape: `[{issuer, subject, runtimes: ["JVM", …]}]`, runtimes
/// by their stored (upper-case) names.
fn signers_to_json(signers: &[SignerRule]) -> Value {
    Value::Array(
        signers
            .iter()
            .map(|rule| {
                json!({
                    "issuer": rule.issuer,
                    "subject": rule.subject,
                    "runtimes": rule.runtimes.iter().map(|r| r.as_str()).collect::<Vec<_>>(),
                })
            })
            .collect(),
    )
}

fn read_signers(root: &Value) -> Vec<SignerRule> {
    let Some(entries) = root.as_array() else {
        return Vec::new();
    };
    entries.iter().filter_map(read_signer_rule).collect()
}

fn read_signer_rule(node: &Value) -> Option<SignerRule> {
    let object = node.as_object()?;
    let issuer = object
        .get("issuer")?
        .as_str()
        .filter(|s| !super::java_is_blank(s))?;
    let subject = object
        .get("subject")?
        .as_str()
        .filter(|s| !super::java_is_blank(s))?;
    let runtimes: Vec<Runtime> = object
        .get("runtimes")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .filter_map(|r| r.parse::<Runtime>().ok())
                .collect()
        })
        .unwrap_or_default();
    Some(SignerRule::new(issuer, subject, runtimes))
}

#[async_trait]
impl Persist<ClientPolicy> for ClientPolicyRepository {
    /// Upsert by owner key: a full replacement of every column but
    /// `created_at`.
    async fn persist(&self, p: &ClientPolicy, tx: &mut DbTx<'_>) -> Result<()> {
        sqlx::query(
            "INSERT INTO fn_client_policies \
                (client_id, created_at, signers, max_duration_ms, max_concurrency, \
                 max_wasm_memory_mb, max_db_pool_size, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
             ON CONFLICT (client_id) DO UPDATE SET \
                signers = EXCLUDED.signers, \
                max_duration_ms = EXCLUDED.max_duration_ms, \
                max_concurrency = EXCLUDED.max_concurrency, \
                max_wasm_memory_mb = EXCLUDED.max_wasm_memory_mb, \
                max_db_pool_size = EXCLUDED.max_db_pool_size, \
                updated_at = EXCLUDED.updated_at",
        )
        .bind(p.owner.key())
        .bind(p.created_at)
        .bind(signers_to_json(&p.signers))
        .bind(p.max_duration_ms)
        .bind(p.max_concurrency)
        .bind(p.max_wasm_memory_mb)
        .bind(p.max_db_pool_size)
        .bind(p.updated_at)
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }

    async fn delete(&self, p: &ClientPolicy, tx: &mut DbTx<'_>) -> Result<()> {
        sqlx::query("DELETE FROM fn_client_policies WHERE client_id = $1")
            .bind(p.owner.key())
            .execute(&mut **tx.inner)
            .await?;
        Ok(())
    }
}

/// Java `ClientPolicyRepositoryTest`'s tolerant-reader cases.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signers_round_trip_through_the_stored_shape() {
        let rules = vec![SignerRule::new(
            "https://i",
            "s",
            [Runtime::Wasm, Runtime::Jvm],
        )];
        let stored = signers_to_json(&rules);
        assert_eq!(
            stored,
            json!([{"issuer": "https://i", "subject": "s", "runtimes": ["JVM", "WASM"]}])
        );
        assert_eq!(read_signers(&stored), rules);
    }

    #[test]
    fn unreadable_rules_and_runtimes_are_dropped() {
        let read = read_signers(&json!([
            {"issuer": " ", "subject": "s", "runtimes": ["JVM"]},
            {"issuer": "i", "runtimes": ["JVM"]},
            {"issuer": "i", "subject": "s", "runtimes": ["JVM", "PYTHON", 3]},
            "junk",
        ]));
        assert_eq!(read, vec![SignerRule::new("i", "s", [Runtime::Jvm])]);
        assert!(read_signers(&json!({"not": "an array"})).is_empty());
        assert!(read_signers(&Value::Null).is_empty());
    }
}
