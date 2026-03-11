//! Connection Entity

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ConnectionStatus {
    Active,
    Paused,
}

impl Default for ConnectionStatus {
    fn default() -> Self { Self::Active }
}

impl ConnectionStatus {
    pub fn as_str(&self) -> &'static str {
        match self { Self::Active => "ACTIVE", Self::Paused => "PAUSED" }
    }
    pub fn from_str(s: &str) -> Self {
        match s { "PAUSED" => Self::Paused, _ => Self::Active }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    pub id: String,
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub endpoint: String,
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
        endpoint: impl Into<String>,
        service_account_id: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: crate::TsidGenerator::generate(crate::EntityType::Connection),
            code: code.into(),
            name: name.into(),
            description: None,
            endpoint: endpoint.into(),
            external_id: None,
            status: ConnectionStatus::Active,
            service_account_id: service_account_id.into(),
            client_id: None,
            client_identifier: None,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self { self.description = Some(desc.into()); self }
    pub fn with_external_id(mut self, id: impl Into<String>) -> Self { self.external_id = Some(id.into()); self }
    pub fn with_client_id(mut self, id: impl Into<String>) -> Self { self.client_id = Some(id.into()); self }
    pub fn with_client_identifier(mut self, id: impl Into<String>) -> Self { self.client_identifier = Some(id.into()); self }

    pub fn pause(&mut self) {
        self.status = ConnectionStatus::Paused;
        self.updated_at = Utc::now();
    }

    pub fn activate(&mut self) {
        self.status = ConnectionStatus::Active;
        self.updated_at = Utc::now();
    }
}

impl From<crate::entities::msg_connections::Model> for Connection {
    fn from(m: crate::entities::msg_connections::Model) -> Self {
        Self {
            id: m.id,
            code: m.code,
            name: m.name,
            description: m.description,
            endpoint: m.endpoint,
            external_id: m.external_id,
            status: ConnectionStatus::from_str(&m.status),
            service_account_id: m.service_account_id,
            client_id: m.client_id,
            client_identifier: m.client_identifier,
            created_at: m.created_at.with_timezone(&Utc),
            updated_at: m.updated_at.with_timezone(&Utc),
        }
    }
}
