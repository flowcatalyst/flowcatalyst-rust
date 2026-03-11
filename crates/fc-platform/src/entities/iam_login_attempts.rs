//! SeaORM Entity: iam_login_attempts

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "iam_login_attempts")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub attempt_type: String,
    pub outcome: String,
    pub failure_reason: Option<String>,
    pub identifier: Option<String>,
    pub principal_id: Option<String>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub attempted_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
