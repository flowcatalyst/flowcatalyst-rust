# Shared Crates

The FlowCatalyst Rust implementation is built on a foundation of shared library crates that provide common functionality across all services.

## Crate Overview

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           Binaries                                       │
│  fc-dev │ fc-router │ fc-platform-server │ fc-outbox │ fc-stream │ ...  │
└────┬────────┬────────────────┬───────────────┬───────────┬──────────────┘
     │        │                │               │           │
     ▼        ▼                ▼               ▼           ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                         Application Crates                               │
│   fc-api   │   fc-platform   │   fc-router   │   fc-outbox   │ fc-stream│
└────┬───────────┬──────────────────┬──────────────┬──────────────┬───────┘
     │           │                  │              │              │
     ▼           ▼                  ▼              ▼              ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                         Foundation Crates                                │
│   fc-common   │   fc-config   │   fc-queue   │   fc-standby   │fc-secrets│
└─────────────────────────────────────────────────────────────────────────┘
```

## fc-common

**Purpose**: Core types and models shared across all crates.

### Key Types

#### Message

```rust
pub struct Message {
    pub id: String,
    pub pool_code: String,
    pub mediation_type: MediationType,
    pub target: String,
    pub payload: Value,
    pub headers: Option<HashMap<String, String>>,
    pub message_group_id: Option<String>,
    pub deduplication_id: Option<String>,
}

pub enum MediationType {
    Http,
    Sqs,
    Kafka,
}
```

#### QueuedMessage

```rust
pub struct QueuedMessage {
    pub message: Message,
    pub receipt_handle: String,
    pub queue_name: String,
    pub approximate_receive_count: u32,
    pub sent_timestamp: DateTime<Utc>,
}
```

#### Configuration Types

```rust
pub struct PoolConfig {
    pub pool_code: String,
    pub concurrency: u32,
    pub rate_limit: Option<u32>,
    pub rate_limit_burst: Option<u32>,
    pub timeout_seconds: Option<u64>,
}

pub struct QueueConfig {
    pub queue_url: String,
    pub visibility_timeout: u32,
    pub max_messages: u32,
}

pub struct RouterConfig {
    pub pools: Vec<PoolConfig>,
    pub queues: Vec<QueueConfig>,
    pub default_pool: String,
}

pub struct StandbyConfig {
    pub enabled: bool,
    pub redis_url: String,
    pub lock_key: String,
    pub instance_id: String,
    pub lock_ttl_secs: u64,
    pub refresh_interval_secs: u64,
}
```

### Usage

```rust
use fc_common::{Message, MediationType, PoolConfig};
```

### Dependencies

- `serde`: Serialization
- `chrono`: Date/time handling
- `uuid`: ID generation
- `utoipa`: OpenAPI schema generation

---

## fc-config

**Purpose**: TOML-based configuration management.

### Features

- Load from TOML files or environment variables
- Validation and defaults
- Type-safe configuration structures

### Configuration Structure

```rust
pub struct AppConfig {
    pub mongodb: MongoConfig,
    pub http: HttpConfig,
    pub scheduler: Option<SchedulerConfig>,
    pub queue: Option<QueueConfig>,
}

pub struct MongoConfig {
    pub url: String,
    pub database: String,
}

pub struct HttpConfig {
    pub port: u16,
    pub metrics_port: u16,
}

pub struct SchedulerConfig {
    pub poll_interval_ms: u64,
    pub batch_size: u32,
    pub stale_threshold_secs: u64,
}
```

### Usage

```rust
use fc_config::AppConfig;

// From TOML file
let config = AppConfig::from_file("config.toml")?;

// From environment
let config = AppConfig::from_env()?;
```

### Dependencies

- `toml`: TOML parsing
- `serde`: Deserialization
- `anyhow`, `thiserror`: Error handling

---

## fc-queue

**Purpose**: Pluggable queue abstraction with multiple backend implementations.

### Traits

```rust
#[async_trait]
pub trait QueueConsumer: Send + Sync {
    async fn poll(&self, max_messages: u32) -> Result<Vec<QueuedMessage>>;
    async fn ack(&self, receipt_handle: &str) -> Result<()>;
    async fn nack(&self, receipt_handle: &str) -> Result<()>;
    async fn extend_visibility(&self, receipt_handle: &str, seconds: u32) -> Result<()>;
    async fn get_metrics(&self) -> Result<QueueMetrics>;
}

#[async_trait]
pub trait QueuePublisher: Send + Sync {
    async fn publish(&self, message: &Message) -> Result<String>;
    async fn publish_batch(&self, messages: &[Message]) -> Result<Vec<String>>;
}

pub trait EmbeddedQueue: QueueConsumer + QueuePublisher {}
```

### Implementations

#### SQLite (Development)

```rust
use fc_queue::sqlite::SqliteQueue;

let queue = SqliteQueue::new(":memory:").await?;  // In-memory
let queue = SqliteQueue::new("./queue.db").await?;  // File-based
```

Features:
- FIFO ordering with message groups
- In-memory mode for testing
- Persistent mode for development
- Full consumer/publisher interface

#### AWS SQS (Production)

```rust
use fc_queue::sqs::SqsQueue;

let queue = SqsQueue::new(
    "https://sqs.us-east-1.amazonaws.com/123456789/queue.fifo"
).await?;
```

Features:
- FIFO queue support
- Message deduplication
- Visibility timeout management
- Batch operations (up to 10 messages)

#### ActiveMQ (Alternative)

```rust
use fc_queue::activemq::ActiveMqQueue;

let queue = ActiveMqQueue::new("amqp://localhost:5672").await?;
```

Features:
- AMQP 1.0 protocol (via Lapin)
- Durable queues
- Message acknowledgment

### Feature Flags

```toml
[dependencies]
fc-queue = { path = "../fc-queue", features = ["sqlite", "sqs"] }
```

| Feature | Backend |
|---------|---------|
| `sqlite` | SQLite embedded queue |
| `sqs` | AWS SQS |
| `activemq` | ActiveMQ via AMQP |

### Dependencies

- `fc-common`: Message types
- `sqlx`: SQLite/PostgreSQL (optional)
- `aws-sdk-sqs`: AWS SQS (optional)
- `lapin`: AMQP client (optional)

---

## fc-standby

**Purpose**: Redis-based leader election for high availability.

### How It Works

```
┌─────────────────────────────────────────────────────────────────────────┐
│                        Leader Election                                   │
│                                                                          │
│   Instance 1                Instance 2                Instance 3         │
│   ┌─────────┐              ┌─────────┐              ┌─────────┐         │
│   │ LEADER  │              │ STANDBY │              │ STANDBY │         │
│   └────┬────┘              └────┬────┘              └────┬────┘         │
│        │                        │                        │               │
│        │  SET lock NX EX 30     │                        │               │
│        │───────────────────────▶│                        │               │
│        │        OK              │                        │               │
│        │◀───────────────────────│                        │               │
│        │                        │                        │               │
│        │  (refresh every 10s)   │                        │               │
│        │  SET lock XX EX 30     │                        │               │
│        │───────────────────────▶│                        │               │
│                                                                          │
│   If leader dies, lock expires after TTL (30s)                          │
│   Next instance to acquire lock becomes leader                          │
└─────────────────────────────────────────────────────────────────────────┘
```

### Usage

```rust
use fc_standby::{StandbyManager, StandbyConfig};

let config = StandbyConfig {
    enabled: true,
    redis_url: "redis://localhost:6379".to_string(),
    lock_key: "my-service-lock".to_string(),
    instance_id: "instance-1".to_string(),
    lock_ttl_secs: 30,
    refresh_interval_secs: 10,
};

let manager = StandbyManager::new(config).await?;

// Check if this instance is leader
if manager.is_leader().await {
    // Do leader work
}

// Or use callback pattern
manager.run_as_leader(|| async {
    // This only runs when leader
}).await;
```

### Configuration

| Parameter | Description | Default |
|-----------|-------------|---------|
| `redis_url` | Redis connection URL | required |
| `lock_key` | Unique lock identifier | required |
| `instance_id` | This instance's identifier | required |
| `lock_ttl_secs` | Lock expiration time | 30 |
| `refresh_interval_secs` | Lock refresh interval | 10 |

### Dependencies

- `redis`: Redis client
- `tokio`: Async runtime

---

## fc-secrets

**Purpose**: Multi-backend secrets management.

### Supported Backends

| Backend | Feature Flag | Description |
|---------|--------------|-------------|
| AWS Secrets Manager | `aws` | Production secrets |
| AWS Parameter Store | `aws-ssm` | Configuration parameters |
| HashiCorp Vault | `vault` | Self-hosted secrets |

### Usage

```rust
use fc_secrets::{SecretsManager, AwsSecretsBackend};

// AWS Secrets Manager
let secrets = SecretsManager::new(AwsSecretsBackend::new().await?);
let db_password = secrets.get("database/password").await?;

// AWS Parameter Store
use fc_secrets::SsmBackend;
let secrets = SecretsManager::new(SsmBackend::new().await?);

// HashiCorp Vault
use fc_secrets::VaultBackend;
let secrets = SecretsManager::new(VaultBackend::new(
    "https://vault.example.com",
    "my-token"
).await?);
```

### Environment Variable Integration

```rust
// Load secrets into environment variables
secrets.load_into_env(&[
    ("database/password", "DB_PASSWORD"),
    ("api/key", "API_KEY"),
]).await?;
```

### Encryption Support

```rust
// Client-side encryption with AES-GCM
use fc_secrets::encryption::AesGcmEncryptor;

let encryptor = AesGcmEncryptor::new(key);
let encrypted = encryptor.encrypt(plaintext)?;
let decrypted = encryptor.decrypt(&encrypted)?;
```

### Feature Flags

```toml
[dependencies]
fc-secrets = { path = "../fc-secrets", features = ["aws", "vault"] }
```

### Dependencies

- `aws-sdk-secretsmanager`: AWS Secrets Manager (optional)
- `aws-sdk-ssm`: AWS Parameter Store (optional)
- `reqwest`: Vault HTTP client (optional)
- `aes-gcm`: Encryption

---

## fc-api

**Purpose**: HTTP API layer built on Axum.

### Components

- REST endpoint handlers
- OpenAPI/Swagger documentation
- Authentication middleware
- CORS configuration
- Prometheus metrics export

### Router Building

```rust
use fc_api::{build_router, ApiState};

let state = ApiState {
    queue_manager: Arc::new(queue_manager),
    platform_db: Arc::new(platform_db),
    // ...
};

let router = build_router(state);
```

### Standard Endpoints

All services include:

| Endpoint | Description |
|----------|-------------|
| `/q/live` | Kubernetes liveness probe |
| `/q/ready` | Kubernetes readiness probe |
| `/health` | Detailed health status |
| `/metrics` | Prometheus metrics |
| `/swagger-ui` | OpenAPI documentation |

### Dependencies

- `axum`: Web framework
- `tower`: Middleware
- `utoipa`: OpenAPI generation
- `utoipa-swagger-ui`: Swagger UI
- `prometheus`: Metrics

---

## fc-scheduler

**Purpose**: Job scheduling engine.

### Core Functionality

- Poll pending dispatch jobs
- Build messages from jobs
- Publish to queue
- Update job status
- Stale job recovery

### Usage

```rust
use fc_scheduler::{Scheduler, SchedulerConfig};

let config = SchedulerConfig {
    poll_interval_ms: 1000,
    batch_size: 100,
    stale_threshold_secs: 300,
};

let scheduler = Scheduler::new(
    config,
    job_repository,
    event_repository,
    queue_publisher,
).await?;

scheduler.run().await?;
```

### Dependencies

- `fc-common`: Message types
- `fc-config`: Configuration
- `fc-queue`: Queue publisher

---

## Dependency Graph

```
fc-common (no internal deps)
    │
    ├── fc-config
    │
    ├── fc-queue
    │       │
    │       └── fc-router
    │               │
    │               └── fc-api
    │
    ├── fc-standby
    │       │
    │       ├── fc-outbox
    │       ├── fc-stream
    │       └── fc-scheduler
    │
    ├── fc-platform
    │       │
    │       └── fc-api
    │
    └── fc-secrets (standalone)
```

## Building Individual Crates

```bash
# Build specific crate
cargo build -p fc-common
cargo build -p fc-queue --features sqlite,sqs
cargo build -p fc-secrets --features aws

# Run crate tests
cargo test -p fc-common
cargo test -p fc-queue --features sqlite

# Build with all features
cargo build -p fc-queue --all-features
```

## Feature Matrix

| Crate | Required Features | Optional Features |
|-------|-------------------|-------------------|
| fc-common | - | - |
| fc-config | - | - |
| fc-queue | - | `sqlite`, `sqs`, `activemq` |
| fc-standby | - | - |
| fc-secrets | - | `aws`, `aws-ssm`, `vault` |
| fc-outbox | - | `sqlite`, `postgres`, `mongo` |
| fc-router | - | - |
| fc-api | - | - |
| fc-platform | - | - |
| fc-stream | - | - |
| fc-scheduler | - | - |
