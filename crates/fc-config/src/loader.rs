//! Configuration loader with file and environment variable support

use crate::{AppConfig, ConfigError};
use std::env;
use std::path::PathBuf;
use tracing::info;

/// Standard config file search paths
const CONFIG_PATHS: &[&str] = &[
    "config.toml",
    "application.toml",
    "flowcatalyst.toml",
    "./config/config.toml",
    "./config/application.toml",
    "/etc/flowcatalyst/config.toml",
];

/// Configuration loader
pub struct ConfigLoader {
    config_path: Option<PathBuf>,
}

impl ConfigLoader {
    /// Create a new configuration loader
    pub fn new() -> Self {
        Self { config_path: None }
    }

    /// Create a loader with a specific config file path
    pub fn with_path<P: Into<PathBuf>>(path: P) -> Self {
        Self {
            config_path: Some(path.into()),
        }
    }

    /// Load configuration from file (if found) with environment variable overrides
    pub fn load(&self) -> Result<AppConfig, ConfigError> {
        // Start with defaults
        let mut config = AppConfig::default();

        // Try to load from file
        if let Some(path) = self.find_config_file() {
            info!(?path, "Loading configuration from file");
            config = AppConfig::from_file(&path)?;
        }

        // Apply environment variable overrides
        self.apply_env_overrides(&mut config);

        Ok(config)
    }

    /// Find the configuration file to use
    fn find_config_file(&self) -> Option<PathBuf> {
        // Check explicit path first
        if let Some(path) = &self.config_path {
            if path.exists() {
                return Some(path.clone());
            }
        }

        // Check FLOWCATALYST_CONFIG env var
        if let Ok(path) = env::var("FLOWCATALYST_CONFIG") {
            let path = PathBuf::from(path);
            if path.exists() {
                return Some(path);
            }
        }

        // Search standard paths
        for path in CONFIG_PATHS {
            let path = PathBuf::from(path);
            if path.exists() {
                return Some(path);
            }
        }

        None
    }

    /// Apply environment variable overrides
    fn apply_env_overrides(&self, config: &mut AppConfig) {
        // HTTP
        if let Ok(val) = env::var("FLOWCATALYST_HTTP_PORT") {
            if let Ok(port) = val.parse() {
                config.http.port = port;
            }
        }
        if let Ok(val) = env::var("FLOWCATALYST_HTTP_HOST") {
            config.http.host = val;
        }
        if let Ok(val) = env::var("FLOWCATALYST_CORS_ORIGINS") {
            config.http.cors_origins = val.split(',').map(|s| s.trim().to_string()).collect();
        }

        // MongoDB
        if let Ok(val) = env::var("FLOWCATALYST_MONGODB_URI") {
            config.mongodb.uri = val;
        }
        if let Ok(val) = env::var("FLOWCATALYST_MONGODB_DATABASE") {
            config.mongodb.database = val;
        }

        // Redis
        if let Ok(val) = env::var("FLOWCATALYST_REDIS_URL") {
            config.redis.url = val;
        }
        if let Ok(val) = env::var("FLOWCATALYST_REDIS_POOL_SIZE") {
            if let Ok(size) = val.parse() {
                config.redis.pool_size = size;
            }
        }

        // Queue
        if let Ok(val) = env::var("FLOWCATALYST_QUEUE_TYPE") {
            config.queue.queue_type = val;
        }
        if let Ok(val) = env::var("FLOWCATALYST_NATS_URL") {
            config.queue.nats.url = val;
        }
        if let Ok(val) = env::var("FLOWCATALYST_SQS_QUEUE_URL") {
            config.queue.sqs.queue_url = val;
        }
        if let Ok(val) = env::var("FLOWCATALYST_SQS_REGION") {
            config.queue.sqs.region = val;
        }

        // Router
        if let Ok(val) = env::var("FLOWCATALYST_ROUTER_TIMEOUT_MS") {
            if let Ok(timeout) = val.parse() {
                config.router.timeout_ms = timeout;
            }
        }
        if let Ok(val) = env::var("FLOWCATALYST_ROUTER_MAX_WORKERS") {
            if let Ok(workers) = val.parse() {
                config.router.max_workers_per_pool = workers;
            }
        }
        if let Ok(val) = env::var("FLOWCATALYST_ROUTER_MAX_POOLS") {
            if let Ok(pools) = val.parse() {
                config.router.max_pools = pools;
            }
        }

        // Router Config Sync
        if let Ok(val) = env::var("FLOWCATALYST_CONFIG_SYNC_ENABLED") {
            config.router.config_sync.enabled = val.parse().unwrap_or(false);
        }
        if let Ok(val) = env::var("FLOWCATALYST_CONFIG_SYNC_URL") {
            config.router.config_sync.config_url = val;
        }
        if let Ok(val) = env::var("FLOWCATALYST_CONFIG_SYNC_INTERVAL") {
            if let Ok(interval) = val.parse() {
                config.router.config_sync.interval_seconds = interval;
            }
        }
        if let Ok(val) = env::var("FLOWCATALYST_CONFIG_SYNC_FAIL_ON_ERROR") {
            config.router.config_sync.fail_on_initial_error = val.parse().unwrap_or(true);
        }

        // Router Standby/HA
        if let Ok(val) = env::var("FLOWCATALYST_STANDBY_ENABLED") {
            config.router.standby.enabled = val.parse().unwrap_or(false);
        }
        if let Ok(val) = env::var("FLOWCATALYST_STANDBY_REDIS_URL") {
            config.router.standby.redis_url = val;
        }
        if let Ok(val) = env::var("FLOWCATALYST_STANDBY_LOCK_KEY") {
            config.router.standby.lock_key = val;
        }
        if let Ok(val) = env::var("FLOWCATALYST_STANDBY_LOCK_TTL") {
            if let Ok(ttl) = val.parse() {
                config.router.standby.lock_ttl_seconds = ttl;
            }
        }
        if let Ok(val) = env::var("FLOWCATALYST_STANDBY_HEARTBEAT_INTERVAL") {
            if let Ok(interval) = val.parse() {
                config.router.standby.heartbeat_interval_seconds = interval;
            }
        }

        // Stream
        if let Ok(val) = env::var("FLOWCATALYST_STREAM_BATCH_SIZE") {
            if let Ok(size) = val.parse() {
                config.stream.batch_size = size;
            }
        }
        if let Ok(val) = env::var("FLOWCATALYST_STREAM_CHECKPOINT_STORE") {
            config.stream.checkpoint_store = val;
        }

        // Outbox
        if let Ok(val) = env::var("FLOWCATALYST_OUTBOX_POLL_INTERVAL_MS") {
            if let Ok(interval) = val.parse() {
                config.outbox.poll_interval_ms = interval;
            }
        }
        if let Ok(val) = env::var("FLOWCATALYST_OUTBOX_BATCH_SIZE") {
            if let Ok(size) = val.parse() {
                config.outbox.batch_size = size;
            }
        }

        // Scheduler
        if let Ok(val) = env::var("FLOWCATALYST_SCHEDULER_ENABLED") {
            config.scheduler.enabled = val.parse().unwrap_or(true);
        }
        if let Ok(val) = env::var("FLOWCATALYST_SCHEDULER_POLL_INTERVAL_MS") {
            if let Ok(interval) = val.parse() {
                config.scheduler.poll_interval_ms = interval;
            }
        }
        if let Ok(val) = env::var("FLOWCATALYST_SCHEDULER_DISPATCH_MODE") {
            config.scheduler.default_dispatch_mode = val;
        }

        // Secrets
        if let Ok(val) = env::var("FLOWCATALYST_SECRETS_PROVIDER") {
            config.secrets.provider = val;
        }
        if let Ok(val) = env::var("FLOWCATALYST_SECRETS_ENCRYPTION_KEY") {
            config.secrets.encryption_key = val;
        }
        if let Ok(val) = env::var("FLOWCATALYST_SECRETS_AWS_REGION") {
            config.secrets.aws_region = val;
        }
        if let Ok(val) = env::var("FLOWCATALYST_SECRETS_AWS_PREFIX") {
            config.secrets.aws_prefix = val;
        }
        if let Ok(val) = env::var("FLOWCATALYST_SECRETS_VAULT_ADDR") {
            config.secrets.vault_addr = val;
        }
        if let Ok(val) = env::var("FLOWCATALYST_SECRETS_VAULT_PATH") {
            config.secrets.vault_path = val;
        }
        if let Ok(val) = env::var("FLOWCATALYST_SECRETS_GCP_PROJECT") {
            config.secrets.gcp_project = val;
        }

        // Leader
        if let Ok(val) = env::var("FLOWCATALYST_LEADER_ENABLED") {
            config.leader.enabled = val.parse().unwrap_or(false);
        }
        if let Ok(val) = env::var("FLOWCATALYST_LEADER_INSTANCE_ID") {
            config.leader.instance_id = val;
        }
        if let Ok(val) = env::var("FLOWCATALYST_LEADER_TTL_SECS") {
            if let Ok(ttl) = val.parse() {
                config.leader.ttl_secs = ttl;
            }
        }

        // Auth
        if let Ok(val) = env::var("FLOWCATALYST_AUTH_MODE") {
            config.auth.mode = val;
        }
        if let Ok(val) = env::var("FLOWCATALYST_AUTH_EXTERNAL_BASE") {
            config.auth.external_base = val;
        }
        if let Ok(val) = env::var("FLOWCATALYST_JWT_ISSUER") {
            config.auth.jwt.issuer = val;
        }
        if let Ok(val) = env::var("FLOWCATALYST_JWT_PRIVATE_KEY_PATH") {
            config.auth.jwt.private_key_path = val;
        }
        if let Ok(val) = env::var("FLOWCATALYST_JWT_PUBLIC_KEY_PATH") {
            config.auth.jwt.public_key_path = val;
        }
        if let Ok(val) = env::var("FLOWCATALYST_AUTH_JWKS_URL") {
            config.auth.remote.jwks_url = val;
        }

        // General
        if let Ok(val) = env::var("FLOWCATALYST_DATA_DIR") {
            config.data_dir = val;
        }
        if let Ok(val) = env::var("FLOWCATALYST_DEV_MODE") {
            config.dev_mode = val.parse().unwrap_or(false);
        }
    }
}

impl Default for ConfigLoader {
    fn default() -> Self {
        Self::new()
    }
}
