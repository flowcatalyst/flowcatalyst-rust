//! SeaORM Entity: msg_dispatch_jobs_read

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "msg_dispatch_jobs_read")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub external_id: Option<String>,
    pub source: Option<String>,
    pub kind: String,
    pub code: String,
    pub subject: Option<String>,
    pub event_id: Option<String>,
    pub correlation_id: Option<String>,
    pub target_url: String,
    pub protocol: String,
    pub client_id: Option<String>,
    pub subscription_id: Option<String>,
    pub service_account_id: Option<String>,
    pub dispatch_pool_id: Option<String>,
    pub message_group: Option<String>,
    pub mode: String,
    pub sequence: i32,
    pub status: String,
    pub attempt_count: i32,
    pub max_retries: i32,
    pub last_error: Option<String>,
    pub timeout_seconds: i32,
    pub retry_strategy: String,
    pub application: Option<String>,
    pub subdomain: Option<String>,
    pub aggregate: Option<String>,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
    pub scheduled_for: Option<DateTimeWithTimeZone>,
    pub expires_at: Option<DateTimeWithTimeZone>,
    pub completed_at: Option<DateTimeWithTimeZone>,
    pub last_attempt_at: Option<DateTimeWithTimeZone>,
    pub duration_millis: Option<i64>,
    pub idempotency_key: Option<String>,
    pub is_completed: Option<bool>,
    pub is_terminal: Option<bool>,
    pub projected_at: Option<DateTimeWithTimeZone>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
