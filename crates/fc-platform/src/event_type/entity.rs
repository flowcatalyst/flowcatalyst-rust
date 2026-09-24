//! EventType Entity — matches TypeScript EventType domain

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[derive(Default)]
pub enum EventTypeStatus {
    #[default]
    Current,
    Archived,
}

impl EventTypeStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Current => "CURRENT",
            Self::Archived => "ARCHIVED",
        }
    }
    pub fn from_str(s: &str) -> Self {
        match s {
            "ARCHIVED" => Self::Archived,
            _ => Self::Current,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[derive(Default)]
pub enum EventTypeSource {
    Code,
    Api,
    #[default]
    Ui,
}

impl EventTypeSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Code => "CODE",
            Self::Api => "API",
            Self::Ui => "UI",
        }
    }
    pub fn from_str(s: &str) -> Self {
        match s {
            "CODE" => Self::Code,
            "API" => Self::Api,
            _ => Self::Ui,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[derive(Default)]
pub enum SpecVersionStatus {
    #[default]
    Finalising,
    Current,
    Deprecated,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SchemaType {
    #[serde(rename = "JSON_SCHEMA")]
    #[default]
    JsonSchema,
    #[serde(rename = "XSD")]
    Xsd,
    #[serde(rename = "PROTO")]
    Proto,
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
    pub fn new(
        event_type_id: impl Into<String>,
        version: impl Into<String>,
        schema_content: Option<serde_json::Value>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: crate::shared::tsid::generate(crate::EntityType::Schema),
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

    pub fn is_current(&self) -> bool {
        self.status == SpecVersionStatus::Current
    }
    pub fn is_deprecated(&self) -> bool {
        self.status == SpecVersionStatus::Deprecated
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

/// Why an event type code was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EventTypeCodeError {
    #[error("Event type code must follow format: application:subdomain:aggregate:event")]
    WrongSegmentCount,
    #[error("Event type code segments cannot be empty")]
    EmptySegment,
}

impl EventType {
    /// Create from a colon-separated code (application:subdomain:aggregate:event) and name.
    /// Returns Err if the code format is invalid.
    pub fn new(
        code: impl Into<String>,
        name: impl Into<String>,
    ) -> Result<Self, EventTypeCodeError> {
        let code = code.into();
        let parts: Vec<&str> = code.split(':').collect();
        if parts.len() != 4 {
            return Err(EventTypeCodeError::WrongSegmentCount);
        }
        for part in &parts {
            if part.trim().is_empty() {
                return Err(EventTypeCodeError::EmptySegment);
            }
        }
        let application = parts[0].to_string();
        let subdomain = parts[1].to_string();
        let aggregate = parts[2].to_string();
        let event_name = parts[3].to_string();
        let now = Utc::now();
        Ok(Self {
            id: crate::shared::tsid::generate(crate::EntityType::EventType),
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

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }
    pub fn with_client_id(mut self, id: impl Into<String>) -> Self {
        self.client_id = Some(id.into());
        self
    }

    pub fn archive(&mut self) {
        self.status = EventTypeStatus::Archived;
        self.updated_at = Utc::now();
    }

    pub fn add_schema_version(&mut self, version: SpecVersion) {
        self.spec_versions.push(version);
        self.updated_at = Utc::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── EventType::new validation ────────────────────────────────────────

    #[test]
    fn new_accepts_valid_four_part_code() {
        let et = EventType::new("orders:fulfillment:shipment:shipped", "Shipment Shipped")
            .expect("valid code");
        assert_eq!(et.code, "orders:fulfillment:shipment:shipped");
        assert_eq!(et.application, "orders");
        assert_eq!(et.subdomain, "fulfillment");
        assert_eq!(et.aggregate, "shipment");
        assert_eq!(et.event_name, "shipped");
        assert_eq!(et.status, EventTypeStatus::Current);
        assert!(!et.client_scoped);
        assert!(et.spec_versions.is_empty());
    }

    #[test]
    fn new_rejects_too_few_segments() {
        assert_eq!(
            EventType::new("orders:fulfillment:shipment", "x").unwrap_err(),
            EventTypeCodeError::WrongSegmentCount
        );
        assert!(EventType::new("orders:fulfillment", "x").is_err());
        assert!(EventType::new("orders", "x").is_err());
        assert!(EventType::new("", "x").is_err());
    }

    #[test]
    fn new_rejects_too_many_segments() {
        assert!(EventType::new("orders:fulfillment:shipment:shipped:extra", "x").is_err());
    }

    #[test]
    fn new_rejects_empty_segment() {
        assert_eq!(
            EventType::new("orders::shipment:shipped", "x").unwrap_err(),
            EventTypeCodeError::EmptySegment
        );
        assert!(EventType::new(":fulfillment:shipment:shipped", "x").is_err());
        assert!(EventType::new("orders:fulfillment:shipment:", "x").is_err());
    }

    #[test]
    fn new_rejects_whitespace_only_segment() {
        assert!(EventType::new("orders: :shipment:shipped", "x").is_err());
    }

    // ── State transitions ─────────────────────────────────────────────────

    #[test]
    fn archive_flips_status_and_bumps_updated_at() {
        let mut et = EventType::new("a:b:c:d", "Name").unwrap();
        let before = et.updated_at;
        std::thread::sleep(std::time::Duration::from_millis(2));
        et.archive();
        assert_eq!(et.status, EventTypeStatus::Archived);
        assert!(et.updated_at > before);
    }

    #[test]
    fn add_schema_version_appends_and_bumps_updated_at() {
        let mut et = EventType::new("a:b:c:d", "Name").unwrap();
        let before = et.updated_at;
        std::thread::sleep(std::time::Duration::from_millis(2));
        let sv = SpecVersion::new(&et.id, "1.0.0", None);
        et.add_schema_version(sv);
        assert_eq!(et.spec_versions.len(), 1);
        assert_eq!(et.spec_versions[0].version, "1.0.0");
        assert!(et.updated_at > before);
    }

    // ── SpecVersion status helpers ────────────────────────────────────────

    #[test]
    fn spec_version_status_helpers() {
        let mut sv = SpecVersion::new("et_1", "1.0", None);
        assert!(!sv.is_current());
        assert!(!sv.is_deprecated());
        sv.status = SpecVersionStatus::Current;
        assert!(sv.is_current());
        sv.status = SpecVersionStatus::Deprecated;
        assert!(sv.is_deprecated());
    }

    // ── Enum roundtrips with fallback ─────────────────────────────────────

    #[test]
    fn event_type_status_roundtrip_with_fallback() {
        assert_eq!(
            EventTypeStatus::from_str("CURRENT"),
            EventTypeStatus::Current
        );
        assert_eq!(
            EventTypeStatus::from_str("ARCHIVED"),
            EventTypeStatus::Archived
        );
        // Unknown falls back to Current
        assert_eq!(
            EventTypeStatus::from_str("UNKNOWN"),
            EventTypeStatus::Current
        );
        for s in [EventTypeStatus::Current, EventTypeStatus::Archived] {
            assert_eq!(EventTypeStatus::from_str(s.as_str()), s);
        }
    }

    #[test]
    fn event_type_source_roundtrip_with_fallback() {
        assert_eq!(EventTypeSource::from_str("CODE"), EventTypeSource::Code);
        assert_eq!(EventTypeSource::from_str("API"), EventTypeSource::Api);
        assert_eq!(EventTypeSource::from_str("UI"), EventTypeSource::Ui);
        // Unknown falls back to Ui
        assert_eq!(EventTypeSource::from_str("UNKNOWN"), EventTypeSource::Ui);
    }

    #[test]
    fn spec_version_status_roundtrip_with_fallback() {
        assert_eq!(
            SpecVersionStatus::from_str("CURRENT"),
            SpecVersionStatus::Current
        );
        assert_eq!(
            SpecVersionStatus::from_str("DEPRECATED"),
            SpecVersionStatus::Deprecated
        );
        assert_eq!(
            SpecVersionStatus::from_str("FINALISING"),
            SpecVersionStatus::Finalising
        );
        // Unknown falls back to Finalising
        assert_eq!(
            SpecVersionStatus::from_str("UNKNOWN"),
            SpecVersionStatus::Finalising
        );
    }

    #[test]
    fn schema_type_accepts_aliases() {
        assert_eq!(SchemaType::from_str("JSON_SCHEMA"), SchemaType::JsonSchema);
        assert_eq!(SchemaType::from_str("XSD"), SchemaType::Xsd);
        assert_eq!(SchemaType::from_str("XML_SCHEMA"), SchemaType::Xsd);
        assert_eq!(SchemaType::from_str("PROTO"), SchemaType::Proto);
        assert_eq!(SchemaType::from_str("PROTOBUF"), SchemaType::Proto);
        // Unknown falls back to JsonSchema
        assert_eq!(SchemaType::from_str("UNKNOWN"), SchemaType::JsonSchema);
    }
}
