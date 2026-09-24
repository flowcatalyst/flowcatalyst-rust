# Function Runner Plan (Rust)

Status: plan, 2026-09-24. Nothing is built yet. Java is the reference implementation (owner, 2026-09-24).
Reference: `../flowcatalyst-javalin` at **`0118cdca`**. Pin this commit and don't chase a moving target. Re-baseline
it only at a phase boundary, the same way `docs/java-parity-plan.md` §1.3 does.

> **How to use this doc:** each workstream is sized for one session and names its deliverables (files, symbols,
> tests), its dependencies, and the Java sources it mirrors. To execute one, point a fresh session at its ID,
> for example "execute H4 from `docs/function-runner-plan.md`".

## 1. Goal

A Rust function runner that is a **drop-in for the Java one**: the same HTTP APIs, tables, manifest, desired-state
and heartbeat wire formats, artifact refs and WASM guest ABI. Anything published to the Java platform then runs on
Rust unchanged, and the Java and Rust hosts can serve one pool side by side.

> **Owner ruling (2026-09-24): two different parity bars.**
> - **Management interface: as close to Java as possible.** That covers the function API, manifest, versions,
>   aliases, promote wiring, domains, policies, config and secrets, and the control plane's desired-state and
>   heartbeat. The only exception is where copying Java would compromise the underlying assets.
> - **Functions themselves: whatever makes the most sense, with no Java compromises.** The guest contract,
>   engine and ABI are free: WASI 0.2 components with `wasi:http`, typed WIT host interfaces, and so on. Rust
>   does **not** need to run Java's Extism artifacts. JVM jars stay on Java hosts.
>
> This supersedes the "WASM guest ABI" and "Guest types" rows of §3 and decision 2 in §4. F0 recommends the
> guest contract, H4 implements it, and G1 targets it.

The owner's aim for functions is **density and fine-grained deployment**: many small, independently deployable
units per host (adapters, edge endpoints, customer custom code), at low cost.

**Scope decisions:**

- **Rust hosts `runtime: wasm` only.** That covers Rust, JS (QuickJS through `extism-js`) and TinyGo guests.
  `runtime: jvm` jars stay on JVM hosts. A Rust host that is given a JVM entry reports it `FAILED`
  (`RUNTIME_UNSUPPORTED`) in its heartbeat. The manifest's `pool` field decides placement, so operators keep
  JVM functions in a pool of their own.
- **Both halves get built.** The **control plane** goes inside `fc-platform` (tables, `/api/functions*`,
  `/control/functions/*`, the artifact store and the wiring done at promote). The **host** is a new `fc-fnhost`
  binary. Neither exists in Rust or Go today.
- **The Java W3 (JS guest library) and W4 (`fc_db_*`) work is not on Java's main branch yet.** It lives in Java
  worktrees. Rust follows it once it lands (§6).

## 2. Architecture

```
 fcdev / CI ──PUT artifact, POST version, PUT alias──▶ ┌─ fc-platform (Rust) ────────────────────────────────┐
                                                       │ fn_* tables (mirror Java V13/V15/V16)              │
                                                       │ /api/functions*, /api/function-{policies,domains,  │
                                                       │   routes,pools}, /api/openapi-functions.json,      │
                                                       │   /api/schemas/function-manifest.json              │
                                                       │ promote ⇒ dispatch pool fn-<fid>, subscriptions    │
                                                       │   (source FUNCTION), scheduled jobs, fn_routes     │
                                                       │ ArtifactBlobStore: file:// | s3://                 │
                                                       │ /control/functions/{desired-state, heartbeat,      │
                                                       │   events, artifacts/{versionId}}                   │
                                                       └──────▲──────────────────────────────▲──────────────┘
                            poll 15 s + ETag, OAuth client_credentials │              heartbeat / emit │
 router / scheduler ── signed webhook ─┐                               │                               │
 VPC caller ── bearer ────────────────▶│ ┌─ fc-fnhost (Rust, one per pool, N tasks) ──┴───────────────────────────────┴─┐
 internet (ALB) ──────────────────────▶│ │ reconciler: fetch → sha256 → Sigstore → load new before old → unload        │
                                        └▶│ :8080 private  /functions/{address}[:{version}]/{path}                      │
                                          │ :8081 public   Host header → claimed zone → route (+ alias prefixes)        │
                                          │ :9090 /health /ready /metrics                                               │
                                          │ wasm: wasmtime (Extism ABI), compile once per version, instance pool        │
                                          └──────────────────────────────────────────────────────────────────────────────┘
```

Everything is HTTP. The host never touches the database. Events and schedules reach functions as ordinary signed
webhooks from the platform's dispatcher and scheduler.

## 3. The contract: what "matching" means

Every item below is copied from Java or verified against it. The Java paths are relative to `../flowcatalyst-javalin`.

| Area | Java source of truth | Rust must match |
|---|---|---|
| API routes | `server/.../function/api/{FunctionApi,FunctionDomainApi,FunctionPolicyApi,FunctionControlApi}.java`, `resources/openapi/functions.openapi.json` | Paths, methods, DTO fields (camelCase, nulls omitted), status codes, error codes (`{error,message,details?}`), a 404 for anything out of reach |
| Schema | `M/V13__functions.sql`, `V15`, `V16`; `msg_subscriptions.source` gains `FUNCTION` | Tables, CHECKs, uniques and indexes; TSID prefixes `fnc_ fnv_ fnd_ fnr_` |
| Manifest | `function/Manifest.java`, `resources/schemas/function-manifest.schema.json` | Parsing, normalisation (defaults filled), `MANIFEST_UNKNOWN_FIELD`, the check endpoint collecting every error with a JSON pointer |
| Events and audit | `operations/FunctionEvents.java` | Event types `platform:function:*`, source, subject and group formats; one event plus one audit row per commit |
| Permissions | `shared/auth/Permission.java:271-293`, `seed/PlatformRoles.java` | Nine `platform:function:*` strings; roles `function-publisher`, `function-host`, plus the grants on `messaging-admin` (follow the code where it differs from the spec) |
| Desired state | `operations/DesiredState.java`, `api/ETags.java` | Deterministic bytes, sorted by (address, version), ETag = sha256 of the body, 304 when unchanged, entry fields exactly as Java |
| Heartbeat and emit | `FunctionControlApi.java` | Shapes, 204/201, the PUBLISHED → READY transition, emit error codes |
| Artifacts | `function/artifact/*` | Raw streamed PUT, a 256 MiB cap, `platform://<fnId>/<hex>` refs, `file://` and `s3://` stores, idempotent upload |
| Signatures | `artifact/{Signatures,SignatureVerifier,TrustRoot}.java` | Sigstore bundle v0.3; `FC_FN_SIGNATURES=required` (turning it off needs dev mode); the signer must equal the recorded one |
| Host environment and behaviour | `fnhost/reconcile/HostEnv.java`, `docs/spec/function-host-*.md` | Every `FC_FN_*` variable and default, the 10-step listener pipeline, error codes, the metric names `fc_fn_*`, `/ready` precedence |
| WASM ABI | `fnhost/wasm/{WasmAbi,HostFunctions,CompiledWasm,WasmModuleCheck}.java`, `docs/spec/function-wasm-runtime.md` | Input and output JSON (key order: Jackson's field order, `permissions` sorted), allowed imports, refusal codes, host-function semantics (`fc_secret_get` returns offset 0 when missing; HTTP denial is status 0 with an error body), WASI configuration |
| Guest types | `function-api/` (`Request`, `Caller`, `Result`, `OutboundEvent`, `Webhook`/`Event`/`Schedule`, `FunctionAddress`) | Mirror types in `fc-function-abi`, pinned by Java's own tests and `function-address-table.csv` |

Where the Java spec and code disagree, **the code wins**. The known differences are listed in §9.

## 4. Decisions for the owner before or during execution

1. **~~Customer code WASM-only~~ — resolved (owner, 2026-09-24): functions are *our* code, not tenants'.** JVM jars in class-loader isolation are therefore an acceptable trust model and stay a first-class runtime on Java hosts. WASM is chosen for **density and fine-grained deployment**, not as a security boundary. (Should tenant-authored code ever be admitted, it must be WASM-only — JVM class loaders are not a boundary.)
2. **~~Extism ABI now, Component Model later?~~ Resolved by the ruling in §1: the guest contract is free; F0 recommends it (components + `wasi:http` are the leading candidate).** The Extism ABI is what Java ships, so matching it is required for
   drop-in. WASI 0.2 components (WIT) are the standard that edge runtimes are converging on, and they give typed,
   versioned host interfaces. *Recommended:* ship Extism for parity now, and decide on components before customers
   build on the ABI in volume. Moving later means every guest must be republished.
3. **The guest database connection model (before Java W4 ships).** A pool per function doesn't scale when there
   are thousands of fine-grained units. The options are a pool per tenant, PgBouncer in the path, or a data API
   mediated by the host.
4. **Fuel and memory metering.** wasmtime can meter per call, which enables cost attribution and fair shares per
   customer. It goes beyond Java, so it's an extension (§7).
5. **HTTP egress parity: resolved by F0 (§4a).** With components, egress policy lives in the host's `wasi:http` outgoing-handler, and a denial is a typed error. Java's host allowlist rules are: https only, no redirects, a cap tied
   to the deadline, and a denial returned as status 0 with a body rather than a trap. The Rust `extism` crate can't
   express these. Either vendor or fork the crate (Java did the same with `extism-endive`), or run the Extism
   kernel on plain wasmtime.

## 4a. F0 outcome and runtime decisions (2026-09-25)

The measurements are in `docs/function-runner-density.md` (macOS M4 Pro; redo on Linux in H7).

- **Chosen guest contract:** plain **wasmtime** + **WASI 0.2 components** + **`wasi:http/proxy`**, plus a typed
  **`flowcatalyst:function` WIT package** (`wit/flowcatalyst-function/`) for config, secrets, emit, log and the
  invocation context. The package is an optional import, so a pure `wasi:http` component also runs on
  `wasmtime serve`, Spin and wasmCloud.
  - The extism crate was rejected on merit: 3.0 vs 0.64–0.75 MB per function, and no per-function WASI log routing.
  - Measured: about 0.75 MB per function compiled, sub-MB from `.cwasm`, first-call p99 under 1 ms, steady call
    about 10 µs pooled or about 35 µs instance-per-request.
- **Runtime value:** the manifest keeps `runtime: wasm`, so Java's management interface is unchanged. The Rust host
  sniffs the artifact: a **component** loads, and a **core module** (Java/Extism style) is refused with
  `WASM_CORE_MODULE_UNSUPPORTED`. `entrypoint` names `wasi:http/incoming-handler`. Pools separate the Java hosts
  (jars and Extism) from the Rust hosts (components).
  *Owner decision (open):* adopt an explicit runtime value (e.g. `component`) in Java's schema and DB CHECK
  instead of sniffing.
- **Instance per request** (the standard `wasi:http` model; stateless), made cheap by the pooling allocator.
  This deliberately differs from Java, which reuses instances.
- **Noisy neighbours:** contained with a host-wide executing-guests cap below the core count (`FC_FN_MAX_EXECUTING`)
  plus per-function `maxConcurrency`. No engine meets the +30% p99 bar under saturation without the cap.
- **JS/TS guests: open owner decision.**
  - componentize-js (StarlingMonkey) works but is heavy: 14 MB artifact, 26 MB per function, 0.6 ms per request.
  - The QuickJS component backend didn't build.
  - V8 isolates (`deno_core`) are best for JS (1.65 MB, 2.3 µs), but they are a second engine with a different
    sandbox.
  - Options: wait for or invest in QuickJS components (Javy, componentize-qjs); use componentize-js as is; or add a
    V8 isolate pool for JS.
  - This matters because JSON mapping and transform adapters are natural in TS.
  - H4 ships Rust components first.

## 5. Workstreams

**Tracks:** F = spike, P = platform, H = host, G = guest. P and H can run in parallel after P1 and H1; they meet
at P6 and H3. The sizes below are rough lines of code excluding tests.

### F0: density spike (do this first, about 1 session)
- A throwaway `spikes/fnhost-density` binary. It loads N copies of `fc_test_guest.wasm` (copied byte-identical
  from Java's `function-host/src/test/resources/wasm/`, with its pinned sha256) on (a) the `extism` crate and
  (b) plain wasmtime plus the Extism kernel.
- Measure, on the same machine as a Java host run of the same guest:
  - resident memory per loaded function and per warm instance
  - compile time and first call
  - steady p50/p99
  - a noisy neighbour (`spin`/`alloc`)
- Also answer decision 5: can the crate's `http_request` be overridden, and does `max_pages` cap the kernel's memory?
- Deliverable: `docs/function-runner-density.md` with the numbers and the engine choice. It gates H4.

### Track P: control plane inside fc-platform

**P1: schema, permissions, roles** (about 400)
- A new migration mirroring Java V13 + V15 − V16 (all nine `fn_*` tables, and `msg_subscriptions.source` gains `FUNCTION`), registered with a probe.
- `EntityType` gets `fnc`, `fnv`, `fnd` and `fnr`.
- The nine permission strings go in `role/entity.rs::permissions`.
- Roles: `function-publisher` and `function-host` are added, and `messaging-admin` gets the extra grants, exactly as `PlatformRoles.java` has them.

**P2: value types and manifest** (about 1,200)
- A `function/` module with the value types, one file each: `FunctionAddress`, `FunctionAddressPattern`, `DnsLabel`, `Hostname`, `Digest`, `SettingKey`, `FunctionOwner`, `Runtime`, `EndpointAuth`, `HttpMethod`, `FunctionLimits`, `RoutePattern`, `PoolUrlTemplate`.
- `Manifest` parse and normalise: strict (reject unknown fields), a check mode that collects errors with JSON pointers, and the `FC_FN_DEFAULT_*` defaults.
- Serve `function-manifest.schema.json`, copied byte-identical.
- Tests: port Java's manifest and value-type tests and `function-address-table.csv`.

**P3: functions, config, secrets, policies, domains and routes** (about 2,000)
- Aggregates and repositories, following CLAUDE.md layering, for: `Function`, `ClientPolicy`, `FunctionDomain`, `FunctionRoute`, `fn_config`, `fn_secrets`. Secrets use `EncryptionService` (`encrypted:`).
- Use cases: CreateFunction, UpdateFunction (with `FUNCTION_IMMUTABLE_FIELD` checked against the raw body), DeleteFunction, SetFunctionConfig, SetFunctionSecret, DeleteFunctionSecret, PutFunctionPolicy, ClaimFunctionDomain, ReleaseFunctionDomain.
- Every use case gets the same events and error codes as Java, and `AuditMasked` where a command carries secrets.
- Routes: `/api/functions` (list, get, create, update, delete, status), `/api/function-pools`, `/api/function-policies*`, `/api/function-domains*`, `/api/function-routes`, and the config and secrets routes. An out-of-reach target answers 404 through an `Access::can_reach` port.

**P4: artifacts, publishing and versions** (about 1,500)
- `ArtifactBlobStore`, with `file://` and `s3://` backends, selected by `FC_FN_ARTIFACT_STORE`.
- `PUT …/artifacts/{digest}`: a streamed upload that is hashed as it's written, capped at 256 MiB, with checks in Java's order.
- `PlatformArtifactRef`.
- Sigstore bundle v0.3 verification. Port `SignatureVerifier` or wrap `sigstore-rs`, checked against Java's fixtures in `server/src/test/resources/function/sigstore/*`.
- Use cases PublishVersion (a `nextVersion` row lock, every `checkPublish` code) and RetireVersion.
- Version reads, and `…/manifest/check`.

**P5: promote, aliases and wiring** (about 1,300)
- PromoteVersion and RemoveAlias.
- A port of `FunctionTriggerSync` and `PromotePlan`, which create, update and delete, through their own use cases and events:
  - the dispatch pool `fn-<fid>`
  - subscriptions `fn-<fid>-<hash8>` with source FUNCTION and endpoint `<FC_FN_POOL_URL>/functions/<address><path>`
  - scheduled jobs, and `fn_trigger_objects`
  - `fn_routes`, replaced wholesale
- The dry-run `plan` in `manifest/check`.
- Needs the Rust dispatcher and scheduler to send the signed webhooks the host expects. Verify the header names and signing against `WebhookVerifier.java`.

**P6: host control plane** (about 800)
- `/control/functions/desired-state`: byte-deterministic serialisation (sorted, key order fixed), a sha256 ETag and 304.
- `heartbeat`: host upsert, MarkVersionReady, and a purge of hosts stale for more than 1 day.
- `events`: ingest with `source = function:<address>` and the `EVENT_TYPE_NOT_OWNED` / `HOST_UNKNOWN` / `FUNCTION_NOT_SERVED_BY_HOST` checks.
- `artifacts/{versionId}`: a stream.
- The gate is anchor plus `platform:function:host:control`; with no credential, 401.
- A golden test compares the desired-state bytes with the Java output for the same fixture rows.

**P7: frontend** (about 2,500 of Vue)
- Port the Java SPA pages: `frontend/src/pages/{functions,function-domains,function-policies}` plus `api/functions.ts`.
- Follow CLAUDE.md frontend conventions (PrimeVue, no Tailwind, `useListState`).
- The Invoke tab only shows curl commands, as in Java.

**P8: parity**
- Run `parity/scenarios/functions/functions.json` (41 steps) against Rust through the `SubprocessSide` runner on the Java branch `parity/rust-side`.
- Add the surfaces Java's `parity/surface.json` doesn't cover yet: manifest check, artifact upload and download, and the schema route.

### Track H: host (`bin/fc-fnhost` plus crates)

**H1: `crates/fc-function-abi`** (about 700)
- serde mirrors of `function-api`: `Request`, `Caller` (with `Principal`'s permission helpers, and a matcher pinned against fc-platform's by an agreement test, as Java does), `Reply` validation, `OutboundEvent`, the emit result, `Webhook::event` / `Webhook::schedule`, and `FunctionAddress`.
- Tests: port the 6 accepted and 12 malformed reply rows from `WasmAbiTest`, and the `Caller`, `Result`, `Webhook` and `Request` tests.
- Shared by the host and the guest PDK.

**H2: process skeleton** (about 600)
- `HostEnv`, with every `FC_FN_*` variable and its default, and exit code 2 naming every bad variable.
- slog-shaped JSON logging (the field set matches `Logging.java`).
- `:9090` `/health`, `/ready` (with its precedence) and `/metrics` (the `fc_fn_*` names and labels).
- Graceful drain (`FC_DRAIN_TIMEOUT_SECONDS`, 30 s in-flight wait per function), and `FC_EXIT_AFTER_START`.

**H3: reconciler** (about 1,400)
- `TokenSource` (client credentials, cached until 60 s before expiry, refreshed once on a 401).
- Desired-state poll with ETag: 15 s measured end to start, with `trigger()` coalescing.
- A tolerant document parser (a bad entry is reported FAILED and protected from unload).
- `prepare`: at most 4 concurrent fetches; `platform://`, `file://` and `oci://` stores; a cache at `<cache>/sha256/<hex>` with atomic moves, a 256 MiB cap and re-hashing on every use; Sigstore verification with the signer checked against the recorded one.
- Load new before unloading old, then unload. Close lazy functions after 1 h idle. Reload when the settings fingerprint changes.
- JVM entries → FAILED `RUNTIME_UNSUPPORTED`.
- Heartbeat (ACTIVE / DRAINING). A platform outage never unloads anything.

**H4: WASM runtime** (about 1,500; engine per F0)
- Compile once per version, with a per-version instance pool (a borrow never waits; lazy up to `maxConcurrency`).
- Memory cap = min(`wasmMemoryMb`, the module's max), applied to the guest and the kernel.
- Deadline via epoch interruption. A failed or trapped instance is discarded.
- The import allowlist, and the refusal codes `WASM_INVALID`, `WASM_ENTRYPOINT_NOT_EXPORTED`, `WASM_IMPORT_NOT_ALLOWED`, `WASM_MEMORY_OVER_CAP`.
- Host functions:
  - `config_get` (declared keys only)
  - `fc_secret_get` (offset 0 when missing)
  - `fc_emit_event` → the control plane `events` route, with Java's error codes
  - `http_request` through the host allowlist (a denial is status 0 with `{"error"}`; https only; no redirects; timeout = min(call, remaining deadline, 30 s); flat headers joined with `", "`)
  - `log_*` → logger `fn.<address>`
  - WASI: clock, random, stdout at INFO and stderr at WARN split at 8 KiB, no preopens, no environment, no arguments.

**H5: listeners** (about 1,600)
- axum/hyper, with HTTP/1.1 and h2c on `:8080` and `:8081`.
- The 10-step pipeline with Java's error codes and statuses. Permits via `Semaphore::try_acquire`, host-wide first and then per function (429 `BUSY` with `Retry-After: 1`).
- Body cap. Strip hop-by-hop headers.
- Auth:
  - `webhook`: HMAC over timestamp‖body, 300 s past / 60 s future skew, the previous secret honoured until the second reconcile after rotation.
  - `platform`: RS256 against JWKS via openid-configuration, with an unknown `kid` refetched at most once per 30 s.
  - `none`.
- Versioned calls: bearer + `platform:function:version:invoke` + reach, and a `PinnedVersions` LRU of 8.
- The public listener:
  - resolves the Host / `:authority` header to the longest whole-segment prefix
  - resolves alias prefixes (`qa-api.acme.com` → `qa`)
  - trusts `X-Forwarded-For` only from trusted proxies (`FC_FN_TRUSTED_PROXIES`)
  - answers CORS preflights
  - exposes no `/functions` routes.
- Invocation context: a TSID invocation id, the log fields `function`, `version`, `execution_id` and `correlation_id`, and the emit default precedence for correlation and causation ids.

**H6: conformance harness** (about 1,200 of tests)
- A black-box harness: a fake `/control/functions` (desired state, emit sink, artifact server), log capture, and fixtures from `fc_test_guest.wasm` (hash pinned).
- Port `WasmFunctionListenerTest` (12 cases), `WasmFunctionLoaderTest` (inline WAT through the `wat` crate), `ReconcilerWasmTest` (3) and `WasmFixturesTest`.
- **A differential mode:** run the Java host and the Rust host against the same fake control plane and guests, and compare responses byte for byte (except `invocationId`). This is the drop-in acceptance test.

**H7: benchmarks**
- B1–B5 from Java's `docs/spec/function-host-benchmark.md`, measured on the Rust host and the Java host with the same WASM guests on the same machine. Record them in `docs/function-runner-density.md`.

**H8: fc-dev integration**
- fc-dev runs the platform and an in-process `fc-fnhost` together (the equivalent of Java's `fcdev`).
- Local publish, deploy and invoke of a Rust hello guest, with signatures off only in dev mode.

### Track G: guests

**G1: `crates/fc-function-pdk`** (about 900)
- A Rust guest SDK over `extism-pdk` 1.4, built for `wasm32-unknown-unknown` and `wasm32-wasip1`, kept out of the default workspace build.
- It covers:
  - request helpers (body, text, json, case-insensitive headers)
  - `Caller` helpers
  - result builders `ack` / `retry(Duration)` / `fail` / `json` / `http`
  - a `#[handler]` macro
  - `ctx.config` / `secrets` / `http` (status 0 → `HttpDenied`) / `events.emit` (`ok:false` → `EventEmitError`) / `log` / `now`
  - `Webhook::event` and `Webhook::schedule`
- Tests from H1.

**G2: examples and templates**
- `examples/function-hello-rust` (a Rust WASM guest) plus a `fc-dev fn init --runtime wasm --lang rust` template.
- Java has no WASM hello yet (`examples/function-hello` is a JVM jar). This can go back to Java as its W5 fixture.

## 6. Following Java work that hasn't landed

- **W3, the JS guest library `clients/function-js`.** The guest side only; no host change. When it lands in Java, JS guests run on the Rust host unchanged. Add its example to the H6 differential fixtures.
- **W4, `fc_db_*` host functions** (`docs/spec/function-wasm-db.md`: query, execute, begin, commit and rollback, scoped to the invocation, at most 10k rows / 8 MiB). Implement it in H4 once Java lands it, after decision 3 above. Rust keeps `FC_FN_MAX_DB_POOLS` and the pool-per-DSN reference counting only if that's what the owner rules.
- **W5, where the SPA enables `wasm`.** Carry it through in P7.

## 7. Extensions beyond Java (owner decision; not part of drop-in)

| Extension | Why (density and granularity) | Notes |
|---|---|---|
| Fuel and memory metering per call | Per-customer cost attribution, fair shares, and billing | wasmtime fuel; export as `fc_fn_fuel_total{address}` |
| Per-tenant pools and quotas | Limit the blast radius per customer | Manifest `pool` already exists; add a policy ceiling per client |
| Per-endpoint deploy units | Finer granularity than a function | Would need manifest and ABI changes, so coordinate with Java |
| WASI 0.2 Component Model host | Standard, typed interfaces; edge portability | Decision 2 |
| Weighted aliases (canary) | Safer rollouts | Java lists this as later (P4) |

## 8. Order, dependencies and effort

```
F0 ─┐
P1 ─┼─ P2 ─ P3 ─ P4 ─ P5 ─ P6 ─ P7 ─ P8
H1 ─┼─ H2 ─ H3 ──────────────┐
    └──────── H4 (after F0) ─┴─ H5 ─ H6 ─ H7 ─ H8
G1 (after H1) ─ G2
```

- **Minimum drop-in host,** usable against the Java platform with no Rust control plane: F0, H1–H6. About 8 sessions.
- **Full Rust stack:** add P1–P8 and H7–H8. About 12 more sessions.
- **Guests:** G1 and G2, 2 sessions.

## 9. Risks and known differences to preserve

- **Extism crate parity:** HTTP egress semantics, kernel memory cap, WASI stdout routing and header flattening
  (decision 5 / F0). Fall back to a vendored fork, as Java did.
- **Sigstore parity:** Java's verifier is JDK-only (Rekor v1, no SCT check). Rust must accept and reject exactly
  the same bundles, so test against Java's fixtures.
- **Byte-level JSON:** the ABI input key order, the desired-state bytes (the ETag depends on them), query parsing
  (`+` as space, repeated keys kept). Use `serde` with `preserve_order` and golden tests.
- **Spec vs code (the code wins):**
  - the `platform://` and `s3://` refs accepted at publish
  - role grants that are wider than the spec
  - `PUBLIC_ROUTE_INVALID` rather than `ROUTE_INVALID`
  - `HOSTNAME_INVALID` rather than `DOMAIN_INVALID`
  - MarkVersionReady takes a row lock
  - response shapes wider than the spec
- **Java is moving** (W3/W4/W5 in flight). Pin `0118cdca` and re-baseline only at a phase boundary. Record every
  re-baseline in this doc.
- **Go has no function runner.** Go doesn't understand `FUNCTION`-sourced subscriptions, so a Go platform can't
  wire functions at all.

## 10. Reading order for an implementer (Java repo)

1. `docs/function-service-overview.md`
2. `docs/function-runner-plan.md` (§4, §4a, §6, §10)
3. `docs/spec/function-invocation.md`
4. `docs/spec/function-api.md` §6
5. `docs/spec/function-host-reconciler.md`
6. `docs/spec/function-host-listener.md`, then `function-public-routes.md` §3 and `function-zones-and-aliases.md` §4
7. `docs/spec/function-wasm-runtime.md` and `docs/plan/wasm-and-js-functions.md`
8. `docs/spec/function-context.md` §2.1 and §3, then `function-wasm-db.md`
9. `docs/spec/function-artifacts.md` and `function-artifact-upload.md`
10. `docs/spec/function-host-process.md`, then `function-registry.md` §2 and §4
