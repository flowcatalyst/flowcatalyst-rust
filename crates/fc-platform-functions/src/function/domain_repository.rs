//! `fnr_domains` (Java `function/FunctionDomainRepository.java`).

use chrono::{DateTime, Utc};
use fc_platform_core::shared::id::ClientId;
use fc_platform_core::shared::id::FunctionDomainId;
use fc_platform_core::shared::id::OptionIdExt;
use sqlx::PgPool;

use super::entity::FunctionDomain;
use super::{FunctionOwner, Hostname};
use fc_platform_core::shared::enum_str::corrupt_value;
use fc_platform_core::shared::error::Result;
use fc_platform_core::usecase::{DbTx, Persist};

struct DomainRow {
    id: FunctionDomainId,
    client_id: Option<ClientId>,
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

    pub async fn find_by_id(&self, id: &FunctionDomainId) -> Result<Option<FunctionDomain>> {
        let row = sqlx::query_as!(
            DomainRow,
            "SELECT id AS \"id: FunctionDomainId\", client_id AS \"client_id: ClientId\", \
                    hostname, created_at \
                    FROM fnr_domains WHERE id = $1",
            id as &FunctionDomainId
        )
        .fetch_optional(&self.pool)
        .await?;
        row.map(to_entity).transpose()
    }

    /// The one claim covering `hostname`: `hostname` itself or its nearest
    /// claimed ancestor zone. At most one exists, since claims never nest.
    /// One query over every candidate apex; the most specific wins.
    pub async fn covering(&self, hostname: &Hostname) -> Result<Option<FunctionDomain>> {
        let candidates = hostname.zone_candidates();
        let mut rows = sqlx::query_as!(
            DomainRow,
            "SELECT id AS \"id: FunctionDomainId\", client_id AS \"client_id: ClientId\", \
                    hostname, created_at \
                    FROM fnr_domains WHERE hostname = ANY($1)",
            &candidates
        )
        .fetch_all(&self.pool)
        .await?;
        for candidate in &candidates {
            if let Some(at) = rows.iter().position(|r| &r.hostname == candidate) {
                return to_entity(rows.swap_remove(at)).map(Some);
            }
        }
        Ok(None)
    }

    /// [`Self::covering`] for each of `hostnames`, in order, in one query.
    pub async fn covering_each(
        &self,
        hostnames: &[&Hostname],
    ) -> Result<Vec<Option<FunctionDomain>>> {
        if hostnames.is_empty() {
            return Ok(Vec::new());
        }
        let candidates: Vec<Vec<String>> = hostnames.iter().map(|h| h.zone_candidates()).collect();
        let all: Vec<&String> = candidates.iter().flatten().collect();
        let rows = sqlx::query_as!(
            DomainRow,
            "SELECT id AS \"id: FunctionDomainId\", client_id AS \"client_id: ClientId\", \
                    hostname, created_at \
                    FROM fnr_domains WHERE hostname = ANY($1)",
            &all as &[&String]
        )
        .fetch_all(&self.pool)
        .await?;
        let claims: Vec<FunctionDomain> = rows.into_iter().map(to_entity).collect::<Result<_>>()?;
        Ok(candidates
            .iter()
            .map(|zones| {
                zones
                    .iter()
                    .find_map(|zone| claims.iter().find(|d| d.hostname.value() == zone).cloned())
            })
            .collect())
    }

    /// Whether any claim, by any owner, is strictly under `zone` (never
    /// `zone` itself).
    pub async fn any_under(&self, zone: &Hostname) -> Result<bool> {
        let suffix = format!(".{}", zone.value());
        let exists = sqlx::query_scalar!(
            "SELECT EXISTS (SELECT 1 FROM fnr_domains WHERE right(hostname, char_length($1)) = $1) AS \"exists!\"",
            &suffix
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(exists)
    }

    /// One owner's claims, by hostname.
    pub async fn list_by_owner(&self, owner: &FunctionOwner) -> Result<Vec<FunctionDomain>> {
        let rows = sqlx::query_as!(
            DomainRow,
            "SELECT id AS \"id: FunctionDomainId\", client_id AS \"client_id: ClientId\", \
                    hostname, created_at \
                    FROM fnr_domains \
             WHERE client_id IS NOT DISTINCT FROM $1 ORDER BY hostname ASC",
            owner.client_id_or_none()
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(to_entity).collect()
    }
}

fn to_entity(row: DomainRow) -> Result<FunctionDomain> {
    let owner = FunctionOwner::of_client_id(row.client_id.as_id_str()).map_err(|_| {
        corrupt_value(
            "fnr_domains",
            "client_id",
            row.client_id.as_id_str().unwrap_or(""),
            row.id.as_str(),
        )
    })?;
    let hostname = Hostname::try_parse(&row.hostname)
        .ok_or_else(|| corrupt_value("fnr_domains", "hostname", &row.hostname, row.id.as_str()))?;
    Ok(FunctionDomain {
        id: row.id,
        owner,
        hostname,
        created_at: row.created_at,
    })
}

impl Persist<FunctionDomain> for FunctionDomainRepository {
    /// Insert-only: a claim never changes once made.
    async fn persist(&self, d: &FunctionDomain, tx: &mut DbTx<'_>) -> Result<()> {
        sqlx::query!(
            "INSERT INTO fnr_domains (id, client_id, hostname, created_at) VALUES ($1, $2, $3, $4) \
             ON CONFLICT (id) DO NOTHING",
            &d.id as &FunctionDomainId,
            d.owner.client_id_or_none(),
            d.hostname.value(),
            d.created_at
        )
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }

    async fn delete(&self, d: &FunctionDomain, tx: &mut DbTx<'_>) -> Result<()> {
        sqlx::query!(
            "DELETE FROM fnr_domains WHERE id = $1",
            &d.id as &FunctionDomainId
        )
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }
}
