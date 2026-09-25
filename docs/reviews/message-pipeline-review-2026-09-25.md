# Message pipeline review: where messages stall or get lost (2026-09-25)

A read-only review of `feat/functions` (= `main` at `f7bcb827`). It covered four layers:
- the queue backends (`fc-queue`)
- the router's pools and delivery path
- the router's manager, config and lifecycle
- the platform edges: outbox → ingest → scheduler → queue → router → `/api/dispatch/process`

Go (`../flowcatalyst-go`) is the reference: Rust must be a drop-in replacement. Items marked
**verified** were re-checked by hand after the reviewers reported them. The rest were confirmed by
the reviewers' reading, unless marked *plausible*. Paths are relative to `crates/` unless noted.

Production runs SQS, so the NATS, Postgres-queue and ActiveMQ findings matter only where those
backends are used (fc-dev, self-host).

## Critical: blocks or loses everything on its path

| # | Finding | Evidence | Go |
|---|---|---|---|
| C1 | **The production scheduler publishes to a no-op.** `fc-server` (the Dockerfile's binary) builds `NoopQueuePublisher`, which only logs. Jobs go PENDING → QUEUED → (15 min stale recovery) → PENDING forever, and no webhook is ever sent. `fc-queue` has no SQS publisher; the only one is in `bin/fc-router`. **Verified**; present since March. | `bin/fc-server/src/main.rs:901-925` | real SQS publisher |
| C2 | **Jobs created through the API are inserted QUEUED with a NULL `queued_at`.** The poller only reads PENDING, and stale recovery needs `queued_at < t`, so every job from `/api/dispatch-jobs[/batch]` (the SDK/outbox path) is stuck forever. **Verified.** | `shared/sdk_dispatch_jobs_api.rs:154`, `dispatch_job/api.rs:680,805`, `entity.rs:544` | inserts PENDING |
| C3 | **The Rust outbox deletes grouped rows before sending them.** "Success" means "queued in memory". Failures retry 3× with no backoff, then block the group in memory; a restart loses everything held. | `fc-outbox/src/enhanced_processor.rs:261-270`, `group_distributor.rs:92-101` | writes status after the outcome, re-queues retryables |
| C4 | **NATS drops messages for good after 10 releases.** `max_deliver: 10` and `max_ack_pending: 1000`; every capacity deferral or nack spends a delivery. Above 1000 ack-pending, the whole stream stops. **Verified.** | `fc-queue/src/nats.rs:168-169` | both −1 (owner ruling 2026-09-22) |
| C5 | **`POST /config/reload` stops every consumer and nothing restarts them.** The handler builds a config with `queues: vec![]`, the reconcile treats every queue as removed, and config sync won't reload an unchanged hash. Health stays UP. **Verified.** | `fc-router/src/api/config.rs:86-97`, `manager/reconcile.rs:306-316` | re-fetches from the config source |

## High

### Delivery and retry semantics
- **H1: `ack:false` with no delay is redelivered at once.** The platform returns it for every
  retryable webhook failure. The router maps it to a release with a 0s delay, so `max_retries`
  (3) is spent in seconds and the job is marked FAILED. The response parser's comment describes a
  "deferred backoff curve" the pool doesn't have. **Verified** (`fc-router/src/mediator/response.rs:78`,
  `pool.rs:268-278`). Go retries in-pipeline on a 5s→60s curve.
- **H2: `/api/dispatch/process` has no authentication and no claim.** Anyone who can reach it can
  trigger a delivery for any job id. Two copies processed at once both deliver. A retryable
  failure is retried twice over: `ack:false` (router redelivery) *and* PENDING (the scheduler
  republishes). **Verified** (`fc-platform/src/shared/dispatch_process_api.rs:56-58`). Go verifies
  the bearer, claims conditionally (`ClaimForDelivery`), always ACKs, and lets the poller own
  retries.
- **H3: per-group FIFO breaks.**
  - On 429/Deferred the router delivers the next message in the group past a released head
    (`pool.rs:1324-1338`).
  - Release cascades only within one batch (`failed_batch_groups`), not the whole group buffer.
  - Go re-fronts the head and takes the whole group buffer.
- **H4: the scheduler blocks a whole group on any past failure.** Any `FAILED`/`ERROR` row in the
  group's history holds it, even under NEXT_ON_ERROR, which owner ruling X-01 makes the default.
  Retries have no backoff because there is no `scheduled_for` filter. Go holds only BLOCK_ON_ERROR
  successors and backs off via `scheduled_for` (`fc-platform/src/scheduler/poller.rs:145-150`,
  `mod.rs:116`).
- **H5: the scheduler duplicates work and overwrites status.**
  - PENDING rows already queued in memory are re-read and appended again on every poll (unbounded
    memory, double publish).
  - The QUEUED update has no status guard (it overwrites COMPLETED/FAILED).
  - There is no `SKIP LOCKED` claim, so multiple replicas double-publish.
  - Go claims and marks QUEUED in one transaction (`scheduler/mod.rs:151-159,212-227,324-327`).
- **H6: the outbox resends dispatch-job and audit rows every second.** `mark_processing` only
  updates `type='EVENT'` rows, and SDK dispatch-job payloads carry no id, so duplicates follow.
  Rows in transient error states are never retried (`fc-outbox/src/repository.rs:142-145`;
  `fetch_recoverable_items` is never called).
- **H7: fan-out loses events when the first subscription load fails.** Up to 200 events are
  stamped `fanned_out_at` with no jobs. This can recur on every leadership acquisition. Go errors
  when the cache has never loaded (`fc-platform/.../event_fan_out.rs:73-87,129-136`).

### Router lifecycle
- **H8: shutdown and consumer restart stop consumers before draining.**
  - NATS `stop()` clears the pending messages, so acks for work that succeeded fail. After 120s
    the work is redelivered, and the 60s duplicate guard has already expired.
  - Callbacks hold the old consumer `Arc` (`manager/shutdown.rs:251`, `nats.rs:729`,
    `routing.rs:555`).
  - Go stops polling first, drains, then stops, and looks up the consumer per ack.
- **H9: dead consumers are never rebuilt.**
  - A NATS subscription that ends exits the loop as `Stopped`, which the watchdog ignores.
  - A factory failure logs and still stores the config hash, so it is never retried.
  - `restart_consumer` stops the old consumer before building the new one.
  - `nats.rs:379-395`; `manager/consumers.rs:255-265,472-531`; `reconcile.rs:360-377`;
    `config_sync.rs:533`. Go rebuilds before retiring and forgets the config on error.
- **H10: consumers waiting for capacity are treated as stalled.** Restart storms follow (a new
  PgPool each time), readiness goes 503 after 10 attempts, and the old tasks' exit erases the
  replacement's health entry (`consumers.rs:206-213,283-286`, `lifecycle.rs:200-242`).
- **H11: the pool-update API swaps in a new pool without draining the old one.** Ordered groups
  run concurrently at double the rate, and shutdown can't see the old pool
  (`api/mutations.rs:64`, `reconcile.rs:581-596`, `manager/mod.rs:730`). Go updates in place.
- **H12: the 15-minute in-flight reaper ages on `started_at`.** Slow work is delivered twice, and
  the original's callback then acks/nacks/clears the *new* copy's entry (`lifecycle.rs:97`,
  `stall.rs:53-66`, `routing.rs:83-110`). Go ages on last-seen, with a 2h ceiling and
  `EnsureTracked`.
- **H13: a hung Redis leaves two leaders.** There is no response timeout on the Redis connection,
  so `is_leader` stays true while the lock expires (`fc-standby/leader.rs`). *Plausible.*
- **H14: a standby never binds HTTP.** ECS health checks then replace it in a loop. A boot while
  the platform is down exits the process after about 7 minutes (`bin/fc-router/src/main.rs:143-199`).

## Medium
- **Postgres queue:**
  - Delayed nacks let a group's successor overtake its head, and `created_at` has no tie-break.
  - `FOR UPDATE SKIP LOCKED` over a windowed CTE likely locks nothing, so concurrent pollers
    double-deliver.
  - Port Go's query (`fc-queue/src/postgres.rs:211-237`).
- **Capacity:**
  - Capacity is gated globally ("any pool has room").
  - A batch is deferred whole when `available < len`, in a hot loop, which inflates SQS receive
    counts toward the DLQ (`manager/snapshots.rs:21-33`, `routing.rs:432-486`).
- **Drain panic leaks queue slots.** A panicking drain task leaks its slots, eventually NACKing
  everything. `&t[..20]` on a token can panic at a char boundary; the immediate path has no panic
  guard (`pool.rs:1137-1149`, `mediator.rs:300`).
- **Health report deadlock.** `get_health_report` re-takes a parking_lot read lock that is already
  held, so it can wedge the runtime (`health.rs:367-369 → 315-316`).
- **Health reaper blinds the watchdog.** It removes consumer health by config name, but health is
  keyed by identifier, so NATS and some SQS consumers become invisible to the watchdog
  (`lifecycle.rs:365-367`).
- **Poll errors look healthy.** A poll error still stamps the heartbeat, so a permanently failing
  consumer looks healthy (`consumers.rs:228-231`). Go stamps only on success.
- **Reconfigure doesn't wake waiters.** Consumers waiting for capacity stay parked
  (`manager/mod.rs:193`).
- **Detectors never run.** The stall detector and queue-health monitor are never started.
- **DEFAULT-POOL is dropped.** A reload drops DEFAULT-POOL, and the recreated one coexists with the
  old, splitting groups.
- **Queue config changes are missed.** A queue's URI change under the same name is ignored, and
  `visibility_timeout` isn't in the config hash.
- **A platform 500 is ACKed (R-57).** `/api/dispatch/process` returns 500 on a transient DB error,
  so the queue copy is dropped. The endpoint should answer 503 / `ack:false` on internal errors.
- **Poll query stalls behind held groups.** Held and paused groups are filtered after the `LIMIT`,
  so they can stall all dispatch.
- **Stale recovery is too short.** 15 min (Go uses 75), while the router timeout is 900s. A job
  whose attempt write fails stays PROCESSING forever.
- **Ingest 409 loses grouped outbox rows.** The outbox treats 409 as retryable, then blocks in
  memory and loses grouped rows (C3). SDK dispatch jobs carry no id, so a lost response
  duplicates.
- **Unclamped delays.** SQS nack/defer delays aren't clamped to 12h; the error is swallowed and
  the natural visibility timeout applies.

## Low
- Shutdown ordering (releasing leadership before polling stops) and `in_pipeline.clear()` after
  a drain timeout.
- Readiness goes 503 on warning volume (shared with Go).
- A concurrency decrease pauses the pool for up to 60s, and `capacity()` uses the configured
  value, not the live one.
- Oversized delays aren't clamped.
- Ack and nack calls have no timeout.
- A malformed 2xx body is ACKed (shared with Go).
- The in-flight reap runs before an immediate redelivery clears tracking.
- ActiveMQ never reconnects and ignores delays (not wired to a scheme).
- The SQLite claim loop can strand rows.
- Scheduled-job instances stay IN_FLIGHT after leadership loss, and a failed `mark_fired`
  fires the job twice.

## Checked and safe (summary)

The following were checked and hold:
- Callback `Drop` sends a fallback NACK.
- Semaphore permits and `SlotGuard` are RAII.
- There is no lost wake-up in the capacity gate.
- SQS receipt-handle refresh on redelivery works, as does the pending-delete re-ACK.
- Circuit-breaker transitions are sound.
- Fan-out claims and job inserts happen in one `SKIP LOCKED` transaction.
- X-01 parsing defaults to NEXT_ON_ERROR.
- A config-fetch failure keeps the last good config.
- No lock is held across `.await` in the consumers/queue_configs paths.
- The CancellationToken tree is sound.
- Malformed messages are deleted or terminated.
- Postgres/NATS poison rows are quarantined.

## Suggested order
1. **Before any cutover:**
   - C1 (a real SQS publisher in `fc-queue`, wired into `fc-server`)
   - C2 (insert PENDING, and recover a NULL `queued_at`)
   - C5 (reload re-fetches, and never empties the queues)
   - H2's authentication
2. **Port Go's scheduler/dispatch model as one unit, not piecemeal (C2, H1, H2, H4, H5, stale
   timings):**
   - claim and mark QUEUED in one transaction
   - hold only BLOCK_ON_ERROR successors
   - `scheduled_for` backoff
   - `/process` verifies the bearer, claims conditionally, always ACKs, and the poller owns
     retries
3. **Outbox (C3, H6):** status after the outcome, an atomic claim for every item type, retry
   with backoff.
4. **Router lifecycle (H8–H12, H14):**
   - stop polling → drain → stop
   - build a consumer before retiring the old one
   - retry failed consumers
   - keep waiting consumers out of the watchdog
   - update pools in place
   - reap on last-seen
   - bind HTTP first
5. **NATS defaults (C4)** and **Postgres claim query**, for the non-SQS backends.
6. The Medium/Low list.
