//! Client Repository
//!
//! PostgreSQL persistence for Client entities using SeaORM.

use async_trait::async_trait;
use sea_orm::*;
use sea_orm::prelude::Expr;
use sea_orm::sea_query::OnConflict;
use chrono::Utc;

use super::entity::{Client, ClientStatus};
use crate::entities::tnt_clients;
use crate::shared::error::Result;
use crate::usecase::unit_of_work::{HasId, PgPersist};

pub struct ClientRepository {
    db: DatabaseConnection,
}

impl ClientRepository {
    pub fn new(db: &DatabaseConnection) -> Self {
        Self { db: db.clone() }
    }

    pub async fn insert(&self, client: &Client) -> Result<()> {
        let notes_json = serde_json::to_value(&client.notes).unwrap_or_default();

        let model = tnt_clients::ActiveModel {
            id: Set(client.id.clone()),
            name: Set(client.name.clone()),
            identifier: Set(client.identifier.clone()),
            status: Set(client.status.as_str().to_string()),
            status_reason: Set(client.status_reason.clone()),
            status_changed_at: Set(client.status_changed_at.map(|dt| dt.into())),
            notes: Set(Some(notes_json)),
            created_at: Set(Utc::now().into()),
            updated_at: Set(Utc::now().into()),
        };

        tnt_clients::Entity::insert(model)
            .exec(&self.db)
            .await?;
        Ok(())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<Client>> {
        let result = tnt_clients::Entity::find_by_id(id)
            .one(&self.db)
            .await?;
        Ok(result.map(Client::from))
    }

    pub async fn find_by_identifier(&self, identifier: &str) -> Result<Option<Client>> {
        let result = tnt_clients::Entity::find()
            .filter(tnt_clients::Column::Identifier.eq(identifier))
            .one(&self.db)
            .await?;
        Ok(result.map(Client::from))
    }

    pub async fn find_active(&self) -> Result<Vec<Client>> {
        let results = tnt_clients::Entity::find()
            .filter(tnt_clients::Column::Status.eq("ACTIVE"))
            .all(&self.db)
            .await?;
        Ok(results.into_iter().map(Client::from).collect())
    }

    pub async fn find_all(&self) -> Result<Vec<Client>> {
        let results = tnt_clients::Entity::find()
            .all(&self.db)
            .await?;
        Ok(results.into_iter().map(Client::from).collect())
    }

    /// Search clients by name or identifier (case-insensitive partial match)
    pub async fn search(&self, term: &str) -> Result<Vec<Client>> {
        let pattern = format!("%{}%", term);
        let results = tnt_clients::Entity::find()
            .filter(
                Condition::any()
                    .add(Expr::col(tnt_clients::Column::Name).like(&pattern))
                    .add(Expr::col(tnt_clients::Column::Identifier).like(&pattern))
            )
            .all(&self.db)
            .await?;
        Ok(results.into_iter().map(Client::from).collect())
    }

    pub async fn find_by_status(&self, status: ClientStatus) -> Result<Vec<Client>> {
        let results = tnt_clients::Entity::find()
            .filter(tnt_clients::Column::Status.eq(status.as_str()))
            .all(&self.db)
            .await?;
        Ok(results.into_iter().map(Client::from).collect())
    }

    pub async fn find_by_ids(&self, ids: &[String]) -> Result<Vec<Client>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let results = tnt_clients::Entity::find()
            .filter(tnt_clients::Column::Id.is_in(ids.to_vec()))
            .all(&self.db)
            .await?;
        Ok(results.into_iter().map(Client::from).collect())
    }

    pub async fn exists(&self, id: &str) -> Result<bool> {
        let count = tnt_clients::Entity::find_by_id(id)
            .count(&self.db)
            .await?;
        Ok(count > 0)
    }

    pub async fn exists_by_identifier(&self, identifier: &str) -> Result<bool> {
        let count = tnt_clients::Entity::find()
            .filter(tnt_clients::Column::Identifier.eq(identifier))
            .count(&self.db)
            .await?;
        Ok(count > 0)
    }

    pub async fn update(&self, client: &Client) -> Result<()> {
        let notes_json = serde_json::to_value(&client.notes).unwrap_or_default();

        let model = tnt_clients::ActiveModel {
            id: Set(client.id.clone()),
            name: Set(client.name.clone()),
            identifier: Set(client.identifier.clone()),
            status: Set(client.status.as_str().to_string()),
            status_reason: Set(client.status_reason.clone()),
            status_changed_at: Set(client.status_changed_at.map(|dt| dt.into())),
            notes: Set(Some(notes_json)),
            created_at: NotSet,
            updated_at: Set(Utc::now().into()),
        };

        tnt_clients::Entity::update(model)
            .exec(&self.db)
            .await?;
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        let result = tnt_clients::Entity::delete_by_id(id)
            .exec(&self.db)
            .await?;
        Ok(result.rows_affected > 0)
    }
}

// ── PgPersist implementation ──────────────────────────────────────────────────

impl HasId for Client {
    fn id(&self) -> &str { &self.id }
}

#[async_trait]
impl PgPersist for Client {
    async fn pg_upsert(&self, txn: &sea_orm::DatabaseTransaction) -> Result<()> {
        let notes_json = serde_json::to_value(&self.notes).unwrap_or_default();
        let model = tnt_clients::ActiveModel {
            id: Set(self.id.clone()),
            name: Set(self.name.clone()),
            identifier: Set(self.identifier.clone()),
            status: Set(self.status.as_str().to_string()),
            status_reason: Set(self.status_reason.clone()),
            status_changed_at: Set(self.status_changed_at.map(|dt| dt.into())),
            notes: Set(Some(notes_json)),
            created_at: Set(Utc::now().into()),
            updated_at: Set(Utc::now().into()),
        };
        tnt_clients::Entity::insert(model)
            .on_conflict(
                OnConflict::column(tnt_clients::Column::Id)
                    .update_columns([
                        tnt_clients::Column::Name,
                        tnt_clients::Column::Identifier,
                        tnt_clients::Column::Status,
                        tnt_clients::Column::StatusReason,
                        tnt_clients::Column::StatusChangedAt,
                        tnt_clients::Column::Notes,
                        tnt_clients::Column::UpdatedAt,
                    ])
                    .to_owned(),
            )
            .exec(txn)
            .await?;
        Ok(())
    }

    async fn pg_delete(&self, txn: &sea_orm::DatabaseTransaction) -> Result<()> {
        tnt_clients::Entity::delete_by_id(&self.id).exec(txn).await?;
        Ok(())
    }
}
