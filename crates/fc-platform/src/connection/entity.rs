//! Connection Entity

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[derive(Default)]
pub enum ConnectionStatus {
    #[default]
    Active,
    Paused,
}

impl ConnectionStatus {
    pub fn from_str(s: &str) -> Self {
        match s {
            "PAUSED" => Self::Paused,
            _ => Self::Active,
        }
    }
}

crate::shared::enum_str::str_enum!(ConnectionStatus, "connection status", {
    Active => "ACTIVE",
    Paused => "PAUSED",
});

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    pub id: String,
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub external_id: Option<String>,
    pub status: ConnectionStatus,
    pub service_account_id: String,
    pub client_id: Option<String>,
    pub client_identifier: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Connection {
    pub fn new(
        code: impl Into<String>,
        name: impl Into<String>,
        service_account_id: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: crate::shared::tsid::generate(crate::EntityType::Connection),
            code: code.into(),
            name: name.into(),
            description: None,
            external_id: None,
            status: ConnectionStatus::Active,
            service_account_id: service_account_id.into(),
            client_id: None,
            client_identifier: None,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }
    pub fn with_external_id(mut self, id: impl Into<String>) -> Self {
        self.external_id = Some(id.into());
        self
    }
    pub fn with_client_id(mut self, id: impl Into<String>) -> Self {
        self.client_id = Some(id.into());
        self
    }
    pub fn with_client_identifier(mut self, id: impl Into<String>) -> Self {
        self.client_identifier = Some(id.into());
        self
    }

    pub fn pause(&mut self) {
        self.status = ConnectionStatus::Paused;
        self.updated_at = Utc::now();
    }

    pub fn activate(&mut self) {
        self.status = ConnectionStatus::Active;
        self.updated_at = Utc::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_connection() {
        let conn = Connection::new("webhook-1", "Webhook Connection", "sa-123");

        assert!(!conn.id.is_empty());
        assert!(
            conn.id.starts_with("con_"),
            "ID should have con_ prefix, got: {}",
            conn.id
        );
        assert_eq!(conn.code, "webhook-1");
        assert_eq!(conn.name, "Webhook Connection");
        assert_eq!(conn.service_account_id, "sa-123");
        assert!(conn.description.is_none());
        assert!(conn.external_id.is_none());
        assert_eq!(conn.status, ConnectionStatus::Active);
        assert!(conn.client_id.is_none());
        assert!(conn.client_identifier.is_none());
        assert_eq!(conn.created_at, conn.updated_at);
    }

    #[test]
    fn test_connection_builder_methods() {
        let conn = Connection::new("c1", "Connection 1", "sa-1")
            .with_description("A test connection")
            .with_external_id("ext-123")
            .with_client_id("client-1")
            .with_client_identifier("client-ident-1");

        assert_eq!(conn.description, Some("A test connection".to_string()));
        assert_eq!(conn.external_id, Some("ext-123".to_string()));
        assert_eq!(conn.client_id, Some("client-1".to_string()));
        assert_eq!(conn.client_identifier, Some("client-ident-1".to_string()));
    }

    #[test]
    fn test_connection_pause_and_activate() {
        let mut conn = Connection::new("c1", "Connection", "sa-1");
        assert_eq!(conn.status, ConnectionStatus::Active);

        conn.pause();
        assert_eq!(conn.status, ConnectionStatus::Paused);

        conn.activate();
        assert_eq!(conn.status, ConnectionStatus::Active);
    }

    #[test]
    fn test_connection_status_as_str() {
        assert_eq!(ConnectionStatus::Active.as_str(), "ACTIVE");
        assert_eq!(ConnectionStatus::Paused.as_str(), "PAUSED");
    }

    #[test]
    fn test_connection_status_from_str() {
        assert_eq!(
            ConnectionStatus::from_str("ACTIVE"),
            ConnectionStatus::Active
        );
        assert_eq!(
            ConnectionStatus::from_str("PAUSED"),
            ConnectionStatus::Paused
        );
        // Default/fallback is Active
        assert_eq!(
            ConnectionStatus::from_str("unknown"),
            ConnectionStatus::Active
        );
        assert_eq!(ConnectionStatus::from_str(""), ConnectionStatus::Active);
    }

    #[test]
    fn test_connection_status_default() {
        let status = ConnectionStatus::default();
        assert_eq!(status, ConnectionStatus::Active);
    }

    #[test]
    fn test_connection_status_serialization() {
        let json = serde_json::to_string(&ConnectionStatus::Active).unwrap();
        assert_eq!(json, "\"ACTIVE\"");

        let json = serde_json::to_string(&ConnectionStatus::Paused).unwrap();
        assert_eq!(json, "\"PAUSED\"");
    }

    #[test]
    fn test_connection_unique_ids() {
        let c1 = Connection::new("a", "A", "sa-1");
        let c2 = Connection::new("b", "B", "sa-2");
        assert_ne!(c1.id, c2.id);
    }

    #[test]
    fn test_pause_updates_timestamp() {
        let mut conn = Connection::new("c1", "C", "sa-1");
        let original_updated_at = conn.updated_at;
        // Small sleep not needed; Utc::now() granularity is sufficient for >=
        conn.pause();
        assert!(conn.updated_at >= original_updated_at);
    }
}
