//! SeaORM Entity: iam_refresh_tokens

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "iam_refresh_tokens")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub token_hash: String,
    pub principal_id: String,
    pub oauth_client_id: Option<String>,
    pub scopes: Option<String>,
    pub accessible_clients: Option<String>,
    pub revoked: bool,
    pub revoked_at: Option<DateTimeWithTimeZone>,
    pub token_family: Option<String>,
    pub replaced_by: Option<String>,
    pub created_at: DateTimeWithTimeZone,
    pub expires_at: DateTimeWithTimeZone,
    pub last_used_at: Option<DateTimeWithTimeZone>,
    pub created_from_ip: Option<String>,
    pub user_agent: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
