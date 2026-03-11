//! SeaORM Entity: msg_subscriptions

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "msg_subscriptions")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub code: String,
    pub application_code: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub client_id: Option<String>,
    pub client_identifier: Option<String>,
    pub client_scoped: bool,
    pub connection_id: Option<String>,
    pub target: String,
    pub queue: Option<String>,
    pub source: String,
    pub status: String,
    pub max_age_seconds: i32,
    pub dispatch_pool_id: Option<String>,
    pub dispatch_pool_code: Option<String>,
    pub delay_seconds: i32,
    pub sequence: i32,
    pub mode: String,
    pub timeout_seconds: i32,
    pub max_retries: i32,
    pub service_account_id: Option<String>,
    pub data_only: bool,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
