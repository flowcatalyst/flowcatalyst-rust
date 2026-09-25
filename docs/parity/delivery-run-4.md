# Delivery parity run 4 (Go vs Rust)

Run `20260925-204709-5df8j` · Go `73a6918` · Rust `1fab1a9f` (branch `feat/delivery-fix`; the two commits
after it, `532349f3` and `8f3e4f59`, change a test allowlist and a return type only) · full run, both sides,
Rust on a Go-migrated database (the cutover path). Harness: `harness/delivery` (see its README).

**Result: 16 PASS, 1 ACCEPTED, exit 0, no stale `expected-diffs.json` entry.** Run 3 was 12 PASS, 5 DIFF.

## Verdicts

| Scenario | Run 3 | Run 4 | Go | Rust |
|---|---|---|---|---|
| plain-events | PASS | **PASS** | 20/20 accepted | 20/20 accepted |
| dispatch-jobs-api | PASS | **PASS** | 10/10 | 10/10 |
| ack-false-no-delay | PASS | **PASS** | 5/5, 10 deliveries | 5/5, 10 deliveries |
| ack-false-delay | PASS | **PASS** | 5/5, 10 deliveries | 5/5, 10 deliveries |
| rate-limited-429 | PASS | **PASS** | 5/5, 15 deliveries | 5/5, 15 deliveries |
| server-error-then-success | PASS | **PASS** | 5/5, 15 deliveries | 5/5, 15 deliveries |
| permanent-errors | PASS | **PASS** | 0/6, 10 deliveries | 0/6, 10 deliveries |
| connection-refused | PASS | **PASS** | 0/3 | 0/3 |
| block-on-error-group | PASS | **PASS** | 6/6, 8 deliveries | 6/6, 8 deliveries |
| next-on-error-group | PASS | **PASS** | 6/6, 8 deliveries | 6/6, 8 deliveries |
| ordered-group-20 | PASS | **PASS** | 20/20 | 20/20 |
| burst-pool-capacity | PASS | **PASS** | 60/60 | 60/60 |
| router-restart | DIFF | **PASS** | 40/40, in order | 40/40, in order |
| worker-restart | DIFF | **ACCEPTED** | 25/40 — 15 stranded QUEUED (Go defect, cited #31) | 40/40, in order |
| platform-down | DIFF | **PASS** | 20/20, in order | 20/20, in order, nothing left PROCESSING |
| outbox-events | DIFF | **PASS** | 10/10, outbox empty | 10/10, outbox empty |
| slow-target-timeout | DIFF | **PASS** | 3/3, 6 deliveries, `attempt_count` 1 | 3/3, 6 deliveries, `attempt_count` 1 |

## What was wrong, and what changed

### platform-down — Rust bug (fixed)

Run 3: the platform SIGTERMed 2 s in stopped in 55 ms and lost 2 jobs to PROCESSING, with 11 FIFO overtakes.

- **Root cause.** `fc-server` *aborted* its API task on the signal. Two `/api/dispatch/process` calls were in flight;
  each had already sent its webhook (the target saw it, but its answer went nowhere), so the job was PROCESSING
  with no outcome. The router's retry 30 s later hit "already claimed" and was acked away: stuck for good, and
  the rest of each group was delivered past it.
- **Go.** `server.Run` cancels the subsystems, then `apiSrv.Shutdown` with 30 s: in-flight requests finish (Go
  stopped in 150–300 ms, nothing lost). Go's `/process` acks a copy that loses the claim, and nothing recovers
  a PROCESSING job left by a dead attempt: `stale_recovery.go` only reverts QUEUED, and the `dispatchjob`
  reaper only resets siblings of a FAILED BLOCK_ON_ERROR head (45 min). Go never shows it here only because
  it drains.
- **Fix.**
  - `fc-server` drains both HTTP servers for up to 30 s (Go's value); run 4 stopped in 126 ms with the
    in-flight delivery completed.
  - `/process` gives a claim a lease: the job's timeout (capped at the 120 s client ceiling) + 30 s.
    - A copy that loses the claim to a PROCESSING job inside its lease answers
      `200 {"ack":false,"delaySeconds":<rest of lease>}`. The router keeps the message and holds its group.
    - Past the lease, the copy takes the claim over (compare-and-set on the claim time) and delivers, at-least-once.
    - The delivery runs on its own task, so a router hanging up no longer cancels an attempt half way.
    - A failed outcome write answers `503 ack:false`, so the message is kept for that recovery.
    - Internal errors stay 503, never 500.
- **Second defect, found once the first was fixed.** Seq 8 of both groups arrived after 9 and 10. While the
  platform was down, the Rust router released each group with the head nacked for 30 s and the untried
  siblings for 10 s, so the siblings surfaced first.
  - Go nacks head and siblings with the same (zero) delay.
  - SQS FIFO would keep the order anyway: a held head blocks its group. LocalStack's long poll does not block it
    (checked by hand: a waiting long poll receives a group's second message while the first is in flight).
  - Siblings are now held back no shorter than their head, which keeps the order on any broker.

### router-restart — Rust bug (fixed)

Run 3: g3 `[10,1,2,3,4,5,7,8,9,6]`, g4 `[10,1,…,9]`, 5 FIFO overtakes. Two independent causes:

1. **`10` first (not a restart effect at all, delivered at 1.4 s).**
   - Event ingest stored a whole batch with one `NOW()`, so every event of a group tied on `created_at`.
   - The fan-out reads `ORDER BY created_at`, and the scheduler orders a group by the job's inherited `created_at`.
     The group went out in whatever order the fan-out read it.
   - Go stamps each event with its own `time.Now()` (`event.New`).
   - Fix: each event keeps its own construction time, strictly increasing in batch order (µs).
2. **One group's head 30 s late.**
   - Reproduced with debug logs. On SIGTERM, `stop_polling` dropped the poll loop's in-flight SQS long poll. The
     broker side stayed open and handed the first released group remainder (R-49 releases it at shutdown) to a
     caller that was gone.
   - That message stayed invisible for the 30 s visibility timeout, and the replacement router delivered its
     successors first. On real SQS the group would instead stall 30 s.
   - Go drains its whole buffer within `DrainTimeout` before flushing (run 4: Go stopped in 1.5 s, delivering its
     buffer), so nothing is released while its dropped poll is open.
   - Fix, keeping R-49's release-the-remainder: a stopped poll loop finishes its receive (bounded by the poll
     timeout) and nacks anything it caught straight back. Shutdown waits for those receives, within the drain
     budget, whenever it released something.

Run 4: both sides deliver every group 1–10.

### worker-restart — Go defect, timing-dependent (cited, #31); Rust hardened

Run 3: Go 38/40, run 4: Go 25/40; the missing jobs are QUEUED with an empty queue.

- **Go's defect.** Go commits a claim QUEUED and only then publishes it (`dispatcher.go`: "A crash between the
  caller's commit and this publish leaves rows QUEUED for stale recovery").
  - Its SQS publisher never puts two jobs of one group in one `SendMessageBatch`, and the claim is ordered by
    group, so this scenario's 40 jobs in 4 groups go out as ~40 sequential calls.
  - A SIGKILL inside that publish strands the rest QUEUED until `StaleAfter` = 75 min, which is not
    env-configurable. That is far past the 180 s settle window.
  - Whether it shows depends on where the kill lands. The two targeted runs between run 3 and run 4 passed.
- **Not a harness config problem.** No Go setting shortens `StaleAfter`.
- **Rust had the identical window.** Rust uses the same claim→commit→publish order and the same chunking. It only
  passed run 3 because its pipeline published about 0.8 s earlier.
- **Fix.** The Rust poller now publishes while its claim is still locked and uncommitted, then marks only the
  published ids QUEUED.
  - A worker that dies mid-publish rolls its whole claim back to PENDING, and the next poll republishes it.
  - Jobs it had already published are published twice. The second copy is a redundant queue message, not a second
    delivery: `/process` claims before delivering.
  - A `/process` call arriving before the commit waits on the row lock.
  - `StaleAfter` stays 75 min, so the owner's duplicate-storm concern is untouched.
- **Harness changes.**
  - A side's broken invariant can now be cited per side (`invariant/go/loss`); a Go citation never excuses Rust.
  - An entry may be `intermittent` (never stale), because this Go defect only shows when the kill lands in its window.
  - `expected-diffs.json` cites #31 for `invariant/go/loss`, `accepted`, `lost`, `attempts`, `groupOrder/*`,
    `jobStatus` and `settled` on worker-restart.

### outbox-events and slow-target-timeout — harness defect (fixed)

- **Root cause.** Run 3 was started with a relative `--report`, so the JWT key path the harness handed Go
  (`FC_JWT_SIGNING_KEY_PATH`) resolved against the Go side's own working directory.
  - Go logged "unreadable, falling back" and signed with an ephemeral key.
  - Every bearer the harness held (the API caller, the outbox processor's token) died with the platform
    restart in `platform-down`.
  - Both later scenarios failed on `401 invalid_token`: the outbox rows were stuck blocked, and events/batch
    was refused.
  - Rust had generated a key pair at the same relative path under its own directory and reloaded it, so it
    survived.
- **Not Go defects.** Go's outbox processor config and Go's subscription timeout handling are fine.
- **Fix.** The harness makes the run directory absolute.
- **Result.** Both sides now pass both scenarios identically. On slow-target-timeout both show the duplicate by
  design: 2 deliveries, 1 acceptance, `attempt_count` 1.

## Left open

- **Router-config shim.** The Rust platform still serves no `/api/dispatch/router-config`, so the Rust router
  reads the harness shim. That route is with `feat/go-routes`.
- **Serial publish, both sides.** The claim is group-ordered and a chunk never holds two jobs of one group, so a
  claim of G groups × N jobs is published as ~G×N single-message calls in Go and Rust alike. That is a
  throughput cost, and it made Go's window above ~1 s wide. Interleaving groups across chunks would keep the
  per-group order with far fewer calls. Not changed here.
- **LocalStack FIFO fidelity.** LocalStack's long poll ignores a group's in-flight lock, so any path that makes a
  group's later message visible before its head reorders the group here where SQS would not. Both Rust fixes
  above hold on either broker.
- **Stale recovery.** A PROCESSING job whose queue message is gone is still only recovered by stale recovery
  (75 min, Rust-only). The message is gone only when `/process` could not answer at all and the router acked
  anyway, which no Rust path does now.
