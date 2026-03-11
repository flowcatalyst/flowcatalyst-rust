//! SeaORM Entity: msg_subscription_event_types (junction)

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "msg_subscription_event_types")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub subscription_id: String,
    pub event_type_id: Option<String>,
    pub event_type_code: String,
    pub spec_version: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
