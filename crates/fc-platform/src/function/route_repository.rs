//! `fn_routes` (Java `function/FunctionRouteRepository.java`). Routes are
//! materialised wholesale from the live manifest at promote: not an
//! aggregate with an event of its own but the `live` alias's projection, so
//! they are written by [`PromotedFunctionRepository`], in the same
//! transaction and the same commit as the alias change that makes that
//! manifest live.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

use super::entity::{Function, FunctionRoute};
use super::repository::FunctionRepository;
use super::{Hostname, RoutePattern};
use crate::shared::enum_str::corrupt_value;
use crate::shared::error::{PlatformError, Result};
use crate::usecase::{DbTx, HasId, Persist};

/// The unique `(hostname, path_prefix)` constraint (Java
/// `PUBLIC_ROUTE_UNIQUE_CONSTRAINT`).
const UNIQUE_CONSTRAINT: &str = "fn_routes_hostname_path_prefix_key";

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

impl FunctionRouteRepository {
    /// `fn_routes` for `function_id` := `routes` (Java `replaceForFunction`).
    /// Two promotes racing for one route past the plan's own check meet the
    /// unique constraint: that one constraint, by name, is `409
    /// PUBLIC_ROUTE_TAKEN`, never a 500; any other failure stays a failure.
    async fn replace_for_function(
        &self,
        function_id: &str,
        routes: &[FunctionRoute],
        tx: &mut DbTx<'_>,
    ) -> Result<()> {
        sqlx::query("DELETE FROM fn_routes WHERE function_id = $1")
            .bind(function_id)
            .execute(&mut **tx.inner)
            .await?;
        for r in routes {
            let inserted = sqlx::query(
                "INSERT INTO fn_routes \
                    (id, function_id, hostname, path_prefix, alias_prefixes, created_at) \
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(&r.id)
            .bind(&r.function_id)
            .bind(r.hostname.value())
            .bind(r.path_prefix.value())
            .bind(&r.alias_prefixes)
            .bind(r.created_at)
            .execute(&mut **tx.inner)
            .await;
            match inserted {
                Ok(_) => {}
                Err(sqlx::Error::Database(db)) if db.constraint() == Some(UNIQUE_CONSTRAINT) => {
                    return Err(PlatformError::BusinessRule {
                        code: "PUBLIC_ROUTE_TAKEN".into(),
                        message: "route is already taken (detected by the database's own \
                                  unique constraint)"
                            .into(),
                    })
                }
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }
}

/// A function whose alias change also materialises its public routes:
/// `routes` is `Some` only when promoting `live` changes them.
pub struct PromotedFunction {
    pub function: Function,
    pub routes: Option<Vec<FunctionRoute>>,
}

impl HasId for PromotedFunction {
    fn id(&self) -> &str {
        &self.function.id
    }
}

/// Persists a [`PromotedFunction`]: the function through its own
/// repository, then its routes.
pub struct PromotedFunctionRepository<'r> {
    pub functions: &'r FunctionRepository,
    pub routes: &'r FunctionRouteRepository,
}

#[async_trait]
impl Persist<PromotedFunction> for PromotedFunctionRepository<'_> {
    async fn persist(&self, p: &PromotedFunction, tx: &mut DbTx<'_>) -> Result<()> {
        self.functions.persist(&p.function, tx).await?;
        match &p.routes {
            Some(routes) => {
                self.routes
                    .replace_for_function(&p.function.id, routes, tx)
                    .await
            }
            None => Ok(()),
        }
    }

    /// The routes cascade with the function.
    async fn delete(&self, p: &PromotedFunction, tx: &mut DbTx<'_>) -> Result<()> {
        self.functions.delete(&p.function, tx).await
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
