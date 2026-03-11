//! SeaORM Entity: msg_dispatch_pools

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "msg_dispatch_pools")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub rate_limit: i32,
    pub concurrency: i32,
    pub client_id: Option<String>,
    pub client_identifier: Option<String>,
    pub status: String,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
