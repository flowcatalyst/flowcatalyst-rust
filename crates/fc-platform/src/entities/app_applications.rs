//! SeaORM Entity: app_applications

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "app_applications")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub r#type: String,
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub icon_url: Option<String>,
    pub website: Option<String>,
    pub logo: Option<String>,
    pub logo_mime_type: Option<String>,
    pub default_base_url: Option<String>,
    pub service_account_id: Option<String>,
    pub active: bool,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
