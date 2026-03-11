//! SeaORM Entity: tnt_client_auth_configs

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "tnt_client_auth_configs")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    #[sea_orm(unique)]
    pub email_domain: String,
    pub config_type: String,
    pub primary_client_id: Option<String>,
    pub additional_client_ids: Json,
    pub granted_client_ids: Json,
    pub auth_provider: String,
    pub oidc_issuer_url: Option<String>,
    pub oidc_client_id: Option<String>,
    pub oidc_multi_tenant: bool,
    pub oidc_issuer_pattern: Option<String>,
    pub oidc_client_secret_ref: Option<String>,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
