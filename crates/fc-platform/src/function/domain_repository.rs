//! `fn_domains` (Java `function/FunctionDomainRepository.java`).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

use super::entity::FunctionDomain;
use super::{FunctionOwner, Hostname};
use crate::shared::enum_str::corrupt_value;
use crate::shared::error::Result;
use crate::usecase::{DbTx, Persist};

#[derive(sqlx::FromRow)]
struct DomainRow {
    id: String,
    client_id: Option<String>,
    hostname: String,
    created_at: DateTime<Utc>,
}

pub struct FunctionDomainRepository {
    pool: PgPool,
}

impl FunctionDomainRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<FunctionDomain>> {
        let row = sqlx::query_as::<_, DomainRow>(
            "SELECT id, client_id, hostname, created_at FROM fn_domains WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(to_entity).transpose()
    }

    /// The one claim covering `hostname`: `hostname` itself or its nearest
    /// claimed ancestor zone. At most one exists, since claims never nest.
    /// One query over every candidate apex; the most specific wins.
    pub async fn covering(&self, hostname: &Hostname) -> Result<Option<FunctionDomain>> {
        let candidates = hostname.zone_candidates();
        let mut rows = sqlx::query_as::<_, DomainRow>(
            "SELECT id, client_id, hostname, created_at FROM fn_domains WHERE hostname = ANY($1)",
        )
        .bind(&candidates)
        .fetch_all(&self.pool)
        .await?;
        for candidate in &candidates {
            if let Some(at) = rows.iter().position(|r| &r.hostname == candidate) {
                return to_entity(rows.swap_remove(at)).map(Some);
            }
        }
        Ok(None)
    }

    /// Whether any claim, by any owner, is strictly under `zone` (never
    /// `zone` itself).
    pub async fn any_under(&self, zone: &Hostname) -> Result<bool> {
        let suffix = format!(".{}", zone.value());
        let (exists,): (bool,) = sqlx::query_as(
            "SELECT EXISTS (SELECT 1 FROM fn_domains WHERE right(hostname, char_length($1)) = $1)",
        )
        .bind(&suffix)
        .fetch_one(&self.pool)
        .await?;
        Ok(exists)
    }

    /// One owner's claims, by hostname.
    pub async fn list_by_owner(&self, owner: &FunctionOwner) -> Result<Vec<FunctionDomain>> {
        let rows = sqlx::query_as::<_, DomainRow>(
            "SELECT id, client_id, hostname, created_at FROM fn_domains \
             WHERE client_id IS NOT DISTINCT FROM $1 ORDER BY hostname ASC",
        )
        .bind(owner.client_id_or_none())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(to_entity).collect()
    }
}

fn to_entity(row: DomainRow) -> Result<FunctionDomain> {
    let owner = FunctionOwner::of_client_id(row.client_id.as_deref()).map_err(|_| {
        corrupt_value(
            "fn_domains",
            "client_id",
            row.client_id.as_deref().unwrap_or(""),
            &row.id,
        )
    })?;
    let hostname = Hostname::try_parse(&row.hostname)
        .ok_or_else(|| corrupt_value("fn_domains", "hostname", &row.hostname, &row.id))?;
    Ok(FunctionDomain {
        id: row.id,
        owner,
        hostname,
        created_at: row.created_at,
    })
}

#[async_trait]
impl Persist<FunctionDomain> for FunctionDomainRepository {
    /// Insert-only: a claim never changes once made.
    async fn persist(&self, d: &FunctionDomain, tx: &mut DbTx<'_>) -> Result<()> {
        sqlx::query(
            "INSERT INTO fn_domains (id, client_id, hostname, created_at) VALUES ($1, $2, $3, $4) \
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(&d.id)
        .bind(d.owner.client_id_or_none())
        .bind(d.hostname.value())
        .bind(d.created_at)
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }

    async fn delete(&self, d: &FunctionDomain, tx: &mut DbTx<'_>) -> Result<()> {
        sqlx::query("DELETE FROM fn_domains WHERE id = $1")
            .bind(&d.id)
            .execute(&mut **tx.inner)
            .await?;
        Ok(())
    }
}
