//! `fn_routes`, read side (Java `function/FunctionRouteRepository.java`).
//! Routes are materialised wholesale from the live manifest at promote,
//! which is P5; this workstream reads them.

use chrono::{DateTime, Utc};
use sqlx::PgPool;

use super::entity::FunctionRoute;
use super::{Hostname, RoutePattern};
use crate::shared::enum_str::corrupt_value;
use crate::shared::error::Result;

#[derive(sqlx::FromRow)]
struct RouteRow {
    id: String,
    function_id: String,
    hostname: String,
    path_prefix: String,
    alias_prefixes: Vec<String>,
    created_at: DateTime<Utc>,
}

const COLUMNS: &str = "id, function_id, hostname, path_prefix, alias_prefixes, created_at";

pub struct FunctionRouteRepository {
    pool: PgPool,
}

impl FunctionRouteRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn list_by_function(&self, function_id: &str) -> Result<Vec<FunctionRoute>> {
        let rows = sqlx::query_as::<_, RouteRow>(&format!(
            "SELECT {COLUMNS} FROM fn_routes WHERE function_id = $1 \
             ORDER BY hostname ASC, path_prefix ASC"
        ))
        .bind(function_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(to_entity).collect()
    }

    /// Every route at one of `keys`' exact `(hostname, pathPrefix)` pairs,
    /// in one query (Java `findPublic`, for many).
    pub async fn find_public_each(
        &self,
        keys: &[(&Hostname, &RoutePattern)],
    ) -> Result<Vec<FunctionRoute>> {
        if keys.is_empty() {
            return Ok(Vec::new());
        }
        let hostnames: Vec<&str> = keys.iter().map(|(h, _)| h.value()).collect();
        let prefixes: Vec<&str> = keys.iter().map(|(_, p)| p.value()).collect();
        let rows = sqlx::query_as::<_, RouteRow>(&format!(
            "SELECT {COLUMNS} FROM fn_routes \
             WHERE (hostname, path_prefix) IN (SELECT * FROM UNNEST($1::text[], $2::text[]))"
        ))
        .bind(&hostnames)
        .bind(&prefixes)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(to_entity).collect()
    }

    pub async fn list_by_hostname(&self, hostname: &Hostname) -> Result<Vec<FunctionRoute>> {
        let rows = sqlx::query_as::<_, RouteRow>(&format!(
            "SELECT {COLUMNS} FROM fn_routes WHERE hostname = $1 \
             ORDER BY hostname ASC, path_prefix ASC"
        ))
        .bind(hostname.value())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(to_entity).collect()
    }

    /// Every route whose hostname the zone covers: the zone itself, or any
    /// hostname strictly under it.
    pub async fn list_under(&self, zone: &Hostname) -> Result<Vec<FunctionRoute>> {
        let suffix = format!(".{}", zone.value());
        let rows = sqlx::query_as::<_, RouteRow>(&format!(
            "SELECT {COLUMNS} FROM fn_routes \
             WHERE hostname = $1 OR right(hostname, char_length($2)) = $2 \
             ORDER BY hostname ASC, path_prefix ASC"
        ))
        .bind(zone.value())
        .bind(&suffix)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(to_entity).collect()
    }
}

fn to_entity(row: RouteRow) -> Result<FunctionRoute> {
    let hostname = Hostname::try_parse(&row.hostname)
        .ok_or_else(|| corrupt_value("fn_routes", "hostname", &row.hostname, &row.id))?;
    let path_prefix = RoutePattern::try_parse(&row.path_prefix)
        .ok_or_else(|| corrupt_value("fn_routes", "path_prefix", &row.path_prefix, &row.id))?;
    Ok(FunctionRoute {
        id: row.id,
        function_id: row.function_id,
        hostname,
        path_prefix,
        alias_prefixes: row.alias_prefixes,
        created_at: row.created_at,
    })
}
