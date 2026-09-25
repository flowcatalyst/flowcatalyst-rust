//! PasswordResetToken Entity

use chrono::{DateTime, Utc};

/// What a token is for (Go `passwordreset.Purpose`): a reset (forgot
/// password, admin reset; 15 minutes) or a first-time invite ("set your
/// password"; 72 hours).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TokenPurpose {
    #[default]
    Reset,
    Invite,
}

crate::shared::enum_str::str_enum!(TokenPurpose, "password reset token purpose", {
    Reset => "reset",
    Invite => "invite",
});

pub struct PasswordResetToken {
    pub id: String,
    pub principal_id: String,
    pub token_hash: String,
    pub purpose: TokenPurpose,
    /// The confirm also clears the user's second factors (lost device).
    pub reset_2fa: bool,
    /// The confirm also needs a current authenticator (TOTP) code.
    pub requires_factor: bool,
    /// Wrong authenticator codes presented against this token.
    pub factor_attempts: i32,
    /// Where the SPA goes once the flow completes.
    pub redirect_uri: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

impl PasswordResetToken {
    pub fn new(
        principal_id: impl Into<String>,
        token_hash: impl Into<String>,
        expires_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id: crate::shared::tsid::generate(crate::EntityType::PasswordResetToken),
            principal_id: principal_id.into(),
            token_hash: token_hash.into(),
            purpose: TokenPurpose::Reset,
            reset_2fa: false,
            requires_factor: false,
            factor_attempts: 0,
            redirect_uri: None,
            expires_at,
            created_at: Utc::now(),
        }
    }

    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }

    /// A token is valid if it has not expired. Consumption (single-use) is
    /// enforced by deleting the token from the repository after use.
    pub fn is_valid(&self) -> bool {
        !self.is_expired()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn make_token(expires_at: DateTime<Utc>) -> PasswordResetToken {
        PasswordResetToken {
            id: "prt_test123".to_string(),
            principal_id: "prn_abc".to_string(),
            token_hash: "deadbeef".to_string(),
            purpose: TokenPurpose::Reset,
            reset_2fa: false,
            requires_factor: false,
            factor_attempts: 0,
            redirect_uri: None,
            expires_at,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn new_token_has_correct_fields() {
        let expires = Utc::now() + Duration::minutes(15);
        let token = PasswordResetToken::new("prn_abc", "somehash", expires);

        assert_eq!(token.principal_id, "prn_abc");
        assert_eq!(token.token_hash, "somehash");
        assert_eq!(token.expires_at, expires);
        // ID should have the password-reset-token prefix
        assert!(
            token.id.starts_with("prt_"),
            "expected prt_ prefix, got: {}",
            token.id
        );
    }

    #[test]
    fn token_with_future_expiry_is_not_expired() {
        let token = make_token(Utc::now() + Duration::hours(1));
        assert!(!token.is_expired());
        assert!(token.is_valid());
    }

    #[test]
    fn token_with_past_expiry_is_expired() {
        let token = make_token(Utc::now() - Duration::seconds(1));
        assert!(token.is_expired());
        assert!(!token.is_valid());
    }

    #[test]
    fn token_expired_far_in_the_past() {
        let token = make_token(Utc::now() - Duration::days(30));
        assert!(token.is_expired());
        assert!(!token.is_valid());
    }

    #[test]
    fn token_15_min_expiry_is_standard() {
        let expires = Utc::now() + Duration::minutes(15);
        let token = make_token(expires);
        assert!(!token.is_expired());

        // Verify the expiry is roughly 15 minutes from now
        let diff = token.expires_at - Utc::now();
        assert!(diff.num_seconds() > 14 * 60 && diff.num_seconds() <= 15 * 60);
    }

    #[test]
    fn different_tokens_get_different_ids() {
        let expires = Utc::now() + Duration::minutes(15);
        let t1 = PasswordResetToken::new("prn_abc", "hash1", expires);
        let t2 = PasswordResetToken::new("prn_abc", "hash2", expires);
        assert_ne!(t1.id, t2.id);
    }
}
