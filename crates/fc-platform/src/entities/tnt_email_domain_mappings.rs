//! SeaORM Entity: tnt_email_domain_mappings

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "tnt_email_domain_mappings")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    #[sea_orm(unique)]
    pub email_domain: String,
    pub identity_provider_id: String,
    pub scope_type: String,
    pub primary_client_id: Option<String>,
    pub required_oidc_tenant_id: Option<String>,
    pub sync_roles_from_idp: bool,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
