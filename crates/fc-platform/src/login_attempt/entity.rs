//! LoginAttempt Entity

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AttemptType {
    UserLogin,
    ServiceAccountToken,
    /// A developer's self-service client_credentials exchange. Go writes it
    /// (auth/oauthapi/token.go:623) and Rust must read those rows.
    DeveloperToken,
}

crate::shared::enum_str::str_enum!(AttemptType, "attempt type", {
    UserLogin => "USER_LOGIN",
    ServiceAccountToken => "SERVICE_ACCOUNT_TOKEN",
    DeveloperToken => "DEVELOPER_TOKEN",
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LoginOutcome {
    Success,
    Failure,
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
    use std::str::FromStr;

    #[test]
    fn attempt_type_roundtrip_rejects_unknown() {
        assert_eq!(
            AttemptType::from_str("USER_LOGIN"),
            Ok(AttemptType::UserLogin)
        );
        assert_eq!(
            AttemptType::from_str("SERVICE_ACCOUNT_TOKEN"),
            Ok(AttemptType::ServiceAccountToken)
        );
        // Unknown values are rejected (X-06)
        assert!(AttemptType::from_str("UNKNOWN").is_err());
        for t in [AttemptType::UserLogin, AttemptType::ServiceAccountToken] {
            assert_eq!(AttemptType::from_str(t.as_str()), Ok(t));
        }
    }

    #[test]
    fn login_outcome_roundtrip_rejects_unknown() {
        assert_eq!(LoginOutcome::from_str("SUCCESS"), Ok(LoginOutcome::Success));
        assert_eq!(LoginOutcome::from_str("FAILURE"), Ok(LoginOutcome::Failure));
        // Unknown values are rejected (X-06)
        assert!(LoginOutcome::from_str("UNKNOWN").is_err());
        for o in [LoginOutcome::Success, LoginOutcome::Failure] {
            assert_eq!(LoginOutcome::from_str(o.as_str()), Ok(o));
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
