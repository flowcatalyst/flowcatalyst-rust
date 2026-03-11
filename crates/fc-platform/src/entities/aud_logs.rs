//! SeaORM Entity: aud_logs

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "aud_logs")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub entity_type: String,
    pub entity_id: String,
    pub operation: String,
    pub operation_json: Option<Json>,
    pub principal_id: Option<String>,
    pub application_id: Option<String>,
    pub client_id: Option<String>,
    pub performed_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
