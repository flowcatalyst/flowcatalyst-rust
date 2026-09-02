# Owner questions — consolidated ledger

_Every open "load-bearing or accident?" question and every ruling already taken,
collected from the Javalin rewrite's behavioural specs (`../flowcatalyst-javalin/docs/spec/*.md`,
`docs/backlog.md`, `docs/STATUS.md`, `conformance/go-runner.md`, as of its 2026-08-27
commit). The specs were extracted from the Go platform, so each question describes
**what the Go does today**; the Rust port inherits the same behaviour until ruled
otherwise._

**Convention.** No ruling ⇒ keep the Go behaviour. A ruling becomes a spec line +
conformance test (+ a "deliberate deviation" note if behaviour changes). IDs are
stable; answer inline after `Ruling:`.

Counts: **Part A** 27 rulings already taken · **Part B** ~290 open questions in 29 areas.

---

## Part A — Rulings already taken (apply to the Rust port; nothing to answer)

| ID | Ruling | Source |
|---|---|---|
| A-01 | **`NEXT_ON_ERROR` ≠ `BLOCK_ON_ERROR`.** NEXT_ON_ERROR: a failed head is failed (after the retry policy), surfaced for human review, siblings continue; it is never retried in front of them. BLOCK_ON_ERROR: the failed head follows the retry policy then fails; the untried siblings are **ACKed off the broker** and the group waits platform-side; a reviewer's *ignore / completed / resend* re-queues the whole group in order. (2026-08-22, confirmed 2026-08-25 over Go's NACK-the-siblings.) **Residual gap, unbuilt:** nothing marks those siblings `FAILED` for review, and the whole design depends on the platform re-queue path existing and being correct — a bug there is silent data loss. **RE-CONFIRMED (owner, 2026-09-01): ACK-the-siblings stands; Go's NACK is to be fixed to match.** The platform re-queue path is committed work, not optional — it must exist before (or with) the router adopting this: (1) the ACKed siblings are marked platform-side so the group is visibly pending, and the failed head is marked `FAILED` for review; (2) the reviewer's *resend* is a use case that mostly just marks the group's records back to `PENDING` — the poller/scheduler then re-publishes them in order (the predecessor system using this router works this way); (3) *ignore*/*completed* are the DJ-1 verbs. Until that lands the Rust router must not ship `BLOCK_ON_ERROR`'s ACK branch. | router Q1; STATUS 08-25; re-confirmed 09-01 |
| A-02 | **No terminal give-up.** No max-attempts, no dead-letter; a message lives until the *queue* expires it. Backoff + per-endpoint breaker are the protection. | router Q2 |
| A-03 | **One named retry policy.** Collapse in-call HTTP retries + pool backoff into a single policy object, observable schedule unchanged, pinned by a conformance test. | router Q3 |
| A-04 | **Carry the real 2xx status** in the success outcome (constructor takes it). | router Q51 |
| A-05 | **Any target may `flushGroup`** (no per-pool opt-in). Logged to revisit: the router ACKs undelivered messages on a target's say-so. | router Q54 |
| A-06 | **Unfollowed 3xx is permanent** → ACK-drop, not retry-forever. | router-fixes Fix 3 |
| A-07 | **Postgres quarantine table = `queue_messages_failed`, keep the LATEST failure** (answers drift-manager's "first cause wins?" — no, latest wins). | STATUS 08-25 |
| A-08 | **`AUTO_ACKNOWLEDGE_AGE` = 1 hour.** | STATUS 08-25 |
| A-09 | **Unspecified/unknown `dispatchMode` ⇒ `NEXT_ON_ERROR`** at the router layer (Go applied it at every layer in `89b195e`). The *subscription* layer is still open — see X-01. | STATUS 08-25 |
| A-10 | **Config poll interval = 5 min.** (Whether it is env-tunable: open, R-31.) | router Q31 |
| A-11 | **Pre-flight rejections must not record a breaker success**; 501 = configuration error, ACK-drop with a CONFIGURATION warning. | router-fixes Fix 6; go-runner |
| A-12 | **`/oauth/userinfo` (and discovery) move to the public route group** — identity tokens were 401'd before the handler. | auth-core Q4 |
| A-13 | **`client_credentials` accepts HTTP Basic** via the shared `authenticateClient`; Basic is kept (RFC 6749 §2.3.1). | auth-core Q6 |
| A-14 | **Basic-only clients are per-client rate-limited**: resolve the client id from Basic *before* the rate-limit decision. | auth-core Q5 |
| A-15 | **Token-endpoint errors are 400** (RFC 6749 §5.2); only `invalid_client` may be 401. | auth-core Q7 |
| A-16 | **Discovery advertises the signing algorithm actually in use.** | auth-core Q8 |
| A-17 | **`defaultScopes` is `array<string>` everywhere**; the `scopes` alias is gone. | auth-core Q9 |
| A-18 | **Cookie-mint failure after a correct password = 500**, fixed message, cause logged. | auth-core Q10 |
| A-19 | **`GlobalLockSecs` is a real lock**: deny while over-ceiling AND now < max(lockEnds, countEnds); `Retry-After` = the later. | auth-core Q11 |
| A-20 | **Prune `iam_rate_limit_events`** on the housekeeping loop with retention `MaxWindow()`. **Port hazard (found 2026-09-02, Go fix a88164f): the check-and-record must count its own just-inserted event — Go's single-statement data-modifying CTE shared a snapshot with its COUNT, admitting one request past every ceiling. In Rust, either run insert+count in one transaction as separate statements, or add the inserted row to the count explicitly; pin with a test that the (limit+1)-th call is denied.** | auth-core Q12 |
| A-21 | **Authorization-code replay does NOT revoke the refresh family** — kept as-is, logged as an improvement, not a port item. | auth-core Q13 |
| A-22 | **Client-secret rotation has a grace window**: optional `graceSeconds` (24 h default, 0 = cut over), `previousSecretExpiresAt`, `POST …/revoke-previous-secret`; both compares always run; expiry enforced on read. | auth-core Q14 |
| A-23 | **`expires_in` is derived from the configured access TTL**, never a literal. | auth-core Q15 |
| A-24 | **`GET /api/config/platform` exists, is public, outside the lockfile** (SPA pre-login store depends on it). | publicapi Q1 |
| A-25 | **The platform WILL emit CORS headers** (filter semantics still open, CORS-3). | cors §9 |
| A-26 | **Audit-trail actor bug**: event records must never shadow the `DomainEvent` actor accessor with a subject field (guard test). | backlog 08-24 |
| A-27 | **Extract `dispositionOf(outcome)` as a pure function** so the conformance runner can assert the message's fate (`DELIVERED / RETRY_IN_PLACE / RETURN_TO_BROKER / REJECTED / UNDELIVERABLE`). | go-runner Phase 2 |

---

## Part B — Open questions

### X. Cross-cutting (rule on these first)

- **X-01** Two `DispatchMode` enums with opposite defaults (router: `NEXT_ON_ERROR`; subscription: `IMMEDIATE`, silent on unknown). Merge into one shared enum — does the merged default become `NEXT_ON_ERROR`, which also changes how **existing rows** with null/unknown mode read? (Also: subscription create currently always stores `IMMEDIATE` and ignores input `mode` — SUB-7.)
  **Ruling (2026-09-01): merge into one shared enum; default `NEXT_ON_ERROR`.** Applies at every layer (wire parse fallback, fan-out mode string, subscription create, column DEFAULT) — the same ruling Go applied in `89b195e`. Existing rows with null/unrecognised mode now read as `NEXT_ON_ERROR`. Closes SUB-7 (create must store the input `mode`; sync must honour it) and DJ-12.
- **X-02** `archiveUnlisted` on `POST /api/applications/{appCode}/scheduled-jobs/sync` sweeps the **whole `clientId` scope**, not the application; `clientId: null` sweeps every platform-scoped job on the instance. Options: (a) keep + document; (b) narrow to `clientId + applicationId`; (c) refuse platform-scope `archiveUnlisted` unless anchor. The same global-sweep shape exists for dispatch pools (DP-5) and principals (PR-2).
  **Ruling (2026-09-01): (b) + (c).** `archiveUnlisted` / `removeUnlisted` sweeps are narrowed to `clientId + applicationId` (the application that owns the sync route). A sync on the platform scope (`clientId: null`) with a sweep flag is refused unless the caller is an anchor. Deliberate deviation from Go — apply the same rule to dispatch pools (DP-5) and principal role sync (PR-2); Go needs the same change.
- **X-03** `iam_login_attempts` has no retention: deliberate security audit trail (then it needs indexes independent of table size + separate archival) or purge on a retention of *days* (well above `GlobalWindowSecs`)?
  **Ruling (2026-09-01): keep as history; index it; range-partition by quarter on `occurred_at`; retain 3 years.** Housekeeping drops partitions older than 3 years (no row DELETEs), creates next quarter's partition ahead of time, and keeps a `DEFAULT` partition as a safety net. PK becomes `(id, occurred_at)`; local indexes `(identifier, occurred_at)` and `(identifier, ip, occurred_at)`. The backoff query must always carry `occurred_at >= cutoff` so pruning confines it to the current partition. Document the 3-year figure in the retention policy (IPs are personal data). Same change wanted in Go.
- **X-04** Notifier webhook minimum severity is `WARNING` (INFO dropped). Keep?
  **Ruling (2026-09-01): INFO is emitted, but into the warning store, not the webhook by default.** Every warning (all categories, all severities, incl. STALL / QUEUE_HEALTH) goes through the store; the notifier is a severity-filtered subscriber of it with threshold `FC_NOTIFY_MIN_SEVERITY` (default `WARNING`, env-tunable). INFO entries carry a shorter TTL (~1 h) so they cannot crowd out real warnings; `/warnings` gains `severity` and `category` filters. Resolves R-38 (single path through the store).
- **X-05** "Gated by *any* write permission rather than the specific verb" recurs in eventtype, process, subscription, dispatchpool, role, principal. One rule for all: keep coarse, or gate per verb?
  **Ruling (2026-09-01): one permission per use case (domain verb), gated at the controller.** `edit` consolidates create + update; lifecycle transitions (archive, suspend/activate, pause/resume), credential operations (rotate/revoke/mint), grants (roles, client access) and `sync` are separate verbs. Permission codes derive from the use case name; a lint requires every use case to declare its permission and every controller to gate with exactly it (so "exists but gates nothing" cannot recur). Roles may hold wildcards (`platform:admin:process:*`). Migration: the existing umbrella `:write`/`:manage` implies every verb on its aggregate for one release. **Direction:** move away from generic `update` commands — an operation should be the specific thing being done to the aggregate (its own use case + permission); a field-editing `update`/`edit` is kept only where plain field editing genuinely is the operation. Closes ET-4, PROC-5, SUB-11, DP-4, ROLE-8; SJ-5's gating half.
- **X-06** "Lenient enum reads mask bad rows" recurs in eventtype, process, connection, dispatchjob, dispatchpool, edm, subscription, client, scheduledjob, serviceaccount. One rule: keep lenient (row stays readable) or fail loudly?
  **Ruling (2026-09-01): fail loudly.** The read boundary is a typed decode; an unknown stored enum value is a read error for that row (distinct error code, row id logged), and a list containing such a row fails too — never a silent default. Make it truly exceptional by enforcing the value set at the write boundary: PG enum types or `CHECK` constraints on every enum column (migration first scans for unknown values so the constraint cannot fail to apply). Deliberate deviation from Go. Closes ET-7, PROC-7, CONN-4, DJ-11, DP-9, EDM-8, SJ-6, SA-1 (unknown auth type ⇒ reject, never `NONE`), APP-5, AC-21, ROLE-5.
- **X-07** "Idempotent state flips with no 409" recurs in connection, application, subscription, client, dispatchpool, scheduledjob, principal. One rule: keep idempotent, or refuse no-op transitions like event types do?
  **Ruling (2026-09-01): the verb decides.** A verb that names a *target state* (`archive`, `activate`, `suspend`, `makeCurrent`) is idempotent: already there ⇒ success, no state change, **no event and no audit row** (nothing happened — closes PR-9). A verb that names a *transition* (`resume` = from PAUSED, `reactivate` = from SUSPENDED) requires its precondition and answers 409 otherwise — so `resume` on a running or archived job fails, and never un-archives (SJ-1). Transitions that are invalid from the current state (suspend an archived pool) are 409 regardless of verb. Closes CONN-3, APP-1, SUB-9, CL-3, DP-1, SJ-1, PR-9, PROC-2 (archive stays idempotent; event types' `ALREADY_ARCHIVED` 409 becomes a no-op success for consistency).
- **X-08** Sync rollup message groups are shared constants (`platform:processes`, `platform:subscriptions`, `platform:dispatchpools`, `platform.roles`) rather than per application. One rule?
  **Ruling (2026-09-01): per application, no finer.** Sync rollup message group = `platform:<aggregate>:<applicationCode>` (one FIFO lane per app, so applications do not queue behind each other); never per entity or per client (too wide). Subject follows the same shape. Closes PROC-4, SUB-12 (group half), DP-7, ROLE-7 (subject half; empty audit `entity_id` still open), PR-11 (no trailing dot — subject is `platform.principals.<appCode>` or `platform.principals` when application-less).
- **X-10 (design note, ruled 2026-09-01): branded types everywhere.** The Go code carries ids, codes, names, URLs and secrets as bare `string`s. In the Rust port every such value is a newtype (`ClientId`, `PoolCode`, `RoleName`, `EventTypeCode`, `TargetUrl`, `Secret`, …) constructed only through a validating parser at the boundary, so a `ClientId` cannot be passed where a `PrincipalId` is expected and an invalid value cannot exist past the parse. Finite sets are `enum`s — the enum *is* the branded type — with no catch-all variant (see X-06). Wire/DB representation stays the plain string; the brand lives in the type only.
- **X-11 (ruled 2026-09-02): reconfiguration is in-place; pools are never rebuilt for a parameter change.** The rate limiter stays simple — it exists as courtesy toward downstream services' limits, not traffic shaping; concurrency (a plain semaphore) is what bounds bursting and always exists. On a config tick: rate limit and concurrency are hot-swapped on the live pool (shrink is admission-only — running deliveries are never interrupted; the pool converges as they finish — never a blocking acquire of excess permits); buffer capacity likewise adjusts dynamically (admission check only; briefly overfull is fine — Go derives it as concurrency × multiplier, which satisfies this automatically). A NEW pool just starts. A REMOVED pool stops admitting, drains its buffers, then is cleaned up. A REMOVED queue stops polling, and its consumer stays addressable for ack/nack until every pool buffer referencing it is empty, then is cleaned up. (Both implementations already do most of this: hot-swap rate+concurrency exist in both; Rust has draining_pools/draining_consumers; Rust's shrink currently BLOCKS up to 60s acquiring excess permits — change to admission-only convergence like Go.)
- **X-09** `""` on an optional update field: stored verbatim (Go) vs "clear the field" (Java) — applies to identityprovider, application, cors, role, dispatchpool description. One rule?
  **Ruling (2026-09-01): no optional fields on operations — every field an operation edits is required in the request** (absent ⇒ 400 `FIELD_REQUIRED`), so "absent = unchanged" and PATCH semantics do not exist. The value may be empty: `""` *is* "no description"; columns become `NOT NULL DEFAULT ''` with a migration normalising `NULL → ''`. No sentinel text in the data — the UI renders its own "No description yet" placeholder for empty. Nullable stays only for fields where absence is a real domain state (e.g. `connectionId`, `primaryClientId`), and those are cleared by a specific operation, not by a field convention. Closes IDP-3 (`""` clears; `hasClientSecret` false for empty), APP-9, CORS-4 (normalise to `""`, not `null`), ROLE-9, DP-8 (description half), SUB-5 (a `clearConnection` operation), PC-7 (400 `FIELD_REQUIRED`).

### R. Message router (`spec/router.md` §13; Q1–3, 51, 54 ruled → Part A)

**Delivery semantics**
- **R-04** Prod request timeout 15 min × 3 in-call attempts can hold a worker ~45 min while queue visibility (120 s) lapses repeatedly. Keep 15 min? Wire `ExtendVisibility` (implemented on every backend, never called) at ~50 % of visibility, or keep it dead?
  Ruling: **(2026-09-01): keep the 15-min timeout; `ExtendVisibility` stays dead for now.** Two follow-ons: (1) **observability requirement** — the router UI must show the message groups currently blocked/held, the pool each belongs to, how many messages each group holds, and that pool's rate/concurrency settings; (2) **future (logged, not now)** — per-message delivery timeout supplied by the producer, and an optional per-message extended-visibility value the router applies when present.
- **R-05** Go's HTTP client follows redirects (301/302/303 downgrade POST→GET and drop the body; 307/308 replay). Follow redirects at all? Which codes? (A-06 makes unfollowed 3xx permanent.)
  Ruling: **(2026-09-01): do not follow redirects.** The delivery client disables redirect-following entirely; a 3xx response is a permanent, non-retryable outcome per A-06 (ACK-drop + warning naming the Location). Deliberate deviation from Go (which follows, and lets 301/302/303 silently convert POST→body-less GET) — Go should be fixed to match.
- **R-06** `mediationType ≠ HTTP` and targets with no host/port are silently ACK-dropped and count as breaker *success*. Keep silent, or raise a CONFIGURATION warning like 400/404?
  Ruling: **(2026-09-01): raise a CONFIGURATION warning** (same class as 400/404 per A-11), still ACK-dropped, and it must not record a breaker success. Go needs the same change.
- **R-07** `ErrorConnection` bumps the pool's permanent `total_failure` on every retried attempt; `ErrorProcess` does not. Intentional asymmetry?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **R-08** `CircuitOpen` records no pool metric at all. Accident?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **R-09** Internal rate-limiter stall and HTTP 429 share one `rateLimited` counter/series. Keep merged or split?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **R-10** Rate limiter burst = `rpm` (a full minute can fire instantly after idle). Keep, or burst = max(rpm/60, 1)?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **R-11** Half-open admits every concurrent caller once `ResetTimeout` elapses. Keep, or single-probe half-open?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **R-12** Breaker key = full URL string (`…?x=1` and `…?x=2` are separate breakers). Keep, or key by origin+path / origin?
  Ruling: **(2026-09-02): key by origin + path (query string excluded).** Per-endpoint breakers (one endpoint can be down while its neighbours are up); the query string is per-message data and would fragment the failure signal so a dead endpoint never trips. Target URLs carry no query string in practice, so today's keys are unchanged. Go should align.
- **R-13** Ordered messages without a group id share the global `""` group per pool and are fully serialised. Keep, or treat as IMMEDIATE / singleton group keyed by id?
  Ruling: **(2026-09-01): no shared group — an ordered message must carry its `messageGroupId`.** Absent group id on an ordered mode is malformed: ACK + notice (owner said INFO; same severity caveat as R-16). The global `""` group is deleted. **Go implementation note (2026-09-02): Go's shipped default routes an ordered message without a group down the IMMEDIATE path; the ruled ACK+notice behaviour is implemented behind `FC_ROUTER_STRICT_ROUTING` (default off) until the owner confirms every producer sends the routing fields — flipping it on is an operational decision, not a code change.**
- **R-14** `HighPriority` is carried and inert. Keep inert (document) or drop?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **R-15** 2xx `ack=false` detection reads the whole body and requires valid JSON; any other 2xx body = success. Keep?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.

**Producer / ordering contract**
- **R-16** The platform scheduler publishes with `messageGroupId` but **no `dispatchMode` and no `poolCode`** → every platform job is IMMEDIATE in `DEFAULT-POOL`; group FIFO is not enforced by the router for platform jobs. Intended (ordering owned by the processing endpoint / `ack:false` deferral), or should the scheduler set a mode / should a present `messageGroupId` imply ordered? **Decides whether the ordered path is load-bearing in production.**
  **Ruling (2026-09-01): a message arriving with no `poolCode` or no `dispatchMode` is malformed — ACK it and raise a notice (owner said INFO; recommended CONFIGURATION/WARNING since X-04 keeps INFO out of the push channel — pending confirmation).** Consequences: (1) the scheduler must publish `poolCode` and `dispatchMode` from the subscription on every job, so the router's ordered path becomes load-bearing for platform jobs; (2) A-09/X-01's "unspecified ⇒ `NEXT_ON_ERROR`" applies at the platform layer (API input, stored rows) only — at the router wire, absence is a drop, not a default. An *unknown* pool code still falls back to `DEFAULT-POOL` with the routing warning (R-29); only absence is dropped.
- **R-17** A malformed payload on Postgres fails the entire poll and the poison row re-claims forever. Ack/park it instead (as SQS does)?
  Ruling: **(2026-09-01): park it.** A malformed payload is quarantined to `queue_messages_failed` (A-07, latest-failure policy) and the poll continues; never fail the batch, never re-claim a poison row. Go needs the same change.
- **R-18** SQS publishes set no `MessageDeduplicationId` (FIFO needs content-based dedup). Rely on content dedup, or set the job id?
  Ruling: **(2026-09-01): do not set `MessageDeduplicationId`.** Content-based dedup stays (FIFO queues must have it enabled or publishes fail); the router's own in-flight dedup ignores it either way, and a message mediated twice is acceptable — targets are expected to tolerate redelivery (same stance as R-58).
- **R-19** NATS: a redelivery carries a new consumer sequence → classified as external requeue → router acks the duplicate while the original is in flight. Is NATS in scope at all? If yes, broker id = stream sequence only.
  Ruling: **(2026-09-01): NATS is in scope.** The broker message id must be the **stream sequence only** — never the consumer sequence — so redeliveries carry a stable id and the dedup layer sees them as the same message (the consumer-sequence scheme turned at-least-once into at-most-once). Go already fixed this; the Rust port pins it with a conformance test.
- **R-20** `QueueConfig.Connections` is parsed and compared for change detection but never used. Drop, or give it meaning (N poll loops per queue)?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **R-21** `VisibilityTimeout` ignored by NATS (ack-wait from the URI). Accept or map?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **R-22** Default-broker Postgres queue uses visibility 30 s vs config default 120 s. Intentional for dev speed?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.

**Dedup / tracker**
- **R-23** Tracker reap max age 15 min on `LastSeenAt` (skipping retrying entries); reaper tick 5 min; breaker eviction 1 h; tracker > 10 000 → RESOURCE warning. Keep these numbers?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **R-24** `ForceAck` does not abort a running delivery, which may still ACK with a stale handle (logged). Acceptable?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **R-25** Pool stop flushes buffered messages without nacking (Postgres: invisible until visibility lapses). Nack-with-0 on flush for brokers that honour it?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.

**Consumer / config lifecycle**
- **R-26** Consumer restart (stall ≥ 60 s), queue config change, leadership loss and shutdown all abort in-flight deliveries (breaker failure recorded, redelivered later). Allow in-flight deliveries to finish (bounded) instead?
  Ruling: **(2026-09-01): finish what is in flight.** A consumer restart, config change or leadership loss never aborts an in-flight delivery: the in-hand HTTP call **detaches**, runs to completion independently, and resolves its broker action afterward; buffered (not-yet-started) siblings follow the normal release rules. **Design direction (owner):** message-group processors are long-lived — a restart/reconfigure affects *polling*, not *processing*; group workers keep running across consumer rebuilds and config reloads and exit only at process shutdown (or when their pool is removed, after finishing their buffer). Nuance kept: after a leadership loss, detached deliveries complete (duplicate delivery by the new leader is acceptable per R-18/R-58), but no *new* deliveries start.
- **R-27** Suspected startup defect (default-broker): `Reconfigure(bootCtx)` runs under a 10 s context that is cancelled on return, so the poll loop exits immediately and the watchdog rebuilds it only after ~65–95 s, with a spurious "stalled" warning. Confirm; the port should start consumers under the run lifetime.
  Ruling: **(2026-09-02): confirmed defect — fix.** Consumers start under the process-lifetime context, never a boot-scoped one. Fix in Go too.
- **R-28** A consumer that cannot be rebuilt never increments `restartAttempts`, so never escalates to CRITICAL. Accident?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **R-29** One ROUTING warning per message with an unknown pool code. Dedupe per pool code with a TTL?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **R-30** Partial multi-source config failure removes that source's pools/queues until it recovers. Keep, or keep last-known-good for failed sources?
  Ruling: **(2026-09-02): hold the last-known-good config for a failed source** — consumers keep running on it; a small bad change must not stop traffic. Raise a notice (CONFIGURATION warning) while a source is failing/stale, cleared on recovery. Deliberate deviation from Go (which removes the source's pools/queues); Go should align.
- **R-31** Config poll = 5 min (ruled). Should it be env-tunable?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **R-32** `POST /config/reload` returns 500 in default-broker mode (no config source) though a friendly 200 branch exists unreachable. 200-with-note, or 400/409?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **R-33** `POST /config/reload` is not leadership-gated (a follower would start consumers). Gate it?
  Ruling: **(2026-09-02): gate it on leadership** — a follower answers 409 (or 200-noop with a "not leader" note) and never starts consumers.
- **R-34** Default-broker + standby: pools start without leadership and are not recreated after loss→regain. Declare default-broker single-instance only, or gate it?
  Ruling: **(2026-09-02): gate it** — default-broker pools/consumers start only under leadership and are recreated on loss→regain, same as config-sourced ones.
- **R-35** A consumer build error mid-`Reconfigure` leaves it half-applied. All-or-nothing, or keep?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.

**Health / warnings / observability**
- **R-36** `HealthService` pool-success-rate and consumer-liveness inputs are never fed in production (no callers of `RecordPoolResult` / `SetConsumerRunning` / `RecordConsumerPoll`); health is warnings-only. Wire them (then < 90 % success or a stalled consumer degrades readiness), or formalise warnings-only?
  Ruling: **(2026-09-02): feed consumer-liveness into readiness** (`SetConsumerRunning` / `RecordConsumerPoll` wired; a non-polling router is not ready). **Pool success rate stays out of readiness** — a failing target is not a failing router; it remains a warning + metric only. Go needs the same wiring.
- **R-37** One unacknowledged 501 from any target → NOT_READY for the whole router (up to 8 h). Keep 501 = CRITICAL?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **R-38** STALL and QUEUE_HEALTH warnings go to the notifier only, never `/warnings`/health; CONNECTION, RATE_LIMIT, CIRCUIT_BREAKER categories are never emitted. Route everything through the store?
  Ruling: closed by X-04 — everything through the store; notifier subscribes with a severity threshold; emit the three missing categories.
- **R-39** `/monitoring` `active_warnings` = all unacked; `/health` uses unacked ≤ 30 min. Unify?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **R-40** `/monitoring/consumer-health` always returns `{}`. Feed it or drop it?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **R-41** Prometheus: keep the acknowledged contract gap (`fc_messages_submitted_total`, `…rejected_total{reason}`, `fc_consumer_polls_total`, `fc_consumer_errors_total{type}`, `result` label, `flowcatalyst_broker_*` not emitted), or emit the fuller set?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **R-42** Notifier's final flush on shutdown runs under a cancelled context and is lost. Give it a short budget?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **R-43** Suspected auth defect: BasicAuth public-path bypass tests the full path, so under the `/router` mount `/router/health/live`, `/router/metrics`, `/router/openapi.json` require credentials. Confirm; match relative to the mount?
  Ruling: **(2026-09-02): confirmed — fix. Health checks must never require auth** (nor metrics / openapi per the bypass list's intent): match bypass paths relative to the mount prefix. Fix in Go too.
- **R-44** The documented webhook-signing golden vector does not exist; the spec supplies a computed one. Adopt it as the committed vector?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **R-45** Header spelling on the wire: Go canonicalises `X-Flowcatalyst-*`; SDK docs `X-FlowCatalyst-*`; constants `X-FLOWCATALYST-*`. Pick one canonical spelling (receivers case-insensitive?).
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **R-46** `docs/architecture.md` claims payload bytes are signed unmodified, lists HdrHistogram / per-route histograms / sqlite & amqp backends — none true. Spec wins, docs stale?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **R-47** Duplicate config: `common.StallConfig` vs `router.StallConfig`; stall threshold 90 s fallback vs 60 s default; two lock-key defaults (`fc:leader` vs `fc:server:leader`). Collapse each to one value?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **R-48** Dead code: `Reserve()` on the rate limiter, `Consumer.Healthy()`, `Consumer.Defer()`, `ErrNotImplemented`, `RouterError` kinds. Drop from the port?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **R-49** Shutdown drain: is the intended semantic "finish what's in flight, up to 60 s" (then workers must not be cancelled at drain start), or "stop now, rely on redelivery"?
  Ruling: **Clarified (2026-09-02): the group processor is never cancelled.** **Owner confirmed 2026-09-02**: stay up, finish what it was mediating, then shut down. At process shutdown it finishes the message currently in the air (within the drain budget) and **releases the rest of its buffer back to the broker** — it does not attempt to drain the whole buffer, because a deep buffer against a slow target could take arbitrarily long and the orchestrator's kill window (SIGTERM→SIGKILL) would sever deliveries mid-flight anyway; the broker holding the remainder is the safe place for it. Within-process events (reconfigure, consumer rebuild, leadership loss) cancel nothing at all — R-26. **(2026-09-01): "finish what's in flight, bounded."** Drain does not cancel workers at start; it waits for in-flight deliveries up to the drain budget (60 s default), then releases what remains. Follows R-26.
- **R-50** Timing constants (consumer pacing 2 s/1 s/1 s/500 ms; nack delays 5 s/10 s; host-pool watermarks; warning 8 h/1000; notifier 20/10 s; SQS pending-delete 15 min; dashboard 5 s): parity, or free to tune?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **R-52** `GroupFlushRegistry.SuppressedUntil / Clear / Stats` have no callers: an operator cannot ask "why is this group quiet?" nor lift a suppression. Expose on the monitoring API + operator clear, or drop?
  Ruling: **(2026-09-02): expose it.** Monitoring API gains: list active suppressions (group, pool, until-when), and an operator clear to lift one early.
- **R-53** A message ACKed because its group is suppressed records no pool metric; a heavily-flushed pool looks idle. Add a suppressed counter, or accept the blind spot?
  Ruling: **(2026-09-02): count it.** Suppressed ACKs get their own pool metric (and Prometheus series) so a heavily-flushed pool reads busy-but-suppressed, not idle.
- **R-55** The flush registry is per pool (same group id in two pools suppresses independently). Correct, or global?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **R-56** `/monitoring/standby-status.instance_id` reports the lock key, not the election's per-process id — every instance reports the same value. Return the real instance id (one line)?
  Ruling: **(2026-09-02): confirmed — fix.** Return the election's per-process instance id, not the lock key. Fix in Go too.
- **R-57** (drift-pool) 5xx classification: Go pins "app ran and threw" to exactly **500** (502/503/504/transport = hold at broker); Java generalises to "any 5xx that is not 502/503/504" = REJECTED. Which boundary?
  Ruling: **(2026-09-02): reject on any 5xx except 502/503/504.** 502/503/504 and transport failures = "target unavailable" → hold at broker with backoff; every other 5xx (500, 501, 505, …) = "the app ran and answered" → REJECTED per the review flow. Adopts the broader boundary; Go pins exactly 500 and should be aligned.
- **R-58** (drift-pool) `pendingDelete` is TTL-bounded on the assumption that every target is idempotent; nothing enforces it. Accept as a stated assumption, or require/verify it?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **R-59** (drift-manager) Synthesised `{client}-DEFAULT-POOL` pools are never evicted; a short-lived client leaves its pool and worker footprint alive forever. Evict on idle?
  Ruling: **(2026-09-02): evict.** A synthesised `{client}-DEFAULT-POOL` idle past a TTL (no message routed to it) is torn down — its group processors finish their buffers first per R-26/R-49; it is re-synthesised on demand as today.
- **R-60** (drift-manager) `queue_message_errors` / quarantine has no index beyond the PK and no retention. Add a cap/retention?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **R-61** (router-fixes Fix 5) Three warning gaps found by the conformance corpus — confirm they are wanted as CONFIGURATION/ERROR warnings in the port.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **R-62** (router-fixes Fix 10) `queue.Defer` has no production caller. Drop?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.

### AC. Auth core (`spec/auth-core.md` §19; Q4–Q15 ruled → Part A). Default if unanswered = keep as Go.
- **AC-1** Keep `auth_time` = ID-token `iat` (not the real login time)? _(Note: Go later fixed this — role-canonicalisation work shipped real `auth_time`; confirm the port follows Go.)_
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AC-2** Keep `email_verified: true` unconditionally whenever an email exists?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AC-3** Keep writing `PendingAuth:{state}` rows that nothing reads?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AC-16** Treat `RefreshTokenExpirySecs` / `SessionTokenExpirySecs` config as dead (7 d / 24 h compile-time)?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AC-17** Keep two different "standard scope" sets (authorize validation excludes `address`/`phone`; narrowing reserves them)?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AC-18** Drop the three caller-less rate buckets (`oauth_introspect_ip`, `oauth_revoke_ip`, `check_domain_ip`)?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AC-19** Keep "any non-empty HS256 secret" (no minimum length)?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AC-20** Keep "empty grant-type list ⇒ every grant allowed"?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AC-21** Keep lenient `clientType` parsing (unknown ⇒ PUBLIC) on create?
  Ruling: closed by X-06 — reject unknown.
- **AC-22** Keep the implicit `state` length cap (≤ 116 chars) from `oauth_oidc_payloads.id VARCHAR(128)`?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AC-23** Keep fail-open on rate-limit backend errors and ignored backoff errors?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AC-24** Keep `/auth/me` emitting `"status": ""`?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AC-25** Keep `/oauth/authorize` preferring the cookie over Bearer while the middleware prefers the header?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AC-26** Keep introspect's `client_id` = first `clients` entry (`id:identifier` or `*`), not the OAuth client?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AC-27** Keep `/oauth/authorize` per-client 429 in the platform envelope while `/oauth/token` uses the RFC envelope?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AC-28** Keep `/oauth/revoke` ignoring `token_type_hint` and only ever revoking refresh tokens?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AC-29** Keep `GET /auth/check-domain` legacy shape (`providerId` for any IdP type, guessed `authorizationUrl`)?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.

### AI. Auth identity (`spec/auth-identity.md` §19)
- **AI-1** May the port invalidate/refresh the cached OIDC client when the IdP row changes (secret rotation, issuer change) instead of requiring a restart?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AI-2** Keep the empty-string `email_domain_mapping_id` as the provider-direct marker in the stored row (schema compat), modelling it as a sealed mode only in memory?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AI-3** Keep leaking the library's verification error text in `OIDC_VERIFY` messages?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AI-4** Keep "single-tenant provider-direct IdP with no mapped domains accepts any account at that IdP"?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AI-5** Keep the plain-text 500 on session-mint failure, or switch to the `ErrorModel` envelope?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AI-6** Keep failing JIT with `CLIENT_REQUIRED` when a CLIENT/PARTNER mapping has no `primaryClientId` (vs refusing at mapping creation)?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AI-7** Keep `GET /auth/oidc/login?provider_id=` without a portal flow (the Phase-1 inert-principal JIT path) now that the portal plane is separate?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AI-8** When every `allowedRoleIds` entry is dangling, keep "reject all roles" (vs unrestricted)?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AI-9** Keep the GET `/auth/check-domain` legacy shape at all?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AI-10** Keep consuming the portal flow at SSO **start** (no retry after an IdP failure), or consume at the callback sink like the password path?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AI-11** Align `rememberAllowed` with `RememberEnabled()` (require internal domain)?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AI-12** Should `/auth/2fa/verify` enforce the domain's allowed-method list?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AI-13** Fix passkey sign-counter / `last_used_at` persistence and emit `passkey:authenticated` in the port?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AI-14** Should admin-triggered reset tokens set `requires_factor` when the user has TOTP?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AI-15** Add `Date` / `Message-ID` headers and RFC 2047 subject encoding to outgoing mail?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AI-16** Change the notification default brand to `FlowCatalyst`?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AI-17** Purge expired PINs / trusted devices / reset tokens / approvals on the housekeeping loop?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AI-18** Keep the 15-minute portal flow TTL (vs 10 min like OIDC state)?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AI-19** Keep `RequireStrongFactorForReset = false` (no-TOTP users get an e-mailed link; approval queue dormant)?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AI-20** Keep `/auth/2fa/*` token routes public and self-service routes behind the auth middleware exactly as mounted?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AI-21** Keep Go-default time serialisation (RFC 3339 nanos) and `principalId` on `GET /auth/2fa/trusted-devices` items, or align to the platform `Time` shape?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AI-22** Unify the system actor spelling (`""` vs `"system"`) in `aud_logs`?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AI-23** Keep the portal-plane 2FA deferral (no second factor for portal password users)?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AI-24** Keep `authenticate/begin` on huma's 429 shape (vs the `TOO_MANY_REQUESTS` envelope)?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AI-25** Keep passkey events under source `platform:admin`?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.

### PR. Principal (`spec/principal.md` §11 — security-critical; rule on PR-3 and PR-4 first)
- **PR-1** `AssignRoles` rewrites every assignment as `ADMIN_ASSIGNED`, silently adopting IdP- and SDK-sourced rows.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **PR-2** `SyncPrincipals` does not validate role names; `removeUnlisted` strips `SDK_SYNC` roles from every USER not in the payload regardless of application.
  Ruling: closed by X-02 for the sweep — `removeUnlisted` strips only `SDK_SYNC` roles belonging to the syncing application; platform-scope sweep anchor-only. (Role-name validation half still open.)
- **PR-3** Role / application-access / developer-credential mutations have no coarse handler gate and load before authorising → **existence oracle** over the principal table.
  **Ruling (2026-09-01): coarse per-use-case gate first (X-05) — no permission ⇒ 403 before any load; after the gate, an out-of-scope target answers 404 byte-identical to not-found.** Real-but-forbidden and nonexistent are indistinguishable, so the oracle is closed for permissioned callers too. Wire change: the by-id read's cross-tenant 403 becomes 404. Applies to every id-addressed route on scoped aggregates, not just principals. Go needs the same change.
- **PR-4** `/{id}/…` sub-routes are not client-scoped while the by-id read is: a clientA admin gets 403 on `GET /principals/{B}` but 200 on `GET /principals/{B}/roles`. (Pinned by tests both ways.)
  Ruling: **(2026-09-01): yes — every `/{id}/…` sub-route carries the same client-scope check as the by-id read**, answering 404 for out-of-scope per PR-3. Go needs the same change.
- **PR-5** `SetClientAssociation` emits `user:updated` (name only) and writes TO_PARTNER grant rows without `client-access-granted` events.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **PR-6** `/users` accepts `enforcePasswordComplexity` and ignores it; create always enforces.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **PR-7** Two not-found resource names (`Principal_NOT_FOUND` vs `User_NOT_FOUND`).
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **PR-8** `SendPasswordReset` bypasses the envelope (no event, no audit).
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **PR-9** Activate/deactivate are idempotent writes that still emit events.
  Ruling: closed by X-07 — a no-op emits no event and no audit row.
- **PR-10** Version read: Go answers 500 for an unknown id (comment says 404); Java 404.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **PR-11** Rollup subject for an application-less sync: Go `platform.principals.` (trailing dot) vs `platform.principals`.
  Ruling: closed by X-08 — no trailing dot.
- **PR-12** Password length counted in bytes (Go) vs chars.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **PR-13** (design smell) `PrincipalApi` is 1051 lines over 29 routes — split per resource group when the port lands?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.

### ET. Event type (`spec/eventtype.md` §10)
- **ET-1** `clientId` is carried on aggregate/command/event but never persisted. Keep carried-but-unstored, add the column, or drop it?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **ET-2** Consequence: post-create writes are effectively anchor-only (CLIENT principals can never update/delete). Intended?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **ET-3** `?clientId=` list filter suppresses the default `status=CURRENT` while filtering nothing.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **ET-4** Create/update/add-schema gated by any write permission (→ X-05).
  Ruling: closed by X-05 — per use case.
- **ET-5** Sync ignores `schema` and does not set `createdBy`.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **ET-6** `/schemas` alias of `/versions` — still needed by the SPA?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **ET-7** Lenient enum reads (→ X-06).
  Ruling: closed by X-06 — fail loudly.
- **ET-8** `DELETE` summary says "Archive" in the lockfile; behaviour is a hard delete.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.

### SA. Service account (`spec/serviceaccount.md` §10)
- **SA-1** `ParseAuthType` unknown → `NONE`: a misspelled auth type silently sends an **unauthenticated** webhook (same shape as the dispatch-mode ruling). Reject unknown values?
  Ruling: closed by X-06 — reject; a misspelled auth type must never send an unauthenticated webhook.
- **SA-2** Stash TTL (2 min) is a constant. Keep?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **SA-3** `principalId` populated on single read, omitted from list. Keep the asymmetry?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.

### IDP. Identity provider (`spec/identityprovider.md` §10)
- **IDP-1** `code`/`name` stored verbatim on create, `name` trimmed on update — trim both on create?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **IDP-2** Update never re-validates OIDC required fields.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **IDP-3** `""` clears an optional field (Java) vs stored `""` (Go); `hasClientSecret` for an empty secret `false` vs `true`. Confirm.
  Ruling: closed by X-09 — required, `""` = cleared; `hasClientSecret` false for empty.
- **IDP-4** A claim toward an `INTERNAL` IdP converts OIDC users but does not count them in `usersReset`.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **IDP-5** Legacy `oauth_identity_provider_allowed_domains` rows are only cleared on delete — drop the table later?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **IDP-6** `update` emits `updated` even when nothing changed.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.

### PROC. Process (`spec/process.md` §10)
- **PROC-1** `createdBy` never persisted (no column). Keep carried-but-unstored, add column, or drop?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **PROC-2** Archive is unconditional (no `ALREADY_ARCHIVED`) — align with event types?
  Ruling: closed by X-07 — archive is idempotent everywhere; event types align to this, not the reverse.
- **PROC-3** Sync manages `CODE`-sourced rows, so `removeUnlisted` can delete the seeded example.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **PROC-4** Sync rollup group is the constant `platform:processes` (→ X-08).
  Ruling: closed by X-08 — per application.
- **PROC-5** Any-write-permission gating; `process:archive` / `process:manage` exist but gate nothing (→ X-05).
  Ruling: closed by X-05 — per use case.
- **PROC-6** Admin create/update trim `name`; sync stores verbatim.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **PROC-7** Lenient enum reads (→ X-06).
  Ruling: closed by X-06 — fail loudly.
- **PROC-8** `/api/processes/sync` body-scoped Laravel alias — still needed?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **PROC-9** Update stores a blank `diagramType` verbatim while create/sync treat blank as absent.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.

### CONN. Connection (`spec/connection.md` §9)
- **CONN-1** `serviceAccountId` not checked against `iam_service_accounts` at create.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **CONN-2** `clientIdentifier` never set by any operation — dead column or pending feature?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **CONN-3** Pause/activate idempotent (→ X-07).
  Ruling: closed by X-07 — target-state verbs, idempotent.
- **CONN-4** Update `status` read leniently (`garbage` → `ACTIVE`).
  Ruling: closed by X-06 — reject unknown.
- **CONN-5** Platform-wide code uniqueness relies on the pre-check (index treats NULL client ids as distinct).
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **CONN-6** `updated` event carries no `status`; pause/activate indistinguishable from a rename on the wire.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **CONN-7** Audit `operation` for pause/activate: `statusCommand` (Go) vs `PauseCommand`/`ActivateCommand`.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **CONN-8** Deleting a connection that subscriptions still reference is allowed.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.

### APP. Application (`spec/application.md` §11)
- **APP-1** Activate/deactivate idempotent (→ X-07).
  Ruling: closed by X-07 — idempotent, no event on a no-op.
- **APP-2** Attach answers `APPLICATION_HAS_SERVICE_ACCOUNT`, provision `ALREADY_PROVISIONED` (both 409) for the same state.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **APP-3** `hasLoginClient` always `false`; `baseUrlOverride` / `configJson` never populated.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **APP-4** `active=<anything but "true">` lists inactive applications.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **APP-5** `type` not validated (unknown → `APPLICATION`).
  Ruling: closed by X-06 — reject unknown.
- **APP-6** Delete leaves orphaned `app_client_configs` rows.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **APP-7** `GET …/{id}/clients` and `…/by-id/{id}/roles` answer `[]` for an unknown application instead of 404.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **APP-8** `service-account-provisioned.serviceAccountId` carries the SA id while the row stores the principal id.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **APP-9** `""` on an optional update field stored verbatim (→ X-09).
  Ruling: closed by X-09.

### DJ. Dispatch job (`spec/dispatchjob.md` §11)
- **DJ-1** Add `CancelDispatchJob` (`FAILED → CANCELLED`) and `CompleteDispatchJob` (`FAILED → COMPLETED`) — the *ignore/completed* verbs the A-01 review flow needs? Currently only resend (= requeue) exists.
  Ruling (2026-09-01, via A-01 re-confirmation): yes — build them. They are the review verbs the BLOCK_ON_ERROR flow depends on; resend = a use case marking the group's records `PENDING` (poller re-publishes in order).
- **DJ-2** `requeue()` is total — also resets `PROCESSING` / `QUEUED` / `COMPLETED`. Add a precondition (terminal or `PENDING` only)?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **DJ-3** Requeue gated by the **view** permission and silently skips inaccessible/unknown ids (reports a count). Keep, or 403/404?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **DJ-4** Requeue scope predicate: platform `canAccessScope` (super-admin may requeue platform-scoped jobs) vs Go's `client_id = ANY(clients)`.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **DJ-5** Rollup subject segment: minted `{batchId}` vs a constant.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **DJ-6** Empty `ids` still writes the rollup event + audit.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **DJ-7** `filter-options` facets are not tenant-scoped.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **DJ-8** `since`/`until` parse failures ignored; `source` is equality not free-text; list order has no tie-breaker.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **DJ-9** `list-raw` / `{id}/raw` return the same shapes as non-raw (only the gate differs).
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **DJ-10** `attempts` never present on `DispatchJobResponse`; `clientIdentifier` / `priority` never emitted on `DispatchJobRead`.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **DJ-11** Lenient reads (`status` → `PENDING`, `protocol` ignored, legacy object `metadata` → empty) (→ X-06).
  Ruling: closed by X-06 — fail loudly (legacy object-shaped `metadata` needs a one-off migration first).
- **DJ-12** `DispatchMode` shared home (→ X-01).
  Ruling: closed by X-01 — one shared enum.

### SUB. Subscription (`spec/subscription.md` §10)
- **SUB-1** Platform-wide code uniqueness relies on the pre-check; sync does no `(code, clientId)` check and can hit the index (500).
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **SUB-2** `filter` on a binding is accepted everywhere and stored nowhere.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **SUB-3** Sync does not normalise/validate the code nor the target URL; admin create does both.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **SUB-4** Sync `dataOnly` is a plain boolean — an SDK that omits it flips `true` → `false` on every sync.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **SUB-5** Sync clears `connectionId` when omitted; admin update cannot clear it.
  Ruling: closed by X-09 — `connectionId` is cleared by a specific operation; sync must send it explicitly (null = clear) rather than omit.
- **SUB-6** Unresolvable `dispatchPoolCode` silently ignored; admin `dispatchPoolId` stored without existence check and never sets `dispatchPoolCode`.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **SUB-7** `mode` parsed leniently (typo ⇒ `IMMEDIATE`) and **ignored by sync; create always stores `IMMEDIATE`** (→ X-01).
  Ruling: closed by X-01 — unknown/absent ⇒ `NEXT_ON_ERROR`; create stores the input mode; sync honours it.
- **SUB-8** `clientIdentifier`, `clientScoped`, `queue`, `sequence`, `eventTypeId` never set by any operation.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **SUB-9** Pause/resume idempotent; `updated` carries no settings.
  Ruling: closed by X-07 for the flips — `pause` idempotent, `resume` requires PAUSED (409 otherwise). `updated` payload half still open.
- **SUB-10** Numeric settings unbounded; a binding's `eventTypeCode` may be blank; `eventTypes: []` leaves zero bindings.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **SUB-11** Any-write-permission gating (→ X-05).
  Ruling: closed by X-05 — per use case.
- **SUB-12** Rollup type singular (`subscription:synced`), group shared (→ X-08).
  Ruling: closed by X-08 for the group — per application. Singular type name kept.
- **SUB-13** Admin create/update do not check `connectionId` exists; sync does.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.

### AUD. Audit log (`spec/audit.md` §10)
- **AUD-1** `pageSize > 200` resets to 50 rather than clamping to 200 (lockfile says "capped").
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AUD-2** Filtered lists order by `performed_at DESC` only; the cursor list adds `id DESC`. Align?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AUD-3** `clientId` / `since` / `until` / offset exist in the repository filter but no route passes them.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AUD-4** Repository silently corrects out-of-range limits — keep, or programming error?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **AUD-5** No client-scope visibility: any holder of `audit-log:view` sees every client's rows. Intended (platform-admin permission)?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.

### DP. Dispatch pool (`spec/dispatchpool.md` §10)
- **DP-1** Transitions are unconditional flips (archive twice, suspend an archived pool) (→ X-07).
  Ruling: closed by X-07 — archive twice = no-op success; suspend an archived pool = 409.
- **DP-2** `rateLimit` bound: admin ≥ 0, sync ≥ 1. What does `0` mean to the router?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **DP-3** Admin create lowercases the code, sync does not.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **DP-4** Any-write-permission gating (→ X-05).
  Ruling: closed by X-05 — per use case.
- **DP-5** Sync is global: `removeUnlisted` archives other applications' and admin-created pools; not per-pool scope-checked (→ X-02).
  Ruling: closed by X-02 — sweep narrowed to client + application; platform-scope sweep anchor-only. **Implementation note (Go, 2026-09-02): dispatch pools carry no application_id column, so true application-narrowing is not achievable there — the sweep is anchor-gated instead (deviation recorded; revisit if pools ever gain an owning application).**
- **DP-6** `updated` event omits rate-limit/concurrency.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **DP-7** Rollup group `platform:dispatchpools` shared (→ X-08).
  Ruling: closed by X-08 — per application.
- **DP-8** `clientIdentifier` never set; `description` cannot be cleared (→ X-09).
  Ruling: description half closed by X-09. `clientIdentifier` half still open.
- **DP-9** Lenient status read (→ X-06).
  Ruling: closed by X-06 — fail loudly.
- **DP-10** Sync matching by code over all scopes: which row wins when a code exists platform-wide *and* per client; a batch repeating a code creates duplicate platform-wide pools.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.

### EDM. Email domain mapping (`spec/emaildomainmapping.md` §9)
- **EDM-1** `GET /lookup` has no authorization gate (unauthenticated read of a domain's routing + client grants). Intended?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **EDM-2** `identityProviderId` and client ids never validated against their tables.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **EDM-3** Update replaces `primaryClientId` / `requiredOidcTenantId` wholesale (absent ⇒ cleared) and does not re-check `PRIMARY_CLIENT_REQUIRED`.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **EDM-4** `rememberDeviceDays`: create ignores `≤ 0` (→ 30) while update stores any value.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **EDM-5** Lookups match the domain as given while storage is lower-cased.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **EDM-6** `email-domain-mapping:*` permissions exist but every route is anchor-only.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **EDM-7** `scopeType` is immutable (no route changes it).
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **EDM-8** Unknown stored `scope_type` → `ANCHOR`; an unknown stored `method` is dropped on read (Go passed it through). Keep the lenient drop?
  Ruling: closed by X-06 — fail loudly; add the CHECK constraint on the junction table.
- **EDM-9** Seeded catalogue type `platform:admin:edm:*` vs emitted `platform:admin:email-domain-mapping:*`.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.

### PUB. Public API (`spec/publicapi.md` §9; Q1 ruled → A-24)
- **PUB-2** `?clientId=` on `/api/public/login-theme` is ignored server-side. _(Note: Go has since built per-client login themes — `&client=<identifier>` layering CLIENT over GLOBAL, 2026-08-24 — so the port should follow that, not the older spec.)_
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **PUB-3** A failed theme lookup (DB down) returns defaults with a WARN, not 500. Load-bearing (login page must render during an outage)?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **PUB-4** `logoHeight` typing: Go rejects a string/negative/fractional; Java accepts `"48"` and negatives. Keep lenient?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **PUB-5** `messagingEnabled` is hard-coded `true`. Keep static until a flag source exists?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.

### CORS. CORS origins (`spec/cors.md` §10)
- **CORS-1** Origin format admits `*` inside the host (`https://*.example.com`). Intended wildcard support, or regex accident?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **CORS-2** Delete with a blank id: 404 (Go) vs 400 `ID_REQUIRED` (template rule).
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **CORS-3** Filter matching semantics (exact vs wildcard; `Allow-Credentials` emitted?) — to decide when the filter is specified.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **CORS-4** `description` `""` vs `null` stored as given — normalise to `null`?
  Ruling: closed by X-09 — normalise to `""`, column NOT NULL.
- **CORS-5** Host keeps its case and uniqueness is case-sensitive; browsers send lower-case, so an upper-case row never matches. Lower-case in the parser (wire-visible) or leave to the filter?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.

### ROLE. Role (`spec/role.md` §10)
- **ROLE-1** Grant/revoke allowed on `CODE` roles while update/delete are refused.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **ROLE-2** SDK sync's "existing" lookup is by `applicationId`, not name — a same-named admin (`DATABASE`) role makes sync fail with a unique violation.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **ROLE-3** `SyncPlatformRoles` warns past stale-but-assigned `CODE` roles; `SyncRoles` refuses with `ROLE_HAS_ASSIGNMENTS`. Intended asymmetry?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **ROLE-4** Catalogue delete (`DELETE /api/roles/permissions/{permission}`) is a direct repository write — no event, no audit.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **ROLE-5** `/by-source/{anything-else}` lists `DATABASE` roles (lenient parse).
  Ruling: closed by X-06 — 400 on an unknown source.
- **ROLE-6** Create stores `roleName`/`displayName` untrimmed; update trims.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **ROLE-7** Roles-synced rollup subject is always `platform.roles`; audit `entity_id` empty.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **ROLE-8** Any-write-permission gating (→ X-05).
  Ruling: closed by X-05 — per use case.
- **ROLE-9** `description: ""` stores empty; absent leaves old — no way to clear (→ X-09).
  Ruling: closed by X-09 — required field; `""` clears.

### EV. Event read surface (`spec/event.md` §10)
- **EV-1** Confirm singular `POST /api/events` belongs with the batch-ingest unit, not the read surface.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **EV-2** `EventResponse.specVersion` / `subject` / `deduplicationId` emit `""` for NULL (lockfile marks them required). Keep, or make optional?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **EV-3** `principalId` accepted on list routes and ignored (no column). Keep accepting or reject?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **EV-4** Unparseable `since`/`until` silently ignored rather than 400.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **EV-5** `size`/`limit`: default 100, `> 1000` → 100, while lockfile says "default 50, max 1000".
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **EV-6** List orders by `created_at DESC` only — add `id DESC`?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **EV-7** Repository allows eight facet columns; API routes three. Keep the unused five?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **EV-8** Filter options not tenant-scoped (a client-scoped viewer sees every application/subdomain/type).
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **EV-9** `GET /{id}` scans every partition of `msg_events_read` (no `created_at` bound) — bound the partition from the TSID timestamp?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.

### LA. Login attempts (`spec/loginattempt.md` §8)
- **LA-1** List is anchor-only with no permission code. Keep, or introduce a permission?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **LA-2** `identifier` emitted as `""` when absent while other optionals are `null`.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **LA-3** `pageSize > 200` resets to 50 rather than clamping.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **LA-4** Malformed `after` cursor / unparseable dates silently ignored (audit list 400s).
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **LA-5** `attemptType` / `outcome` filters are raw string equality (unknown lists nothing).
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **LA-6** Identifier case-folding is the caller's job; repository compares raw.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **LA-7** `countRecentFailures` exists with no caller — confirm dropping.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.

### PC. Platform config (`spec/platformconfig.md` §11)
- **PC-1** `canRead` is always `true` — drop from the model or expose a read-only grant?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **PC-2** `roleCode` / `applicationCode` not validated against roles/applications (bootstrap before the app exists?).
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **PC-3** Property **delete** emits no event and writes no audit (handler, not use case). Make it `DeleteProperty` + `property-deleted`?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **PC-4** Audit row stores the command JSON, i.e. a `SECRET` value in clear in `aud_logs.operation_json`.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **PC-5** `PUT` answers with the unmasked value even for non-anchor writers.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **PC-6** Unknown `valueType` silently stored as `PLAIN`.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **PC-7** `value` missing: 400 `FIELD_REQUIRED` (Java) vs huma 422 (Go).
  Ruling: closed by X-09 — 400 `FIELD_REQUIRED`.
- **PC-8** Body `clientId: ""` treated as absent (Go: `CLIENT` scope with empty id).
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **PC-9** `PUT` with body-only `clientId` answers 200 with the client value (Go: 500 after the write). Confirm the fix is wanted.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **PC-10** `uq_app_platform_config_key` does not enforce uniqueness for `GLOBAL` rows (NULLs distinct): concurrent first sets can both insert. Accept, or `NULLS NOT DISTINCT` index?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.

### CL. Client (`spec/client.md` §9)
- **CL-1** `INACTIVE` exists in the enum but nothing sets it; deactivate is a hard delete.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **CL-2** Deactivate's `reason` is discarded (not audited).
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **CL-3** Suspend/activate have no preconditions (→ X-07).
  Ruling: closed by X-07 — idempotent; invalid from-state ⇒ 409.
- **CL-4** `PUT` with no `name` re-persists and emits `updated`.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **CL-5** `client:*` permissions exist but every route is anchor-only.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **CL-6** `by-identifier/{identifier}` does not normalise while create lower-cases.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **CL-7** Hard delete leaves `client_id` references dangling elsewhere.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **CL-8** Search is "contains"; Go comment says "prefix".
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.

### SJ. Scheduled job (`spec/scheduledjob.md` §12)
- **SJ-1** No preconditions on pause/resume/archive (`resume` un-archives).
  Ruling: closed by X-07 — `resume` requires PAUSED (409 on running/archived, never un-archives); pause/archive idempotent.
- **SJ-2** `timezone` never validated; unknown zones fire in UTC.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **SJ-3** Sync entry codes bypass the code format rule; sync crons are now parsed (Go did not).
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **SJ-4** Delete orphans instances/logs.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **SJ-5** Instance reads gated by `scheduled-job:view` not `scheduled-job-instance:view`; log/complete writes gated by the job write permission and written outside the envelope (no event, no audit).
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **SJ-6** Lenient instance `status` filter / complete dialect (unknown → `QUEUED`).
  Ruling: closed by X-06 — reject unknown (400 on the filter/complete input).
- **SJ-7** `hasActiveInstance` per row (N+1).
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **SJ-8** FireNow two-phase: instance insert outside the event transaction.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **SJ-9** Audit `operation` name for pause/resume/archive (`transitionCommand` in Go).
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **SJ-10** Forward cron walk unbounded for dense expressions over a long window.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **SJ-11** BFF-only list filters (`clientIds` incl. `platform` literal, `applicationIds`, `statuses`) not ported until the BFF lands.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **SJ-12** Empty cron list items rejected (robfig ignored them).
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.

### SEED. Seeder / bootstrap (`spec/seeder.md`)
- **SEED-1** Event-type names are overwritten on every start — catalogue sync or accident?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **SEED-2** Bootstrap admin only when **no** anchor user exists: the env is ignored forever once any anchor exists. Intended (first-boot only), or should a configured email that does not exist yet still be created?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **SEED-3** By-email idempotency compares stripped-but-not-lower-cased email against a lower-cased column (edge case → unique-index failure).
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **SEED-4** Password strength not validated for the bootstrap admin.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.

### ENC. Encryption (`spec/encryption.md`)
- **ENC-1** Malformed configured key: fatal at `wire_services`, but discarded by `wire_routes` and `cmd/fcdev`. Fatal everywhere?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **ENC-2** Whitespace: Go trims the previous key, not the current. Trim both?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **ENC-3** A v0 envelope whose nonce starts with `0x01` (1/256) is misread as v1 by Go and fails to decrypt. Fall back to a v0 read when v1 fails with every key (GCM makes it safe)?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **ENC-4** Unknown `scheme://` values are treated as plaintext and encrypted. Intended (fail safe), or treat any scheme as external?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **ENC-5** `encrypted:` with a non-base64 payload: Go stores it untouched; reject at the boundary?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **ENC-6** `encrypt:` with an empty payload encrypts the empty string (`""` alone clears). Keep?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **ENC-7** `literal:` — Go's `Decrypt` rejects it (only `secrets.Service.Resolve` honours it). Honour it in decrypt, or `Failed(NOT_ENCRYPTED)`?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **ENC-8** `needsReEncryption`: Go says `true` for short base64 junk and `false` for anything undecodable. Keep Go's edge behaviour?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **ENC-9** `reEncrypt` output shape: Go always emits bare (drops the `encrypted:` prefix); preserve the column's convention instead?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.

### PW. Password hashing (`spec/password-hash.md`)
- **PW-1** argon2id 64 MiB × 4 lanes per verify on the login path — tuned choice or untuned default? (Changing is safe; `needsRehash` upgrades on next login.)
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **PW-2** Accepting legacy argon2i and bcrypt (`$2a$`/`$2b$`, though PHP emits `$2y$`) — still needed, or retire once every row is argon2id?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **PW-3** `p > 255` ceiling on decode with no ceiling on `m` / `t` — DoS guard or accident?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **PW-4** Salt length not compared in `needsRehash`. Intended?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.

### DEV. fcdev (`spec/fcdev.md`)
- **DEV-1** Stub subcommands share exit code 2 with usage errors. Distinct code (e.g. 3)?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **DEV-2** PID-file write failure is a warning, not an error (then `fcdev stop` cannot find it).
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **DEV-3** `--embedded-db-reset` deletes the whole `<path>` including `data.bak-pg*` backups.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **DEV-4** `--database-url` set + `--embedded-db=true` silently skips embedded PG. Warn?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **DEV-5** `stop`: 150 ms poll / 5 s post-SIGKILL wait are not flags.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **DEV-6** `fresh` truncates an explicit table list (with two duplicates) and misses `msg_processes` and `tnt_email_domain_mappings`.
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **DEV-7** `db upgrade` re-seeds through the same path as `start` — must keep doing so (confirm).
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **DEV-8** Distribution: fat single artifact bundling PG binaries (~168 MB) vs resolving the host's PG at install (~50 MB). (Rust: download-on-first-run vs bundle.)
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.
- **DEV-9** `--*-port 0` binds a free port (tests rely on it). Keep?
  Ruling: — deferred (2026-09-02); no ruling yet, current (Go) behaviour stands per the standing convention.

### IMP. Improvements deferred out of the port (`docs/improvements.md`) — not blocking, listed for completeness
- **IMP-1** Auth-code replay does not revoke the refresh family (A-21).
- **IMP-2** Secret-rotation UI: grace control on rotate (0 = "cut over now"), revoke-previous action, and a "still authenticating on the old secret" signal.
- **IMP-3** `flushGroup` safety condition unenforced (A-05).
