//! DispatchPool Repository — PostgreSQL via SeaORM

use async_trait::async_trait;
use sea_orm::*;
use sea_orm::sea_query::OnConflict;
use chrono::Utc;

use super::entity::{DispatchPool, DispatchPoolStatus};
use crate::entities::msg_dispatch_pools;
use crate::shared::error::Result;
use crate::usecase::unit_of_work::{HasId, PgPersist};

pub struct DispatchPoolRepository {
    db: DatabaseConnection,
}

impl DispatchPoolRepository {
    pub fn new(db: &DatabaseConnection) -> Self {
        Self { db: db.clone() }
    }

    pub async fn insert(&self, pool: &DispatchPool) -> Result<()> {
        let model = msg_dispatch_pools::ActiveModel {
            id: Set(pool.id.clone()),
            code: Set(pool.code.clone()),
            name: Set(pool.name.clone()),
            description: Set(pool.description.clone()),
            rate_limit: Set(pool.rate_limit),
            concurrency: Set(pool.concurrency),
            client_id: Set(pool.client_id.clone()),
            client_identifier: Set(pool.client_identifier.clone()),
            status: Set(pool.status.as_str().to_string()),
            created_at: Set(Utc::now().into()),
            updated_at: Set(Utc::now().into()),
        };
        msg_dispatch_pools::Entity::insert(model).exec(&self.db).await?;
        Ok(())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<DispatchPool>> {
        let result = msg_dispatch_pools::Entity::find_by_id(id).one(&self.db).await?;
        Ok(result.map(DispatchPool::from))
    }

    pub async fn find_by_code(&self, code: &str, client_id: Option<&str>) -> Result<Option<DispatchPool>> {
        let mut q = msg_dispatch_pools::Entity::find()
            .filter(msg_dispatch_pools::Column::Code.eq(code));
        if let Some(cid) = client_id {
            q = q.filter(msg_dispatch_pools::Column::ClientId.eq(cid));
        } else {
            q = q.filter(msg_dispatch_pools::Column::ClientId.is_null());
        }
        Ok(q.one(&self.db).await?.map(DispatchPool::from))
    }

    pub async fn find_all(&self) -> Result<Vec<DispatchPool>> {
        let results = msg_dispatch_pools::Entity::find()
            .order_by_asc(msg_dispatch_pools::Column::Code)
            .all(&self.db)
            .await?;
        Ok(results.into_iter().map(DispatchPool::from).collect())
    }

    pub async fn find_by_status(&self, status: DispatchPoolStatus) -> Result<Vec<DispatchPool>> {
        let results = msg_dispatch_pools::Entity::find()
            .filter(msg_dispatch_pools::Column::Status.eq(status.as_str()))
            .order_by_asc(msg_dispatch_pools::Column::Code)
            .all(&self.db)
            .await?;
        Ok(results.into_iter().map(DispatchPool::from).collect())
    }

    /// Search dispatch pools by code or name (case-insensitive partial match)
    pub async fn search(&self, term: &str) -> Result<Vec<DispatchPool>> {
        let pattern = format!("%{}%", term);
        let results = msg_dispatch_pools::Entity::find()
            .filter(
                Condition::any()
                    .add(msg_dispatch_pools::Column::Code.like(&pattern))
                    .add(msg_dispatch_pools::Column::Name.like(&pattern))
            )
            .all(&self.db)
            .await?;
        Ok(results.into_iter().map(DispatchPool::from).collect())
    }

    pub async fn find_active(&self) -> Result<Vec<DispatchPool>> {
        let results = msg_dispatch_pools::Entity::find()
            .filter(msg_dispatch_pools::Column::Status.eq("ACTIVE"))
            .all(&self.db)
            .await?;
        Ok(results.into_iter().map(DispatchPool::from).collect())
    }

    pub async fn find_by_client(&self, client_id: Option<&str>) -> Result<Vec<DispatchPool>> {
        let results = if let Some(cid) = client_id {
            msg_dispatch_pools::Entity::find()
                .filter(
                    Condition::any()
                        .add(msg_dispatch_pools::Column::ClientId.eq(cid))
                        .add(msg_dispatch_pools::Column::ClientId.is_null()),
                )
                .all(&self.db)
                .await?
        } else {
            msg_dispatch_pools::Entity::find()
                .filter(msg_dispatch_pools::Column::ClientId.is_null())
                .all(&self.db)
                .await?
        };
        Ok(results.into_iter().map(DispatchPool::from).collect())
    }

    pub async fn update(&self, pool: &DispatchPool) -> Result<()> {
        let model = msg_dispatch_pools::ActiveModel {
            id: Set(pool.id.clone()),
            code: Set(pool.code.clone()),
            name: Set(pool.name.clone()),
            description: Set(pool.description.clone()),
            rate_limit: Set(pool.rate_limit),
            concurrency: Set(pool.concurrency),
            client_id: Set(pool.client_id.clone()),
            client_identifier: Set(pool.client_identifier.clone()),
            status: Set(pool.status.as_str().to_string()),
            created_at: NotSet,
            updated_at: Set(Utc::now().into()),
        };
        msg_dispatch_pools::Entity::update(model).exec(&self.db).await?;
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        let result = msg_dispatch_pools::Entity::delete_by_id(id).exec(&self.db).await?;
        Ok(result.rows_affected > 0)
    }
}

// ── PgPersist implementation ──────────────────────────────────────────────────

impl HasId for DispatchPool {
    fn id(&self) -> &str { &self.id }
}

#[async_trait]
impl PgPersist for DispatchPool {
    async fn pg_upsert(&self, txn: &sea_orm::DatabaseTransaction) -> Result<()> {
        let model = msg_dispatch_pools::ActiveModel {
            id: Set(self.id.clone()),
            code: Set(self.code.clone()),
            name: Set(self.name.clone()),
            description: Set(self.description.clone()),
            rate_limit: Set(self.rate_limit),
            concurrency: Set(self.concurrency),
            client_id: Set(self.client_id.clone()),
            client_identifier: Set(self.client_identifier.clone()),
            status: Set(self.status.as_str().to_string()),
            created_at: Set(Utc::now().into()),
            updated_at: Set(Utc::now().into()),
        };
        msg_dispatch_pools::Entity::insert(model)
            .on_conflict(
                OnConflict::column(msg_dispatch_pools::Column::Id)
                    .update_columns([
                        msg_dispatch_pools::Column::Code,
                        msg_dispatch_pools::Column::Name,
                        msg_dispatch_pools::Column::Description,
                        msg_dispatch_pools::Column::RateLimit,
                        msg_dispatch_pools::Column::Concurrency,
                        msg_dispatch_pools::Column::ClientId,
                        msg_dispatch_pools::Column::ClientIdentifier,
                        msg_dispatch_pools::Column::Status,
                        msg_dispatch_pools::Column::UpdatedAt,
                    ])
                    .to_owned(),
            )
            .exec(txn)
            .await?;
        Ok(())
    }

    async fn pg_delete(&self, txn: &sea_orm::DatabaseTransaction) -> Result<()> {
        msg_dispatch_pools::Entity::delete_by_id(&self.id).exec(txn).await?;
        Ok(())
    }
}
