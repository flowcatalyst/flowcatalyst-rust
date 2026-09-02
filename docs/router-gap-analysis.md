# fc-router / fc-queue vs owner rulings — gap analysis

_Verified 2026-09-02 against `feat/owner-rulings` (post tokio-lifecycle work, 140/140
tests green). Companion to [`owner-questions.md`](./owner-questions.md) (the decision
ledger — IDs below are its). Every row was corroborated by a direct read of the cited
file:line. Reference implementation for most ruled behaviour: the Go repo
(`../flowcatalyst-go`, branch `feat/owner-rulings`), which implemented the rulings first._

## Already correct (no work)

| ID | Evidence |
|---|---|
| R-43 auth bypass | `api/mod.rs:454-643` — public routes are a structurally separate sub-router with no auth layer; immune to mount-prefix bugs by construction. (Delete the vestigial `is_public_path`, `auth.rs:790`.) |
| R-56 instance id | `standby.rs:27,64-71` — real per-process id, never the lock key. |
| Route-time dedup | `manager.rs:1224-1301` `filter_duplicates` before `pool.submit()` (Rust is the origin Go ported). |
| SQS pendingDelete guard | `sqs.rs:11-47`, 15-min TTL. |
| Receipt-handle freshness | `manager.rs:1240-1246,1290-1295`. |
| Whole-group release (mechanism) | `failed_batch_groups` cascade releases buffered + unsubmitted siblings; scope is per poll batch (acceptable; tighten later). |
| Per-host HTTP slots | `http_pool.rs` — ahead of Go (dynamic grow/shrink, warning on saturation). |

## Gaps by lane

### Mediator lane (RA — in progress)
- **A-04** `MediationOutcome::success()` hardcodes 200 (`fc-common/src/lib.rs:468-473`).
- **R-05/A-06** reqwest follows redirects (no `Policy::none()`, `mediator/inner.rs:36-50`); no 3xx branch in `response.rs` at all.
- **R-57** only 501 special-cased; 500/505+ retry uniformly with 502/503/504 (`response.rs:150-181`). Ruled: 502/503/504 hold; every other 5xx = permanent reject.
- **A-03** in-call retry loop (`retry.rs:36-39`) + scattered pool nack delays = two uncoordinated mechanisms; collapse to one named policy (mediator half first, pool half in RB).
- **flushGroup wire half**: `MediationResponse` parses only ack/delaySeconds — add the field + outcome carry.
- **Conformance runner**: none exists repo-wide. Build it against `../flowcatalyst-javalin/conformance/mediation-outcomes.json`.

### Queue backends lane (RC — in progress)
- **R-17/A-07** `postgres.rs:133` / `sqlite.rs:150`: decode failure `?`-aborts the whole poll batch; poison row re-claims forever; no `queue_messages_failed` table exists.
- **R-19** `nats.rs:266-267`: dedup id includes `consumer_sequence` → every redelivery looks new (at-most-once). Ack addressing (`:213`) already correct.

### Pool/breaker lane (RB — after RA lands; single owner for all of these: they converge on the duplicated match blocks `pool.rs:453-490` and `~723-800` + `failed_batch_groups`)
- **A-27** extract `disposition_of(outcome, attempts, mode)` as a pure function (Go's `pool.go` `DispositionOf` is the target shape); deduplicate the two inline match blocks.
- **A-01 router half** `DispatchMode::requires_ordering()` is the only mode read — NEXT_ON_ERROR and BLOCK_ON_ERROR behave identically (cascade-NACK). Ruled minimum now: NEXT_ON_ERROR continues past a failed, surfaced head; BLOCK_ON_ERROR keeps the NACK-cascade (do NOT ship the ACK-siblings branch until the platform settled/reaper half exists in fc-platform — ledger A-01).
- **22b** an `ack:false` deferral records a breaker failure (`pool.rs:457-458,726-727`) — must be breaker-neutral (like RateLimited).
- **R-06/A-11** `ErrorConfig => record_success` credits pre-flight rejections as breaker successes; no CONFIGURATION warnings reach the store (`mediator.rs:198-220` only `tracing::warn!`). Needs a pre-flight distinction on the outcome.
- **R-12** breaker keyed by raw full URL (`pool.rs:443,701`) — key by origin+path, query stripped.
- **A-05/R-52/R-53 flushGroup pool half**: registry + suppression ACK path + suppressed metric + monitoring exposure (wire field lands in RA).
- **R-59 note**: Rust has NO per-client `{identifier}-DEFAULT-POOL` synthesis at all (single global fallback, `manager.rs:440,1313-1330`) — the feature itself is absent, not just its eviction. Decide with the owner whether to add it (manager lane if so).

### Manager/routing + lifecycle lane (RD — after RA; one owner, manager.rs is one file)
- **X-01/A-09** `#[serde(default)]` on `pool_code`/`dispatch_mode` with `Default = Immediate` and lenient `from_str → Immediate` (`fc-common/src/lib.rs:59-116`) — wrong default; ruled fallback is NEXT_ON_ERROR, and absence at the router wire is malformed under the strict gate.
- **R-13/R-16** empty pool code routes silently (unknown-but-nonempty warns; empty doesn't, `manager.rs:1322-1341`); absent group id → shared `"__DEFAULT__"` group (`manager.rs:1365-1367`) — the exact anti-pattern R-13 deleted. Implement the `FC_ROUTER_STRICT_ROUTING`-gated ACK+notice like Go (default off).
- **R-26 half** consumers keep polling and delivering after leadership LOSS — `spawn_leadership_monitor` transitions (`standby.rs:302-331`) are wired to nothing. Live correctness bug.
- **R-49** shutdown drains the whole buffered backlog (`pool.rs:930-933`) and on 60s timeout abandons in-flight work mid-call (`manager.rs:~1650-1720`) — ruled: finish the in-hand message, release the remainder to the broker.
- **R-30** no per-source last-known-good cache (`config_sync.rs:231-288`); partial failure tears down that source's pools; only `tracing::warn!`.
- **R-33** reload handler has no leadership check. **R-34** leadership gating is a one-time startup wait in `bin/fc-router/src/main.rs`; no stop/recreate on loss→regain.

### Health/warnings lane (RF — last; small)
- **A-08** `warning.rs:36` auto-acknowledge default 8h → 1h.
- **R-36** wrong in both directions: pool success rate gates readiness (`health.rs:245-330`) but must not; `set_consumer_running` (`health.rs:165`) has zero production call sites so consumer liveness never does (also blocks 22f stall detection).
- **X-04** store + severity-filtered notifier exist (`warning.rs:76-114`, default Warn) but: no `Stall`/`CircuitBreaker` category variants; `RateLimiting`/`QueueConnectivity` never constructed; stall check (`manager.rs:1864-1920`) bypasses the store; env var is `NOTIFICATION_MIN_SEVERITY` not `FC_NOTIFY_MIN_SEVERITY`; no INFO-specific TTL.
