//! PlatformConfigAccess Repository — PostgreSQL via SeaORM

use sea_orm::*;
use chrono::Utc;

use super::access_entity::PlatformConfigAccess;
use crate::entities::app_platform_config_access;
use crate::shared::error::Result;

pub struct PlatformConfigAccessRepository {
    db: DatabaseConnection,
}

impl PlatformConfigAccessRepository {
    pub fn new(db: &DatabaseConnection) -> Self {
        Self { db: db.clone() }
    }

    pub async fn find_by_application(&self, app_code: &str) -> Result<Vec<PlatformConfigAccess>> {
        let results = app_platform_config_access::Entity::find()
            .filter(app_platform_config_access::Column::ApplicationCode.eq(app_code))
            .order_by_asc(app_platform_config_access::Column::RoleCode)
            .all(&self.db)
            .await?;
        Ok(results.into_iter().map(PlatformConfigAccess::from).collect())
    }

    pub async fn find_by_application_and_role(&self, app_code: &str, role_code: &str) -> Result<Option<PlatformConfigAccess>> {
        let result = app_platform_config_access::Entity::find()
            .filter(app_platform_config_access::Column::ApplicationCode.eq(app_code))
            .filter(app_platform_config_access::Column::RoleCode.eq(role_code))
            .one(&self.db)
            .await?;
        Ok(result.map(PlatformConfigAccess::from))
    }

    pub async fn find_by_role_codes(&self, app_code: &str, role_codes: &[String]) -> Result<Vec<PlatformConfigAccess>> {
        let results = app_platform_config_access::Entity::find()
            .filter(app_platform_config_access::Column::ApplicationCode.eq(app_code))
            .filter(app_platform_config_access::Column::RoleCode.is_in(role_codes.iter().cloned()))
            .all(&self.db)
            .await?;
        Ok(results.into_iter().map(PlatformConfigAccess::from).collect())
    }

    pub async fn insert(&self, access: &PlatformConfigAccess) -> Result<()> {
        let model = app_platform_config_access::ActiveModel {
            id: Set(access.id.clone()),
            application_code: Set(access.application_code.clone()),
            role_code: Set(access.role_code.clone()),
            can_read: Set(access.can_read),
            can_write: Set(access.can_write),
            created_at: Set(Utc::now().into()),
        };
        app_platform_config_access::Entity::insert(model).exec(&self.db).await?;
        Ok(())
    }

    pub async fn update(&self, access: &PlatformConfigAccess) -> Result<()> {
        let model = app_platform_config_access::ActiveModel {
            id: Set(access.id.clone()),
            application_code: Set(access.application_code.clone()),
            role_code: Set(access.role_code.clone()),
            can_read: Set(access.can_read),
            can_write: Set(access.can_write),
            created_at: NotSet,
        };
        app_platform_config_access::Entity::update(model).exec(&self.db).await?;
        Ok(())
    }

    pub async fn delete_by_application_and_role(&self, app_code: &str, role_code: &str) -> Result<bool> {
        let access = self.find_by_application_and_role(app_code, role_code).await?;
        if let Some(a) = access {
            let result = app_platform_config_access::Entity::delete_by_id(&a.id).exec(&self.db).await?;
            Ok(result.rows_affected > 0)
        } else {
            Ok(false)
        }
    }
}
