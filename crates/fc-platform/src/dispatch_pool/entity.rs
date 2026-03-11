//! DispatchPool Entity — matches TypeScript DispatchPool domain

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DispatchPoolStatus {
    Active,
    Suspended,
    Archived,
}

impl Default for DispatchPoolStatus {
    fn default() -> Self { Self::Active }
}

impl DispatchPoolStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::Suspended => "SUSPENDED",
            Self::Archived => "ARCHIVED",
        }
    }
    pub fn from_str(s: &str) -> Self {
        match s {
            "SUSPENDED" => Self::Suspended,
            "ARCHIVED" => Self::Archived,
            _ => Self::Active,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchPool {
    pub id: String,
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub rate_limit: i32,
    pub concurrency: i32,
    pub client_id: Option<String>,
    pub client_identifier: Option<String>,
    pub status: DispatchPoolStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl DispatchPool {
    pub fn new(code: impl Into<String>, name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: crate::TsidGenerator::generate(crate::EntityType::DispatchPool),
            code: code.into(),
            name: name.into(),
            description: None,
            rate_limit: 100,
            concurrency: 10,
            client_id: None,
            client_identifier: None,
            status: DispatchPoolStatus::Active,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self { self.description = Some(desc.into()); self }
    pub fn with_client_id(mut self, id: impl Into<String>) -> Self { self.client_id = Some(id.into()); self }
    pub fn with_rate_limit(mut self, rate: u32) -> Self { self.rate_limit = rate as i32; self }
    pub fn with_concurrency(mut self, conc: u32) -> Self { self.concurrency = conc as i32; self }

    pub fn suspend(&mut self) {
        self.status = DispatchPoolStatus::Suspended;
        self.updated_at = Utc::now();
    }

    pub fn activate(&mut self) {
        self.status = DispatchPoolStatus::Active;
        self.updated_at = Utc::now();
    }

    pub fn archive(&mut self) {
        self.status = DispatchPoolStatus::Archived;
        self.updated_at = Utc::now();
    }
}

impl From<crate::entities::msg_dispatch_pools::Model> for DispatchPool {
    fn from(m: crate::entities::msg_dispatch_pools::Model) -> Self {
        Self {
            id: m.id,
            code: m.code,
            name: m.name,
            description: m.description,
            rate_limit: m.rate_limit,
            concurrency: m.concurrency,
            client_id: m.client_id,
            client_identifier: m.client_identifier,
            status: DispatchPoolStatus::from_str(&m.status),
            created_at: m.created_at.with_timezone(&Utc),
            updated_at: m.updated_at.with_timezone(&Utc),
        }
    }
}
