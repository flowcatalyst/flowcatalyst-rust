//! SeaORM Entity: iam_principal_application_access (junction table)

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "iam_principal_application_access")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub principal_id: String,
    #[sea_orm(primary_key, auto_increment = false)]
    pub application_id: String,
    pub granted_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
