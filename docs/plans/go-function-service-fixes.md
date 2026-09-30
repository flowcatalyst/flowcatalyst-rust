# Go function service: fixes needed (hand-off for the Go agent)

Source: a read-only review of `../flowcatalyst-go` on 2026-09-30 (runner: `internal/functions/*`; platform:
`internal/platform/function/**`, `internal/server/functions.go`, `cmd/fcdev/fn*.go`), by two review passes.
Nothing was changed in the Go repo. **Line numbers are the reviewers'; re-read the code before editing.**

**Verified** means the owner's assistant re-read the code and confirmed it. **Reported** means a reviewer read it and
the assistant did not re-check it; confirm each with a failing test before fixing.

## Threat model (owner, 2026-09-30)

The function service **only ever hosts the owner's own code**. So guests are trusted, but callers of the public
entry are not, and the host has bugs. Priority therefore goes to (1) caller-facing problems on the public entry,
(2) reliability bugs in the host itself, and (3) integrity of the platform side. Hardening aimed at hostile guests
is deferred (last section).

Follow `CONVENTIONS.md` throughout: one operation per file, writes through use cases, permission gate before the
body is used, typed ids where `internal/ids` has them, no new raw SQL outside sqlc. Every fix gets a test that fails
before the fix.

## Tranche 1: secrets (do these first, they are small and the first is serious)

### 1.1 Secret and DB values are written to the audit log. Verified.
- **Where:** `internal/platform/function/operations/settings.go:107` and `:179` pass `cmd` (`PutSettingCommand`, JSON key
  `value`) to `usecasepgx.EmitEventScoped`. `pkg/fcsdk/usecase/audit_redaction.go` masks only keys ending in
  `password`, `passwordhash`, `secret`, `secretref`, `passphrase`, `token`, plus fields a command lists through
  `AuditMaskedFields()`. `PutSettingCommand` implements neither, so SECRET and DB values land in
  `aud_logs.operation_json`.
- **Fix:** implement `AuditMaskedFields() []string { return []string{"value"} }` on `PutSettingCommand`. Check every
  other command in the function package for a secret-bearing field (settings, policies, routes, publish manifests).
- **Test:** `TestSettings_SecretAndDB_WriteOnly` only checks the API response and uses the value `"s3cr3t"`. Add one
  that writes SECRET and DB settings with a sentinel and asserts the sentinel is absent from `aud_logs.operation_json`
  and from the event payload.
- **Data:** past rows already hold plaintext. Run a read-only query for `aud_logs` rows of the function-setting
  operations with kind `SECRET` or `DB`, scrub `value`, and **rotate every secret and DSN that was ever written**.
  Decide with the owner how to do the scrub (a one-off migration).

### 1.2 A real `postgres://` DSN is rejected with a 500. Verified.
- **Where:** `internal/platform/shared/encryption/secretref.go:49-62` and `:105-113`: any `scheme://` value whose scheme is
  not an external secret-manager scheme is refused with `ErrUnsupportedScheme`. `operations/settings.go:91-98` wraps
  every error as `usecase.Internal("REPO", "setting write failed", err)`, so the caller sees a 500 without the helpful
  message. Neither `fcdev fn set --db` (`cmd/fcdev/fn_platform.go:498`) nor the API adds the `encrypt:` prefix.
  `control/desired_test.go:77-79` works around it by seeding `"encrypt:postgres://…"`.
- **Fix:** for the function settings path (kinds SECRET and DB), treat the value as opaque plaintext to encrypt unless it
  starts with an external secret-manager scheme (`aws-sm://` and so on). Map `ErrUnsupportedScheme` and
  `ErrNotConfigured` to a validation error (400 or 422) with the message, not a 500.
- **Test:** PUT a DB setting `postgres://u:p@host/db`, a SECRET containing `://` (`https://x?token=abc`) and a plain
  secret; each stores encrypted and decrypts in the desired-state document. `ErrNotConfigured` is a clean 4xx or 503.

## Tranche 2: runner, caller-facing and reliability

Files are under `internal/functions/runner/` unless stated.

### 2.1 Guest `fetch` bypasses the SSRF guard. Verified.
- **Where:** `internal/functions` never references `netguard`. `capabilities.go:134-138` builds the client with a plain
  `net.Dialer`; the only control is the hostname allowlist (`hostAllowed`, `capabilities.go:112-130`) plus `CheckRedirect`.
  An allowlisted name that resolves or rebinds to loopback, private ranges or `169.254.169.254` is reachable.
  `docs/function-runner-plan.md:358` claims a dial-time check that the code does not do.
- **Fix:** use `netguard.Default.DialContext(dialer)` (the `internal/router/mediator.go:231` pattern). Drop
  `Proxy: ProxyFromEnvironment` or document it. Decide whether an allowlist entry without a port should default to
  80 and 443 (`capabilities.go:116` currently matches any port). Keep `FC_DELIVERY_ALLOW_LOOPBACK` for `fcdev`.
  Reject IP literals, `localhost` and broad wildcards such as `*.com` in `abi/describe.go` `hostPattern`.
- **Test:** an allowlisted name resolving to 127.0.0.1 is blocked; a redirect to a private address is blocked; an IP
  literal and the metadata address are blocked; loopback works only with the dev policy; the 16 MiB response cap.

### 2.2 The body is read before authentication with no ceiling. Verified.
- **Where:** `invoke.go:150-184` reads the body (`http.MaxBytesReader`) before webhook or platform auth. `ep.MaxBodyBytes`
  (a pointer, `abi/describe.go:42`) overrides the function limit with no upper bound. The servers
  (`internal/server/functions.go:186-194`) set only `ReadHeaderTimeout`.
- **Fix:** add a hard runner ceiling on the body size regardless of the manifest; verify a platform bearer token
  before reading the body; set a `ReadTimeout` (or per-request read deadlines) on the public and private servers.
- **Test:** an endpoint declaring a huge `maxBodyBytes` is capped; an unauthenticated platform request is refused
  without the body being read; a slow-body client is cut off.

### 2.3 Unbounded metric labels from unauthenticated input. Verified.
- **Where:** `invoke.go:101` calls `r.metrics.observe(t.address, 0, rerr.code, start)` with `t.address` taken from the URL
  on a miss; `metrics.go:39-40` creates a counter and histogram series per address. `/metrics` is served
  unauthenticated on the same port as `/fn/*`, bound to `0.0.0.0` by default (`internal/server/functions.go:56-58,157`).
- **Fix:** use a fixed label (`unknown`) on a miss. Consider serving `/metrics` on a separate port.
- **Test:** request 1,000 random addresses and assert the series count does not grow.

### 2.4 A transient load failure is permanent. Reported.
- **Where:** `runner.go:244-248` keeps an existing version when the number and digest are unchanged, even if it is
  `stateFailed`; `prepare` (`runner.go:350-380`) never retries; `invoke.go:356` answers 503 with `Retry-After: 30`.
- **Fix:** retry a failed version with backoff (on the next apply or maintenance tick); do not pin a failed version.
- **Test:** an artifact download that fails once then succeeds makes the version ready without a restart.

### 2.5 A version closed before it finishes preparing leaks memory. Partly verified (no `stateClosed` check found).
- **Where:** `prepare` waits on `prepareSem` (`runner.go:351`); if `close()` runs first (`state.go:128-144`) `compileLocked`
  (`runner.go:357-400`) never checks for `stateClosed`, sets `stateReady`, compiles and pre-warms an instance charged
  to the budget on a version already removed from every map.
- **Fix:** check `stateClosed` after taking `v.mu` and abort, closing anything compiled.
- **Test:** race prepare against close under `-race`; the budget returns to zero.

### 2.6 A lock held across download and compile stalls everything. Reported (lock structure verified by the reviewer).
- **Where:** `resolve` holds `r.mu.RLock` while `currentState()` blocks on `v.mu` (`invoke.go:322-352`); `prepare`
  (`runner.go:357-374`) and `acquirePool`'s recompile (`state.go:96-104`) hold `v.mu` through I/O. A pending `apply()`
  writer then blocks every reader: all invocations, the heartbeat (`runner.go:503`) and maintenance (`:425`).
- **Fix:** keep state in an atomic and never hold `v.mu` across I/O; make the lock wait honour the request context.
- **Test:** while one version's artifact fetch is blocked, invocations of other functions and the heartbeat stay prompt.

### 2.7 A failed shared-JS-engine compile is cached forever. Reported.
- **Where:** `runtimes.go:95-101` stores the result in a `sync.Once`, so a failed or cancelled first compile fails every
  JS function until restart. **Fix:** retry on failure; do not cache a cancellation. **Test:** first compile fails, second works.

### 2.8 Shutdown order is inverted. Reported.
- **Where:** `internal/server/functions.go:178-183` runs `r.Run` (unloads everything, closes the engine) and only then
  `Server.Shutdown`, so requests during the drain get 404 `FUNCTION_NOT_FOUND`. `r.ready` is never set false
  (`runner.go:161-178`); `prepare` goroutines are not waited for before `eng.Close`; `DrainGrace` (30 s) equals the
  default timeout, so `pool.close` can run under live instances (`state.go:128-143`).
- **Fix:** mark not-ready (503) and stop accepting first, then drain, then unload, and wait for prepares before closing
  the engine. **Test:** requests during a drain get 503; no instance is closed while a call is in flight (`-race`).

### 2.9 Smaller caller-facing fixes. Reported.
- **Error text to callers:** `invoke.go:211` and `:356` write `err.Error()` (wazero and control-plane text) to the
  public entry. Return a generic body and log the detail. A client disconnect is counted and answered as 504
  (`invoke.go:268`, `engine.go` `classify`); a body read error is always 413 (`invoke.go:157-161`).
- **CORS:** `abi/describe.go` allows `origins: ["*"]` with `allowCredentials`, and `handleCORS` (`invoke.go:386-390`) then
  reflects any origin with credentials. Reject it at validation. Use `Set` (not `Add`) for the runner's CORS headers
  (`invoke.go:290-294`); require a response status of 200 or more (`invoke.go:284` accepts 100-999).
- **Panic leak:** from `pool.get` to `pool.put` (`invoke.go:214-265`) only the marshal-error path returns the instance;
  add a `defer` so a panic does not leak the memory charge.
- **Webhook replay:** the signature covers only timestamp and body (`auth.go:128-133`, 300 s window, no replay cache), so a
  captured delivery can be replayed to any webhook endpoint of the same function. Bind the method and path, and
  reject non-POST methods on webhook endpoints with an empty method list (`abi/describe.go:137`).

## Tranche 3: platform integrity (`internal/platform/function`)

### 3.1 Concurrent promote and retire are unserialised. Reported.
- **Where:** only publish locks the function row (`repository.go:281`). `PutAlias`, `DeleteAlias`, `RetireVersion` and
  `DeleteFunction` read then write without a lock (`alias.go:87`, `:274`; `publish.go:350`). Two concurrent `live`
  promotes can each reconcile against their own snapshot (`wiring.go:224`, `:385`), leaving subscriptions and jobs from
  both versions with the alias pointing at one; retire versus promote can leave `live` pointing at a RETIRED version.
- **Fix:** `SELECT … FOR UPDATE` on `fng_functions` first in all of these operations (and `UpdateFunction`). Consider an
  `expectedVersion`/`If-Match` compare-and-swap on alias updates (Rust returns 412 `ALIAS_VERSION_CONFLICT`).
- **Test:** concurrent promotes end with exactly one version's wiring; retire versus promote never leaves a dangling alias.

### 3.2 A duplicate subscription or schedule in a manifest makes promote fail with a 500. Reported.
- **Where:** `abi/describe.go:173-206` does not reject duplicate `(eventType, path)` or `(path, cron)`; the second hits the
  unique index at promote. Rust rejects them at parse time (`SUBSCRIPTION_DUPLICATE`, `SCHEDULE_DUPLICATE`).
- **Fix:** add the checks to `Describe.Validate`. **Test:** a manifest with a duplicate is refused at publish.

### 3.3 Heartbeat is unscoped, unguarded, and writes no event or audit row. Reported.
- **Where:** `control/heartbeat.go:76-90` lets any runner credential mark any function's version READY or FAILED; the SQL
  (`function.sql:59-63`) has no `WHERE status = 'PUBLISHED'`, so a concurrent retire can be overwritten (RETIRED to
  READY); no event or audit row is written (Rust emits both via `MarkVersionReadyUseCase`).
- **Fix:** scope to versions in the runner's pool; make the transition an atomic conditional update; emit the event and
  audit row through a use case. **Test:** a retire during a heartbeat stays retired.

### 3.4 One flaky runner permanently FAILs a good version. Reported.
- **Where:** FAILED is terminal; re-publishing the same digest returns the existing FAILED row (`publish.go:117-123`);
  `waitReady` in the CLI errors on FAILED and spins to timeout on RETIRED (`fn_platform.go:288-296`).
- **Fix:** allow a re-publish of the same digest to retry a FAILED version (or add a retry action); handle RETIRED in
  `waitReady`. **Test:** re-publish recovers a FAILED version.

### 3.5 Function-owned subscriptions are editable through the ordinary API. Reported (by grep).
- **Where:** `subscription/operations/update.go`, `delete.go` and `pause.go` never check `FunctionID`/`SourceFunction`; only
  sync protects them (`sync.go:282`, `:343`). A paused subscription is never resumed by promote, while scheduled jobs
  are forced Active (`wiring.go:428`).
- **Fix:** refuse (409 or 403) edits, deletes and pauses of function-owned rows outside the function operations, for
  both subscriptions and scheduled jobs; make promote heal both the same way.

### 3.6 Changing `pool` does not rewire. Reported.
- **Where:** `UpdateFunction` (`update.go:54-76`) changes pool, warm and limits with no pool-revision bump; target URLs embed
  the pool (`wiring.go:163-166`) and go stale until re-promote. `Pool` is unvalidated and a value over 100
  characters gives a 500 (`create.go:132`).
- **Fix:** bump the revision of both pools and re-wire target URLs, or refuse a pool change on a promoted function;
  validate `Pool` as a DNS label.

### 3.7 Smaller platform fixes. Reported.
- **Emit dedup ids are global:** `control/events.go:81-106` uses the function-supplied `DedupID` unscoped, so a collision
  across functions silently drops an event. Prefix with the function id; do not let the function set an arbitrary `source`.
- **Artifacts:** `DeleteFunction` counts orphans in its transaction but deletes the blob after commit (`delete.go:95-114`,
  `api.go:223`), which can lose a concurrent publish's blob; uploads buffer the whole 64 MiB body (`api.go:235`,
  `s3.go:65`); `FileStore.Put` does not fsync before rename; orphan blobs are never collected.
- **N+1:** the desired-state document runs about 4 queries per function on every 15 s re-render (`buildFunction`,
  `buildVersions`). Batch them (the repo's own rule).
- **Manifest ceilings:** endpoint `timeoutMs` and `maxBodyBytes` have no upper bound; describe lists and payloads have no
  count or size caps; `httpAllow` accepts IPs and `*.com`; local `fcdev fn build` and `deploy` skip the emit-ownership,
  cron and timezone checks that publish makes (`fn.go:121-148`).
- **Route integrity:** deleting an alias does not check routes that reference it (`alias.go:274`); `PathPrefix` has no length
  or charset validation (`routes.go:79-80`).
- **CLI credentials:** `fcdev fn set --secret K=V` and `--db` put values on argv; add a stdin or env-file path. `fn run` prints
  the webhook secret to stdout.
- **Conventions:** raw SQL sits in operations (`publish.go:366`, `delete.go:102`) and the hand-written
  `repository_control.go`/`repository_routes.go`; function, version and setting ids are untyped strings (`internal/ids`
  covers Principal, Client and Application only). See `docs/plans/go-typed-ids-handoff.md`.

## Decisions needed from the owner

1. **Instance reuse:** the pool reuses instances across requests, up to 10,000 calls (`pool.go`, plan section 6.4), unlike Rust's
   fresh instance per request. Keep it, and document that functions must be stateless?
2. **Runner credentials:** any runner credential can request any pool and receives every function's decrypted secrets,
   DSNs and webhook secrets (`repository_control.go:72-97`). Bind credentials to pools? (`decryptSettingValue` also
   returns the raw stored value on decrypt failure, so an `env://` or `aws-sm://` reference reaches the runner as if
   it were the secret.)
3. **Existing audit data:** who scrubs `aud_logs` and rotates the affected secrets (1.1)?
4. **Manifest schema:** Go's `fc_describe` document differs from the Rust and Java manifest (default subscription mode
   NEXT_ON_ERROR against IMMEDIATE; subscription identity `(eventType, path)` against `eventType`). Irrelevant if only
   one implementation continues.

## Deferred (written for untrusted guests; revisit only if that changes)

Host-call amplification caps (fetch, emit, log, DB calls per invocation, log rate); host memory not in the budget (fetch
and DB buffers, wazero code and stack) and no stack ceiling; the transaction cap and 4-connection pool per DSN; SQL
placeholder rewriting quirks (`E'\''`, jsonb `?`); hop-header handling of `Connection`-named headers; webhook secret
rotation overlap; route-matching niceties (`Allow` header on 405, `%2F`, unclean paths); artifact and compile caches
never pruned. Observability additions are worth doing later: per-host-call metrics, pool gauges (idle, in use, cold
starts), queue and wait time, trace-header propagation, and a reason on the 504 log line.

## Checked and fine (do not churn)

Import allowlist and exact signatures (`engine/check.go`); memory cap, allocator and budget accounting; deadlines via
`WithCloseOnContextDone`; host-call frame bounds checks; every `/api/functions` write handler calls a permission gate
and every use case re-checks scope on the loaded function; secrets encrypted at rest and never returned by reads;
artifact digest checks and no path traversal; control-plane routes need anchor scope plus `runner:control`; promote
wiring commits atomically with events and audit; sync skips function-owned rows; function delete unwires everything;
retire refuses when an alias still points at the version; emit checks the declared `emits` and application ownership.

## Definition of done

`go vet ./...`, `make analyze` (uowseal, idconv), golangci-lint clean, `go test -race ./internal/functions/...
./internal/platform/function/...` including the Postgres-backed tests, and every fix above has a test that fails
without it.
