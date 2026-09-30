//! The outbox row's status codes and item types.
//!
//! Every SDK (Rust, TypeScript, Laravel, Go) and the outbox processor read
//! and write these values, so they are a storage contract.

use serde::{Deserialize, Serialize};
use std::error;
use std::fmt;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

/// Outbox row status.
///
/// Stored as the integer discriminant (`status` column); every SDK (Rust,
/// TypeScript, Laravel, Go) writes and reads these codes, so they must never
/// change. Only [`OutboxStatus::Pending`] is written by producers; the rest are
/// set by the outbox processor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[repr(i32)]
pub enum OutboxStatus {
    /// Waiting to be picked up.
    #[default]
    Pending = 0,
    /// Accepted by the platform.
    Success = 1,
    /// Client error (4xx), won't retry.
    BadRequest = 2,
    /// Server error (5xx), will retry.
    InternalError = 3,
    /// Authentication failed, will retry.
    Unauthorized = 4,
    /// Permission denied, won't retry.
    Forbidden = 5,
    /// Gateway/upstream error, will retry.
    GatewayError = 6,
    /// Claimed by a processor.
    InProgress = 9,
}

/// An integer read from the `status` column that is not an [`OutboxStatus`] code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnknownOutboxStatus(pub i32);

impl Display for UnknownOutboxStatus {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "unknown outbox status code {}", self.0)
    }
}

impl error::Error for UnknownOutboxStatus {}

impl OutboxStatus {
    /// Every status, in code order.
    pub const ALL: [OutboxStatus; 8] = [
        OutboxStatus::Pending,
        OutboxStatus::Success,
        OutboxStatus::BadRequest,
        OutboxStatus::InternalError,
        OutboxStatus::Unauthorized,
        OutboxStatus::Forbidden,
        OutboxStatus::GatewayError,
        OutboxStatus::InProgress,
    ];

    /// The integer stored in the `status` column.
    pub const fn code(self) -> i32 {
        self as i32
    }

    /// Check if this status is retryable
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            OutboxStatus::InternalError
                | OutboxStatus::Unauthorized
                | OutboxStatus::GatewayError
                | OutboxStatus::InProgress
        )
    }

    /// Check if this status is terminal (won't retry)
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            OutboxStatus::Success | OutboxStatus::BadRequest | OutboxStatus::Forbidden
        )
    }
}

impl From<OutboxStatus> for i32 {
    fn from(status: OutboxStatus) -> i32 {
        status.code()
    }
}

impl TryFrom<i32> for OutboxStatus {
    type Error = UnknownOutboxStatus;

    /// Strict: an unknown code is an error, never a default (owner ruling X-06).
    fn try_from(code: i32) -> Result<Self, Self::Error> {
        Self::ALL
            .into_iter()
            .find(|s| s.code() == code)
            .ok_or(UnknownOutboxStatus(code))
    }
}

/// Outbox item type, stored as the row's `type` string ([`OutboxItemType::as_str`]).
///
/// Serde uses the same strings, so [`OutboxItemType::as_str`] is the single
/// source of truth for every representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(into = "&'static str", try_from = "String")]
pub enum OutboxItemType {
    /// Event items - sent to /api/events/batch
    #[default]
    Event,
    /// Dispatch job items - sent to /api/dispatch-jobs/batch
    DispatchJob,
    /// Audit log items - sent to /api/audit-logs/batch
    AuditLog,
}

/// A string that is not an [`OutboxItemType`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownOutboxItemType(pub String);

impl Display for UnknownOutboxItemType {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "unknown outbox item type {:?}", self.0)
    }
}

impl error::Error for UnknownOutboxItemType {}

impl OutboxItemType {
    /// All item types for iteration
    pub const ALL: [OutboxItemType; 3] = [
        OutboxItemType::Event,
        OutboxItemType::DispatchJob,
        OutboxItemType::AuditLog,
    ];

    /// The value of the `type` column (and the serde string).
    pub const fn as_str(self) -> &'static str {
        match self {
            OutboxItemType::Event => "EVENT",
            OutboxItemType::DispatchJob => "DISPATCH_JOB",
            OutboxItemType::AuditLog => "AUDIT_LOG",
        }
    }

    /// Get the API endpoint path for this item type
    pub fn api_path(&self) -> &'static str {
        match self {
            OutboxItemType::Event => "/api/events/batch",
            OutboxItemType::DispatchJob => "/api/dispatch-jobs/batch",
            OutboxItemType::AuditLog => "/api/audit-logs/batch",
        }
    }
}

impl Display for OutboxItemType {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<OutboxItemType> for &'static str {
    fn from(item_type: OutboxItemType) -> Self {
        item_type.as_str()
    }
}

impl FromStr for OutboxItemType {
    type Err = UnknownOutboxItemType;

    /// Strict, exact match on [`OutboxItemType::as_str`] (owner ruling X-06).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|t| t.as_str() == s)
            .ok_or_else(|| UnknownOutboxItemType(s.to_string()))
    }
}

impl TryFrom<String> for OutboxItemType {
    type Error = UnknownOutboxItemType;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outbox_status_codes_are_the_storage_contract() {
        let expected = [
            (OutboxStatus::Pending, 0),
            (OutboxStatus::Success, 1),
            (OutboxStatus::BadRequest, 2),
            (OutboxStatus::InternalError, 3),
            (OutboxStatus::Unauthorized, 4),
            (OutboxStatus::Forbidden, 5),
            (OutboxStatus::GatewayError, 6),
            (OutboxStatus::InProgress, 9),
        ];
        assert_eq!(OutboxStatus::ALL.len(), expected.len());
        for (status, code) in expected {
            assert_eq!(status.code(), code);
            assert_eq!(i32::from(status), code);
            assert_eq!(OutboxStatus::try_from(code), Ok(status));
        }
    }

    #[test]
    fn outbox_status_unknown_code_is_an_error() {
        for code in [-1, 7, 8, 10, 99] {
            assert_eq!(OutboxStatus::try_from(code), Err(UnknownOutboxStatus(code)));
        }
    }

    #[test]
    fn outbox_status_serde_strings() {
        let expected = [
            (OutboxStatus::Pending, "PENDING"),
            (OutboxStatus::Success, "SUCCESS"),
            (OutboxStatus::BadRequest, "BAD_REQUEST"),
            (OutboxStatus::InternalError, "INTERNAL_ERROR"),
            (OutboxStatus::Unauthorized, "UNAUTHORIZED"),
            (OutboxStatus::Forbidden, "FORBIDDEN"),
            (OutboxStatus::GatewayError, "GATEWAY_ERROR"),
            (OutboxStatus::InProgress, "IN_PROGRESS"),
        ];
        for (status, s) in expected {
            assert_eq!(serde_json::to_value(status).unwrap(), s);
            assert_eq!(
                serde_json::from_value::<OutboxStatus>(serde_json::json!(s)).unwrap(),
                status
            );
        }
    }

    #[test]
    fn outbox_item_type_strings_are_the_storage_contract() {
        let expected = [
            (OutboxItemType::Event, "EVENT"),
            (OutboxItemType::DispatchJob, "DISPATCH_JOB"),
            (OutboxItemType::AuditLog, "AUDIT_LOG"),
        ];
        assert_eq!(OutboxItemType::ALL.len(), expected.len());
        for (t, s) in expected {
            assert_eq!(t.as_str(), s);
            assert_eq!(t.to_string(), s);
            assert_eq!(s.parse::<OutboxItemType>(), Ok(t));
            assert_eq!(serde_json::to_value(t).unwrap(), s);
            assert_eq!(
                serde_json::from_value::<OutboxItemType>(serde_json::json!(s)).unwrap(),
                t
            );
        }
    }

    #[test]
    fn outbox_item_type_parse_is_strict() {
        for bad in ["event", "DISPATCH-JOB", "AUDITLOG", ""] {
            assert!(bad.parse::<OutboxItemType>().is_err(), "{bad}");
            assert!(serde_json::from_value::<OutboxItemType>(serde_json::json!(bad)).is_err());
        }
    }
}
