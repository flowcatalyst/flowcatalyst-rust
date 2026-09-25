# Delivery parity harness — run 1 (2026-09-25)

The first full run of `harness/delivery` (owner decisions #28/#29): 17 scenarios driven through
Go `flowcatalyst-go@73a6918` and Rust `feat/harness-delivery` (Rust binaries built from
`4d39bf78`; the report header's `5364c6f3` is the harness commit, which changes no Rust code),
each in the production topology (platform + stream processor, a scheduler worker, a separate
router, an outbox processor) on one Postgres and one LocalStack SQS. The Rust side adopted a
Go-migrated database (the cutover path). How to run and read it: `harness/delivery/README.md`.

## Outcome

- **Go: 17/17 scenarios hold every invariant.** No loss, no duplicate acceptance, BLOCK_ON_ERROR
  group order kept, pool concurrency honoured, every event delivery validly signed, through a
  router restart, a worker SIGKILL and 5 s of platform downtime.
- **Rust: delivers nothing (0 of 255 stimuli accepted in 15 scenarios that expect delivery).**
  Every scenario is `DIFF`; the harness locates where each one stopped. Rust fails *before* the
  known no-op scheduler publisher (review C1) on every path, so C1 itself is not yet observable:

| Path | Where Rust stops | Evidence in the report |
|---|---|---|
| events (`/api/events/batch`) → fan-out | **New finding.** The fan-out's job insert names `msg_dispatch_jobs.connection_id`, a column no migration creates — neither Rust's own (`migrations/009_p0_alignment.sql` adds it to `msg_subscriptions` only, and says "when migrated to PG" for jobs) nor Go's. Every fan-out cycle fails; events are stored and never fanned out. Also reproduced with `--rust-schema own` (a database Rust migrated itself). | "N events stored, none fanned out"; 409 × `column "connection_id" of relation "msg_dispatch_jobs" does not exist` in the platform log (`crates/fc-stream/src/event_fan_out.rs:526-531`) |
| dispatch jobs (`/api/dispatch-jobs/batch`) | **C2**: inserted `QUEUED` with no message sent; the poller reads only `PENDING`. | `dispatch-jobs-api`: all 10 jobs `QUEUED`, queue empty |
| SDK outbox → outbox processor | **New finding.** `fc-outbox-processor` exits at start: `init_schema` sends the table + three index DDL statements as one prepared statement ("cannot insert multiple commands into a prepared statement", `crates/fc-outbox/src/postgres.rs:333-364`). Rows stay at status 0. | `outbox-events`: 10 rows left at status 0, "outbox exited during boot" under Stacks |
| router config | Rust's platform serves no `/api/dispatch/router-config` (404), so the Rust router reads a harness-served shim in Go's shape (noted under Stacks). Not a delivery failure by itself; listed in the owner decisions' "missing Go routes". | Stacks → rust |

Once fan-out, C2 and the outbox are fixed, the same run will reach C1 (no-op publisher), and the
diagnosis will read "all N jobs are QUEUED, the queue is empty" for the event scenarios too. After
C1, the retry/ordering scenarios (H1, H3, H4, H5, H8–H12) become the live comparison.

## What Go does (the reference this harness now pins)

Read off the Go column; each is a row a Rust fix will be compared against.

- **Direct dispatch jobs are delivered unsigned** even when the job names `serviceAccountId`:
  Go resolves the signer from the subscription, its connection, or the application named by the
  code's first segment, never from the job itself. Event deliveries (via a subscription naming the
  signer) are signed and carry `authorization`, `x-flowcatalyst-signature`,
  `x-flowcatalyst-timestamp`, `x-dispatch-job-id`, `x-event-type`.
- **`ack:false` without `delaySeconds`**: the next attempt comes 20–45 s later (Go's 30 s default
  reschedule), no retry budget spent (`attempt_count` stays 0).
- **`ack:false, delaySeconds:3`**: next attempt 3–10 s later; no budget spent.
- **429 with `Retry-After: 2`** → next attempt 1–10 s later; **429 without** → 10–45 s (30 s
  default); no budget spent.
- **503, 503, 200** (maxRetries 5): gaps 1–10 s then 10–45 s (Go's 5 s / 15 s backoff),
  `attempt_count` 2, `COMPLETED`.
- **400 and 501** (maxRetries 2): 2 attempts, then `FAILED`; **401**: 1 attempt, `FAILED` at
  once. Go's `attempt_count` counts failed attempts before the last: 1 and 0.
- **Connection refused** (maxRetries 2): `FAILED`, `attempt_count` 1.
- **BLOCK_ON_ERROR, seq 2 failing twice**: acceptance order 1-6, no FIFO overtakes.
- **NEXT_ON_ERROR, the same script**: acceptance order `1,3-6,2`: the group keeps flowing past the
  failing member (4 overtakes). This is Go's NEXT_ON_ERROR contract and the X-01 default.
- **Ordered group of 20**: strictly 1-20, one at a time.
- **Pool concurrency 2, 60 messages at 300 ms**: never more than 2 in flight at the target.
- **Router restart, worker SIGKILL, platform down 5 s**: all delivered exactly once, groups in order.
- **Slow target past `timeoutSeconds`**: the first exchange is abandoned by the platform, the
  second is accepted; the target sees each message twice and accepts it once; `COMPLETED`.
- **Outbox → platform**: all delivered, groups in order, table empty afterwards.

## What made Go hard to run

Nothing needed a change to Go; the build is `go build -mod=readonly` into `target/go-bin` with
the checkout's `git status` verified unchanged. The Java harness's Go seeding quirk (seeder writes
`schema_type='JSON'` against migration 051's CHECK) is **fixed at `73a6918`**, so no workaround
was needed. What took finding out:

- Go's router mediator speaks **h2c with prior knowledge** to `http://` targets and does not fall
  back, while the platform listener is plain HTTP/1.1: the Go router runs with
  `FLOWCATALYST_DEV_MODE=true`, which only switches the mediator to HTTP/1.1.
- The router needs `FC_ROUTER_CLIENT_ID`/`SECRET` + `FC_ROUTER_PLATFORM_URL` (a service account
  with `platform:router`) to fetch `/api/dispatch/router-config`; it refuses to start otherwise.
- SQS: no endpoint knob in Go's code; the AWS SDK's own `AWS_ENDPOINT_URL_SQS` works. Queue URIs
  must stay in the `https://sqs.<region>.amazonaws.com/<acct>/<name>` form (the router picks the
  SQS backend by host), which LocalStack resolves by path. Go creates queues lazily on publish but
  the router never does, so the harness pre-creates `FC-go-platform-DEFAULT.fifo`.
- Client-scoped jobs go to `{prefix}-{client}-DEFAULT.fifo`, which the router-config document
  never lists (the API never sets `client_identifier`), so they would sit `QUEUED`; the scenarios
  are platform-level. Worth a Go backlog line.
- The worker's processing endpoint defaults to its own port; every process's metrics port
  defaults to 9090; migrations take no lock (platform first, then the rest).

## The generated report

Verbatim `report.md` of run `20260925-182311-h7k79` (run directory paths shortened to
`<run>`; `report.json` there holds every recorded delivery with headers and body).

---

## Delivery parity run `20260925-182311-h7k79`

Go `73a6918` · Rust `5364c6f3`

### Verdicts

| Scenario | Verdict | Go | Rust | Unaccepted diffs |
|---|---|---|---|---|
| [plain-events](#plain-events) | **DIFF** | 20/20 accepted, 20 deliveries; invariants ok | 0/20 accepted, 0 deliveries; **1 invariant(s) broken** | 12 |
| [dispatch-jobs-api](#dispatch-jobs-api) | **DIFF** | 10/10 accepted, 10 deliveries; invariants ok | 0/10 accepted, 0 deliveries; **1 invariant(s) broken** | 9 |
| [ack-false-no-delay](#ack-false-no-delay) | **DIFF** | 5/5 accepted, 10 deliveries; invariants ok | 0/5 accepted, 0 deliveries; **1 invariant(s) broken** | 9 |
| [ack-false-delay](#ack-false-delay) | **DIFF** | 5/5 accepted, 10 deliveries; invariants ok | 0/5 accepted, 0 deliveries; **1 invariant(s) broken** | 9 |
| [rate-limited-429](#rate-limited-429) | **DIFF** | 5/5 accepted, 15 deliveries; invariants ok | 0/5 accepted, 0 deliveries; **1 invariant(s) broken** | 9 |
| [server-error-then-success](#server-error-then-success) | **DIFF** | 5/5 accepted, 15 deliveries; invariants ok | 0/5 accepted, 0 deliveries; **1 invariant(s) broken** | 9 |
| [permanent-errors](#permanent-errors) | **DIFF** | 0/6 accepted, 10 deliveries; invariants ok | 0/6 accepted, 0 deliveries; invariants ok | 7 |
| [connection-refused](#connection-refused) | **DIFF** | 0/3 accepted, 0 deliveries; invariants ok | 0/3 accepted, 0 deliveries; invariants ok | 3 |
| [block-on-error-group](#block-on-error-group) | **DIFF** | 6/6 accepted, 8 deliveries; invariants ok | 0/6 accepted, 0 deliveries; **1 invariant(s) broken** | 10 |
| [next-on-error-group](#next-on-error-group) | **DIFF** | 6/6 accepted, 8 deliveries; invariants ok | 0/6 accepted, 0 deliveries; **1 invariant(s) broken** | 11 |
| [ordered-group-20](#ordered-group-20) | **DIFF** | 20/20 accepted, 20 deliveries; invariants ok | 0/20 accepted, 0 deliveries; **1 invariant(s) broken** | 9 |
| [burst-pool-capacity](#burst-pool-capacity) | **DIFF** | 60/60 accepted, 60 deliveries; invariants ok | 0/60 accepted, 0 deliveries; **1 invariant(s) broken** | 8 |
| [router-restart](#router-restart) | **DIFF** | 40/40 accepted, 40 deliveries; invariants ok | 0/40 accepted, 0 deliveries; **1 invariant(s) broken** | 12 |
| [worker-restart](#worker-restart) | **DIFF** | 40/40 accepted, 40 deliveries; invariants ok | 0/40 accepted, 0 deliveries; **1 invariant(s) broken** | 12 |
| [platform-down](#platform-down) | **DIFF** | 20/20 accepted, 20 deliveries; invariants ok | 0/20 accepted, 0 deliveries; **1 invariant(s) broken** | 10 |
| [outbox-events](#outbox-events) | **DIFF** | 10/10 accepted, 10 deliveries; invariants ok | 0/10 accepted, 0 deliveries; **1 invariant(s) broken** | 11 |
| [slow-target-timeout](#slow-target-timeout) | **DIFF** | 3/3 accepted, 6 deliveries; invariants ok | 0/3 accepted, 0 deliveries; **1 invariant(s) broken** | 9 |

PASS: identical after normalisation, invariants hold on both sides. ACCEPTED: only diffs listed in `expected-diffs.json`. DIFF: an unaccepted difference. FAIL: no diff, but an invariant broken on some side. ERROR: a side could not run the scenario.

### Stacks

#### go

- platform/worker: <workspace>/target/go-bin/fc-server
- router: <workspace>/target/go-bin/fc-server
- outbox: <workspace>/target/go-bin/fc-server
- platform healthy after 2.803092625s
- API caller: service account harness-api (platform:super-admin), client_credentials bearer
- router config: the platform's own document (/api/dispatch/router-config); router authenticates with harness-router
- stack up after 16.250120083s

#### rust

- platform/worker: <workspace>/target/debug/fc-server
- router: <workspace>/target/debug/fc-router-bin
- outbox: <workspace>/target/debug/fc-outbox-processor
- database migrated and seeded by Go fc-server first (2.762919459s); this side adopts it (the cutover path)
- platform healthy after 6.881175459s
- API caller: service account harness-api (platform:super-admin), client_credentials bearer
- router config: HARNESS SHIM — the platform answered 404 on GET /api/dispatch/router-config; the router reads a harness-served document in Go's shape instead
- outbox exited during boot (see <run>/rust/outbox.log)
- stack up after 17.7413735s

Most frequent ERROR lines in the process logs (whole run):

| process | count | error |
|---|---|---|
| platform | 409 | `fc_stream::event_fan_out: Event fan-out cycle failed error=error returned from database: column "connection_id" of relation "msg_dispatch_jobs" does not exist` |
| outbox | 1 | `Error: error returned from database: cannot insert multiple commands into a prepared statement` |

### Scenarios

#### plain-events

**DIFF** — Baseline: 20 events in 4 message groups through events/batch → fan-out → scheduler → SQS → router → /api/dispatch/process → webhook; every target answers 200.

Covers: baseline, C1

| | go | rust | 
|---|---|---|
| accepted / sent | 20 / 20 | 0 / 20 | 
| deliveries (all attempts) | 20 | 0 | 
| lost | 0 | 20 | 
| accepted more than once | 0 | 0 | 
| attempts per stimulus → count | 1: 20 | 0: 20 | 
| group acceptance order | g1: 1-5<br>g2: 1-5<br>g3: 1-5<br>g4: 1-5 | — | 
| FIFO overtakes | 0 | 0 | 
| job statuses | COMPLETED: 20 | — | 
| job attempt_count → jobs | 0: 20 | — | 
| retry gaps | — | — | 
| max in flight at target | 1 | 0 | 
| signatures | valid: 20 | — | 
| ingest errors | 0 | 0 | 
| outbox rows left | — | — | 
| events stored / fanned out | 20 / 20 | 20 / 0 | 
| queue (visible, in flight) | 0, 0 | 0, 0 | 
| settled | yes, 1019ms | **no** (gave up at 60338ms) | 
| disruptions | — | — | 

Invariants broken on **rust**:
- loss: 20 of 20 stimuli never accepted by the target (e.g. plain-events-0001, plain-events-0002, plain-events-0003)

Diagnosis (rust): 20 events stored, none fanned out, no dispatch job: event fan-out is not running or failing (see the platform's ERROR lines under Stacks); did not settle within 60338ms

| Diff | go | rust | accepted by |
|---|---|---|---|
| accepted | `20` | `0` | — |
| lost | `0` | `20` | — |
| attempts | `{"1":20}` | `{"0":20}` | — |
| groupOrder/g1 | `[1,2,3,4,5]` | `null` | — |
| groupOrder/g2 | `[1,2,3,4,5]` | `null` | — |
| groupOrder/g3 | `[1,2,3,4,5]` | `null` | — |
| groupOrder/g4 | `[1,2,3,4,5]` | `null` | — |
| jobStatus | `{"COMPLETED":20}` | `{}` | — |
| jobAttempts | `{"0":20}` | `{}` | — |
| signatures | `["valid"]` | `[]` | — |
| headers | `["authorization","content-type","x-dispatch-job-id","x-event-type","x-flowcatalyst-signature","x-flowcatalyst-timestamp"]` | `[]` | — |
| settled | `true` | `false` | — |

#### dispatch-jobs-api

**DIFF** — 10 dispatch jobs created directly through POST /api/dispatch-jobs/batch (the SDK path), 2 groups; target answers 200. Go delivers directly-created jobs unsigned: the signer is resolved from the job's subscription, its connection, or the application named by the code's first segment — never from the job's own serviceAccountId — so the signature invariant is off; the signature verdicts are still compared.

Covers: C2

| | go | rust | 
|---|---|---|
| accepted / sent | 10 / 10 | 0 / 10 | 
| deliveries (all attempts) | 10 | 0 | 
| lost | 0 | 10 | 
| accepted more than once | 0 | 0 | 
| attempts per stimulus → count | 1: 10 | 0: 10 | 
| group acceptance order | g1: 1-5<br>g2: 1-5 | — | 
| FIFO overtakes | 0 | 0 | 
| job statuses | COMPLETED: 10 | QUEUED: 10 | 
| job attempt_count → jobs | 0: 10 | 0: 10 | 
| retry gaps | — | — | 
| max in flight at target | 1 | 0 | 
| signatures | unsigned: 10 | — | 
| ingest errors | 0 | 0 | 
| outbox rows left | — | — | 
| events stored / fanned out | 0 / 0 | 0 / 0 | 
| queue (visible, in flight) | 0, 0 | 0, 0 | 
| settled | yes, 1013ms | **no** (gave up at 60042ms) | 
| disruptions | — | — | 

Invariants broken on **rust**:
- loss: 10 of 10 stimuli never accepted by the target (e.g. dispatch-jobs-api-0001, dispatch-jobs-api-0002, dispatch-jobs-api-0003)

Diagnosis (rust): all 10 jobs are QUEUED, the queue is empty and the target saw nothing: the jobs were marked queued without a message reaching SQS (review C1: the scheduler's publisher is a no-op; or C2: API-created jobs inserted QUEUED, which the poller never reads); job statuses {"QUEUED": 10}; did not settle within 60042ms

| Diff | go | rust | accepted by |
|---|---|---|---|
| accepted | `10` | `0` | — |
| lost | `0` | `10` | — |
| attempts | `{"1":10}` | `{"0":10}` | — |
| groupOrder/g1 | `[1,2,3,4,5]` | `null` | — |
| groupOrder/g2 | `[1,2,3,4,5]` | `null` | — |
| jobStatus | `{"COMPLETED":10}` | `{"QUEUED":10}` | — |
| signatures | `["unsigned"]` | `[]` | — |
| headers | `["content-type","x-dispatch-job-id","x-event-type"]` | `[]` | — |
| settled | `true` | `false` | — |

#### ack-false-no-delay

**DIFF** — Target answers 200 {"ack":false} (no delaySeconds) on the first attempt, 200 after. Go reschedules the job (default 30s, no retry budget spent); the second attempt must not follow at once.

Covers: H1, H2

| | go | rust | 
|---|---|---|
| accepted / sent | 5 / 5 | 0 / 5 | 
| deliveries (all attempts) | 10 | 0 | 
| lost | 0 | 5 | 
| accepted more than once | 0 | 0 | 
| attempts per stimulus → count | 2: 5 | 0: 5 | 
| group acceptance order | — | — | 
| FIFO overtakes | 0 | 0 | 
| job statuses | COMPLETED: 5 | — | 
| job attempt_count → jobs | 0: 5 | — | 
| retry gaps | c:10-45s: 5 | — | 
| max in flight at target | 1 | 0 | 
| signatures | valid: 10 | — | 
| ingest errors | 0 | 0 | 
| outbox rows left | — | — | 
| events stored / fanned out | 5 / 5 | 5 / 0 | 
| queue (visible, in flight) | 0, 0 | 0, 0 | 
| settled | yes, 33203ms | **no** (gave up at 120112ms) | 
| disruptions | — | — | 

Invariants broken on **rust**:
- loss: 5 of 5 stimuli never accepted by the target (e.g. ack-false-no-delay-0001, ack-false-no-delay-0002, ack-false-no-delay-0003)

Diagnosis (rust): 5 events stored, none fanned out, no dispatch job: event fan-out is not running or failing (see the platform's ERROR lines under Stacks); did not settle within 120112ms

| Diff | go | rust | accepted by |
|---|---|---|---|
| accepted | `5` | `0` | — |
| lost | `0` | `5` | — |
| attempts | `{"2":5}` | `{"0":5}` | — |
| jobStatus | `{"COMPLETED":5}` | `{}` | — |
| jobAttempts | `{"0":5}` | `{}` | — |
| retryGaps | `{"c:10-45s":5}` | `{}` | — |
| signatures | `["valid"]` | `[]` | — |
| headers | `["authorization","content-type","x-dispatch-job-id","x-event-type","x-flowcatalyst-signature","x-flowcatalyst-timestamp"]` | `[]` | — |
| settled | `true` | `false` | — |

#### ack-false-delay

**DIFF** — Target answers 200 {"ack":false,"delaySeconds":3} on the first attempt, 200 after: the next attempt comes no sooner than the delay asked for.

Covers: H1

| | go | rust | 
|---|---|---|
| accepted / sent | 5 / 5 | 0 / 5 | 
| deliveries (all attempts) | 10 | 0 | 
| lost | 0 | 5 | 
| accepted more than once | 0 | 0 | 
| attempts per stimulus → count | 2: 5 | 0: 5 | 
| group acceptance order | — | — | 
| FIFO overtakes | 0 | 0 | 
| job statuses | COMPLETED: 5 | — | 
| job attempt_count → jobs | 0: 5 | — | 
| retry gaps | b:1-10s: 5 | — | 
| max in flight at target | 1 | 0 | 
| signatures | valid: 10 | — | 
| ingest errors | 0 | 0 | 
| outbox rows left | — | — | 
| events stored / fanned out | 5 / 5 | 5 / 0 | 
| queue (visible, in flight) | 0, 0 | 0, 0 | 
| settled | yes, 5048ms | **no** (gave up at 90042ms) | 
| disruptions | — | — | 

Invariants broken on **rust**:
- loss: 5 of 5 stimuli never accepted by the target (e.g. ack-false-delay-0001, ack-false-delay-0002, ack-false-delay-0003)

Diagnosis (rust): 5 events stored, none fanned out, no dispatch job: event fan-out is not running or failing (see the platform's ERROR lines under Stacks); did not settle within 90042ms

| Diff | go | rust | accepted by |
|---|---|---|---|
| accepted | `5` | `0` | — |
| lost | `0` | `5` | — |
| attempts | `{"2":5}` | `{"0":5}` | — |
| jobStatus | `{"COMPLETED":5}` | `{}` | — |
| jobAttempts | `{"0":5}` | `{}` | — |
| retryGaps | `{"b:1-10s":5}` | `{}` | — |
| signatures | `["valid"]` | `[]` | — |
| headers | `["authorization","content-type","x-dispatch-job-id","x-event-type","x-flowcatalyst-signature","x-flowcatalyst-timestamp"]` | `[]` | — |
| settled | `true` | `false` | — |

#### rate-limited-429

**DIFF** — Target answers 429 with Retry-After: 2 on attempt 1, 429 without Retry-After on attempt 2, 200 after. A 429 is a deferral, not a failure: no budget spent, the wait honoured.

Covers: H1, H3

| | go | rust | 
|---|---|---|
| accepted / sent | 5 / 5 | 0 / 5 | 
| deliveries (all attempts) | 15 | 0 | 
| lost | 0 | 5 | 
| accepted more than once | 0 | 0 | 
| attempts per stimulus → count | 3: 5 | 0: 5 | 
| group acceptance order | — | — | 
| FIFO overtakes | 0 | 0 | 
| job statuses | COMPLETED: 5 | — | 
| job attempt_count → jobs | 0: 5 | — | 
| retry gaps | b:1-10s: 5, c:10-45s: 5 | — | 
| max in flight at target | 1 | 0 | 
| signatures | valid: 15 | — | 
| ingest errors | 0 | 0 | 
| outbox rows left | — | — | 
| events stored / fanned out | 5 / 5 | 5 / 0 | 
| queue (visible, in flight) | 0, 0 | 0, 0 | 
| settled | yes, 35690ms | **no** (gave up at 150100ms) | 
| disruptions | — | — | 

Invariants broken on **rust**:
- loss: 5 of 5 stimuli never accepted by the target (e.g. rate-limited-429-0001, rate-limited-429-0002, rate-limited-429-0003)

Diagnosis (rust): 5 events stored, none fanned out, no dispatch job: event fan-out is not running or failing (see the platform's ERROR lines under Stacks); did not settle within 150100ms

| Diff | go | rust | accepted by |
|---|---|---|---|
| accepted | `5` | `0` | — |
| lost | `0` | `5` | — |
| attempts | `{"3":5}` | `{"0":5}` | — |
| jobStatus | `{"COMPLETED":5}` | `{}` | — |
| jobAttempts | `{"0":5}` | `{}` | — |
| retryGaps | `{"b:1-10s":5,"c:10-45s":5}` | `{}` | — |
| signatures | `["valid"]` | `[]` | — |
| headers | `["authorization","content-type","x-dispatch-job-id","x-event-type","x-flowcatalyst-signature","x-flowcatalyst-timestamp"]` | `[]` | — |
| settled | `true` | `false` | — |

#### server-error-then-success

**DIFF** — Target answers 503 twice, then 200 (maxRetries 5): retried with backoff, accepted once, job COMPLETED with 3 attempts.

Covers: H1, H4

| | go | rust | 
|---|---|---|
| accepted / sent | 5 / 5 | 0 / 5 | 
| deliveries (all attempts) | 15 | 0 | 
| lost | 0 | 5 | 
| accepted more than once | 0 | 0 | 
| attempts per stimulus → count | 3: 5 | 0: 5 | 
| group acceptance order | — | — | 
| FIFO overtakes | 0 | 0 | 
| job statuses | COMPLETED: 5 | — | 
| job attempt_count → jobs | 2: 5 | — | 
| retry gaps | b:1-10s: 5, c:10-45s: 5 | — | 
| max in flight at target | 2 | 0 | 
| signatures | valid: 15 | — | 
| ingest errors | 0 | 0 | 
| outbox rows left | — | — | 
| events stored / fanned out | 5 / 5 | 5 / 0 | 
| queue (visible, in flight) | 0, 0 | 0, 0 | 
| settled | yes, 23781ms | **no** (gave up at 120487ms) | 
| disruptions | — | — | 

Invariants broken on **rust**:
- loss: 5 of 5 stimuli never accepted by the target (e.g. server-error-then-success-0001, server-error-then-success-0002, server-error-then-success-0003)

Diagnosis (rust): 5 events stored, none fanned out, no dispatch job: event fan-out is not running or failing (see the platform's ERROR lines under Stacks); did not settle within 120487ms

| Diff | go | rust | accepted by |
|---|---|---|---|
| accepted | `5` | `0` | — |
| lost | `0` | `5` | — |
| attempts | `{"3":5}` | `{"0":5}` | — |
| jobStatus | `{"COMPLETED":5}` | `{}` | — |
| jobAttempts | `{"2":5}` | `{}` | — |
| retryGaps | `{"b:1-10s":5,"c:10-45s":5}` | `{}` | — |
| signatures | `["valid"]` | `[]` | — |
| headers | `["authorization","content-type","x-dispatch-job-id","x-event-type","x-flowcatalyst-signature","x-flowcatalyst-timestamp"]` | `[]` | — |
| settled | `true` | `false` | — |

#### permanent-errors

**DIFF** — Three targets that never accept: 400 (maxRetries 2), 401 (maxRetries 3; Go fails an auth refusal on the first attempt) and 501 (maxRetries 2). Every job ends FAILED; the attempt counts are the retry policy.

Covers: retry budget, conformance config-error-501

| | go | rust | 
|---|---|---|
| accepted / sent | 0 / 6 | 0 / 6 | 
| deliveries (all attempts) | 10 | 0 | 
| lost | 6 | 6 | 
| accepted more than once | 0 | 0 | 
| attempts per stimulus → count | 1: 2, 2: 4 | 0: 6 | 
| group acceptance order | — | — | 
| FIFO overtakes | 0 | 0 | 
| job statuses | FAILED: 6 | — | 
| job attempt_count → jobs | 0: 2, 1: 4 | — | 
| retry gaps | b:1-10s: 4 | — | 
| max in flight at target | 1 | 0 | 
| signatures | valid: 10 | — | 
| ingest errors | 0 | 0 | 
| outbox rows left | — | — | 
| events stored / fanned out | 6 / 6 | 6 / 0 | 
| queue (visible, in flight) | 0, 0 | 0, 0 | 
| settled | yes, 7124ms | **no** (gave up at 120402ms) | 
| disruptions | — | — | 

Diagnosis (go): job statuses {"FAILED": 6}

Diagnosis (rust): 6 events stored, none fanned out, no dispatch job: event fan-out is not running or failing (see the platform's ERROR lines under Stacks); did not settle within 120402ms

| Diff | go | rust | accepted by |
|---|---|---|---|
| attempts | `{"1":2,"2":4}` | `{"0":6}` | — |
| jobStatus | `{"FAILED":6}` | `{}` | — |
| jobAttempts | `{"0":2,"1":4}` | `{}` | — |
| retryGaps | `{"b:1-10s":4}` | `{}` | — |
| signatures | `["valid"]` | `[]` | — |
| headers | `["authorization","content-type","x-dispatch-job-id","x-event-type","x-flowcatalyst-signature","x-flowcatalyst-timestamp"]` | `[]` | — |
| settled | `true` | `false` | — |

#### connection-refused

**DIFF** — The subscription endpoint is a port nothing listens on (maxRetries 2): each job is retried and ends FAILED; the receiver sees nothing.

Covers: transport errors

| | go | rust | 
|---|---|---|
| accepted / sent | 0 / 3 | 0 / 3 | 
| deliveries (all attempts) | 0 | 0 | 
| lost | 3 | 3 | 
| accepted more than once | 0 | 0 | 
| attempts per stimulus → count | 0: 3 | 0: 3 | 
| group acceptance order | — | — | 
| FIFO overtakes | 0 | 0 | 
| job statuses | FAILED: 3 | — | 
| job attempt_count → jobs | 1: 3 | — | 
| retry gaps | — | — | 
| max in flight at target | 0 | 0 | 
| signatures | — | — | 
| ingest errors | 0 | 0 | 
| outbox rows left | — | — | 
| events stored / fanned out | 3 / 3 | 3 / 0 | 
| queue (visible, in flight) | 0, 0 | 0, 0 | 
| settled | yes, 6122ms | **no** (gave up at 90008ms) | 
| disruptions | — | — | 

Diagnosis (go): job statuses {"FAILED": 3}

Diagnosis (rust): 3 events stored, none fanned out, no dispatch job: event fan-out is not running or failing (see the platform's ERROR lines under Stacks); did not settle within 90008ms

| Diff | go | rust | accepted by |
|---|---|---|---|
| jobStatus | `{"FAILED":3}` | `{}` | — |
| jobAttempts | `{"1":3}` | `{}` | — |
| settled | `true` | `false` | — |

#### block-on-error-group

**DIFF** — BLOCK_ON_ERROR, one group of 6: seq 2 fails twice (500) then succeeds (maxRetries 5). Seq 3-6 must not be delivered until seq 2 is accepted; acceptance order 1-6.

Covers: H3, H4

| | go | rust | 
|---|---|---|
| accepted / sent | 6 / 6 | 0 / 6 | 
| deliveries (all attempts) | 8 | 0 | 
| lost | 0 | 6 | 
| accepted more than once | 0 | 0 | 
| attempts per stimulus → count | 1: 5, 3: 1 | 0: 6 | 
| group acceptance order | g1: 1-6 | — | 
| FIFO overtakes | 0 | 0 | 
| job statuses | COMPLETED: 6 | — | 
| job attempt_count → jobs | 0: 5, 2: 1 | — | 
| retry gaps | b:1-10s: 1, c:10-45s: 1 | — | 
| max in flight at target | 1 | 0 | 
| signatures | valid: 8 | — | 
| ingest errors | 0 | 0 | 
| outbox rows left | — | — | 
| events stored / fanned out | 6 / 6 | 6 / 0 | 
| queue (visible, in flight) | 0, 0 | 0, 0 | 
| settled | yes, 23721ms | **no** (gave up at 120101ms) | 
| disruptions | — | — | 

Invariants broken on **rust**:
- loss: 6 of 6 stimuli never accepted by the target (e.g. block-on-error-group-0001, block-on-error-group-0002, block-on-error-group-0003)

Diagnosis (rust): 6 events stored, none fanned out, no dispatch job: event fan-out is not running or failing (see the platform's ERROR lines under Stacks); did not settle within 120101ms

| Diff | go | rust | accepted by |
|---|---|---|---|
| accepted | `6` | `0` | — |
| lost | `0` | `6` | — |
| attempts | `{"1":5,"3":1}` | `{"0":6}` | — |
| groupOrder/g1 | `[1,2,3,4,5,6]` | `null` | — |
| jobStatus | `{"COMPLETED":6}` | `{}` | — |
| jobAttempts | `{"0":5,"2":1}` | `{}` | — |
| retryGaps | `{"b:1-10s":1,"c:10-45s":1}` | `{}` | — |
| signatures | `["valid"]` | `[]` | — |
| headers | `["authorization","content-type","x-dispatch-job-id","x-event-type","x-flowcatalyst-signature","x-flowcatalyst-timestamp"]` | `[]` | — |
| settled | `true` | `false` | — |

#### next-on-error-group

**DIFF** — NEXT_ON_ERROR (the X-01 default), one group of 6: seq 2 fails twice (500) then succeeds (maxRetries 5). The rest of the group keeps flowing past the failing member; everything is accepted exactly once.

Covers: H4, X-01

| | go | rust | 
|---|---|---|
| accepted / sent | 6 / 6 | 0 / 6 | 
| deliveries (all attempts) | 8 | 0 | 
| lost | 0 | 6 | 
| accepted more than once | 0 | 0 | 
| attempts per stimulus → count | 1: 5, 3: 1 | 0: 6 | 
| group acceptance order | g1: 1,3-6,2 | — | 
| FIFO overtakes | 4 | 0 | 
| job statuses | COMPLETED: 6 | — | 
| job attempt_count → jobs | 0: 5, 2: 1 | — | 
| retry gaps | b:1-10s: 1, c:10-45s: 1 | — | 
| max in flight at target | 1 | 0 | 
| signatures | valid: 8 | — | 
| ingest errors | 0 | 0 | 
| outbox rows left | — | — | 
| events stored / fanned out | 6 / 6 | 6 / 0 | 
| queue (visible, in flight) | 0, 0 | 0, 0 | 
| settled | yes, 22160ms | **no** (gave up at 120214ms) | 
| disruptions | — | — | 

Invariants broken on **rust**:
- loss: 6 of 6 stimuli never accepted by the target (e.g. next-on-error-group-0001, next-on-error-group-0002, next-on-error-group-0003)

Diagnosis (rust): 6 events stored, none fanned out, no dispatch job: event fan-out is not running or failing (see the platform's ERROR lines under Stacks); did not settle within 120214ms

| Diff | go | rust | accepted by |
|---|---|---|---|
| accepted | `6` | `0` | — |
| lost | `0` | `6` | — |
| attempts | `{"1":5,"3":1}` | `{"0":6}` | — |
| groupOrder/g1 | `[1,3,4,5,6,2]` | `null` | — |
| fifoBreaks | `4` | `0` | — |
| jobStatus | `{"COMPLETED":6}` | `{}` | — |
| jobAttempts | `{"0":5,"2":1}` | `{}` | — |
| retryGaps | `{"b:1-10s":1,"c:10-45s":1}` | `{}` | — |
| signatures | `["valid"]` | `[]` | — |
| headers | `["authorization","content-type","x-dispatch-job-id","x-event-type","x-flowcatalyst-signature","x-flowcatalyst-timestamp"]` | `[]` | — |
| settled | `true` | `false` | — |

#### ordered-group-20

**DIFF** — 20 events in one message group, target answers 200 after 50ms: delivered one at a time in order 1-20 (FIFO per group).

Covers: H3, H5

| | go | rust | 
|---|---|---|
| accepted / sent | 20 / 20 | 0 / 20 | 
| deliveries (all attempts) | 20 | 0 | 
| lost | 0 | 20 | 
| accepted more than once | 0 | 0 | 
| attempts per stimulus → count | 1: 20 | 0: 20 | 
| group acceptance order | ordered: 1-20 | — | 
| FIFO overtakes | 0 | 0 | 
| job statuses | COMPLETED: 20 | — | 
| job attempt_count → jobs | 0: 20 | — | 
| retry gaps | — | — | 
| max in flight at target | 1 | 0 | 
| signatures | valid: 20 | — | 
| ingest errors | 0 | 0 | 
| outbox rows left | — | — | 
| events stored / fanned out | 20 / 20 | 20 / 0 | 
| queue (visible, in flight) | 0, 0 | 0, 0 | 
| settled | yes, 2522ms | **no** (gave up at 90422ms) | 
| disruptions | — | — | 

Invariants broken on **rust**:
- loss: 20 of 20 stimuli never accepted by the target (e.g. ordered-group-20-0001, ordered-group-20-0002, ordered-group-20-0003)

Diagnosis (rust): 20 events stored, none fanned out, no dispatch job: event fan-out is not running or failing (see the platform's ERROR lines under Stacks); did not settle within 90422ms

| Diff | go | rust | accepted by |
|---|---|---|---|
| accepted | `20` | `0` | — |
| lost | `0` | `20` | — |
| attempts | `{"1":20}` | `{"0":20}` | — |
| groupOrder/ordered | `[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20]` | `null` | — |
| jobStatus | `{"COMPLETED":20}` | `{}` | — |
| jobAttempts | `{"0":20}` | `{}` | — |
| signatures | `["valid"]` | `[]` | — |
| headers | `["authorization","content-type","x-dispatch-job-id","x-event-type","x-flowcatalyst-signature","x-flowcatalyst-timestamp"]` | `[]` | — |
| settled | `true` | `false` | — |

#### burst-pool-capacity

**DIFF** — 60 ungrouped events into a pool of concurrency 2, target answers after 300ms: no more than 2 in flight at the target, nothing lost or duplicated.

Covers: capacity gating, H10

| | go | rust | 
|---|---|---|
| accepted / sent | 60 / 60 | 0 / 60 | 
| deliveries (all attempts) | 60 | 0 | 
| lost | 0 | 60 | 
| accepted more than once | 0 | 0 | 
| attempts per stimulus → count | 1: 60 | 0: 60 | 
| group acceptance order | — | — | 
| FIFO overtakes | 0 | 0 | 
| job statuses | COMPLETED: 60 | — | 
| job attempt_count → jobs | 0: 60 | — | 
| retry gaps | — | — | 
| max in flight at target | 2 | 0 | 
| signatures | valid: 60 | — | 
| ingest errors | 0 | 0 | 
| outbox rows left | — | — | 
| events stored / fanned out | 60 / 60 | 60 / 0 | 
| queue (visible, in flight) | 0, 0 | 0, 0 | 
| settled | yes, 10627ms | **no** (gave up at 150208ms) | 
| disruptions | — | — | 

Invariants broken on **rust**:
- loss: 60 of 60 stimuli never accepted by the target (e.g. burst-pool-capacity-0001, burst-pool-capacity-0002, burst-pool-capacity-0003)

Diagnosis (rust): 60 events stored, none fanned out, no dispatch job: event fan-out is not running or failing (see the platform's ERROR lines under Stacks); did not settle within 150208ms

| Diff | go | rust | accepted by |
|---|---|---|---|
| accepted | `60` | `0` | — |
| lost | `0` | `60` | — |
| attempts | `{"1":60}` | `{"0":60}` | — |
| jobStatus | `{"COMPLETED":60}` | `{}` | — |
| jobAttempts | `{"0":60}` | `{}` | — |
| signatures | `["valid"]` | `[]` | — |
| headers | `["authorization","content-type","x-dispatch-job-id","x-event-type","x-flowcatalyst-signature","x-flowcatalyst-timestamp"]` | `[]` | — |
| settled | `true` | `false` | — |

#### router-restart

**DIFF** — 40 events in 4 groups, target answers after 250ms; the router is restarted (SIGTERM, start) 3s in. Nothing lost; nothing accepted twice.

Covers: H8, H9, H12

| | go | rust | 
|---|---|---|
| accepted / sent | 40 / 40 | 0 / 40 | 
| deliveries (all attempts) | 40 | 0 | 
| lost | 0 | 40 | 
| accepted more than once | 0 | 0 | 
| attempts per stimulus → count | 1: 40 | 0: 40 | 
| group acceptance order | g1: 1-10<br>g2: 1-10<br>g3: 1-10<br>g4: 1-10 | — | 
| FIFO overtakes | 0 | 0 | 
| job statuses | COMPLETED: 40 | — | 
| job attempt_count → jobs | 0: 40 | — | 
| retry gaps | — | — | 
| max in flight at target | 4 | 0 | 
| signatures | valid: 40 | — | 
| ingest errors | 0 | 0 | 
| outbox rows left | — | — | 
| events stored / fanned out | 40 / 40 | 40 / 0 | 
| queue (visible, in flight) | 0, 0 | 0, 0 | 
| settled | yes, 4034ms | **no** (gave up at 180237ms) | 
| disruptions | at 3.001363417s: restart router: stopped in 1.015488084s, back after 1.322255459s | at 3.013016917s: restart router: stopped in 32.392875ms, back after 642.725709ms | 

Invariants broken on **rust**:
- loss: 40 of 40 stimuli never accepted by the target (e.g. router-restart-0001, router-restart-0002, router-restart-0003)

Diagnosis (rust): 40 events stored, none fanned out, no dispatch job: event fan-out is not running or failing (see the platform's ERROR lines under Stacks); did not settle within 180237ms

| Diff | go | rust | accepted by |
|---|---|---|---|
| accepted | `40` | `0` | — |
| lost | `0` | `40` | — |
| attempts | `{"1":40}` | `{"0":40}` | — |
| groupOrder/g1 | `[1,2,3,4,5,6,7,8,9,10]` | `null` | — |
| groupOrder/g2 | `[1,2,3,4,5,6,7,8,9,10]` | `null` | — |
| groupOrder/g3 | `[1,2,3,4,5,6,7,8,9,10]` | `null` | — |
| groupOrder/g4 | `[1,2,3,4,5,6,7,8,9,10]` | `null` | — |
| jobStatus | `{"COMPLETED":40}` | `{}` | — |
| jobAttempts | `{"0":40}` | `{}` | — |
| signatures | `["valid"]` | `[]` | — |
| headers | `["authorization","content-type","x-dispatch-job-id","x-event-type","x-flowcatalyst-signature","x-flowcatalyst-timestamp"]` | `[]` | — |
| settled | `true` | `false` | — |

#### worker-restart

**DIFF** — 40 events in 4 groups, target answers after 250ms; the worker (dispatch scheduler) is killed (SIGKILL) 2s in and started again. Nothing lost; nothing published or accepted twice.

Covers: H5, stale recovery

| | go | rust | 
|---|---|---|
| accepted / sent | 40 / 40 | 0 / 40 | 
| deliveries (all attempts) | 40 | 0 | 
| lost | 0 | 40 | 
| accepted more than once | 0 | 0 | 
| attempts per stimulus → count | 1: 40 | 0: 40 | 
| group acceptance order | g1: 1-10<br>g2: 1-10<br>g3: 1-10<br>g4: 1-10 | — | 
| FIFO overtakes | 0 | 0 | 
| job statuses | COMPLETED: 40 | — | 
| job attempt_count → jobs | 0: 40 | — | 
| retry gaps | — | — | 
| max in flight at target | 4 | 0 | 
| signatures | valid: 40 | — | 
| ingest errors | 0 | 0 | 
| outbox rows left | — | — | 
| events stored / fanned out | 40 / 40 | 40 / 0 | 
| queue (visible, in flight) | 0, 0 | 0, 0 | 
| settled | yes, 4035ms | **no** (gave up at 180193ms) | 
| disruptions | at 2.004319167s: kill worker: stopped in 3.697625ms, back after 649.1395ms | at 2.003291917s: kill worker: stopped in 4.588875ms, back after 1.255395791s | 

Invariants broken on **rust**:
- loss: 40 of 40 stimuli never accepted by the target (e.g. worker-restart-0001, worker-restart-0002, worker-restart-0003)

Diagnosis (rust): 40 events stored, none fanned out, no dispatch job: event fan-out is not running or failing (see the platform's ERROR lines under Stacks); did not settle within 180193ms

| Diff | go | rust | accepted by |
|---|---|---|---|
| accepted | `40` | `0` | — |
| lost | `0` | `40` | — |
| attempts | `{"1":40}` | `{"0":40}` | — |
| groupOrder/g1 | `[1,2,3,4,5,6,7,8,9,10]` | `null` | — |
| groupOrder/g2 | `[1,2,3,4,5,6,7,8,9,10]` | `null` | — |
| groupOrder/g3 | `[1,2,3,4,5,6,7,8,9,10]` | `null` | — |
| groupOrder/g4 | `[1,2,3,4,5,6,7,8,9,10]` | `null` | — |
| jobStatus | `{"COMPLETED":40}` | `{}` | — |
| jobAttempts | `{"0":40}` | `{}` | — |
| signatures | `["valid"]` | `[]` | — |
| headers | `["authorization","content-type","x-dispatch-job-id","x-event-type","x-flowcatalyst-signature","x-flowcatalyst-timestamp"]` | `[]` | — |
| settled | `true` | `false` | — |

#### platform-down

**DIFF** — 20 events in 2 groups, target answers after 200ms; the platform is stopped 2s in and kept down 5s (the router's /api/dispatch/process calls fail meanwhile). Nothing lost; nothing accepted twice.

Covers: R-57, router → platform transport errors

| | go | rust | 
|---|---|---|
| accepted / sent | 20 / 20 | 0 / 20 | 
| deliveries (all attempts) | 20 | 0 | 
| lost | 0 | 20 | 
| accepted more than once | 0 | 0 | 
| attempts per stimulus → count | 1: 20 | 0: 20 | 
| group acceptance order | g1: 1-10<br>g2: 1-10 | — | 
| FIFO overtakes | 0 | 0 | 
| job statuses | COMPLETED: 20 | — | 
| job attempt_count → jobs | 0: 20 | — | 
| retry gaps | — | — | 
| max in flight at target | 2 | 0 | 
| signatures | valid: 20 | — | 
| ingest errors | 0 | 0 | 
| outbox rows left | — | — | 
| events stored / fanned out | 20 / 20 | 20 / 0 | 
| queue (visible, in flight) | 0, 0 | 0, 0 | 
| settled | yes, 9063ms | **no** (gave up at 180416ms) | 
| disruptions | at 2.002221833s: down platform: stopped in 187.918667ms, back after 5.505395125s | at 2.001282667s: down platform: stopped in 55.649292ms, back after 5.971420917s | 

Invariants broken on **rust**:
- loss: 20 of 20 stimuli never accepted by the target (e.g. platform-down-0001, platform-down-0002, platform-down-0003)

Diagnosis (rust): 20 events stored, none fanned out, no dispatch job: event fan-out is not running or failing (see the platform's ERROR lines under Stacks); did not settle within 180416ms

| Diff | go | rust | accepted by |
|---|---|---|---|
| accepted | `20` | `0` | — |
| lost | `0` | `20` | — |
| attempts | `{"1":20}` | `{"0":20}` | — |
| groupOrder/g1 | `[1,2,3,4,5,6,7,8,9,10]` | `null` | — |
| groupOrder/g2 | `[1,2,3,4,5,6,7,8,9,10]` | `null` | — |
| jobStatus | `{"COMPLETED":20}` | `{}` | — |
| jobAttempts | `{"0":20}` | `{}` | — |
| signatures | `["valid"]` | `[]` | — |
| headers | `["authorization","content-type","x-dispatch-job-id","x-event-type","x-flowcatalyst-signature","x-flowcatalyst-timestamp"]` | `[]` | — |
| settled | `true` | `false` | — |

#### outbox-events

**DIFF** — 10 events written as rows of the SDK outbox table (2 groups), forwarded by the side's outbox processor to /api/events/batch, then delivered; the outbox table is empty afterwards.

Covers: C3, H6

| | go | rust | 
|---|---|---|
| accepted / sent | 10 / 10 | 0 / 10 | 
| deliveries (all attempts) | 10 | 0 | 
| lost | 0 | 10 | 
| accepted more than once | 0 | 0 | 
| attempts per stimulus → count | 1: 10 | 0: 10 | 
| group acceptance order | g1: 1-5<br>g2: 1-5 | — | 
| FIFO overtakes | 0 | 0 | 
| job statuses | COMPLETED: 10 | — | 
| job attempt_count → jobs | 0: 10 | — | 
| retry gaps | — | — | 
| max in flight at target | 1 | 0 | 
| signatures | valid: 10 | — | 
| ingest errors | 0 | 0 | 
| outbox rows left | — | 0: 10 | 
| events stored / fanned out | 10 / 10 | 0 / 0 | 
| queue (visible, in flight) | 0, 0 | 0, 0 | 
| settled | yes, 2032ms | **no** (gave up at 90499ms) | 
| disruptions | — | — | 

Invariants broken on **rust**:
- loss: 10 of 10 stimuli never accepted by the target (e.g. outbox-events-0001, outbox-events-0002, outbox-events-0003)

Diagnosis (rust): 10 outbox row(s) still in the table by status {0: 10} (0 pending, 9 in progress, 2-6 error); no event of the scenario was stored: ingest (or the outbox processor) never delivered it to the platform; did not settle within 90499ms

| Diff | go | rust | accepted by |
|---|---|---|---|
| accepted | `10` | `0` | — |
| lost | `0` | `10` | — |
| attempts | `{"1":10}` | `{"0":10}` | — |
| groupOrder/g1 | `[1,2,3,4,5]` | `null` | — |
| groupOrder/g2 | `[1,2,3,4,5]` | `null` | — |
| jobStatus | `{"COMPLETED":10}` | `{}` | — |
| jobAttempts | `{"0":10}` | `{}` | — |
| signatures | `["valid"]` | `[]` | — |
| headers | `["authorization","content-type","x-dispatch-job-id","x-event-type","x-flowcatalyst-signature","x-flowcatalyst-timestamp"]` | `[]` | — |
| outboxLeft | `{}` | `{"0":10}` | — |
| settled | `true` | `false` | — |

#### slow-target-timeout

**DIFF** — Subscription timeout 2s; on attempt 1 the target answers only after 4s, by which time the platform has hung up (so that answer never goes out); attempt 2 is answered at once. The target sees the message twice, accepts it once; the job ends COMPLETED.

Covers: duplicate/redelivery, H12

| | go | rust | 
|---|---|---|
| accepted / sent | 3 / 3 | 0 / 3 | 
| deliveries (all attempts) | 6 | 0 | 
| lost | 0 | 3 | 
| accepted more than once | 0 | 0 | 
| attempts per stimulus → count | 2: 3 | 0: 3 | 
| group acceptance order | — | — | 
| FIFO overtakes | 0 | 0 | 
| job statuses | COMPLETED: 3 | — | 
| job attempt_count → jobs | 1: 3 | — | 
| retry gaps | b:1-10s: 3 | — | 
| max in flight at target | 3 | 0 | 
| signatures | valid: 6 | — | 
| ingest errors | 0 | 0 | 
| outbox rows left | — | — | 
| events stored / fanned out | 3 / 3 | 3 / 0 | 
| queue (visible, in flight) | 0, 0 | 0, 0 | 
| settled | yes, 9046ms | **no** (gave up at 90440ms) | 
| disruptions | — | — | 

Invariants broken on **rust**:
- loss: 3 of 3 stimuli never accepted by the target (e.g. slow-target-timeout-0001, slow-target-timeout-0002, slow-target-timeout-0003)

Diagnosis (rust): 3 events stored, none fanned out, no dispatch job: event fan-out is not running or failing (see the platform's ERROR lines under Stacks); did not settle within 90440ms

| Diff | go | rust | accepted by |
|---|---|---|---|
| accepted | `3` | `0` | — |
| lost | `0` | `3` | — |
| attempts | `{"2":3}` | `{"0":3}` | — |
| jobStatus | `{"COMPLETED":3}` | `{}` | — |
| jobAttempts | `{"1":3}` | `{}` | — |
| retryGaps | `{"b:1-10s":3}` | `{}` | — |
| signatures | `["valid"]` | `[]` | — |
| headers | `["authorization","content-type","x-dispatch-job-id","x-event-type","x-flowcatalyst-signature","x-flowcatalyst-timestamp"]` | `[]` | — |
| settled | `true` | `false` | — |

### expected-diffs.json

No entries used.
