//! ServiceAccount Repository
//!
//! PostgreSQL persistence for ServiceAccount entities using SeaORM.
//! Webhook credentials are stored as flat columns (wh_*) on iam_service_accounts.

use sea_orm::*;
use chrono::Utc;

use crate::ServiceAccount;
use crate::entities::iam_service_accounts;
use crate::shared::error::Result;

pub struct ServiceAccountRepository {
    db: DatabaseConnection,
}

impl ServiceAccountRepository {
    pub fn new(db: &DatabaseConnection) -> Self {
        Self { db: db.clone() }
    }

    pub async fn insert(&self, account: &ServiceAccount) -> Result<()> {
        let model = self.to_active_model(account, true);

        iam_service_accounts::Entity::insert(model)
            .exec(&self.db)
            .await?;
        Ok(())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<ServiceAccount>> {
        let result = iam_service_accounts::Entity::find_by_id(id)
            .one(&self.db)
            .await?;
        Ok(result.map(ServiceAccount::from))
    }

    pub async fn find_by_code(&self, code: &str) -> Result<Option<ServiceAccount>> {
        let result = iam_service_accounts::Entity::find()
            .filter(iam_service_accounts::Column::Code.eq(code))
            .one(&self.db)
            .await?;
        Ok(result.map(ServiceAccount::from))
    }

    pub async fn find_active(&self) -> Result<Vec<ServiceAccount>> {
        let results = iam_service_accounts::Entity::find()
            .filter(iam_service_accounts::Column::Active.eq(true))
            .all(&self.db)
            .await?;
        Ok(results.into_iter().map(ServiceAccount::from).collect())
    }

    pub async fn find_by_application(&self, application_id: &str) -> Result<Vec<ServiceAccount>> {
        let results = iam_service_accounts::Entity::find()
            .filter(iam_service_accounts::Column::ApplicationId.eq(application_id))
            .all(&self.db)
            .await?;
        Ok(results.into_iter().map(ServiceAccount::from).collect())
    }

    pub async fn find_by_client(&self, _client_id: &str) -> Result<Vec<ServiceAccount>> {
        // In the PostgreSQL model, client access is tracked via iam_client_access_grants
        // on the linked principal, not directly on the service account.
        // For now, return all active service accounts (filtering happens at API layer)
        self.find_active().await
    }

    pub async fn find_with_role(&self, _role: &str) -> Result<Vec<ServiceAccount>> {
        // Roles are tracked via iam_principal_roles on the linked principal.
        // For now, return all active service accounts
        self.find_active().await
    }

    pub async fn update(&self, account: &ServiceAccount) -> Result<()> {
        let model = self.to_active_model(account, false);

        iam_service_accounts::Entity::update(model)
            .exec(&self.db)
            .await?;
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        let result = iam_service_accounts::Entity::delete_by_id(id)
            .exec(&self.db)
            .await?;
        Ok(result.rows_affected > 0)
    }

    // ── Helpers ──────────────────────────────────────────────

    fn to_active_model(&self, account: &ServiceAccount, is_insert: bool) -> iam_service_accounts::ActiveModel {
        let wh = &account.webhook_credentials;

        iam_service_accounts::ActiveModel {
            id: Set(account.id.clone()),
            code: Set(account.code.clone()),
            name: Set(account.name.clone()),
            description: Set(account.description.clone()),
            application_id: Set(account.application_id.clone()),
            active: Set(account.active),
            wh_auth_type: Set(Some(wh.auth_type.as_str().to_string())),
            wh_auth_token_ref: Set(wh.token.clone()),
            wh_signing_secret_ref: Set(wh.signing_secret.clone()),
            wh_signing_algorithm: Set(wh.signing_algorithm.clone()),
            wh_credentials_created_at: if is_insert {
                Set(Some(Utc::now().into()))
            } else {
                NotSet
            },
            wh_credentials_regenerated_at: NotSet,
            last_used_at: Set(account.last_used_at.map(|dt| dt.into())),
            created_at: if is_insert { Set(Utc::now().into()) } else { NotSet },
            updated_at: Set(Utc::now().into()),
        }
    }
}
