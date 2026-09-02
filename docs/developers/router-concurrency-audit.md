# Message router — concurrency primitive audit

## Status (2026-09-02)

`manager.rs` (~3.8k lines) has been split into `crates/fc-router/src/manager/`
— `mod.rs` (the `QueueManager` struct, its builder, construction/misc-getter
methods), `routing.rs` (`route_batch` + duplicate filtering + the strict
gate + `QueueMessageCallback`), `synth_pools.rs` (R-59 machinery),
`reconcile.rs` (`apply_config`/`reload_config`/`sync_queue_consumers` + the
pool-drain machinery), `shutdown.rs` (`start`/the in-pipeline
reaper/`shutdown`), `stall.rs` (stall detection/reporting + the stale-entry
reaper), `snapshots.rs` (pool stats, dashboard views, force-ack, in-flight
lookups), `consumers.rs` (the consumer poll loop + consumer
queries/health/restart). The `pub` surface is unchanged — every method stays
on `QueueManager` regardless of which file its `impl` block lives in.

Of the three concrete simplifications below, (2) active/draining
unification and (3) `self_ref` removal are **done**. (1) the rate-limiter
`ArcSwap` is still open (it's in `pool.rs`, out of scope for the pass that
did (2)/(3)). Of the larger simplifications, (4) `pending_delete_broker_ids`
style alignment is **done** (converted to `DashMap`). (5) and (6) — both
`pool.rs` — are still open. See the field census below, refreshed against
current code (it had also drifted from a few features added since this
audit was first written: R-59 synth pools, R-13/R-16 strict routing, R-26
leadership — none of that changes the audit's conclusions, it's just fields
the original table predates).

## Goal

A primitive-by-primitive inventory of `crates/fc-router/src/manager.rs` and
`crates/fc-router/src/pool.rs`, tagging each shared-state primitive as
**essential**, **defensive**, or **optimization** so the question of
"keep Rust vs port to TS" can be made against concrete state rather than
gut feel.

- **Essential** — required by an architectural feature (FIFO ordering,
  dedup, circuit breaker, rate limiting, reconfig draining).
- **Defensive** — safety net for known failure modes (panic recovery,
  receipt-handle expiry, stuck-message reaper).
- **Optimization** — performance shortcut that could be a simpler primitive
  at a measurable cost (DashMap vs RwLock<HashMap>, ArcSwap vs RwLock).

The TS port doesn't need most of these. Single-threaded event loop with
`Map`/`Set` covers what we use DashMap, RwLock, Mutex, AtomicXxx, and
Arc/Weak for here. The Rust verbosity is the cost of multi-threaded
concurrency with the borrow checker enforcing soundness — most of it
disappears in TS.

---

## QueueManager (`manager/`)

| # | Field | Type | Tag | Notes |
|---|-------|------|-----|-------|
| 1 | `in_pipeline` | `Arc<DashMap<String, InFlightMessage>>` | essential | Dedup on SQS redelivery. Without it `filter_duplicates` can't swap in a fresh receipt handle when the same broker message reappears. |
| 2 | `app_message_to_pipeline_key` | `Arc<DashMap<String, String>>` | essential | Secondary index — app message id → pipeline key — used by callback drop to clean both maps in one shot. Could be derived but lookup is hot-path. |
| 3 | `pools` | `DashMap<String, PoolEntry>` | essential | **DONE (consolidation #2).** Replaces the old `pools`/`draining_pools` pair — one map, `PoolEntry { pool: Arc<ProcessPool>, state: PoolState::{Active,Draining} }`. Routing (`route_batch`/`group_by_pool`/`has_pool_capacity`/`get_pool`/`ensure_fallback_pool`/`get_or_create_pool`) matches `Active` only; `all_pools`/`cleanup_draining_pools`/`shutdown` traverse every entry. A `DashMap` can only hold one entry per key, so a code removed and re-added before its predecessor finishes draining — the coexistence case the old two-map design allowed for free — can't have both under one key: the new `Active` insert would displace the map's only reference to the still-draining predecessor. Resolved via row 3b (`orphaned_draining`), not left as a narrowing: every insert site that can displace an entry (`insert_active_pool`, `insert_draining_pool`) checks `DashMap::insert`'s returned previous value and routes a displaced `Draining` entry there instead of dropping it. Removal from `pools` itself is identity-checked (`remove_if` on state + `Arc::ptr_eq`) so a predecessor finishing late can never delete the new occupant. |
| 3b | `orphaned_draining` | `parking_lot::Mutex<Vec<Arc<ProcessPool>>>` | essential | Added during this pass to close the gap row 3 would otherwise have left open. Holds pools displaced from `pools` while still `Draining` (see row 3) — both directions are handled: an `Active` insert landing on a `Draining` slot orphans the predecessor (`insert_active_pool`), and the symmetric race — `begin_pool_drain`'s `Draining` insert landing after a concurrent creator already claimed the slot with a fresh `Active` entry (realistic case: `ensure_fallback_pool` racing `evict_idle_synth_pools`'s drain of the same synth code) — orphans itself instead of clobbering the newer pool (`insert_draining_pool`). `all_pools()` chains this in, so the operator dashboard (`mediating_snapshot`/`blocked_groups`/`group_flush_snapshots`) keeps showing a displaced predecessor's buffered/in-flight work; `shutdown()` chains it into the pool list it explicitly drains/releases/waits/shuts down, so the router specification's shutdown MUST (release every pool's buffered remainder, never abandon it — `docs/router-specification.md` §5.3) holds for an orphan too, not just for `pools`' own entries. `cleanup_draining_pools` sweeps it the same way it sweeps `Draining` map entries, as a belt-and-braces backstop alongside each orphan's own watcher task (spawned back when it first started draining), which independently calls `pool.shutdown()` regardless of which list references it — a double `shutdown()` call is an expected, idempotent race, not a bug. Plain `Mutex<Vec<_>>` rather than `DashMap`: identified by `Arc` identity, not a key, and this path is rare (only the coexistence edge case) next to the hot-path maps elsewhere in this struct. Pinned by `manager_tests.rs::displaced_draining_predecessor_stays_visible_and_gets_released_at_shutdown`. |
| 4 | `synth_pools` | `DashMap<String, SynthPoolState>` | essential | R-59 idle tracker for synthesised per-client fallback pools — added since this audit was first written; wasn't in the original table. |
| 5 | `consumers` | `RwLock<HashMap<String, Arc<dyn QueueConsumer>>>` | essential | Active queue consumers. No `draining_consumers` equivalent exists or is needed — verified (see `sync_queue_consumers`'s "X-11" comment): a phased-out consumer's still-buffered messages hold their own `Arc<dyn QueueConsumer>` clone from route time, independent of this map. |
| 6 | `pool_configs` | `RwLock<HashMap<String, PoolConfig>>` | essential | Last-applied pool configs, for diff during `sync_pools`; also the reload-serialisation lock. |
| 7 | `queue_configs` | `RwLock<HashMap<String, QueueConfig>>` | essential | Same diff role for queues. |
| 8 | `consumer_factory` | `Option<Arc<dyn ConsumerFactory>>` | essential | Used by `sync_queue_consumers` to create new consumers when a queue is hot-added. |
| 9 | `mediator_factory` | `Arc<dyn Fn(...) -> Arc<dyn Mediator>>` | essential | Boxed-factory idiom, replaces the old `MediatorSource` enum this row used to describe. Prod builds a fresh `HttpMediator` per pool; tests inject a shared mock. |
| 10 | `running` | `AtomicBool` | essential | Start/stop lifecycle. Cross-task read of "are we shutting down". |
| 11 | `shutdown` | `CancellationToken` | essential | Replaces the old `shutdown_tx: broadcast::Sender<()>` this row used to describe — level-triggered, so a task spawned after shutdown began still observes cancellation instantly (no "subscribed too late" window the broadcast channel had). |
| 12 | `batch_counter` | `AtomicU64` | essential | Monotonic batch id for tracing & batch-group dedup. |
| 13 | `pending_delete_broker_ids` | `Arc<DashMap<String, Instant>>` | **defensive** | **DONE (consolidation #4/"larger simplification #4").** Converted from `Mutex<HashMap>` to `DashMap` for consistency — audited every lock site (`route_batch`'s batch scan, `QueueMessageCallback::ack`'s insert, `reap_stale_entries`' retain sweep) and confirmed none depends on one lock covering every key at once; each is keyed by a distinct `broker_message_id` with no cross-key atomicity requirement. "ACK succeeded but the receipt handle had already expired" recovery: when the same broker message id reappears, ack it immediately. |
| 14 | `stall_warned` | `Mutex<HashSet<String>>` | essential | X-04 once-per-episode stall-warning dedup — added since this audit was first written. |
| 15 | `warning_service` | `Arc<WarningService>` | essential | Plumbing. |
| 16 | `health_service` | `Option<Arc<HealthService>>` | essential | Plumbing. |
| 17 | `strict_routing` | `AtomicBool` | essential | R-13/R-16 `FC_ROUTER_STRICT_ROUTING` gate — added since this audit was first written. |
| 18 | `is_leader` | `AtomicBool` | essential | R-26/R-33/R-34 standby leadership flag — added since this audit was first written. |

`self_ref` (`parking_lot::RwLock<Option<Weak<Self>>>`) — **DONE (consolidation #3), field deleted.** Every call site that used to need it (`sync_queue_consumers` and everything downstream of it) now takes `self: &Arc<Self>` / `self: Arc<Self>`, the same refactor already applied to `init_self_ref`/`start`/`spawn_in_pipeline_reaper` before this audit was written. No `Weak<Self>`/`self_ref` reference remains anywhere in `manager/`.

**Plain non-concurrent fields** (config values, not primitives):
`default_pool_code`, `max_pools`, `pool_warning_threshold`, `stall_config`.

### Patterns

- **Active-map + draining-map** (row 3) — **resolved.** One map, tagged
  value, makes the state machine explicit instead of
  implicit-via-which-map-it's-in. The `consumers`/`draining_consumers` half
  of this pattern never actually existed (row 5) — already verified as a
  non-issue before this pass.
- **Inconsistent map primitive** (row 13) — **resolved.** Converted to
  `DashMap`; the `Mutex<HashMap>`'s single-lock semantics weren't
  load-bearing (see row 13's note), so consistency won over "document why
  it's different".
- **Weak self-reference** — **resolved**, field deleted.

---

## ProcessPool (`pool.rs`)

| # | Field | Type | Tag | Notes |
|---|-------|------|-----|-------|
| 1 | `mediator` | `Arc<dyn Mediator>` | essential | Plumbing. |
| 2 | `concurrency` | `AtomicU32` | essential | Hot-swap value reflecting the live limit (config can change at runtime via `update_concurrency`). |
| 3 | `semaphore` | `Arc<Semaphore>` | essential | Pool-wide concurrency cap. Cloned into every spawned task to await a permit. |
| 4 | `group_handlers` | `Arc<DashMap<Arc<str>, parking_lot::Mutex<MessageGroupHandler>>>` | essential | Per-group FIFO queue + processing flag. The Mutex inside the DashMap value guards the VecDeque + the `processing` boolean — that's the canonical "lightweight drain task" pattern. Nested primitive is hard to read but each layer earns its keep: DashMap so groups don't contend on a single lock; Mutex on each value because handler state is brief and never `.await`s. |
| 5 | `in_flight_groups` | `DashSet<Arc<str>>` | **defensive/optimization** | Tracks "this group is currently holding a semaphore permit, with a drain task in flight." Overlaps with `group_handlers[id].lock().processing`: `processing == true` means "a drain task exists"; `in_flight_groups.contains(id)` means "and it's holding a permit right now." Used by the panic guard to release `active_workers` correctly. **Consolidation candidate**: fold into the `MessageGroupHandler` as a `holding_permit: bool` and remove the `DashSet`. |
| 6 | `failed_batch_groups` | `Arc<DashSet<BatchGroupKey>>` | essential | BlockOnError dispatch mode: when any message in a batch+group fails, every later message in the same batch+group is fast-nacked instead of delivered out of order. The set is the cascade marker. |
| 7 | `batch_group_message_count` | `Arc<DashMap<BatchGroupKey, AtomicU32>>` | essential | Reference count for the cascade marker — entry is removed from `failed_batch_groups` when count hits 0. Without it the set would leak entries forever. |
| 8 | `rate_limiter` | `Arc<RwLock<Option<Arc<RateLimiter>>>>` | essential | **The worst-reading primitive in the file**: triple-nested. The shape is "shareable handle to a lock guarding an optional shareable rate limiter." It's correct — the outer Arc shares to spawned tasks, the RwLock allows hot-swap, the Option allows "no rate limit", the inner Arc lets a snapshot survive past a swap. **Cleaner**: `arc_swap::ArcSwap<Option<Arc<RateLimiter>>>` — one primitive, lock-free read on the hot path. |
| 9 | `rate_limit_per_minute` | `Arc<RwLock<Option<u32>>>` | **optimization** | Separately tracked u32 used only to check "did the rate limit value change?" during `update_rate_limit`. Could be replaced by reading the current value off the `ArcSwap` from (8) without a second lock. Saves a field. |
| 10 | `running` | `AtomicBool` | essential | Start/stop lifecycle, same role as the manager's. |
| 11 | `queue_size` | `Arc<AtomicU32>` | essential | Queue capacity check on `submit()`, cloned into spawned tasks for decrement. |
| 12 | `active_workers` | `Arc<AtomicU32>` | essential | Metrics + capacity tracking. |
| 13 | `metrics_collector` | `Arc<PoolMetricsCollector>` | essential | Plumbing. |
| 14 | `circuit_breaker_registry` | `Arc<CircuitBreakerRegistry>` | essential | Per-endpoint CB shared across pools. |
| 15 | `warning_service` | `Arc<WarningService>` | essential | Plumbing. |

**MessageGroupHandler** (inside the `Mutex` in row 4):

| Field | Type | Tag | Notes |
|-------|------|-----|-------|
| `high_priority` | `VecDeque<PoolTask>` | essential | FIFO bucket A. |
| `regular` | `VecDeque<PoolTask>` | essential | FIFO bucket B. |
| `processing` | `bool` | essential | "Is a drain task currently active?" — the gate that prevents two drain tasks racing on the same group. |

### Patterns

- **Triple-nested hot-swap** (row 8) — most-cited example of "this is
  hard to audit". `arc_swap::ArcSwap` is the idiomatic replacement.
- **Counter sprawl** (rows 11, 12, plus 2) — three separate
  `Arc<AtomicU32>` / `AtomicU32` that all describe the same pool's
  state. **Consolidation candidate**: one `Arc<PoolCounters>` with the
  three atomics inline, cloned once into each spawned task instead of
  three Arc clones per spawn.
- **State on parallel tracks** (rows 4, 5) — handler-state vs
  in-flight-set. Overlap was documented above.
- **Side state for change detection** (row 9 alongside 8) — the only
  reason `rate_limit_per_minute` exists is to compare against itself.
  Comparison can be done on the live primitive.

---

## What this means in numbers

- **QueueManager**: 19 primitive-bearing fields (post-consolidation; see
  the table above — this count now also includes five fields the original
  audit predates: `synth_pools`, `stall_warned`, `strict_routing`,
  `is_leader`, and `orphaned_draining` — the last one added by this pass
  itself, to close a visibility/shutdown gap the map unification would
  otherwise have opened, not a pre-existing field the original table
  missed).
  - Essential: 18
  - Defensive: 1 (`pending_delete_broker_ids`, now a `DashMap`)
  - Defensive/optimization (removable): 0 — the two candidates this audit
    named (`pools`/`draining_pools` unification, `self_ref`) are both done.
- **ProcessPool**: 15 primitive-bearing fields + 3 inside the per-group
  Mutex — **unchanged, out of scope for this pass** (the manager/pool.rs
  split named in this doc's "recommended next step" only covered
  `manager.rs`).
  - Essential: 12
  - Optimization: 2 (`rate_limit_per_minute` once `ArcSwap` is in,
    counter consolidation removes 2 Arc-wraps but not the atomics)
  - Defensive/optimization: 1 (`in_flight_groups` foldable into the
    handler)

**Post-consolidation count so far**: 19 (manager, done) + 15 (pool, open) =
34 primitive-bearing fields — up from the original 32 despite one field
being *net* removed from the manager side (two removed by the map
unification, one added back by `orphaned_draining`), because five fields
the manager gained since this audit was first written (R-59/R-13/R-26
features, plus this pass's own `orphaned_draining`) outweigh the two the
consolidation removed. The *manager*'s own count is the meaningful
comparison: 17 (original, undercounting real fields at the time) → 19 now,
net of −2 (unified map, `self_ref` deleted), +4 (real fields the original
table simply didn't list), +1 (`orphaned_draining`, the coexistence-case
fix). None of this changes the pool.rs side of the "~25 realistic" estimate
below, which still stands as the target once (1), (5), (6) are done. Field
*count* going up while the manager still reads more explicitly than before
isn't a contradiction — `orphaned_draining` trades one field for a
correctly-modelled edge case instead of a silently-dropped one; that's the
kind of primitive this audit tags essential, not optimization.

### Three concrete simplifications worth doing regardless of port-vs-keep

1. **`ArcSwap` for the rate limiter** (`pool.rs` row 8 + 9). One line of
   types changes from triple-nested to a single primitive; lock-free on
   the hot path. Removes one whole field. ~30 minutes. **Still open** —
   `pool.rs`, out of scope for this pass.
2. **Active/draining unification** in QueueManager (rows 3+4, 5+6). One
   map keyed by code, value is `(Arc<X>, PoolStatus)`. Makes
   "is this pool draining?" a value check, not a map check. Removes two
   fields. ~1 hour. **DONE** — see row 3 above (`pools: DashMap<String,
   PoolEntry>`); the `consumers`/`draining_consumers` half was already a
   non-issue before this pass (no such field ever existed).
3. **Delete `self_ref`** (manager row 17) by promoting the affected call
   sites to `&Arc<Self>` / `Arc<Self>`. Same pattern we already used for
   `init_self_ref`, `start`, `spawn_in_pipeline_reaper`. Removes the
   field, removes the "must call init_self_ref before start" footgun,
   removes the upgrade-on-use codepath. ~30 minutes. **DONE** — the field,
   the RwLock, and every `Weak<Self>` upgrade site are gone; every caller
   that needed it now takes `self: &Arc<Self>`/`self: Arc<Self>`.

Manager-side total: both applicable items done, zero fields removable by
this route left in `manager/`. Item (1) remains for a future `pool.rs`
pass. None of these changed behaviour — `cargo test -p fc-router -p
fc-common` and the mediation conformance suite are unchanged before/after.

### Three larger simplifications worth considering

4. **`pending_delete_broker_ids` style alignment** — move to DashMap to
   match the rest of the manager, or document why the consolidated lock
   matters. Trivial change, comprehension win. **DONE** — converted to
   `DashMap<String, Instant>`; audited every lock site first and confirmed
   none depends on cross-key atomicity (see row 13 above), so consistency
   won over keeping the single lock.
5. **Counter consolidation** in `ProcessPool` — bundle `queue_size`,
   `active_workers` into one `PoolCounters` struct behind one Arc.
   Touches every `tokio::spawn` site in `pool.rs` (clone count drops
   from N to 1 per spawn). ~1 hour. **Still open** — `pool.rs`, out of
   scope for this pass.
6. **`in_flight_groups` fold** — move the bit into `MessageGroupHandler`
   as `holding_permit: bool`. Removes a field, removes a `DashSet`,
   makes the per-group state live in one place. ~1 hour. **Still open** —
   `pool.rs`, out of scope for this pass.

---

## What this audit *doesn't* tell you

- **Whether you can read the result.** It tells you what would remain to
  be read. The number going from 32 to ~25 (or ~22 with the larger pass)
  is a real but bounded improvement.
- **Whether silent failure modes lurk.** The audit confirms each
  primitive earns its keep but it cannot certify absence of bugs.
  Staging being clean is the evidence we have on that.
- **Whether TS would be easier in practice.** The TS port replaces this
  whole concurrency story with `Map` + `Set` + `Promise.all` + a single
  event loop. Auditability goes up sharply; throughput ceiling goes
  down (single-thread CPU bound, no per-worker mediation parallelism).

## Recommended next step, port-or-keep aside

(2) and (3) are done, plus the `manager/` module split and (4)
(`pending_delete_broker_ids`). (1), (5), and (6) remain — all three are in
`pool.rs`, not touched by this pass. Do those as the next PR. Re-read
`manager/` and `pool.rs` after. If at that point the answer to "could I
find a stuck message at 3am here" feels closer to "yes," the Rust path is
viable. If not, the audit doubles as the spec for the TS preservation work
— every "essential" entry above is a feature that must survive the port.
