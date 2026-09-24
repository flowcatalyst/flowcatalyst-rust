//! Client Entity
//!
//! Represents a tenant/organization in the multi-tenant system.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Client status — matches TypeScript ClientStatus enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[derive(Default)]
pub enum ClientStatus {
    /// Client is active and operational
    #[default]
    Active,
    /// Client is inactive
    Inactive,
    /// Client is suspended (temporarily disabled)
    Suspended,
}

impl ClientStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::Inactive => "INACTIVE",
            Self::Suspended => "SUSPENDED",
        }
    }

    // Lenient: unknown input maps to Active by design (legacy DB rows).
    #[allow(clippy::should_implement_trait)]
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
            id: crate::shared::tsid::generate(crate::EntityType::Client),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_client() {
        let client = Client::new("Acme Corp", "acme-corp");

        assert!(!client.id.is_empty());
        assert!(
            client.id.starts_with("clt_"),
            "ID should have clt_ prefix, got: {}",
            client.id
        );
        assert_eq!(
            client.id.len(),
            17,
            "Typed ID should be 17 chars, got: {}",
            client.id.len()
        );
        assert_eq!(client.name, "Acme Corp");
        assert_eq!(client.identifier, "acme-corp");
        assert_eq!(client.status, ClientStatus::Active);
        assert!(client.status_reason.is_none());
        assert!(client.status_changed_at.is_none());
        assert!(client.notes.is_empty());
        assert_eq!(client.created_at, client.updated_at);
    }

    #[test]
    fn test_client_unique_ids() {
        let c1 = Client::new("A", "a");
        let c2 = Client::new("B", "b");
        assert_ne!(c1.id, c2.id);
    }

    #[test]
    fn test_client_status_as_str() {
        assert_eq!(ClientStatus::Active.as_str(), "ACTIVE");
        assert_eq!(ClientStatus::Inactive.as_str(), "INACTIVE");
        assert_eq!(ClientStatus::Suspended.as_str(), "SUSPENDED");
    }

    #[test]
    fn test_client_status_from_str() {
        assert_eq!(ClientStatus::from_str("ACTIVE"), ClientStatus::Active);
        assert_eq!(ClientStatus::from_str("INACTIVE"), ClientStatus::Inactive);
        assert_eq!(ClientStatus::from_str("SUSPENDED"), ClientStatus::Suspended);
        // Unknown values default to Active
        assert_eq!(ClientStatus::from_str("unknown"), ClientStatus::Active);
        assert_eq!(ClientStatus::from_str(""), ClientStatus::Active);
    }

    #[test]
    fn test_client_status_default() {
        assert_eq!(ClientStatus::default(), ClientStatus::Active);
    }

    #[test]
    fn test_client_status_roundtrip() {
        for status in [
            ClientStatus::Active,
            ClientStatus::Inactive,
            ClientStatus::Suspended,
        ] {
            let s = status.as_str();
            assert_eq!(
                ClientStatus::from_str(s),
                status,
                "Roundtrip failed for {:?}",
                status
            );
        }
    }

    #[test]
    fn test_client_suspend() {
        let mut client = Client::new("Test", "test");
        assert!(client.is_active());
        assert!(!client.is_suspended());

        client.suspend("Payment overdue");
        assert!(!client.is_active());
        assert!(client.is_suspended());
        assert_eq!(client.status, ClientStatus::Suspended);
        assert_eq!(client.status_reason, Some("Payment overdue".to_string()));
        assert!(client.status_changed_at.is_some());
    }

    #[test]
    fn test_client_activate() {
        let mut client = Client::new("Test", "test");
        client.suspend("Test reason");

        client.activate();
        assert!(client.is_active());
        assert!(!client.is_suspended());
        assert_eq!(client.status, ClientStatus::Active);
        assert!(client.status_reason.is_none());
        assert!(client.status_changed_at.is_some());
    }

    #[test]
    fn test_client_deactivate() {
        let mut client = Client::new("Test", "test");

        client.deactivate(Some("No longer needed".to_string()));
        assert!(client.is_inactive());
        assert!(!client.is_active());
        assert_eq!(client.status, ClientStatus::Inactive);
        assert_eq!(client.status_reason, Some("No longer needed".to_string()));
    }

    #[test]
    fn test_client_deactivate_no_reason() {
        let mut client = Client::new("Test", "test");
        client.deactivate(None);
        assert!(client.is_inactive());
        assert!(client.status_reason.is_none());
    }

    #[test]
    fn test_client_status_transitions() {
        let mut client = Client::new("Test", "test");
        assert!(client.is_active());

        // Active -> Suspended
        client.suspend("billing");
        assert!(client.is_suspended());

        // Suspended -> Active
        client.activate();
        assert!(client.is_active());

        // Active -> Inactive
        client.deactivate(Some("done".to_string()));
        assert!(client.is_inactive());

        // Inactive -> Active
        client.activate();
        assert!(client.is_active());
    }

    #[test]
    fn test_client_add_note() {
        let mut client = Client::new("Test", "test");
        assert!(client.notes.is_empty());

        let note = ClientNote::new("general", "First note");
        client.add_note(note);
        assert_eq!(client.notes.len(), 1);
        assert_eq!(client.notes[0].category, "general");
        assert_eq!(client.notes[0].text, "First note");
    }

    #[test]
    fn test_client_note_with_author() {
        let note = ClientNote::new("billing", "Payment received").with_author("admin@example.com");
        assert_eq!(note.added_by, Some("admin@example.com".to_string()));
    }

    #[test]
    fn test_client_set_status() {
        let mut client = Client::new("Test", "test");
        let before = client.updated_at;

        client.set_status(ClientStatus::Suspended, Some("reason".to_string()));
        assert_eq!(client.status, ClientStatus::Suspended);
        assert_eq!(client.status_reason, Some("reason".to_string()));
        assert!(client.status_changed_at.is_some());
        assert!(client.updated_at >= before);
    }
}
