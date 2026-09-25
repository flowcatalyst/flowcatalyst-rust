//! `fn_hosts` (Java `function/FunctionHostRepository.java`).
//!
//! **Written by the heartbeat only, outside the unit of work.** A host row is
//! telemetry a host sends every 15 s, not a business operation: Java writes
//! it straight through the repository with no event and no audit
//! (`FunctionControlApi.heartbeat`, spec `function-api.md` §6.2 step 1), and
//! so does this. It is the same category as CLAUDE.md's platform
//! infrastructure exceptions: an event per beat would swamp `msg_events` at
//! four rows per host per minute. What a heartbeat *causes* (a version
//! becoming `READY`) does go through a use case with its event and audit.

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

    pub async fn find_by_id(&self, id: &str) -> Result<Option<FunctionHost>> {
        let row = sqlx::query_as::<_, HostRow>(
            "SELECT id, pool, state, loaded, started_at, last_heartbeat FROM fn_hosts WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(to_entity).transpose()
    }

    /// Hosts of `pool` heartbeated at or after `seen_since`, by id (Java
    /// `listLive`): the hosts whose loaded versions feed desired state's
    /// `unload`.
    pub async fn list_live(
        &self,
        pool: &str,
        seen_since: DateTime<Utc>,
    ) -> Result<Vec<FunctionHost>> {
        let rows = sqlx::query_as::<_, HostRow>(
            "SELECT id, pool, state, loaded, started_at, last_heartbeat FROM fn_hosts \
             WHERE pool = $1 AND last_heartbeat >= $2 ORDER BY id ASC",
        )
        .bind(pool)
        .bind(seen_since)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(to_entity).collect()
    }

    /// One heartbeat's write, in one transaction (Java
    /// `FunctionControlApi.heartbeat`: `deleteStale` then `persist`): first
    /// every row whose last heartbeat is strictly before `purge_before`,
    /// except `host`'s own (which this beat is about to refresh); then the
    /// upsert of `host`. The upsert never rewrites `pool` or `started_at`,
    /// so a host's pool is the one it first registered with. Returns how
    /// many stale rows were purged.
    pub async fn heartbeat(&self, host: &FunctionHost, purge_before: DateTime<Utc>) -> Result<u64> {
        let mut tx = self.pool.begin().await?;
        let purged = sqlx::query("DELETE FROM fn_hosts WHERE last_heartbeat < $1 AND id <> $2")
            .bind(purge_before)
            .bind(&host.id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        sqlx::query(
            "INSERT INTO fn_hosts (id, pool, state, loaded, started_at, last_heartbeat) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             ON CONFLICT (id) DO UPDATE SET \
                state = EXCLUDED.state, \
                loaded = EXCLUDED.loaded, \
                last_heartbeat = EXCLUDED.last_heartbeat",
        )
        .bind(&host.id)
        .bind(&host.pool)
        .bind(host.state.as_str())
        .bind(write_loaded(&host.loaded))
        .bind(host.started_at)
        .bind(host.last_heartbeat)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(purged)
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

/// `[{address, version, state, error?}]`, `error` only on `FAILED` (Java
/// `loadedToJson`).
fn write_loaded(loaded: &[LoadedVersion]) -> Value {
    Value::Array(
        loaded
            .iter()
            .map(|lv| {
                let mut entry = serde_json::Map::new();
                entry.insert("address".into(), Value::String(lv.address.render()));
                entry.insert("version".into(), Value::from(lv.version));
                entry.insert("state".into(), Value::String(lv.state.name().into()));
                if let Some(error) = lv.state.error() {
                    entry.insert("error".into(), Value::String(error.into()));
                }
                Value::Object(entry)
            })
            .collect(),
    )
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
    fn loaded_round_trips_with_the_error_only_on_failed() {
        let address = FunctionAddress::parse("a.b.c").unwrap();
        let loaded = vec![
            LoadedVersion {
                address: address.clone(),
                version: 1,
                state: LoadState::Registered,
            },
            LoadedVersion {
                address: address.clone(),
                version: 2,
                state: LoadState::Loaded,
            },
            LoadedVersion {
                address,
                version: 3,
                state: LoadState::Failed("LOAD:WASM_INVALID".into()),
            },
        ];
        let json = write_loaded(&loaded);
        assert_eq!(
            json,
            json!([
                {"address": "a.b.c", "version": 1, "state": "REGISTERED"},
                {"address": "a.b.c", "version": 2, "state": "LOADED"},
                {"address": "a.b.c", "version": 3, "state": "FAILED", "error": "LOAD:WASM_INVALID"},
            ])
        );
        assert_eq!(read_loaded(&json), loaded);
    }

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
