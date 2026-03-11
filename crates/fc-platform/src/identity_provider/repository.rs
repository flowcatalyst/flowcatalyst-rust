//! IdentityProvider Repository — PostgreSQL via SeaORM

use sea_orm::*;
use chrono::Utc;

use super::entity::IdentityProvider;
use crate::entities::{oauth_identity_providers, oauth_identity_provider_allowed_domains};
use crate::shared::error::Result;

pub struct IdentityProviderRepository {
    db: DatabaseConnection,
}

impl IdentityProviderRepository {
    pub fn new(db: &DatabaseConnection) -> Self {
        Self { db: db.clone() }
    }

    async fn load_allowed_domains(&self, idp_id: &str) -> Result<Vec<String>> {
        let domains = oauth_identity_provider_allowed_domains::Entity::find()
            .filter(oauth_identity_provider_allowed_domains::Column::IdentityProviderId.eq(idp_id))
            .all(&self.db)
            .await?;
        Ok(domains.into_iter().map(|d| d.email_domain).collect())
    }

    async fn hydrate(&self, model: oauth_identity_providers::Model) -> Result<IdentityProvider> {
        let id = model.id.clone();
        let mut idp = IdentityProvider::from(model);
        idp.allowed_email_domains = self.load_allowed_domains(&id).await?;
        Ok(idp)
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<IdentityProvider>> {
        let result = oauth_identity_providers::Entity::find_by_id(id).one(&self.db).await?;
        match result {
            Some(m) => Ok(Some(self.hydrate(m).await?)),
            None => Ok(None),
        }
    }

    pub async fn find_by_code(&self, code: &str) -> Result<Option<IdentityProvider>> {
        let result = oauth_identity_providers::Entity::find()
            .filter(oauth_identity_providers::Column::Code.eq(code))
            .one(&self.db)
            .await?;
        match result {
            Some(m) => Ok(Some(self.hydrate(m).await?)),
            None => Ok(None),
        }
    }

    pub async fn find_all(&self) -> Result<Vec<IdentityProvider>> {
        let results = oauth_identity_providers::Entity::find()
            .order_by_asc(oauth_identity_providers::Column::Code)
            .all(&self.db)
            .await?;
        let mut idps = Vec::with_capacity(results.len());
        for m in results {
            idps.push(self.hydrate(m).await?);
        }
        Ok(idps)
    }

    pub async fn insert(&self, idp: &IdentityProvider) -> Result<()> {
        let model = oauth_identity_providers::ActiveModel {
            id: Set(idp.id.clone()),
            code: Set(idp.code.clone()),
            name: Set(idp.name.clone()),
            r#type: Set(idp.r#type.as_str().to_string()),
            oidc_issuer_url: Set(idp.oidc_issuer_url.clone()),
            oidc_client_id: Set(idp.oidc_client_id.clone()),
            oidc_client_secret_ref: Set(idp.oidc_client_secret_ref.clone()),
            oidc_multi_tenant: Set(idp.oidc_multi_tenant),
            oidc_issuer_pattern: Set(idp.oidc_issuer_pattern.clone()),
            created_at: Set(Utc::now().into()),
            updated_at: Set(Utc::now().into()),
        };
        oauth_identity_providers::Entity::insert(model).exec(&self.db).await?;
        self.save_allowed_domains(&idp.id, &idp.allowed_email_domains).await?;
        Ok(())
    }

    pub async fn update(&self, idp: &IdentityProvider) -> Result<()> {
        let model = oauth_identity_providers::ActiveModel {
            id: Set(idp.id.clone()),
            code: Set(idp.code.clone()),
            name: Set(idp.name.clone()),
            r#type: Set(idp.r#type.as_str().to_string()),
            oidc_issuer_url: Set(idp.oidc_issuer_url.clone()),
            oidc_client_id: Set(idp.oidc_client_id.clone()),
            oidc_client_secret_ref: Set(idp.oidc_client_secret_ref.clone()),
            oidc_multi_tenant: Set(idp.oidc_multi_tenant),
            oidc_issuer_pattern: Set(idp.oidc_issuer_pattern.clone()),
            created_at: NotSet,
            updated_at: Set(Utc::now().into()),
        };
        oauth_identity_providers::Entity::update(model).exec(&self.db).await?;
        // Replace allowed domains
        self.delete_allowed_domains(&idp.id).await?;
        self.save_allowed_domains(&idp.id, &idp.allowed_email_domains).await?;
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        self.delete_allowed_domains(id).await?;
        let result = oauth_identity_providers::Entity::delete_by_id(id).exec(&self.db).await?;
        Ok(result.rows_affected > 0)
    }

    async fn save_allowed_domains(&self, idp_id: &str, domains: &[String]) -> Result<()> {
        for domain in domains {
            let model = oauth_identity_provider_allowed_domains::ActiveModel {
                id: NotSet,
                identity_provider_id: Set(idp_id.to_string()),
                email_domain: Set(domain.clone()),
            };
            oauth_identity_provider_allowed_domains::Entity::insert(model).exec(&self.db).await?;
        }
        Ok(())
    }

    async fn delete_allowed_domains(&self, idp_id: &str) -> Result<()> {
        oauth_identity_provider_allowed_domains::Entity::delete_many()
            .filter(oauth_identity_provider_allowed_domains::Column::IdentityProviderId.eq(idp_id))
            .exec(&self.db)
            .await?;
        Ok(())
    }
}
