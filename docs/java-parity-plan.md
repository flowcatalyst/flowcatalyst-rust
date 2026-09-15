# Plan: bring flowcatalyst-rust to parity with flowcatalyst-javalin, verified by the Java repo's assets

Status: proposal for execution by Sonnet 5 implementation agents, orchestrated by a supervising agent.
Reference: `../flowcatalyst-javalin` at the commit recorded in §1.3 (freeze it; do not chase a moving target).
Sources: three code-only comparisons run 2026-09-15 (API surface, schema and engines, verification assets).

## 1. Goal and gate

**Goal.** The Rust platform passes the Java repo's verification assets as a second side, so the two can be
compared on equal footing and a language decision made on numbers:

1. **Parity harness** (`javalin/parity/`): Rust-vs-Java two-sided run over the 43 scenarios and the 104-route
   surface, with any remaining differences recorded in an allow-list with a ruling, and the count trending to zero.
2. **Conformance** (`javalin/conformance/mediation-outcomes.json`, 29 cases): all fields asserted, including
   `disposition`, and the test running in Rust CI.
3. **E2E** (`javalin/e2e/`, 10 Playwright specs): green against Rust `fc-dev`, with the SPA gate replaced by a
   route allow-list (the Rust frontend is a fork, see §4.5).
4. **Bench** (`javalin/bench/real` and `bench/router`): four-way table Java jar, Java native, Go, Rust on the
   pinned-core protocol from `../test-size/RESULTS.md` round 9.
5. **CI**: the Rust repo's CI runs the equivalents of the Java jobs (test with services, drift check, image boot,
   parity, e2e, audit) and pins its toolchain.

**Decision gate** (§7) runs when 1–5 are met. Nothing in this plan changes Java behaviour; Java is the contract.

### 1.1 Gap in numbers

| dimension | Java (reference) | Rust today | gap |
|---|---:|---:|---|
| OpenAPI operations | 254 (187 paths) | 347 platform + 68 router-operator | 199 common; **55 Java-only**; 148 Rust-only (mostly `/bff` mirrors, `/api/monitoring`, router operator API) |
| parity `surface.json` routes | 104 | 75 present | **29 missing** (16 are 2FA, 5 portal, 4 password/session, 4 spec/misc) |
| parity scenarios that can run | 43 | ~0 | `POST /api/principals` is missing and is a fixture step in ~30 scenarios |
| tables | 69 | 57 | **12 missing** (MFA ×4, portal ×4, `mail_outbox`, `app_docs`, `iam_reset_approval_requests`, `oauth_identity_provider_allowed_roles`) + `iam_login_attempts` unpartitioned |
| conformance fields asserted | 8 | 7 | `disposition` skipped; 1xx cases skipped; test silently skips in CI (no sibling checkout) |
| env vars Java defines that Rust never reads | — | 52 | incl. `FC_JWT_SIGNING_KEY_PATH`, `FC_AUTH_ALLOW_TEST_HEADERS`, `FC_SCHEDULED_JOB_ENABLED`, `FC_MCP_ENABLED`, `FC_DRAIN_TIMEOUT_SECONDS` |
| CI | 6 jobs incl. parity, e2e, image boot, native matrix | clippy, fmt, unit tests, frontend | no services, no parity, no e2e, no boot test, no audit, no toolchain pin |

### 1.2 Contract differences on shared routes (must be fixed before the harness is useful)

- Java emits `$schema` on every JSON body; Rust emits none. Error envelope Java `{$schema,error,message,details}` vs Rust `{error,message}`.
- Status codes: Java returns **204** on dispatch-pool `activate/suspend/archive` and subscription `pause/resume`; Rust returns 200 with a body. `POST /api/principals/{id}/roles` returns `PrincipalResponse` in Java, `{roles}` in Rust. `POST /api/scheduled-jobs/{id}/fire` returns `{id,instanceId,scheduledJobId}` in Java, `{id}` in Rust.
- Path params: `/api/roles/{id}` vs `{roleName}`; `/api/config/{app}` vs `{appCode}`; `/bff/developer/.../{app_id}` snake_case in Rust; `lookup?domain=` vs `lookup/{domain}`.
- Pagination: Java per-endpoint `page` (1-based), `pageSize`, `size`, `limit`, `offset`, `sortField`, `sortOrder`; Rust central `PaginationParams{page,size}` **0-based**, no sort, no offset.
- Dropped fields: applications `logo/logoMimeType/website`; event types `clientId/source/createdBy/updatedBy`; subscriptions `createdBy`; scheduled jobs `applicationId`; principals `hasDeveloperCredential/developerCredentialUpdatedAt/inviteLink/twoFactorMethods`; clients `notes`.
- HMAC header casing `X-FlowCatalyst-*` (Java) vs `X-FLOWCATALYST-*` (Rust): harmless on the wire, but the Rust comment claiming parity is wrong; align for byte-identical logs.

### 1.3 Reference freeze

Reference commit (recorded 2026-09-15): `646794d3` (`git -C ../flowcatalyst-javalin rev-parse HEAD`); all lanes verify against that
checkout. Re-baseline only at a phase boundary, by the supervisor.

## 2. Ground rules for every agent

1. Java is the contract. When Rust and Java disagree, Rust changes, unless the Java behaviour is a bug the
   corpus or a scenario already rules against (the corpus's `divergence.correct` field is the only such authority).
2. Spec first for API work: the Java `sdk/openapi/openapi.json` operation is the definition; implement the
   Rust handler, `#[utoipa::path]` and serde structs to match it field-for-field, then run the parity subset.
3. One lane, one worktree, one branch, one PR. No cross-lane edits. The supervisor merges in the order in §5.
4. Definition of done for any lane: `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`,
   `cargo test --workspace` (with services where needed), the lane's parity/conformance/e2e subset green, and a
   report file `docs/parity/<lane>.md` listing what changed, what was deferred and why, and the numbers.
5. No new dependencies without a line in the report saying why; nothing that adds `unsafe`.
6. Never modify the Java repo except in lane L1 (harness generalisation), and there only the files named.
7. Do not touch generated SDK output or the frontend except where a lane says so.

## 3. Lanes

Sizes are rough LOC of Rust to add or change. Dependencies are in §5.

### L0 — Foundation (blocks everything)
- Pin toolchain (`rust-toolchain.toml`), add `deny.toml` + `cargo-deny` and `cargo-audit` CI jobs, `cargo nextest` with JUnit upload.
- CI `services: postgres` (and LocalStack/NATS where the testcontainers suites need them) so integration tests run; conformance job checks out the Java repo read-only and sets `FC_CONFORMANCE_CORPUS`.
- Decide sqlx offline mode (`.sqlx` committed, `cargo sqlx prepare --check` in CI) — do it; compile-time query checks are the point.
- Env aliases so the Java harness can drive Rust: read `FC_JWT_SIGNING_KEY_PATH` (single PKCS#8; derive the public key), `FC_SCHEDULED_JOB_ENABLED`, `FC_MCP_ENABLED`, `FC_DRAIN_TIMEOUT_SECONDS`, `FC_DEFAULT_BROKER`; honour `FC_METRICS_PORT=0` as ephemeral; `fc-router` must honour `FC_METRICS_PORT`.
- Test-principal header: `FC_AUTH_ALLOW_TEST_HEADERS` + `X-FC-Test-Principal` in the auth layer, off by default, on in `fc-dev start` (Java `Env.java:332,514`, `StartCommand.java:233`).
- Response conventions (§1.2): `$schema` on every body, Java error envelope, 204s, path-param names, 1-based pagination with `offset/sortField/sortOrder`, `FireNowResponse`, `PrincipalResponse` on role grant.
- `POST /api/principals` (Java `createPrincipal`) — the fixture step that unblocks ~30 scenarios.
Size ≈ 2–3k. One agent, sequential.

### L1 — Harness and local-dev parity
- Java repo, `parity/` only: generalise `GoSide` into `SubprocessSide`; add `RustBinaries` (`PARITY_RUST_SRC`/`PARITY_RUST_BIN_DIR`, cargo build with a long timeout); allow a Rust-vs-Java run reusing the two-sided `Diff`; a third column is a later nicety.
- Java repo, `e2e/runner/side.ts`: add a `rust` launcher; per-side `startArgs()`; replace the SPA sha gate with a route allow-list when `E2E_SIDE=rust`.
- Rust `fc-dev`: add `--embedded-db-path`, `--embedded-db-reset`, `--pid-file`, `stop`, `version`, `completion`, `db upgrade` (or a documented no-op); `init --no-oauth-client` (or the harness ignores those rows); dev defaults `FC_AUTH_ALLOW_TEST_HEADERS=true`, `FC_DEFAULT_BROKER=postgres`.
- Mail-from-log: emit the `"SMTP not configured"` marker with flat `to/subject/body` fields so `mail.ts` parses it.
- Spec diff step: CI boots `fc-server`, dumps `/q/openapi`, diffs against the Java `openapi.json` with an allow-list file mirroring `parity/expected-diffs.json`; the count is reported and must not grow.
Size ≈ 1.5–2k Rust + ~800 Java/TS. One agent.

### L2 — Schema alignment
- Add the 12 tables with Java's columns and indexes (`javalin/server/src/main/resources/db/migration/V1__baseline.sql`, V5, V8, V9); partition `iam_login_attempts` by `attempted_at`; make `023` a real file or document pg_partman as out-of-band; add `scheduledJobRetentionDays` split to the partition manager config.
- Strict wire parser for retry strategy (invalid values are 400s, not silent defaults).
Size ≈ 1–1.5k. One agent. Blocks L3 and L4.

### L3 — Auth subsystem parity (largest single gap)
- MFA: TOTP enrol/confirm/verify, email PIN, recovery codes, trusted devices + cookie, login gate, domain policy; the 16 `/auth/2fa/*` routes; `iam_*_mfa_*` and `tnt_email_domain_mapping_2fa_methods`.
- Password/session: `change-password` (+ email code), `password-setup/request`, `login-history`; reset approvals (`iam_reset_approval_requests`, 3 routes).
- Portal: identities, login flows, apps, identity-apps; `/portal/*` routes (5) and `/api/portal-users`, `/api/portal-apps` (12 ops).
- Close the widenings Java rejected: remove the HS256 signing fallback and the `password` grant; RS256 only, discovery advertises only RS256; dummy-verify on unknown user for timing equalisation.
- Principals: `reset-2fa`, developer credential set/revoke, `developer-users`, `twoFactorMethods` fields.
Verified by scenarios `auth/mfa.json`, `auth/session.json`, `auth-remainder/*`, `portal/*`, `portal-users/*`, `portal-apps/*`, `portal-assign/*`, `reset-approvals/*`, and e2e `2fa.spec.ts`, `passkeys.spec.ts`.
Size ≈ 8–12k. Two agents in sequence (MFA+password first, portal second) or one agent per half in parallel after L2.

### L4 — Platform API gaps (the rest of the 55 ops and the dropped fields)
Split by aggregate for parallel agents:
- **L4a principals/roles/clients**: bulk-import, sync, client-association, version; roles permission routes (3); `clients/search`; DTO fields.
- **L4b dispatch/events/event-types**: dispatch-jobs `by event`, `list-raw`, `requeue`, `cancel`, `complete`; `events/list-raw`; `event-types/{id}/schemas` under `/api`; `PUT /bff/event-types/{id}`; `GET /api/dispatch/router-config`.
- **L4c config/service-accounts/oauth-clients/email-domain-mappings/applications**: platform-config `{app}` and access tree under Java's paths (keep `/api/config-access` as alias or remove); service-account deactivate/regenerate/token mint (camelCase `ServiceAccountTokenResponse`); `revoke-previous-secret`; email-domain-mapping `by-domain`, `lookup` (query), `move-provider`; applications `clients/{clientId}` GET and `service-account` POST verbs; `logo/logoMimeType/website`.
- **L4d docs and sdksync**: `app_docs`, `/api/docs/*` (3), `docs/sync`, `processes/sync` by body; `SyncDocsRequest`; `/api/openapi.json` and `.yaml` aliases of `/q/openapi`.
Each verified by the matching scenario files listed in the API comparison. Size ≈ 5–7k total.

### L5 — Engine behaviour parity
- Router: settled reporting client (`POST /api/dispatch/settled`, chunked, 5 s timeout) and platform endpoint with the `status IN ('QUEUED','PROCESSING')` guard; `BLOCK_ON_ERROR` stranded-sibling recovery; collapse the two retry layers into Java's single `RetryPolicy` (burst 1 s/2 s, `B(n)=clamp(min<<min(n,12), floor, max)`, DELIVERY 100 ms→5 min, DEFERRED 5 s→1 min, per-burst breaker accounting); model `disposition` on `MediationResult` (`ErrorConfig` split into `UNDELIVERABLE`/`REJECTED`, 500/505 → `REJECTED`); 1xx → `ErrorConnection`, status 0, delay 30; assert every corpus field.
- Scheduler: `next_attempt_at` predicate in `find_pending_jobs` (backoff hold-back by earliest holder per group); `DispatchJobReaper` (2 min cadence, PROCESSING live-after 45 min, sweep stranded siblings, not leader-gated).
- Stream: `FOR UPDATE SKIP LOCKED` in both projections.
- Outbox: defaults to Java's (batch 100, maxInFlight 1000).
- SDK sync: `docs` and processes-by-body (shared with L4d; L5 owns the engine side only).
Verified by conformance (all 29 cases, all fields), router integration tests, `router-config` and `dispatch-jobs` scenarios. Size ≈ 3–4k. One agent, can run parallel to L3/L4.

### L6 — Operational parity
- Health triad `/health/live|ready|startup` on platform and router; `/health` `checks[]` shape.
- Metrics: add `fc_circuit_breaker_*`, queue depth, admission (`fc_request_*`), mail, `fc_db_gate_*`; resolve the `fc_router_mediation_http_version` vs `fc_mediation_http_version_total` collision by adopting Java's name; commit a `metrics-surface.json` and diff it in CI.
- Shutdown: explicit order (listeners → router drain → loops), honour `FC_DRAIN_TIMEOUT_SECONDS`.
- Env: commit `env-surface.json` from Java's `Env.java`; Rust reads every name in it (alias the renamed TTL/scheduler vars); CI diffs.
- `tracing` spans named after Java's 10 JFR events so bench traces line up.
Size ≈ 1.5–2k. One agent, parallel.

### L7 — Bench
- `javalin/bench/real/Dockerfile.rust` building `fc-server`, seeded via `fc-dev init`; run the platform leg four-way; rerun `bench/router` with `Dockerfile.rust`.
- Pinned protocol (cpuset, Postgres pinned away, 30 s warm-up, 60 s run) per `../test-size/bench/run-onecore.sh`; record RSS, p99, req/s, context switches; add incremental-build-time measurement (`touch` one file in fc-platform, time `cargo build`).
Size small; one agent after L4/L5.

### L8 — Spec and SDKs
- Make spec generation deterministic: commit `openapi.json`, CI fails on drift instead of auto-committing.
- When the Java-only count from the L1 diff reaches zero, pin the SDK generators to Java's `openapi.json` and regenerate TS/Laravel/Go once; delete the Rust-only `/bff` duplicates that exist only for fc-dev, or mark them `x-internal` and exclude from the spec.
One agent after L4.

### L9 — Idiomatic Rust pass (owner request)
Remove Rust that reads like Java and replace it with idiomatic Rust, **without changing behaviour** (the parity
harness and conformance suite are the safety net; run them before and after). Targets, in priority order:
- Class-shaped code: `Foo::new()` + setter methods where a struct literal or a builder-by-value fits; `*Manager`/
  `*Service`/`*Repository` layers that only forward calls; `impl` blocks that exist to mimic a class.
- Dynamic dispatch by habit: `Box<dyn Trait>`/`Arc<dyn Trait>` and `#[async_trait]` where there is one
  implementation or a closed set (use generics or an enum); trait objects for testability where a generic bound
  or a small trait with a test impl does the same.
- Shared mutable state by habit: `Arc<Mutex<T>>`/`RwLock` where ownership or channels work; `.clone()` to
  satisfy the borrow checker where a borrow or `Cow` is correct.
- Stringly typing: `String` status/kind fields where an enum with `serde` renames belongs; `Result<T, String>`;
  `Option<Box<T>>` used as a nullable; error enums with catch-all `Other(String)` variants used for control flow.
- Nulls and exceptions: `unwrap()`/`expect()` in non-test code paths; `Option` where the absence is impossible by
  construction; panics used as errors.
- API shape: `&String`/`&Vec<T>` parameters (take `&str`/`&[T]`); returning `Vec` where an iterator is natural;
  getters/setters on plain data (make fields `pub` or `pub(crate)`); `Into`/`From` conversions instead of
  `to_xxx()` methods; `impl Trait` in argument position; `?` instead of match-and-return.
- Async shape: spawning a task per unit of work where a stream/`join_set` fits; `tokio::sync::Mutex` held across
  `.await` where a `std` mutex or a redesign is right.
Method: one crate per sub-lane (fc-platform split by aggregate), `cargo clippy` with `pedantic` enabled for the
crate as a discovery tool (fix what is genuinely un-idiomatic, allow what is noise, record which), before/after
parity and conformance numbers in the report, and a short "patterns replaced" table. Size: unknown until the
pedantic pass; expect the largest touch count in fc-platform. Runs in phase 5 after L4 merges, or interleaved
per aggregate where a lane already rewrites a file (then the lane does it in place).

### Final deliverable — language review
After the decision-gate inputs are collected, the supervisor writes `docs/parity/rust-language-review.md`: an
assessment of whether Rust was a good language for *this* work, grounded in the lanes' reports: where the type
system caught real defects, where the borrow checker or async model cost time, how model-written Rust held up
under review, the incremental loop numbers, dependency and unsafe counts, and the places where the code still
wanted to be Java. It is an input to the decision, written before it.

## 4. Things this plan deliberately does not do

1. **Frontend unification.** The Rust SPA is a fork (72 routes vs Java's 59, seven Java-only routes including portal and docs). E2E runs on a route allow-list; the Java-only specs are skipped on the Rust side until the portal work (L3) lands the routes. Merging the two SPAs is a separate decision.
2. **Router operator API (68 Rust-only ops)** and `/api/monitoring/*` are kept; they are not in Java's contract and are not diffed.
3. **Outbox extra backends** (MySQL/SQLite/Mongo in Rust) are kept.
4. **Java changes** beyond L1's harness files.
5. **Function runner.** Out of scope here; its Rust design (wasmtime host) is written after the decision gate.

## 5. Orchestration

Supervisor: the orchestrating model (this session's). Implementers: Sonnet 5 agents, one per lane or sub-lane,
each in its own git worktree on a `parity/<lane>` branch. Reviewers: a Sonnet 5 agent per merged lane that runs
the lane's definition of done independently and reads the diff for contract drift; disagreements go to the
supervisor.

```
phase 1   L0 ──────────────────────────────► review ─► merge
phase 2   L1 ─┐  L2 ─┐                       (parallel, 2 worktrees)
              └──────┴─► reviews ─► merges
phase 3   L3 (MFA+password) ─┐  L5 ─┐  L6 ─┐ (parallel, 3 worktrees)
          L3 (portal) after L3a │      │     │
              └─────────────────┴──────┴─────┴─► reviews ─► merges ─► parity run #1 (report the DIFF count)
phase 4   L4a  L4b  L4c  L4d                 (parallel, 4 worktrees)
              └─► reviews ─► merges ─► parity run #2, e2e run #1
phase 5   L7 ─┐  L8 ─┐  L9 (per crate) ─┐    (parallel)
              └──────┴──────────────────┴─► final parity, conformance, e2e, bench ─► language review ─► decision gate (§7)
```

Per-agent brief (the supervisor fills the placeholders from this document):
```
You are implementing lane <L> of docs/java-parity-plan.md in worktree <path>, branch parity/<lane>.
Reference checkout: ../flowcatalyst-javalin @ <commit>. Java is the contract (plan §2).
Scope: exactly the bullets under §3 <L>. Inputs: <Java files>, <Rust files>, <scenario/corpus files>.
Definition of done: plan §2 rule 4, plus: <lane-specific checks>.
Report: docs/parity/<lane>.md with: changed files, deferred items with reasons, test/parity/conformance
numbers before and after, any place the plan was wrong about the code.
Do not: edit outside scope; add deps without justification; touch the Java repo (except L1's named files).
```

Supervisor duties per phase: cut worktrees; issue briefs; on completion run the reviewer; merge in order;
re-run the parity harness and record the DIFF count and the surface coverage in `docs/parity/progress.md`;
re-baseline the reference commit only at a phase boundary. The Workflow tool runs phases 2–5 as parallel
`agent()` calls with a verify step per lane; phase 1 is a single agent.

Effort, rough: ~14 implementation agents and ~8 reviewer runs; the lanes total ≈ 25–35k lines of Rust plus
harness changes. Wall-clock is dominated by L3 and the parity iteration loop, not by agent count.

## 6. Risks and how each is handled

- **Java surface moves during the work.** Frozen reference commit; re-baseline per phase only.
- **Harness assumptions baked for Go/Java** (single JWT key path, init rows, test headers). Cleared in L0/L1 before any behavioural lane starts, so failures after that are real differences.
- **`disposition` vocabulary mismatch** (corpus has five values, Rust's type has fewer). L5 models it on the outcome exactly as Java's `MediationOutcome` does; no skipping.
- **Frontend fork.** Allow-list in e2e; explicitly out of scope.
- **Compile-time loop.** L0 adds a measured incremental-build time to CI output; L7 reports it; it is a decision-gate input.
- **Scope creep from the 148 Rust-only ops.** Not diffed, not removed except the fc-dev-only `/bff` duplicates in L8.

## 7. Decision gate — what gets compared

| input | source |
|---|---|
| Parity: DIFF count, ACCEPTED count with rulings, surface coverage | `parity/report.md` for Rust-vs-Java and Go-vs-Java on the same reference commit |
| Conformance: 29/29 with all fields | Rust CI job |
| E2E: specs passing on Rust side vs Java side; skipped list | e2e artifacts |
| Bench: req/s, p99, max, RSS on one pinned core; router drain bench; platform leg | L7 tables |
| Loop: incremental build + test time for a one-file change in fc-platform, vs `mvn -q test` on the Java repo | L7 |
| Supply chain: `cargo deny` result, crate count, unsafe count, SBOM | L0 |
| Size: hand-written LOC, test LOC, test ratio | `tokei` / `wc` |
| Ops: metrics-surface and env-surface diffs remaining | L6 |

The gate is a written comparison in `docs/parity/decision.md` with those rows filled for both ports, followed by
the language decision. The function-runner design for the chosen language is written after it.
