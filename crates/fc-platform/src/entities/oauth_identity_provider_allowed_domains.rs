//! SeaORM Entity: oauth_identity_provider_allowed_domains

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "oauth_identity_provider_allowed_domains")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub identity_provider_id: String,
    pub email_domain: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
