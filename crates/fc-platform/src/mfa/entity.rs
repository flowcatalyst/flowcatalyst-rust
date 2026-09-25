//! Two-factor records (Go `internal/platform/mfa/entity.go`): enrolled
//! factors, single-use recovery codes, pending email-PIN challenges and
//! remembered ("trusted") devices. Federated (OIDC) users never have any.

use chrono::{DateTime, Utc};
use serde::Serialize;

/// TSID prefixes, as Go's `tsid` (mfm/mrc/mep/mtd).
pub const METHOD_ID_PREFIX: &str = "mfm";
pub const RECOVERY_CODE_ID_PREFIX: &str = "mrc";
pub const EMAIL_PIN_ID_PREFIX: &str = "mep";
pub const TRUSTED_DEVICE_ID_PREFIX: &str = "mtd";

/// An enrolled second-factor mechanism.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum MethodType {
    /// An authenticator app (RFC 6238 TOTP).
    #[serde(rename = "TOTP")]
    Totp,
    /// A one-time numeric PIN sent by email.
    #[serde(rename = "EMAIL_PIN")]
    EmailPin,
}

crate::shared::enum_str::str_enum!(MethodType, "two-factor method", {
    Totp => "TOTP",
    EmailPin => "EMAIL_PIN",
});

/// A user's second factor. Unconfirmed until a first code verifies; an
/// unconfirmed factor is a pending enrolment and never satisfies a
/// challenge.
#[derive(Debug, Clone)]
pub struct Method {
    pub id: String,
    pub principal_id: String,
    pub method: MethodType,
    /// The encrypted TOTP secret (none for EMAIL_PIN).
    pub secret_encrypted: Option<String>,
    pub confirmed_at: Option<DateTime<Utc>>,
    /// For TOTP, the start of the last accepted time-step.
    pub last_used_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

impl Method {
    pub fn new(principal_id: &str, method: MethodType) -> Self {
        Self {
            id: fc_common::tsid::generate_with_prefix(METHOD_ID_PREFIX),
            principal_id: principal_id.to_string(),
            method,
            secret_encrypted: None,
            confirmed_at: None,
            last_used_at: None,
            created_at: Utc::now(),
        }
    }

    pub fn is_confirmed(&self) -> bool {
        self.confirmed_at.is_some()
    }
}

/// What an email PIN is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmailPinPurpose {
    /// A second-factor challenge at sign-in.
    Login,
    /// Proving inbox control while enrolling the email factor.
    Enroll,
}

crate::shared::enum_str::str_enum!(EmailPinPurpose, "email PIN purpose", {
    Login => "login",
    Enroll => "enroll",
});

/// A pending email-PIN challenge (only the PIN's hash is kept).
#[derive(Debug, Clone)]
pub struct EmailPin {
    pub id: String,
    pub principal_id: String,
    pub purpose: EmailPinPurpose,
    pub pin_hash: String,
    pub attempts: i32,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

impl EmailPin {
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }
}

/// A remembered browser, as the self-service list shows it (Go's JSON
/// tags; the token hash never leaves the server).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustedDevice {
    pub id: String,
    pub principal_id: String,
    #[serde(skip)]
    pub token_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<DateTime<Utc>>,
}
