//! CORS Origin Repository — PostgreSQL via SeaORM

use sea_orm::*;
use chrono::Utc;

use super::entity::CorsAllowedOrigin;
use crate::entities::tnt_cors_allowed_origins;
use crate::shared::error::Result;

pub struct CorsOriginRepository {
    db: DatabaseConnection,
}

impl CorsOriginRepository {
    pub fn new(db: &DatabaseConnection) -> Self {
        Self { db: db.clone() }
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<CorsAllowedOrigin>> {
        let result = tnt_cors_allowed_origins::Entity::find_by_id(id).one(&self.db).await?;
        Ok(result.map(CorsAllowedOrigin::from))
    }

    pub async fn find_by_origin(&self, origin: &str) -> Result<Option<CorsAllowedOrigin>> {
        let result = tnt_cors_allowed_origins::Entity::find()
            .filter(tnt_cors_allowed_origins::Column::Origin.eq(origin))
            .one(&self.db)
            .await?;
        Ok(result.map(CorsAllowedOrigin::from))
    }

    pub async fn find_all(&self) -> Result<Vec<CorsAllowedOrigin>> {
        let results = tnt_cors_allowed_origins::Entity::find()
            .order_by_asc(tnt_cors_allowed_origins::Column::Origin)
            .all(&self.db)
            .await?;
        Ok(results.into_iter().map(CorsAllowedOrigin::from).collect())
    }

    pub async fn get_allowed_origins(&self) -> Result<Vec<String>> {
        let results = tnt_cors_allowed_origins::Entity::find()
            .all(&self.db)
            .await?;
        Ok(results.into_iter().map(|m| m.origin).collect())
    }

    pub async fn insert(&self, origin: &CorsAllowedOrigin) -> Result<()> {
        let model = tnt_cors_allowed_origins::ActiveModel {
            id: Set(origin.id.clone()),
            origin: Set(origin.origin.clone()),
            description: Set(origin.description.clone()),
            created_by: Set(origin.created_by.clone()),
            created_at: Set(Utc::now().into()),
            updated_at: Set(Utc::now().into()),
        };
        tnt_cors_allowed_origins::Entity::insert(model).exec(&self.db).await?;
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        let result = tnt_cors_allowed_origins::Entity::delete_by_id(id).exec(&self.db).await?;
        Ok(result.rows_affected > 0)
    }
}
