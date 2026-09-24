//! `fn_functions` + `fn_aliases` (Java `function/FunctionRepository.java`).
//! Aliases are hydrated with one `ANY` query per read and replaced
//! wholesale on write. Writes happen only on the unit of work's transaction.

use std::collections::HashMap;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, QueryBuilder};

use super::entity::{Function, FunctionAlias, FunctionStatus};
use super::{FunctionAddress, FunctionAddressPattern, FunctionOwner, Runtime};
use crate::shared::enum_str::{corrupt_value, decode};
use crate::shared::error::Result;
use crate::usecase::{DbTx, Persist};

#[derive(sqlx::FromRow)]
struct FunctionRow {
    id: String,
    application_id: String,
    application_code: String,
    service_name: String,
    name: String,
    client_id: Option<String>,
    runtime: String,
    description: Option<String>,
    status: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct AliasRow {
    function_id: String,
    alias: String,
    version_id: String,
    updated_by: String,
    updated_at: DateTime<Utc>,
}

const COLUMNS: &str = "id, application_id, application_code, service_name, name, client_id, \
                       runtime, description, status, created_at, updated_at";

/// Which owners a list may return (Java `Visibility` as
/// `FunctionRepository.reachCondition` reads it). Deliberately not the
/// platform's usual `client_id IS NULL OR …`: a non-anchor reaches no
/// platform-owned function at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnerReach {
    /// An anchor: every owner.
    Everything,
    /// A non-anchor holding the `*` client grant: every client, never the
    /// platform.
    AnyClient,
    /// A non-anchor: exactly these clients' functions (none when empty).
    Clients(Vec<String>),
}

/// The paginated list filter (Java `FunctionRepository.PageFilter`):
/// `pattern`, `owner` and `status` are the caller's optional narrowing;
/// `owners` and `applications` are its mandatory reach, always applied.
#[derive(Debug, Clone)]
pub struct FunctionListFilter {
    pub pattern: Option<FunctionAddressPattern>,
    pub owner: Option<FunctionOwner>,
    pub status: Option<FunctionStatus>,
    pub owners: OwnerReach,
    /// `None`: every application. `Some`: only these (none when empty).
    pub applications: Option<Vec<String>>,
}

pub struct FunctionRepository {
    pool: PgPool,
}

impl FunctionRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<Function>> {
        let row = sqlx::query_as::<_, FunctionRow>(&format!(
            "SELECT {COLUMNS} FROM fn_functions WHERE id = $1"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        self.hydrate_one(row).await
    }

    pub async fn find_by_address(&self, address: &FunctionAddress) -> Result<Option<Function>> {
        let row = sqlx::query_as::<_, FunctionRow>(&format!(
            "SELECT {COLUMNS} FROM fn_functions \
             WHERE application_code = $1 AND service_name = $2 AND name = $3"
        ))
        .bind(address.application())
        .bind(address.service())
        .bind(address.name())
        .fetch_optional(&self.pool)
        .await?;
        self.hydrate_one(row).await
    }

    /// Every function named by `ids`, in no particular order; an id with no
    /// row is simply absent.
    pub async fn find_by_ids(&self, ids: &[String]) -> Result<Vec<Function>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query_as::<_, FunctionRow>(&format!(
            "SELECT {COLUMNS} FROM fn_functions WHERE id = ANY($1)"
        ))
        .bind(ids)
        .fetch_all(&self.pool)
        .await?;
        self.hydrate(rows).await
    }

    /// One page of functions matching `filter`, ordered by address.
    pub async fn find_with_filters(
        &self,
        filter: &FunctionListFilter,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Function>> {
        let mut qb: QueryBuilder<Postgres> =
            QueryBuilder::new(format!("SELECT {COLUMNS} FROM fn_functions WHERE TRUE"));
        push_filter(&mut qb, filter);
        qb.push(" ORDER BY application_code ASC, service_name ASC, name ASC LIMIT ");
        qb.push_bind(limit);
        qb.push(" OFFSET ");
        qb.push_bind(offset);
        let rows = qb
            .build_query_as::<FunctionRow>()
            .fetch_all(&self.pool)
            .await?;
        self.hydrate(rows).await
    }

    /// The total for [`Self::find_with_filters`]'s filter, ignoring the page.
    pub async fn count_with_filters(&self, filter: &FunctionListFilter) -> Result<i64> {
        let mut qb: QueryBuilder<Postgres> =
            QueryBuilder::new("SELECT COUNT(*) FROM fn_functions WHERE TRUE");
        push_filter(&mut qb, filter);
        let (count,): (i64,) = qb.build_query_as().fetch_one(&self.pool).await?;
        Ok(count)
    }

    async fn hydrate_one(&self, row: Option<FunctionRow>) -> Result<Option<Function>> {
        match row {
            None => Ok(None),
            Some(row) => Ok(self.hydrate(vec![row]).await?.pop()),
        }
    }

    /// Rows to entities, loading every row's aliases in one query.
    async fn hydrate(&self, rows: Vec<FunctionRow>) -> Result<Vec<Function>> {
        if rows.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        let alias_rows = sqlx::query_as::<_, AliasRow>(
            "SELECT function_id, alias, version_id, updated_by, updated_at FROM fn_aliases \
             WHERE function_id = ANY($1) ORDER BY function_id ASC, alias ASC",
        )
        .bind(&ids)
        .fetch_all(&self.pool)
        .await?;
        let mut aliases: HashMap<String, Vec<FunctionAlias>> = HashMap::new();
        for a in alias_rows {
            aliases
                .entry(a.function_id)
                .or_default()
                .push(FunctionAlias {
                    alias: a.alias,
                    version_id: a.version_id,
                    updated_by: a.updated_by,
                    updated_at: a.updated_at,
                });
        }
        rows.into_iter()
            .map(|row| {
                let own = aliases.remove(&row.id).unwrap_or_default();
                to_entity(row, own)
            })
            .collect()
    }
}

/// The optional narrowing, then the mandatory reach. A pattern becomes
/// whole-segment column equalities, never `LIKE`.
fn push_filter(qb: &mut QueryBuilder<'_, Postgres>, filter: &FunctionListFilter) {
    match &filter.pattern {
        None => {}
        Some(FunctionAddressPattern::Exact(address)) => {
            qb.push(" AND application_code = ");
            qb.push_bind(address.application().to_string());
            qb.push(" AND service_name = ");
            qb.push_bind(address.service().to_string());
            qb.push(" AND name = ");
            qb.push_bind(address.name().to_string());
        }
        Some(FunctionAddressPattern::Service {
            application,
            service,
        }) => {
            qb.push(" AND application_code = ");
            qb.push_bind(application.value().to_string());
            qb.push(" AND service_name = ");
            qb.push_bind(service.value().to_string());
        }
        Some(FunctionAddressPattern::Application(application)) => {
            qb.push(" AND application_code = ");
            qb.push_bind(application.value().to_string());
        }
    }
    match &filter.owner {
        None => {}
        Some(FunctionOwner::Platform) => {
            qb.push(" AND client_id IS NULL");
        }
        Some(FunctionOwner::Client(id)) => {
            qb.push(" AND client_id = ");
            qb.push_bind(id.clone());
        }
    }
    if let Some(status) = filter.status {
        qb.push(" AND status = ");
        qb.push_bind(status.as_str());
    }
    match &filter.owners {
        OwnerReach::Everything => {}
        OwnerReach::AnyClient => {
            qb.push(" AND client_id IS NOT NULL");
        }
        OwnerReach::Clients(ids) => {
            qb.push(" AND client_id = ANY(");
            qb.push_bind(ids.clone());
            qb.push(")");
        }
    }
    if let Some(ids) = &filter.applications {
        qb.push(" AND application_id = ANY(");
        qb.push_bind(ids.clone());
        qb.push(")");
    }
}

fn to_entity(row: FunctionRow, aliases: Vec<FunctionAlias>) -> Result<Function> {
    let address = FunctionAddress::new(&row.application_code, &row.service_name, &row.name)
        .map_err(|_| {
            corrupt_value(
                "fn_functions",
                "application_code/service_name/name",
                &format!("{}.{}.{}", row.application_code, row.service_name, row.name),
                &row.id,
            )
        })?;
    let owner = FunctionOwner::of_client_id(row.client_id.as_deref()).map_err(|_| {
        corrupt_value(
            "fn_functions",
            "client_id",
            row.client_id.as_deref().unwrap_or(""),
            &row.id,
        )
    })?;
    let runtime: Runtime = decode(&row.runtime, "fn_functions", "runtime", &row.id)?;
    let status: FunctionStatus = decode(&row.status, "fn_functions", "status", &row.id)?;
    Ok(Function {
        id: row.id,
        application_id: row.application_id,
        address,
        owner,
        runtime,
        description: row.description,
        status,
        aliases,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

#[async_trait]
impl Persist<Function> for FunctionRepository {
    /// Upserts `fn_functions`, setting only `description`, `status` and
    /// `updated_at` on conflict (nothing moves a function), then replaces
    /// the alias rows: those no longer listed are deleted, the rest upserted.
    async fn persist(&self, f: &Function, tx: &mut DbTx<'_>) -> Result<()> {
        sqlx::query(
            "INSERT INTO fn_functions \
                (id, application_id, application_code, service_name, name, client_id, \
                 runtime, description, status, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) \
             ON CONFLICT (id) DO UPDATE SET \
                description = EXCLUDED.description, \
                status = EXCLUDED.status, \
                updated_at = EXCLUDED.updated_at",
        )
        .bind(&f.id)
        .bind(&f.application_id)
        .bind(f.address.application())
        .bind(f.address.service())
        .bind(f.address.name())
        .bind(f.owner.client_id_or_none())
        .bind(f.runtime.as_str())
        .bind(&f.description)
        .bind(f.status.as_str())
        .bind(f.created_at)
        .bind(f.updated_at)
        .execute(&mut **tx.inner)
        .await?;

        let aliases: Vec<&str> = f.aliases.iter().map(|a| a.alias.as_str()).collect();
        sqlx::query("DELETE FROM fn_aliases WHERE function_id = $1 AND NOT (alias = ANY($2))")
            .bind(&f.id)
            .bind(&aliases)
            .execute(&mut **tx.inner)
            .await?;

        if !f.aliases.is_empty() {
            let version_ids: Vec<&str> = f.aliases.iter().map(|a| a.version_id.as_str()).collect();
            let updated_by: Vec<&str> = f.aliases.iter().map(|a| a.updated_by.as_str()).collect();
            let updated_at: Vec<DateTime<Utc>> = f.aliases.iter().map(|a| a.updated_at).collect();
            sqlx::query(
                "INSERT INTO fn_aliases (function_id, alias, version_id, updated_by, updated_at) \
                 SELECT $1, * FROM UNNEST($2::text[], $3::text[], $4::text[], $5::timestamptz[]) \
                 ON CONFLICT (function_id, alias) DO UPDATE SET \
                    version_id = EXCLUDED.version_id, \
                    updated_by = EXCLUDED.updated_by, \
                    updated_at = EXCLUDED.updated_at",
            )
            .bind(&f.id)
            .bind(&aliases)
            .bind(&version_ids)
            .bind(&updated_by)
            .bind(&updated_at)
            .execute(&mut **tx.inner)
            .await?;
        }
        Ok(())
    }

    /// Versions, aliases, routes, config, secrets and trigger-object links
    /// cascade by foreign key.
    async fn delete(&self, f: &Function, tx: &mut DbTx<'_>) -> Result<()> {
        sqlx::query("DELETE FROM fn_functions WHERE id = $1")
            .bind(&f.id)
            .execute(&mut **tx.inner)
            .await?;
        Ok(())
    }
}
