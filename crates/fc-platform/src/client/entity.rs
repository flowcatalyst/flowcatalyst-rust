//! Client Entity
//!
//! Represents a tenant/organization in the multi-tenant system.

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

/// Client status — matches TypeScript ClientStatus enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ClientStatus {
    /// Client is active and operational
    Active,
    /// Client is inactive
    Inactive,
    /// Client is suspended (temporarily disabled)
    Suspended,
}

impl Default for ClientStatus {
    fn default() -> Self {
        Self::Active
    }
}

impl ClientStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::Inactive => "INACTIVE",
            Self::Suspended => "SUSPENDED",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "ACTIVE" => Self::Active,
            "INACTIVE" => Self::Inactive,
            "SUSPENDED" => Self::Suspended,
            _ => Self::Active,
        }
    }
}

/// Client note for audit trail (stored as JSONB in PostgreSQL)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientNote {
    /// Note category
    pub category: String,

    /// Note text
    pub text: String,

    /// Who added the note
    #[serde(skip_serializing_if = "Option::is_none")]
    pub added_by: Option<String>,

    /// When the note was added (ISO 8601 string)
    pub added_at: DateTime<Utc>,
}

impl ClientNote {
    pub fn new(category: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            category: category.into(),
            text: text.into(),
            added_by: None,
            added_at: Utc::now(),
        }
    }

    pub fn with_author(mut self, author: impl Into<String>) -> Self {
        self.added_by = Some(author.into());
        self
    }
}

/// Client entity - represents a tenant/organization
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Client {
    /// TSID as Crockford Base32 string
    pub id: String,

    /// Human-readable name
    pub name: String,

    /// Unique identifier/slug (URL-safe)
    pub identifier: String,

    /// Current status
    #[serde(default)]
    pub status: ClientStatus,

    /// Reason for current status
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_reason: Option<String>,

    /// When status was last changed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_changed_at: Option<DateTime<Utc>>,

    /// Audit notes (JSONB in PostgreSQL)
    #[serde(default)]
    pub notes: Vec<ClientNote>,

    /// Audit fields
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Client {
    pub fn new(name: impl Into<String>, identifier: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: crate::TsidGenerator::generate(crate::EntityType::Client),
            name: name.into(),
            identifier: identifier.into(),
            status: ClientStatus::Active,
            status_reason: None,
            status_changed_at: None,
            notes: vec![],
            created_at: now,
            updated_at: now,
        }
    }

    pub fn add_note(&mut self, note: ClientNote) {
        self.notes.push(note);
        self.updated_at = Utc::now();
    }

    pub fn set_status(&mut self, status: ClientStatus, reason: Option<String>) {
        self.status = status;
        self.status_reason = reason;
        self.status_changed_at = Some(Utc::now());
        self.updated_at = Utc::now();
    }

    pub fn suspend(&mut self, reason: impl Into<String>) {
        self.set_status(ClientStatus::Suspended, Some(reason.into()));
    }

    pub fn activate(&mut self) {
        self.set_status(ClientStatus::Active, None);
    }

    pub fn deactivate(&mut self, reason: Option<String>) {
        self.set_status(ClientStatus::Inactive, reason);
    }

    pub fn is_active(&self) -> bool {
        self.status == ClientStatus::Active
    }

    pub fn is_suspended(&self) -> bool {
        self.status == ClientStatus::Suspended
    }

    pub fn is_inactive(&self) -> bool {
        self.status == ClientStatus::Inactive
    }
}

/// Conversion from SeaORM database model to domain entity
impl From<crate::entities::tnt_clients::Model> for Client {
    fn from(model: crate::entities::tnt_clients::Model) -> Self {
        let notes: Vec<ClientNote> = model
            .notes
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();

        Self {
            id: model.id,
            name: model.name,
            identifier: model.identifier,
            status: ClientStatus::from_str(&model.status),
            status_reason: model.status_reason,
            status_changed_at: model.status_changed_at.map(|dt| dt.with_timezone(&Utc)),
            notes,
            created_at: model.created_at.with_timezone(&Utc),
            updated_at: model.updated_at.with_timezone(&Utc),
        }
    }
}
