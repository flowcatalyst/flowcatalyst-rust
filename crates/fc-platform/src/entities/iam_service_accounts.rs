//! SeaORM Entity: iam_service_accounts

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "iam_service_accounts")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    #[sea_orm(unique)]
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub application_id: Option<String>,
    pub active: bool,
    pub wh_auth_type: Option<String>,
    pub wh_auth_token_ref: Option<String>,
    pub wh_signing_secret_ref: Option<String>,
    pub wh_signing_algorithm: Option<String>,
    pub wh_credentials_created_at: Option<DateTimeWithTimeZone>,
    pub wh_credentials_regenerated_at: Option<DateTimeWithTimeZone>,
    pub last_used_at: Option<DateTimeWithTimeZone>,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
