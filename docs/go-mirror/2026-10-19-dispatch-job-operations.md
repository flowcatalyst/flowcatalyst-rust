# Go dispatch-job operations mirrored (2026-10-19)

Go `29e8cb11` (edit a payload), `ad37de21` (mandatory time window, requeue/redrive guard, split write
permissions, Go 085) and `bbbb4bb3` (event type, subscription and connection filters, Go 086, cached
filter-options), against the contract in Go's `docs/wire-contract.md` ("Dispatch jobs: time window,
requeue guard, write permissions"). Nothing is pushed. The Go repo was only read. The SPA is not
mirrored (P5); what it needs is listed at the end.

| Item | Go | Rust |
|---|---|---|
| 1. `PUT /api/dispatch-jobs/{id}/payload` (+ `/bff`), event `payload-edited`, seeded schema | `29e8cb11` | `8ab6c939` |
| 2. Mandatory window on list, list-raw/raw, filter-options, preview and run | `ad37de21` | `50b14d05` |
| 3. Requeue/redrive status guard, double confirmation, response and preview shapes, `resent` event | `ad37de21` | `de81a920` |
| 4. Split permissions in the role catalogue, migration Go 085 = Rust 095 | `ad37de21` | `60f63ae9` |
| 5. Filters, `connection_id` projection, Go 086 = Rust 096 + ops script, filter-options cache | `bbbb4bb3` | `6f43f32f` |

Tests (all under `crates/fc-platform/tests/it/`, `FC_TEST_PG_BIN=<postgres 18 bin> cargo test -p fc-platform
--test it -- --include-ignored <module>::`; unit tests in `fc-platform-messaging` and `fc-common`):
`dispatch_job_edit_payload_test` (5), `dispatch_job_window_test` (7), `dispatch_job_requeue_guard_test` (5),
`dispatch_job_write_permissions_test` (2), `dispatch_job_filters_test` (6), `dispatch_lifecycle_test` (+3:
requeue guard, redrive, edit payload), `requeue_by_filter_test` and `go_routes_test` updated; window
(8), refs (5), requeue rules (3), filter-options cache (2) unit tests. Mutation checks (each failed at least one
test): in-flight treated as requeueable, the confirmation literal ignored, the redrive permission check removed,
the id walk without its status restriction, the connection predicate `OR` turned `AND`, the projector's
conflict arm without `connection_id`.

## 1. Payload edit

PENDING, FAILED and the legacy ERROR only (`lifecycle::edit_payload`, the status guard in SQL, `updated_at`
moves so a scheduler claim that read the old payload is refused). Validation in Go's order:
`ID_REQUIRED`, `PAYLOAD_REQUIRED` (empty), `PAYLOAD_TOO_LARGE` (over 256 KiB), `CONTENT_TYPE_INVALID`
(blank, over 100, no `/`), then after the load `404 DispatchJob_NOT_FOUND`, `403 SCOPE_FORBIDDEN` (the
job's client), `409 NOT_EDITABLE`, `400 PAYLOAD_NOT_JSON` (a JSON or `+json` type, parameters ignored). The
content type is stored trimmed. Permission `platform:messaging:dispatch-job:edit-payload`
(`403 PERMISSION_REQUIRED`). The command is `EditPayloadCommand`; `payload` is masked in
`aud_logs.operation_json`. The event data is `{dispatchJobId, status (as stored, ERROR stays ERROR),
previousSize, previousSha256, size, sha256, previousContentType, contentType}`; its code and schema are
seeded (existing databases gain the type at the next start; the seeder inserts missing codes only).

Differences: `PAYLOAD_NOT_UTF8` cannot arise (a `String`; a body with invalid UTF-8 does not decode, a 400
before the use case). A race that moves the job between the read and the write is a 409 `NOT_EDITABLE` here
(Go's persister returns a transition-refused error).

## 2. Window

`enforce_window` (`dispatch_job/window.rs`) is Go's `EnforceWindow`: none given is now-24h..now (defaulted),
since only is since..now, until only is until-24h..until (defaulted), both as given (`400 INVALID_WINDOW` when
until is before since); wider than `FC_DISPATCH_JOB_MAX_WINDOW_DAYS` (default 31; non-positive or
unparseable is the default) is `400 WINDOW_TOO_LARGE` (message with the Go-spelled duration, `745h0m0s`).
An unparseable `since`/`until` is ignored on the list and is `400 INVALID_TIMESTAMP` on filter-options and the
POST bodies. Headers `X-Window-From`, `X-Window-To` (RFC 3339 UTC, whole seconds) and `X-Window-Defaulted` on
`GET /api/dispatch-jobs` (+ `/bff`), `/list-raw`, `/raw`; `window {from, to, defaulted}` in the body of
filter-options, the preview and the run. The filter-options DISTINCT reads carry the window. Exempt: by id,
`/raw` by id, attempts, by-event, sign. The variable is read when the routes are built
(`docs/operations/configuration.md`).

## 3. Guard

`REQUEUE` resets PENDING, FAILED/ERROR, CANCELLED, EXPIRED (SQL guard, legacy ERROR included); QUEUED,
PROCESSING, IN_PROGRESS never; `REDRIVE` is a separate transition from COMPLETED only (`requeue_rules.rs`).
Requeue by ids needs `dispatch-job:requeue`; `redriveCompleted: true` also needs `dispatch-job:redrive-completed`
(`403 PERMISSION_REQUIRED` otherwise, checked even for an empty `ids`). COMPLETED is redriven only with
`redriveCompleted` and `confirmRedriveCompleted == "REDRIVE"`; otherwise it is skipped and reported.
Responses: `{requeued, redriven, skippedInFlight, skippedCompleted, skipped[{id, status, reason}]}` (reasons
`IN_FLIGHT`, `COMPLETED_REDRIVE_NOT_CONFIRMED`). Preview: `count`, `byStatus`, `eligibleCount` (not COMPLETED, not in
flight), `completedCount`, `inFlightCount`, `redriveEligibleCount`, `window`; it admits `requeue` or `view`.
Run: `{requeued, redriven, skippedInFlight, skippedCompleted, truncated, remaining, window}`; a confirmed
redrive confirms `redriveEligibleCount`; a stale `confirmCount` is `409 FILTER_MATCH_CHANGED` with
`count, eligibleCount, completedCount, inFlightCount, redriveEligibleCount, byStatus` in `details`; the id walk
is restricted in SQL to the resettable statuses. The command JSON in `aud_logs` carries `redriveCompleted` and
`confirmRedriveCompleted` when sent.

**Owner ruling (coordinator, overrides the commit): the `platform:messaging:dispatch-jobs:resent` event data is
`{ids}` only**, exactly as before `ad37de21`; the schema is unchanged. The redrive mark lives in the audit
row and the response bodies. `de81a920` first carried the six-field shape; `6f43f32f` reverts it (the event
struct keeps the other members as `serde(skip)` for the response).

Cancel and complete are gated on `dispatch-job:cancel` / `:complete` (no longer `view`). `view` is read-only.

## 4. Permissions and migration 095

Catalogue constants `DISPATCH_JOB_{REQUEUE,REDRIVE_COMPLETED,CANCEL,COMPLETE,EDIT_PAYLOAD}` (and the BFF
permission list). `platform:messaging-admin` gets the four ordinary ones; no built-in role holds
`redrive-completed`; `viewer`, `admin-readonly` and `application-service` hold none. Migration 095 (Go 085,
verbatim) grants the four to every role holding `view` except `platform:viewer`, idempotently. Its probe reads
as applied where no such role lacks requeue (checked both ways by hand). Tokens carry permissions from mint
time: users sign in again.

## 5. Filters, projection, indexes, cache

`eventType` + `eventTypes` (CSV, at most 10, filters `code`), `subscriptionId` + `subscriptionIds` (at most 50),
`subscriptionCode` / `subscriptionName` (exact, trailing `*` prefix with `%`/`_`/`\` escaped, name folds case;
resolved to ids within the platform's rows and the caller's tenants; over 200 is `400 TOO_MANY_MATCHES`),
`connectionId` / `connectionCode` (code over 50 is `TOO_MANY_MATCHES`; both given: the id must be one the code
names; predicate `connection_id = ANY(ids) OR subscription_id = ANY(its subscriptions)`); a reference that
matches nothing lists nothing, an unresolved one fails closed. On the list, list-raw and the preview/run bodies
(`dispatch_job/refs.rs`). The projector (`fc-stream`) copies `msg_dispatch_jobs.connection_id` on insert and
conflict. Migration 096 (Go 086, verbatim) adds `msg_dispatch_jobs_read.connection_id` and swaps
`idx_msg_dispatch_jobs_read_code` and `_subscription_id` for `(col, created_at DESC)` composites plus a partial
`(connection_id, created_at DESC)`; Rust had no `scripts/ops`, so Go's online-build script is copied (renamed
096): run it before deploying on a populated database, otherwise the migration builds the partitioned indexes
under one lock. Rows projected before keep `connection_id` NULL until their next projection (optional
backfill in the script). filter-options gains `eventTypes` (the same list as `codes`) and is cached 60 s per tenant
scope and five-minute window bucket; the cache is per `DispatchJobsState`, and a partial answer is never kept.
The Go schema fingerprint fixture takes 086's delta only (Go 083/084 are function work not mirrored):
`rust_schema_matches_go_fingerprint` passes.

## Spec diff

Against Go's live lock (`GO_LOCK_PATH=.../api/openapi.lock.json OPENAPI_REPORT=1 cargo test -p fc-platform
--test it -- openapi_go_contract_test`): the dispatch-job operations match (one new operation, `PUT
/api/dispatch-jobs/{id}/payload`; list, list-raw, raw, filter-options, requeue and by-filter parameters,
statuses and headers match), and the schemas `EditPayloadRequest`, `SkippedJobResponse`, `WindowResponse` exist
(closed against the live lock; they were not in the old vendored copy). The vendored copy
`frontend/openapi/openapi.json` takes the dispatch-job path items and schemas of Go's lock (precedent: the
bug-fix batches copy only the lock hunks they mirror; the function and F31 hunks are not taken), so
the in-tree ratchet baseline is unchanged (131 lines, nothing added). Against the live lock the only gaps beyond
the baseline are Go's function work: `POST /api/functions/{}/versions/prune`, `PruneVersionsRequest`,
`PruneVersionsResponse`, `PrunedVersionResponse`, `RunnerCapabilities`. Known doc gap, older than this batch:
the flattened filter members of the two `requeue-by-filter` request bodies are documented as an `allOf` of
`DispatchJobFilter`, not as properties (names match; a generated client lacks the filter fields).

## What the SPA needs (P5)

Edit-payload dialog (PUT, 409/400 codes above); window picker or default and the `X-Window-*` echo; Failed lens;
event type, subscription and connection filters with their codes (`TOO_MANY_*`); redrive double confirmation
(a second dialog sending `redriveCompleted` + `confirmRedriveCompleted: "REDRIVE"`, shown only with the
`redrive-completed` permission) fed by the preview's `completedCount`/`inFlightCount`/`redriveEligibleCount`;
button gating on `requeue`, `redrive-completed`, `cancel`, `complete`, `edit-payload` (`view` no longer enough);
filter-options `eventTypes`; the bulk requeue sends `window` bounds as `since`/`until`.

## Not verified / open

- The takeover script (`scripts/schema-fingerprint.sh takeover`, Go-migrated database then `fc-migrate`) was not
  run (no Docker here); migration 096 on a Go-086 database is a set of `IF NOT EXISTS` / `DROP IF EXISTS`
  statements and the probe reads the partial index. The fresh-database fingerprint gate passes.
- `scripts/ops/096-...sql` was not executed against a populated database.
- The Rust entity `DispatchJob` still has no `connection_id` (F24): Rust never writes it, it only projects what
  Go writes.
- The new `cancel`/`complete`/`requeue` permissions break callers that held only `dispatch-job:view` until
  migration 095 ran; custom roles created after the migration need them granted.
