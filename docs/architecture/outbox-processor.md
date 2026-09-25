# Outbox Processor

The outbox processor lives on the **consumer-application side**, not inside the FlowCatalyst platform. Its job is to read messages that an application has written to its own outbox table (in the same transaction as its business write) and forward them to the platform's HTTP API. Source: `crates/fc-outbox/`, binary `bin/fc-outbox-processor/`, also embeddable in `fc-server` for self-contained deployments.

In local development, the same processor is also reachable as a
subcommand of `fc-dev`:

- **Embedded in `fc-dev`** — set `FC_OUTBOX_ENABLED=true` and the
  processor runs in-process alongside the platform. Suitable when the
  app's outbox table lives in fc-dev's embedded Postgres (i.e. the app
  shares the platform DB).
- **Standalone via `fc-dev outbox poll`** — runs only the processor (no
  platform, no embedded PG), pointed at an external app database via
  `--db-url` and forwarding to a platform via `--api-url` + `--token`.
  This is the path for apps that can't share fc-dev's embedded Postgres
  (e.g. a PostGIS-dependent app in Docker). See
  [../developers/fc-dev.md#fc-dev-outbox-poll](../developers/fc-dev.md#fc-dev-outbox-poll--standalone-outbox-poller).

Both modes use the same `EnhancedOutboxProcessor` described below; the
only difference is what process owns it and how the connection / token
are sourced.

The pattern is the standard transactional outbox: an application that wants to emit a FlowCatalyst event writes a business row and an outbox row inside the same database transaction, then trusts a background process to deliver the outbox row eventually. This trades immediate delivery for crash-safety — the message is durable before any HTTP call is attempted.

---

## Position in the system

```
┌────────────────────────────────────────────┐
│   Consumer application                     │
│                                            │
│   BEGIN;                                   │
│     INSERT INTO orders (...);              │
│     INSERT INTO outbox_messages (...);   ◄─── same tx
│   COMMIT;                                  │
│                                            │
│   ┌──────────────────────────────────────┐ │
│   │  fc-outbox-processor                 │ │
│   │  (deployed alongside the app)        │ │
│   │                                      │ │
│   │  claim → group distributor / batch   │ │
│   │       → HTTP POST → row outcome      │ │
│   └─────────────┬────────────────────────┘ │
└─────────────────┼──────────────────────────┘
                  │  POST /api/events/batch
                  │  POST /api/dispatch-jobs/batch
                  │  POST /api/audit-logs/batch
                  ▼
        ┌─────────────────────────┐
        │  FlowCatalyst Platform  │
        └─────────────────────────┘
```

Three things matter:

1. **The outbox table lives in the application's database**, not the platform's. The processor is a sidecar/companion to each application, not a centralised platform component.
2. **Three item types share the same table** (default `outbox_messages`): `EVENT`, `DISPATCH_JOB`, `AUDIT_LOG`. The processor dispatches each type to its own platform endpoint.
3. **Per-group FIFO**. Messages with the same `message_group` deliver in order. Different groups deliver in parallel.

---

## Behaviour (aligned to Go)

The processor is a drop-in for Go's (`flowcatalyst-go/internal/outbox`, owner
decision #28). Every rule below is Go's unless marked **(beyond Go)**.

- **Claim.** Every poll (`FC_OUTBOX_POLL_INTERVAL_MS`, 1 s) claims up to
  `FC_OUTBOX_BATCH_SIZE` (100) PENDING rows of every type, ordered by
  `message_group, created_at` (then `id`, **beyond Go**), and marks them
  IN_PROGRESS in the same atomic statement (Postgres: `UPDATE … FROM (SELECT …
  FOR UPDATE SKIP LOCKED) RETURNING`). A claimed row is never polled again
  while it is in flight. No poll runs while `FC_OUTBOX_MAX_IN_FLIGHT` (1000)
  items are in flight. Rows of an unknown `type` are never claimed.
- **Outcome, then the row.** A row is **deleted** only after the platform
  accepted it (SUCCESS, or SKIPPED for audit logs). A failure bumps
  `retry_count` and stores `error_message`; a retryable status returns the row
  to PENDING while `retry_count + 1 < FC_OUTBOX_MAX_RETRIES` (3), otherwise the
  row keeps its failure status and is not claimed again. There is no backoff
  between attempts: the next poll re-claims a re-queued row.
- **Ungrouped items** of one type go in one request (at most
  `FC_API_BATCH_SIZE`, never more than the platform's 1000).
- **Grouped items** are sent one at a time, in order, by one drain per group;
  at most `FC_OUTBOX_MAX_CONCURRENT_GROUPS` (10) groups drain at once. With
  `FC_OUTBOX_BLOCK_ON_ERROR` (true), a failed item stops its group: the
  group's other claimed items (and any claimed while it stops, **beyond Go**)
  are released to PENDING with no penalty and re-claimed in order behind it. A
  *final* failure (terminal status, or retries exhausted) also **blocks** the
  group: its items are released every poll until an operator unblocks it
  (the item is re-queued with `retry_count` 0) or skips it (the item stays
  failed). The block is held in memory, as in Go; the rows are not.
- **Recovery.** Every 60 s, rows IN_PROGRESS for more than 5 minutes (their
  processor died) return to PENDING. A row this process still holds is not
  sent a second time if it is recovered and re-claimed meanwhile **(beyond
  Go)**.
- **Leadership.** Polling and recovery run only while the processor is primary.

A restart loses nothing: every item in memory is a claimed row that recovery
returns to PENDING.

### Answers from the platform

The request is `{"items": [payload, …]}`. A 2xx (the platform answers 201) is
matched to the items **by position** (`results[i]` is `items[i]`; the result id
is the platform's resource id, not the outbox row id). A result count that
doesn't match, or an unreadable body, fails every item as INTERNAL_ERROR.

| Answer | Status stored | Retried |
|---|---|---|
| 2xx, item `SUCCESS` / `SKIPPED` | row deleted | — |
| 2xx, item `BAD_REQUEST` / `FORBIDDEN` | that status | no |
| 2xx, item `INTERNAL_ERROR` / `UNAUTHORIZED` / `GATEWAY_ERROR` | that status | yes |
| 400 | BAD_REQUEST (2) | no |
| 401 | UNAUTHORIZED (4) | yes |
| 403 (e.g. a reach refusal) | FORBIDDEN (5) | no |
| 502, 503, 504, network error | GATEWAY_ERROR (6) | yes |
| any other status (404, 409, 422, 429, 500, …) | INTERNAL_ERROR (3) | yes |

**Beyond Go**, three whole-batch refusals are split instead: 409
`DUPLICATE_ID`, 400 `BATCH_TOO_LARGE` and 413 are retried as two halves, down
to single items, so one item can't keep the others from being accepted. A
single item refused 409 `DUPLICATE_ID` whose payload `id` is its own outbox row
id counts as SUCCESS: the platform already holds the job an earlier attempt
created (the answer was lost). The SDKs (Rust, TypeScript, Laravel) put the
row id in every dispatch-job payload for this reason (owner decision #24).

### Status codes

Stored as integers in `status`; every SDK and processor uses the same codes.

| Code | Status | |
|---|---|---|
| 0 | PENDING | waiting to be claimed |
| 1 | SUCCESS | (never stored: accepted rows are deleted) |
| 2 | BAD_REQUEST | terminal |
| 3 | INTERNAL_ERROR | retryable |
| 4 | UNAUTHORIZED | retryable |
| 5 | FORBIDDEN | terminal |
| 6 | GATEWAY_ERROR | retryable |
| 9 | IN_PROGRESS | claimed |

---

## Module layout

| File | Owns |
|---|---|
| `enhanced_processor.rs` | `EnhancedOutboxProcessor`: the poll and recovery loop, outcome → row, group controls, `EnhancedProcessorConfig::from_env`. |
| `repository.rs` | `OutboxRepository` trait (claim, mark success/failed, release, requeue, recover) and `OutboxTableConfig`. |
| `postgres.rs`, `sqlite.rs`, `mysql.rs`, `mongo.rs` | Per-backend repositories (feature-gated). |
| `group_distributor.rs` | `GroupDistributor`: per-group serial drains, bounded group concurrency, stop on failure. |
| `group_state.rs` | `GroupStateManager`: Running / Paused / Blocked per group. |
| `http_dispatcher.rs` | `HttpDispatcher`: batch requests and the classification above. |
| `recovery.rs` | `RecoveryTask`, a standalone recovery loop for callers driving the repository themselves. |

## Tables

The processor reads the table the SDKs create and only its own columns (`id`,
`type`, `message_group`, `payload`, `status`, `retry_count`, `error_message`,
`created_at`, `updated_at`). Column types differ between SDKs (Go, TypeScript,
Java and Laravel: `payload TEXT`, `SMALLINT` codes; the Rust SDK: `payload
JSONB`, `INTEGER`; Laravel: timestamps without a zone), so each backend casts
what it reads. A row whose payload is not JSON fails as BAD_REQUEST.

By default all three types share one table; `FC_OUTBOX_EVENTS_TABLE` /
`FC_OUTBOX_DISPATCH_JOBS_TABLE` / `FC_OUTBOX_AUDIT_LOGS_TABLE` route a type to
its own table.

| Backend | Feature | Claim |
|---|---|---|
| PostgreSQL | `postgres` | one `UPDATE … RETURNING` over `FOR UPDATE SKIP LOCKED` |
| SQLite | `sqlite` | one `UPDATE … WHERE id IN (SELECT …) RETURNING` |
| MySQL | `mysql` | `SELECT … FOR UPDATE SKIP LOCKED` + `UPDATE` in one transaction (not selectable by `FC_OUTBOX_DB_TYPE`, as in Go) |
| MongoDB | `mongo` | `findOneAndUpdate` per document (Go's document shape) |

---

## Configuration

Read by `EnhancedProcessorConfig::from_env` (the standalone binary and
`fc-server`). Go's names come first; the earlier names still work.

| Variable | Default | Description |
|---|---|---|
| `FC_OUTBOX_BACKEND` / `FC_OUTBOX_DB_TYPE` | `postgres` | `sqlite`, `postgres`, `mongo` (standalone binary; `fc-server` reads `FC_OUTBOX_DB_TYPE`) |
| `FC_OUTBOX_DB_URL` | — (required) | Application database URL |
| `FC_OUTBOX_MONGO_DB` | `flowcatalyst` | MongoDB database name (mongo only) |
| `FC_OUTBOX_EVENTS_TABLE` / `…_DISPATCH_JOBS_TABLE` / `…_AUDIT_LOGS_TABLE` | `outbox_messages` | Per-type table |
| `FC_OUTBOX_PLATFORM_URL` / `FC_OUTBOX_API_URL` / `FC_API_BASE_URL` / `FLOWCATALYST_URL` | `http://localhost:8080` | Platform API base URL |
| `FC_OUTBOX_PLATFORM_AUTH_TOKEN` / `FC_OUTBOX_TOKEN` / `FC_API_TOKEN` | — | Bearer token |
| `FC_OUTBOX_POLL_INTERVAL_MS` | `1000` | Poll interval |
| `FC_OUTBOX_BATCH_SIZE` | `100` | Rows claimed per poll |
| `FC_API_BATCH_SIZE` | `100` | Most items per request (ungrouped) |
| `FC_OUTBOX_MAX_IN_FLIGHT` / `FC_MAX_IN_FLIGHT` | `1000` | No poll at or above this many in flight |
| `FC_OUTBOX_MAX_CONCURRENT_GROUPS` / `FC_MAX_CONCURRENT_GROUPS` | `10` | Groups sending at once |
| `FC_OUTBOX_MAX_RETRIES` | `3` | Attempts before a retryable failure is final |
| `FC_OUTBOX_BLOCK_ON_ERROR` | `true` | A failed item stops (and a final failure blocks) its group |
| `FC_OUTBOX_ADMIN_PORT` | — | Standalone binary: serve Go's group admin API on 127.0.0.1 |
| `FC_METRICS_PORT` | `9090` | Standalone binary: `/metrics`, `/health`, `/ready` |

Group admin API (`FC_OUTBOX_ADMIN_PORT`, same routes and bodies as Go):
`GET /outbox/groups`, `GET /outbox/groups/blocked`,
`POST /outbox/groups/{group}/pause|resume|unblock|skip` (unblock and skip
answer 404 `{"error":"group not blocked"}` when the group isn't Blocked).

---

## Standby integration

The claim is atomic on every backend, so two instances never claim the same row. Group ordering, however, is only guaranteed within one instance, and the group state (Paused/Blocked) is per process, so run one active instance per outbox.

For HA, run the processor with `FC_STANDBY_ENABLED=true` and let only the leader poll. The standalone binary supports this directly via the `fc-standby` crate; the embedded variant in `fc-server` uses the cluster-wide leader lock.

```
FC_OUTBOX_ENABLED=true
FC_STANDBY_ENABLED=true
FC_STANDBY_REDIS_URL=redis://redis:6379
FC_STANDBY_LOCK_KEY=app-acme-outbox-leader   # one key per application's outbox
```

The lock key must be **unique per outbox**, not per FlowCatalyst cluster — if you have three applications with three outboxes, each gets its own lock.

---

## How the outbox relates to the platform's own UoW

The platform itself uses a different write path. Its own write operations go through `UnitOfWork::commit` (see [platform-control-plane.md](platform-control-plane.md)), which inserts directly into `msg_events` in the same transaction as the entity change — no outbox needed because the platform owns both tables.

The outbox pattern exists for the case where the **application** and **platform** databases are different (which is the common case: application Postgres on one host, FlowCatalyst Postgres on another). The application can't write atomically across them, so it writes to its own outbox and lets a separate process bridge.

If your application happens to share a database with FlowCatalyst, you can write to FlowCatalyst's `outbox_messages` table directly and skip the outbox processor — but that couples your app's schema to FlowCatalyst's, which is rarely what you want.

---

## Code references

- Entry point (standalone): `bin/fc-outbox-processor/src/main.rs`.
- Entry point (embedded): `bin/fc-server/src/main.rs::spawn_outbox_processor`.
- Orchestrator: `crates/fc-outbox/src/enhanced_processor.rs::EnhancedOutboxProcessor`.
- Repository trait: `crates/fc-outbox/src/repository.rs`.
- Backends: `crates/fc-outbox/src/{postgres,sqlite,mysql,mongo}.rs`.
- Distributor: `crates/fc-outbox/src/group_distributor.rs`; group state: `group_state.rs`.
- HTTP dispatcher: `crates/fc-outbox/src/http_dispatcher.rs`.
- Recovery: `crates/fc-outbox/src/recovery.rs`.
- End-to-end tests: `crates/fc-outbox/src/processor_tests.rs` (SQLite), `postgres.rs` tests (Docker, `--ignored`).
- SDK helper for *writing* outbox rows from an application: `crates/fc-sdk/` (with the `outbox-postgres` / `outbox-sqlite` features).
