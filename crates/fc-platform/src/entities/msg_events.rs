//! SeaORM entity for msg_events table

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "msg_events")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub spec_version: Option<String>,
    #[sea_orm(column_name = "type")]
    pub event_type: String,
    pub source: String,
    pub subject: Option<String>,
    pub time: DateTimeWithTimeZone,
    pub data: Option<Json>,
    pub correlation_id: Option<String>,
    pub causation_id: Option<String>,
    pub deduplication_id: Option<String>,
    pub message_group: Option<String>,
    pub client_id: Option<String>,
    pub context_data: Option<Json>,
    pub created_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
