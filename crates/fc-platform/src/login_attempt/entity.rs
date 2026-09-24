//! LoginAttempt Entity

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AttemptType {
    UserLogin,
    ServiceAccountToken,
}

impl AttemptType {
    pub fn from_str(s: &str) -> Self {
        match s {
            "SERVICE_ACCOUNT_TOKEN" => Self::ServiceAccountToken,
            _ => Self::UserLogin,
        }
    }
}

crate::shared::enum_str::str_enum!(AttemptType, "attempt type", {
    UserLogin => "USER_LOGIN",
    ServiceAccountToken => "SERVICE_ACCOUNT_TOKEN",
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LoginOutcome {
    Success,
    Failure,
}

impl LoginOutcome {
    pub fn from_str(s: &str) -> Self {
        match s {
            "FAILURE" => Self::Failure,
            _ => Self::Success,
        }
    }
}

crate::shared::enum_str::str_enum!(LoginOutcome, "login outcome", {
    Success => "SUCCESS",
    Failure => "FAILURE",
});

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
            id: crate::shared::tsid::generate(crate::EntityType::LoginAttempt),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attempt_type_roundtrip_with_fallback() {
        assert_eq!(AttemptType::from_str("USER_LOGIN"), AttemptType::UserLogin);
        assert_eq!(
            AttemptType::from_str("SERVICE_ACCOUNT_TOKEN"),
            AttemptType::ServiceAccountToken
        );
        // Unknown falls back to UserLogin
        assert_eq!(AttemptType::from_str("UNKNOWN"), AttemptType::UserLogin);
        for t in [AttemptType::UserLogin, AttemptType::ServiceAccountToken] {
            assert_eq!(AttemptType::from_str(t.as_str()), t);
        }
    }

    #[test]
    fn login_outcome_roundtrip_with_fallback() {
        assert_eq!(LoginOutcome::from_str("SUCCESS"), LoginOutcome::Success);
        assert_eq!(LoginOutcome::from_str("FAILURE"), LoginOutcome::Failure);
        // Unknown falls back to Success
        assert_eq!(LoginOutcome::from_str("UNKNOWN"), LoginOutcome::Success);
        for o in [LoginOutcome::Success, LoginOutcome::Failure] {
            assert_eq!(LoginOutcome::from_str(o.as_str()), o);
        }
    }

    #[test]
    fn new_populates_type_and_outcome_with_defaults_elsewhere() {
        let a = LoginAttempt::new(AttemptType::UserLogin, LoginOutcome::Failure);
        assert_eq!(a.attempt_type, AttemptType::UserLogin);
        assert_eq!(a.outcome, LoginOutcome::Failure);
        assert!(a.failure_reason.is_none());
        assert!(a.identifier.is_none());
        assert!(a.principal_id.is_none());
        assert!(a.ip_address.is_none());
        assert!(a.user_agent.is_none());
        assert!(!a.id.is_empty());
    }

    #[test]
    fn new_attempts_get_distinct_ids() {
        let a = LoginAttempt::new(AttemptType::UserLogin, LoginOutcome::Success);
        let b = LoginAttempt::new(AttemptType::UserLogin, LoginOutcome::Success);
        assert_ne!(a.id, b.id);
    }
}
