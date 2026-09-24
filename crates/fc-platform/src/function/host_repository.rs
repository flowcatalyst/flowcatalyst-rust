//! `fn_hosts`, read side (Java `function/FunctionHostRepository.java`). The
//! heartbeat that writes these rows is the host control plane (P6).

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgPool;

use super::entity::{FunctionHost, HostState, LoadState, LoadedVersion};
use super::FunctionAddress;
use crate::shared::enum_str::decode;
use crate::shared::error::Result;

#[derive(sqlx::FromRow)]
struct HostRow {
    id: String,
    pool: String,
    state: String,
    loaded: Value,
    started_at: DateTime<Utc>,
    last_heartbeat: DateTime<Utc>,
}

/// One pool and how many hosts in it heartbeated since the cut-off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolSummary {
    pub pool: String,
    pub hosts: i64,
}

pub struct FunctionHostRepository {
    pool: PgPool,
}

impl FunctionHostRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    /// Every host, any pool and any age, that reports a version of
    /// `address`, ordered by id (Java reads every host and filters in
    /// memory; the containment test does the same filter in SQL).
    pub async fn list_reporting(&self, address: &FunctionAddress) -> Result<Vec<FunctionHost>> {
        let probe = serde_json::json!([{ "address": address.render() }]);
        let rows = sqlx::query_as::<_, HostRow>(
            "SELECT id, pool, state, loaded, started_at, last_heartbeat FROM fn_hosts \
             WHERE loaded @> $1 ORDER BY id ASC",
        )
        .bind(probe)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(to_entity).collect()
    }

    /// Every pool with at least one host heartbeated at or after
    /// `seen_since`, with that count, by pool name.
    pub async fn pools(&self, seen_since: DateTime<Utc>) -> Result<Vec<PoolSummary>> {
        let rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT pool, COUNT(*) FROM fn_hosts WHERE last_heartbeat >= $1 \
             GROUP BY pool ORDER BY pool ASC",
        )
        .bind(seen_since)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(pool, hosts)| PoolSummary { pool, hosts })
            .collect())
    }
}

fn to_entity(row: HostRow) -> Result<FunctionHost> {
    let state: HostState = decode(&row.state, "fn_hosts", "state", &row.id)?;
    Ok(FunctionHost {
        loaded: read_loaded(&row.loaded),
        id: row.id,
        pool: row.pool,
        state,
        started_at: row.started_at,
        last_heartbeat: row.last_heartbeat,
    })
}

/// Tolerant reader (Java `readLoaded`): an entry with a bad address, a
/// version that is not a positive integer, or an unknown state is dropped
/// rather than failing the row; a non-array reads as nothing loaded.
fn read_loaded(root: &Value) -> Vec<LoadedVersion> {
    let Some(entries) = root.as_array() else {
        return Vec::new();
    };
    entries.iter().filter_map(read_loaded_version).collect()
}

fn read_loaded_version(node: &Value) -> Option<LoadedVersion> {
    let object = node.as_object()?;
    let address = FunctionAddress::parse(object.get("address")?.as_str()?).ok()?;
    let version = object
        .get("version")?
        .as_i64()
        .and_then(|v| i32::try_from(v).ok())
        .filter(|v| *v > 0)?;
    let state = match object.get("state")?.as_str()? {
        "REGISTERED" => LoadState::Registered,
        "LOADED" => LoadState::Loaded,
        "FAILED" => LoadState::Failed(
            object
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        ),
        _ => return None,
    };
    Some(LoadedVersion {
        address,
        version,
        state,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn loaded_reader_drops_what_it_cannot_read() {
        let loaded = read_loaded(&json!([
            {"address": "a.b.c", "version": 1, "state": "LOADED"},
            {"address": "a.b.c", "version": 2, "state": "FAILED", "error": "boom"},
            {"address": "a.b.c", "version": 3, "state": "FAILED"},
            {"address": "a.b", "version": 1, "state": "LOADED"},
            {"address": "a.b.c", "version": 0, "state": "LOADED"},
            {"address": "a.b.c", "version": 1.5, "state": "LOADED"},
            {"address": "a.b.c", "version": 1, "state": "ELSEWHERE"},
            "junk",
        ]));
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded[0].state, LoadState::Loaded);
        assert_eq!(loaded[1].state, LoadState::Failed("boom".into()));
        assert_eq!(loaded[2].state, LoadState::Failed(String::new()));
        assert!(read_loaded(&json!({"not": "an array"})).is_empty());
    }
}
