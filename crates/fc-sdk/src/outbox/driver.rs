//! Outbox Driver Trait
//!
//! Pluggable persistence backend for outbox messages.
//! Implement this trait to store outbox messages in your database.
//!
//! # Example
//!
//! ```ignore
//! use fc_sdk::outbox::{OutboxDriver, OutboxError, OutboxMessage};
//!
//! struct MyPgDriver { pool: sqlx::PgPool }
//!
//! #[async_trait::async_trait]
//! impl OutboxDriver for MyPgDriver {
//!     async fn insert(&self, message: OutboxMessage) -> Result<(), OutboxError> {
//!         sqlx::query("INSERT INTO outbox_messages ...")
//!             .bind(&message.id).execute(&self.pool).await?;
//!         Ok(())
//!     }
//!     async fn insert_batch(&self, messages: Vec<OutboxMessage>) -> Result<(), OutboxError> {
//!         for m in messages { self.insert(m).await?; }
//!         Ok(())
//!     }
//! }
//! ```

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::error::OutboxError;

/// Message types supported by the outbox: the row's `type` column.
///
/// This is [`fc_common::OutboxItemType`]; the `as_str()` / serde strings
/// (`EVENT`, `DISPATCH_JOB`, `AUDIT_LOG`) are what every SDK stores.
pub use fc_common::OutboxItemType as MessageType;

/// Outbox status codes, stored as integers in the `status` column.
///
/// Only [`OutboxStatus::Pending`] (0) is written by the SDK; all others are
/// managed by the processor.
pub use fc_common::OutboxStatus;

/// An outbox message record to be persisted by the driver.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxMessage {
    pub id: String,
    #[serde(rename = "type")]
    pub message_type: MessageType,
    pub message_group: Option<String>,
    pub payload: String,
    /// Serialized as the integer code, like the database column.
    #[serde(with = "status_code")]
    pub status: OutboxStatus,
    pub created_at: String,
    pub updated_at: String,
    pub client_id: String,
    pub payload_size: i32,
    pub headers: Option<HashMap<String, String>>,
}

/// Serde adapter: [`OutboxStatus`] as its integer storage code.
mod status_code {
    use super::OutboxStatus;
    use serde::{de::Error, Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(status: &OutboxStatus, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_i32(status.code())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<OutboxStatus, D::Error> {
        OutboxStatus::try_from(i32::deserialize(de)?).map_err(D::Error::custom)
    }
}

/// Driver interface for outbox persistence.
///
/// Implement this with your database client to participate in the same
/// transaction as your business logic. The SDK provides a built-in
/// [`SqlxPgDriver`](super::SqlxPgDriver) for PostgreSQL via sqlx.
#[async_trait]
pub trait OutboxDriver: Send + Sync {
    /// Insert a single message into the outbox.
    async fn insert(&self, message: OutboxMessage) -> Result<(), OutboxError>;

    /// Insert multiple messages into the outbox (batch).
    async fn insert_batch(&self, messages: Vec<OutboxMessage>) -> Result<(), OutboxError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── MessageType ────────────────────────────────────────────────────

    #[test]
    fn message_type_as_str() {
        assert_eq!(MessageType::Event.as_str(), "EVENT");
        assert_eq!(MessageType::DispatchJob.as_str(), "DISPATCH_JOB");
        assert_eq!(MessageType::AuditLog.as_str(), "AUDIT_LOG");
    }

    #[test]
    fn message_type_equality() {
        assert_eq!(MessageType::Event, MessageType::Event);
        assert_ne!(MessageType::Event, MessageType::DispatchJob);
        assert_ne!(MessageType::DispatchJob, MessageType::AuditLog);
    }

    #[test]
    fn message_type_serialization() {
        let json = serde_json::to_string(&MessageType::Event).unwrap();
        assert_eq!(json, r#""EVENT""#);

        let json = serde_json::to_string(&MessageType::DispatchJob).unwrap();
        assert_eq!(json, r#""DISPATCH_JOB""#);

        let json = serde_json::to_string(&MessageType::AuditLog).unwrap();
        assert_eq!(json, r#""AUDIT_LOG""#);
    }

    #[test]
    fn message_type_deserialization() {
        let mt: MessageType = serde_json::from_str(r#""EVENT""#).unwrap();
        assert_eq!(mt, MessageType::Event);

        let mt: MessageType = serde_json::from_str(r#""DISPATCH_JOB""#).unwrap();
        assert_eq!(mt, MessageType::DispatchJob);

        let mt: MessageType = serde_json::from_str(r#""AUDIT_LOG""#).unwrap();
        assert_eq!(mt, MessageType::AuditLog);
    }

    #[test]
    fn message_type_copy() {
        let a = MessageType::Event;
        let b = a; // Copy
        assert_eq!(a, b);
    }

    // ─── OutboxStatus ───────────────────────────────────────────────────

    #[test]
    fn outbox_status_codes() {
        assert_eq!(OutboxStatus::Pending.code(), 0);
        assert_eq!(OutboxStatus::Success.code(), 1);
        assert_eq!(OutboxStatus::BadRequest.code(), 2);
        assert_eq!(OutboxStatus::InternalError.code(), 3);
        assert_eq!(OutboxStatus::Unauthorized.code(), 4);
        assert_eq!(OutboxStatus::Forbidden.code(), 5);
        assert_eq!(OutboxStatus::GatewayError.code(), 6);
        assert_eq!(OutboxStatus::InProgress.code(), 9);
    }

    // ─── OutboxMessage ──────────────────────────────────────────────────

    #[test]
    fn outbox_message_serialization_round_trip() {
        let msg = OutboxMessage {
            id: "msg_1".to_string(),
            message_type: MessageType::Event,
            message_group: Some("grp:1".to_string()),
            payload: r#"{"key":"value"}"#.to_string(),
            status: OutboxStatus::Pending,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            client_id: "clt_test".to_string(),
            payload_size: 15,
            headers: Some({
                let mut h = HashMap::new();
                h.insert("X-Custom".to_string(), "val".to_string());
                h
            }),
        };

        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: OutboxMessage = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.id, "msg_1");
        assert_eq!(deserialized.message_type, MessageType::Event);
        assert_eq!(deserialized.message_group.as_deref(), Some("grp:1"));
        assert_eq!(deserialized.status, OutboxStatus::Pending);
        assert_eq!(deserialized.client_id, "clt_test");
        assert_eq!(deserialized.payload_size, 15);
        assert_eq!(deserialized.headers.unwrap()["X-Custom"], "val");
    }

    #[test]
    fn outbox_message_type_field_rename() {
        let msg = OutboxMessage {
            id: "m".into(),
            message_type: MessageType::AuditLog,
            message_group: None,
            payload: "{}".into(),
            status: OutboxStatus::Pending,
            created_at: "t".into(),
            updated_at: "t".into(),
            client_id: "c".into(),
            payload_size: 2,
            headers: None,
        };
        let json = serde_json::to_value(&msg).unwrap();
        // message_type serialized as "type", status as its integer code
        assert_eq!(json["type"], "AUDIT_LOG");
        assert_eq!(json["status"], 0);
        assert!(json.get("message_type").is_none());
    }

    #[test]
    fn outbox_message_optional_fields_none() {
        let msg = OutboxMessage {
            id: "m".into(),
            message_type: MessageType::Event,
            message_group: None,
            payload: "{}".into(),
            status: OutboxStatus::Pending,
            created_at: "t".into(),
            updated_at: "t".into(),
            client_id: "c".into(),
            payload_size: 2,
            headers: None,
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert!(json["message_group"].is_null());
        assert!(json["headers"].is_null());
    }

    #[test]
    fn outbox_message_clone() {
        let msg = OutboxMessage {
            id: "m".into(),
            message_type: MessageType::Event,
            message_group: Some("grp".into()),
            payload: "{}".into(),
            status: OutboxStatus::Pending,
            created_at: "t".into(),
            updated_at: "t".into(),
            client_id: "c".into(),
            payload_size: 2,
            headers: None,
        };
        let cloned = msg.clone();
        assert_eq!(cloned.id, msg.id);
        assert_eq!(cloned.message_group, msg.message_group);
    }
}
