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
            sync_interval: Duration::from_secs(300), // 5 minutes
            max_retry_attempts: 12,                  // 12 attempts
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

/// Response from the configuration service (`MessageRouterConfig`).
///
/// Parsed as Go's `common.RouterConfig` is: an absent or `null`
/// `processingPools` / `queues` is an empty list, not a parse error.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageRouterConfigResponse {
    #[serde(default, deserialize_with = "null_as_empty")]
    pub processing_pools: Vec<PoolConfigResponse>,
    #[serde(default, deserialize_with = "null_as_empty")]
    pub queues: Vec<QueueConfigResponse>,
}

/// `null` reads as an empty list (Go unmarshals `null` into a nil slice).
fn null_as_empty<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolConfigResponse {
    pub code: String,
    /// Absent reads as 0, as Go's `uint32` does; the pool then derives its
    /// effective concurrency (see `ProcessPool::new`).
    #[serde(default)]
    pub concurrency: usize,
    #[serde(default)]
    pub rate_limit_per_minute: Option<u32>,
}

/// One queue of the document. Deserialized as Go's
/// `common.QueueConfig.UnmarshalJSON`: `queueUri` (legacy `uri`) and
/// `queueName` (legacy `name`, else the URI); `connections` and
/// `visibilityTimeout` are left `None` — meaning "unstated", so the
/// defaults of 1 and 120 apply — when absent, `null` **or zero**. A
/// producer that writes the full struct writes 0 for "no opinion", and a
/// consumer at 0 connections / a 0-second visibility timeout is never what
/// it meant.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", from = "RawQueueConfig")]
pub struct QueueConfigResponse {
    pub queue_name: Option<String>,
    pub queue_uri: String,
    pub connections: Option<u32>,
    pub visibility_timeout: Option<u32>,
}

/// The wire keys Go accepts for a queue, before its defaults are applied.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawQueueConfig {
    queue_name: Option<String>,
    queue_uri: Option<String>,
    name: Option<String>,
    uri: Option<String>,
    connections: Option<u32>,
    visibility_timeout: Option<u32>,
}

impl From<RawQueueConfig> for QueueConfigResponse {
    fn from(raw: RawQueueConfig) -> Self {
        Self {
            queue_name: raw.queue_name.or(raw.name),
            queue_uri: raw.queue_uri.or(raw.uri).unwrap_or_default(),
            connections: raw.connections.filter(|&c| c > 0),
            visibility_timeout: raw.visibility_timeout.filter(|&v| v > 0),
        }
    }
}

/// Go's defaults for an unstated `connections` / `visibilityTimeout`.
const DEFAULT_QUEUE_CONNECTIONS: u32 = 1;
const DEFAULT_VISIBILITY_TIMEOUT_SECS: u32 = 120;

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
                    connections: q
                        .connections
                        .filter(|&c| c > 0)
                        .unwrap_or(DEFAULT_QUEUE_CONNECTIONS),
                    visibility_timeout: q
                        .visibility_timeout
                        .filter(|&v| v > 0)
                        .unwrap_or(DEFAULT_VISIBILITY_TIMEOUT_SECS),
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
    /// The active "config sync failed" warning, if any — one per failure
    /// streak (Go: `watchWarnID`).
    watch_warning: parking_lot::Mutex<Option<String>>,
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
            watch_warning: parking_lot::Mutex::new(None),
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
        let response =
            self.http_client
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

        // Hash queues — every field, as Go's change detection compares the
        // whole marshalled config. A visibility-timeout change used to hash
        // the same, so it was never applied.
        for queue in &config.queues {
            queue.name.hash(&mut hasher);
            queue.uri.hash(&mut hasher);
            queue.connections.hash(&mut hasher);
            queue.visibility_timeout.hash(&mut hasher);
        }

        hasher.finish()
    }

    /// Fetch the config and apply it if it changed (Go: one `apply` of
    /// `Watch`, and `Server.Reload`). `Ok(true)` when a changed config was
    /// applied, `Ok(false)` when it matched the last applied one.
    ///
    /// A config the manager fails to apply (a consumer could not be built,
    /// the manager is shutting down) is NOT recorded as applied — the change
    /// baseline is forgotten, as Go's `forgetLast` does — so the next sync
    /// applies it again instead of reporting "unchanged" for ever.
    pub async fn apply_latest(&self) -> Result<bool, ConfigSyncError> {
        let new_config = self.fetch_config().await?;
        let new_hash = Self::compute_config_hash(&new_config);
        if *self.last_config_hash.lock() == Some(new_hash) {
            debug!("Configuration unchanged, skipping reload");
            return Ok(false);
        }

        info!(
            pools = new_config.processing_pools.len(),
            queues = new_config.queues.len(),
            "Configuration changed, applying updates"
        );
        match self.queue_manager.reload_config(new_config).await {
            Ok(true) => {
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
                Ok(true)
            }
            Ok(false) => {
                *self.last_config_hash.lock() = None;
                Err(ConfigSyncError::Apply(
                    "the queue manager is shutting down".to_string(),
                ))
            }
            Err(e) => {
                *self.last_config_hash.lock() = None;
                Err(ConfigSyncError::Apply(e.to_string()))
            }
        }
    }

    /// Sync configuration - fetch and apply if changed. On failure the
    /// existing configuration keeps running and one CONFIGURATION warning
    /// is raised per failure streak (Go: `raiseWatchWarning`), resolved on
    /// the next successful sync — not one warning per tick.
    pub async fn sync(&self) -> ConfigSyncResult {
        match self.apply_latest().await {
            Ok(_) => {
                self.clear_watch_warning();
                ConfigSyncResult {
                    success: true,
                    pools_updated: 0,
                    pools_created: 0,
                    pools_removed: 0,
                    error: None,
                }
            }
            Err(e) => {
                error!(error = %e, "Configuration sync failed; the current configuration keeps running");
                self.raise_watch_warning(format!("Config sync failed: {}", e));
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

    fn raise_watch_warning(&self, message: String) {
        let mut id = self.watch_warning.lock();
        if id.is_none() {
            *id = Some(self.warning_service.add_warning(
                WarningCategory::Configuration,
                WarningSeverity::Warn,
                message,
                "ConfigSyncService".to_string(),
            ));
        }
    }

    fn clear_watch_warning(&self) {
        if let Some(id) = self.watch_warning.lock().take() {
            self.warning_service.acknowledge_warning(&id);
        }
    }

    /// Perform one initial fetch-and-apply. Kept for callers that manage
    /// their own retry; the router binary uses [`Self::run`], which retries
    /// until a configuration lands.
    pub async fn initial_sync(&self) -> Result<RouterConfig, ConfigSyncError> {
        info!("Performing initial configuration sync...");
        let config = self.fetch_config().await?;
        if let Err(e) = self.queue_manager.reload_config(config.clone()).await {
            let error = ConfigSyncError::Apply(e.to_string());
            if self.config.fail_on_initial_sync_error {
                return Err(error);
            } else {
                warn!("{}", error);
            }
        }
        *self.last_config_hash.lock() = Some(Self::compute_config_hash(&config));
        info!(
            pools = config.processing_pools.len(),
            queues = config.queues.len(),
            "Initial configuration sync completed successfully"
        );
        Ok(config)
    }

    /// Go's `Watch`: apply the configuration, retrying at the retry cadence
    /// (`retry_delay`, 5s) until one lands — however long the config source
    /// is down at boot — and only then poll every `sync_interval`. A router
    /// that boots while its platform is down therefore keeps running (HTTP
    /// up, health answering) and picks the config up the moment the source
    /// recovers, instead of exiting after one fetch's retry budget. Returns
    /// when `shutdown` is cancelled.
    pub async fn run(self: Arc<Self>, shutdown: CancellationToken) {
        let retry = if self.config.retry_delay.is_zero() {
            Duration::from_secs(5)
        } else {
            self.config.retry_delay
        };
        let mut failures = 0u32;
        loop {
            match self.apply_latest().await {
                Ok(_) => {
                    self.clear_watch_warning();
                    info!(failed_attempts = failures, "Router configuration applied");
                    break;
                }
                Err(e) => {
                    if failures == 0 {
                        warn!(error = %e, "Initial configuration not applied yet; retrying");
                    }
                    failures += 1;
                    self.raise_watch_warning(format!("Config sync failed: {}", e));
                }
            }
            tokio::select! {
                _ = shutdown.cancelled() => return,
                _ = tokio::time::sleep(retry) => {}
            }
        }

        let mut ticker = tokio::time::interval(self.config.sync_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        ticker.tick().await; // the first tick fires immediately
        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    let result = self.sync().await;
                    if !result.success {
                        warn!(error = ?result.error, "Scheduled config sync failed - continuing with existing config");
                    }
                }
                _ = shutdown.cancelled() => {
                    info!("Config sync task shutting down");
                    return;
                }
            }
        }
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

    /// A queue's visibility timeout is part of the change detection: a
    /// config that changes only that must be re-applied (the consumer is
    /// rebuilt with the new timeout).
    #[test]
    fn config_hash_covers_visibility_timeout() {
        let q = |vt| RouterConfig {
            processing_pools: vec![],
            queues: vec![QueueConfig {
                name: "q".to_string(),
                uri: "https://sqs/q".to_string(),
                connections: 1,
                visibility_timeout: vt,
            }],
        };
        assert_ne!(
            ConfigSyncService::compute_config_hash(&q(30)),
            ConfigSyncService::compute_config_hash(&q(120))
        );
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

    fn parse(doc: serde_json::Value) -> RouterConfig {
        serde_json::from_value::<MessageRouterConfigResponse>(doc)
            .expect("document parses")
            .into()
    }

    /// Go `QueueConfig.UnmarshalJSON`: an explicit 0 means "unstated",
    /// exactly as an absent key or `null` does.
    #[test]
    fn zero_absent_and_null_connections_and_visibility_take_gos_defaults() {
        let cfg = parse(serde_json::json!({
            "processingPools": [],
            "queues": [
                {"queueName": "zero", "queueUri": "u0", "connections": 0, "visibilityTimeout": 0},
                {"queueName": "absent", "queueUri": "u1"},
                {"queueName": "null", "queueUri": "u2", "connections": null, "visibilityTimeout": null},
                {"queueName": "set", "queueUri": "u3", "connections": 3, "visibilityTimeout": 45},
            ],
        }));
        let got: Vec<(u32, u32)> = cfg
            .queues
            .iter()
            .map(|q| (q.connections, q.visibility_timeout))
            .collect();
        assert_eq!(got, vec![(1, 120), (1, 120), (1, 120), (3, 45)]);
    }

    /// Go accepts the legacy `{name, uri}` keys, prefers the canonical ones,
    /// and names a queue by its URI when no name is given.
    #[test]
    fn queue_keys_and_name_fallback_follow_go() {
        let cfg = parse(serde_json::json!({
            "queues": [
                {"name": "legacy", "uri": "legacy-uri"},
                {"queueUri": "only-uri"},
                {"queueName": "canon", "name": "ignored", "queueUri": "canon-uri", "uri": "ignored-uri"},
            ],
        }));
        let got: Vec<(&str, &str)> = cfg
            .queues
            .iter()
            .map(|q| (q.name.as_str(), q.uri.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("legacy", "legacy-uri"),
                ("only-uri", "only-uri"),
                ("canon", "canon-uri")
            ]
        );
        assert!(cfg.processing_pools.is_empty(), "absent pools read as none");
    }

    /// `null` lists and an absent pool concurrency parse (Go: nil slice, 0).
    #[test]
    fn null_lists_and_absent_concurrency_parse() {
        let cfg = parse(serde_json::json!({
            "processingPools": [{"code": "P"}],
            "queues": null,
        }));
        assert_eq!(cfg.processing_pools[0].concurrency, 0);
        assert!(cfg.queues.is_empty());
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
        let cfg3 = service
            .fetch_config()
            .await
            .expect("still served from cache");
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
        assert_eq!(
            service.warning_service.get_unacknowledged_warnings().len(),
            1
        );

        service.fetch_config().await.unwrap(); // tick 3: recovers
        assert_eq!(
            service.warning_service.get_unacknowledged_warnings().len(),
            0,
            "the source's warning must be acknowledged once it recovers"
        );
    }

    /// H14 (Go `Watch`): a config source that is down at boot is retried at
    /// the retry cadence until a configuration lands — the router keeps
    /// running meanwhile — rather than failing after one fetch's retry
    /// budget (which used to exit the process).
    #[tokio::test]
    async fn run_retries_at_boot_until_the_config_lands() {
        use std::sync::atomic::{AtomicU32, Ordering};
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let calls = Arc::new(AtomicU32::new(0));
        let calls_clone = calls.clone();
        Mock::given(method("GET"))
            .respond_with(move |_req: &wiremock::Request| {
                if calls_clone.fetch_add(1, Ordering::SeqCst) < 3 {
                    ResponseTemplate::new(503)
                } else {
                    ResponseTemplate::new(200).set_body_json(good_config_body("BOOT"))
                }
            })
            .mount(&server)
            .await;

        let mut service = test_service(server.uri());
        service.warning_service = Arc::new(WarningService::default());
        let service = Arc::new(service);
        let manager = service.queue_manager.clone();
        let token = CancellationToken::new();
        let task = tokio::spawn(service.clone().run(token.clone()));

        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while manager.get_pool("BOOT").is_none() {
            assert!(
                std::time::Instant::now() < deadline,
                "the config must be applied once the source recovers"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(calls.load(Ordering::SeqCst) >= 4);
        assert_eq!(
            service.warning_service.warning_count(),
            1,
            "one warning per streak"
        );
        assert_eq!(
            service.warning_service.get_unacknowledged_warnings().len(),
            0,
            "the failure-streak warning is resolved once the config lands"
        );
        token.cancel();
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .expect("run() exits on cancel")
            .unwrap();
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
