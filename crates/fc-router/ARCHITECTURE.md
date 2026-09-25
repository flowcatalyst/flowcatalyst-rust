# FlowCatalyst Message Router — Architecture Documentation

## System Context (C4 Level 1)

The Message Router sits between SQS message queues and downstream HTTP webhook endpoints. It consumes dispatch job pointers from SQS, applies rate limiting, concurrency control, and circuit breaking, then delivers each message via HTTP POST to its target endpoint.

```
┌──────────────┐     ┌──────────────────┐     ┌───────────────────┐
│  Dispatch     │────>│  SQS FIFO        │────>│  Message Router   │
│  Scheduler    │     │  Queues          │     │  (fc-router)      │
└──────────────┘     └──────────────────┘     └────────┬──────────┘
                                                       │
                     ┌──────────────────┐              │  HTTP POST
                     │  Config Service  │─ ─ ─ ─ ─ ─ ─┤  per message
                     │  (Platform API)  │  config sync │
                     └──────────────────┘              │
                                                       ▼
                     ┌──────────────────┐     ┌───────────────────┐
                     │  Redis           │     │  Webhook Endpoints│
                     │  (Leader Lock)   │     │  (per connection) │
                     └──────────────────┘     └───────────────────┘
```

**External systems:**

| System | Protocol | Purpose |
|--------|----------|---------|
| SQS FIFO Queues | AWS SDK | Message consumption (poll, ACK, NACK, visibility extension) |
| Webhook Endpoints | HTTP/2 POST | Message delivery to subscriber endpoints |
| Config Service | HTTP GET | Dynamic pool/queue configuration (5-minute sync) |
| Redis | TCP | Leader election for active/standby HA |
| Teams Webhook | HTTP POST | Operational alerts (batched, severity-filtered) |

---

## Container View (C4 Level 2)

The router runs as a single binary (`fc-router`) or embedded in the unified server (`fc-server`). Internally it consists of these containers:

```
┌─────────────────────────────────────────────────────────────────────┐
│                        Message Router                               │
│                                                                     │
│  ┌─────────────┐  ┌──────────────┐  ┌────────────────────────┐     │
│  │  SQS Poll   │─>│ QueueManager │─>│ ProcessPool (per code) │     │
│  │  Tasks      │  │ (orchestrator)│  │  ┌─ Semaphore          │     │
│  │  (per queue)│  │              │  │  ├─ Rate Limiter       │     │
│  └─────────────┘  │  Dedup       │  │  ├─ Group Handlers     │     │
│                    │  Route       │  │  └─ Drain/Imm Tasks   │     │
│                    │  Track       │  └──────────┬─────────────┘     │
│                    └──────────────┘             │                    │
│                                                 ▼                    │
│  ┌─────────────┐  ┌──────────────┐  ┌──────────────────────┐       │
│  │ Config Sync │  │  CB Registry │  │    HttpMediator       │       │
│  │ Service     │  │ (per endpoint)│  │  HTTP POST + signing │       │
│  └─────────────┘  └──────────────┘  └──────────────────────┘       │
│                                                                     │
│  ┌─────────────┐  ┌──────────────┐  ┌──────────────────────┐       │
│  │ Lifecycle   │  │   Health     │  │   Warning +          │       │
│  │ Manager     │  │   Service    │  │   Notifications      │       │
│  └─────────────┘  └──────────────┘  └──────────────────────┘       │
│                                                                     │
│  ┌──────────────────────────────────────────┐                       │
│  │            REST API (Axum)               │                       │
│  │  /health  /monitoring  /publish  /pools  │                       │
│  └──────────────────────────────────────────┘                       │
└─────────────────────────────────────────────────────────────────────┘
```

---

## Component View (C4 Level 3)

### Message Flow

```
SQS Queue
    │
    ▼
Consumer.poll(10)                          ← 1 task per queue, adaptive sleep
    │
    ▼
QueueManager.route_batch()
    │
    ├─ Phase 0: pending_delete check       ← ACK messages that failed to delete last time
    ├─ Phase 1: filter_duplicates()        ← broker_id redelivery + app_id requeue detection
    ├─ Phase 2: group_by_pool()            ← route to pool by pool_code (fallback: DEFAULT-POOL)
    └─ Phase 3: group_by_message_group()   ← FIFO enforcement within each pool
         │
         ▼
ProcessPool.submit()
    │
    ├─ dispatch_mode == IMMEDIATE ──────> spawn_immediate_task()
    │                                        │
    │                                        ├─ rate_limiter.until_ready()   ← async, zero CPU
    │                                        ├─ semaphore.acquire()          ← concurrency gate
    │                                        ├─ cb_registry.allow_request()  ← per-endpoint CB
    │                                        ├─ mediator.mediate()           ← HTTP POST
    │                                        ├─ cb_registry.record_*()       ← update CB state
    │                                        └─ callback.ack() / nack()     ← SQS operation
    │
    └─ dispatch_mode == BLOCK_ON_ERROR / NEXT_ON_ERROR
         │
         ▼
    MessageGroupHandler.enqueue()
         │
         └─ if idle: spawn_drain_task()
              │
              ▼
         Sequential loop (one message at a time per group):
              ├─ dequeue next (high_priority first, then regular)
              ├─ rate_limiter.until_ready()
              ├─ semaphore.acquire()
              ├─ cb_registry.allow_request()
              ├─ mediator.mediate()
              ├─ record metrics + CB
              ├─ callback.ack() / nack()
              └─ loop (or exit if queue empty)
```

### Deduplication

```
Message arrives from SQS:
    │
    ├─ broker_message_id in in_pipeline?
    │     YES → SQS redelivery (visibility timeout expired while processing)
    │           Update receipt_handle to latest, skip processing
    │
    ├─ app_message_id in app_message_to_pipeline_key?
    │     YES, different broker_id → External requeue (stale recovery created new SQS msg)
    │           ACK the new message (original still processing)
    │
    └─ Neither → New message, process normally
```

### Circuit Breaker State Machine

```
    ┌──────────┐
    │  CLOSED  │◄─────────────── success_threshold met
    │ (normal) │                        │
    └────┬─────┘                        │
         │ failure_rate ≥ 50%     ┌─────┴──────┐
         │ (after min_calls)      │  HALF_OPEN  │
         │                        │  (testing)  │
         ▼                        └─────┬──────┘
    ┌──────────┐                        │
    │   OPEN   │───────────────────────►┘
    │(rejecting)│  reset_timeout expires
    └──────────┘
         │
         │ any failure in HALF_OPEN → back to OPEN
```

Per-endpoint keyed by `mediation_target` URL. Shared across all pools targeting the same endpoint.

### Rate Limiting

```
governor token bucket (per pool):

    rate_limit_per_minute: 60  →  1 token/second

    Fast path:  rl.check().is_ok()  →  permit available, proceed
    Slow path:  rl.until_ready().await  →  async sleep, zero CPU
    Timeout:    30 seconds max wait  →  NACK with 10s delay
```

### Group Outcomes (FIFO)

One drain task per ordered group takes messages from a strict FIFO buffer
(no priority lane inside a group), one at a time. What the mediation outcome
does to the group follows Go's `pool.go` (`disposition_of`):

```
Group [order_456], buffer: A B C D

    A: Success                  → ACK, move on to B
    B: 429 / ack:false          → put B back at the FRONT, wait out the backoff,
                                  attempt B again (C and D wait behind it);
                                  after MAX_IN_PIPELINE_ATTEMPTS, release as below
    B: 502/503/504, unreachable,
       breaker open             → NACK B, take the WHOLE buffer (C, D) and NACK it
    B: ack:false + delaySeconds
       (broker holds delays)    → same whole-group release, B with exactly that delay
    B: 4xx / R-57 5xx           → ACK B away; NEXT_ON_ERROR moves on to C,
                                  BLOCK_ON_ERROR takes and NACKs C, D
```

---

## Component Details

### QueueManager

Central orchestrator. Owns all state.

| Field | Type | Purpose |
|-------|------|---------|
| `in_pipeline` | `DashMap<String, InFlightMessage>` | Track messages being processed (keyed by broker_message_id) |
| `app_message_to_pipeline_key` | `DashMap<String, String>` | App message ID → pipeline key (requeue detection) |
| `pools` | `DashMap<String, Arc<ProcessPool>>` | Active process pools by code |
| `draining_pools` | `DashMap<String, Arc<ProcessPool>>` | Pools removed from config, finishing in-flight |
| `consumers` | `RwLock<HashMap<...>>` | Active SQS consumers |
| `pending_delete_broker_ids` | `Mutex<HashMap<String, Instant>>` | Messages that ACKed but SQS delete failed (TTL eviction) |
| `self_ref` | `RwLock<Option<Weak<Self>>>` | For spawning poll tasks from config sync |

**Adaptive polling** (per consumer):
- Full batch (10 messages) → re-poll immediately
- Partial batch → 500ms pause
- Empty → 1s pause
- All pools at capacity → 2s pause (backpressure)

### ProcessPool

Worker pool per processing pool code.

| Field | Type | Purpose |
|-------|------|---------|
| `semaphore` | `Arc<Semaphore>` | Concurrency limit (number of permits = concurrency level) |
| `group_handlers` | `DashMap<Arc<str>, Mutex<MessageGroupHandler>>` | Per-group FIFO queues (~200 bytes idle) |
| `rate_limiter` | `RwLock<Option<Arc<RateLimiter>>>` | Governor token bucket (updatable at runtime) |
| `circuit_breaker_registry` | `Arc<CircuitBreakerRegistry>` | Shared per-endpoint circuit breakers |
| `metrics_collector` | `Arc<PoolMetricsCollector>` | HdrHistogram + windowed counters |

**Two task types:**
- `spawn_immediate_task()` — one independent task per IMMEDIATE message, fully concurrent
- `spawn_drain_task()` — one sequential task per message group (ordered modes), exits when empty

**Panic safety:** `WorkerGuard` (both task types) gives back the task's queue slot, active-worker count and `mediating` entry on any exit, panic included. `DrainGuard` also empties an abandoned group's buffer (the callbacks' Drop nacks each message), gives back a queue slot per message, and resets `processing`.

### HttpMediator

Stateless HTTP client. One instance shared across all pools.

- HTTP/2 production, HTTP/1.1 dev
- 15-minute request timeout (matches Java)
- HMAC-SHA256 webhook signing (`X-FLOWCATALYST-SIGNATURE`, `X-FLOWCATALYST-TIMESTAMP`)
- Bearer token injection
- Response classification: Success / ErrorConfig (4xx, ACK) / ErrorProcess (5xx, NACK) / ErrorConnection (NACK 30s)
- Custom retry delay from response body (`{"ack": false, "delaySeconds": 60}`)
- 429 Too Many Requests: respects `Retry-After` header

### CircuitBreakerRegistry

Per-endpoint circuit breakers keyed by mediation target URL. Single `Mutex<BreakerInner>` per endpoint protects all state transitions atomically.

| Config | Default | Description |
|--------|---------|-------------|
| `failure_rate_threshold` | 0.5 | 50% failure rate trips the breaker |
| `min_calls` | 10 | Minimum samples before evaluation |
| `success_threshold` | 3 | Successes in half-open to close |
| `reset_timeout` | 5s | Open → half-open transition delay |
| `buffer_size` | 100 | Sliding window size |

Idle breakers evicted after 1 hour of no activity.

### ConfigSyncService

Periodically fetches pool/queue configuration from one or more URLs.

- Merges pools by code (last wins), merges all queues
- Detects changes via hash comparison
- Hot-reloads: updates concurrency/rate-limit in-place, creates new pools, drains removed pools
- New queue consumers get poll tasks spawned automatically
- Retry: 12 attempts with 5s delay

### LifecycleManager

Background task coordinator. All tasks respect shutdown via `broadcast::Receiver`.

| Task | Interval | Purpose |
|------|----------|---------|
| Visibility extension | 55s | Extend SQS visibility for long-running messages |
| Memory health | 60s | Detect large in_pipeline map (potential leak) |
| Consumer health | 30s | Detect stalled consumers, auto-restart |
| Warning cleanup | 5m | Auto-acknowledge old warnings (8h TTL) |
| Health report | 60s | Compute overall status (Healthy/Warning/Degraded) |
| Stale entry reaper | 5m | Evict old in_pipeline entries (15m), pending_delete (1m), idle CBs (1h) |
| Config sync | 5m | Dynamic configuration reload |
| Standby heartbeat | 10s | Renew Redis leader lock |

### HealthService

Rolling window (VecDeque) success rate calculation per pool.

- **Healthy**: >90% success rate, <5 active warnings
- **Warning**: >70% success rate, <20 active warnings
- **Degraded**: <70% success rate or >20 active warnings

Consumer stall detection: if no poll recorded for 60s, trigger restart.

### Metrics (PoolMetricsCollector)

Per-pool metrics using HdrHistogram for O(1) percentile queries.

- **All-time**: total_success, total_failure, total_rate_limited, p50/p95/p99 latency
- **5-minute window**: success rate, throughput/sec, latency percentiles
- **30-minute window**: longer-term trend view
- **Transient errors**: tracked separately (not counted against success rate since message will be retried)

---

## Concurrency Primitives

| Primitive | Where | Why |
|-----------|-------|-----|
| `DashMap` | in_pipeline, pools, group_handlers | Lock-free concurrent access from multiple poll tasks |
| `parking_lot::Mutex` | pending_delete, MessageGroupHandler, BreakerInner | Brief sync locks, never held across .await |
| `parking_lot::RwLock` | rate_limiter, health counters | Read-heavy access patterns |
| `tokio::sync::RwLock` | consumers, pool_configs | Async-safe, held across .await in config sync |
| `tokio::sync::Semaphore` | pool concurrency | Async permit acquisition, dynamically resizable |
| `AtomicU32/U64 (Relaxed)` | queue_size, active_workers, counters | Independent counters, no ordering requirements |
| `AtomicBool (SeqCst)` | running flags, concurrency | Control flow gates requiring visibility guarantees |
| `broadcast::channel` | shutdown signal | Fan-out to all background tasks |

---

## Deployment

### Standalone Binary (fc-router)

Dedicated message router with no database dependency. Configured via environment variables:

```
FLOWCATALYST_CONFIG_URL=https://platform.example.com/api/config
FLOWCATALYST_CONFIG_INTERVAL=300
FLOWCATALYST_STANDBY_ENABLED=false
API_PORT=8080
AUTH_MODE=NONE
NOTIFICATION_TEAMS_ENABLED=true
NOTIFICATION_TEAMS_WEBHOOK_URL=https://...
```

### Embedded in fc-server

Runs alongside platform API, scheduler, and stream processor. Enabled via `FC_ROUTER_ENABLED=true` (alias: `MESSAGE_ROUTER_ENABLED`).

---

## Error Handling

| Scenario | Action | Retry Delay |
|----------|--------|-------------|
| HTTP 2xx, ack=true | ACK | — |
| HTTP 2xx, ack=false | NACK | response `delaySeconds` or 30s |
| HTTP 400/401/403/404 | ACK (config error, no retry) | — |
| HTTP 429 | NACK | `Retry-After` header or 30s |
| HTTP 5xx | NACK (transient) | 30s |
| Connection timeout | NACK | 30s |
| Circuit breaker open | NACK | 5s |
| Rate limit timeout | NACK | 10s |
| Pool at capacity | Defer (not counted as failure) | 5s |
| SQS ACK fails | Add to pending_delete, re-ACK on next poll | — |
