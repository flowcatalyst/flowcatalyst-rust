//! SeaORM Entity: iam_roles

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "iam_roles")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub application_id: Option<String>,
    pub application_code: Option<String>,
    #[sea_orm(unique)]
    pub name: String,
    pub display_name: String,
    pub description: Option<String>,
    pub source: String,
    pub client_managed: bool,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
