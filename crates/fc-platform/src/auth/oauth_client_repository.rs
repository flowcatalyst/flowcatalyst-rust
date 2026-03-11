//! OAuth Client Repository — PostgreSQL via SeaORM

use sea_orm::*;
use chrono::Utc;

use crate::auth::oauth_entity::{OAuthClient, GrantType};
use crate::entities::{oauth_clients, oauth_client_collections};
use crate::shared::error::Result;

pub struct OAuthClientRepository {
    db: DatabaseConnection,
}

impl OAuthClientRepository {
    pub fn new(db: &DatabaseConnection) -> Self {
        Self { db: db.clone() }
    }

    // ── Junction table helpers ───────────────────────────────────

    async fn load_redirect_uris(&self, oauth_client_id: &str) -> Result<Vec<String>> {
        let rows = oauth_client_collections::redirect_uris::Entity::find()
            .filter(oauth_client_collections::redirect_uris::Column::OauthClientId.eq(oauth_client_id))
            .all(&self.db)
            .await?;
        Ok(rows.into_iter().map(|r| r.redirect_uri).collect())
    }

    async fn load_grant_types(&self, oauth_client_id: &str) -> Result<Vec<GrantType>> {
        let rows = oauth_client_collections::grant_types::Entity::find()
            .filter(oauth_client_collections::grant_types::Column::OauthClientId.eq(oauth_client_id))
            .all(&self.db)
            .await?;
        Ok(rows.into_iter().filter_map(|r| GrantType::from_str(&r.grant_type)).collect())
    }

    async fn load_application_ids(&self, oauth_client_id: &str) -> Result<Vec<String>> {
        let rows = oauth_client_collections::application_ids::Entity::find()
            .filter(oauth_client_collections::application_ids::Column::OauthClientId.eq(oauth_client_id))
            .all(&self.db)
            .await?;
        Ok(rows.into_iter().map(|r| r.application_id).collect())
    }

    async fn hydrate(&self, mut client: OAuthClient) -> Result<OAuthClient> {
        client.redirect_uris = self.load_redirect_uris(&client.id).await?;
        client.grant_types = self.load_grant_types(&client.id).await?;
        client.application_ids = self.load_application_ids(&client.id).await?;
        Ok(client)
    }

    async fn save_redirect_uris(&self, oauth_client_id: &str, uris: &[String]) -> Result<()> {
        oauth_client_collections::redirect_uris::Entity::delete_many()
            .filter(oauth_client_collections::redirect_uris::Column::OauthClientId.eq(oauth_client_id))
            .exec(&self.db)
            .await?;
        for uri in uris {
            let model = oauth_client_collections::redirect_uris::ActiveModel {
                oauth_client_id: Set(oauth_client_id.to_string()),
                redirect_uri: Set(uri.clone()),
            };
            oauth_client_collections::redirect_uris::Entity::insert(model).exec(&self.db).await?;
        }
        Ok(())
    }

    async fn save_grant_types(&self, oauth_client_id: &str, grant_types: &[GrantType]) -> Result<()> {
        oauth_client_collections::grant_types::Entity::delete_many()
            .filter(oauth_client_collections::grant_types::Column::OauthClientId.eq(oauth_client_id))
            .exec(&self.db)
            .await?;
        for gt in grant_types {
            let model = oauth_client_collections::grant_types::ActiveModel {
                oauth_client_id: Set(oauth_client_id.to_string()),
                grant_type: Set(gt.as_str().to_string()),
            };
            oauth_client_collections::grant_types::Entity::insert(model).exec(&self.db).await?;
        }
        Ok(())
    }

    async fn save_application_ids(&self, oauth_client_id: &str, app_ids: &[String]) -> Result<()> {
        oauth_client_collections::application_ids::Entity::delete_many()
            .filter(oauth_client_collections::application_ids::Column::OauthClientId.eq(oauth_client_id))
            .exec(&self.db)
            .await?;
        for app_id in app_ids {
            let model = oauth_client_collections::application_ids::ActiveModel {
                oauth_client_id: Set(oauth_client_id.to_string()),
                application_id: Set(app_id.clone()),
            };
            oauth_client_collections::application_ids::Entity::insert(model).exec(&self.db).await?;
        }
        Ok(())
    }

    // ── CRUD ─────────────────────────────────────────────────────

    pub async fn insert(&self, client: &OAuthClient) -> Result<()> {
        let scopes = if client.default_scopes.is_empty() {
            None
        } else {
            Some(client.default_scopes.join(","))
        };

        let model = oauth_clients::ActiveModel {
            id: Set(client.id.clone()),
            client_id: Set(client.client_id.clone()),
            client_name: Set(client.client_name.clone()),
            client_type: Set(client.client_type.as_str().to_string()),
            client_secret_ref: Set(client.client_secret_ref.clone()),
            default_scopes: Set(scopes),
            pkce_required: Set(client.pkce_required),
            service_account_principal_id: Set(client.service_account_principal_id.clone()),
            active: Set(client.active),
            created_at: Set(Utc::now().into()),
            updated_at: Set(Utc::now().into()),
        };
        oauth_clients::Entity::insert(model).exec(&self.db).await?;

        self.save_redirect_uris(&client.id, &client.redirect_uris).await?;
        self.save_grant_types(&client.id, &client.grant_types).await?;
        self.save_application_ids(&client.id, &client.application_ids).await?;
        Ok(())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<OAuthClient>> {
        let result = oauth_clients::Entity::find_by_id(id).one(&self.db).await?;
        match result {
            Some(m) => Ok(Some(self.hydrate(OAuthClient::from(m)).await?)),
            None => Ok(None),
        }
    }

    pub async fn find_by_client_id(&self, client_id: &str) -> Result<Option<OAuthClient>> {
        let result = oauth_clients::Entity::find()
            .filter(oauth_clients::Column::ClientId.eq(client_id))
            .one(&self.db)
            .await?;
        match result {
            Some(m) => Ok(Some(self.hydrate(OAuthClient::from(m)).await?)),
            None => Ok(None),
        }
    }

    pub async fn find_active(&self) -> Result<Vec<OAuthClient>> {
        let rows = oauth_clients::Entity::find()
            .filter(oauth_clients::Column::Active.eq(true))
            .all(&self.db)
            .await?;
        let mut results = Vec::with_capacity(rows.len());
        for m in rows {
            results.push(self.hydrate(OAuthClient::from(m)).await?);
        }
        Ok(results)
    }

    pub async fn find_all(&self) -> Result<Vec<OAuthClient>> {
        let rows = oauth_clients::Entity::find().all(&self.db).await?;
        let mut results = Vec::with_capacity(rows.len());
        for m in rows {
            results.push(self.hydrate(OAuthClient::from(m)).await?);
        }
        Ok(results)
    }

    pub async fn find_by_application(&self, application_id: &str) -> Result<Vec<OAuthClient>> {
        let client_ids: Vec<String> = oauth_client_collections::application_ids::Entity::find()
            .filter(oauth_client_collections::application_ids::Column::ApplicationId.eq(application_id))
            .all(&self.db)
            .await?
            .into_iter()
            .map(|r| r.oauth_client_id)
            .collect();

        if client_ids.is_empty() {
            return Ok(vec![]);
        }

        let rows = oauth_clients::Entity::find()
            .filter(oauth_clients::Column::Id.is_in(client_ids))
            .all(&self.db)
            .await?;
        let mut results = Vec::with_capacity(rows.len());
        for m in rows {
            results.push(self.hydrate(OAuthClient::from(m)).await?);
        }
        Ok(results)
    }

    pub async fn exists_by_client_id(&self, client_id: &str) -> Result<bool> {
        let count = oauth_clients::Entity::find()
            .filter(oauth_clients::Column::ClientId.eq(client_id))
            .count(&self.db)
            .await?;
        Ok(count > 0)
    }

    pub async fn update(&self, client: &OAuthClient) -> Result<()> {
        let scopes = if client.default_scopes.is_empty() {
            None
        } else {
            Some(client.default_scopes.join(","))
        };

        let model = oauth_clients::ActiveModel {
            id: Set(client.id.clone()),
            client_id: Set(client.client_id.clone()),
            client_name: Set(client.client_name.clone()),
            client_type: Set(client.client_type.as_str().to_string()),
            client_secret_ref: Set(client.client_secret_ref.clone()),
            default_scopes: Set(scopes),
            pkce_required: Set(client.pkce_required),
            service_account_principal_id: Set(client.service_account_principal_id.clone()),
            active: Set(client.active),
            created_at: NotSet,
            updated_at: Set(Utc::now().into()),
        };
        oauth_clients::Entity::update(model).exec(&self.db).await?;

        self.save_redirect_uris(&client.id, &client.redirect_uris).await?;
        self.save_grant_types(&client.id, &client.grant_types).await?;
        self.save_application_ids(&client.id, &client.application_ids).await?;
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        // Junction tables cascade or delete manually
        oauth_client_collections::redirect_uris::Entity::delete_many()
            .filter(oauth_client_collections::redirect_uris::Column::OauthClientId.eq(id))
            .exec(&self.db).await?;
        oauth_client_collections::grant_types::Entity::delete_many()
            .filter(oauth_client_collections::grant_types::Column::OauthClientId.eq(id))
            .exec(&self.db).await?;
        oauth_client_collections::application_ids::Entity::delete_many()
            .filter(oauth_client_collections::application_ids::Column::OauthClientId.eq(id))
            .exec(&self.db).await?;

        let result = oauth_clients::Entity::delete_by_id(id).exec(&self.db).await?;
        Ok(result.rows_affected > 0)
    }
}
