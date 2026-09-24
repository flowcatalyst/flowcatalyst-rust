//! OpenAPI spec entity. Pure data — no sqlx.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::shared::tsid;
use crate::EntityType;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OpenApiSpecStatus {
    Current,
    Archived,
}

crate::shared::enum_str::str_enum!(OpenApiSpecStatus, "open api spec status", {
    Current => "CURRENT",
    Archived => "ARCHIVED",
});

/// Structured diff between two OpenAPI documents. Persisted as JSONB so the UI
/// can render rich diffs; `OpenApiSpec::change_notes_text` is the pre-rendered
/// human summary for listings.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChangeNotes {
    #[serde(default)]
    pub added_paths: Vec<String>,
    #[serde(default)]
    pub removed_paths: Vec<String>,
    #[serde(default)]
    pub added_schemas: Vec<String>,
    #[serde(default)]
    pub removed_schemas: Vec<String>,
    /// Verbs removed from a path that survived (e.g. "GET /users", "POST /users"
    /// when GET was dropped). Each entry is `"METHOD path"`.
    #[serde(default)]
    pub removed_operations: Vec<String>,
    /// True if anything was removed (path, schema, or verb). Removals are the
    /// canonical breaking-change signal we track in v1.
    #[serde(default)]
    pub has_breaking: bool,
}

impl ChangeNotes {
    pub fn is_empty(&self) -> bool {
        self.added_paths.is_empty()
            && self.removed_paths.is_empty()
            && self.added_schemas.is_empty()
            && self.removed_schemas.is_empty()
            && self.removed_operations.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenApiSpec {
    pub id: String,
    pub application_id: String,
    pub version: String,
    pub status: OpenApiSpecStatus,
    pub spec: serde_json::Value,
    pub spec_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change_notes: Option<ChangeNotes>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change_notes_text: Option<String>,
    pub synced_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub synced_by: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl OpenApiSpec {
    pub fn new(
        application_id: impl Into<String>,
        version: impl Into<String>,
        spec: serde_json::Value,
        spec_hash: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: tsid::generate(EntityType::ApplicationOpenApiSpec),
            application_id: application_id.into(),
            version: version.into(),
            status: OpenApiSpecStatus::Current,
            spec,
            spec_hash: spec_hash.into(),
            change_notes: None,
            change_notes_text: None,
            synced_at: now,
            synced_by: None,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn with_synced_by(mut self, principal_id: Option<String>) -> Self {
        self.synced_by = principal_id;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn new_spec_has_oas_prefix_and_current_status() {
        let s = OpenApiSpec::new("app_X", "1.0.0", serde_json::json!({}), "h");
        assert!(s.id.starts_with("oas_"));
        assert_eq!(s.id.len(), 17);
        assert_eq!(s.status, OpenApiSpecStatus::Current);
        assert_eq!(s.version, "1.0.0");
    }

    #[test]
    fn status_round_trip() {
        for v in [OpenApiSpecStatus::Current, OpenApiSpecStatus::Archived] {
            assert_eq!(OpenApiSpecStatus::from_str(v.as_str()), Ok(v));
        }
    }
}
