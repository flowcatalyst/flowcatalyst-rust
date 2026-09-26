# Stream Processor

The stream processor is the read-model + fan-out worker. It owns four polling loops, all running in one process:

1. **Event fan-out** — turns `msg_events` rows into `msg_dispatch_jobs` rows by matching subscriptions.
2. **Event projection** — denormalises `msg_events` into `msg_events_read` for fast API queries.
3. **Dispatch-job projection** — denormalises `msg_dispatch_jobs` into `msg_dispatch_jobs_read`.
4. **Partition manager** — creates next-month partitions and drops expired ones for the seven partitioned tables.

Source: `crates/fc-stream/`. It runs inside `fc-server` with `FC_STREAM_PROCESSOR_ENABLED=true` (on its own, set `FC_PLATFORM_ENABLED=false` too), and inside `fc-dev`. There is no separate stream-processor binary.

The older `docs/stream-processor.md` described a MongoDB change-stream architecture. That's gone. The current implementation is poll-based against Postgres with `FOR UPDATE SKIP LOCKED` claims, no change streams.

---

## Position in the system

```
┌────────────────────────────────────────────┐
│   Platform API                             │
│                                            │
│   POST /api/events/batch                   │
│   POST /api/dispatch-jobs/batch            │
│   POST /api/audit-logs/batch               │
│   plus every UseCase write                 │
└──────────────────┬─────────────────────────┘
                   │  INSERT INTO msg_events / msg_dispatch_jobs (write models)
                   ▼
        ╔════════════════════════════════════════════════════╗
        ║   Postgres                                         ║
        ║                                                    ║
        ║   msg_events            (write,  partitioned)      ║
        ║   msg_events_read       (read,   partitioned)      ║
        ║   msg_dispatch_jobs     (write,  partitioned)      ║
        ║   msg_dispatch_jobs_read(read,   partitioned)      ║
        ║   msg_dispatch_job_attempts (partitioned)          ║
        ║   msg_scheduled_job_instances (partitioned)        ║
        ║   msg_scheduled_job_instance_logs (partitioned)    ║
        ╚════════════════════════════════════════════════════╝
                   ▲                            ▲
                   │ event_projection           │ partition_manager
                   │ dispatch_job_projection    │ (create/drop monthly)
                   │ event_fan_out              │
                   │                            │
        ┌──────────┴────────────────────────────┴──────────┐
        │   fc-stream::StreamProcessor                     │
        │   (one process, four loops, leader-gated)        │
        └──────────────────────────────────────────────────┘
```

Why four loops in one process? They all want a small dedicated Postgres pool (4 connections), they all run on the same standby leader, and they all benefit from being co-located so the partition manager can ensure the projection loops have somewhere to write. Splitting into four binaries would be operational overhead with no upside.

---

## Module layout

| File | Owns |
|---|---|
| `lib.rs` | `start_stream_processor(pool, config)` — boots all four services, returns a `StreamProcessorHandle` with a shared shutdown channel. |
| `config.rs` | `StreamProcessorConfig` — per-loop enable flags + batch sizes. |
| `event_projection.rs` | `EventProjectionService` — `msg_events` → `msg_events_read`. |
| `dispatch_job_projection.rs` | `DispatchJobProjectionService` — `msg_dispatch_jobs` → `msg_dispatch_jobs_read`. |
| `event_fan_out.rs` | `EventFanOutService` — events → dispatch jobs. |
| `partition_manager.rs` | `PartitionManagerService` — monthly partition lifecycle for seven tables. |
| `health.rs` | `StreamHealthService` — aggregated last-tick timestamps + counters. |

Each service runs as its own `tokio::spawn`'d task with adaptive sleep:

- Batch fully drained → sleep `idle_interval` (default 1 s).
- Batch capped → poll again immediately.

---

## Event fan-out

The most consequential of the four loops. It's how subscriptions actually fire.

### What it does

For every event that hasn't been fanned out yet, find every active subscription that matches the event type and client, and insert one `msg_dispatch_jobs` row per match. All in one transaction with the stamp that marks the event as fanned-out.

### Why it's split from event projection

Event projection writes `msg_events_read` (denormalised columns for API queries — `application`, `subdomain`, `aggregate`, `event_name`, etc.). Fan-out writes `msg_dispatch_jobs`. The two consumers want different progress markers:

- `msg_events.projected_at` — projection's marker.
- `msg_events.fanned_out_at` — fan-out's marker.

If you tried to share one column, then an admin who wanted to rebuild the read model by clearing the marker would inadvertently trigger refanning, doubling every customer's webhook traffic. Keeping the markers independent makes each loop safely re-runnable.

There are matching partial indexes on each column (`WHERE projected_at IS NULL`, `WHERE fanned_out_at IS NULL`) so each loop's claim query stays cheap regardless of how big the events table gets.

### Claim and fan algorithm

```rust
loop {
    let mut tx = pool.begin().await?;

    let events = sqlx::query(
        "SELECT id, type, source, client_id, data, ...
         FROM msg_events
         WHERE fanned_out_at IS NULL
         ORDER BY created_at
         LIMIT $1
         FOR UPDATE SKIP LOCKED"
    ).fetch_all(&mut tx).await?;

    if events.is_empty() { tx.commit().await?; break; }

    // subscriptions are loaded once, cached, refreshed every 5 s
    let subscriptions = subscription_cache.get();

    let mut jobs = Vec::new();
    for event in &events {
        for sub in subscriptions {
            if !sub.matches_event_type(&event.event_type) { continue; }
            if !sub.matches_client(event.client_id.as_deref()) { continue; }
            jobs.push(NewJobRow::build(event, sub));
        }
    }

    if !jobs.is_empty() {
        // single INSERT with UNNEST — never one INSERT per job
        insert_dispatch_jobs_tx(&mut tx, &jobs).await?;
    }

    // stamp every claimed event
    sqlx::query(
        "UPDATE msg_events SET fanned_out_at = NOW() WHERE id = ANY($1)"
    ).bind(event_ids).execute(&mut tx).await?;

    tx.commit().await?;
}
```

Several things matter here:

- **`FOR UPDATE SKIP LOCKED`** — safe with multiple processes (though in practice only the leader runs). A second process that managed to start during failover would just claim different rows.
- **Atomic stamp + insert.** If the transaction rolls back, neither the jobs nor the stamp are written, and the events are reclaimed on the next iteration. At-least-once is the contract; downstream deduplication (the dispatch job ID is deterministic for an `(event, subscription)` pair) makes it effectively exactly-once.
- **Bulk insert.** The CLAUDE.md "use UNNEST not loops" rule is load-bearing here — a busy fan-out doing 1000 events × 5 subscriptions = 5000 INSERTs per cycle would melt Postgres. One `INSERT ... SELECT FROM UNNEST(...)` is one round trip.

### Subscription cache

Subscriptions are loaded once and cached. Refresh interval: `fan_out_subscription_refresh_secs` (default 5 s). When a new subscription is created through the platform API:

1. Operator clicks "save" in the UI.
2. UoW commits to `msg_subscriptions`, emits `SubscriptionCreated`.
3. Within 5 s the next cache refresh picks it up.
4. The next event matching its pattern fans out to it.

So new subscriptions become live within a handful of seconds, not instantly. This is acceptable for the use case (no one needs sub-second subscription registration) and avoids cache-invalidation chatter from the platform process to the stream process.

### Event-type matching

`Subscription::matches_event_type(event_type: &str) -> bool`:

- Exact match — `orders.created` matches subscription pattern `orders.created`.
- Wildcard at the end — `orders.*` matches `orders.created`, `orders.cancelled`, `orders.fulfilled`.
- Wildcards in segments — `orders.*.created` matches `orders.standard.created` and `orders.express.created`.

The pattern format is determined by the subscription's `event_type` field. There's no JSONPath filter on payload at the fan-out layer — payload filtering happens downstream in the receiver.

---

## Event projection

`event_projection.rs::EventProjectionService`. Polls `msg_events` for rows where `projected_at IS NULL`, denormalises into `msg_events_read`, stamps `projected_at`.

The read table has the same partition scheme as the source table and additional columns:

| Column | Source |
|---|---|
| `application` | parsed from `event_type` (segment 0) |
| `subdomain`   | parsed from `event_type` (segment 1) |
| `aggregate`   | parsed from `event_type` (segment 2) |
| `event_name`  | parsed from `event_type` (segment 3) |
| `correlation_id`, `causation_id` | extracted from event metadata |
| Indexed copies of common search predicates |

API list endpoints (`/bff/events`, `/api/events`) always read from `msg_events_read`, never from `msg_events`. This keeps the write model lean (no indexes that aren't needed for the projection claim itself).

Configuration:

| Var | Default |
|---|---|
| `FC_STREAM_EVENTS_ENABLED` | `true` |
| `FC_STREAM_EVENTS_BATCH_SIZE` | `100` |

---

## Dispatch-job projection

`dispatch_job_projection.rs::DispatchJobProjectionService`. Same pattern, different source/target:

`msg_dispatch_jobs` → `msg_dispatch_jobs_read`. The read model adds:

- Denormalised connection name, subscription name, event type code.
- A `terminal` boolean derived from status (so list queries can filter for "open work" without a CASE).
- Indexes for filter dropdowns (status, client_id, application, etc.) — note that on the write table these indexes are deliberately absent (see `migrations/015_dispatch_jobs_write_indexes.sql`), so transactional paths stay fast.

Configuration:

| Var | Default |
|---|---|
| `FC_STREAM_DISPATCH_JOBS_ENABLED` | `true` |
| `FC_STREAM_DISPATCH_JOBS_BATCH_SIZE` | `100` |

---

## Partition manager

`partition_manager.rs::PartitionManagerService`. Maintains monthly RANGE partitions for the high-volume tables. Detailed write-up in [partitioning.md](partitioning.md); brief summary here:

### Tables managed

`PARTITIONED_PARENTS` (constant list in `partition_manager.rs`):

```
msg_events
msg_events_read
msg_dispatch_jobs
msg_dispatch_jobs_read
msg_dispatch_job_attempts
msg_scheduled_job_instances
msg_scheduled_job_instance_logs
```

### Tick

Once on startup, then every 24 h:

1. **Create forward partitions.** For each parent, ensure partitions exist for `now-1mo` through `now+months_forward` (default `months_forward = 3`). `CREATE TABLE IF NOT EXISTS` is idempotent.
2. **Drop expired partitions.** Any partition whose range is older than `retention_days` (default 90) gets `DROP TABLE IF EXISTS`. Constant-time cleanup vs a `DELETE` over millions of rows.

### Why pure Rust, no pg_partman

RDS allowlists change on AWS's schedule, not ours. `pg_partman_bgw` got dropped from the PG 18 allowlist between two RDS releases. Same code running in dev (embedded Postgres in fc-dev) and prod (RDS / self-hosted PG) means partition-related bugs (missing partition key in a UNIQUE, query missing `created_at` in `WHERE`) fail in dev before reaching prod.

Configuration:

| Var | Default |
|---|---|
| `FC_STREAM_PARTITION_MANAGER_ENABLED` | `true` |

`months_forward` and `retention_days` aren't currently env-var-exposed — they're set in `PartitionManagerConfig::default()`. To change in production, edit `spawn_stream_processor` in `bin/fc-server/src/main.rs`. The defaults (90-day retention, 3 months ahead) are appropriate for most deployments; if you need different retention, that's the place to override.

---

## Configuration, all together

`StreamProcessorConfig` (read by `fc-server`):

| Variable | Default | Description |
|---|---|---|
| `FC_STREAM_EVENTS_ENABLED` | `true` | Toggle event projection |
| `FC_STREAM_EVENTS_BATCH_SIZE` | `100` | Events projected per cycle |
| `FC_STREAM_DISPATCH_JOBS_ENABLED` | `true` | Toggle dispatch-job projection |
| `FC_STREAM_DISPATCH_JOBS_BATCH_SIZE` | `100` | Jobs projected per cycle |
| `FC_STREAM_FAN_OUT_ENABLED` | `true` | Toggle event-to-job fan-out |
| `FC_STREAM_FAN_OUT_BATCH_SIZE` | `200` | Events fanned per cycle |
| `FC_STREAM_FAN_OUT_SUBS_REFRESH_SECS` | `5` | Subscription cache TTL |
| `FC_STREAM_PARTITION_MANAGER_ENABLED` | `true` | Toggle partition maintenance |

Disabling any one of these is a useful operational lever:

- Turning off projection (events / dispatch jobs) lets you reduce DB load if the read model is briefly behind acceptable (UI lists will lag).
- Turning off fan-out is the kill switch for new dispatch jobs — events keep landing but nothing dispatches. Useful during incidents.
- Turning off partition manager during a maintenance window where you're managing partitions manually.

---

## Standby gating and shutdown

`bin/fc-server/src/main.rs::spawn_stream_processor` creates a small dedicated Postgres pool (4 connections, separate from the main API pool — projection loops shouldn't contend with HTTP traffic), wires in the optional secret-rotation refresher (so RDS Secrets Manager rotation doesn't silently kill the stream pool while the API pool keeps working — both pools cache their connect options independently), and gates the whole bundle on `watch::Receiver<bool>` leadership.

Two-state loop:

```
loop {
    wait until is_leader
    let handle = start_stream_processor(pool, config);
    wait until !is_leader OR shutdown
    handle.stop().await
}
```

Stopping the handle drops the shared `tokio::sync::broadcast::Receiver`, which causes every loop to exit on its next iteration. In-flight transactions either commit or rolled back via the connection's `Drop`; no work is lost (claimed events stay claimed for the next leader).

---

## Code references

- Entry point: `bin/fc-server/src/main.rs::spawn_stream_processor` (its own 4-connection pool, with the secret refresh registered on it).
- Orchestrator: `crates/fc-stream/src/lib.rs::start_stream_processor`.
- Fan-out: `crates/fc-stream/src/event_fan_out.rs`.
- Projections: `crates/fc-stream/src/event_projection.rs`, `dispatch_job_projection.rs`.
- Partition maintenance: `crates/fc-stream/src/partition_manager.rs`.
- Migrations introducing the tables: `migrations/004_messaging_tables.sql`, `migrations/012_projection_columns.sql`, `migrations/015_dispatch_jobs_write_indexes.sql`, `migrations/019_partition_messaging_tables.sql`.
