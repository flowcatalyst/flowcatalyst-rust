//! EventType Entity — matches TypeScript EventType domain

use chrono::{DateTime, Utc};
use fc_platform_core::shared::id::EventTypeId;
use fc_platform_core::shared::tsid;
use fc_platform_core::shared::tsid::EntityType;
use serde::de;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fmt::Display;
use std::fmt::Formatter;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[derive(Default)]
pub enum EventTypeStatus {
    #[default]
    Current,
    Archived,
}

fc_platform_core::shared::enum_str::str_enum!(EventTypeStatus, "event type status", {
    Current => "CURRENT",
    Archived => "ARCHIVED",
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[derive(Default)]
pub enum EventTypeSource {
    Code,
    Api,
    #[default]
    Ui,
}

fc_platform_core::shared::enum_str::str_enum!(EventTypeSource, "event type source", {
    Code => "CODE",
    Api => "API",
    Ui => "UI",
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[derive(Default)]
pub enum SpecVersionStatus {
    #[default]
    Finalising,
    Current,
    Deprecated,
}

fc_platform_core::shared::enum_str::str_enum!(SpecVersionStatus, "spec version status", {
    Finalising => "FINALISING",
    Current => "CURRENT",
    Deprecated => "DEPRECATED",
});

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

fc_platform_core::shared::enum_str::str_enum!(SchemaType, "schema type", {
    JsonSchema => "JSON_SCHEMA",
    Xsd => "XSD" | "XML_SCHEMA",
    Proto => "PROTO" | "PROTOBUF",
});

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
            id: tsid::generate(EntityType::Schema),
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
    pub id: EventTypeId,
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
    /// Empty or only whitespace.
    #[error("Event type code is required")]
    Required,
    #[error("Event type code must follow format: application:subdomain:aggregate:event")]
    WrongSegmentCount,
    /// The named segment (`application`, `subdomain`, `aggregate` or
    /// `event`) is empty or only whitespace.
    #[error("Event type code part '{0}' cannot be empty")]
    EmptySegment(&'static str),
}

/// The segments of an event type code, in order.
const CODE_SEGMENTS: [&str; 4] = ["application", "subdomain", "aggregate", "event"];

/// An event type code, `application:subdomain:aggregate:event`: exactly
/// four colon-separated segments, none of them blank. The code is kept
/// exactly as given (no trimming, no case change), as it always has been.
///
/// The only way in is [`EventTypeCode::parse`] (or `TryFrom<&str>`, or
/// deserializing, which parse), so a value of this type is always valid.
/// It serializes as the plain string.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EventTypeCode(String);

impl EventTypeCode {
    pub fn parse(code: &str) -> Result<Self, EventTypeCodeError> {
        if code.trim().is_empty() {
            return Err(EventTypeCodeError::Required);
        }
        let parts: Vec<&str> = code.split(':').collect();
        if parts.len() != CODE_SEGMENTS.len() {
            return Err(EventTypeCodeError::WrongSegmentCount);
        }
        for (part, name) in parts.iter().zip(CODE_SEGMENTS) {
            if part.trim().is_empty() {
                return Err(EventTypeCodeError::EmptySegment(name));
            }
        }
        Ok(Self(code.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }

    /// Segment `i` of the four `parse` checked.
    fn segment(&self, i: usize) -> &str {
        self.0.split(':').nth(i).unwrap_or_default()
    }

    pub fn application(&self) -> &str {
        self.segment(0)
    }

    pub fn subdomain(&self) -> &str {
        self.segment(1)
    }

    pub fn aggregate(&self) -> &str {
        self.segment(2)
    }

    pub fn event_name(&self) -> &str {
        self.segment(3)
    }
}

impl TryFrom<&str> for EventTypeCode {
    type Error = EventTypeCodeError;

    fn try_from(code: &str) -> Result<Self, Self::Error> {
        Self::parse(code)
    }
}

impl AsRef<str> for EventTypeCode {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Display for EventTypeCode {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for EventTypeCode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for EventTypeCode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let code = String::deserialize(deserializer)?;
        Self::parse(&code).map_err(de::Error::custom)
    }
}

impl EventType {
    /// Create from a parsed code (application:subdomain:aggregate:event) and name.
    pub fn new(code: EventTypeCode, name: impl Into<String>) -> Self {
        let application = code.application().to_string();
        let subdomain = code.subdomain().to_string();
        let aggregate = code.aggregate().to_string();
        let event_name = code.event_name().to_string();
        let now = Utc::now();
        Self {
            id: EventTypeId::generate(),
            code: code.into_string(),
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
        }
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
    use std::str::FromStr;
    use std::thread;
    use std::time::Duration;

    // ── EventTypeCode parsing ─────────────────────────────────────────────

    fn code(s: &str) -> EventTypeCode {
        EventTypeCode::parse(s).expect("valid code")
    }

    #[test]
    fn new_splits_a_valid_four_part_code() {
        let et = EventType::new(
            code("orders:fulfillment:shipment:shipped"),
            "Shipment Shipped",
        );
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
    fn a_code_is_kept_exactly_as_given() {
        let c = code(" Orders:a:b:c ");
        assert_eq!(c.as_str(), " Orders:a:b:c ");
        assert_eq!(c.application(), " Orders");
        assert_eq!(c.event_name(), "c ");
        assert_eq!(serde_json::to_string(&c).unwrap(), r#"" Orders:a:b:c ""#);
        let back: EventTypeCode = serde_json::from_str(r#"" Orders:a:b:c ""#).unwrap();
        assert_eq!(back, c);
        assert!(serde_json::from_str::<EventTypeCode>(r#""a:b""#).is_err());
    }

    #[test]
    fn parse_rejects_a_blank_code() {
        assert_eq!(EventTypeCode::parse(""), Err(EventTypeCodeError::Required));
        assert_eq!(
            EventTypeCode::parse("  "),
            Err(EventTypeCodeError::Required)
        );
    }

    #[test]
    fn parse_rejects_too_few_segments() {
        assert_eq!(
            EventTypeCode::parse("orders:fulfillment:shipment"),
            Err(EventTypeCodeError::WrongSegmentCount)
        );
        assert!(EventTypeCode::parse("orders:fulfillment").is_err());
        assert!(EventTypeCode::parse("orders").is_err());
    }

    #[test]
    fn parse_rejects_too_many_segments() {
        assert_eq!(
            EventTypeCode::try_from("orders:fulfillment:shipment:shipped:extra"),
            Err(EventTypeCodeError::WrongSegmentCount)
        );
    }

    #[test]
    fn parse_names_the_empty_segment() {
        assert_eq!(
            EventTypeCode::parse("orders::shipment:shipped"),
            Err(EventTypeCodeError::EmptySegment("subdomain"))
        );
        assert_eq!(
            EventTypeCode::parse(":fulfillment:shipment:shipped"),
            Err(EventTypeCodeError::EmptySegment("application"))
        );
        assert_eq!(
            EventTypeCode::parse("orders:fulfillment:shipment:"),
            Err(EventTypeCodeError::EmptySegment("event"))
        );
        assert_eq!(
            EventTypeCode::parse("orders: :shipment:shipped")
                .unwrap_err()
                .to_string(),
            "Event type code part 'subdomain' cannot be empty"
        );
    }

    // ── State transitions ─────────────────────────────────────────────────

    #[test]
    fn archive_flips_status_and_bumps_updated_at() {
        let mut et = EventType::new(code("a:b:c:d"), "Name");
        let before = et.updated_at;
        thread::sleep(Duration::from_millis(2));
        et.archive();
        assert_eq!(et.status, EventTypeStatus::Archived);
        assert!(et.updated_at > before);
    }

    #[test]
    fn add_schema_version_appends_and_bumps_updated_at() {
        let mut et = EventType::new(code("a:b:c:d"), "Name");
        let before = et.updated_at;
        thread::sleep(Duration::from_millis(2));
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

    // ── Enum roundtrips, strict ─────────────────────────────────────

    #[test]
    fn event_type_status_roundtrip_rejects_unknown() {
        assert_eq!(
            EventTypeStatus::from_str("CURRENT"),
            Ok(EventTypeStatus::Current)
        );
        assert_eq!(
            EventTypeStatus::from_str("ARCHIVED"),
            Ok(EventTypeStatus::Archived)
        );
        // Unknown values are rejected (X-06)
        assert!(EventTypeStatus::from_str("UNKNOWN").is_err());
        for s in [EventTypeStatus::Current, EventTypeStatus::Archived] {
            assert_eq!(EventTypeStatus::from_str(s.as_str()), Ok(s));
        }
    }

    #[test]
    fn event_type_source_roundtrip_rejects_unknown() {
        assert_eq!(EventTypeSource::from_str("CODE"), Ok(EventTypeSource::Code));
        assert_eq!(EventTypeSource::from_str("API"), Ok(EventTypeSource::Api));
        assert_eq!(EventTypeSource::from_str("UI"), Ok(EventTypeSource::Ui));
        // Unknown values are rejected (X-06)
        assert!(EventTypeSource::from_str("UNKNOWN").is_err());
    }

    #[test]
    fn spec_version_status_roundtrip_rejects_unknown() {
        assert_eq!(
            SpecVersionStatus::from_str("CURRENT"),
            Ok(SpecVersionStatus::Current)
        );
        assert_eq!(
            SpecVersionStatus::from_str("DEPRECATED"),
            Ok(SpecVersionStatus::Deprecated)
        );
        assert_eq!(
            SpecVersionStatus::from_str("FINALISING"),
            Ok(SpecVersionStatus::Finalising)
        );
        // Unknown values are rejected (X-06)
        assert!(SpecVersionStatus::from_str("UNKNOWN").is_err());
    }

    #[test]
    fn schema_type_accepts_aliases() {
        assert_eq!(
            SchemaType::from_str("JSON_SCHEMA"),
            Ok(SchemaType::JsonSchema)
        );
        assert_eq!(SchemaType::from_str("XSD"), Ok(SchemaType::Xsd));
        assert_eq!(SchemaType::from_str("XML_SCHEMA"), Ok(SchemaType::Xsd));
        assert_eq!(SchemaType::from_str("PROTO"), Ok(SchemaType::Proto));
        assert_eq!(SchemaType::from_str("PROTOBUF"), Ok(SchemaType::Proto));
        // Unknown values are rejected (X-06)
        assert!(SchemaType::from_str("UNKNOWN").is_err());
    }
}
