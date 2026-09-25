//! Result types returned by `DefinitionSynchronizer::sync`.
//!
//! Per-category outcomes are stored as `Option<CategorySyncResult>` so callers
//! can distinguish "skipped" (`None`) from "ran with zero rows changed"
//! (`Some(..)` with all counts at 0). Failures don't propagate as `Err` —
//! they're captured on the per-category result's `error` field so the
//! caller can see what partially succeeded.

/// Per-category outcome counts plus an optional error message.
///
/// `synced_codes` carries the platform's `syncedCodes` field for the
/// categories that return it (all except processes, which returns just
/// the counts). The orchestrator normalises a missing list to an empty
/// `Vec`.
#[derive(Debug, Clone, Default)]
pub struct CategorySyncResult {
    pub created: u32,
    pub updated: u32,
    pub deleted: u32,
    pub synced_codes: Vec<String>,
    /// Principals only: the existing users (emails) whose `password_hash`
    /// the platform did not apply.
    pub password_hash_ignored: Vec<String>,
    pub error: Option<String>,
}

impl CategorySyncResult {
    /// Construct a result from a transport failure. Counts are zero and
    /// `error` carries the message.
    pub fn from_error(error: impl Into<String>) -> Self {
        Self {
            error: Some(error.into()),
            ..Self::default()
        }
    }

    pub fn is_error(&self) -> bool {
        self.error.is_some()
    }

    pub fn touched(&self) -> u32 {
        self.created + self.updated + self.deleted
    }
}

/// Aggregate result of syncing a full `DefinitionSet`. Each category is
/// either `Some(..)` (the orchestrator hit that endpoint) or `None`
/// (skipped because the category was empty or disabled via `SyncOptions`).
#[derive(Debug, Clone, Default)]
pub struct SyncResult {
    pub application_code: String,
    pub roles: Option<CategorySyncResult>,
    pub event_types: Option<CategorySyncResult>,
    pub subscriptions: Option<CategorySyncResult>,
    pub dispatch_pools: Option<CategorySyncResult>,
    pub principals: Option<CategorySyncResult>,
    pub processes: Option<CategorySyncResult>,
    pub scheduled_jobs: Option<CategorySyncResult>,
    /// OpenAPI sync is a single-spec upload, not a per-row list — the
    /// `synced_codes` field always contains exactly the publish-version
    /// string on success.
    pub openapi: Option<CategorySyncResult>,
}

impl SyncResult {
    /// Stable category iteration order matching the orchestrator's call
    /// sequence — roles → event_types → subscriptions → dispatch_pools →
    /// principals → processes → scheduled_jobs → openapi.
    fn categories(&self) -> [(&'static str, Option<&CategorySyncResult>); 8] {
        [
            ("roles", self.roles.as_ref()),
            ("eventTypes", self.event_types.as_ref()),
            ("subscriptions", self.subscriptions.as_ref()),
            ("dispatchPools", self.dispatch_pools.as_ref()),
            ("principals", self.principals.as_ref()),
            ("processes", self.processes.as_ref()),
            ("scheduledJobs", self.scheduled_jobs.as_ref()),
            ("openapi", self.openapi.as_ref()),
        ]
    }

    /// True when at least one category recorded a non-zero count.
    pub fn has_changes(&self) -> bool {
        self.categories()
            .iter()
            .any(|(_, r)| matches!(r, Some(c) if c.touched() > 0))
    }

    /// True when at least one category captured an error.
    pub fn has_errors(&self) -> bool {
        self.categories()
            .iter()
            .any(|(_, r)| matches!(r, Some(c) if c.is_error()))
    }

    /// Per-category errors as (category_name, message) pairs.
    pub fn errors(&self) -> Vec<(&'static str, &str)> {
        self.categories()
            .iter()
            .filter_map(|(name, r)| match r {
                Some(c) => c.error.as_deref().map(|msg| (*name, msg)),
                None => None,
            })
            .collect()
    }

    /// Summed `(created, updated, deleted)` counts across every category
    /// that ran.
    pub fn totals(&self) -> (u32, u32, u32) {
        self.categories()
            .iter()
            .fold((0, 0, 0), |(c, u, d), (_, r)| match r {
                Some(cat) => (c + cat.created, u + cat.updated, d + cat.deleted),
                None => (c, u, d),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cat(created: u32, updated: u32, deleted: u32) -> CategorySyncResult {
        CategorySyncResult {
            created,
            updated,
            deleted,
            synced_codes: Vec::new(),
            password_hash_ignored: Vec::new(),
            error: None,
        }
    }

    #[test]
    fn empty_result_has_no_changes_or_errors() {
        let r = SyncResult::default();
        assert!(!r.has_changes());
        assert!(!r.has_errors());
        assert_eq!(r.totals(), (0, 0, 0));
        assert!(r.errors().is_empty());
    }

    #[test]
    fn totals_sums_across_categories() {
        let mut r = SyncResult::default();
        r.roles = Some(cat(1, 2, 0));
        r.event_types = Some(cat(0, 0, 3));
        r.processes = Some(cat(5, 1, 0));
        assert_eq!(r.totals(), (6, 3, 3));
        assert!(r.has_changes());
        assert!(!r.has_errors());
    }

    #[test]
    fn errors_collected_by_category_name() {
        let mut r = SyncResult::default();
        r.roles = Some(cat(1, 0, 0));
        r.event_types = Some(CategorySyncResult::from_error("validation failed"));
        r.dispatch_pools = Some(CategorySyncResult::from_error("network timeout"));
        assert!(r.has_errors());
        let errors = r.errors();
        assert_eq!(errors.len(), 2);
        assert_eq!(errors[0].0, "eventTypes");
        assert_eq!(errors[0].1, "validation failed");
        assert_eq!(errors[1].0, "dispatchPools");
    }

    #[test]
    fn category_from_error_zeroes_counts() {
        let c = CategorySyncResult::from_error("boom");
        assert_eq!(c.created, 0);
        assert_eq!(c.touched(), 0);
        assert!(c.is_error());
    }
}
