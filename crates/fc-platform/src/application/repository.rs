//! Application Repository — PostgreSQL via SeaORM

use async_trait::async_trait;
use sea_orm::*;
use sea_orm::sea_query::OnConflict;
use chrono::Utc;

use super::entity::{Application, ApplicationType};
use crate::entities::app_applications;
use crate::shared::error::Result;
use crate::usecase::unit_of_work::{HasId, PgPersist};

pub struct ApplicationRepository {
    db: DatabaseConnection,
}

impl ApplicationRepository {
    pub fn new(db: &DatabaseConnection) -> Self {
        Self { db: db.clone() }
    }

    pub async fn insert(&self, app: &Application) -> Result<()> {
        let model = app_applications::ActiveModel {
            id: Set(app.id.clone()),
            r#type: Set(app.application_type.as_str().to_string()),
            code: Set(app.code.clone()),
            name: Set(app.name.clone()),
            description: Set(app.description.clone()),
            icon_url: Set(app.icon_url.clone()),
            website: Set(app.website.clone()),
            logo: Set(app.logo.clone()),
            logo_mime_type: Set(app.logo_mime_type.clone()),
            default_base_url: Set(app.default_base_url.clone()),
            service_account_id: Set(app.service_account_id.clone()),
            active: Set(app.active),
            created_at: Set(Utc::now().into()),
            updated_at: Set(Utc::now().into()),
        };
        app_applications::Entity::insert(model).exec(&self.db).await?;
        Ok(())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<Application>> {
        let result = app_applications::Entity::find_by_id(id).one(&self.db).await?;
        Ok(result.map(Application::from))
    }

    pub async fn find_by_code(&self, code: &str) -> Result<Option<Application>> {
        let result = app_applications::Entity::find()
            .filter(app_applications::Column::Code.eq(code))
            .one(&self.db)
            .await?;
        Ok(result.map(Application::from))
    }

    pub async fn find_active(&self) -> Result<Vec<Application>> {
        let results = app_applications::Entity::find()
            .filter(app_applications::Column::Active.eq(true))
            .all(&self.db)
            .await?;
        Ok(results.into_iter().map(Application::from).collect())
    }

    pub async fn find_all(&self) -> Result<Vec<Application>> {
        let results = app_applications::Entity::find().all(&self.db).await?;
        Ok(results.into_iter().map(Application::from).collect())
    }

    pub async fn find_paged(&self, page: u64, page_size: u64) -> Result<PagedResult<Application>> {
        let paginator = app_applications::Entity::find()
            .order_by_asc(app_applications::Column::Code)
            .paginate(&self.db, page_size);
        let total = paginator.num_items().await?;
        let items = paginator.fetch_page(page).await?;
        Ok(PagedResult {
            items: items.into_iter().map(Application::from).collect(),
            total,
            page,
            page_size,
        })
    }

    pub async fn find_by_type(&self, app_type: ApplicationType) -> Result<Vec<Application>> {
        let results = app_applications::Entity::find()
            .filter(app_applications::Column::Type.eq(app_type.as_str()))
            .filter(app_applications::Column::Active.eq(true))
            .all(&self.db)
            .await?;
        Ok(results.into_iter().map(Application::from).collect())
    }

    pub async fn find_by_service_account(&self, service_account_id: &str) -> Result<Option<Application>> {
        let result = app_applications::Entity::find()
            .filter(app_applications::Column::ServiceAccountId.eq(service_account_id))
            .one(&self.db)
            .await?;
        Ok(result.map(Application::from))
    }

    pub async fn exists(&self, id: &str) -> Result<bool> {
        let count = app_applications::Entity::find_by_id(id).count(&self.db).await?;
        Ok(count > 0)
    }

    pub async fn exists_by_code(&self, code: &str) -> Result<bool> {
        let count = app_applications::Entity::find()
            .filter(app_applications::Column::Code.eq(code))
            .count(&self.db)
            .await?;
        Ok(count > 0)
    }

    pub async fn update(&self, app: &Application) -> Result<Application> {
        let model = app_applications::ActiveModel {
            id: Set(app.id.clone()),
            r#type: Set(app.application_type.as_str().to_string()),
            code: Set(app.code.clone()),
            name: Set(app.name.clone()),
            description: Set(app.description.clone()),
            icon_url: Set(app.icon_url.clone()),
            website: Set(app.website.clone()),
            logo: Set(app.logo.clone()),
            logo_mime_type: Set(app.logo_mime_type.clone()),
            default_base_url: Set(app.default_base_url.clone()),
            service_account_id: Set(app.service_account_id.clone()),
            active: Set(app.active),
            created_at: NotSet,
            updated_at: Set(Utc::now().into()),
        };
        let updated = app_applications::Entity::update(model).exec(&self.db).await?;
        Ok(Application::from(updated))
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        let result = app_applications::Entity::delete_by_id(id).exec(&self.db).await?;
        Ok(result.rows_affected > 0)
    }
}

pub struct PagedResult<T> {
    pub items: Vec<T>,
    pub total: u64,
    pub page: u64,
    pub page_size: u64,
}

// ── PgPersist implementation ──────────────────────────────────────────────────

impl HasId for Application {
    fn id(&self) -> &str { &self.id }
}

#[async_trait]
impl PgPersist for Application {
    async fn pg_upsert(&self, txn: &sea_orm::DatabaseTransaction) -> Result<()> {
        let model = app_applications::ActiveModel {
            id: Set(self.id.clone()),
            r#type: Set(self.application_type.as_str().to_string()),
            code: Set(self.code.clone()),
            name: Set(self.name.clone()),
            description: Set(self.description.clone()),
            icon_url: Set(self.icon_url.clone()),
            website: Set(self.website.clone()),
            logo: Set(self.logo.clone()),
            logo_mime_type: Set(self.logo_mime_type.clone()),
            default_base_url: Set(self.default_base_url.clone()),
            service_account_id: Set(self.service_account_id.clone()),
            active: Set(self.active),
            created_at: Set(Utc::now().into()),
            updated_at: Set(Utc::now().into()),
        };
        app_applications::Entity::insert(model)
            .on_conflict(
                OnConflict::column(app_applications::Column::Id)
                    .update_columns([
                        app_applications::Column::Type,
                        app_applications::Column::Code,
                        app_applications::Column::Name,
                        app_applications::Column::Description,
                        app_applications::Column::IconUrl,
                        app_applications::Column::Website,
                        app_applications::Column::Logo,
                        app_applications::Column::LogoMimeType,
                        app_applications::Column::DefaultBaseUrl,
                        app_applications::Column::ServiceAccountId,
                        app_applications::Column::Active,
                        app_applications::Column::UpdatedAt,
                    ])
                    .to_owned(),
            )
            .exec(txn)
            .await?;
        Ok(())
    }

    async fn pg_delete(&self, txn: &sea_orm::DatabaseTransaction) -> Result<()> {
        app_applications::Entity::delete_by_id(&self.id).exec(txn).await?;
        Ok(())
    }
}
