//! Process Entity — free-form workflow documentation (typically Mermaid diagrams)

use chrono::{DateTime, Utc};
use fc_platform_core::shared::tsid;
use fc_platform_core::shared::tsid::EntityType;
use serde::de;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fmt::Display;
use std::fmt::Formatter;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProcessStatus {
    #[default]
    Current,
    Archived,
}

fc_platform_core::shared::enum_str::str_enum!(ProcessStatus, "process status", {
    Current => "CURRENT",
    Archived => "ARCHIVED",
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProcessSource {
    Code,
    Api,
    #[default]
    Ui,
}

fc_platform_core::shared::enum_str::str_enum!(ProcessSource, "process source", {
    Code => "CODE",
    Api => "API",
    Ui => "UI",
});

/// Process domain entity. The `body` field holds free-form diagram source
/// (typically Mermaid); the platform stores it verbatim and renders it
/// client-side.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Process {
    pub id: String,
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub status: ProcessStatus,
    pub source: ProcessSource,
    pub application: String,
    pub subdomain: String,
    pub process_name: String,
    pub body: String,
    pub diagram_type: String,
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Why a process code was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProcessCodeError {
    /// Empty or only whitespace.
    #[error("Process code is required")]
    Required,
    #[error("Process code must follow format: application:subdomain:process-name")]
    WrongSegmentCount,
    /// The named segment (`application`, `subdomain` or `process-name`) is
    /// empty or only whitespace.
    #[error("Process code part '{0}' cannot be empty")]
    EmptySegment(&'static str),
}

/// The segments of a process code, in order.
const CODE_SEGMENTS: [&str; 3] = ["application", "subdomain", "process-name"];

/// A process code, `application:subdomain:process-name`: exactly three
/// colon-separated segments, none of them blank, kept exactly as given.
///
/// The only way in is [`ProcessCode::parse`] (or `TryFrom<&str>`, or
/// deserializing, which parse). It serializes as the plain string.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProcessCode(String);

impl ProcessCode {
    pub fn parse(code: &str) -> Result<Self, ProcessCodeError> {
        if code.trim().is_empty() {
            return Err(ProcessCodeError::Required);
        }
        let parts: Vec<&str> = code.split(':').collect();
        if parts.len() != CODE_SEGMENTS.len() {
            return Err(ProcessCodeError::WrongSegmentCount);
        }
        for (part, name) in parts.iter().zip(CODE_SEGMENTS) {
            if part.trim().is_empty() {
                return Err(ProcessCodeError::EmptySegment(name));
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

    /// Segment `i` of the three `parse` checked.
    fn segment(&self, i: usize) -> &str {
        self.0.split(':').nth(i).unwrap_or_default()
    }

    pub fn application(&self) -> &str {
        self.segment(0)
    }

    pub fn subdomain(&self) -> &str {
        self.segment(1)
    }

    pub fn process_name(&self) -> &str {
        self.segment(2)
    }
}

impl TryFrom<&str> for ProcessCode {
    type Error = ProcessCodeError;

    fn try_from(code: &str) -> Result<Self, Self::Error> {
        Self::parse(code)
    }
}

impl AsRef<str> for ProcessCode {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Display for ProcessCode {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for ProcessCode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ProcessCode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let code = String::deserialize(deserializer)?;
        Self::parse(&code).map_err(de::Error::custom)
    }
}

impl Process {
    /// Create from a parsed code (application:subdomain:process-name) and name.
    pub fn new(code: ProcessCode, name: impl Into<String>) -> Self {
        let application = code.application().to_string();
        let subdomain = code.subdomain().to_string();
        let process_name = code.process_name().to_string();
        let now = Utc::now();
        Self {
            id: tsid::generate(EntityType::Process),
            code: code.into_string(),
            name: name.into(),
            description: None,
            status: ProcessStatus::Current,
            source: ProcessSource::Ui,
            application,
            subdomain,
            process_name,
            body: String::new(),
            diagram_type: "mermaid".to_string(),
            tags: Vec::new(),
            created_by: None,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn archive(&mut self) {
        self.status = ProcessStatus::Archived;
        self.updated_at = Utc::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    use std::thread;
    use std::time::Duration;

    fn code(s: &str) -> ProcessCode {
        ProcessCode::parse(s).expect("valid code")
    }

    #[test]
    fn new_splits_a_valid_three_part_code() {
        let p = Process::new(code("orders:fulfillment:shipment-flow"), "Shipment Flow");
        assert_eq!(p.code, "orders:fulfillment:shipment-flow");
        assert_eq!(p.application, "orders");
        assert_eq!(p.subdomain, "fulfillment");
        assert_eq!(p.process_name, "shipment-flow");
        assert_eq!(p.status, ProcessStatus::Current);
        assert_eq!(p.diagram_type, "mermaid");
    }

    #[test]
    fn parse_rejects_a_blank_code() {
        assert_eq!(ProcessCode::parse(" "), Err(ProcessCodeError::Required));
    }

    #[test]
    fn parse_rejects_wrong_segment_count() {
        assert_eq!(
            ProcessCode::parse("a:b"),
            Err(ProcessCodeError::WrongSegmentCount)
        );
        assert!(ProcessCode::try_from("a:b:c:d").is_err());
    }

    #[test]
    fn parse_names_the_empty_segment() {
        assert_eq!(
            ProcessCode::parse("a::c"),
            Err(ProcessCodeError::EmptySegment("subdomain"))
        );
        assert_eq!(
            ProcessCode::parse(":b:c"),
            Err(ProcessCodeError::EmptySegment("application"))
        );
        assert_eq!(
            ProcessCode::parse("a:b:").unwrap_err().to_string(),
            "Process code part 'process-name' cannot be empty"
        );
        assert!(ProcessCode::parse("a: :c").is_err());
    }

    #[test]
    fn a_code_serializes_as_given() {
        let c = code(" a:b:c ");
        assert_eq!(serde_json::to_string(&c).unwrap(), r#"" a:b:c ""#);
        assert!(serde_json::from_str::<ProcessCode>(r#""a:b""#).is_err());
    }

    #[test]
    fn archive_flips_status() {
        let mut p = Process::new(code("a:b:c"), "x");
        let before = p.updated_at;
        thread::sleep(Duration::from_millis(2));
        p.archive();
        assert_eq!(p.status, ProcessStatus::Archived);
        assert!(p.updated_at > before);
    }

    #[test]
    fn status_roundtrip_rejects_unknown() {
        assert_eq!(
            ProcessStatus::from_str("CURRENT"),
            Ok(ProcessStatus::Current)
        );
        assert_eq!(
            ProcessStatus::from_str("ARCHIVED"),
            Ok(ProcessStatus::Archived)
        );
        assert!(ProcessStatus::from_str("UNKNOWN").is_err());
    }

    #[test]
    fn source_roundtrip_rejects_unknown() {
        assert_eq!(ProcessSource::from_str("CODE"), Ok(ProcessSource::Code));
        assert_eq!(ProcessSource::from_str("API"), Ok(ProcessSource::Api));
        assert_eq!(ProcessSource::from_str("UI"), Ok(ProcessSource::Ui));
        assert!(ProcessSource::from_str("XYZ").is_err());
    }
}
