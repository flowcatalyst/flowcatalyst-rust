//! EmailDomainMapping Repository — PostgreSQL via SeaORM

use sea_orm::*;
use chrono::Utc;

use super::entity::{EmailDomainMapping, ScopeType};
use crate::entities::{
    tnt_email_domain_mappings,
    tnt_email_domain_mapping_clients,
    tnt_email_domain_mapping_granted_clients,
    tnt_email_domain_mapping_allowed_roles,
};
use crate::shared::error::Result;

pub struct EmailDomainMappingRepository {
    db: DatabaseConnection,
}

impl EmailDomainMappingRepository {
    pub fn new(db: &DatabaseConnection) -> Self {
        Self { db: db.clone() }
    }

    async fn hydrate(&self, model: tnt_email_domain_mappings::Model) -> Result<EmailDomainMapping> {
        let id = model.id.clone();
        let mut edm = EmailDomainMapping::from(model);

        // Load additional clients
        let additional = tnt_email_domain_mapping_clients::Entity::find()
            .filter(tnt_email_domain_mapping_clients::Column::EmailDomainMappingId.eq(&id))
            .all(&self.db)
            .await?;
        edm.additional_client_ids = additional.into_iter().map(|r| r.client_id).collect();

        // Load granted clients
        let granted = tnt_email_domain_mapping_granted_clients::Entity::find()
            .filter(tnt_email_domain_mapping_granted_clients::Column::EmailDomainMappingId.eq(&id))
            .all(&self.db)
            .await?;
        edm.granted_client_ids = granted.into_iter().map(|r| r.client_id).collect();

        // Load allowed roles
        let roles = tnt_email_domain_mapping_allowed_roles::Entity::find()
            .filter(tnt_email_domain_mapping_allowed_roles::Column::EmailDomainMappingId.eq(&id))
            .all(&self.db)
            .await?;
        edm.allowed_role_ids = roles.into_iter().map(|r| r.role_id).collect();

        Ok(edm)
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<EmailDomainMapping>> {
        let result = tnt_email_domain_mappings::Entity::find_by_id(id).one(&self.db).await?;
        match result {
            Some(m) => Ok(Some(self.hydrate(m).await?)),
            None => Ok(None),
        }
    }

    pub async fn find_by_email_domain(&self, domain: &str) -> Result<Option<EmailDomainMapping>> {
        let result = tnt_email_domain_mappings::Entity::find()
            .filter(tnt_email_domain_mappings::Column::EmailDomain.eq(domain))
            .one(&self.db)
            .await?;
        match result {
            Some(m) => Ok(Some(self.hydrate(m).await?)),
            None => Ok(None),
        }
    }

    pub async fn find_all(&self) -> Result<Vec<EmailDomainMapping>> {
        let results = tnt_email_domain_mappings::Entity::find()
            .order_by_asc(tnt_email_domain_mappings::Column::EmailDomain)
            .all(&self.db)
            .await?;
        let mut mappings = Vec::with_capacity(results.len());
        for m in results {
            mappings.push(self.hydrate(m).await?);
        }
        Ok(mappings)
    }

    pub async fn insert(&self, edm: &EmailDomainMapping) -> Result<()> {
        let model = tnt_email_domain_mappings::ActiveModel {
            id: Set(edm.id.clone()),
            email_domain: Set(edm.email_domain.clone()),
            identity_provider_id: Set(edm.identity_provider_id.clone()),
            scope_type: Set(edm.scope_type.as_str().to_string()),
            primary_client_id: Set(edm.primary_client_id.clone()),
            required_oidc_tenant_id: Set(edm.required_oidc_tenant_id.clone()),
            sync_roles_from_idp: Set(edm.sync_roles_from_idp),
            created_at: Set(Utc::now().into()),
            updated_at: Set(Utc::now().into()),
        };
        tnt_email_domain_mappings::Entity::insert(model).exec(&self.db).await?;
        self.save_junctions(&edm.id, edm).await?;
        Ok(())
    }

    pub async fn update(&self, edm: &EmailDomainMapping) -> Result<()> {
        let model = tnt_email_domain_mappings::ActiveModel {
            id: Set(edm.id.clone()),
            email_domain: Set(edm.email_domain.clone()),
            identity_provider_id: Set(edm.identity_provider_id.clone()),
            scope_type: Set(edm.scope_type.as_str().to_string()),
            primary_client_id: Set(edm.primary_client_id.clone()),
            required_oidc_tenant_id: Set(edm.required_oidc_tenant_id.clone()),
            sync_roles_from_idp: Set(edm.sync_roles_from_idp),
            created_at: NotSet,
            updated_at: Set(Utc::now().into()),
        };
        tnt_email_domain_mappings::Entity::update(model).exec(&self.db).await?;
        self.delete_junctions(&edm.id).await?;
        self.save_junctions(&edm.id, edm).await?;
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        self.delete_junctions(id).await?;
        let result = tnt_email_domain_mappings::Entity::delete_by_id(id).exec(&self.db).await?;
        Ok(result.rows_affected > 0)
    }

    async fn save_junctions(&self, id: &str, edm: &EmailDomainMapping) -> Result<()> {
        for cid in &edm.additional_client_ids {
            let model = tnt_email_domain_mapping_clients::ActiveModel {
                id: NotSet,
                email_domain_mapping_id: Set(id.to_string()),
                client_id: Set(cid.clone()),
            };
            tnt_email_domain_mapping_clients::Entity::insert(model).exec(&self.db).await?;
        }
        for cid in &edm.granted_client_ids {
            let model = tnt_email_domain_mapping_granted_clients::ActiveModel {
                id: NotSet,
                email_domain_mapping_id: Set(id.to_string()),
                client_id: Set(cid.clone()),
            };
            tnt_email_domain_mapping_granted_clients::Entity::insert(model).exec(&self.db).await?;
        }
        for rid in &edm.allowed_role_ids {
            let model = tnt_email_domain_mapping_allowed_roles::ActiveModel {
                id: NotSet,
                email_domain_mapping_id: Set(id.to_string()),
                role_id: Set(rid.clone()),
            };
            tnt_email_domain_mapping_allowed_roles::Entity::insert(model).exec(&self.db).await?;
        }
        Ok(())
    }

    async fn delete_junctions(&self, id: &str) -> Result<()> {
        tnt_email_domain_mapping_clients::Entity::delete_many()
            .filter(tnt_email_domain_mapping_clients::Column::EmailDomainMappingId.eq(id))
            .exec(&self.db).await?;
        tnt_email_domain_mapping_granted_clients::Entity::delete_many()
            .filter(tnt_email_domain_mapping_granted_clients::Column::EmailDomainMappingId.eq(id))
            .exec(&self.db).await?;
        tnt_email_domain_mapping_allowed_roles::Entity::delete_many()
            .filter(tnt_email_domain_mapping_allowed_roles::Column::EmailDomainMappingId.eq(id))
            .exec(&self.db).await?;
        Ok(())
    }
}
