//! The two reads behind the router-config document (Go
//! `internal/platform/dispatch/document.go` loadPools/loadTenants).

use crate::subscription::entity::SubscriptionStatus;
use sqlx::PgPool;

use fc_platform_core::shared::error::Result;

/// A pool as the router sees it: every status.
#[derive(Debug, Clone)]
pub struct RouterPoolRow {
    pub code: String,
    pub client_identifier: Option<String>,
    pub concurrency: i32,
    pub rate_limit: Option<i32>,
}

/// An ACTIVE subscription's tenant and queue.
#[derive(Debug, Clone)]
pub struct RouterSubscriptionRow {
    pub client_identifier: Option<String>,
    pub queue: Option<String>,
}

pub struct RouterConfigRepository {
    pool: PgPool,
}

impl RouterConfigRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn pools(&self) -> Result<Vec<RouterPoolRow>> {
        Ok(sqlx::query_as!(
            RouterPoolRow,
            "SELECT code, client_identifier, concurrency, rate_limit \
                    \
             FROM msg_dispatch_pools ORDER BY code"
        )
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn active_subscriptions(&self) -> Result<Vec<RouterSubscriptionRow>> {
        Ok(sqlx::query_as!(
            RouterSubscriptionRow,
            "SELECT client_identifier, queue \
                    FROM msg_subscriptions \
             WHERE status = $1 ORDER BY id",
            SubscriptionStatus::Active as SubscriptionStatus
        )
        .fetch_all(&self.pool)
        .await?)
    }

    /// Every client's identifier: each is a tenant the scheduler can
    /// publish a client-scoped job under (its `PoolCodeResolver` reads
    /// `tnt_clients` whatever the client's status).
    pub async fn client_identifiers(&self) -> Result<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query!(
            "SELECT identifier FROM tnt_clients WHERE identifier <> '' ORDER BY identifier"
        )
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|r| (r.identifier,))
        .collect();
        Ok(rows.into_iter().map(|(i,)| i).collect())
    }
}
