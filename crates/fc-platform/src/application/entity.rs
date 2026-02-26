//! Application Entity
//!
//! Represents an application or integration in the platform.

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use bson::serde_helpers::chrono_datetime_as_bson_datetime;

/// Application type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ApplicationType {
    /// Full application with UI
    Application,
    /// Integration (M2M, no UI)
    Integration,
}

impl Default for ApplicationType {
    fn default() -> Self {
        Self::Application
    }
}

/// Application entity
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Application {
    /// TSID as Crockford Base32 string
    #[serde(rename = "_id")]
    pub id: String,

    /// Unique code (used in role/event type prefixes)
    /// e.g., "orders", "payments"
    pub code: String,

    /// Human-readable name
    pub name: String,

    /// Description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Application type
    #[serde(rename = "type")]
    #[serde(default)]
    pub application_type: ApplicationType,

    /// Icon URL for UI
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,

    /// Default base URL for the application
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_base_url: Option<String>,

    /// Associated service account ID (auto-created)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_account_id: Option<String>,

    /// Whether the application is active
    #[serde(default = "default_active")]
    pub active: bool,

    /// Audit fields
    #[serde(with = "chrono_datetime_as_bson_datetime")]
    pub created_at: DateTime<Utc>,
    #[serde(with = "chrono_datetime_as_bson_datetime")]
    pub updated_at: DateTime<Utc>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
}

fn default_active() -> bool {
    true
}

impl Application {
    pub fn new(code: impl Into<String>, name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: crate::TsidGenerator::generate(),
            code: code.into(),
            name: name.into(),
            description: None,
            application_type: ApplicationType::Application,
            icon_url: None,
            default_base_url: None,
            service_account_id: None,
            active: true,
            created_at: now,
            updated_at: now,
            created_by: None,
        }
    }

    pub fn integration(code: impl Into<String>, name: impl Into<String>) -> Self {
        let mut app = Self::new(code, name);
        app.application_type = ApplicationType::Integration;
        app
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.default_base_url = Some(url.into());
        self
    }

    pub fn with_icon_url(mut self, url: impl Into<String>) -> Self {
        self.icon_url = Some(url.into());
        self
    }

    pub fn with_service_account(mut self, service_account_id: impl Into<String>) -> Self {
        self.service_account_id = Some(service_account_id.into());
        self
    }

    pub fn deactivate(&mut self) {
        self.active = false;
        self.updated_at = Utc::now();
    }

    pub fn activate(&mut self) {
        self.active = true;
        self.updated_at = Utc::now();
    }

    pub fn is_integration(&self) -> bool {
        self.application_type == ApplicationType::Integration
    }
}
