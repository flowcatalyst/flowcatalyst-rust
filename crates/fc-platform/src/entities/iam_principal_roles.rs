//! SeaORM Entity: iam_principal_roles (junction table)

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "iam_principal_roles")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub principal_id: String,
    #[sea_orm(primary_key, auto_increment = false)]
    pub role_name: String,
    pub assignment_source: Option<String>,
    pub assigned_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
