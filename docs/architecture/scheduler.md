# Dispatch Scheduler

The scheduler is the bridge between Postgres (`msg_dispatch_jobs` rows in status `PENDING`) and the queue (SQS or its dev equivalent). It polls for ready jobs, applies a paused-connection filter, orders within message groups, and publishes lightweight pointers for the message router to pick up. Source: `crates/fc-platform/src/scheduler/`, gated by `FC_SCHEDULER_ENABLED` inside `fc-server`.

Historically this was a separate `fc-scheduler` crate and a `fc-scheduler-server` binary. Both have been removed; the scheduler now ships inside `fc-platform` and runs as a subsystem of `fc-server`.

---

## Position in the system

```
Event ingest                          fc-stream::event_fan_out
   POST /api/events/batch             reads msg_events, applies
   POST /api/dispatch-jobs/batch ◄──  subscriptions, inserts
                                      msg_dispatch_jobs rows (status=PENDING)
                                              │
                                              ▼
                                    ┌─────────────────────────────┐
                                    │  Dispatch Scheduler          │
                                    │  (fc-platform::scheduler)    │
                                    │                              │
                                    │  poller → group dispatcher   │
                                    │       → SQS publish          │
                                    │       → status = QUEUED      │
                                    │                              │
                                    │  + stale_recovery sub-loop   │
                                    └────────────┬────────────────┘
                                                 │  MessagePointer
                                                 ▼
                                           SQS FIFO queue
                                                 │
                                                 ▼
                                            Message Router
```

The scheduler **only** transitions `PENDING → QUEUED`. The router (via the dispatch-process endpoint) is responsible for `QUEUED → PROCESSING → COMPLETED | FAILED | EXPIRED | CANCELLED`.

---

## Module layout

| File | Owns |
|---|---|
| `mod.rs` | `DispatchScheduler` orchestrator, `SchedulerConfig`, `MessageGroupDispatcher`, `MessageGroupQueue`, `SchedulerJobRow` (lightweight projection of `msg_dispatch_jobs`). |
| `poller.rs` | `PendingJobPoller` (SQL query), `PausedConnectionCache` (refreshes every 60 s). |
| `dispatcher.rs` | `JobDispatcher` — builds the `fc_common::Message` payload and calls `QueuePublisher`. |
| `auth.rs` | `DispatchAuthService` — issues short-lived HMAC tokens that the router includes in its callback to the platform's dispatch-process endpoint. |
| `stale_recovery.rs` | `StaleQueuedJobPoller` — finds jobs stuck in `QUEUED` past a threshold and resets them to `PENDING`. |

`DispatchScheduler::start()` spawns three concurrent loops:

1. **Pending poller** — runs `poll_interval` (default 5 s).
2. **Stale recovery** — runs every 60 s, independent of the main poll cadence.
3. **Connection-cache refresher** — runs every 60 s.

All three respect a shared shutdown channel; `stop()` flips it and the loops exit.

---

## The poller

`PendingJobPoller::find_pending_jobs()`:

```sql
SELECT id, message_group, dispatch_pool_id, status, mode,
       target_url, payload, sequence, created_at, updated_at,
       queued_at, last_error, subscription_id
FROM msg_dispatch_jobs
WHERE status = 'PENDING'
ORDER BY message_group ASC NULLS LAST,
         sequence ASC,
         created_at ASC
LIMIT $1
```

Key points:

- **No `FOR UPDATE SKIP LOCKED`.** Concurrency is bounded by leader election (one scheduler runs per cluster) and the in-process semaphore inside `MessageGroupDispatcher`. Adding the row-lock would buy nothing here and would block stale-recovery against the same partitions.
- **Batch size** comes from `SchedulerConfig::batch_size`, default 200. Tunable via `FC_SCHEDULER_BATCH_SIZE`.
- **Ordering matters.** Sorting by `(message_group, sequence, created_at)` is what gives the rest of the pipeline FIFO ordering within a group — the in-memory `MessageGroupQueue` relies on it.

After the SQL fetch, two filters apply:

### Paused-connection filter

`PausedConnectionCache` caches the IDs of subscriptions whose connection is in status `PAUSED`. The cache is `Arc<RwLock<HashSet<String>>>` refreshed every 60 s by:

```sql
SELECT s.id
FROM msg_subscriptions s
JOIN msg_connections   c ON c.id = s.connection_id
WHERE c.status = 'PAUSED'
```

Jobs whose `subscription_id` appears in the set are dropped from the batch and stay `PENDING`. They'll be picked up automatically once the connection is un-paused — no manual requeue needed.

Pause is the way operators stop traffic to a misbehaving endpoint without losing work. Compare to circuit breakers in the router: pause is operator-initiated and persistent; CBs are automatic and transient.

### Blocked message-group filter

For dispatch modes `BLOCK_ON_ERROR` and `NEXT_ON_ERROR`, if any prior job in the group is in `FAILED` or `ERROR` status, downstream jobs in that group are skipped until either:

- The failed job transitions to a non-error state (operator retries, the receiver ACKs eventually), or
- A `Cancel`/`Ignore` action is taken on the failed job through the admin UI.

`Immediate` mode skips this filter entirely — order doesn't matter, so a poison message doesn't block its peers.

---

## MessageGroupDispatcher — per-group sequencing

`mod.rs::MessageGroupDispatcher`. Two pieces of state:

- **`semaphore: Arc<Semaphore>`** — capacity = `max_concurrent_groups` (default 10). Cap on the number of distinct groups dispatching simultaneously.
- **`inner: Arc<Mutex<HashMap<String, MessageGroupQueue>>>`** — per-group FIFO queue.

Flow:

```
submit_jobs(group, jobs):
    push jobs into the group's queue, sorted by (sequence, created_at)
    if the group is idle:
        spawn dispatch task
            acquire semaphore permit
            loop:
                pop next job
                if none: release permit, exit
                dispatch single job (below)
```

So each group has at most one dispatch in flight at a time (FIFO), and the cluster has at most `max_concurrent_groups` dispatches in flight overall (cap). Increasing `max_concurrent_groups` raises throughput on workloads with many groups but does nothing for workloads dominated by a single hot group.

### dispatch_single_job

```
let message = JobDispatcher::build_message(job);
let publish_result = queue_publisher.publish(message).await;

match publish_result {
    Ok(_) | Err(DeduplicationCollision) => {
        UPDATE msg_dispatch_jobs
        SET status = 'QUEUED', queued_at = NOW(), updated_at = NOW()
        WHERE id = $1 AND created_at = $2
    }
    Err(other) => {
        leave row as PENDING; loop will retry next tick
    }
}
```

The `WHERE id = $1 AND created_at = $2` predicate looks redundant but isn't — `msg_dispatch_jobs` is RANGE-partitioned by `created_at`. Constraining on both columns lets Postgres prune partitions instead of scanning the whole hierarchy.

`DeduplicationCollision` is treated as success: it means SQS already accepted this exact message ID within the 5-minute dedup window, so the publish is effectively idempotent. We mark `QUEUED` and move on.

---

## MessagePointer payload

The `Message` published to SQS is intentionally minimal (`crates/fc-common::Message`):

```rust
Message {
    id: dispatch_job_id,                 // primary key for the router
    pool_code: dispatch_pool_id,         // routes to the right pool
    auth_token: None,                    // unused at SQS layer
    signing_secret: None,                // unused at SQS layer
    mediation_type: MediationType::HTTP,
    mediation_target: config.processing_endpoint,  // e.g. http://platform:3000/api/dispatch/process
    message_group_id: job.message_group,
    high_priority: false,
    dispatch_mode: job.dispatch_mode(),
}
```

Important: `mediation_target` is **the platform's dispatch-process endpoint**, not the customer's webhook URL. The customer's URL lives in `msg_dispatch_jobs.target_url` and is loaded by the dispatch-process handler when the router calls back. This indirection has three benefits:

1. **SQS messages stay tiny** — kilobytes of payload don't live in the queue.
2. **Single source of truth** — the platform decides retries, attempt accounting, signing-secret rotation. The router doesn't need to be in the loop for any of that.
3. **Auth gate** — the dispatch-process endpoint authenticates the router with a short-lived HMAC token (see `auth.rs`), so a leaked SQS message can't be used to spam the platform with status updates.

`message_group_id` is propagated so the router can preserve FIFO within the same group across its pool processing. `dispatch_mode` is propagated so the router knows whether to use IMMEDIATE-mode concurrency or sequential ordered drain.

---

## Stale recovery

`stale_recovery.rs::StaleQueuedJobPoller::recover_stale_jobs()`:

```sql
UPDATE msg_dispatch_jobs
SET status = 'PENDING', queued_at = NULL, updated_at = NOW()
WHERE status = 'QUEUED' AND queued_at < $1
```

Threshold = `now() - stale_threshold` (default 15 min). Runs every 60 s.

Why this exists: the router could die *after* the scheduler marks `QUEUED` but *before* it actually processes the SQS message — or SQS could lose the message (rare, but possible with FIFO content-based dedup misuse), or the router could be stuck waiting on a wedged downstream. In all those cases, the row sits in `QUEUED` forever without recovery.

Stale recovery is the "leak-stopper". It re-publishes to SQS via the normal poller (because the row goes back to `PENDING`), and the router's deduplication logic (the `app_message_to_pipeline_key` check in `QueueManager`) prevents double-processing if the original SQS message turns out to still be alive.

Setting `stale_threshold` too low causes spurious re-publishes for slow-but-still-working deliveries. The default 15 min comfortably exceeds the router's 15-minute request timeout — a delivery still in flight at 15 minutes is almost certainly stuck.

---

## Status transitions, end to end

```
[ msg_events row inserted ]
          │
          │  fc-stream::event_fan_out
          ▼
[ msg_dispatch_jobs row inserted, status = PENDING ]
          │
          │  scheduler::poller → SQS publish ok
          ▼
[ status = QUEUED, queued_at = NOW() ]
          │
          │  router consumes from SQS, calls /api/dispatch/process
          ▼
[ status = PROCESSING, attempt row inserted ]
          │
          │  HTTP POST to msg_dispatch_jobs.target_url
          ▼
     ┌────┴────────────────────────────┬─────────────┬─────────────┐
     │                                 │             │             │
  2xx ack=true               5xx / connection      4xx        429 / ack=false
     │                                 │             │             │
     ▼                                 ▼             ▼             ▼
COMPLETED                        FAILED → next   FAILED      back to QUEUED
                                 retry per       (terminal)  via SQS NACK
                                 retry policy
```

Compute path responsibilities:

- **fc-stream** writes `PENDING`.
- **fc-platform::scheduler** writes `QUEUED`.
- **fc-platform::dispatch_process_api** (called by the router) writes `PROCESSING`, `COMPLETED`, `FAILED`, attempt rows.
- **Operator actions** (cancel, resend) write `CANCELLED` and create fresh `PENDING` rows for resends.

None of the lifecycle transitions go through the UoW / domain-events pipeline — they're infrastructure traffic that would swamp the event log if every status flip emitted a `DispatchJobStatusChanged` event. The CLAUDE.md "Infrastructure exceptions" rule covers this explicitly. Operator-initiated cancellations *do* go through UoW (because they're an audited human action).

---

## Environment variables

Read by `bin/fc-server/src/main.rs::load_scheduler_config`:

| Variable | Alias | Default | Description |
|---|---|---|---|
| `FC_SCHEDULER_ENABLED` | `DISPATCH_SCHEDULER_ENABLED` | `false` | Master toggle inside fc-server |
| `FLOWCATALYST_SCHEDULER_ENABLED` | — | `true` | Scheduler-internal toggle (only `true`/`false` are understood; `FC_SCHEDULER_ENABLED` is the subsystem switch) |
| `FLOWCATALYST_SCHEDULER_POLL_INTERVAL_MS` | — | `100` | Pending-job poll cadence |
| `FLOWCATALYST_SCHEDULER_DISPATCH_MODE` | — | `immediate` | Default dispatch mode (case-insensitive; unknown values fall back to `NEXT_ON_ERROR`) |
| `FC_SCHEDULER_MAX_CONCURRENT_GROUPS` | — | `10` | Cap on parallel group dispatch |
| `FC_SCHEDULER_DEFAULT_POOL_CODE` | — | `DISPATCH-POOL` | Pool used when `dispatch_pool_id` is null |
| `FC_SCHEDULER_PROCESSING_ENDPOINT` | `DISPATCH_SCHEDULER_PROCESSING_ENDPOINT` | `http://localhost:8080/api/dispatch/process` | Where the router calls back |

Batch size (100) and the stale-job threshold (15 minutes) are fixed.

Plus the cluster-level standby vars (`FC_STANDBY_ENABLED`, `FC_STANDBY_REDIS_URL`, `FC_STANDBY_LOCK_KEY`) — see [high-availability.md](../operations/high-availability.md).

---

## Standby gating

The scheduler is a singleton inside the active region. `bin/fc-server/src/main.rs::spawn_scheduler` wraps the entire poll loop in a `watch::Receiver<bool>` guard. On boot or leadership transition:

- **Standby disabled:** scheduler runs unconditionally.
- **Standby enabled, leader:** scheduler runs.
- **Standby enabled, not leader:** scheduler's tokio task blocks on `active_rx.changed()` until leadership is granted. The HTTP server still runs on standby nodes for health checks; just the scheduler loop is gated.
- **Leadership lost mid-flight:** `stop()` is called; in-flight dispatches finish (the semaphore drains naturally) and no new poll runs until leadership is regained.

This means at most one scheduler is publishing to SQS at any moment, which is what makes the "no FOR UPDATE SKIP LOCKED" decision safe.

---

## Why not pull-from-SQS-directly?

A reasonable design question: why does the scheduler exist at all? Why not let `event_fan_out` write directly to SQS?

Three reasons:

1. **Replay and audit.** Every dispatch job is a row in Postgres before it's ever sent. We can answer "what jobs were generated for event X" deterministically, replay a missed delivery from the database, and produce an audit log of every status transition.
2. **Operator control.** Pause a connection, cancel a job, retry a failed one — all of those are SQL updates on `msg_dispatch_jobs`. If SQS were the source of truth, every one of those operations would need a delete-and-republish dance.
3. **Backpressure absorption.** A surge of inbound events doesn't immediately translate to an SQS burst. Rows accumulate in `PENDING` and drain at the scheduler's pace. SQS publish costs (and downstream HTTP load) are decoupled from ingest rate.

The trade-off is one extra hop and one extra database write per dispatch. For the volume we target this is comfortably worth it.

---

## Scheduled-job scheduler (separate from dispatch scheduler)

A second scheduler runs alongside, in `fc-platform/src/scheduled_job/scheduler/`, gated by the same standby leader. It evaluates cron expressions on `msg_scheduled_jobs` rows and:

1. Inserts a `msg_scheduled_job_instances` row when a cron expression fires.
2. POSTs a webhook to the configured `target_url` (or emits an event via the platform API, depending on the job's mode).
3. Records stdout / errors in `msg_scheduled_job_instance_logs`.

This pipeline does **not** flow through `msg_dispatch_jobs` — it's a separate scheduling concern, intentionally kept distinct. The "scheduled-task-emits-an-event-then-the-event-flows-through-the-normal-dispatch-pipeline" pattern described in [architecture-direction.md](architecture-direction.md) is the long-term intent for keeping the two coupled in spirit but decoupled in implementation.

---

## Code references

- Entry point: `bin/fc-server/src/main.rs::spawn_scheduler` and `::load_scheduler_config`.
- Orchestrator: `crates/fc-platform/src/scheduler/mod.rs::DispatchScheduler`.
- Poller: `crates/fc-platform/src/scheduler/poller.rs`.
- Per-group dispatch: `crates/fc-platform/src/scheduler/mod.rs::MessageGroupDispatcher`.
- Pointer building: `crates/fc-platform/src/scheduler/dispatcher.rs::JobDispatcher::dispatch`.
- Stale recovery: `crates/fc-platform/src/scheduler/stale_recovery.rs`.
- HMAC auth: `crates/fc-platform/src/scheduler/auth.rs::DispatchAuthService`.
- Receiver side: `crates/fc-platform/src/shared/dispatch_process_api.rs`.
- Scheduled jobs (separate): `crates/fc-platform/src/scheduled_job/scheduler/`.
