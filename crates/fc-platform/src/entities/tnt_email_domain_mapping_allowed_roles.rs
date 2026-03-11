//! SeaORM Entity: tnt_email_domain_mapping_allowed_roles

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "tnt_email_domain_mapping_allowed_roles")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub email_domain_mapping_id: String,
    pub role_id: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
