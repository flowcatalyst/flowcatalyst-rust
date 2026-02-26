//! Audit Log Entity
//!
//! Records all significant actions in the platform for compliance and debugging.

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use bson::serde_helpers::chrono_datetime_as_bson_datetime;

/// Audit action type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AuditAction {
    /// Entity created
    Create,
    /// Entity updated
    Update,
    /// Entity deleted
    Delete,
    /// Entity archived
    Archive,
    /// Login attempt
    Login,
    /// Logout
    Logout,
    /// Token issued
    TokenIssued,
    /// Token revoked
    TokenRevoked,
    /// Permission granted
    PermissionGranted,
    /// Permission revoked
    PermissionRevoked,
    /// Role assigned
    RoleAssigned,
    /// Role unassigned
    RoleUnassigned,
    /// Client access granted
    ClientAccessGranted,
    /// Client access revoked
    ClientAccessRevoked,
    /// Subscription paused
    SubscriptionPaused,
    /// Subscription resumed
    SubscriptionResumed,
    /// Dispatch pool paused
    PoolPaused,
    /// Dispatch pool resumed
    PoolResumed,
    /// Configuration changed
    ConfigChanged,
    /// Other custom action
    Other,
}

/// Audit log entry (matches Java AuditLog schema)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditLog {
    /// TSID as Crockford Base32 string
    #[serde(rename = "_id")]
    pub id: String,

    /// Entity type affected (e.g., "Client", "Principal", "Role")
    pub entity_type: String,

    /// Entity ID affected
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_id: Option<String>,

    /// Operation name - the command class simple name (e.g., "GrantClientAccessCommand")
    /// This matches Java's AuditLog.operation field
    pub operation: String,

    /// Full operation payload as JSON string
    /// This matches Java's AuditLog.operationJson field
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_json: Option<String>,

    /// Principal who performed the action
    #[serde(skip_serializing_if = "Option::is_none")]
    pub principal_id: Option<String>,

    /// Timestamp (matches Java's performedAt)
    #[serde(alias = "createdAt", with = "chrono_datetime_as_bson_datetime")]
    pub performed_at: DateTime<Utc>,
}

impl AuditLog {
    /// Create a new audit log entry (matches Java schema)
    pub fn new(
        entity_type: impl Into<String>,
        entity_id: Option<String>,
        operation: impl Into<String>,
        operation_json: Option<String>,
        principal_id: Option<String>,
    ) -> Self {
        Self {
            id: crate::TsidGenerator::generate(),
            entity_type: entity_type.into(),
            entity_id,
            operation: operation.into(),
            operation_json,
            principal_id,
            performed_at: Utc::now(),
        }
    }

    /// Create from a command (for use in UnitOfWork)
    pub fn from_command<C: serde::Serialize>(
        entity_type: impl Into<String>,
        entity_id: impl Into<String>,
        command: &C,
        principal_id: Option<String>,
    ) -> Self {
        let command_name = std::any::type_name::<C>()
            .rsplit("::")
            .next()
            .unwrap_or("Unknown")
            .to_string();

        let operation_json = serde_json::to_string(command).ok();

        Self {
            id: crate::TsidGenerator::generate(),
            entity_type: entity_type.into(),
            entity_id: Some(entity_id.into()),
            operation: command_name,
            operation_json,
            principal_id,
            performed_at: Utc::now(),
        }
    }

    pub fn with_principal(mut self, principal_id: impl Into<String>) -> Self {
        self.principal_id = Some(principal_id.into());
        self
    }

    pub fn with_performed_at(mut self, time: DateTime<Utc>) -> Self {
        self.performed_at = time;
        self
    }
}

// Note: AuditAction enum is kept for backwards compatibility when reading old Rust-created audit logs,
// but new audit logs use the operation field (command class name) like Java does.
