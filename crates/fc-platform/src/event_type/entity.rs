//! Event Type Entity
//!
//! Defines event types with schema versioning.
//! Code format: {application}:{subdomain}:{aggregate}:{event}

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use bson::serde_helpers::chrono_datetime_as_bson_datetime;

/// Event type status (matches Java EventTypeStatus)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventTypeStatus {
    /// Event type is active and can have new events created
    #[serde(rename = "CURRENT")]
    Current,
    /// Event type is archived - no new events can be created
    #[serde(rename = "ARCHIVE")]
    Archive,
}

impl Default for EventTypeStatus {
    fn default() -> Self {
        Self::Current
    }
}

/// Schema version status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SpecVersionStatus {
    /// Schema is being finalized (can still be modified)
    Finalising,
    /// Schema is finalized and immutable
    Finalized,
    /// Schema is deprecated (should migrate away)
    Deprecated,
}

impl Default for SpecVersionStatus {
    fn default() -> Self {
        Self::Finalising
    }
}

/// Schema version for an event type
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpecVersion {
    /// Version number (1, 2, 3, ...)
    pub version: u32,

    /// JSON Schema definition
    pub schema: serde_json::Value,

    /// Version status
    #[serde(default)]
    pub status: SpecVersionStatus,

    /// Description of changes in this version
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// When this version was created
    #[serde(with = "chrono_datetime_as_bson_datetime")]
    pub created_at: DateTime<Utc>,

    /// When this version was finalized
    #[serde(skip_serializing_if = "Option::is_none", default, with = "bson::serde_helpers::chrono_datetime_as_bson_datetime_optional")]
    pub finalized_at: Option<DateTime<Utc>>,

    /// When this version was deprecated
    #[serde(skip_serializing_if = "Option::is_none", default, with = "bson::serde_helpers::chrono_datetime_as_bson_datetime_optional")]
    pub deprecated_at: Option<DateTime<Utc>>,
}

impl SpecVersion {
    pub fn new(version: u32, schema: serde_json::Value) -> Self {
        Self {
            version,
            schema,
            status: SpecVersionStatus::Finalising,
            description: None,
            created_at: Utc::now(),
            finalized_at: None,
            deprecated_at: None,
        }
    }

    pub fn is_finalized(&self) -> bool {
        self.status == SpecVersionStatus::Finalized
    }

    pub fn is_deprecated(&self) -> bool {
        self.status == SpecVersionStatus::Deprecated
    }
}

/// Event type definition
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventType {
    /// TSID as Crockford Base32 string
    #[serde(rename = "_id")]
    pub id: String,

    /// Globally unique code
    /// Format: {application}:{subdomain}:{aggregate}:{event}
    /// Example: "orders:fulfillment:shipment:shipped"
    pub code: String,

    /// Human-readable name
    pub name: String,

    /// Description of the event type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Application code (extracted from code)
    pub application: String,

    /// Subdomain (extracted from code)
    pub subdomain: String,

    /// Aggregate (extracted from code)
    pub aggregate: String,

    /// Event name (extracted from code)
    pub event_name: String,

    /// Schema versions (ordered by version number)
    #[serde(default)]
    pub spec_versions: Vec<SpecVersion>,

    /// Current status
    #[serde(default)]
    pub status: EventTypeStatus,

    /// Multi-tenant: Client ID (null = anchor-level/shared)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,

    /// Audit fields
    #[serde(with = "chrono_datetime_as_bson_datetime")]
    pub created_at: DateTime<Utc>,
    #[serde(with = "chrono_datetime_as_bson_datetime")]
    pub updated_at: DateTime<Utc>,

    /// Who created this
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
}

impl EventType {
    /// Create a new event type from a code
    pub fn new(code: impl Into<String>, name: impl Into<String>) -> Result<Self, String> {
        let code = code.into();
        let parts: Vec<&str> = code.split(':').collect();

        if parts.len() != 4 {
            return Err(format!(
                "Invalid event type code format. Expected 'application:subdomain:aggregate:event', got '{}'",
                code
            ));
        }

        let now = Utc::now();
        Ok(Self {
            id: crate::TsidGenerator::generate(),
            code: code.clone(),
            name: name.into(),
            description: None,
            application: parts[0].to_string(),
            subdomain: parts[1].to_string(),
            aggregate: parts[2].to_string(),
            event_name: parts[3].to_string(),
            spec_versions: vec![],
            status: EventTypeStatus::Current,
            client_id: None,
            created_at: now,
            updated_at: now,
            created_by: None,
        })
    }

    /// Add a new schema version
    pub fn add_schema_version(&mut self, schema: serde_json::Value) -> &SpecVersion {
        let next_version = self.spec_versions.iter().map(|v| v.version).max().unwrap_or(0) + 1;
        let spec = SpecVersion::new(next_version, schema);
        self.spec_versions.push(spec);
        self.updated_at = Utc::now();
        self.spec_versions.last().unwrap()
    }

    /// Get the latest finalized schema version
    pub fn latest_finalized_version(&self) -> Option<&SpecVersion> {
        self.spec_versions
            .iter()
            .filter(|v| v.is_finalized())
            .max_by_key(|v| v.version)
    }

    /// Get a specific schema version
    pub fn get_version(&self, version: u32) -> Option<&SpecVersion> {
        self.spec_versions.iter().find(|v| v.version == version)
    }

    /// Get a mutable reference to a specific schema version
    pub fn get_version_mut(&mut self, version: u32) -> Option<&mut SpecVersion> {
        self.spec_versions.iter_mut().find(|v| v.version == version)
    }

    /// Finalize a schema version
    pub fn finalize_version(&mut self, version: u32) -> Result<(), String> {
        let spec = self.get_version_mut(version)
            .ok_or_else(|| format!("Version {} not found", version))?;

        if spec.is_finalized() {
            return Err(format!("Version {} is already finalized", version));
        }

        spec.status = SpecVersionStatus::Finalized;
        spec.finalized_at = Some(Utc::now());
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Deprecate a schema version
    pub fn deprecate_version(&mut self, version: u32) -> Result<(), String> {
        let spec = self.get_version_mut(version)
            .ok_or_else(|| format!("Version {} not found", version))?;

        if spec.is_deprecated() {
            return Err(format!("Version {} is already deprecated", version));
        }

        spec.status = SpecVersionStatus::Deprecated;
        spec.deprecated_at = Some(Utc::now());
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Archive this event type
    pub fn archive(&mut self) {
        self.status = EventTypeStatus::Archive;
        self.updated_at = Utc::now();
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_client_id(mut self, client_id: impl Into<String>) -> Self {
        self.client_id = Some(client_id.into());
        self
    }
}
