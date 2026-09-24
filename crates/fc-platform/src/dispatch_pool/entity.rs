//! DispatchPool Entity — matches TypeScript DispatchPool domain

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[derive(Default)]
pub enum DispatchPoolStatus {
    #[default]
    Active,
    Suspended,
    Archived,
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
    /// `None` means concurrency-only (no rate limit applied by the message router).
    pub rate_limit: Option<i32>,
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
            id: crate::shared::tsid::generate(crate::EntityType::DispatchPool),
            code: code.into(),
            name: name.into(),
            description: None,
            rate_limit: None,
            concurrency: 10,
            client_id: None,
            client_identifier: None,
            status: DispatchPoolStatus::Active,
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
    pub fn with_rate_limit(mut self, rate: Option<u32>) -> Self {
        self.rate_limit = rate.map(|r| r as i32);
        self
    }
    pub fn with_concurrency(mut self, conc: u32) -> Self {
        self.concurrency = conc as i32;
        self
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_dispatch_pool() {
        let pool = DispatchPool::new("default-pool", "Default Pool");

        assert!(!pool.id.is_empty());
        assert!(
            pool.id.starts_with("dpl_"),
            "ID should have dpl_ prefix, got: {}",
            pool.id
        );
        assert_eq!(
            pool.id.len(),
            17,
            "Typed ID should be 17 chars, got: {}",
            pool.id.len()
        );
        assert_eq!(pool.code, "default-pool");
        assert_eq!(pool.name, "Default Pool");
        assert!(pool.description.is_none());
        assert_eq!(pool.rate_limit, None);
        assert_eq!(pool.concurrency, 10);
        assert!(pool.client_id.is_none());
        assert!(pool.client_identifier.is_none());
        assert_eq!(pool.status, DispatchPoolStatus::Active);
        assert_eq!(pool.created_at, pool.updated_at);
    }

    #[test]
    fn test_dispatch_pool_unique_ids() {
        let p1 = DispatchPool::new("a", "A");
        let p2 = DispatchPool::new("b", "B");
        assert_ne!(p1.id, p2.id);
    }

    #[test]
    fn test_dispatch_pool_status_as_str() {
        assert_eq!(DispatchPoolStatus::Active.as_str(), "ACTIVE");
        assert_eq!(DispatchPoolStatus::Suspended.as_str(), "SUSPENDED");
        assert_eq!(DispatchPoolStatus::Archived.as_str(), "ARCHIVED");
    }

    #[test]
    fn test_dispatch_pool_status_from_str() {
        assert_eq!(
            DispatchPoolStatus::from_str("ACTIVE"),
            DispatchPoolStatus::Active
        );
        assert_eq!(
            DispatchPoolStatus::from_str("SUSPENDED"),
            DispatchPoolStatus::Suspended
        );
        assert_eq!(
            DispatchPoolStatus::from_str("ARCHIVED"),
            DispatchPoolStatus::Archived
        );
        assert_eq!(
            DispatchPoolStatus::from_str("unknown"),
            DispatchPoolStatus::Active
        );
    }

    #[test]
    fn test_dispatch_pool_status_default() {
        assert_eq!(DispatchPoolStatus::default(), DispatchPoolStatus::Active);
    }

    #[test]
    fn test_dispatch_pool_status_roundtrip() {
        for s in [
            DispatchPoolStatus::Active,
            DispatchPoolStatus::Suspended,
            DispatchPoolStatus::Archived,
        ] {
            assert_eq!(
                DispatchPoolStatus::from_str(s.as_str()),
                s,
                "Roundtrip failed for {:?}",
                s
            );
        }
    }

    #[test]
    fn test_dispatch_pool_builder_methods() {
        let pool = DispatchPool::new("pool", "Pool")
            .with_description("A test pool")
            .with_client_id("client-1")
            .with_rate_limit(Some(200))
            .with_concurrency(20);

        assert_eq!(pool.description, Some("A test pool".to_string()));
        assert_eq!(pool.client_id, Some("client-1".to_string()));
        assert_eq!(pool.rate_limit, Some(200));
        assert_eq!(pool.concurrency, 20);
    }

    #[test]
    fn test_dispatch_pool_suspend() {
        let mut pool = DispatchPool::new("pool", "Pool");
        assert_eq!(pool.status, DispatchPoolStatus::Active);

        pool.suspend();
        assert_eq!(pool.status, DispatchPoolStatus::Suspended);
    }

    #[test]
    fn test_dispatch_pool_activate() {
        let mut pool = DispatchPool::new("pool", "Pool");
        pool.suspend();

        pool.activate();
        assert_eq!(pool.status, DispatchPoolStatus::Active);
    }

    #[test]
    fn test_dispatch_pool_archive() {
        let mut pool = DispatchPool::new("pool", "Pool");
        pool.archive();
        assert_eq!(pool.status, DispatchPoolStatus::Archived);
    }

    #[test]
    fn test_dispatch_pool_status_transitions() {
        let mut pool = DispatchPool::new("pool", "Pool");
        assert_eq!(pool.status, DispatchPoolStatus::Active);

        pool.suspend();
        assert_eq!(pool.status, DispatchPoolStatus::Suspended);

        pool.activate();
        assert_eq!(pool.status, DispatchPoolStatus::Active);

        pool.archive();
        assert_eq!(pool.status, DispatchPoolStatus::Archived);
    }
}
