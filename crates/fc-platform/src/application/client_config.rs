//! ApplicationClientConfig Entity — matches TypeScript ApplicationClientConfig

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationClientConfig {
    pub id: String,
    pub application_id: String,
    pub client_id: String,
    pub enabled: bool,
    /// Transient: not stored in DB, used by API layer
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url_override: Option<String>,
    /// Transient: not stored in DB, used by API layer
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_json: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ApplicationClientConfig {
    pub fn new(application_id: impl Into<String>, client_id: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: crate::TsidGenerator::generate(crate::EntityType::AppClientConfig),
            application_id: application_id.into(),
            client_id: client_id.into(),
            enabled: true,
            base_url_override: None,
            config_json: None,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn enable(&mut self) {
        self.enabled = true;
        self.updated_at = Utc::now();
    }

    pub fn disable(&mut self) {
        self.enabled = false;
        self.updated_at = Utc::now();
    }
}

impl From<crate::entities::app_client_configs::Model> for ApplicationClientConfig {
    fn from(m: crate::entities::app_client_configs::Model) -> Self {
        Self {
            id: m.id,
            application_id: m.application_id,
            client_id: m.client_id,
            enabled: m.enabled,
            base_url_override: None,
            config_json: None,
            created_at: m.created_at.with_timezone(&Utc),
            updated_at: m.updated_at.with_timezone(&Utc),
        }
    }
}
