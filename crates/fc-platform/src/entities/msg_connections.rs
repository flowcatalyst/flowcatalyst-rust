//! SeaORM Entity: msg_connections

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "msg_connections")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub endpoint: String,
    pub external_id: Option<String>,
    pub status: String,
    pub service_account_id: String,
    pub client_id: Option<String>,
    pub client_identifier: Option<String>,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
