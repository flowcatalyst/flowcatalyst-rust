//! Options controlling a `DefinitionSynchronizer::sync` call.
//!
//! `SyncOptions` carries two distinct knobs:
//!
//! - `remove_unlisted` — passes `?removeUnlisted=true` to every per-category
//!   endpoint so the platform archives/removes SDK-sourced rows not in the
//!   submitted list. UI-sourced rows are never touched by the platform.
//! - `sync_<category>` flags — per-category skip switches. A category is also
//!   implicitly skipped when its vector on the `DefinitionSet` is empty.

/// Options for a `DefinitionSynchronizer::sync` call.
///
/// By default every category is enabled. Use `defaults()` for a no-op-friendly
/// baseline, `with_remove_unlisted()` to opt into removal, or the
/// `*_only()` factories to drive a single category in isolation.
#[derive(Debug, Clone)]
pub struct SyncOptions {
    /// When true, the platform removes API/CODE-sourced rows for each
    /// category that aren't in the submitted list. UI-sourced rows are
    /// preserved regardless. Default `false`.
    pub remove_unlisted: bool,
    pub sync_roles: bool,
    pub sync_event_types: bool,
    pub sync_subscriptions: bool,
    pub sync_dispatch_pools: bool,
    pub sync_principals: bool,
    pub sync_processes: bool,
    pub sync_scheduled_jobs: bool,
    pub sync_openapi: bool,
}

impl Default for SyncOptions {
    fn default() -> Self {
        Self::defaults()
    }
}

impl SyncOptions {
    /// Every category enabled, `remove_unlisted = false`.
    pub const fn defaults() -> Self {
        Self {
            remove_unlisted: false,
            sync_roles: true,
            sync_event_types: true,
            sync_subscriptions: true,
            sync_dispatch_pools: true,
            sync_principals: true,
            sync_processes: true,
            sync_scheduled_jobs: true,
            sync_openapi: true,
        }
    }

    /// Disable every category. Use as the starting point for selectively
    /// enabling just one or two via `..SyncOptions::none()` spread.
    pub const fn none() -> Self {
        Self {
            remove_unlisted: false,
            sync_roles: false,
            sync_event_types: false,
            sync_subscriptions: false,
            sync_dispatch_pools: false,
            sync_principals: false,
            sync_processes: false,
            sync_scheduled_jobs: false,
            sync_openapi: false,
        }
    }

    /// `defaults()` plus `remove_unlisted = true`.
    pub const fn with_remove_unlisted() -> Self {
        Self {
            remove_unlisted: true,
            ..Self::defaults()
        }
    }

    /// Toggle `remove_unlisted` while keeping the same category mask.
    pub fn remove_unlisted_enabled(mut self) -> Self {
        self.remove_unlisted = true;
        self
    }

    pub const fn roles_only() -> Self {
        Self {
            sync_roles: true,
            ..Self::none()
        }
    }

    pub const fn event_types_only() -> Self {
        Self {
            sync_event_types: true,
            ..Self::none()
        }
    }

    pub const fn subscriptions_only() -> Self {
        Self {
            sync_subscriptions: true,
            ..Self::none()
        }
    }

    pub const fn dispatch_pools_only() -> Self {
        Self {
            sync_dispatch_pools: true,
            ..Self::none()
        }
    }

    pub const fn principals_only() -> Self {
        Self {
            sync_principals: true,
            ..Self::none()
        }
    }

    pub const fn processes_only() -> Self {
        Self {
            sync_processes: true,
            ..Self::none()
        }
    }

    pub const fn scheduled_jobs_only() -> Self {
        Self {
            sync_scheduled_jobs: true,
            ..Self::none()
        }
    }

    pub const fn openapi_only() -> Self {
        Self {
            sync_openapi: true,
            ..Self::none()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_enables_everything() {
        let opts = SyncOptions::defaults();
        assert!(!opts.remove_unlisted);
        assert!(opts.sync_roles);
        assert!(opts.sync_event_types);
        assert!(opts.sync_subscriptions);
        assert!(opts.sync_dispatch_pools);
        assert!(opts.sync_principals);
        assert!(opts.sync_processes);
        assert!(opts.sync_scheduled_jobs);
        assert!(opts.sync_openapi);
    }

    #[test]
    fn category_only_factories_isolate_one_category() {
        let opts = SyncOptions::roles_only();
        assert!(opts.sync_roles);
        assert!(!opts.sync_event_types);
        assert!(!opts.sync_subscriptions);
        assert!(!opts.sync_dispatch_pools);
        assert!(!opts.sync_principals);
        assert!(!opts.sync_processes);
        assert!(!opts.sync_scheduled_jobs);
        assert!(!opts.sync_openapi);

        let opts = SyncOptions::scheduled_jobs_only();
        assert!(opts.sync_scheduled_jobs);
        assert!(!opts.sync_roles);

        let opts = SyncOptions::openapi_only();
        assert!(opts.sync_openapi);
        assert!(!opts.sync_roles);
    }

    #[test]
    fn remove_unlisted_enabled_preserves_mask() {
        let opts = SyncOptions::roles_only().remove_unlisted_enabled();
        assert!(opts.remove_unlisted);
        assert!(opts.sync_roles);
        assert!(!opts.sync_event_types);
    }
}
