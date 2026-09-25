# Delivery parity harness (Go vs Rust)

Owner decisions #28/#29 (`docs/owner-decisions-2026-09-25.md`): the message pipeline is aligned to
**Go** as a drop-in, and correctness is gated by harnesses. This one drives identical delivery
scenarios through the Go stack (`../flowcatalyst-go`) and the Rust stack (this repo) end to end and
compares what each delivered:

```
producer ──▶ platform ingest ──▶ stream fan-out ──▶ scheduler (worker) ──▶ SQS FIFO ──▶ router ──▶ /api/dispatch/process ──▶ webhook
 (events/batch,                                                        (LocalStack)                  (platform)            (recording
  dispatch-jobs/batch,                                                                                                     receiver)
  SDK outbox table → outbox processor)
```

It answers "does Rust deliver what Go delivers, as often, in the same order, with the same final
job states", which the per-component tests (and `docs/reviews/message-pipeline-review-2026-09-25.md`)
cannot.

## Topology

Per run: **one Postgres** container (`postgres:17`, a database per side) and **one LocalStack**
(`localstack/localstack:3.0`, SQS only). Per side, the production topology of
`inhance/iac/compute/flowcatalyst.ts` + `fc-router.ts`, as separate OS processes:

| Process | Go | Rust |
|---|---|---|
| platform: API + stream processor, scheduler **off** | `fc-server` | `fc-server` |
| worker: dispatch scheduler **on**, platform off | `fc-server` | `fc-server` |
| router | `fc-server` with `MESSAGE_ROUTER_ENABLED=true`, `PLATFORM_ENABLED=false` | `fc-router-bin` |
| outbox processor (SDK outbox table in the platform DB) | `fc-server` with `FC_OUTBOX_ENABLED=true` | `fc-outbox-processor` |

Each side has its **own recording receiver** (an axum server on a random loopback port) so the two
recordings never mix. Queues are FIFO, named as Go composes them: `FC-go-platform-DEFAULT.fifo`
and `FC-rs-platform-DEFAULT.fifo` (prefix per side, `{prefix}-{tenant}-{priority}.fifo`). Queue
URIs are in the `https://sqs.us-east-1.amazonaws.com/000000000000/<name>` form both
implementations require; `AWS_ENDPOINT_URL[_SQS]` points the SDKs at LocalStack, which resolves the
queue by path.

Both sides are provisioned **identically through their own API** (same codes, names, order) after
boot: the bootstrap admin logs in; a `harness-api` service account with `platform:super-admin`
gets a client-credentials bearer used for everything after; a `harness-signer` service account
signs deliveries (every subscription / job names it, so the receiver verifies
`X-FlowCatalyst-Signature` against its secret); a `harness-router` service account with
`platform:router` is the router's credential. Every scenario's pools, event types and
subscriptions are created **before** the worker and router start, so no scheduler/router cache
delay (Go caches pool codes 60 s, the router config 5 s here) can skew a scenario.

By default the **Rust side adopts a Go-migrated database**: Go's `fc-server` (platform only) runs
once against the Rust database (goose migrations + Go seed), is stopped, and Rust boots on it — the
production cutover path. `--rust-schema own` gives Rust a fresh database it migrates itself.

Ports are random, container names carry the run id (`fc-harness-delivery-{pg,sqs}-<run>`), and
the containers are removed at the end (`--keep-infra` keeps them). Processes get a **clean
environment** (only `PATH`, `HOME`, `TMPDIR`, `USER`, `LANG` pass through), so nothing in your
shell leaks into either side.

## Prerequisites

- Docker, with `postgres:17` and `localstack/localstack:3.0` available (pulled on first use).
- A Go 1.27+ toolchain, or a prebuilt Go `fc-server` (`--go-bin-dir`).
- `openssl` (JWT key pair and app key per run).
- The Rust binaries:

  ```sh
  cargo build -p fc-server -p fc-router-bin -p fc-outbox-processor
  ```

  or `--rust-bin-dir <dir>` holding `fc-server`, `fc-router-bin` (or `fc-router`) and
  `fc-outbox-processor` — e.g. built from a branch with fixes.

### Building Go

Unless `--go-bin-dir` is given, the harness runs, from the Go checkout (`--go-src`, default
`../flowcatalyst-go` next to this repo):

```sh
go build -mod=readonly -o <workspace>/target/go-bin/fc-server ./cmd/fc-server
```

and compares `git -C <go-src> status --porcelain` before and after; any change aborts the run. It
never writes to, tests or runs anything else in the Go repo.

## Running

```sh
# everything, both sides
cargo run -p fc-delivery-harness --

# one scenario (or a comma list, or a prefix ending in *)
cargo run -p fc-delivery-harness -- --only block-on-error-group

# one side only (invariants are still checked; no diff)
cargo run -p fc-delivery-harness -- --sides go

# Rust binaries from a fixed branch, prebuilt Go
cargo run -p fc-delivery-harness -- --rust-bin-dir ../fc-fixes/target/debug --go-bin-dir target/go-bin
```

Options: `--sides go,rust`, `--only`, `--scenarios <dir>`, `--expected-diffs <file>`,
`--report <dir>` (default `target/delivery-harness/<run-id>/`), `--go-src`, `--go-bin-dir`,
`--rust-bin-dir`, `--rust-schema go|own`, `--keep-infra`. `HARNESS_GO_SRC`,
`HARNESS_GO_BIN_DIR` and `HARNESS_RUST_BIN_DIR` work as env vars too.

Exit status: `0` when every scenario is PASS or ACCEPTED and no `expected-diffs.json` entry is
stale; `1` otherwise; `2` when the harness itself failed.

As a test (ignored by default; asserts only that the harness worked — both stacks booted, every
scenario ran — never that Rust passed):

```sh
cargo test -p fc-delivery-harness --test delivery_run -- --ignored --nocapture
HARNESS_ONLY=plain-events cargo test -p fc-delivery-harness --test delivery_run -- --ignored --nocapture
```

`cargo test -p fc-delivery-harness` (not ignored) only checks that every scenario and
`expected-diffs.json` parse.

Both sides run each scenario at the same time, so a run takes as long as the slower side: about
10 minutes when both deliver (the retry scenarios wait out Go's 30 s deferrals and backoff), about
35 minutes when one side delivers nothing and every scenario runs to its timeout. Use `--only`
while iterating.

## Scenarios

JSON files in `scenarios/`, run in file-name order. Each scenario is data:

```jsonc
{
  "name": "block-on-error-group",          // [a-z0-9-]; receiver path, codes
  "description": "…what it pins…",
  "covers": ["H3", "H4"],                   // review findings / rulings
  "pools": [{"code": "narrow", "concurrency": 2, "rateLimit": 60}],   // optional
  "targets": [{                             // one subscription (event paths) per target
    "name": "main",
    "dispatchMode": "BLOCK_ON_ERROR",       // BLOCK_ON_ERROR | NEXT_ON_ERROR | IMMEDIATE
    "pool": "narrow",                       // optional: a pool above
    "maxRetries": 5, "timeoutSeconds": 2,   // optional
    "endpoint": "refused",                  // optional: a port nothing listens on
    "script": {                             // receiver answers (default: 200 {"ack":true})
      "default": {"status": 200, "body": {"ack": true}, "delayMs": 50},
      "byAttempt": [{"attempts": [1, 2], "respond": {"status": 503}}],
      "bySeq":     [{"seq": 2, "attempts": [1, 2], "respond": {"status": 500}}]
    }
  }],
  "stimuli": [
    {"kind": "events", "target": "main", "count": 6, "groups": ["g1"], "batch": 50},
    {"kind": "dispatchJobs", "target": "main", "count": 10, "groups": ["g1", "g2"]},
    {"kind": "outboxEvents", "target": "main", "count": 10, "groups": ["g1"]},
    {"kind": "pause", "ms": 1000}
  ],
  "disruptions": [{"atMs": 3000, "process": "router", "action": "restart"}],
      // process: router | worker | platform | outbox; action: restart (SIGTERM) | kill (SIGKILL) | down (+ "downMs")
  "settle": {"timeoutMs": 120000, "quietMs": 5000,
             "allAccepted": true, "minDeliveries": null, "allTerminal": false},
  "invariants": {"noLoss": true, "noDuplicateAcceptance": true, "groupOrder": true,
                 "maxAttempts": 3, "minRetryGapMs": 3000, "maxConcurrency": 2, "signed": true}
}
```

A response is `{"status", "body"?, "headers"?, "delayMs"?, "hang"?}`; `hang` never answers. The
response for a delivery is the first matching `bySeq` rule, else the first matching `byAttempt`
rule, else `default`. A 2xx without `"ack": false` **that actually went out** counts as the target
accepting the message; if the caller hung up first (its timeout), it did not.

Each stimulus carries a harness key in its payload — `{"hk": "<scenario>-0007", "hg": "<group>",
"hs": <seq in group>}` — so the receiver identifies a message the same way on both sides whatever
envelope, job id or event id each side uses. Messages are dealt round-robin over `groups`; with no
groups they carry no message group.

**Settle**: the side is settled when every stimulus was accepted (`allAccepted`, default on), or
`minDeliveries` deliveries arrived, or (`allTerminal`) its database holds a terminal dispatch job
for every stimulus; then the harness keeps recording for `quietMs` (late duplicates land here).
Not settling by `timeoutMs` is reported, not an error.

Scenarios today (17): plain events; dispatch jobs through the API (C2); `ack:false` without and
with `delaySeconds` (H1); 429 with/without `Retry-After`; 503 twice then 200; permanent 400 / 401
/ 501; connection refused; BLOCK_ON_ERROR vs NEXT_ON_ERROR group with a failing member (H3/H4);
ordered group of 20; burst of 60 into a concurrency-2 pool; router restart mid-flight; worker
SIGKILL mid-flight; platform down 5 s; outbox → platform (C3/H6); slow target past its timeout
(duplicate by design).

## Reading the report

`report.md` (and `report.json` with every recorded delivery, headers and body) lands in the run
directory beside `go/*.log` and `rust/*.log` — one log per process.

- **Verdicts** table: per scenario `PASS` (identical after normalisation, invariants hold on both
  sides), `ACCEPTED` (only diffs covered by `expected-diffs.json`), `DIFF` (an unaccepted
  difference), `FAIL` (no diff, but an invariant broken on a side), `ERROR` (a side could not run
  it).
- **Stacks**: binaries, and how each side was brought up — including every place the harness had
  to stand in for something (e.g. `router config: HARNESS SHIM` when a platform serves no
  router-config document), and any process that died during boot.
- Per scenario, side by side: accepted/sent, deliveries (every attempt), lost, accepted-more-than-
  once, attempts-per-stimulus histogram, per-group acceptance order, FIFO overtakes (a group
  member that arrived while an earlier member was still unaccepted), final job statuses and
  `attempt_count`s (from `msg_dispatch_jobs`, matched by the scenario's receiver path), retry-gap
  buckets (`<1s`, `1-10s`, `10-45s`, `45-120s`, `>120s`, measured from the end of one exchange to the
  next arrival; the boundaries sit between the delays either side can mean, so poll jitter never
  flips a bucket), the most requests in
  flight at the target at once, signature verdicts, outbox rows left, the queue depth, settle time
  and disruptions.
- **Invariants broken**, per side (with the entry citing it, if any), checked on that side alone: no loss, no duplicate acceptance,
  group order (default for BLOCK_ON_ERROR targets), retry budget, minimum retry gap, pool
  concurrency, valid signatures, and "everything accepted but jobs not terminal". A **Go**
  invariant failure means the scenario asks for something Go does not guarantee (fix the
  scenario) or a Go defect (cite it).
- **Diagnosis**, per side, when stimuli were lost: where the pipeline stopped — ingest refused, no
  dispatch job created (ingest/fan-out), all jobs QUEUED with an empty queue (publisher: review C1,
  or C2), all still PENDING (scheduler), messages sitting on the queue (router), outbox rows never
  forwarded.
- **Diff** table: every normalised field that differs, Go vs Rust, and the `expected-diffs.json`
  entry accepting it, if any.

Normalisation: identity is the harness key, never a platform id; times are offsets from the first
stimulus; gaps are bucketed; header *names* (minus transport headers) are compared, not values;
signatures are verified, not compared.

## expected-diffs.json

Deliberate deviations from Go. Each entry must cite a ruling:

```json
[{"scenario": "permanent-errors", "field": "jobAttempts",
  "reason": "…why Rust differs on purpose…", "ruling": "owner-decisions-2026-09-25 #29 (corpus row …)"}]
```

`scenario` and `field` match exactly or by a prefix ending in `*` (`groupOrder/*`). On a full
two-sided run an entry that matched nothing is **stale** and fails the run, so a fix that removes
a difference must remove its excuse too. No `ruling`, no entry.

A side's broken invariant is cited the same way, under the field `invariant/<side>/<kind>` —
`kind` is the violation's prefix: `loss`, `duplicates`, `group-order`, `fifo`, `retry-budget`,
`backoff`, `pool-concurrency`, `signature`, `status` (e.g. `invariant/go/loss`). A scenario whose
only differences and broken invariants are all cited is ACCEPTED; an uncited broken invariant is
still a FAIL. Citations are per side, so a Go defect's entry never excuses the same failure on Rust.

`"intermittent": true` marks a difference that only shows when a disruption lands in a narrow
window (a timing-dependent Go defect): such an entry is never reported stale.

## Things that made Go (and Rust) awkward to run

- Go's router mediator speaks **h2c with prior knowledge** to any `http://` target and does not
  fall back, while Go's platform listener is plain HTTP/1.1; the Go router therefore runs with
  `FLOWCATALYST_DEV_MODE=true`, which only switches the mediator to HTTP/1.1. (Rust's router dev
  mode swaps in a built-in config, so it is not set there.)
- Go's router fetches `/api/dispatch/router-config` with a client-credentials token and refuses to
  start without `FC_ROUTER_CLIENT_ID`/`SECRET` + `FC_ROUTER_PLATFORM_URL`.
- Go creates SQS queues lazily on publish; the router never does. The harness pre-creates the
  platform tenant's DEFAULT queue.
- Go publishes a client's jobs to `{prefix}-{client identifier}-DEFAULT.fifo`, but the router-config
  document only lists tenants from `client_identifier` on pools/subscriptions, which the API never
  sets — so client-scoped jobs sit QUEUED. The scenarios stay platform-level (no `clientId`).
- Neither side's migrator takes a lock: the platform is started and healthy before the worker.
- The session cookie is `Secure` on both, so the harness carries it by hand over loopback HTTP.
- Rust's platform serves **no router-config document**, so the Rust router reads a harness-served
  shim in Go's shape (pools `platform-<code>`, the DEFAULT queue); this is noted in the report.
