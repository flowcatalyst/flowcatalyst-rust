//! Configuration Sync Service
//!
//! Periodically fetches configuration from a central service and applies changes
//! to the router without restart. Mirrors the Java QueueManager.scheduledSync() behavior.

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::{info, warn, error, debug};
use serde::{Deserialize, Serialize};

use fc_common::{RouterConfig, PoolConfig, QueueConfig};
use crate::manager::QueueManager;
use crate::warning::WarningService;

/// Configuration for the config sync service
#[derive(Debug, Clone)]
pub struct ConfigSyncConfig {
    /// Enable configuration sync
    pub enabled: bool,

    /// URL to fetch configuration from
    pub config_url: String,

    /// Sync interval (how often to check for config changes)
    pub sync_interval: Duration,

    /// Maximum retry attempts on failure
    pub max_retry_attempts: u32,

    /// Delay between retry attempts
    pub retry_delay: Duration,

    /// HTTP request timeout
    pub request_timeout: Duration,

    /// Whether to fail startup if initial sync fails
    pub fail_on_initial_sync_error: bool,
}

impl Default for ConfigSyncConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            config_url: String::new(),
            sync_interval: Duration::from_secs(300), // 5 minutes (matches Java)
            max_retry_attempts: 12,                   // 12 attempts (matches Java)
            retry_delay: Duration::from_secs(5),     // 5 seconds between retries
            request_timeout: Duration::from_secs(30),
            fail_on_initial_sync_error: true,
        }
    }
}

impl ConfigSyncConfig {
    pub fn new(config_url: String) -> Self {
        Self {
            enabled: true,
            config_url,
            ..Default::default()
        }
    }

    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.sync_interval = interval;
        self
    }

    pub fn with_retry_config(mut self, max_attempts: u32, delay: Duration) -> Self {
        self.max_retry_attempts = max_attempts;
        self.retry_delay = delay;
        self
    }
}

/// Response from the configuration service
/// Matches the Java MessageRouterConfig structure
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageRouterConfigResponse {
    pub processing_pools: Vec<PoolConfigResponse>,
    pub queues: Vec<QueueConfigResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolConfigResponse {
    pub code: String,
    pub concurrency: usize,
    #[serde(default)]
    pub rate_limit_per_minute: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueConfigResponse {
    #[serde(alias = "queueName")]
    pub queue_name: Option<String>,
    #[serde(alias = "queueUri")]
    pub queue_uri: String,
    #[serde(default)]
    pub connections: Option<u32>,
    #[serde(default)]
    pub visibility_timeout: Option<u32>,
}

impl From<MessageRouterConfigResponse> for RouterConfig {
    fn from(response: MessageRouterConfigResponse) -> Self {
        RouterConfig {
            processing_pools: response.processing_pools
                .into_iter()
                .map(|p| PoolConfig {
                    code: p.code,
                    concurrency: p.concurrency as u32,
                    rate_limit_per_minute: p.rate_limit_per_minute,
                })
                .collect(),
            queues: response.queues
                .into_iter()
                .map(|q| QueueConfig {
                    name: q.queue_name.unwrap_or_else(|| q.queue_uri.clone()),
                    uri: q.queue_uri,
                    connections: q.connections.unwrap_or(1),
                    visibility_timeout: q.visibility_timeout.unwrap_or(120),
                })
                .collect(),
        }
    }
}

/// Configuration sync result
#[derive(Debug, Clone)]
pub struct ConfigSyncResult {
    pub success: bool,
    pub pools_updated: usize,
    pub pools_created: usize,
    pub pools_removed: usize,
    pub error: Option<String>,
}

/// Service that periodically syncs configuration from a central service
pub struct ConfigSyncService {
    config: ConfigSyncConfig,
    http_client: reqwest::Client,
    queue_manager: Arc<QueueManager>,
    warning_service: Arc<WarningService>,
    last_config_hash: parking_lot::Mutex<Option<u64>>,
}

impl ConfigSyncService {
    pub fn new(
        config: ConfigSyncConfig,
        queue_manager: Arc<QueueManager>,
        warning_service: Arc<WarningService>,
    ) -> Self {
        let http_client = reqwest::Client::builder()
            .timeout(config.request_timeout)
            .build()
            .expect("Failed to create HTTP client");

        Self {
            config,
            http_client,
            queue_manager,
            warning_service,
            last_config_hash: parking_lot::Mutex::new(None),
        }
    }

    /// Fetch configuration from the remote service with retry logic
    pub async fn fetch_config(&self) -> Result<RouterConfig, String> {
        let mut last_error = String::new();

        for attempt in 1..=self.config.max_retry_attempts {
            debug!(
                attempt = attempt,
                max_attempts = self.config.max_retry_attempts,
                url = %self.config.config_url,
                "Fetching configuration"
            );

            match self.fetch_config_once().await {
                Ok(config) => {
                    if attempt > 1 {
                        info!(
                            attempt = attempt,
                            "Successfully fetched configuration after retries"
                        );
                    }
                    return Ok(config);
                }
                Err(e) => {
                    last_error = e.clone();
                    if attempt < self.config.max_retry_attempts {
                        warn!(
                            attempt = attempt,
                            max_attempts = self.config.max_retry_attempts,
                            error = %e,
                            retry_delay_secs = self.config.retry_delay.as_secs(),
                            "Failed to fetch config, retrying..."
                        );
                        tokio::time::sleep(self.config.retry_delay).await;
                    }
                }
            }
        }

        error!(
            attempts = self.config.max_retry_attempts,
            error = %last_error,
            "Failed to fetch configuration after all retries"
        );

        Err(last_error)
    }

    /// Single fetch attempt
    async fn fetch_config_once(&self) -> Result<RouterConfig, String> {
        let response = self.http_client
            .get(&self.config.config_url)
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {}", e))?;

        if !response.status().is_success() {
            return Err(format!(
                "Config service returned status {}",
                response.status()
            ));
        }

        let config_response: MessageRouterConfigResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse config response: {}", e))?;

        Ok(config_response.into())
    }

    /// Compute a hash of the configuration for change detection
    fn compute_config_hash(config: &RouterConfig) -> u64 {
        use std::hash::{Hash, Hasher};
        use std::collections::hash_map::DefaultHasher;

        let mut hasher = DefaultHasher::new();

        // Hash pools
        for pool in &config.processing_pools {
            pool.code.hash(&mut hasher);
            pool.concurrency.hash(&mut hasher);
            pool.rate_limit_per_minute.hash(&mut hasher);
        }

        // Hash queues
        for queue in &config.queues {
            queue.name.hash(&mut hasher);
            queue.uri.hash(&mut hasher);
            queue.connections.hash(&mut hasher);
        }

        hasher.finish()
    }

    /// Sync configuration - fetch and apply if changed
    pub async fn sync(&self) -> ConfigSyncResult {
        // Fetch new config
        let new_config = match self.fetch_config().await {
            Ok(config) => config,
            Err(e) => {
                // Java: periodic sync failure → CONFIG_SYNC_FAILED WARN (not CRITICAL/ERROR)
                // continues processing with existing configuration
                self.warning_service.add_warning(
                    fc_common::WarningCategory::Configuration,
                    fc_common::WarningSeverity::Warn,
                    format!("Config sync failed: {}", e),
                    "ConfigSyncService".to_string(),
                );
                return ConfigSyncResult {
                    success: false,
                    pools_updated: 0,
                    pools_created: 0,
                    pools_removed: 0,
                    error: Some(e),
                };
            }
        };

        // Check if config has changed
        let new_hash = Self::compute_config_hash(&new_config);

        // Check hash with lock held briefly
        let config_changed = {
            let last_hash = self.last_config_hash.lock();
            Some(new_hash) != *last_hash
        };

        if !config_changed {
            debug!("Configuration unchanged, skipping reload");
            return ConfigSyncResult {
                success: true,
                pools_updated: 0,
                pools_created: 0,
                pools_removed: 0,
                error: None,
            };
        }

        info!(
            pools = new_config.processing_pools.len(),
            queues = new_config.queues.len(),
            "Configuration changed, applying updates"
        );

        // Apply config changes (lock is not held here)
        match self.queue_manager.reload_config(new_config).await {
            Ok(true) => {
                // Update the hash after successful reload
                *self.last_config_hash.lock() = Some(new_hash);
                info!("Configuration sync completed successfully");
                ConfigSyncResult {
                    success: true,
                    pools_updated: 0,
                    pools_created: 0,
                    pools_removed: 0,
                    error: None,
                }
            }
            Ok(false) => {
                warn!("Configuration reload returned false (shutting down?)");
                ConfigSyncResult {
                    success: false,
                    pools_updated: 0,
                    pools_created: 0,
                    pools_removed: 0,
                    error: Some("Reload returned false".to_string()),
                }
            }
            Err(e) => {
                error!(error = %e, "Failed to apply configuration");
                // Java: periodic sync failure → CONFIG_SYNC_FAILED WARN
                self.warning_service.add_warning(
                    fc_common::WarningCategory::Configuration,
                    fc_common::WarningSeverity::Warn,
                    format!("Config reload failed: {}", e),
                    "ConfigSyncService".to_string(),
                );
                ConfigSyncResult {
                    success: false,
                    pools_updated: 0,
                    pools_created: 0,
                    pools_removed: 0,
                    error: Some(e.to_string()),
                }
            }
        }
    }

    /// Perform initial sync (blocks until successful or fails)
    /// Returns the fetched RouterConfig on success so consumers can be created from queue URLs
    pub async fn initial_sync(&self) -> Result<RouterConfig, String> {
        info!("Performing initial configuration sync...");

        // Fetch config first
        let config = self.fetch_config().await?;

        // Apply to queue manager
        if let Err(e) = self.queue_manager.reload_config(config.clone()).await {
            let error = format!("Failed to apply config: {}", e);
            if self.config.fail_on_initial_sync_error {
                return Err(error);
            } else {
                warn!("{}", error);
            }
        }

        // Update hash
        let new_hash = Self::compute_config_hash(&config);
        *self.last_config_hash.lock() = Some(new_hash);

        info!(
            pools = config.processing_pools.len(),
            queues = config.queues.len(),
            "Initial configuration sync completed successfully"
        );

        Ok(config)
    }

    /// Get the sync interval
    pub fn sync_interval(&self) -> Duration {
        self.config.sync_interval
    }

    /// Check if sync is enabled
    pub fn is_enabled(&self) -> bool {
        self.config.enabled && !self.config.config_url.is_empty()
    }
}

/// Spawn the config sync background task
pub fn spawn_config_sync_task(
    config_sync: Arc<ConfigSyncService>,
    shutdown_tx: broadcast::Sender<()>,
) -> tokio::task::JoinHandle<()> {
    let mut shutdown_rx = shutdown_tx.subscribe();
    let interval = config_sync.sync_interval();

    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);

        // Skip the first tick (initial sync already done)
        ticker.tick().await;

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    debug!("Running scheduled configuration sync");
                    let result = config_sync.sync().await;
                    if !result.success {
                        warn!(
                            error = ?result.error,
                            "Scheduled config sync failed - continuing with existing config"
                        );
                    }
                }
                _ = shutdown_rx.recv() => {
                    info!("Config sync task shutting down");
                    break;
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_sync_config_defaults() {
        let config = ConfigSyncConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.sync_interval, Duration::from_secs(300));
        assert_eq!(config.max_retry_attempts, 12);
    }

    #[test]
    fn test_config_hash_changes() {
        let config1 = RouterConfig {
            processing_pools: vec![PoolConfig {
                code: "POOL1".to_string(),
                concurrency: 10,
                rate_limit_per_minute: None,
            }],
            queues: vec![],
        };

        let config2 = RouterConfig {
            processing_pools: vec![PoolConfig {
                code: "POOL1".to_string(),
                concurrency: 20, // Changed
                rate_limit_per_minute: None,
            }],
            queues: vec![],
        };

        let hash1 = ConfigSyncService::compute_config_hash(&config1);
        let hash2 = ConfigSyncService::compute_config_hash(&config2);

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_config_hash_stable() {
        let config = RouterConfig {
            processing_pools: vec![PoolConfig {
                code: "POOL1".to_string(),
                concurrency: 10,
                rate_limit_per_minute: Some(100),
            }],
            queues: vec![],
        };

        let hash1 = ConfigSyncService::compute_config_hash(&config);
        let hash2 = ConfigSyncService::compute_config_hash(&config);

        assert_eq!(hash1, hash2);
    }
}
