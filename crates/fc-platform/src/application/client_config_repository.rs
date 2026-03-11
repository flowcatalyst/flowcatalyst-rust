//! ApplicationClientConfig Repository — PostgreSQL via SeaORM

use async_trait::async_trait;
use sea_orm::*;
use sea_orm::sea_query::OnConflict;
use chrono::Utc;

use super::client_config::ApplicationClientConfig;
use crate::entities::app_client_configs;
use crate::shared::error::Result;
use crate::usecase::unit_of_work::{HasId, PgPersist};

pub struct ApplicationClientConfigRepository {
    db: DatabaseConnection,
}

impl ApplicationClientConfigRepository {
    pub fn new(db: &DatabaseConnection) -> Self {
        Self { db: db.clone() }
    }

    pub async fn insert(&self, config: &ApplicationClientConfig) -> Result<()> {
        let model = app_client_configs::ActiveModel {
            id: Set(config.id.clone()),
            application_id: Set(config.application_id.clone()),
            client_id: Set(config.client_id.clone()),
            enabled: Set(config.enabled),
            created_at: Set(Utc::now().into()),
            updated_at: Set(Utc::now().into()),
        };
        app_client_configs::Entity::insert(model).exec(&self.db).await?;
        Ok(())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<ApplicationClientConfig>> {
        let result = app_client_configs::Entity::find_by_id(id).one(&self.db).await?;
        Ok(result.map(ApplicationClientConfig::from))
    }

    pub async fn find_by_application(&self, application_id: &str) -> Result<Vec<ApplicationClientConfig>> {
        let results = app_client_configs::Entity::find()
            .filter(app_client_configs::Column::ApplicationId.eq(application_id))
            .all(&self.db)
            .await?;
        Ok(results.into_iter().map(ApplicationClientConfig::from).collect())
    }

    pub async fn find_by_client(&self, client_id: &str) -> Result<Vec<ApplicationClientConfig>> {
        let results = app_client_configs::Entity::find()
            .filter(app_client_configs::Column::ClientId.eq(client_id))
            .all(&self.db)
            .await?;
        Ok(results.into_iter().map(ApplicationClientConfig::from).collect())
    }

    pub async fn find_by_application_and_client(
        &self,
        application_id: &str,
        client_id: &str,
    ) -> Result<Option<ApplicationClientConfig>> {
        let result = app_client_configs::Entity::find()
            .filter(app_client_configs::Column::ApplicationId.eq(application_id))
            .filter(app_client_configs::Column::ClientId.eq(client_id))
            .one(&self.db)
            .await?;
        Ok(result.map(ApplicationClientConfig::from))
    }

    pub async fn find_enabled_for_client(&self, client_id: &str) -> Result<Vec<ApplicationClientConfig>> {
        let results = app_client_configs::Entity::find()
            .filter(app_client_configs::Column::ClientId.eq(client_id))
            .filter(app_client_configs::Column::Enabled.eq(true))
            .all(&self.db)
            .await?;
        Ok(results.into_iter().map(ApplicationClientConfig::from).collect())
    }

    pub async fn enable_for_client(&self, application_id: &str, client_id: &str) -> Result<ApplicationClientConfig> {
        // Check if exists
        let existing = self.find_by_application_and_client(application_id, client_id).await?;
        if let Some(config) = existing {
            // Update
            let model = app_client_configs::ActiveModel {
                id: Set(config.id.clone()),
                application_id: NotSet,
                client_id: NotSet,
                enabled: Set(true),
                created_at: NotSet,
                updated_at: Set(Utc::now().into()),
            };
            app_client_configs::Entity::update(model).exec(&self.db).await?;
            Ok(self.find_by_id(&config.id).await?.unwrap())
        } else {
            // Insert new
            let config = ApplicationClientConfig::new(application_id, client_id);
            self.insert(&config).await?;
            Ok(config)
        }
    }

    pub async fn disable_for_client(&self, application_id: &str, client_id: &str) -> Result<bool> {
        let existing = self.find_by_application_and_client(application_id, client_id).await?;
        if let Some(config) = existing {
            let model = app_client_configs::ActiveModel {
                id: Set(config.id),
                application_id: NotSet,
                client_id: NotSet,
                enabled: Set(false),
                created_at: NotSet,
                updated_at: Set(Utc::now().into()),
            };
            app_client_configs::Entity::update(model).exec(&self.db).await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub async fn update(&self, config: &ApplicationClientConfig) -> Result<()> {
        let model = app_client_configs::ActiveModel {
            id: Set(config.id.clone()),
            application_id: Set(config.application_id.clone()),
            client_id: Set(config.client_id.clone()),
            enabled: Set(config.enabled),
            created_at: NotSet,
            updated_at: Set(Utc::now().into()),
        };
        app_client_configs::Entity::update(model).exec(&self.db).await?;
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        let result = app_client_configs::Entity::delete_by_id(id).exec(&self.db).await?;
        Ok(result.rows_affected > 0)
    }

    pub async fn delete_by_application_and_client(
        &self,
        application_id: &str,
        client_id: &str,
    ) -> Result<bool> {
        let result = app_client_configs::Entity::delete_many()
            .filter(app_client_configs::Column::ApplicationId.eq(application_id))
            .filter(app_client_configs::Column::ClientId.eq(client_id))
            .exec(&self.db)
            .await?;
        Ok(result.rows_affected > 0)
    }
}

// ── PgPersist for ApplicationClientConfig ────────────────────────────────────

impl HasId for ApplicationClientConfig {
    fn id(&self) -> &str { &self.id }
}

#[async_trait]
impl PgPersist for ApplicationClientConfig {
    async fn pg_upsert(&self, txn: &DatabaseTransaction) -> Result<()> {
        let model = app_client_configs::ActiveModel {
            id: Set(self.id.clone()),
            application_id: Set(self.application_id.clone()),
            client_id: Set(self.client_id.clone()),
            enabled: Set(self.enabled),
            created_at: Set(Utc::now().into()),
            updated_at: Set(Utc::now().into()),
        };
        app_client_configs::Entity::insert(model)
            .on_conflict(
                OnConflict::column(app_client_configs::Column::Id)
                    .update_columns([
                        app_client_configs::Column::Enabled,
                        app_client_configs::Column::UpdatedAt,
                    ])
                    .to_owned(),
            )
            .exec(txn)
            .await?;
        Ok(())
    }

    async fn pg_delete(&self, txn: &DatabaseTransaction) -> Result<()> {
        app_client_configs::Entity::delete_by_id(&self.id).exec(txn).await?;
        Ok(())
    }
}
