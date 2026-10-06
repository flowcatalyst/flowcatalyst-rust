//! ApplicationClientConfig Entity — matches TypeScript ApplicationClientConfig

use chrono::{DateTime, Utc};
use fc_platform_core::shared::id::AppClientConfigId;
use fc_platform_core::shared::id::ApplicationId;
use fc_platform_core::shared::id::ClientId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationClientConfig {
    pub id: AppClientConfigId,
    pub application_id: ApplicationId,
    pub client_id: ClientId,
    pub enabled: bool,
    /// The base URL this client reaches the application at, when it is not
    /// the application's `default_base_url` (`app_client_configs.base_url_override`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url_override: Option<String>,
    /// The application's configuration document for this client
    /// (`app_client_configs.config_json`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_json: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ApplicationClientConfig {
    pub fn new(application_id: ApplicationId, client_id: ClientId) -> Self {
        let now = Utc::now();
        Self {
            id: AppClientConfigId::generate(),
            application_id,
            client_id,
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
