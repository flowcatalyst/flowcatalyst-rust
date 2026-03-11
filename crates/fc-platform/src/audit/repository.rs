//! Audit Log Repository — PostgreSQL via SeaORM

use sea_orm::*;
use chrono::Utc;

use super::entity::AuditLog;
use crate::entities::aud_logs;
use crate::shared::error::Result;

pub struct AuditLogRepository {
    db: DatabaseConnection,
}

impl AuditLogRepository {
    pub fn new(db: &DatabaseConnection) -> Self {
        Self { db: db.clone() }
    }

    pub async fn insert(&self, log: &AuditLog) -> Result<()> {
        let model = aud_logs::ActiveModel {
            id: Set(log.id.clone()),
            entity_type: Set(log.entity_type.clone()),
            entity_id: Set(log.entity_id.clone()),
            operation: Set(log.operation.clone()),
            operation_json: Set(log.operation_json.clone().map(sea_orm::JsonValue::from)),
            principal_id: Set(log.principal_id.clone()),
            application_id: Set(log.application_id.clone()),
            client_id: Set(log.client_id.clone()),
            performed_at: Set(Utc::now().into()),
        };
        aud_logs::Entity::insert(model).exec(&self.db).await?;
        Ok(())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<AuditLog>> {
        let result = aud_logs::Entity::find_by_id(id)
            .one(&self.db)
            .await?;
        Ok(result.map(AuditLog::from))
    }

    pub async fn find_by_entity(&self, entity_type: &str, entity_id: &str, limit: u64) -> Result<Vec<AuditLog>> {
        let results = aud_logs::Entity::find()
            .filter(aud_logs::Column::EntityType.eq(entity_type))
            .filter(aud_logs::Column::EntityId.eq(entity_id))
            .order_by_desc(aud_logs::Column::PerformedAt)
            .limit(limit)
            .all(&self.db)
            .await?;
        Ok(results.into_iter().map(AuditLog::from).collect())
    }

    pub async fn find_by_principal(&self, principal_id: &str, limit: u64) -> Result<Vec<AuditLog>> {
        let results = aud_logs::Entity::find()
            .filter(aud_logs::Column::PrincipalId.eq(principal_id))
            .order_by_desc(aud_logs::Column::PerformedAt)
            .limit(limit)
            .all(&self.db)
            .await?;
        Ok(results.into_iter().map(AuditLog::from).collect())
    }

    pub async fn find_recent(&self, limit: u64) -> Result<Vec<AuditLog>> {
        let results = aud_logs::Entity::find()
            .order_by_desc(aud_logs::Column::PerformedAt)
            .limit(limit)
            .all(&self.db)
            .await?;
        Ok(results.into_iter().map(AuditLog::from).collect())
    }

    pub async fn search(
        &self,
        entity_type: Option<&str>,
        entity_id: Option<&str>,
        operation: Option<&str>,
        principal_id: Option<&str>,
        skip: u64,
        limit: i64,
    ) -> Result<Vec<AuditLog>> {
        let mut query = aud_logs::Entity::find();
        if let Some(et) = entity_type {
            query = query.filter(aud_logs::Column::EntityType.eq(et));
        }
        if let Some(eid) = entity_id {
            query = query.filter(aud_logs::Column::EntityId.eq(eid));
        }
        if let Some(op) = operation {
            query = query.filter(aud_logs::Column::Operation.eq(op));
        }
        if let Some(pid) = principal_id {
            query = query.filter(aud_logs::Column::PrincipalId.eq(pid));
        }
        let results = query
            .order_by_desc(aud_logs::Column::PerformedAt)
            .offset(skip)
            .limit(limit as u64)
            .all(&self.db)
            .await?;
        Ok(results.into_iter().map(AuditLog::from).collect())
    }

    pub async fn count_with_filters(
        &self,
        entity_type: Option<&str>,
        entity_id: Option<&str>,
        operation: Option<&str>,
        principal_id: Option<&str>,
    ) -> Result<i64> {
        let mut query = aud_logs::Entity::find();
        if let Some(et) = entity_type {
            query = query.filter(aud_logs::Column::EntityType.eq(et));
        }
        if let Some(eid) = entity_id {
            query = query.filter(aud_logs::Column::EntityId.eq(eid));
        }
        if let Some(op) = operation {
            query = query.filter(aud_logs::Column::Operation.eq(op));
        }
        if let Some(pid) = principal_id {
            query = query.filter(aud_logs::Column::PrincipalId.eq(pid));
        }
        let count = query.count(&self.db).await?;
        Ok(count as i64)
    }

    pub async fn find_distinct_entity_types(&self) -> Result<Vec<String>> {
        use sea_orm::QuerySelect;
        let results: Vec<(String,)> = aud_logs::Entity::find()
            .select_only()
            .column(aud_logs::Column::EntityType)
            .group_by(aud_logs::Column::EntityType)
            .order_by_asc(aud_logs::Column::EntityType)
            .into_tuple()
            .all(&self.db)
            .await?;
        Ok(results.into_iter().map(|(t,)| t).collect())
    }

    pub async fn find_distinct_application_ids(&self) -> Result<Vec<String>> {
        use sea_orm::QuerySelect;
        let results: Vec<(Option<String>,)> = aud_logs::Entity::find()
            .select_only()
            .column(aud_logs::Column::ApplicationId)
            .group_by(aud_logs::Column::ApplicationId)
            .order_by_asc(aud_logs::Column::ApplicationId)
            .into_tuple()
            .all(&self.db)
            .await?;
        Ok(results.into_iter().filter_map(|(t,)| t).collect())
    }

    pub async fn find_distinct_client_ids(&self) -> Result<Vec<String>> {
        use sea_orm::QuerySelect;
        let results: Vec<(Option<String>,)> = aud_logs::Entity::find()
            .select_only()
            .column(aud_logs::Column::ClientId)
            .group_by(aud_logs::Column::ClientId)
            .order_by_asc(aud_logs::Column::ClientId)
            .into_tuple()
            .all(&self.db)
            .await?;
        Ok(results.into_iter().filter_map(|(t,)| t).collect())
    }

    pub async fn find_distinct_operations(&self) -> Result<Vec<String>> {
        use sea_orm::QuerySelect;
        let results: Vec<(String,)> = aud_logs::Entity::find()
            .select_only()
            .column(aud_logs::Column::Operation)
            .group_by(aud_logs::Column::Operation)
            .order_by_asc(aud_logs::Column::Operation)
            .into_tuple()
            .all(&self.db)
            .await?;
        Ok(results.into_iter().map(|(t,)| t).collect())
    }
}
