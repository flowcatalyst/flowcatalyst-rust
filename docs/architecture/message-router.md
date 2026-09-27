# Message Router

The message router is the delivery engine. It consumes dispatch-job pointers from a queue (SQS in production, SQLite/Postgres in dev), maintains FIFO ordering within message groups, applies per-pool rate limits and per-endpoint circuit breakers, and POSTs each message to its target webhook. Source: `crates/fc-router/`; it runs in `fc-server`'s router role (`FC_ROUTER_ENABLED`) and inside `fc-dev`.

This document supersedes the older `docs/message-router.md` and the design notes in `crates/fc-router/ARCHITECTURE.md`.

---

## Position in the system

```
fc-platform (scheduler)
        │
        │  PENDING → QUEUED + SQS publish
        ▼
   SQS FIFO queue
        │
        ▼
┌──────────────────────────────────────────────────────┐
│  Message Router (fc-router)                          │
│                                                      │
│  Poll tasks ─▶ QueueManager ─▶ ProcessPool ─▶ Mediator │
│                    │              │            │     │
│              (dedup,           (FIFO,       (HTTP/2,│
│              routing)          rate-limit,  signing,│
│                                CB gate)     classify)│
└──────────────────────────────────────────────────────┘
        │
        ▼ HTTP POST { "messageId": "..." }
   FlowCatalyst dispatch-process endpoint
        │
        ▼ delivers to actual webhook, then ACK/NACK back to router
```

The router does **not** carry the payload. SQS holds a pointer (`MessagePointer`); the dispatch-process endpoint loads the full dispatch job, attempts delivery, records the attempt, and returns an ACK or NACK with a delay hint. This keeps SQS message size small and centralises retry/attempt accounting in Postgres.

External systems the router touches:

| System | Protocol | Purpose |
|---|---|---|
| SQS FIFO queues | AWS SDK | Poll, ACK, NACK, visibility extension |
| Platform config endpoint(s) | HTTP GET | Pool + queue configuration (5-min sync) |
| Webhook endpoint (dispatch-process) | HTTP POST + HMAC | Message delivery |
| Redis | TCP | Leader election for active/standby (optional) |
| Teams webhook | HTTP POST | Operational alerts (optional, batched) |
| AWS ALB | AWS SDK | Target-group register/deregister on leadership transitions (optional, `alb` feature) |

---

## Components

```
┌────────────────────────────────────────────────────────────────────┐
│                          QueueManager                              │
│                                                                    │
│  in_pipeline (DashMap<broker_id, InFlight>)                        │
│  app_message_to_pipeline_key (DashMap)        ← requeue detection  │
│  pending_delete_broker_ids (Mutex<HashMap>)   ← ACK retry buffer   │
│  pools (DashMap<pool_code, Arc<ProcessPool>>)                      │
│  draining_pools (DashMap)                                          │
│  consumers (RwLock<HashMap>)                                       │
│                                                                    │
└────┬───────────────────────────────────────────────────────────────┘
     │
     │  one poll task per queue, adaptive sleep
     │
     ▼
┌──────────────────────────────────────────────────┐
│ Phase 0  pending_delete check                    │
│ Phase 1  filter_duplicates()                     │
│ Phase 2  group_by_pool()                         │
│ Phase 3  group_by_message_group()                │
└────┬─────────────────────────────────────────────┘
     │
     ▼ ProcessPool.submit()
┌────────────────────────────────────────────────────────────────────┐
│  ProcessPool (per pool code)                                       │
│                                                                    │
│  semaphore (concurrency)                                           │
│  rate_limiter (governor, optional)                                 │
│  group_handlers (DashMap<group, Mutex<MessageGroupHandler>>)       │
│  circuit_breaker_registry (shared per-endpoint)                    │
│  failed_batch_groups (DashSet<(batch_id, group)>)                  │
│  metrics_collector (HdrHistogram + rolling windows)                │
│                                                                    │
│  Two task shapes:                                                  │
│   - spawn_immediate_task()  IMMEDIATE mode: fully concurrent       │
│   - spawn_drain_task()      ordered modes: one task per group      │
└────┬───────────────────────────────────────────────────────────────┘
     │
     ▼
┌────────────────────────────────────────────────────────────────────┐
│  HttpMediator (one per pool — see "Per-pool mediators" below)      │
│                                                                    │
│  reqwest::Client (HTTP/2 prod, HTTP/1 dev, 15-min timeout)         │
│  HMAC-SHA256 sign  →  X-FLOWCATALYST-SIGNATURE + ...-TIMESTAMP     │
│  Response classification: Success / ConfigError / ProcessError /   │
│                           ConnectionError / RateLimited            │
└────────────────────────────────────────────────────────────────────┘
```

### QueueManager — the orchestrator

`crates/fc-router/src/manager.rs`. Owns all router state. Critical fields:

- **`in_pipeline: DashMap<String, InFlightMessage>`** — every message currently being processed, keyed by broker message ID. The reaper evicts entries that have been there longer than ~15 minutes (which would indicate a leaked callback).
- **`app_message_to_pipeline_key: DashMap<String, String>`** — maps application-level message ID to pipeline key. Used to detect external requeues (stale recovery in the scheduler created a fresh SQS message for the same dispatch job while the original was still in flight).
- **`pending_delete_broker_ids: Mutex<HashMap<String, Instant>>`** — messages we successfully delivered but SQS rejected the `DeleteMessage` call (transient AWS failures). On the next poll we retry the delete before doing anything else. TTL ~1 minute.
- **`pools` / `draining_pools`** — `Arc<ProcessPool>` keyed by pool code. Removed pools migrate into `draining_pools` and stay alive until their in-flight work clears.
- **`consumers`** — active SQS consumers, addressable by queue URL.

Polling is **adaptive** per consumer:

| Outcome | Next-poll delay |
|---|---|
| Full batch of 10 returned | 0 (re-poll immediately) |
| Partial batch | 500 ms |
| Empty queue | 1 s |
| All target pools at capacity | 2 s (backpressure) |

The "capacity gate" is important: a noisy queue with no available pool slots would otherwise have us spin-polling SQS (which costs money and CPU). Polling backs off until a pool slot frees up.

### Message flow per batch

Every poll runs four phases inside the manager:

**Phase 0 — pending-delete retry.** Drain `pending_delete_broker_ids`, calling `consumer.delete()` for each. Anything that fails again is re-buffered with a fresh deadline. This keeps SQS clean even when AWS throws transient errors.

**Phase 1 — deduplicate.** For every newly polled message:

```
in_pipeline already has this broker_message_id?
  → SQS redelivery (visibility timeout expired while we were still working).
    Update the receipt_handle to the latest copy, skip processing.

app_message_to_pipeline_key already has this app_message_id, different broker_id?
  → External requeue (stale-recovery in the scheduler created a fresh SQS message).
    ACK the new copy immediately (the original is still in flight).

Neither matched?
  → Add to in_pipeline, proceed.
```

**Phase 2 — route to pool.** Look up the pool by `message.pool_code`. If the pool was removed mid-flight and is now in `draining_pools`, route there. If the pool doesn't exist at all, fall back to `DEFAULT-POOL` (configurable). The "DEFAULT-POOL exists in your router config" assumption is a real one — losing it means dispatch jobs with unknown pool codes get NACKed forever.

**Phase 3 — group by message_group within the pool.** Pool's `submit()` makes the dispatch-mode decision (see below).

### ProcessPool — the workhorse

`crates/fc-router/src/pool.rs`. Per pool code. The split between IMMEDIATE and ordered modes is the core architectural decision:

#### Two task shapes

```rust
match message.dispatch_mode {
    Immediate     => spawn_immediate_task(),          // fully concurrent
    NextOnError   => enqueue_to_group_handler(),      // sequential per group,
    BlockOnError  => enqueue_to_group_handler(),      // sequential per group,
}
```

**`spawn_immediate_task()`** — one independent tokio task per message. Each task acquires a semaphore permit, waits for a rate-limit permit, checks the per-endpoint circuit breaker, calls the mediator, records metrics, then ACKs or NACKs. No coordination between tasks. Throughput is bounded by `concurrency` × `rate_limit_per_minute`.

**`spawn_drain_task()`** — one task per active message group. The task pulls from the group's `MessageGroupHandler` queue in order: high-priority first, then regular. For each message it does the same semaphore/rate-limit/CB/mediate cycle, then loops. Exits when the queue is empty. If another message arrives for the same group before the drain task exits, it's appended; the running task picks it up on the next iteration. If the drain task has already exited, a fresh one is spawned.

Why "one task per group" rather than "one task always polling all groups"? Per-task spawn cost is negligible compared to the cost of contention on a shared queue across many groups. Also: per-group panic isolation — if a group's drain task panics, only that group is affected.

#### Panic safety

The drain task is wrapped in a `PanicGuard` (RAII). If the closure panics or returns early:

1. The group's `processing` flag is reset, allowing a future submit to spawn a fresh drain task.
2. `active_workers` is decremented.
3. `in_flight_groups` is cleaned up.

Without this guard a panic in one delivery would leak the group's state and starve the group forever. The router has been hardened on this once already (search the codebase for `PanicGuard`).

#### Cascading NACK on batch+group failure

The router treats the combination of `batch_id` and `message_group` as a unit. When a message inside a batch+group fails and the dispatch mode is `BlockOnError`, every subsequent message in the same batch+group is short-circuit-NACKed without an HTTP attempt:

```
Batch B1, Group order_456, mode BLOCK_ON_ERROR:

  msg A → mediator → Success           → ACK
  msg B → mediator → ErrorProcess (5xx) → NACK + insert (B1, order_456) into failed_batch_groups
  msg C → failed_batch_groups hit       → NACK immediately, no HTTP call
  msg D → failed_batch_groups hit       → NACK immediately, no HTTP call

  Next poll: all NACKed messages reappear; B has the lowest sequence so it's retried first.
```

This preserves FIFO without holding up the queue: instead of blocking new arrivals waiting for the failed message to retry, we NACK and let SQS reorder via visibility-timeout expiry. The `failed_batch_groups` entries time out on a TTL (~5 minutes) and are also cleared once the batch's message count for that group reaches zero.

`NextOnError` differs only in policy: a failure of msg B doesn't poison the group; msg C and D proceed.

### HttpMediator — the actual delivery

`crates/fc-router/src/mediator.rs`. One `HttpMediator` per pool (see [per-pool mediators](#per-pool-mediators-the-128-stream-cap) below for why). Wraps a single `reqwest::Client` with:

| Setting | Production | Dev |
|---|---|---|
| HTTP version | HTTP/2 only (ALPN) | HTTP/1.1 only |
| Request timeout | 900 s (15 min, matches the Java implementation) | same |
| Connect timeout | 30 s | same |
| Idle pool per host | 10 connections | same |

Request building:

1. Build the URL from `message.mediation_target`.
2. If `message.signing_secret` is set, compute HMAC-SHA256(secret, timestamp + body) and attach two headers: `X-FLOWCATALYST-SIGNATURE` (hex digest) and `X-FLOWCATALYST-TIMESTAMP` (ISO-8601 with ms precision). The receiver uses these to detect spoofing and replay.
3. If `message.auth_token` is set, attach `Authorization: Bearer <token>`.
4. POST, await response.

Response classification (the function that decides ACK vs NACK):

| Status | Result | Action | Retry delay |
|---|---|---|---|
| 2xx with `{"ack": true}` (or no body) | `Success` | ACK | — |
| 2xx with `{"ack": false, "delaySeconds": N}` | `RateLimited` | NACK | N (default 30 s) |
| 400 | `ConfigError` | ACK (no retry — caller is broken) | — |
| 401, 403 | `ConfigError` (CRITICAL warning) | ACK | — |
| 404 | `ConfigError` (endpoint missing) | ACK | — |
| 429 | `RateLimited` | NACK | `Retry-After` header or 30 s |
| 501 | `ConfigError` (CRITICAL — not implemented) | ACK | — |
| Other 4xx | `ConfigError` | ACK | — |
| 5xx | `ProcessError` (transient) | NACK | 30 s |
| Network error, timeout | `ConnectionError` | NACK | 30 s |
| CB rejected | `CircuitOpen` | NACK | 5 s |
| Rate-limit wait > 30 s | `RateLimitTimeout` | NACK | 10 s |

Key invariants:

- **4xx is terminal.** A 4xx means the caller's request is wrong (bad payload, wrong URL, missing auth). Retrying won't help. Router ACKs and emits a warning.
- **5xx is transient.** Server problem; NACK and let SQS redeliver.
- **429 is not a circuit-breaker failure.** Throttling is normal back-pressure; counting it against the CB would prematurely trip on healthy-but-throttling endpoints.
- **Body `ack=false` is a polite NACK.** Webhook receivers use this to ask for backoff without returning an HTTP error (which is good for observability — their dashboards don't fill with "errors").

### CircuitBreakerRegistry — per endpoint, shared across pools

`crates/fc-router/src/circuit_breaker_registry.rs`. The breaker key is the `mediation_target` URL, not the pool code. Two pools targeting the same webhook share one breaker, which is the right call — the failure characteristic belongs to the endpoint, not to who's calling it.

State machine:

```
       ┌──────────────┐
       │    CLOSED    │◄──────── success_threshold met
       │   (normal)   │
       └──────┬───────┘
              │ failure_rate ≥ threshold (after min_calls)
              ▼
       ┌──────────────┐
       │     OPEN     │
       │  (rejecting) │
       └──────┬───────┘
              │ reset_timeout expires
              ▼
       ┌──────────────┐
       │  HALF_OPEN   │── failure ──▶ OPEN
       │  (probing)   │
       └──────┬───────┘
              │ success_threshold consecutive successes
              ▼
            CLOSED
```

Configurable defaults:

| Field | Default | Meaning |
|---|---|---|
| `failure_rate_threshold` | 0.5 | Trip when ≥ 50 % of recent calls fail |
| `min_calls` | 10 | Don't evaluate until we have at least 10 samples |
| `success_threshold` | 3 | Three consecutive half-open successes to close |
| `reset_timeout` | 5 s | Open → half-open delay |
| `buffer_size` | 100 | Sliding window of recent outcomes |

Idle breakers (no activity for ~1 h) are evicted by the lifecycle reaper to keep memory bounded — a flapping deploy could otherwise create millions of breakers, one per ephemeral URL.

All state transitions are guarded by a single `Mutex` per endpoint, held only for the duration of the transition (never across `.await`). Reads of state for the "should I allow this request" check go through the same mutex but with minimal contention.

### ConfigSyncService — hot reload

`crates/fc-router/src/config_sync.rs`. The router does not know its pools and queues at boot — it fetches them from one or more platform URLs (`FLOWCATALYST_CONFIG_URL`, comma-separated), then refreshes every 5 minutes.

Sync algorithm:

1. Fetch from all URLs in parallel. Tolerate partial failures: if 2 of 3 URLs return successfully, merge what came back. (If all fail, retry with backoff; up to 12 attempts × 5 s delay before giving up.)
2. Merge: pools deduplicated by `code` (later URLs win on conflict); queues all included.
3. Compare to current config:
   - **New pool** → create `ProcessPool`, register with manager.
   - **Pool with changed concurrency or rate_limit** → update in place (governor's `RateLimiter` and the `Semaphore` are swappable at runtime; in-flight tasks aren't disturbed).
   - **Removed pool** → move to `draining_pools`. New messages with that pool code now route to DEFAULT-POOL or get NACKed (consumer's choice via `unknown_pool_action`). In-flight work in the draining pool finishes naturally; once empty, the pool is dropped.
   - **New queue** → spawn a poll task, create an SQS consumer.
   - **Removed queue** → consumer stops accepting new polls; finishes any in-flight visibility extensions; drops.

Important consequence: **the router can be reconfigured without restart**. A platform operator adds a new pool through the admin UI, the router picks it up within 5 minutes, no rolling restart needed.

### Lifecycle — background tasks

`crates/fc-router/src/lifecycle.rs`. Each task runs in its own tokio task, listens on a shared `broadcast::Receiver<()>` for shutdown.

| Task | Interval | Job |
|---|---|---|
| Visibility extension | 55 s | For every in-flight message whose SQS visibility timeout is within 5 s, extend it by the configured amount. Prevents long-running deliveries from being redelivered while still processing. |
| Memory health | 60 s | Sample `in_pipeline.len()` and `app_message_to_pipeline_key.len()`; warn if either grows unboundedly (callback leak indicator). |
| Consumer health | 30 s | Check the last-poll timestamp of every consumer; restart any that haven't polled in 60 s. |
| Warning cleanup | 5 min | Drop warnings older than 8 h from the warning service. |
| Health report | 60 s | Compute Healthy / Warning / Degraded for the router as a whole. Cached so `/health` reads are O(1). |
| Reaper | 5 min | Evict in_pipeline entries older than 15 min, pending_delete entries older than 1 min, idle CBs older than 1 h. |
| Config sync | 5 min | Refresh pool/queue config (above). |
| Standby heartbeat | 10 s | Renew Redis leader lock (only when standby enabled). |

### HTTP API

`crates/fc-router/src/api/`. Routes:

| Path | Auth | Purpose |
|---|---|---|
| `GET /health`, `/q/health` | none | Liveness with cheap latency probe |
| `GET /health/live`, `/health/ready`, `/health/startup` | none | Kubernetes probes |
| `GET /metrics`, `/q/metrics` | none | Prometheus scrape |
| `GET /dashboard.html`, `/monitoring/dashboard`, `GET /dashboard/auth-config`, `POST /dashboard/token` | none | The dashboard page and its platform sign-in helpers (no router data) |
| `GET /monitoring` | platform bearer, `platform:messaging:router:view` | Overview JSON |
| `GET /monitoring/health` | as above | Detailed health report |
| `GET /monitoring/pools` / `PUT /monitoring/pools/:code` | `:view` / `:operate` | List pools; update concurrency at runtime |
| `GET /monitoring/queues` | as above | Queue depths / consumer stats |
| `GET /monitoring/circuit-breakers` | as above | Per-endpoint state + recent failure rate |
| `GET /monitoring/pool-stats` | as above | HdrHistogram-backed p50/p95/p99 per pool |
| `GET /monitoring/warnings` | as above | Active warnings (filterable) |
| `GET /monitoring/consumer-health` | as above | Lag distribution per consumer |
| `GET /monitoring/standby-status` | as above | Leader / standby state |
| `GET /monitoring/traffic-status` | as above | ALB target-group registration state |

**Auth (owner ruling 2, decision #21; `api/platform_auth.rs`).** Outside dev mode every route
but the public ones above needs `Authorization: Bearer <platform JWT>`, verified against the
platform's JWKS (`FC_ROUTER_PLATFORM_URL`, else the platform in the same `fc-server`) with the
verifier the function host uses (`fc-platform-jwks`): issuer, expiry, RS256 signature,
`token_use=api`. `GET`/`HEAD` and `POST /monitoring/in-flight-messages/check-batch` need
`platform:messaging:router:view`; every other method (publish, breaker resets, in-flight ACK,
group-flush clear, pool update, config reload, warning acknowledge/clear, broker-stats refresh)
needs `platform:messaging:router:operate`. A missing or bad token is 401 (`WWW-Authenticate:
Bearer`, `X-Auth-Mode: BEARER`); a token without the permission is 403 `PERMISSION_REQUIRED`.
`platform:router-operator` holds both, `platform:viewer` and `platform:application-service` hold
`:view`, and super-admin holds both through its wildcard. Dev mode keeps `AUTH_MODE` (Basic, the
OIDC modes, open), and `AUTH_MODE=BEARER` opts into platform tokens there. Decision #43: outside
dev mode `AUTH_MODE=NONE` is still honoured for now, with a WARN at startup and an `authWarning`
on the health and monitoring output. The dashboard signs in through the platform (authorization
code + PKCE, the public client `FC_ROUTER_DASHBOARD_CLIENT_ID`), holding the access token in
memory only.

The mock, benchmark and seed endpoints (`/api/test/*`, `/api/benchmark/*`, `/api/seed/messages`)
are mounted in dev mode (`FLOWCATALYST_DEV_MODE`, fc-dev) only; elsewhere they are absent.

### Standby

`crates/fc-router/src/standby.rs` wraps `fc-standby::LeaderElection`. When enabled the router still starts its HTTP servers (so health probes and metrics stay reachable on standby nodes), but all polling tasks block in `wait_for_leadership()` until the Redis lock is acquired. On leadership loss, polling stops, in-flight messages drain naturally, the SQS consumers are shut down. See [high-availability.md](../operations/high-availability.md).

### ALB integration (optional, `alb` feature)

When `FC_ALB_ENABLED=true` and standby is enabled, on leadership transitions the router registers (leader) or deregisters (standby) the current node in an AWS ALB target group. Combined with ALB-based health checks this is how an active/standby pair appears as a single endpoint to upstream callers. `crates/fc-router/src/traffic.rs`.

---

## Per-pool mediators (the 128-stream cap)

Until recently the router had a single `HttpMediator` (one reqwest `Client`, one connection pool) shared across all pools. AWS ALB / API Gateway / NLB cap **HTTP/2 streams at 128 per connection**. Once 128 in-flight requests existed across all pools targeting the same origin, every pool queued behind those 128 slots regardless of how concurrency was configured per pool.

Per-pool mediators (current code) give each pool its own `Client` and thus its own connection pool. With N pools targeting the same origin, you get up to N × 128 streams. The circuit-breaker registry is still shared — breakers are keyed by endpoint, not by pool.

Adaptive concurrency (TCP Vegas) is designed but not yet shipped — see [adaptive-concurrency.md](adaptive-concurrency.md) for the algorithm and why AIMD isn't the right shape here.

---

## Concurrency primitives, quick reference

| Primitive | Where | Why |
|---|---|---|
| `DashMap` | in_pipeline, pools, group_handlers | Lock-free reads/writes from multiple poll tasks |
| `parking_lot::Mutex` | pending_delete, MessageGroupHandler, BreakerInner | Short critical sections, never held across `.await` |
| `parking_lot::RwLock` | rate_limiter, health counters | Read-heavy |
| `tokio::sync::RwLock` | consumers, pool_configs | Held across `.await` in config sync |
| `tokio::sync::Semaphore` | per-pool concurrency | Async permit, resizable at runtime |
| `Atomic*` (Relaxed) | counters | No ordering requirement |
| `Atomic*` (SeqCst) | running flags | Control-flow correctness |
| `broadcast::channel` | shutdown signal | Fan-out to every background task |

---

## Deployment patterns

The router can be deployed two ways. Both are supported in production.

### Embedded in `fc-server`

```sh
FC_ROUTER_ENABLED=true \
FC_STANDBY_ENABLED=true \
FC_STANDBY_REDIS_URL=redis://redis:6379 \
FLOWCATALYST_CONFIG_URL=http://platform:3000/api/config/router \
  ./fc-server
```

This is the simplest topology and what most deployments use. The platform API, router, scheduler, and stream processor share one process; standby leadership coordinates which subsystems run on which node.

### Router-only `fc-server`

```sh
FC_PLATFORM_ENABLED=false \
FC_ROUTER_ENABLED=true \
FLOWCATALYST_CONFIG_URL=https://platform.example.com/api/dispatch/router-config \
FC_ROUTER_CLIENT_ID=... FC_ROUTER_CLIENT_SECRET=... \
FC_ROUTER_PLATFORM_URL=https://platform.example.com \
FC_STANDBY_ENABLED=true \
FC_STANDBY_REDIS_URL=redis://redis:6379 \
FC_STANDBY_LOCK_KEY=fc:router:leader \
  ./fc-server
```

Use this when the router needs separate scaling, separate IAM credentials (SQS-only), or separate network isolation from the platform. It is Go's router task: with only the router enabled, `fc-server` connects to no database (all router state is in memory and SQS), serves the router surface under `FC_ROUTER_HTTP_PREFIX` (default `/router`) and Go's `/health` at the root. There is no separate router binary; the full contract is `docs/parity/router-env-vs-go.md`.

---

## Code references

- Entry point: `bin/fc-server/src/main.rs::start_router`, over `crates/fc-router/src/bootstrap/` (`RouterEnv`, `RouterRuntime`).
- Orchestrator: `crates/fc-router/src/manager.rs::QueueManager`.
- Pool: `crates/fc-router/src/pool.rs::ProcessPool`.
- Mediator: `crates/fc-router/src/mediator.rs::HttpMediator`.
- Circuit breaker: `crates/fc-router/src/circuit_breaker_registry.rs`.
- Config sync: `crates/fc-router/src/config_sync.rs::ConfigSyncService`.
- Lifecycle: `crates/fc-router/src/lifecycle.rs::LifecycleManager`.
- Standby: `crates/fc-router/src/standby.rs`.
- ALB traffic: `crates/fc-router/src/traffic.rs`.
- HTTP API: `crates/fc-router/src/api/`.
- Adaptive concurrency design: [adaptive-concurrency.md](adaptive-concurrency.md).
