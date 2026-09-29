//! Warning Service - In-memory warning storage and management
//!
//! Provides:
//! - Warning storage with categories and severity levels
//! - Automatic cleanup of old warnings
//! - Warning acknowledgment
//! - Filtering by severity/category
//! - Optional notification integration (Teams, email, etc.)

use chrono::Utc;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info};

use crate::notification::NotificationService;
use fc_common::{Warning, WarningCategory, WarningSeverity};
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use tokio::runtime::Handle;
use tokio::sync::Semaphore;

/// Parse a severity name as the warnings API and `FC_NOTIFY_MIN_SEVERITY`
/// accept it: case-insensitive `INFO`, `WARN`/`WARNING`, `ERROR` or
/// `CRITICAL`. `None` for anything else.
///
/// (A free function rather than `impl FromStr` because `WarningSeverity`
/// lives in fc-common.)
pub fn parse_severity(s: &str) -> Option<WarningSeverity> {
    match s.to_uppercase().as_str() {
        "INFO" => Some(WarningSeverity::Info),
        "WARN" | "WARNING" => Some(WarningSeverity::Warn),
        "ERROR" => Some(WarningSeverity::Error),
        "CRITICAL" => Some(WarningSeverity::Critical),
        _ => None,
    }
}

/// Configuration for warning service
#[derive(Debug, Clone)]
pub struct WarningServiceConfig {
    /// Maximum age of warnings in hours before auto-cleanup (hard removal;
    /// see [`Self::info_max_age_minutes`] for INFO's shorter override)
    pub max_warning_age_hours: i64,
    /// Maximum number of warnings to keep
    pub max_warnings: usize,
    /// Auto-acknowledge warnings older than this (hours). A-08
    /// (2026-09-02): `AUTO_ACKNOWLEDGE_AGE` = 1 hour (was Java's 8 hours).
    pub auto_acknowledge_hours: i64,
    /// X-04: INFO-severity warnings are purged after this many minutes
    /// instead of `max_warning_age_hours`. INFO covers routine/expected
    /// conditions that recur far more often than real WARNING+ signals;
    /// without a shorter TTL a chatty INFO source can crowd the bounded
    /// `max_warnings` store via the oldest-10% eviction on overflow
    /// ("INFO must not crowd the store"). Kept meaningfully shorter than
    /// `auto_acknowledge_hours` (now 1h under A-08), so this defaults to
    /// 15 minutes. See `WarningService::clear_aged_warnings`.
    pub info_max_age_minutes: i64,
}

impl Default for WarningServiceConfig {
    fn default() -> Self {
        Self {
            // Hard-delete ceiling for non-INFO warnings; unaffected by A-08.
            max_warning_age_hours: 8,
            max_warnings: 1000,
            // A-08 (2026-09-02): AUTO_ACKNOWLEDGE_AGE = 1 hour.
            auto_acknowledge_hours: 1,
            info_max_age_minutes: 15,
        }
    }
}

/// Most notification deliveries in flight at once. Past it a warning is
/// still stored (and served by the API); only its notification is dropped,
/// and counted.
const MAX_NOTIFICATIONS_IN_FLIGHT: usize = 64;

/// In-memory warning service
pub struct WarningService {
    warnings: RwLock<HashMap<String, Warning>>,
    config: WarningServiceConfig,
    notification_service: Option<Arc<dyn NotificationService>>,
    /// Bounds the notification tasks `add_warning` spawns: a warning storm
    /// against a slow or hung channel used to spawn one task per warning
    /// without limit (Go's unbounded notification spawns).
    notify_permits: Arc<Semaphore>,
    notifications_dropped: AtomicU64,
}

impl WarningService {
    pub fn new(config: WarningServiceConfig) -> Self {
        Self {
            warnings: RwLock::new(HashMap::new()),
            config,
            notification_service: None,
            notify_permits: Arc::new(Semaphore::new(MAX_NOTIFICATIONS_IN_FLIGHT)),
            notifications_dropped: AtomicU64::new(0),
        }
    }

    /// Create a new warning service with notification support
    pub fn with_notification(
        config: WarningServiceConfig,
        notification: Arc<dyn NotificationService>,
    ) -> Self {
        Self {
            warnings: RwLock::new(HashMap::new()),
            config,
            notification_service: Some(notification),
            notify_permits: Arc::new(Semaphore::new(MAX_NOTIFICATIONS_IN_FLIGHT)),
            notifications_dropped: AtomicU64::new(0),
        }
    }

    /// Add a new warning
    pub fn add_warning(
        &self,
        category: WarningCategory,
        severity: WarningSeverity,
        message: String,
        source: String,
    ) -> String {
        let warning = Warning::new(category, severity, message, source);
        let id = warning.id.clone();

        let mut warnings = self.warnings.write();

        // Enforce max warnings limit
        if warnings.len() >= self.config.max_warnings {
            self.cleanup_oldest_internal(&mut warnings);
        }

        debug!(
            id = %id,
            category = ?category,
            severity = ?severity,
            "Added warning"
        );

        warnings.insert(id.clone(), warning.clone());

        // Send notification if service is configured.
        //
        // **Spawn:** fire-and-forget, at most `MAX_NOTIFICATIONS_IN_FLIGHT`
        // at once. **Owns:** an Arc clone of the notification service, the
        // `warning` value (moved in) and a permit. **Exits:** as soon as
        // `notify_warning` returns (one-shot). **Joined by:** nobody — we
        // don't block `add_warning` on notification delivery, since
        // notification failures (Teams / email transient errors) must not
        // stall warning ingestion. Outside a runtime (a sync caller in a
        // test) there is nothing to spawn on and the notification is
        // skipped.
        if let Some(ns) = self.notification_service.clone() {
            match (
                self.notify_permits.clone().try_acquire_owned(),
                Handle::try_current(),
            ) {
                (Ok(permit), Ok(rt)) => {
                    rt.spawn(async move {
                        ns.notify_warning(&warning).await;
                        drop(permit);
                    });
                }
                _ => {
                    let dropped = self.notifications_dropped.fetch_add(1, Ordering::Relaxed) + 1;
                    if dropped.is_power_of_two() {
                        tracing::warn!(
                            dropped,
                            in_flight_limit = MAX_NOTIFICATIONS_IN_FLIGHT,
                            "Notification deliveries saturated; warning stored but not notified"
                        );
                    }
                }
            }
        }

        id
    }

    /// Notifications dropped because too many deliveries were in flight.
    pub fn notifications_dropped(&self) -> u64 {
        self.notifications_dropped.load(Ordering::Relaxed)
    }

    /// Add a warning. Returns the new warning's id.
    ///
    /// **`self: &Arc<Self>` is speculative here** — the body only forwards
    /// to `add_warning(&self, …)` and doesn't use the Arc-ness of the
    /// receiver, so a plain `&self` would do. Kept under the audit-only
    /// pass; safe to downgrade to `&self` in a follow-up.
    pub fn warn(
        self: &Arc<Self>,
        category: WarningCategory,
        severity: WarningSeverity,
        message: impl Into<String>,
        source: impl Into<String>,
    ) -> String {
        self.add_warning(category, severity, message.into(), source.into())
    }

    /// Get all warnings
    pub fn get_all_warnings(&self) -> Vec<Warning> {
        self.warnings.read().values().cloned().collect()
    }

    /// Get warnings by severity
    pub fn get_warnings_by_severity(&self, severity: WarningSeverity) -> Vec<Warning> {
        self.warnings
            .read()
            .values()
            .filter(|w| w.severity == severity)
            .cloned()
            .collect()
    }

    /// Get warnings by category
    pub fn get_warnings_by_category(&self, category: WarningCategory) -> Vec<Warning> {
        self.warnings
            .read()
            .values()
            .filter(|w| w.category == category)
            .cloned()
            .collect()
    }

    /// Get unacknowledged warnings
    pub fn get_unacknowledged_warnings(&self) -> Vec<Warning> {
        self.warnings
            .read()
            .values()
            .filter(|w| !w.acknowledged)
            .cloned()
            .collect()
    }

    /// Get active warnings (unacknowledged and not too old)
    pub fn get_active_warnings(&self, max_age_minutes: i64) -> Vec<Warning> {
        self.warnings
            .read()
            .values()
            .filter(|w| !w.acknowledged && w.age_minutes() <= max_age_minutes)
            .cloned()
            .collect()
    }

    /// Get critical warnings
    pub fn get_critical_warnings(&self) -> Vec<Warning> {
        self.get_warnings_by_severity(WarningSeverity::Critical)
    }

    /// Acknowledge a warning
    pub fn acknowledge_warning(&self, id: &str) -> bool {
        let mut warnings = self.warnings.write();
        if let Some(warning) = warnings.get_mut(id) {
            warning.acknowledged = true;
            warning.acknowledged_at = Some(Utc::now());
            debug!(id = %id, "Warning acknowledged");
            true
        } else {
            false
        }
    }

    /// Acknowledge all warnings matching a predicate
    pub fn acknowledge_matching<F>(&self, predicate: F) -> usize
    where
        F: Fn(&Warning) -> bool,
    {
        let mut warnings = self.warnings.write();
        let now = Utc::now();
        let mut count = 0;

        for warning in warnings.values_mut() {
            if !warning.acknowledged && predicate(warning) {
                warning.acknowledged = true;
                warning.acknowledged_at = Some(now);
                count += 1;
            }
        }

        if count > 0 {
            debug!(count = count, "Acknowledged warnings");
        }
        count
    }

    /// Auto-acknowledge old warnings
    pub fn auto_acknowledge_old_warnings(&self) -> usize {
        let threshold_hours = self.config.auto_acknowledge_hours;
        self.acknowledge_matching(|w| w.age_minutes() > threshold_hours * 60)
    }

    /// X-04: severity-aware sweep — removes every warning past its own
    /// retention window. INFO-severity warnings are removed once older than
    /// `info_max_age_minutes`; every other severity keeps the general
    /// `max_warning_age_hours` window. This is what [`Self::cleanup`] runs
    /// (replacing a flat `clear_old_warnings` call) so a chatty INFO source
    /// can't sit in the bounded store for the full general age.
    ///
    /// Distinct from [`Self::clear_old_warnings`], which stays a blunt
    /// uniform-cutoff tool (used by the `/warnings/old` admin endpoint).
    pub fn clear_aged_warnings(&self) -> usize {
        let mut warnings = self.warnings.write();
        let general_limit_minutes = self.config.max_warning_age_hours * 60;
        let info_limit_minutes = self.config.info_max_age_minutes;
        let before_count = warnings.len();

        warnings.retain(|_, w| {
            let limit = if w.severity == WarningSeverity::Info {
                info_limit_minutes
            } else {
                general_limit_minutes
            };
            w.age_minutes() <= limit
        });

        let removed = before_count - warnings.len();
        if removed > 0 {
            info!(
                removed = removed,
                "Cleared aged warnings (severity-aware sweep)"
            );
        }
        removed
    }

    /// Clear warnings older than specified hours
    pub fn clear_old_warnings(&self, hours_old: i64) -> usize {
        let mut warnings = self.warnings.write();
        let threshold_minutes = hours_old * 60;
        let before_count = warnings.len();

        warnings.retain(|_, w| w.age_minutes() <= threshold_minutes);

        let removed = before_count - warnings.len();
        if removed > 0 {
            info!(removed = removed, "Cleared old warnings");
        }
        removed
    }

    /// Clear all acknowledged warnings
    pub fn clear_acknowledged(&self) -> usize {
        let mut warnings = self.warnings.write();
        let before_count = warnings.len();

        warnings.retain(|_, w| !w.acknowledged);

        before_count - warnings.len()
    }

    /// Remove a specific warning
    pub fn remove_warning(&self, id: &str) -> bool {
        self.warnings.write().remove(id).is_some()
    }

    /// Get warning count
    pub fn warning_count(&self) -> usize {
        self.warnings.read().len()
    }

    /// Get unacknowledged warning count
    pub fn unacknowledged_count(&self) -> usize {
        self.warnings
            .read()
            .values()
            .filter(|w| !w.acknowledged)
            .count()
    }

    /// Get critical warning count
    pub fn critical_count(&self) -> usize {
        self.warnings
            .read()
            .values()
            .filter(|w| w.severity == WarningSeverity::Critical && !w.acknowledged)
            .count()
    }

    /// Check if there are any critical unacknowledged warnings
    pub fn has_critical_warnings(&self) -> bool {
        self.warnings
            .read()
            .values()
            .any(|w| w.severity == WarningSeverity::Critical && !w.acknowledged)
    }

    /// Periodic cleanup task
    pub fn cleanup(&self) {
        // Auto-acknowledge old warnings
        self.auto_acknowledge_old_warnings();

        // X-04: severity-aware sweep — INFO ages out sooner than everything
        // else so it can't crowd the bounded store.
        self.clear_aged_warnings();
    }

    /// Internal helper to remove oldest warnings
    fn cleanup_oldest_internal(&self, warnings: &mut HashMap<String, Warning>) {
        // Remove oldest 10% when at capacity
        let to_remove = warnings.len() / 10;
        if to_remove == 0 {
            return;
        }

        let mut sorted: Vec<_> = warnings.iter().collect();
        sorted.sort_by_key(|(_, w)| w.created_at);

        let ids_to_remove: Vec<String> = sorted
            .into_iter()
            .take(to_remove)
            .map(|(id, _)| id.clone())
            .collect();

        for id in ids_to_remove {
            warnings.remove(&id);
        }
    }
}

impl WarningService {
    /// Create a no-op warning service with default config.
    /// Used as the default when no explicit warning service is configured.
    pub fn noop() -> Self {
        Self::new(WarningServiceConfig::default())
    }
}

impl Default for WarningService {
    fn default() -> Self {
        Self::noop()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future;
    use tokio::runtime::Handle;
    use tokio::task;

    /// A notification channel that never answers.
    struct Hung;

    #[async_trait::async_trait]
    impl NotificationService for Hung {
        async fn notify_warning(&self, _: &Warning) {
            future::pending::<()>().await;
        }
        async fn notify_critical_error(&self, _: &str, _: &str) {}
        async fn notify_system_event(&self, _: &str, _: &str) {}
        fn is_enabled(&self) -> bool {
            true
        }
    }

    /// A warning storm against a hung channel spawns a bounded number of
    /// notification tasks (Go spawned one per warning without limit); the
    /// warnings themselves are all stored.
    #[tokio::test]
    async fn notification_spawns_are_bounded() {
        let service =
            WarningService::with_notification(WarningServiceConfig::default(), Arc::new(Hung));
        let before = Handle::current().metrics().num_alive_tasks();
        for i in 0..500 {
            service.add_warning(
                WarningCategory::Processing,
                WarningSeverity::Error,
                format!("w{i}"),
                "test".into(),
            );
        }
        task::yield_now().await;
        let spawned = Handle::current().metrics().num_alive_tasks() - before;
        assert_eq!(spawned, MAX_NOTIFICATIONS_IN_FLIGHT);
        assert_eq!(
            service.notifications_dropped(),
            (500 - MAX_NOTIFICATIONS_IN_FLIGHT) as u64
        );
        assert_eq!(service.get_all_warnings().len(), 500);
    }

    #[test]
    fn test_add_and_get_warning() {
        let service = WarningService::default();

        let id = service.add_warning(
            WarningCategory::Processing,
            WarningSeverity::Error,
            "Test error".to_string(),
            "test".to_string(),
        );

        let warnings = service.get_all_warnings();
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].id, id);
    }

    #[test]
    fn test_acknowledge_warning() {
        let service = WarningService::default();

        let id = service.add_warning(
            WarningCategory::Processing,
            WarningSeverity::Warn,
            "Test warning".to_string(),
            "test".to_string(),
        );

        assert_eq!(service.unacknowledged_count(), 1);

        service.acknowledge_warning(&id);

        assert_eq!(service.unacknowledged_count(), 0);
    }

    #[test]
    fn test_filter_by_severity() {
        let service = WarningService::default();

        service.add_warning(
            WarningCategory::Processing,
            WarningSeverity::Warn,
            "Warning".to_string(),
            "test".to_string(),
        );
        service.add_warning(
            WarningCategory::Processing,
            WarningSeverity::Critical,
            "Critical".to_string(),
            "test".to_string(),
        );

        let critical = service.get_critical_warnings();
        assert_eq!(critical.len(), 1);
        assert_eq!(critical[0].message, "Critical");
    }

    /// X-04: INFO must not crowd the store — it ages out (is actually
    /// removed) on a shorter TTL than every other severity. Backdate both
    /// an INFO and a WARN warning by the same amount (20 minutes: past
    /// INFO's 15-minute default, well under the 8h general window) and
    /// confirm the sweep only removes the INFO one.
    #[test]
    fn clear_aged_warnings_purges_info_sooner_than_other_severities() {
        let service = WarningService::default();
        assert_eq!(service.config.info_max_age_minutes, 15);

        let info_id = service.add_warning(
            WarningCategory::Processing,
            WarningSeverity::Info,
            "info warning".to_string(),
            "test".to_string(),
        );
        let warn_id = service.add_warning(
            WarningCategory::Processing,
            WarningSeverity::Warn,
            "warn warning".to_string(),
            "test".to_string(),
        );

        let backdate = Utc::now() - chrono::Duration::minutes(20);
        {
            let mut warnings = service.warnings.write();
            warnings.get_mut(&info_id).unwrap().created_at = backdate;
            warnings.get_mut(&warn_id).unwrap().created_at = backdate;
        }

        let removed = service.clear_aged_warnings();
        assert_eq!(removed, 1, "only the aged INFO warning should be swept");

        let remaining_ids: Vec<String> = service
            .get_all_warnings()
            .into_iter()
            .map(|w| w.id)
            .collect();
        assert!(
            !remaining_ids.contains(&info_id),
            "aged INFO warning must be gone"
        );
        assert!(
            remaining_ids.contains(&warn_id),
            "equally-aged WARNING must remain"
        );
    }
}
