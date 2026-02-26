//! SeaORM Entity: iam_role_permissions (junction table)

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "iam_role_permissions")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub role_id: String,
    #[sea_orm(primary_key, auto_increment = false)]
    pub permission: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
