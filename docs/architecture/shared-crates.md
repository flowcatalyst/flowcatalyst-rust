# Shared Crates

FlowCatalyst is a workspace of focused crates. This document maps what each one is for and what the cross-crate dependency graph looks like. Source: top-level `Cargo.toml`, individual crate `lib.rs` files.

```
                     ┌─────────────────────────────┐
                     │    Binaries (bin/*)         │
                     │  fc-server, fc-dev,         │
                     │  fc-router, fc-platform-srv,│
                     │  fc-stream-processor,       │
                     │  fc-outbox-processor,       │
                     │  fc-mcp-server              │
                     └──────────────┬──────────────┘
                                    │ uses
   ┌────────────────────────────────┼────────────────────────────────┐
   │                                │                                │
   ▼                                ▼                                ▼
┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐
│fc-platform│  │fc-router │   │fc-stream │   │fc-outbox │   │fc-mcp    │
│(40+ KLoC) │  │          │   │          │   │          │   │          │
└─────┬────┘   └─────┬────┘   └─────┬────┘   └─────┬────┘   └─────┬────┘
      │              │              │              │              │
      │              │              │              │              │
      └──────┬───────┴──────┬───────┴──────┬───────┴──────────────┘
             │              │              │
             ▼              ▼              ▼
       ┌──────────┐   ┌──────────┐   ┌──────────┐
       │fc-common │   │fc-queue  │   │fc-standby│
       └──────────┘   └──────────┘   └──────────┘

       fc-sdk: standalone — for consumer apps, depends on fc-common only.
```

No internal crate depends "upward" — `fc-common` never imports `fc-platform`. Workspace-wide rule.

---

## fc-common — shared types

`crates/fc-common/src/lib.rs`. The lowest layer. Anything used across more than one crate goes here.

Key types:

```rust
// The unit of work flowing through the dispatch pipeline
pub struct Message {
    pub id: String,
    pub pool_code: String,
    pub auth_token: Option<String>,
    pub signing_secret: Option<String>,
    pub mediation_type: MediationType,
    pub mediation_target: String,
    pub message_group_id: Option<String>,
    pub high_priority: bool,
    pub dispatch_mode: DispatchMode,
}

pub enum MediationType { HTTP /* future: SQS, Kafka */ }

pub enum DispatchMode {
    Immediate,        // fully concurrent, no ordering
    NextOnError,      // skip failed message, continue
    BlockOnError,     // halt the group on failure
}

pub enum DispatchStatus {
    Pending, Queued, Processing,
    Completed, Failed, Cancelled, Expired,
}
```

Lenient parsing on enums: unknown `DispatchMode` strings map to `Immediate`; legacy `DispatchStatus` strings (`IN_PROGRESS`, `ERROR`) map to their current equivalents (`Processing`, `Failed`). Wire format is always SCREAMING_SNAKE_CASE.

Other essentials:

- `QueuedMessage` — `Message` + `receipt_handle` + `broker_message_id`. The shape the router sees when polling.
- `MessageCallback` trait — `ack() / nack(delay_seconds)`. Implemented by each queue backend.
- `InFlightMessage` — internal router bookkeeping.
- `PoolConfig`, `QueueConfig`, `RouterConfig` — what the platform's config endpoint returns to the router.
- `LeaderElectionConfig` — leader election parameters.
- `EntityType` enum + `tsid::generate`/`generate_with_prefix`/`generate_untyped` free functions — 30 entity types, prefixed Crockford-Base32 IDs (`clt_0HZXEQ5Y8JY5Z`, `usr_…`, `evt_…`, `sub_…`, etc.).
- `config::env_or`, `env_bool`, `env_or_alias`, `env_or_parse` — the env-var helpers every binary uses. `_alias` variants accept a legacy TS name for ECS-task-def compatibility.

---

## fc-queue — queue abstraction

`crates/fc-queue/src/lib.rs`. The router and scheduler both use these traits without caring which backend backs them.

```rust
#[async_trait]
pub trait QueueConsumer: Send + Sync {
    fn identifier(&self) -> &str;
    async fn poll(&self, max_messages: u32) -> Result<Vec<QueuedMessage>>;
    async fn ack(&self, receipt_handle: &str) -> Result<()>;
    async fn nack(&self, receipt_handle: &str, delay_seconds: Option<u32>) -> Result<()>;
    async fn defer(&self, receipt_handle: &str, delay_seconds: Option<u32>) -> Result<()>;
    async fn extend_visibility(&self, receipt_handle: &str, seconds: u32) -> Result<()>;
    fn is_healthy(&self) -> bool;
    async fn stop(&self);
    async fn get_metrics(&self) -> Result<Option<QueueMetrics>>;
    fn get_counters(&self) -> Option<QueueMetrics>;
}

#[async_trait]
pub trait QueuePublisher: Send + Sync {
    fn identifier(&self) -> &str;
    async fn publish(&self, message: Message) -> Result<String>;
    async fn publish_batch(&self, messages: Vec<Message>) -> Result<Vec<String>>;
}

pub trait EmbeddedQueue: QueueConsumer + QueuePublisher {
    async fn init_schema(&self) -> Result<()>;
}
```

The `defer` distinction (vs `nack`) matters: `nack` is "delivery failed, retry"; `defer` is "I can't process this right now, backpressure". Routers use `defer` when a pool is at capacity. Counted separately for observability.

Backends:

| Backend | Feature | Module | When to use |
|---|---|---|---|
| AWS SQS | `sqs` | `sqs.rs` | Production |
| SQLite (embedded) | `sqlite` | `sqlite.rs` | fc-dev, tests |
| PostgreSQL | `postgres` | `postgres.rs` | Alt-cloud deploys without SQS |
| ActiveMQ | `activemq` | `activemq.rs` | Existing AMQP infrastructure |
| NATS JetStream | `nats` | `nats.rs` | NATS-shop |

All backends implement the same trait surface. Choosing one is a deploy-time decision, not a code change.

---

## fc-standby — leader election

`crates/fc-standby/src/lib.rs`. Redis-based mutual exclusion with lease renewal.

```rust
let cfg = LeaderElectionConfig::new("redis://redis:6379")
    .with_lock_key("fc:server:leader")
    .with_lock_ttl(Duration::from_secs(30))
    .with_refresh_interval(Duration::from_secs(10));

let election = Arc::new(LeaderElection::new(cfg).await?);
election.clone().start().await?;     // starts refresh task

if election.is_leader() { /* run background work */ }
```

Mechanism:

- `SET key value NX EX 30` to acquire. The value is a per-instance UUID so a node can't accidentally renew someone else's lock.
- Every 10 s the leader runs `SET key value XX EX 30` to renew. Non-leaders try to `SET ... NX` and fail until the TTL expires.
- Lost leadership is observable via `LeaderElection::subscribe()` (returns a `watch::Receiver<LeadershipStatus>`).

Trade-offs:

- **At most one leader at a time given Redis is consistent.** A network split between leader and Redis still respects the lease — when the lease expires, the standby acquires; the original leader's renewal will fail and it learns it's no longer leader within one refresh interval.
- **Up to lock_ttl seconds of no leader during failover.** Acceptable for the use case; tune `lock_ttl` down at the cost of being more sensitive to transient Redis hiccups.

`fc-server` uses this to gate router, scheduler, stream processor, and outbox subsystems. The platform API and metrics endpoints always run regardless of leadership.

---

## fc-platform — control plane

The big one (40+ KLoC). Owns aggregates, repositories, use cases, the UoW seal, every API router, the OIDC bridge, the scheduler, all the auth machinery. Full architecture in [platform-control-plane.md](platform-control-plane.md).

Public surface in `lib.rs`: every module is re-exported at top level so binaries can compose them — `fc_platform::api`, `fc_platform::repository`, `fc_platform::service`, `fc_platform::usecase`, `fc_platform::shared`, `fc_platform::seed`, and one module per aggregate.

---

## fc-router — message router

`crates/fc-router/src/lib.rs`. Detailed walkthrough in [message-router.md](message-router.md). Exports:

- `QueueManager`, `ProcessPool`, `MessageGroupHandler` — orchestration.
- `HttpMediator`, `HttpMediatorConfig` — delivery.
- `CircuitBreakerRegistry`, `CircuitBreakerConfig` — per-endpoint breakers.
- `LifecycleManager`, `LifecycleConfig` — background tasks.
- `ConfigSyncService`, `ConfigSyncConfig` — hot-reload config.
- `HealthService`, `WarningService` — observability.
- `StandbyProcessor` — standby integration.
- `AlbTrafficConfig`, `spawn_traffic_watcher` (feature `alb`) — ALB target-group automation.
- HTTP API routes (`api::create_router`).

Standalone binary (`bin/fc-router/`) and embedded usage (`bin/fc-server/src/main.rs::spawn_router`) both come from this crate.

---

## fc-stream — stream processor

`crates/fc-stream/src/lib.rs`. See [stream-processor.md](stream-processor.md). Exports:

- `start_stream_processor(pool, config) -> (StreamProcessorHandle, StreamHealthService)` — the entry point.
- `StreamProcessorConfig` — enable flags + batch sizes for each loop.
- `EventProjectionService`, `DispatchJobProjectionService` — CQRS read models.
- `EventFanOutService` — events → dispatch jobs.
- `PartitionManagerService`, `PartitionManagerConfig` — monthly partition lifecycle.

---

## fc-outbox — outbox processor

`crates/fc-outbox/src/lib.rs`. See [outbox-processor.md](outbox-processor.md). Exports:

- `EnhancedOutboxProcessor`, `EnhancedProcessorConfig` — orchestrator.
- `OutboxRepository` trait + per-backend impls (`postgres`, `sqlite`, `mysql`, `mongo` — feature-gated).
- `GroupDistributor` (per-group serial drains), `GroupStateManager` (Running/Paused/Blocked) — pipeline pieces.
- `HttpDispatcher`, `HttpDispatcherConfig` — POSTs to platform endpoints.
- `RecoveryTask` — stuck-item recovery.

Note: this crate is the **only** workspace member where MongoDB support is still wired in. Everywhere else (fc-platform, fc-stream, fc-router) MongoDB was removed during the great Postgres consolidation.

---

## fc-sdk — application SDK

`crates/fc-sdk/src/lib.rs`. The crate that consumer applications depend on. Not used internally by the platform; intentionally narrow.

```rust
use fc_sdk::{
    DomainEvent, ExecutionContext, EventMetadata,
    UseCase, UseCaseError, UseCaseResult,
    UnitOfWork,                  // feature: outbox-postgres / outbox-sqlite / outbox-mysql
};
```

Optional features:

| Feature | Adds |
|---|---|
| `outbox-postgres` (default) | `OutboxUnitOfWork` against PG `outbox_messages`. |
| `outbox-sqlite` | Same against SQLite. |
| `outbox-mysql` | Same against MySQL. |
| `client` | HTTP client for platform APIs (sync event types, subscriptions, connections from your manifest). |
| `auth` | JWKS cache + token validation — for an application that needs to validate FC tokens. |
| `webhook` | HMAC-SHA256 signature validation — for an application that receives FC webhooks. |

The two main usage patterns:

1. **Event publisher.** Use the SDK's `OutboxUnitOfWork` to write business state and an outbox row in one transaction; the outbox processor delivers them. See [developers/publishing-events.md](../developers/publishing-events.md).
2. **Webhook receiver.** Use the SDK's `webhook` module to validate the HMAC on incoming `/dispatch/process`-pattern POSTs. See [developers/receiving-webhooks.md](../developers/receiving-webhooks.md).

Language equivalents live in `clients/typescript-sdk/` and `clients/laravel-sdk/`. They follow the same patterns; the Rust crate is the reference.

---

## fc-mcp — MCP server

`crates/fc-mcp/src/lib.rs`. Model Context Protocol server. Read-only surface for AI agents that need to query event types, subscriptions, schemas — useful when an AI assistant is helping a developer figure out which event to publish or which subscription to create.

```rust
pub async fn run_stdio(config: &Config) -> Result<()>;
pub async fn run_http(config: &Config, bind: SocketAddr) -> Result<()>;
```

Config is OAuth client credentials (`base_url`, `client_id`, `client_secret`, `token_url`). The server authenticates as a service account, queries the platform via its HTTP API, and exposes the results as MCP tools to whatever LLM client is connected.

Read-only by design. The MCP role is "explain the platform to an agent", not "let the agent reconfigure the platform".

Two transports: stdio (default — for editor extensions) and HTTP (for standalone running). The `fc-mcp-server` binary just wraps `run_stdio` / `run_http`; you can also embed via `fc-dev mcp` for development.

---

## Adding a new shared dep

Workspace-wide rule: keep `fc-common` light. New types added there should be genuinely cross-crate. Types used only by one crate stay in that crate.

Adding a brand-new backend (queue, secrets, outbox)? Add it as a feature, behind `#[cfg(feature = "foo")]`. The feature shouldn't be enabled by default unless it's reasonable to compile without external dependencies on every workstation. Default features should fit the "developer just cloned, can run tests" use case.

---

## Code references

- Workspace manifest: `Cargo.toml`.
- Per-crate manifests: `crates/*/Cargo.toml`.
- Crate roots: `crates/*/src/lib.rs`.
- Per-component detail: [message-router](message-router.md), [scheduler](scheduler.md), [stream-processor](stream-processor.md), [outbox-processor](outbox-processor.md), [platform-control-plane](platform-control-plane.md), [auth-and-oidc](auth-and-oidc.md), [partitioning](partitioning.md).
