//! PlatformConfig Repository — PostgreSQL via SeaORM

use sea_orm::*;
use chrono::Utc;

use super::entity::PlatformConfig;
use crate::entities::app_platform_configs;
use crate::shared::error::Result;

pub struct PlatformConfigRepository {
    db: DatabaseConnection,
}

impl PlatformConfigRepository {
    pub fn new(db: &DatabaseConnection) -> Self {
        Self { db: db.clone() }
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<PlatformConfig>> {
        let result = app_platform_configs::Entity::find_by_id(id).one(&self.db).await?;
        Ok(result.map(PlatformConfig::from))
    }

    pub async fn find_by_key(
        &self,
        app_code: &str,
        section: &str,
        property: &str,
        scope: &str,
        client_id: Option<&str>,
    ) -> Result<Option<PlatformConfig>> {
        let mut q = app_platform_configs::Entity::find()
            .filter(app_platform_configs::Column::ApplicationCode.eq(app_code))
            .filter(app_platform_configs::Column::Section.eq(section))
            .filter(app_platform_configs::Column::Property.eq(property))
            .filter(app_platform_configs::Column::Scope.eq(scope));
        if let Some(cid) = client_id {
            q = q.filter(app_platform_configs::Column::ClientId.eq(cid));
        } else {
            q = q.filter(app_platform_configs::Column::ClientId.is_null());
        }
        Ok(q.one(&self.db).await?.map(PlatformConfig::from))
    }

    pub async fn find_by_section(
        &self,
        app_code: &str,
        section: &str,
        scope: Option<&str>,
        client_id: Option<&str>,
    ) -> Result<Vec<PlatformConfig>> {
        let mut q = app_platform_configs::Entity::find()
            .filter(app_platform_configs::Column::ApplicationCode.eq(app_code))
            .filter(app_platform_configs::Column::Section.eq(section));
        if let Some(s) = scope {
            q = q.filter(app_platform_configs::Column::Scope.eq(s));
        }
        if let Some(cid) = client_id {
            q = q.filter(app_platform_configs::Column::ClientId.eq(cid));
        }
        let results = q.order_by_asc(app_platform_configs::Column::Property).all(&self.db).await?;
        Ok(results.into_iter().map(PlatformConfig::from).collect())
    }

    pub async fn find_by_application(
        &self,
        app_code: &str,
        scope: Option<&str>,
        client_id: Option<&str>,
    ) -> Result<Vec<PlatformConfig>> {
        let mut q = app_platform_configs::Entity::find()
            .filter(app_platform_configs::Column::ApplicationCode.eq(app_code));
        if let Some(s) = scope {
            q = q.filter(app_platform_configs::Column::Scope.eq(s));
        }
        if let Some(cid) = client_id {
            q = q.filter(app_platform_configs::Column::ClientId.eq(cid));
        }
        let results = q
            .order_by_asc(app_platform_configs::Column::Section)
            .order_by_asc(app_platform_configs::Column::Property)
            .all(&self.db)
            .await?;
        Ok(results.into_iter().map(PlatformConfig::from).collect())
    }

    pub async fn insert(&self, config: &PlatformConfig) -> Result<()> {
        let model = app_platform_configs::ActiveModel {
            id: Set(config.id.clone()),
            application_code: Set(config.application_code.clone()),
            section: Set(config.section.clone()),
            property: Set(config.property.clone()),
            scope: Set(config.scope.as_str().to_string()),
            client_id: Set(config.client_id.clone()),
            value_type: Set(config.value_type.as_str().to_string()),
            value: Set(config.value.clone()),
            description: Set(config.description.clone()),
            created_at: Set(Utc::now().into()),
            updated_at: Set(Utc::now().into()),
        };
        app_platform_configs::Entity::insert(model).exec(&self.db).await?;
        Ok(())
    }

    pub async fn update(&self, config: &PlatformConfig) -> Result<()> {
        let model = app_platform_configs::ActiveModel {
            id: Set(config.id.clone()),
            application_code: Set(config.application_code.clone()),
            section: Set(config.section.clone()),
            property: Set(config.property.clone()),
            scope: Set(config.scope.as_str().to_string()),
            client_id: Set(config.client_id.clone()),
            value_type: Set(config.value_type.as_str().to_string()),
            value: Set(config.value.clone()),
            description: Set(config.description.clone()),
            created_at: NotSet,
            updated_at: Set(Utc::now().into()),
        };
        app_platform_configs::Entity::update(model).exec(&self.db).await?;
        Ok(())
    }

    pub async fn delete_by_key(
        &self,
        app_code: &str,
        section: &str,
        property: &str,
        scope: &str,
        client_id: Option<&str>,
    ) -> Result<bool> {
        let config = self.find_by_key(app_code, section, property, scope, client_id).await?;
        if let Some(c) = config {
            let result = app_platform_configs::Entity::delete_by_id(&c.id).exec(&self.db).await?;
            Ok(result.rows_affected > 0)
        } else {
            Ok(false)
        }
    }
}
