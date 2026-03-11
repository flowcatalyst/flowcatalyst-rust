//! SeaORM Entity: oauth_clients

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "oauth_clients")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    #[sea_orm(unique)]
    pub client_id: String,
    pub client_name: String,
    pub client_type: String,
    pub client_secret_ref: Option<String>,
    pub default_scopes: Option<String>,
    pub pkce_required: bool,
    pub service_account_principal_id: Option<String>,
    pub active: bool,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
