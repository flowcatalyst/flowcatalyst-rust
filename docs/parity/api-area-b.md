# API parity, area B: scheduled jobs, processes, event types, events, dispatch jobs, functions

Date: 2026-09-26. Branch `feat/api-area-b` (off `main` @ `f7580210`). The per-area pass of the cutover checklist
("API convergence") for the scenario groups `scheduled-jobs/*`, `processes/*`, `event-types/*`, `events/*`,
`dispatch-jobs/*` and `functions/*`, plus the two files that exercise the same routes (`smoke/event-types.json`,
`login-attempts/login-attempts.json`).

| | |
|---|---|
| Go | `flowcatalyst-go` @ `73a6918` (read-only; `go build -mod=readonly`) |
| Before | parity run 3 on `main` @ `a15dbbf3` |
| After | `feat/api-area-b`, `fc-server` debug build |
| Command | `./target/debug/fc-parity --rust-bin-dir target/debug --only '{scheduled-jobs,processes,event-types,events,dispatch-jobs,functions,smoke,login-attempts}/*'` |

## Before / after

| scenario file | before OK / ACC / DIFF / ERR | after OK / ACC / DIFF / ERR |
|---|---|---|
| scheduled-jobs/scheduled-jobs.json | 25 / 0 / 27 / 0 | **51 / 1 / 0 / 0** |
| processes/processes.json | 27 / 0 / 4 / 0 | **30 / 1 / 0 / 0** |
| event-types/event-types.json | 15 / 0 / 15 / 0 | **29 / 1 / 0 / 0** |
| events/events.json | 9 / 0 / 6 / 2 | **17 / 0 / 0 / 0** |
| dispatch-jobs/dispatch-jobs.json | 18 / 0 / 10 / 0 | **27 / 1 / 0 / 0** |
| functions/functions.json | 5 / 33 / 3 / 0 | 5 / 33 / 3 / 0 |
| smoke/event-types.json | 12 / 0 / 6 / 0 | 15 / 1 / 2 / 0 (with the smoke allow-list entry) |
| login-attempts/login-attempts.json | 10 / 0 / 7 / 0 | 12 / 0 / 5 / 0 |
| **area B groups (first six)** | **99 / 33 / 65 / 2** | **159 / 37 / 3 / 0** |

## What changed

**Scheduled jobs.** Absent members are absent (Go's `omitempty`) on jobs, instances and logs; logs carry
`scheduledJobId`/`clientId`, jobs `applicationId`. `POST …/log` is 204 and requires `level`; `POST …/complete` takes
both of Go's dialects (SDK `status: SUCCESS|FAILURE` + `result`; SPA instance status + `completionStatus` +
`completionResult`) and looks the instance up before anything else (404). Logs of an unknown instance are `[]`.
Scope follows Go: reads refuse a foreign client's job (`FORBIDDEN`, "No access to this scheduled job" / "… instance")
and show platform jobs; by-id writes and the instance callbacks use `CheckScopeAccess` (403 `SCOPE_FORBIDDEN`);
the list is scoped in SQL and ordered by code; `hasActiveInstance` counts a delivered, uncompleted firing only for
a job that tracks completion (one query per page). Update/pause/resume/archive and the callbacks are gated by Go's
`CanWriteScheduledJobs` (create, update or delete; the SDK's instance-write permission still works). Create checks
the lower-cased code (`INVALID_CODE_FORMAT`) and uses Go's codes and messages; pause/resume/archive flip
unconditionally (a repeat is 204).

**Processes.** Archive is unconditional (repeat 204); the list filters on the status as given with no default, and an
unknown status is an empty list; `description`/`createdBy` absent when unset.

**Event types.** Go's `EventTypeResponse` (`eventName`, `source`, `clientId`/`createdBy` when set, spec versions with
`createdAt`). `createdBy` is stored: **migration `053_event_type_created_by`** adds `msg_event_types.created_by`
(Go's 035 has it; `IF NOT EXISTS`, with a probe). `POST …/versions` is Go's `/schemas`: the caller's version, both
members required (huma `VALIDATION`), repeat 409 `VERSION_EXISTS`. The BFF add-schema route now uses the version the
user entered. `PUT` requires a name (`NAME_REQUIRED`); an unchanged update is 204. Writes check scope as Go
(`SCOPE_FORBIDDEN`); the list ignores `clientId`, as Go.

**Events.** `GET /api/events/{id}` reads the read projection, as Go (see the decision below), and answers Go's
`EventResponse`. The list, `/list-raw` and `/raw` (now Go's alias of `/list-raw`) answer Go's `EventRead` rows with
Go's filters, scoped in SQL; a non-integer `limit` is 400 `VALIDATION`. `filter-options` is
`{applications, subdomains, eventTypes}` of `{value, label}`. `POST /api/events` refuses `"data": null` and always
answers a `deduplicationId`. `POST /api/events/batch` reports an item without type, source or data as `BAD_REQUEST`
in its own slot and stores the rest.

**Dispatch jobs.** `GET /{id}` and `/{id}/raw` answer Go's `DispatchJobResponse` (payload, content type, `dataOnly`;
no `isCompleted`/`isTerminal`). `/{id}/attempts` now reads `msg_dispatch_job_attempts` (it always answered `[]`),
with the request summary. The list, `/list-raw`, `/raw`, `/event/{id}` and `/by-event/{id}` answer Go's
`DispatchJobRead` from the read projection, with `clientIdentifier` and Go's filters (`status`, `clientId`, `code`,
`since`, `until`, `limit`, `sort`, …) scoped in SQL. `filter-options` is Go's facet lists. Reads by id check scope as
Go. Requeue/cancel/complete stay gated on `dispatch-job:view`, as Go.

**Login attempts** (known issue in this area). The `client_credentials` grant records the caller's IP on every
`SERVICE_ACCOUNT_TOKEN` attempt; the list treats an out-of-range page size as 50 and ignores an undecodable cursor,
as Go. Failure reasons were already Go's.

## Decision: `GET /api/events/{id}` right after ingest

Rust answered the event straight from `msg_events` after ingest; Go answers 404 until the stream projector has
written `msg_events_read`. Rust now matches Go. Evidence: Go's `FindByID` reads `msg_events_read` like its list does
(`event/repository.go`), and returns the projection's shape (`type`, `projectedAt`, `application`, …) that the SPA's
event detail page binds; reading the write table gave a different shape (`eventType`) and let the detail disagree with
the list it was opened from. In production the projector runs continuously, so the window is the projection lag.
No owner decision covers the old behaviour, so it was not allow-listed.

## Allow-list entries added

`login-as-b` `/permissions/**` on the dispatch-jobs, event-types, processes, scheduled-jobs and S0 smoke files: the
principal holds `platform:messaging-admin`, which in Rust also grants the eight `platform:function:*` permissions, as
Java's `PlatformRoles` defines the role (Direction: the function runner follows Java). All forty Go permissions are
present in Go's order; the extra entries sort first and shift every index. The function-route entries were
re-checked: every allow-listed step is a function route Go serves as its SPA, and Rust meets Java's expected status
on each.

## What remains, and why

| step(s) | difference | owner |
|---|---|---|
| functions `provision-host-service-account`, `host-token`; login-attempts `list-*` (item 3) | a Rust service account's principal id is the service account id (`sub`, `principalId`); Go's differ | service accounts (area C) |
| functions `host-service-account` | `lastUsedAt: null` vs absent | service accounts (area C) |
| functions `host-token` `/scope` | the granted scope lists differ | roles / tokens |
| login-attempts `login-as-b` | `/auth/login` answers `permissions: []` where Go answers `null` for a role-less user | auth |
| smoke `health` | `version` `0.1.0` vs `dev` | platform |
| smoke `list` | Rust's seeded catalogue adds the event types it emits that Go's catalogue lacks (`docs/parity/domain-events-vs-go.md` §7), so the first page differs | kept; needs an owner ruling to allow-list |

Not exercised by the harness but found while matching Go:

- **Dispatch-job `descriptor` and read-side `metadata`** (Go migration 057): Go's fan-out stores the raising
  subscription's name as the job's descriptor and projects metadata into `msg_dispatch_jobs_read`; the SPA's list and
  detail show both. Rust has neither column. It needs a migration plus changes in the fan-out and the projector (the
  delivery path), so it was left for the pipeline owner.
- `POST /api/events` with a known `deduplicationId` answers 200 `isDuplicate: true` with the stored event; Go always
  answers 201 `isDuplicate: false`. The SDKs accept both.
- The scheduled-job and event-type write handlers load the row (404) before the use case validates; Go validates
  first, so an invalid body on an unknown id is 404 on Rust and 400 on Go.

## App impact

- **SPA** (Go's): the event list/detail/filter-options, dispatch-job list/detail/attempts/filter-options and the
  add-schema page now receive the shapes they were written against; the dispatch-job attempt history appears.
- **Laravel SDK** (integral, hr, rfp): scheduled-job instance `log` answers 204 (it treated 202 as success too); an
  app service account holding Go's scheduled-job create/update/delete permissions can now log and complete
  instances, as it could on Go. Events and dispatch-job reads decode Go's shapes.
- **AgentPlanner**: unaffected.
