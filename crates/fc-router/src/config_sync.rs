//! Configuration Sync Service
//!
//! Periodically fetches configuration from a central service and applies changes
//! to the router without restart. Mirrors the Java QueueManager.scheduledSync() behavior.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::manager::QueueManager;
use crate::warning::WarningService;
use fc_common::{PoolConfig, QueueConfig, RouterConfig, WarningCategory, WarningSeverity};

/// Configuration for the config sync service
#[derive(Debug, Clone)]
pub struct ConfigSyncConfig {
    /// Enable configuration sync
    pub enabled: bool,

    /// URLs to fetch configuration from (merged when multiple).
    /// Pools with the same code are deduplicated (last wins).
    /// Queues are merged (all included).
    pub config_urls: Vec<String>,

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
            config_urls: Vec::new(),
            sync_interval: Duration::from_secs(300), // 5 minutes (matches Java)
            max_retry_attempts: 12,                  // 12 attempts (matches Java)
            retry_delay: Duration::from_secs(5),     // 5 seconds between retries
            request_timeout: Duration::from_secs(30),
            fail_on_initial_sync_error: true,
        }
    }
}

impl ConfigSyncConfig {
    pub fn new(config_url: String) -> Self {
        let config_urls = config_url
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        Self {
            enabled: true,
            config_urls,
            ..Default::default()
        }
    }

    /// Kept for backwards compatibility — returns the first URL or empty string.
    pub fn config_url(&self) -> &str {
        self.config_urls.first().map(|s| s.as_str()).unwrap_or("")
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
            processing_pools: response
                .processing_pools
                .into_iter()
                .map(|p| PoolConfig {
                    code: p.code,
                    concurrency: p.concurrency as u32,
                    rate_limit_per_minute: p.rate_limit_per_minute,
                })
                .collect(),
            queues: response
                .queues
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

/// Typed error for the configuration fetch/parse pipeline.
///
/// Replaces the previous ad-hoc `Result<_, String>` so callers can match on
/// the failure kind (e.g. distinguish a bad HTTP status from a parse error)
/// instead of string-sniffing. Every variant carries the source `url` because
/// fetches fan out across multiple config endpoints and the operator needs to
/// know which one failed. It derives `Clone` (the network/codec source errors
/// are flattened to `String`/`StatusCode`) so the retry loop can hold the last
/// error without juggling non-`Clone` `reqwest::Error`s. `Display` text is kept
/// byte-for-byte compatible with the old format strings, since these messages
/// are logged and surfaced at startup.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ConfigSyncError {
    #[error("No config URLs configured")]
    NoUrls,

    #[error("HTTP request failed ({url}): {message}")]
    Request { url: String, message: String },

    #[error("Config service returned status {status} ({url})")]
    BadStatus {
        url: String,
        status: reqwest::StatusCode,
    },

    #[error("Failed to read response body ({url}): {message}")]
    Body { url: String, message: String },

    #[error("Failed to parse config response ({url}): {message}")]
    Parse { url: String, message: String },

    #[error("All {attempted} config source(s) failed — {summary}")]
    AllSourcesFailed { attempted: usize, summary: String },

    #[error("Failed to apply config: {0}")]
    Apply(String),
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
    /// R-30: the last successfully-fetched config per source URL, kept so a
    /// source that starts failing can still contribute its last-known-good
    /// config to the merge instead of dropping its pools/queues outright.
    source_cache: parking_lot::Mutex<HashMap<String, RouterConfig>>,
    /// R-30: the active "source failing" warning id per source URL, if any —
    /// raised once per failure streak (not once per tick) and acknowledged
    /// the moment the source recovers. Absence of an entry means the source
    /// is currently healthy (or has never been warned about).
    source_warnings: parking_lot::Mutex<HashMap<String, String>>,
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
            source_cache: parking_lot::Mutex::new(HashMap::new()),
            source_warnings: parking_lot::Mutex::new(HashMap::new()),
        }
    }

    /// R-30: source recovered — clear (acknowledge) its active "failing"
    /// warning, if any. A no-op for a source that was never warned about.
    fn clear_source_warning(&self, url: &str) {
        if let Some(id) = self.source_warnings.lock().remove(url) {
            self.warning_service.acknowledge_warning(&id);
            info!(source_url = %url, "Config source recovered");
        }
    }

    /// R-30: source is failing but has a cached last-known-good config —
    /// raise a CONFIGURATION warning once per failure streak (skip if one is
    /// already active for this source) rather than once per sync tick.
    fn raise_source_warning(&self, url: &str, err: &ConfigSyncError) {
        let mut warnings = self.source_warnings.lock();
        if !warnings.contains_key(url) {
            let id = self.warning_service.add_warning(
                WarningCategory::Configuration,
                WarningSeverity::Warn,
                format!(
                    "Config source [{}] is failing; serving its last-known-good configuration ({})",
                    url, err
                ),
                "ConfigSyncService".to_string(),
            );
            warnings.insert(url.to_string(), id);
        }
    }

    /// Fetch configuration from all configured URLs in parallel and merge.
    ///
    /// Per-URL failures are tolerated — the merge proceeds with whatever
    /// sources succeeded, plus (R-30) the last-known-good cached config of
    /// any failing source that has previously succeeded at least once: a
    /// small bad change on one source must not tear down that source's
    /// pools/queues, so consumers keep running on the stale-but-good config
    /// while a CONFIGURATION warning stays active for that source (cleared
    /// on recovery). Only fails outright if **every** source both fails
    /// *and* has no cache to fall back on — i.e. first boot with nothing up
    /// yet (matches TS `MultiConfigFetcher`'s "fails if all sources fail",
    /// extended with the last-known-good fallback).
    ///
    /// Merge strategy (union, first-wins; matches `multi-config-client.ts`):
    /// - Pools deduped by `code`; warn on conflicting duplicates.
    /// - Queues deduped by `uri`; warn on conflicting duplicates.
    pub async fn fetch_config(&self) -> Result<RouterConfig, ConfigSyncError> {
        if self.config.config_urls.is_empty() {
            return Err(ConfigSyncError::NoUrls);
        }

        // Fetch all sources in parallel.
        let tasks: Vec<_> = self
            .config
            .config_urls
            .iter()
            .map(|url| {
                let url = url.clone();
                let svc = self;
                async move {
                    let result = svc.fetch_config_from_url(&url).await;
                    (url, result)
                }
            })
            .collect();
        let results = futures::future::join_all(tasks).await;

        let mut contributions: Vec<(String, RouterConfig)> = Vec::new();
        let mut succeeded = 0usize;
        let mut served_from_cache = 0usize;
        let mut hard_failures: Vec<(String, ConfigSyncError)> = Vec::new();
        for (url, result) in results {
            match result {
                Ok(cfg) => {
                    self.source_cache.lock().insert(url.clone(), cfg.clone());
                    self.clear_source_warning(&url);
                    succeeded += 1;
                    contributions.push((url, cfg));
                }
                Err(e) => {
                    warn!(source_url = %url, error = %e, "Config source failed; continuing with remaining sources");
                    let cached = self.source_cache.lock().get(&url).cloned();
                    match cached {
                        Some(cfg) => {
                            self.raise_source_warning(&url, &e);
                            served_from_cache += 1;
                            contributions.push((url, cfg));
                        }
                        None => {
                            hard_failures.push((url, e));
                        }
                    }
                }
            }
        }

        if contributions.is_empty() {
            let summary = hard_failures
                .iter()
                .map(|(u, e)| format!("{}: {}", u, e))
                .collect::<Vec<_>>()
                .join("; ");
            return Err(ConfigSyncError::AllSourcesFailed {
                attempted: hard_failures.len(),
                summary,
            });
        }

        let merged = merge_configs(&contributions);

        info!(
            sources_attempted = self.config.config_urls.len(),
            sources_succeeded = succeeded,
            sources_served_from_cache = served_from_cache,
            sources_hard_failed = hard_failures.len(),
            pools = merged.processing_pools.len(),
            queues = merged.queues.len(),
            "Merged configuration from all sources"
        );

        Ok(merged)
    }

    /// Fetch configuration from a single URL with retry logic
    async fn fetch_config_from_url(&self, url: &str) -> Result<RouterConfig, ConfigSyncError> {
        let mut last_error: Option<ConfigSyncError> = None;

        for attempt in 1..=self.config.max_retry_attempts {
            debug!(
                attempt = attempt,
                max_attempts = self.config.max_retry_attempts,
                url = %url,
                "Fetching configuration"
            );

            match self.fetch_config_once(url).await {
                Ok(config) => {
                    if attempt > 1 {
                        info!(
                            attempt = attempt,
                            url = %url,
                            "Successfully fetched configuration after retries"
                        );
                    }
                    return Ok(config);
                }
                Err(e) => {
                    if attempt < self.config.max_retry_attempts {
                        warn!(
                            attempt = attempt,
                            max_attempts = self.config.max_retry_attempts,
                            url = %url,
                            error = %e,
                            retry_delay_secs = self.config.retry_delay.as_secs(),
                            "Failed to fetch config, retrying..."
                        );
                        last_error = Some(e);
                        tokio::time::sleep(self.config.retry_delay).await;
                    } else {
                        last_error = Some(e);
                    }
                }
            }
        }

        // Loop runs at least once (max_retry_attempts >= 1 in practice), so
        // last_error is set; fall back defensively if configured to 0.
        let last_error = last_error.unwrap_or_else(|| ConfigSyncError::Request {
            url: url.to_string(),
            message: "no fetch attempts were made (max_retry_attempts = 0)".to_string(),
        });

        error!(
            attempts = self.config.max_retry_attempts,
            url = %url,
            error = %last_error,
            "Failed to fetch configuration after all retries"
        );

        Err(last_error)
    }

    /// Single fetch attempt from a specific URL
    async fn fetch_config_once(&self, url: &str) -> Result<RouterConfig, ConfigSyncError> {
        let response = self
            .http_client
            .get(url)
            .send()
            .await
            .map_err(|e| ConfigSyncError::Request {
                url: url.to_string(),
                message: e.to_string(),
            })?;

        let status = response.status();
        if !status.is_success() {
            return Err(ConfigSyncError::BadStatus {
                url: url.to_string(),
                status,
            });
        }

        let body = response.text().await.map_err(|e| ConfigSyncError::Body {
            url: url.to_string(),
            message: e.to_string(),
        })?;

        debug!(url = %url, body_length = body.len(), "Config response received");

        let config_response: MessageRouterConfigResponse =
            serde_json::from_str(&body).map_err(|e| {
                warn!(
                    error = %e,
                    url = %url,
                    body = %body.chars().take(500).collect::<String>(),
                    "Failed to parse config response"
                );
                ConfigSyncError::Parse {
                    url: url.to_string(),
                    message: format!("{} — body: {}", e, &body[..body.len().min(200)]),
                }
            })?;

        Ok(config_response.into())
    }

    /// Compute a hash of the configuration for change detection
    fn compute_config_hash(config: &RouterConfig) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

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
                    error: Some(e.to_string()),
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

                // Java: QueueValidationService — validate consumer connectivity after config sync
                if !self.queue_manager.check_broker_connectivity().await {
                    self.warning_service.add_warning(
                        fc_common::WarningCategory::Configuration,
                        fc_common::WarningSeverity::Warn,
                        "Queue validation: one or more consumers report unhealthy after config sync".to_string(),
                        "ConfigSyncService".to_string(),
                    );
                }

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
    pub async fn initial_sync(&self) -> Result<RouterConfig, ConfigSyncError> {
        info!("Performing initial configuration sync...");

        // Fetch config first
        let config = self.fetch_config().await?;

        // Apply to queue manager
        if let Err(e) = self.queue_manager.reload_config(config.clone()).await {
            let error = ConfigSyncError::Apply(e.to_string());
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
        self.config.enabled && !self.config.config_urls.is_empty()
    }
}

/// Union-merge multiple `RouterConfig`s with first-wins semantics.
/// Mirrors the TS `mergeConfigs` in `multi-config-client.ts`.
///
/// - Pools deduped by `code`; conflicting duplicates (different
///   `concurrency` or `rate_limit_per_minute`) log a warning and the
///   first-seen value wins.
/// - Queues deduped by `uri`; conflicting duplicates (different `name`,
///   `connections`, or `visibility_timeout`) log a warning and the
///   first-seen value wins.
///
/// `sources` is `(source_url, config)` pairs so warnings can name the
/// kept-vs-dropped source.
pub fn merge_configs(sources: &[(String, RouterConfig)]) -> RouterConfig {
    if sources.len() == 1 {
        return sources[0].1.clone();
    }

    use std::collections::HashMap;

    let mut pools: Vec<PoolConfig> = Vec::new();
    let mut pool_origin: HashMap<String, String> = HashMap::new();
    let mut queues: Vec<QueueConfig> = Vec::new();
    let mut queue_origin: HashMap<String, String> = HashMap::new();

    for (source_url, cfg) in sources {
        for pool in &cfg.processing_pools {
            if let Some(existing) = pools.iter().find(|p| p.code == pool.code) {
                if existing.concurrency != pool.concurrency
                    || existing.rate_limit_per_minute != pool.rate_limit_per_minute
                {
                    let kept_source = pool_origin
                        .get(&pool.code)
                        .map(|s| s.as_str())
                        .unwrap_or("(unknown)");
                    warn!(
                        pool_code = %pool.code,
                        kept_source = %kept_source,
                        dropped_source = %source_url,
                        "Duplicate pool with conflicting values — keeping first"
                    );
                }
                continue;
            }
            pool_origin.insert(pool.code.clone(), source_url.clone());
            pools.push(pool.clone());
        }

        for queue in &cfg.queues {
            if let Some(existing) = queues.iter().find(|q| q.uri == queue.uri) {
                if existing.name != queue.name
                    || existing.connections != queue.connections
                    || existing.visibility_timeout != queue.visibility_timeout
                {
                    let kept_source = queue_origin
                        .get(&queue.uri)
                        .map(|s| s.as_str())
                        .unwrap_or("(unknown)");
                    warn!(
                        queue_uri = %queue.uri,
                        kept_source = %kept_source,
                        dropped_source = %source_url,
                        "Duplicate queue with conflicting values — keeping first"
                    );
                }
                continue;
            }
            queue_origin.insert(queue.uri.clone(), source_url.clone());
            queues.push(queue.clone());
        }
    }

    RouterConfig {
        processing_pools: pools,
        queues,
    }
}

/// Spawn the config sync background task.
///
/// **Owns:** the supplied `Arc<ConfigSyncService>` and a [`CancellationToken`]
/// (typically a child of the lifecycle manager's shutdown token).
/// **Exits:** when `shutdown.cancelled()` resolves — level-triggered, so this
/// still exits immediately even if the token was already cancelled before
/// this task started.
/// **Joined by:** the caller via the returned `JoinHandle` (lifecycle
/// manager awaits it on graceful shutdown).
pub fn spawn_config_sync_task(
    config_sync: Arc<ConfigSyncService>,
    shutdown: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    let interval = config_sync.sync_interval();

    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

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
                _ = shutdown.cancelled() => {
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

    fn pool(code: &str, concurrency: u32) -> PoolConfig {
        PoolConfig {
            code: code.to_string(),
            concurrency,
            rate_limit_per_minute: None,
        }
    }

    fn queue(uri: &str, name: &str, connections: u32) -> QueueConfig {
        QueueConfig {
            name: name.to_string(),
            uri: uri.to_string(),
            connections,
            visibility_timeout: 120,
        }
    }

    #[test]
    fn merge_configs_first_wins_on_pool_conflict() {
        let sources = vec![
            (
                "src-a".to_string(),
                RouterConfig {
                    processing_pools: vec![pool("P1", 10), pool("P2", 5)],
                    queues: vec![],
                },
            ),
            (
                "src-b".to_string(),
                RouterConfig {
                    processing_pools: vec![pool("P1", 99), pool("P3", 7)],
                    queues: vec![],
                },
            ),
        ];

        let merged = merge_configs(&sources);
        let p1 = merged
            .processing_pools
            .iter()
            .find(|p| p.code == "P1")
            .unwrap();
        // First-wins: src-a's concurrency (10) survives, src-b's (99) is dropped.
        assert_eq!(p1.concurrency, 10);
        assert_eq!(merged.processing_pools.len(), 3);
    }

    #[test]
    fn merge_configs_dedups_queues_by_uri() {
        let sources = vec![
            (
                "src-a".to_string(),
                RouterConfig {
                    processing_pools: vec![],
                    queues: vec![queue("sqs://q1", "q1", 1), queue("sqs://q2", "q2", 1)],
                },
            ),
            (
                "src-b".to_string(),
                RouterConfig {
                    processing_pools: vec![],
                    // Same uri as src-a's q1, different connections — first wins.
                    queues: vec![queue("sqs://q1", "q1", 5), queue("sqs://q3", "q3", 1)],
                },
            ),
        ];

        let merged = merge_configs(&sources);
        assert_eq!(merged.queues.len(), 3);
        let q1 = merged.queues.iter().find(|q| q.uri == "sqs://q1").unwrap();
        assert_eq!(q1.connections, 1, "first-source connections should win");
    }

    #[test]
    fn merge_configs_single_source_passthrough() {
        let cfg = RouterConfig {
            processing_pools: vec![pool("P1", 10)],
            queues: vec![queue("sqs://q1", "q1", 1)],
        };
        let merged = merge_configs(&[("only".to_string(), cfg.clone())]);
        assert_eq!(merged.processing_pools.len(), 1);
        assert_eq!(merged.queues.len(), 1);
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

    // ========================================================================
    // R-30: last-known-good per source
    // ========================================================================

    fn test_service(config_url: String) -> ConfigSyncService {
        let manager = Arc::new(QueueManager::new(crate::mediator::HttpMediatorConfig::dev()));
        let warning_service = Arc::new(WarningService::noop());
        let mut config = ConfigSyncConfig::new(config_url);
        // Keep the per-URL retry loop fast and short — these tests exercise
        // the fetch_config-level cache/warning logic, not the retry policy.
        config.max_retry_attempts = 1;
        config.retry_delay = Duration::from_millis(1);
        ConfigSyncService::new(config, manager, warning_service)
    }

    fn good_config_body(pool_code: &str) -> serde_json::Value {
        serde_json::json!({
            "processingPools": [{"code": pool_code, "concurrency": 5}],
            "queues": [],
        })
    }

    /// R-30: a source that fails after a prior success keeps contributing
    /// its last-known-good config to the merge (consumers keep running on
    /// it) and raises exactly one CONFIGURATION warning for the failure
    /// streak — not one per failed tick.
    #[tokio::test]
    async fn failing_source_serves_last_known_good_and_warns_once_per_streak() {
        use std::sync::atomic::{AtomicU32, Ordering};
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let calls = Arc::new(AtomicU32::new(0));
        let calls_clone = calls.clone();

        Mock::given(method("GET"))
            .respond_with(move |_req: &wiremock::Request| {
                let call = calls_clone.fetch_add(1, Ordering::SeqCst);
                if call == 0 {
                    // First tick: succeeds.
                    ResponseTemplate::new(200).set_body_json(good_config_body("P1"))
                } else {
                    // Every tick after that fails.
                    ResponseTemplate::new(500)
                }
            })
            .mount(&server)
            .await;

        let service = test_service(server.uri());

        // Tick 1: succeeds, populates the cache.
        let cfg1 = service.fetch_config().await.expect("first fetch succeeds");
        assert_eq!(cfg1.processing_pools[0].code, "P1");
        assert_eq!(service.warning_service.warning_count(), 0);

        // Tick 2: source fails, but the cache still contributes P1 — the
        // merge must not come back empty/erroring.
        let cfg2 = service
            .fetch_config()
            .await
            .expect("a cached source must not fail fetch_config outright");
        assert_eq!(cfg2.processing_pools[0].code, "P1");
        assert_eq!(
            service.warning_service.warning_count(),
            1,
            "first failure in the streak raises a CONFIGURATION warning"
        );

        // Tick 3: still failing — must NOT raise a second warning for the
        // same streak.
        let cfg3 = service.fetch_config().await.expect("still served from cache");
        assert_eq!(cfg3.processing_pools[0].code, "P1");
        assert_eq!(
            service.warning_service.warning_count(),
            1,
            "a still-failing source must not accumulate a warning per tick"
        );
    }

    /// R-30: once a previously-failing source recovers, its active warning
    /// is acknowledged (resolved) rather than left open forever.
    #[tokio::test]
    async fn recovered_source_clears_its_warning() {
        use std::sync::atomic::{AtomicU32, Ordering};
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let calls = Arc::new(AtomicU32::new(0));
        let calls_clone = calls.clone();

        Mock::given(method("GET"))
            .respond_with(move |_req: &wiremock::Request| {
                let call = calls_clone.fetch_add(1, Ordering::SeqCst);
                match call {
                    0 => ResponseTemplate::new(200).set_body_json(good_config_body("P1")),
                    1 => ResponseTemplate::new(500),
                    _ => ResponseTemplate::new(200).set_body_json(good_config_body("P1")),
                }
            })
            .mount(&server)
            .await;

        let service = test_service(server.uri());

        service.fetch_config().await.unwrap(); // tick 1: success
        service.fetch_config().await.unwrap(); // tick 2: fails, served from cache, warns
        assert_eq!(service.warning_service.warning_count(), 1);
        assert_eq!(service.warning_service.get_unacknowledged_warnings().len(), 1);

        service.fetch_config().await.unwrap(); // tick 3: recovers
        assert_eq!(
            service.warning_service.get_unacknowledged_warnings().len(),
            0,
            "the source's warning must be acknowledged once it recovers"
        );
    }

    /// R-30: a source that has never succeeded (no cache yet — first boot)
    /// contributes nothing when it fails; if every configured source is in
    /// that state, `fetch_config` still fails outright rather than
    /// pretending an empty merge is a success.
    #[tokio::test]
    async fn first_boot_all_failing_with_no_cache_is_still_an_error() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let service = test_service(server.uri());
        let result = service.fetch_config().await;
        assert!(
            result.is_err(),
            "first-boot failure with no last-known-good cache must still be an error"
        );
    }
}
