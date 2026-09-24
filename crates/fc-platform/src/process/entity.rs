//! Process Entity — free-form workflow documentation (typically Mermaid diagrams)

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProcessStatus {
    #[default]
    Current,
    Archived,
}

impl ProcessStatus {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProcessSource {
    Code,
    Api,
    #[default]
    Ui,
}

impl ProcessSource {
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
    #[error("Process code must follow format: application:subdomain:process-name")]
    WrongSegmentCount,
    #[error("Process code segments cannot be empty")]
    EmptySegment,
}

impl Process {
    /// Create from a colon-separated code (application:subdomain:process-name) and name.
    pub fn new(code: impl Into<String>, name: impl Into<String>) -> Result<Self, ProcessCodeError> {
        let code = code.into();
        let parts: Vec<&str> = code.split(':').collect();
        if parts.len() != 3 {
            return Err(ProcessCodeError::WrongSegmentCount);
        }
        for part in &parts {
            if part.trim().is_empty() {
                return Err(ProcessCodeError::EmptySegment);
            }
        }
        let application = parts[0].to_string();
        let subdomain = parts[1].to_string();
        let process_name = parts[2].to_string();
        let now = Utc::now();
        Ok(Self {
            id: crate::shared::tsid::generate(crate::EntityType::Process),
            code,
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
        })
    }

    pub fn archive(&mut self) {
        self.status = ProcessStatus::Archived;
        self.updated_at = Utc::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_accepts_valid_three_part_code() {
        let p = Process::new("orders:fulfillment:shipment-flow", "Shipment Flow").unwrap();
        assert_eq!(p.application, "orders");
        assert_eq!(p.subdomain, "fulfillment");
        assert_eq!(p.process_name, "shipment-flow");
        assert_eq!(p.status, ProcessStatus::Current);
        assert_eq!(p.diagram_type, "mermaid");
    }

    #[test]
    fn new_rejects_wrong_segment_count() {
        assert_eq!(
            Process::new("a:b", "x").unwrap_err(),
            ProcessCodeError::WrongSegmentCount
        );
        assert!(Process::new("a:b:c:d", "x").is_err());
    }

    #[test]
    fn new_rejects_empty_segment() {
        assert_eq!(
            Process::new("a::c", "x").unwrap_err(),
            ProcessCodeError::EmptySegment
        );
        assert!(Process::new(":b:c", "x").is_err());
        assert!(Process::new("a:b:", "x").is_err());
        assert!(Process::new("a: :c", "x").is_err());
    }

    #[test]
    fn archive_flips_status() {
        let mut p = Process::new("a:b:c", "x").unwrap();
        let before = p.updated_at;
        std::thread::sleep(std::time::Duration::from_millis(2));
        p.archive();
        assert_eq!(p.status, ProcessStatus::Archived);
        assert!(p.updated_at > before);
    }

    #[test]
    fn status_roundtrip_with_fallback() {
        assert_eq!(ProcessStatus::from_str("CURRENT"), ProcessStatus::Current);
        assert_eq!(ProcessStatus::from_str("ARCHIVED"), ProcessStatus::Archived);
        assert_eq!(ProcessStatus::from_str("UNKNOWN"), ProcessStatus::Current);
    }

    #[test]
    fn source_roundtrip_with_fallback() {
        assert_eq!(ProcessSource::from_str("CODE"), ProcessSource::Code);
        assert_eq!(ProcessSource::from_str("API"), ProcessSource::Api);
        assert_eq!(ProcessSource::from_str("UI"), ProcessSource::Ui);
        assert_eq!(ProcessSource::from_str("XYZ"), ProcessSource::Ui);
    }
}
