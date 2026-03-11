//! SeaORM Entity: iam_authorization_codes

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "iam_authorization_codes")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub code: String,
    pub client_id: String,
    pub principal_id: String,
    pub redirect_uri: String,
    pub scope: Option<String>,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
    pub nonce: Option<String>,
    pub state: Option<String>,
    pub context_client_id: Option<String>,
    pub created_at: DateTimeWithTimeZone,
    pub expires_at: DateTimeWithTimeZone,
    pub used: bool,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
