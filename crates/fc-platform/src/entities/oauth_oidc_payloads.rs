//! SeaORM Entity: oauth_oidc_payloads

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "oauth_oidc_payloads")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    #[sea_orm(column_name = "type")]
    pub r#type: String,
    #[sea_orm(column_type = "JsonBinary")]
    pub payload: Json,
    pub grant_id: Option<String>,
    pub user_code: Option<String>,
    pub uid: Option<String>,
    pub expires_at: Option<DateTimeWithTimeZone>,
    pub consumed_at: Option<DateTimeWithTimeZone>,
    pub created_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
