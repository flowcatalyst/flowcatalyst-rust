//! Dispatch Pool Entity
//!
//! Rate limiting and concurrency control for dispatch jobs.

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use bson::serde_helpers::chrono_datetime_as_bson_datetime;

/// Dispatch pool status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DispatchPoolStatus {
    Active,
    Archived,
}

impl Default for DispatchPoolStatus {
    fn default() -> Self {
        Self::Active
    }
}

/// Dispatch pool for rate limiting
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchPool {
    /// TSID as Crockford Base32 string
    #[serde(rename = "_id")]
    pub id: String,

    /// Unique code (unique per client_id)
    pub code: String,

    /// Human-readable name
    pub name: String,

    /// Description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Rate limit: maximum messages per minute
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<u32>,

    /// Maximum concurrent dispatches
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concurrency: Option<u32>,

    /// Multi-tenant: Client ID (null = anchor-level/shared)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,

    /// Status
    #[serde(default)]
    pub status: DispatchPoolStatus,

    /// Audit fields
    #[serde(with = "chrono_datetime_as_bson_datetime")]
    pub created_at: DateTime<Utc>,
    #[serde(with = "chrono_datetime_as_bson_datetime")]
    pub updated_at: DateTime<Utc>,
}

impl DispatchPool {
    pub fn new(code: impl Into<String>, name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: crate::TsidGenerator::generate(),
            code: code.into(),
            name: name.into(),
            description: None,
            rate_limit: None,
            concurrency: None,
            client_id: None,
            status: DispatchPoolStatus::Active,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn with_rate_limit(mut self, rate_limit: u32) -> Self {
        self.rate_limit = Some(rate_limit);
        self
    }

    pub fn with_concurrency(mut self, concurrency: u32) -> Self {
        self.concurrency = Some(concurrency);
        self
    }

    pub fn with_client_id(mut self, client_id: impl Into<String>) -> Self {
        self.client_id = Some(client_id.into());
        self
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn archive(&mut self) {
        self.status = DispatchPoolStatus::Archived;
        self.updated_at = Utc::now();
    }

    pub fn is_active(&self) -> bool {
        self.status == DispatchPoolStatus::Active
    }
}
