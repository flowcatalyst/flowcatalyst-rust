//! Application Client Configuration Entity
//!
//! Manages the relationship between applications and clients.
//! Controls which applications a client has access to.

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use bson::serde_helpers::chrono_datetime_as_bson_datetime;
use std::collections::HashMap;

/// Application-Client configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationClientConfig {
    /// TSID as Crockford Base32 string
    #[serde(rename = "_id")]
    pub id: String,

    /// Application ID
    pub application_id: String,

    /// Client ID
    pub client_id: String,

    /// Whether the application is enabled for this client
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Client-specific base URL override
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url_override: Option<String>,

    /// Custom configuration JSON
    #[serde(default)]
    pub config_json: HashMap<String, serde_json::Value>,

    /// Audit fields
    #[serde(with = "chrono_datetime_as_bson_datetime")]
    pub created_at: DateTime<Utc>,
    #[serde(with = "chrono_datetime_as_bson_datetime")]
    pub updated_at: DateTime<Utc>,
}

fn default_enabled() -> bool {
    true
}

impl ApplicationClientConfig {
    pub fn new(application_id: impl Into<String>, client_id: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: crate::TsidGenerator::generate(),
            application_id: application_id.into(),
            client_id: client_id.into(),
            enabled: true,
            base_url_override: None,
            config_json: HashMap::new(),
            created_at: now,
            updated_at: now,
        }
    }

    pub fn with_base_url_override(mut self, url: impl Into<String>) -> Self {
        self.base_url_override = Some(url.into());
        self
    }

    pub fn enable(&mut self) {
        self.enabled = true;
        self.updated_at = Utc::now();
    }

    pub fn disable(&mut self) {
        self.enabled = false;
        self.updated_at = Utc::now();
    }

    pub fn set_base_url_override(&mut self, url: Option<String>) {
        self.base_url_override = url;
        self.updated_at = Utc::now();
    }

    pub fn set_config(&mut self, key: impl Into<String>, value: serde_json::Value) {
        self.config_json.insert(key.into(), value);
        self.updated_at = Utc::now();
    }
}
