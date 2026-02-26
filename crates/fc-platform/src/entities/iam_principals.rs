//! SeaORM Entity: iam_principals

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "iam_principals")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    #[sea_orm(column_name = "type")]
    pub principal_type: String,
    pub scope: Option<String>,
    pub client_id: Option<String>,
    pub application_id: Option<String>,
    pub name: String,
    pub active: bool,
    // Flattened user identity fields
    pub email: Option<String>,
    pub email_domain: Option<String>,
    pub idp_type: Option<String>,
    pub external_idp_id: Option<String>,
    pub password_hash: Option<String>,
    pub last_login_at: Option<DateTimeWithTimeZone>,
    // FK to service_accounts
    pub service_account_id: Option<String>,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
