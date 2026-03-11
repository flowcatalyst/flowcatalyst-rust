//! LoginAttempt Entity

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AttemptType {
    UserLogin,
    ServiceAccountToken,
}

impl AttemptType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::UserLogin => "USER_LOGIN",
            Self::ServiceAccountToken => "SERVICE_ACCOUNT_TOKEN",
        }
    }
    pub fn from_str(s: &str) -> Self {
        match s {
            "SERVICE_ACCOUNT_TOKEN" => Self::ServiceAccountToken,
            _ => Self::UserLogin,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LoginOutcome {
    Success,
    Failure,
}

impl LoginOutcome {
    pub fn as_str(&self) -> &'static str {
        match self { Self::Success => "SUCCESS", Self::Failure => "FAILURE" }
    }
    pub fn from_str(s: &str) -> Self {
        match s { "FAILURE" => Self::Failure, _ => Self::Success }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginAttempt {
    pub id: String,
    pub attempt_type: AttemptType,
    pub outcome: LoginOutcome,
    pub failure_reason: Option<String>,
    pub identifier: Option<String>,
    pub principal_id: Option<String>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub attempted_at: DateTime<Utc>,
}

impl LoginAttempt {
    pub fn new(attempt_type: AttemptType, outcome: LoginOutcome) -> Self {
        Self {
            id: crate::TsidGenerator::generate(crate::EntityType::LoginAttempt),
            attempt_type,
            outcome,
            failure_reason: None,
            identifier: None,
            principal_id: None,
            ip_address: None,
            user_agent: None,
            attempted_at: Utc::now(),
        }
    }
}

impl From<crate::entities::iam_login_attempts::Model> for LoginAttempt {
    fn from(m: crate::entities::iam_login_attempts::Model) -> Self {
        Self {
            id: m.id,
            attempt_type: AttemptType::from_str(&m.attempt_type),
            outcome: LoginOutcome::from_str(&m.outcome),
            failure_reason: m.failure_reason,
            identifier: m.identifier,
            principal_id: m.principal_id,
            ip_address: m.ip_address,
            user_agent: m.user_agent,
            attempted_at: m.attempted_at.with_timezone(&Utc),
        }
    }
}
