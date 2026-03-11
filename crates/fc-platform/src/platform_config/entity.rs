//! PlatformConfig Entity

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ConfigScope {
    Global,
    Client,
}

impl ConfigScope {
    pub fn as_str(&self) -> &'static str {
        match self { Self::Global => "GLOBAL", Self::Client => "CLIENT" }
    }
    pub fn from_str(s: &str) -> Self {
        match s { "CLIENT" => Self::Client, _ => Self::Global }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ConfigValueType {
    Plain,
    Secret,
}

impl ConfigValueType {
    pub fn as_str(&self) -> &'static str {
        match self { Self::Plain => "PLAIN", Self::Secret => "SECRET" }
    }
    pub fn from_str(s: &str) -> Self {
        match s { "SECRET" => Self::Secret, _ => Self::Plain }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformConfig {
    pub id: String,
    pub application_code: String,
    pub section: String,
    pub property: String,
    pub scope: ConfigScope,
    pub client_id: Option<String>,
    pub value_type: ConfigValueType,
    pub value: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl PlatformConfig {
    pub fn new(
        application_code: impl Into<String>,
        section: impl Into<String>,
        property: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: crate::TsidGenerator::generate(crate::EntityType::PlatformConfig),
            application_code: application_code.into(),
            section: section.into(),
            property: property.into(),
            scope: ConfigScope::Global,
            client_id: None,
            value_type: ConfigValueType::Plain,
            value: value.into(),
            description: None,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn masked_value(&self) -> &str {
        if self.value_type == ConfigValueType::Secret { "***" } else { &self.value }
    }
}

impl From<crate::entities::app_platform_configs::Model> for PlatformConfig {
    fn from(m: crate::entities::app_platform_configs::Model) -> Self {
        Self {
            id: m.id,
            application_code: m.application_code,
            section: m.section,
            property: m.property,
            scope: ConfigScope::from_str(&m.scope),
            client_id: m.client_id,
            value_type: ConfigValueType::from_str(&m.value_type),
            value: m.value,
            description: m.description,
            created_at: m.created_at.with_timezone(&Utc),
            updated_at: m.updated_at.with_timezone(&Utc),
        }
    }
}
