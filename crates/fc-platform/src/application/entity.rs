//! Application Entity — matches TypeScript Application domain

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ApplicationType {
    Application,
    Integration,
}

impl Default for ApplicationType {
    fn default() -> Self {
        Self::Application
    }
}

impl ApplicationType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Application => "APPLICATION",
            Self::Integration => "INTEGRATION",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "INTEGRATION" => Self::Integration,
            _ => Self::Application,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Application {
    pub id: String,
    #[serde(rename = "type")]
    pub application_type: ApplicationType,
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub icon_url: Option<String>,
    pub website: Option<String>,
    pub logo: Option<String>,
    pub logo_mime_type: Option<String>,
    pub default_base_url: Option<String>,
    pub service_account_id: Option<String>,
    pub active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Application {
    pub fn new(code: impl Into<String>, name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: crate::TsidGenerator::generate(crate::EntityType::Application),
            application_type: ApplicationType::Application,
            code: code.into(),
            name: name.into(),
            description: None,
            icon_url: None,
            website: None,
            logo: None,
            logo_mime_type: None,
            default_base_url: None,
            service_account_id: None,
            active: true,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn integration(code: impl Into<String>, name: impl Into<String>) -> Self {
        let mut app = Self::new(code, name);
        app.application_type = ApplicationType::Integration;
        app
    }

    pub fn is_integration(&self) -> bool {
        self.application_type == ApplicationType::Integration
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
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

    pub fn activate(&mut self) {
        self.active = true;
        self.updated_at = Utc::now();
    }

    pub fn deactivate(&mut self) {
        self.active = false;
        self.updated_at = Utc::now();
    }
}

impl From<crate::entities::app_applications::Model> for Application {
    fn from(m: crate::entities::app_applications::Model) -> Self {
        Self {
            id: m.id,
            application_type: ApplicationType::from_str(&m.r#type),
            code: m.code,
            name: m.name,
            description: m.description,
            icon_url: m.icon_url,
            website: m.website,
            logo: m.logo,
            logo_mime_type: m.logo_mime_type,
            default_base_url: m.default_base_url,
            service_account_id: m.service_account_id,
            active: m.active,
            created_at: m.created_at.with_timezone(&Utc),
            updated_at: m.updated_at.with_timezone(&Utc),
        }
    }
}
