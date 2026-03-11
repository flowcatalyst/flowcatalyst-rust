//! SeaORM Entity: msg_event_types

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "msg_event_types")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    #[sea_orm(unique)]
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub status: String,
    pub source: String,
    pub client_scoped: bool,
    pub application: String,
    pub subdomain: String,
    pub aggregate: String,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
