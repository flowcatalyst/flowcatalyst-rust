//! SeaORM Entity: msg_subscription_custom_configs (junction)

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "msg_subscription_custom_configs")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub subscription_id: String,
    pub config_key: String,
    pub config_value: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
