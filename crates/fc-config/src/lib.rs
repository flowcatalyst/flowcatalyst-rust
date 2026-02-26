//! FlowCatalyst Configuration System
//!
//! This crate provides TOML-based configuration with environment variable override support.

use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

mod loader;

pub use loader::ConfigLoader;

/// Configuration error types
#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Failed to read config file: {0}")]
    ReadError(#[from] std::io::Error),

    #[error("Failed to parse TOML: {0}")]
    ParseError(#[from] toml::de::Error),

    #[error("Invalid configuration: {0}")]
    ValidationError(String),

    #[error("Environment variable error: {0}")]
    EnvError(String),
}

/// Root application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub http: HttpConfig,
    pub mongodb: MongoConfig,
    pub redis: RedisConfig,
    pub queue: QueueConfig,
    pub router: RouterConfig,
    pub stream: StreamConfig,
    pub outbox: OutboxConfig,
    pub scheduler: SchedulerConfig,
    pub secrets: SecretsConfig,
    pub leader: LeaderConfig,
    pub auth: AuthConfig,

    /// Data directory for local storage
    pub data_dir: String,

    /// Enable development mode
    pub dev_mode: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            http: HttpConfig::default(),
            mongodb: MongoConfig::default(),
            redis: RedisConfig::default(),
            queue: QueueConfig::default(),
            router: RouterConfig::default(),
            stream: StreamConfig::default(),
            outbox: OutboxConfig::default(),
            scheduler: SchedulerConfig::default(),
            secrets: SecretsConfig::default(),
            leader: LeaderConfig::default(),
            auth: AuthConfig::default(),
            data_dir: "./data".to_string(),
            dev_mode: false,
        }
    }
}

/// HTTP server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HttpConfig {
    pub port: u16,
    pub host: String,
    pub cors_origins: Vec<String>,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            port: 8080,
            host: "0.0.0.0".to_string(),
            cors_origins: vec!["http://localhost:4200".to_string()],
        }
    }
}

/// MongoDB configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MongoConfig {
    pub uri: String,
    pub database: String,
}

impl Default for MongoConfig {
    fn default() -> Self {
        Self {
            uri: "mongodb://localhost:27017/?replicaSet=rs0&directConnection=true".to_string(),
            database: "flowcatalyst".to_string(),
        }
    }
}

/// Redis configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RedisConfig {
    pub url: String,
    pub pool_size: usize,
}

impl Default for RedisConfig {
    fn default() -> Self {
        Self {
            url: "redis://localhost:6379".to_string(),
            pool_size: 10,
        }
    }
}

/// Queue configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct QueueConfig {
    #[serde(rename = "type")]
    pub queue_type: String,
    pub nats: NatsConfig,
    pub sqs: SqsConfig,
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            queue_type: "embedded".to_string(),
            nats: NatsConfig::default(),
            sqs: SqsConfig::default(),
        }
    }
}

/// NATS configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NatsConfig {
    pub url: String,
    pub data_dir: String,
}

impl Default for NatsConfig {
    fn default() -> Self {
        Self {
            url: "nats://localhost:4222".to_string(),
            data_dir: "./data/nats".to_string(),
        }
    }
}

/// AWS SQS configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SqsConfig {
    pub queue_url: String,
    pub region: String,
    pub wait_time_seconds: u32,
    pub visibility_timeout: u32,
}

impl Default for SqsConfig {
    fn default() -> Self {
        Self {
            queue_url: String::new(),
            region: "us-east-1".to_string(),
            wait_time_seconds: 20,
            visibility_timeout: 120,
        }
    }
}

/// Message router configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RouterConfig {
    /// HTTP client timeout in milliseconds
    pub timeout_ms: u64,
    /// Maximum connections per host
    pub max_connections_per_host: usize,
    /// Maximum concurrent workers per pool
    pub max_workers_per_pool: usize,
    /// Maximum total pools
    pub max_pools: usize,
    /// Enable circuit breaker
    pub circuit_breaker_enabled: bool,
    /// Circuit breaker failure rate threshold (0.0-1.0). Default: 0.5
    pub circuit_breaker_failure_rate: f64,
    /// Minimum calls before evaluating failure rate. Default: 10
    pub circuit_breaker_min_calls: u32,
    /// Circuit breaker reset timeout in seconds. Default: 5
    pub circuit_breaker_reset_secs: u64,
    /// Configuration sync settings
    pub config_sync: ConfigSyncSettings,
    /// Standby/HA settings
    pub standby: StandbySettings,
}

/// Configuration sync settings for dynamic config updates
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ConfigSyncSettings {
    /// Enable configuration sync from remote service
    pub enabled: bool,
    /// URL to fetch configuration from
    pub config_url: String,
    /// Sync interval in seconds (default: 300 = 5 minutes)
    pub interval_seconds: u64,
    /// Maximum retry attempts on failure
    pub max_retry_attempts: u32,
    /// Delay between retries in seconds
    pub retry_delay_seconds: u64,
    /// HTTP request timeout in seconds
    pub request_timeout_seconds: u64,
    /// Fail startup if initial sync fails
    pub fail_on_initial_error: bool,
}

impl Default for ConfigSyncSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            config_url: String::new(),
            interval_seconds: 300, // 5 minutes (matches Java)
            max_retry_attempts: 12,
            retry_delay_seconds: 5,
            request_timeout_seconds: 30,
            fail_on_initial_error: true,
        }
    }
}

/// Standby/High Availability settings
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StandbySettings {
    /// Enable active/standby mode (requires Redis)
    pub enabled: bool,
    /// Redis URL for leader election (uses main redis.url if empty)
    pub redis_url: String,
    /// Lock key for leader election
    pub lock_key: String,
    /// Lock TTL in seconds
    pub lock_ttl_seconds: u64,
    /// Heartbeat interval in seconds
    pub heartbeat_interval_seconds: u64,
}

impl Default for StandbySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            redis_url: String::new(),
            lock_key: "fc:router:leader".to_string(),
            lock_ttl_seconds: 30,
            heartbeat_interval_seconds: 10,
        }
    }
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            timeout_ms: 30000,
            max_connections_per_host: 100,
            max_workers_per_pool: 10,
            max_pools: 100,
            circuit_breaker_enabled: true,
            circuit_breaker_failure_rate: 0.5,
            circuit_breaker_min_calls: 10,
            circuit_breaker_reset_secs: 5,
            config_sync: ConfigSyncSettings::default(),
            standby: StandbySettings::default(),
        }
    }
}

/// Stream processor configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StreamConfig {
    /// Batch size for stream processing
    pub batch_size: usize,
    /// Batch wait timeout in milliseconds
    pub batch_wait_ms: u64,
    /// Checkpoint store type: mongodb, redis, memory
    pub checkpoint_store: String,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            batch_size: 100,
            batch_wait_ms: 1000,
            checkpoint_store: "mongodb".to_string(),
        }
    }
}

/// Outbox processor configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OutboxConfig {
    /// Polling interval in milliseconds
    pub poll_interval_ms: u64,
    /// Batch size per poll
    pub batch_size: usize,
    /// Enable database-specific outbox processors
    pub enabled_databases: Vec<String>,
}

impl Default for OutboxConfig {
    fn default() -> Self {
        Self {
            poll_interval_ms: 100,
            batch_size: 50,
            enabled_databases: vec!["mongodb".to_string()],
        }
    }
}

/// Dispatch scheduler configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SchedulerConfig {
    /// Enable the dispatch scheduler
    pub enabled: bool,
    /// Polling interval in milliseconds
    pub poll_interval_ms: u64,
    /// Batch size per poll
    pub batch_size: usize,
    /// Stale job threshold in minutes
    pub stale_threshold_minutes: u64,
    /// Default dispatch mode: immediate, next_on_error, block_on_error
    pub default_dispatch_mode: String,
    /// App key for HMAC auth token generation
    pub app_key: String,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            poll_interval_ms: 100,
            batch_size: 100,
            stale_threshold_minutes: 15,
            default_dispatch_mode: "immediate".to_string(),
            app_key: String::new(),
        }
    }
}

/// Secrets provider configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SecretsConfig {
    /// Provider type: env, encrypted, aws-sm, vault, gcp-sm
    pub provider: String,
    /// Encryption key for encrypted provider (base64)
    pub encryption_key: String,
    /// Data directory for encrypted provider
    pub data_dir: String,

    // AWS Secrets Manager
    pub aws_region: String,
    pub aws_prefix: String,
    pub aws_endpoint: String,

    // HashiCorp Vault
    pub vault_addr: String,
    pub vault_path: String,
    pub vault_namespace: String,

    // GCP Secret Manager
    pub gcp_project: String,
    pub gcp_prefix: String,
}

impl Default for SecretsConfig {
    fn default() -> Self {
        Self {
            provider: "env".to_string(),
            encryption_key: String::new(),
            data_dir: "./data/secrets".to_string(),
            aws_region: String::new(),
            aws_prefix: "/flowcatalyst/".to_string(),
            aws_endpoint: String::new(),
            vault_addr: String::new(),
            vault_path: "secret/data/flowcatalyst".to_string(),
            vault_namespace: String::new(),
            gcp_project: String::new(),
            gcp_prefix: "flowcatalyst-".to_string(),
        }
    }
}

/// Leader election configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LeaderConfig {
    pub enabled: bool,
    pub instance_id: String,
    pub ttl_secs: u64,
    pub refresh_interval_secs: u64,
}

impl Default for LeaderConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            instance_id: String::new(),
            ttl_secs: 30,
            refresh_interval_secs: 10,
        }
    }
}

/// Authentication configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AuthConfig {
    /// Auth mode: embedded, remote
    pub mode: String,
    /// External base URL for OAuth callbacks
    pub external_base: String,
    pub jwt: JwtConfig,
    pub session: SessionConfig,
    pub pkce: PkceConfig,
    pub remote: RemoteAuthConfig,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            mode: "embedded".to_string(),
            external_base: "http://localhost:4200".to_string(),
            jwt: JwtConfig::default(),
            session: SessionConfig::default(),
            pkce: PkceConfig::default(),
            remote: RemoteAuthConfig::default(),
        }
    }
}

/// JWT configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct JwtConfig {
    pub issuer: String,
    pub private_key_path: String,
    pub public_key_path: String,
    pub access_token_expiry_secs: u64,
    pub session_token_expiry_secs: u64,
    pub refresh_token_expiry_secs: u64,
    pub authorization_code_expiry_secs: u64,
}

impl Default for JwtConfig {
    fn default() -> Self {
        Self {
            issuer: "flowcatalyst".to_string(),
            private_key_path: String::new(),
            public_key_path: String::new(),
            access_token_expiry_secs: 3600,      // 1 hour
            session_token_expiry_secs: 28800,    // 8 hours
            refresh_token_expiry_secs: 2592000,  // 30 days
            authorization_code_expiry_secs: 600, // 10 minutes
        }
    }
}

/// Session configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionConfig {
    pub cookie_name: String,
    pub secure: bool,
    pub same_site: String,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            cookie_name: "FLOWCATALYST_SESSION".to_string(),
            secure: true,
            same_site: "Strict".to_string(),
        }
    }
}

/// PKCE configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PkceConfig {
    pub required: bool,
}

impl Default for PkceConfig {
    fn default() -> Self {
        Self { required: true }
    }
}

/// Remote authentication configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RemoteAuthConfig {
    pub jwks_url: String,
    pub issuer: String,
}

impl Default for RemoteAuthConfig {
    fn default() -> Self {
        Self {
            jwks_url: String::new(),
            issuer: String::new(),
        }
    }
}

impl AppConfig {
    /// Load configuration from a TOML file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)?;
        let config: AppConfig = toml::from_str(&content)?;
        Ok(config)
    }

    /// Load configuration with environment variable override
    pub fn load() -> Result<Self, ConfigError> {
        let loader = ConfigLoader::new();
        loader.load()
    }

    /// Generate an example TOML configuration
    pub fn example_toml() -> String {
        r#"# FlowCatalyst Configuration
# Environment variables override these settings

[http]
port = 8080
host = "0.0.0.0"
cors_origins = ["http://localhost:4200"]

[mongodb]
uri = "mongodb://localhost:27017/?replicaSet=rs0&directConnection=true"
database = "flowcatalyst"

[redis]
url = "redis://localhost:6379"
pool_size = 10

[queue]
type = "embedded"  # embedded, nats, or sqs

[queue.nats]
url = "nats://localhost:4222"
data_dir = "./data/nats"

[queue.sqs]
queue_url = ""
region = "us-east-1"
wait_time_seconds = 20
visibility_timeout = 120

[router]
timeout_ms = 30000
max_connections_per_host = 100
max_workers_per_pool = 10
max_pools = 100
circuit_breaker_enabled = true
circuit_breaker_failure_rate = 0.5
circuit_breaker_min_calls = 10
circuit_breaker_reset_secs = 5

[stream]
batch_size = 100
batch_wait_ms = 1000
checkpoint_store = "mongodb"  # mongodb, redis, memory

[outbox]
poll_interval_ms = 100
batch_size = 50
enabled_databases = ["mongodb"]

[scheduler]
enabled = true
poll_interval_ms = 100
batch_size = 100
stale_threshold_minutes = 15
default_dispatch_mode = "immediate"  # immediate, next_on_error, block_on_error

[secrets]
provider = "env"  # env, encrypted, aws-sm, vault, gcp-sm
encryption_key = ""
data_dir = "./data/secrets"

# AWS Secrets Manager
aws_region = ""
aws_prefix = "/flowcatalyst/"
aws_endpoint = ""

# HashiCorp Vault
vault_addr = ""
vault_path = "secret/data/flowcatalyst"
vault_namespace = ""

# GCP Secret Manager
gcp_project = ""
gcp_prefix = "flowcatalyst-"

[leader]
enabled = false
instance_id = ""
ttl_secs = 30
refresh_interval_secs = 10

[auth]
mode = "embedded"
external_base = "http://localhost:4200"

[auth.jwt]
issuer = "flowcatalyst"
private_key_path = ""
public_key_path = ""
access_token_expiry_secs = 3600
session_token_expiry_secs = 28800
refresh_token_expiry_secs = 2592000
authorization_code_expiry_secs = 600

[auth.session]
cookie_name = "FLOWCATALYST_SESSION"
secure = true
same_site = "Strict"

[auth.pkce]
required = true

[auth.remote]
jwks_url = ""
issuer = ""

data_dir = "./data"
dev_mode = false
"#
        .to_string()
    }
}
