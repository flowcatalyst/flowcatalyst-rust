//! PasswordResetToken Entity

use chrono::{DateTime, Utc};

pub struct PasswordResetToken {
    pub id: String,
    pub principal_id: String,
    pub token_hash: String,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

impl PasswordResetToken {
    pub fn new(principal_id: impl Into<String>, token_hash: impl Into<String>, expires_at: DateTime<Utc>) -> Self {
        Self {
            id: crate::TsidGenerator::generate(crate::EntityType::PasswordResetToken),
            principal_id: principal_id.into(),
            token_hash: token_hash.into(),
            expires_at,
            created_at: Utc::now(),
        }
    }

    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }
}

impl From<crate::entities::iam_password_reset_tokens::Model> for PasswordResetToken {
    fn from(m: crate::entities::iam_password_reset_tokens::Model) -> Self {
        Self {
            id: m.id,
            principal_id: m.principal_id,
            token_hash: m.token_hash,
            expires_at: m.expires_at.with_timezone(&Utc),
            created_at: m.created_at.with_timezone(&Utc),
        }
    }
}
