//! Warning Service - In-memory warning storage and management
//!
//! Provides:
//! - Warning storage with categories and severity levels
//! - Automatic cleanup of old warnings
//! - Warning acknowledgment
//! - Filtering by severity/category
//! - Optional notification integration (Teams, email, etc.)

use std::collections::HashMap;
use std::sync::Arc;
use chrono::Utc;
use parking_lot::RwLock;
use tracing::{debug, info};

use fc_common::{Warning, WarningCategory, WarningSeverity};
use crate::notification::NotificationService;

/// Configuration for warning service
#[derive(Debug, Clone)]
pub struct WarningServiceConfig {
    /// Maximum age of warnings in hours before auto-cleanup
    pub max_warning_age_hours: i64,
    /// Maximum number of warnings to keep
    pub max_warnings: usize,
    /// Auto-acknowledge warnings older than this (hours)
    pub auto_acknowledge_hours: i64,
}

impl Default for WarningServiceConfig {
    fn default() -> Self {
        Self {
            // Java: warnings auto-expire after 8 hours
            max_warning_age_hours: 8,
            max_warnings: 1000,
            auto_acknowledge_hours: 8,
        }
    }
}

/// In-memory warning service
pub struct WarningService {
    warnings: RwLock<HashMap<String, Warning>>,
    config: WarningServiceConfig,
    notification_service: RwLock<Option<Arc<dyn NotificationService>>>,
}

impl WarningService {
    pub fn new(config: WarningServiceConfig) -> Self {
        Self {
            warnings: RwLock::new(HashMap::new()),
            config,
            notification_service: RwLock::new(None),
        }
    }

    /// Set the notification service for sending alerts
    pub fn set_notification_service(&self, service: Arc<dyn NotificationService>) {
        *self.notification_service.write() = Some(service);
        info!("Notification service attached to WarningService");
    }

    /// Create a new warning service with notification support
    pub fn with_notification(config: WarningServiceConfig, notification: Arc<dyn NotificationService>) -> Self {
        Self {
            warnings: RwLock::new(HashMap::new()),
            config,
            notification_service: RwLock::new(Some(notification)),
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

        // Send notification if service is configured
        if let Some(ref notification_service) = *self.notification_service.read() {
            let ns = notification_service.clone();
            tokio::spawn(async move {
                ns.notify_warning(&warning).await;
            });
        }

        id
    }

    /// Add a warning and return Arc<Self> for chaining
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

        // Clear very old warnings
        self.clear_old_warnings(self.config.max_warning_age_hours);
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

impl Default for WarningService {
    fn default() -> Self {
        Self {
            warnings: RwLock::new(HashMap::new()),
            config: WarningServiceConfig::default(),
            notification_service: RwLock::new(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
