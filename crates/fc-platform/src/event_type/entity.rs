//! EventType Entity — matches TypeScript EventType domain

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EventTypeStatus {
    Current,
    Archived,
}

impl Default for EventTypeStatus {
    fn default() -> Self { Self::Current }
}

impl EventTypeStatus {
    pub fn as_str(&self) -> &'static str {
        match self { Self::Current => "CURRENT", Self::Archived => "ARCHIVED" }
    }
    pub fn from_str(s: &str) -> Self {
        match s { "ARCHIVED" => Self::Archived, _ => Self::Current }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EventTypeSource {
    Code,
    Api,
    Ui,
}

impl Default for EventTypeSource {
    fn default() -> Self { Self::Ui }
}

impl EventTypeSource {
    pub fn as_str(&self) -> &'static str {
        match self { Self::Code => "CODE", Self::Api => "API", Self::Ui => "UI" }
    }
    pub fn from_str(s: &str) -> Self {
        match s { "CODE" => Self::Code, "API" => Self::Api, _ => Self::Ui }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SpecVersionStatus {
    Finalising,
    Current,
    Deprecated,
}

impl Default for SpecVersionStatus {
    fn default() -> Self { Self::Finalising }
}

impl SpecVersionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Finalising => "FINALISING",
            Self::Current => "CURRENT",
            Self::Deprecated => "DEPRECATED",
        }
    }
    pub fn from_str(s: &str) -> Self {
        match s {
            "CURRENT" => Self::Current,
            "DEPRECATED" => Self::Deprecated,
            _ => Self::Finalising,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SchemaType {
    #[serde(rename = "JSON_SCHEMA")]
    JsonSchema,
    #[serde(rename = "XSD")]
    Xsd,
    #[serde(rename = "PROTO")]
    Proto,
}

impl Default for SchemaType {
    fn default() -> Self { Self::JsonSchema }
}

impl SchemaType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::JsonSchema => "JSON_SCHEMA",
            Self::Xsd => "XSD",
            Self::Proto => "PROTO",
        }
    }
    pub fn from_str(s: &str) -> Self {
        match s {
            "XSD" | "XML_SCHEMA" => Self::Xsd,
            "PROTO" | "PROTOBUF" => Self::Proto,
            _ => Self::JsonSchema,
        }
    }
}

/// Schema version stored in msg_event_type_spec_versions
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpecVersion {
    pub id: String,
    pub event_type_id: String,
    pub version: String,
    pub mime_type: String,
    pub schema_content: Option<serde_json::Value>,
    pub schema_type: SchemaType,
    pub status: SpecVersionStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl SpecVersion {
    pub fn new(event_type_id: impl Into<String>, version: impl Into<String>, schema_content: Option<serde_json::Value>) -> Self {
        let now = Utc::now();
        Self {
            id: crate::TsidGenerator::generate(crate::EntityType::Schema),
            event_type_id: event_type_id.into(),
            version: version.into(),
            mime_type: "application/schema+json".to_string(),
            schema_content,
            schema_type: SchemaType::JsonSchema,
            status: SpecVersionStatus::Finalising,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn is_current(&self) -> bool { self.status == SpecVersionStatus::Current }
    pub fn is_deprecated(&self) -> bool { self.status == SpecVersionStatus::Deprecated }
}

impl From<crate::entities::msg_event_type_spec_versions::Model> for SpecVersion {
    fn from(m: crate::entities::msg_event_type_spec_versions::Model) -> Self {
        Self {
            id: m.id,
            event_type_id: m.event_type_id,
            version: m.version,
            mime_type: m.mime_type,
            schema_content: m.schema_content.map(Into::into),
            schema_type: SchemaType::from_str(&m.schema_type),
            status: SpecVersionStatus::from_str(&m.status),
            created_at: m.created_at.with_timezone(&Utc),
            updated_at: m.updated_at.with_timezone(&Utc),
        }
    }
}

/// EventType domain entity — matches TypeScript EventType interface
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventType {
    pub id: String,
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub spec_versions: Vec<SpecVersion>,
    pub status: EventTypeStatus,
    pub source: EventTypeSource,
    pub client_scoped: bool,
    pub application: String,
    pub subdomain: String,
    pub aggregate: String,
    /// Derived from code (4th segment)
    pub event_name: String,
    /// Optional client scoping
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// Who created this event type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl EventType {
    /// Create from a colon-separated code (application:subdomain:aggregate:event) and name.
    /// Returns Err if the code format is invalid.
    pub fn new(code: impl Into<String>, name: impl Into<String>) -> Result<Self, String> {
        let code = code.into();
        let parts: Vec<&str> = code.split(':').collect();
        if parts.len() != 4 {
            return Err("Event type code must follow format: application:subdomain:aggregate:event".to_string());
        }
        for part in &parts {
            if part.trim().is_empty() {
                return Err("Event type code segments cannot be empty".to_string());
            }
        }
        let application = parts[0].to_string();
        let subdomain = parts[1].to_string();
        let aggregate = parts[2].to_string();
        let event_name = parts[3].to_string();
        let now = Utc::now();
        Ok(Self {
            id: crate::TsidGenerator::generate(crate::EntityType::EventType),
            code,
            name: name.into(),
            description: None,
            spec_versions: vec![],
            status: EventTypeStatus::Current,
            source: EventTypeSource::Ui,
            client_scoped: false,
            application,
            subdomain,
            aggregate,
            event_name,
            client_id: None,
            created_by: None,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self { self.description = Some(desc.into()); self }
    pub fn with_client_id(mut self, id: impl Into<String>) -> Self { self.client_id = Some(id.into()); self }

    pub fn archive(&mut self) {
        self.status = EventTypeStatus::Archived;
        self.updated_at = Utc::now();
    }

    pub fn add_schema_version(&mut self, version: SpecVersion) {
        self.spec_versions.push(version);
        self.updated_at = Utc::now();
    }
}

impl From<crate::entities::msg_event_types::Model> for EventType {
    fn from(m: crate::entities::msg_event_types::Model) -> Self {
        let event_name = m.code.split(':').nth(3).unwrap_or("").to_string();
        Self {
            id: m.id,
            code: m.code,
            name: m.name,
            description: m.description,
            spec_versions: vec![], // loaded separately
            status: EventTypeStatus::from_str(&m.status),
            source: EventTypeSource::from_str(&m.source),
            client_scoped: m.client_scoped,
            application: m.application,
            subdomain: m.subdomain,
            aggregate: m.aggregate,
            event_name,
            client_id: None, // not stored in DB; derived from context
            created_by: None, // not stored in msg_event_types
            created_at: m.created_at.with_timezone(&Utc),
            updated_at: m.updated_at.with_timezone(&Utc),
        }
    }
}
