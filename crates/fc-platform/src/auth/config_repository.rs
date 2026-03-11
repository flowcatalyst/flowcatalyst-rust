//! Authentication Configuration Repositories — PostgreSQL via SeaORM

use sea_orm::*;
use chrono::Utc;

use async_trait::async_trait;
use sea_orm::sea_query::OnConflict;

use crate::auth::config_entity::{AnchorDomain, ClientAuthConfig, IdpRoleMapping, AuthConfigType, AuthProvider};
use crate::principal::entity::ClientAccessGrant;
use crate::entities::{tnt_anchor_domains, tnt_client_auth_configs, iam_client_access_grants, oauth_idp_role_mappings};
use crate::shared::error::Result;
use crate::usecase::unit_of_work::{HasId, PgPersist};

/// Anchor Domain Repository
pub struct AnchorDomainRepository {
    db: DatabaseConnection,
}

impl AnchorDomainRepository {
    pub fn new(db: &DatabaseConnection) -> Self {
        Self { db: db.clone() }
    }

    pub async fn insert(&self, domain: &AnchorDomain) -> Result<()> {
        let model = tnt_anchor_domains::ActiveModel {
            id: Set(domain.id.clone()),
            domain: Set(domain.domain.clone()),
            created_at: Set(Utc::now().into()),
            updated_at: Set(Utc::now().into()),
        };
        tnt_anchor_domains::Entity::insert(model).exec(&self.db).await?;
        Ok(())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<AnchorDomain>> {
        let result = tnt_anchor_domains::Entity::find_by_id(id).one(&self.db).await?;
        Ok(result.map(AnchorDomain::from))
    }

    pub async fn find_by_domain(&self, domain: &str) -> Result<Option<AnchorDomain>> {
        let result = tnt_anchor_domains::Entity::find()
            .filter(tnt_anchor_domains::Column::Domain.eq(domain.to_lowercase()))
            .one(&self.db)
            .await?;
        Ok(result.map(AnchorDomain::from))
    }

    pub async fn find_all(&self) -> Result<Vec<AnchorDomain>> {
        let results = tnt_anchor_domains::Entity::find()
            .order_by_asc(tnt_anchor_domains::Column::Domain)
            .all(&self.db)
            .await?;
        Ok(results.into_iter().map(AnchorDomain::from).collect())
    }

    pub async fn is_anchor_domain(&self, domain: &str) -> Result<bool> {
        let count = tnt_anchor_domains::Entity::find()
            .filter(tnt_anchor_domains::Column::Domain.eq(domain.to_lowercase()))
            .count(&self.db)
            .await?;
        Ok(count > 0)
    }

    pub async fn update(&self, domain: &AnchorDomain) -> Result<()> {
        let model = tnt_anchor_domains::ActiveModel {
            id: Set(domain.id.clone()),
            domain: Set(domain.domain.clone()),
            created_at: NotSet,
            updated_at: Set(Utc::now().into()),
        };
        tnt_anchor_domains::Entity::update(model).exec(&self.db).await?;
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        let result = tnt_anchor_domains::Entity::delete_by_id(id).exec(&self.db).await?;
        Ok(result.rows_affected > 0)
    }
}

/// Client Auth Config Repository
pub struct ClientAuthConfigRepository {
    db: DatabaseConnection,
}

impl ClientAuthConfigRepository {
    pub fn new(db: &DatabaseConnection) -> Self {
        Self { db: db.clone() }
    }

    pub async fn insert(&self, config: &ClientAuthConfig) -> Result<()> {
        let model = tnt_client_auth_configs::ActiveModel {
            id: Set(config.id.clone()),
            email_domain: Set(config.email_domain.clone()),
            config_type: Set(config.config_type.as_str().to_string()),
            primary_client_id: Set(config.primary_client_id.clone()),
            additional_client_ids: Set(serde_json::to_value(&config.additional_client_ids).unwrap_or_default().into()),
            granted_client_ids: Set(serde_json::to_value(&config.granted_client_ids).unwrap_or_default().into()),
            auth_provider: Set(config.auth_provider.as_str().to_string()),
            oidc_issuer_url: Set(config.oidc_issuer_url.clone()),
            oidc_client_id: Set(config.oidc_client_id.clone()),
            oidc_multi_tenant: Set(config.oidc_multi_tenant),
            oidc_issuer_pattern: Set(config.oidc_issuer_pattern.clone()),
            oidc_client_secret_ref: Set(config.oidc_client_secret_ref.clone()),
            created_at: Set(Utc::now().into()),
            updated_at: Set(Utc::now().into()),
        };
        tnt_client_auth_configs::Entity::insert(model).exec(&self.db).await?;
        Ok(())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<ClientAuthConfig>> {
        let result = tnt_client_auth_configs::Entity::find_by_id(id).one(&self.db).await?;
        Ok(result.map(ClientAuthConfig::from))
    }

    pub async fn find_by_email_domain(&self, domain: &str) -> Result<Option<ClientAuthConfig>> {
        let result = tnt_client_auth_configs::Entity::find()
            .filter(tnt_client_auth_configs::Column::EmailDomain.eq(domain.to_lowercase()))
            .one(&self.db)
            .await?;
        Ok(result.map(ClientAuthConfig::from))
    }

    pub async fn find_by_client_id(&self, client_id: &str) -> Result<Vec<ClientAuthConfig>> {
        let results = tnt_client_auth_configs::Entity::find()
            .filter(tnt_client_auth_configs::Column::PrimaryClientId.eq(client_id))
            .all(&self.db)
            .await?;
        Ok(results.into_iter().map(ClientAuthConfig::from).collect())
    }

    pub async fn find_all(&self) -> Result<Vec<ClientAuthConfig>> {
        let results = tnt_client_auth_configs::Entity::find()
            .order_by_asc(tnt_client_auth_configs::Column::EmailDomain)
            .all(&self.db)
            .await?;
        Ok(results.into_iter().map(ClientAuthConfig::from).collect())
    }

    pub async fn update(&self, config: &ClientAuthConfig) -> Result<()> {
        let model = tnt_client_auth_configs::ActiveModel {
            id: Set(config.id.clone()),
            email_domain: Set(config.email_domain.clone()),
            config_type: Set(config.config_type.as_str().to_string()),
            primary_client_id: Set(config.primary_client_id.clone()),
            additional_client_ids: Set(serde_json::to_value(&config.additional_client_ids).unwrap_or_default().into()),
            granted_client_ids: Set(serde_json::to_value(&config.granted_client_ids).unwrap_or_default().into()),
            auth_provider: Set(config.auth_provider.as_str().to_string()),
            oidc_issuer_url: Set(config.oidc_issuer_url.clone()),
            oidc_client_id: Set(config.oidc_client_id.clone()),
            oidc_multi_tenant: Set(config.oidc_multi_tenant),
            oidc_issuer_pattern: Set(config.oidc_issuer_pattern.clone()),
            oidc_client_secret_ref: Set(config.oidc_client_secret_ref.clone()),
            created_at: NotSet,
            updated_at: Set(Utc::now().into()),
        };
        tnt_client_auth_configs::Entity::update(model).exec(&self.db).await?;
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        let result = tnt_client_auth_configs::Entity::delete_by_id(id).exec(&self.db).await?;
        Ok(result.rows_affected > 0)
    }
}

/// Client Access Grant Repository
pub struct ClientAccessGrantRepository {
    db: DatabaseConnection,
}

impl ClientAccessGrantRepository {
    pub fn new(db: &DatabaseConnection) -> Self {
        Self { db: db.clone() }
    }

    pub async fn insert(&self, grant: &ClientAccessGrant) -> Result<()> {
        let model = iam_client_access_grants::ActiveModel {
            id: Set(grant.id.clone()),
            principal_id: Set(grant.principal_id.clone()),
            client_id: Set(grant.client_id.clone()),
            granted_by: Set(grant.granted_by.clone()),
            granted_at: Set(grant.granted_at.into()),
            created_at: Set(grant.created_at.into()),
            updated_at: Set(grant.updated_at.into()),
        };
        iam_client_access_grants::Entity::insert(model).exec(&self.db).await?;
        Ok(())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<ClientAccessGrant>> {
        let result = iam_client_access_grants::Entity::find_by_id(id).one(&self.db).await?;
        Ok(result.map(ClientAccessGrant::from))
    }

    pub async fn find_by_principal(&self, principal_id: &str) -> Result<Vec<ClientAccessGrant>> {
        let results = iam_client_access_grants::Entity::find()
            .filter(iam_client_access_grants::Column::PrincipalId.eq(principal_id))
            .all(&self.db)
            .await?;
        Ok(results.into_iter().map(ClientAccessGrant::from).collect())
    }

    pub async fn find_by_client(&self, client_id: &str) -> Result<Vec<ClientAccessGrant>> {
        let results = iam_client_access_grants::Entity::find()
            .filter(iam_client_access_grants::Column::ClientId.eq(client_id))
            .all(&self.db)
            .await?;
        Ok(results.into_iter().map(ClientAccessGrant::from).collect())
    }

    pub async fn find_by_principal_and_client(&self, principal_id: &str, client_id: &str) -> Result<Option<ClientAccessGrant>> {
        let result = iam_client_access_grants::Entity::find()
            .filter(iam_client_access_grants::Column::PrincipalId.eq(principal_id))
            .filter(iam_client_access_grants::Column::ClientId.eq(client_id))
            .one(&self.db)
            .await?;
        Ok(result.map(ClientAccessGrant::from))
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        let result = iam_client_access_grants::Entity::delete_by_id(id).exec(&self.db).await?;
        Ok(result.rows_affected > 0)
    }

    pub async fn delete_by_principal_and_client(&self, principal_id: &str, client_id: &str) -> Result<bool> {
        let result = iam_client_access_grants::Entity::delete_many()
            .filter(iam_client_access_grants::Column::PrincipalId.eq(principal_id))
            .filter(iam_client_access_grants::Column::ClientId.eq(client_id))
            .exec(&self.db)
            .await?;
        Ok(result.rows_affected > 0)
    }
}

// ── PgPersist for ClientAccessGrant ──────────────────────────────────────────

impl HasId for ClientAccessGrant {
    fn id(&self) -> &str { &self.id }
}

#[async_trait]
impl PgPersist for ClientAccessGrant {
    async fn pg_upsert(&self, txn: &DatabaseTransaction) -> Result<()> {
        let model = iam_client_access_grants::ActiveModel {
            id: Set(self.id.clone()),
            principal_id: Set(self.principal_id.clone()),
            client_id: Set(self.client_id.clone()),
            granted_by: Set(self.granted_by.clone()),
            granted_at: Set(self.granted_at.into()),
            created_at: Set(self.created_at.into()),
            updated_at: Set(self.updated_at.into()),
        };
        iam_client_access_grants::Entity::insert(model)
            .on_conflict(
                OnConflict::column(iam_client_access_grants::Column::Id)
                    .update_columns([
                        iam_client_access_grants::Column::GrantedBy,
                        iam_client_access_grants::Column::UpdatedAt,
                    ])
                    .to_owned(),
            )
            .exec(txn)
            .await?;
        Ok(())
    }

    async fn pg_delete(&self, txn: &DatabaseTransaction) -> Result<()> {
        iam_client_access_grants::Entity::delete_by_id(&self.id).exec(txn).await?;
        Ok(())
    }
}

// ── PgPersist for AnchorDomain ────────────────────────────────────────────────

impl HasId for AnchorDomain {
    fn id(&self) -> &str { &self.id }
}

#[async_trait]
impl PgPersist for AnchorDomain {
    async fn pg_upsert(&self, txn: &DatabaseTransaction) -> Result<()> {
        let model = tnt_anchor_domains::ActiveModel {
            id: Set(self.id.clone()),
            domain: Set(self.domain.clone()),
            created_at: Set(Utc::now().into()),
            updated_at: Set(Utc::now().into()),
        };
        tnt_anchor_domains::Entity::insert(model)
            .on_conflict(
                OnConflict::column(tnt_anchor_domains::Column::Id)
                    .update_columns([
                        tnt_anchor_domains::Column::Domain,
                        tnt_anchor_domains::Column::UpdatedAt,
                    ])
                    .to_owned(),
            )
            .exec(txn)
            .await?;
        Ok(())
    }

    async fn pg_delete(&self, txn: &DatabaseTransaction) -> Result<()> {
        tnt_anchor_domains::Entity::delete_by_id(&self.id).exec(txn).await?;
        Ok(())
    }
}

// ── PgPersist for ClientAuthConfig ───────────────────────────────────────────

impl HasId for ClientAuthConfig {
    fn id(&self) -> &str { &self.id }
}

#[async_trait]
impl PgPersist for ClientAuthConfig {
    async fn pg_upsert(&self, txn: &DatabaseTransaction) -> Result<()> {
        let model = tnt_client_auth_configs::ActiveModel {
            id: Set(self.id.clone()),
            email_domain: Set(self.email_domain.clone()),
            config_type: Set(self.config_type.as_str().to_string()),
            primary_client_id: Set(self.primary_client_id.clone()),
            additional_client_ids: Set(serde_json::to_value(&self.additional_client_ids).unwrap_or_default().into()),
            granted_client_ids: Set(serde_json::to_value(&self.granted_client_ids).unwrap_or_default().into()),
            auth_provider: Set(self.auth_provider.as_str().to_string()),
            oidc_issuer_url: Set(self.oidc_issuer_url.clone()),
            oidc_client_id: Set(self.oidc_client_id.clone()),
            oidc_multi_tenant: Set(self.oidc_multi_tenant),
            oidc_issuer_pattern: Set(self.oidc_issuer_pattern.clone()),
            oidc_client_secret_ref: Set(self.oidc_client_secret_ref.clone()),
            created_at: Set(Utc::now().into()),
            updated_at: Set(Utc::now().into()),
        };
        tnt_client_auth_configs::Entity::insert(model)
            .on_conflict(
                OnConflict::column(tnt_client_auth_configs::Column::Id)
                    .update_columns([
                        tnt_client_auth_configs::Column::EmailDomain,
                        tnt_client_auth_configs::Column::ConfigType,
                        tnt_client_auth_configs::Column::PrimaryClientId,
                        tnt_client_auth_configs::Column::AdditionalClientIds,
                        tnt_client_auth_configs::Column::GrantedClientIds,
                        tnt_client_auth_configs::Column::AuthProvider,
                        tnt_client_auth_configs::Column::OidcIssuerUrl,
                        tnt_client_auth_configs::Column::OidcClientId,
                        tnt_client_auth_configs::Column::OidcMultiTenant,
                        tnt_client_auth_configs::Column::OidcIssuerPattern,
                        tnt_client_auth_configs::Column::OidcClientSecretRef,
                        tnt_client_auth_configs::Column::UpdatedAt,
                    ])
                    .to_owned(),
            )
            .exec(txn)
            .await?;
        Ok(())
    }

    async fn pg_delete(&self, txn: &DatabaseTransaction) -> Result<()> {
        tnt_client_auth_configs::Entity::delete_by_id(&self.id).exec(txn).await?;
        Ok(())
    }
}

/// IDP Role Mapping Repository
pub struct IdpRoleMappingRepository {
    db: DatabaseConnection,
}

impl IdpRoleMappingRepository {
    pub fn new(db: &DatabaseConnection) -> Self {
        Self { db: db.clone() }
    }

    pub async fn insert(&self, mapping: &IdpRoleMapping) -> Result<()> {
        let model = oauth_idp_role_mappings::ActiveModel {
            id: Set(mapping.id.clone()),
            idp_role_name: Set(mapping.idp_role_name.clone()),
            internal_role_name: Set(mapping.platform_role_name.clone()),
            created_at: Set(Utc::now().into()),
            updated_at: Set(Utc::now().into()),
        };
        oauth_idp_role_mappings::Entity::insert(model).exec(&self.db).await?;
        Ok(())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<IdpRoleMapping>> {
        let result = oauth_idp_role_mappings::Entity::find_by_id(id).one(&self.db).await?;
        Ok(result.map(IdpRoleMapping::from))
    }

    pub async fn find_by_idp_role(&self, _idp_type: &str, idp_role_name: &str) -> Result<Option<IdpRoleMapping>> {
        let result = oauth_idp_role_mappings::Entity::find()
            .filter(oauth_idp_role_mappings::Column::IdpRoleName.eq(idp_role_name))
            .one(&self.db)
            .await?;
        Ok(result.map(IdpRoleMapping::from))
    }

    pub async fn find_by_idp_type(&self, _idp_type: &str) -> Result<Vec<IdpRoleMapping>> {
        // DB doesn't have idp_type column — return all
        self.find_all().await
    }

    pub async fn find_all(&self) -> Result<Vec<IdpRoleMapping>> {
        let results = oauth_idp_role_mappings::Entity::find()
            .order_by_asc(oauth_idp_role_mappings::Column::IdpRoleName)
            .all(&self.db)
            .await?;
        Ok(results.into_iter().map(IdpRoleMapping::from).collect())
    }

    pub async fn find_idp_role_mapping(&self, idp_role_name: &str) -> Result<Option<IdpRoleMapping>> {
        let result = oauth_idp_role_mappings::Entity::find()
            .filter(oauth_idp_role_mappings::Column::IdpRoleName.eq(idp_role_name))
            .one(&self.db)
            .await?;
        Ok(result.map(IdpRoleMapping::from))
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        let result = oauth_idp_role_mappings::Entity::delete_by_id(id).exec(&self.db).await?;
        Ok(result.rows_affected > 0)
    }
}
