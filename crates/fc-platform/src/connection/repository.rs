//! Connection Repository — PostgreSQL via SeaORM

use async_trait::async_trait;
use sea_orm::*;
use sea_orm::sea_query::OnConflict;
use chrono::Utc;

use super::entity::Connection;
use crate::entities::msg_connections;
use crate::shared::error::Result;
use crate::usecase::unit_of_work::{HasId, PgPersist};

pub struct ConnectionRepository {
    db: DatabaseConnection,
}

impl ConnectionRepository {
    pub fn new(db: &DatabaseConnection) -> Self {
        Self { db: db.clone() }
    }

    pub async fn insert(&self, conn: &Connection) -> Result<()> {
        let model = to_active_model(conn);
        msg_connections::Entity::insert(model).exec(&self.db).await?;
        Ok(())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<Connection>> {
        let result = msg_connections::Entity::find_by_id(id).one(&self.db).await?;
        Ok(result.map(Connection::from))
    }

    pub async fn find_by_code_and_client(&self, code: &str, client_id: Option<&str>) -> Result<Option<Connection>> {
        let mut q = msg_connections::Entity::find()
            .filter(msg_connections::Column::Code.eq(code));
        if let Some(cid) = client_id {
            q = q.filter(msg_connections::Column::ClientId.eq(cid));
        } else {
            q = q.filter(msg_connections::Column::ClientId.is_null());
        }
        Ok(q.one(&self.db).await?.map(Connection::from))
    }

    pub async fn find_all(&self) -> Result<Vec<Connection>> {
        let results = msg_connections::Entity::find()
            .order_by_asc(msg_connections::Column::Code)
            .all(&self.db)
            .await?;
        Ok(results.into_iter().map(Connection::from).collect())
    }

    pub async fn find_with_filters(
        &self,
        client_id: Option<&str>,
        status: Option<&str>,
        service_account_id: Option<&str>,
    ) -> Result<Vec<Connection>> {
        let mut q = msg_connections::Entity::find();
        if let Some(cid) = client_id {
            q = q.filter(msg_connections::Column::ClientId.eq(cid));
        }
        if let Some(s) = status {
            q = q.filter(msg_connections::Column::Status.eq(s));
        }
        if let Some(sa) = service_account_id {
            q = q.filter(msg_connections::Column::ServiceAccountId.eq(sa));
        }
        let results = q.order_by_asc(msg_connections::Column::Code).all(&self.db).await?;
        Ok(results.into_iter().map(Connection::from).collect())
    }

    pub async fn find_by_status(&self, status: &str) -> Result<Vec<Connection>> {
        let results = msg_connections::Entity::find()
            .filter(msg_connections::Column::Status.eq(status))
            .order_by_asc(msg_connections::Column::Code)
            .all(&self.db)
            .await?;
        Ok(results.into_iter().map(Connection::from).collect())
    }

    pub async fn find_by_client_id(&self, client_id: &str) -> Result<Vec<Connection>> {
        let results = msg_connections::Entity::find()
            .filter(msg_connections::Column::ClientId.eq(client_id))
            .order_by_asc(msg_connections::Column::Code)
            .all(&self.db)
            .await?;
        Ok(results.into_iter().map(Connection::from).collect())
    }

    pub async fn find_by_service_account(&self, service_account_id: &str) -> Result<Vec<Connection>> {
        let results = msg_connections::Entity::find()
            .filter(msg_connections::Column::ServiceAccountId.eq(service_account_id))
            .all(&self.db)
            .await?;
        Ok(results.into_iter().map(Connection::from).collect())
    }

    pub async fn update(&self, conn: &Connection) -> Result<()> {
        let model = msg_connections::ActiveModel {
            id: Set(conn.id.clone()),
            code: Set(conn.code.clone()),
            name: Set(conn.name.clone()),
            description: Set(conn.description.clone()),
            endpoint: Set(conn.endpoint.clone()),
            external_id: Set(conn.external_id.clone()),
            status: Set(conn.status.as_str().to_string()),
            service_account_id: Set(conn.service_account_id.clone()),
            client_id: Set(conn.client_id.clone()),
            client_identifier: Set(conn.client_identifier.clone()),
            created_at: NotSet,
            updated_at: Set(Utc::now().into()),
        };
        msg_connections::Entity::update(model).exec(&self.db).await?;
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        let result = msg_connections::Entity::delete_by_id(id).exec(&self.db).await?;
        Ok(result.rows_affected > 0)
    }
}

fn to_active_model(conn: &Connection) -> msg_connections::ActiveModel {
    msg_connections::ActiveModel {
        id: Set(conn.id.clone()),
        code: Set(conn.code.clone()),
        name: Set(conn.name.clone()),
        description: Set(conn.description.clone()),
        endpoint: Set(conn.endpoint.clone()),
        external_id: Set(conn.external_id.clone()),
        status: Set(conn.status.as_str().to_string()),
        service_account_id: Set(conn.service_account_id.clone()),
        client_id: Set(conn.client_id.clone()),
        client_identifier: Set(conn.client_identifier.clone()),
        created_at: Set(Utc::now().into()),
        updated_at: Set(Utc::now().into()),
    }
}

impl HasId for Connection {
    fn id(&self) -> &str { &self.id }
}

#[async_trait]
impl PgPersist for Connection {
    async fn pg_upsert(&self, txn: &DatabaseTransaction) -> Result<()> {
        let model = to_active_model(self);
        msg_connections::Entity::insert(model)
            .on_conflict(
                OnConflict::column(msg_connections::Column::Id)
                    .update_columns([
                        msg_connections::Column::Code,
                        msg_connections::Column::Name,
                        msg_connections::Column::Description,
                        msg_connections::Column::Endpoint,
                        msg_connections::Column::ExternalId,
                        msg_connections::Column::Status,
                        msg_connections::Column::ServiceAccountId,
                        msg_connections::Column::ClientId,
                        msg_connections::Column::ClientIdentifier,
                        msg_connections::Column::UpdatedAt,
                    ])
                    .to_owned(),
            )
            .exec(txn)
            .await?;
        Ok(())
    }

    async fn pg_delete(&self, txn: &DatabaseTransaction) -> Result<()> {
        msg_connections::Entity::delete_by_id(&self.id).exec(txn).await?;
        Ok(())
    }
}
