# L9 input: Java-idiom inventory

Status: findings from a read-only review on 2026-09-24. This is the input for lane L9 of
`docs/java-parity-plan.md`. It is not L9's report.
Method: six parallel reviews covered about 150k lines. Each item marked **✔** was checked by hand against
the code. The other items come from the reviews and should be confirmed by whichever lane picks them up.

## Summary

The Java shows in five concentrated patterns, not everywhere. The reviews found **none** of the following:
- `*Impl`, `*Manager`-per-class, `Abstract*` or `I*` types
- `Deref` used as inheritance
- getters and setters on data structs
- `&String`/`&Vec` parameters
- index loops
- `is_some()`→`unwrap()`

The five patterns:

1. **Enums carried as strings.** They are parsed by hand-written `from_str`s that fall back to a default on bad
   input. This affects every domain and the auth claims, and `lib.rs:5` allows `clippy::should_implement_trait`
   for the whole crate to hide it.
2. **Errors turned into strings.** `UseCaseResult` can't use `?`, so about 110 hand-written
   `match … Err(e) => failure(format!(..))` blocks exist, and error details are lost at the HTTP boundary.
3. **Event construction as a Java class hierarchy.** This means:
   - `EventMetadata::new` with 10 positional args, called at 93 sites
   - `DomainEvent` as a trait of 12 getters, like an abstract base class
   - builders that `.expect()` required fields
4. **Null injection and null objects.**
   - `Option<Arc<Repo>>` state fields that are always `Some`
   - `Noop*`/`Disabled*` strategy types next to an `Option`
   - `""` sentinels
5. **One lock per field**, in the router and outbox, much like `synchronized` on each field.

Several of these are **live bugs**, not just style (section A). L9's rule is "without changing behaviour", so
section A is kept apart: it needs a ruling against the Java contract (§2 rule 1) and belongs with the §1.2
contract fixes / L4, not L9.

---

## A. Correctness: changes behaviour, so it needs a contract ruling (not L9)

| # | Finding | Evidence | Fix |
|---|---|---|---|
| A1 ✔ | Enums go out on the wire through `format!("{:?}").to_uppercase()`, which emits `HTTPWEBHOOK`, `NEXTONERROR`, `BLOCKONERROR`, `EXPONENTIALBACKOFF` and `HTTPERROR`. The frontend matches `NEXT_ON_ERROR` (`DispatchJobListPage.vue:168`), so that branch never fires. | `dispatch_job/api.rs:73,79,86,379` (11 sites), `subscription/api.rs:199`, `principal/api.rs`, `client/api.rs:75`, `event_type/api.rs:106`, `auth/config_api.rs:239`, `auth_service.rs:710` (≈25 in total) | Make the DTO field the enum type, which already derives SCREAMING_SNAKE serde, or at least call `as_str()`. Check against the Java value. |
| A2 ✔ | Request input is coerced silently by lenient `from_str`. A mistyped `scope_type` becomes **Client scope** for a login domain mapping. A bad `config_type`/`auth_provider` becomes the default. `?status=paused` filters on ACTIVE. | `email_domain_mapping/operations/create.rs:138`, `auth/operations/create_auth_config.rs:93-99`, `auth/config_entity.rs:22`, `scheduled_job/api.rs:426`, `platform_config/operations/set_property.rs:121` | Type the Command and DTO fields as enums so serde rejects bad input with a 400. Stored rows are strict too, under X-06 (see "Go port comparison" in E); the only lenient enum is dispatch mode (X-01). |
| A3 ✔ | A dispatch-mode fallback contradicts ledger A-09. `_ => DispatchMode::Immediate`, but `fc_common::DispatchMode::default()` is `NextOnError`. Go (ruling X-01) falls back to NEXT_ON_ERROR with a warning. | `dispatch_job/api.rs:597,740`, `shared/sdk_dispatch_jobs_api.rs:66-81` | Match X-01: absent or unknown becomes `NEXT_ON_ERROR` plus `warn!`. This is the only enum that stays lenient. |
| A4 ✔ | Sync use cases skip the UoW for each item. They call `repo.insert/update/delete` in a loop and then only `emit_event`. The sync is not atomic and the individual changes get no audit rows. `uow_convention_test` passes anyway, because the body does reach `unit_of_work.*`. | `event_type/operations/sync.rs:129-218`, `dispatch_pool/operations/sync.rs:150-182`, `process/operations/sync.rs:118-162`, `email_domain_mapping/operations/create.rs:153` | Use `commit_all` with batched persists. Tighten the convention test to ban `self.*_repo.(insert\|update\|delete)` inside `execute`. |
| A5 ✔ | The `UseCaseResult` seal leaks. It is a `pub enum`, so `UseCaseResult::Success(x)` compiles in any module. Only the grep test catches misuse, and nothing misuses it today. | `usecase/result.rs:25` | `pub struct UseCaseResult<T>(Result<T, UseCaseError>)` with a private field. 9 handlers that match on the variants change. This makes the `CLAUDE.md` claim true. **It keeps behaviour, so it can go in L9.** |
| A6 ✔ | `TracingContext` is a `thread_local!` (Java's ThreadLocal/MDC). `run_with_context_async` sets it and then `.await`s, so the context can leak between tokio tasks. | `fc-platform/src/usecase/tracing_context.rs:9,147`, `fc-sdk/src/usecase/tracing_context.rs:8` | Delete it from fc-platform, where only tests use it. In fc-sdk, use `tokio::task_local!`. **This breaks the SDK API.** |
| A7 ✔ | `fc-secrets` returns `format!("encrypted:{}", plaintext)`, which stores the plaintext under an "encrypted" label. No crate depends on it. The live `encrypted:` scheme is fc-platform's `EncryptionService` (see A13). | `fc-secrets/src/service.rs:358` | Delete the crate. |
| A13 ✔ | **Secrets stored in plaintext.** Owner ruling: every stored secret is `encrypted:`-prefixed ciphertext produced by `shared/encryption_service.rs`. Places that don't comply: **(a)** the IDP `oidc_client_secret_ref` is stored exactly as the frontend's "Client Secret" field sends it, and the login read path falls back to the raw value when decryption fails or no key is set. **(b)** Service-account webhook `signing_secret`/`token` are generated and stored unencrypted in `wh_*_ref` columns (nothing reads them yet). **(c)** Platform-config `SECRET` values are plaintext and only masked on output; lowercase `"secret"` silently becomes `PLAIN` and is returned unmasked. **(d)** `decrypt` accepts values with no prefix. | (a) `identity_provider/operations/create.rs:114`, `update.rs:107`, `auth/oidc_login_api.rs:918-927`; (b) `service_account/operations/create.rs:182`, `regenerate_secret.rs:140`, `regenerate_token.rs`, `repository.rs:99-100`; (c) `platform_config/entity.rs:42-46,90`; (d) `shared/encryption_service.rs:123`. The correct sites are `oauth_clients_api.rs:254,613`, `application/api.rs:969`, `service_account/api.rs:401` and `fc-dev/init.rs:313`. | Encrypt on write in those use cases. Make reads refuse the request when decryption fails (as `oauth_api.rs:701,1450` already do). Require the prefix in `decrypt`. Add a one-off Rust backfill that encrypts non-prefixed values; it needs the app key, so it can't be a SQL migration. Remove the dead auth-config `oidc_client_secret_ref` request field and the `secret://` validator stub. |
| A8 | Monitoring endpoints always return defaults. `LeaderState`, `CircuitBreakerRegistry` and `InFlightTracker` are built but never written. `HealthChecker` has 0 impls and `checks` is never filled. | `shared/monitoring_api.rs:140-193`, `platform_routes.rs:961`, `shared/health_api.rs:78,130` | Wire them up or delete the endpoints. |
| A9 | Serialisation failures silently become `{}`. | `fc-sdk/src/outbox/dto.rs:133,238,479`, `usecase/domain_event.rs:358`, `unit_of_work.rs:188` | Return the error. |
| A10 | Token rows decode missing fields to `""`. A corrupt row gives a token with an empty principal. | `auth/authorization_code_repository.rs:47-61`, `refresh_token_repository.rs:98-107` | Use a `#[derive(Deserialize)]` payload struct. Round-trip test against the oidc-provider format. |
| A11 | Router consistency windows. Linked maps sit under separate locks (`add_consumer` inserts into two maps under two locks, and `reconcile` takes three locks in sequence). In the outbox, `skip_blocking_message` reads state, locks the queue, then locks state again. | `fc-router/src/manager/mod.rs:710`, `manager/reconcile.rs:302`, `fc-outbox/src/message_group_processor.rs:226` | See C6. |
| A12 | N+1 queries, which `CLAUDE.md` bans; found in passing. | `event/api.rs:513`, `principal/api.rs:1711,1810,1901`, `application/api.rs:1194`, `fc-outbox/src/repository.rs:232` | Batch with `ANY($1)`. |

---

## B. Tooling first: one session, unblocks everything else

- Remove the crate-wide `#![allow(clippy::should_implement_trait)]` and `#![allow(clippy::too_many_arguments)]`
  (`fc-platform/src/lib.rs:5,10`). Turn each resulting warning into a work item or a local `#[allow]` with a reason.
- Run L9's planned `clippy::pedantic` pass per crate and keep the counts, so the before/after table
  has a baseline.

---

## C. L9 work, behaviour-preserving, in dependency order

Each item is sized to one session. "Mechanical" means an agent can do it from the description. "Judgement" means someone has to own the design.

### C1. Errors: stop turning them into strings (mechanical, ~2 days, internal)
- Add `impl From<PlatformError> for UseCaseError` and a `find_or_not_found` helper. Move loading into an inner
  `async fn load(..) -> Result<_, UseCaseError>` that can use `?`, so `execute` ends in
  `match { Ok(x) => uow.commit(..), Err(e) => failure(e) }`. The seal is unaffected. This removes about 110
  match blocks (≈800 lines in domains A alone). It also fixes read failures that are currently mislabelled
  `COMMIT_FAILED` (`role/operations/delete.rs:64`), and the re-query that ends in
  `Ok(Some(_)) => unreachable!()` (`application/operations/update_client_config.rs:94-115`), which can panic
  in a race.
- `UseCaseError` has 5 identical `{code,message,details}` variants. Replace it with
  `struct { kind, code, message, details }`.
- Typed `thiserror` enums for:
  - `fc-queue` `QueueError`, which has 12 AMQP errors stuffed into `Database(String)`
  - `RouterError::Consumer(String)`
  - JWKS/OIDC (`auth/jwks_cache.rs:68`, `oidc_login_api.rs:902`)
  - `encryption_service.rs`
  - fc-outbox's 8 `Result<_, String>` functions
- Delete the unused `FlowCatalystError` (`fc-common/src/lib.rs:1134`) and the dead `PlatformError` variants.
- *Not in C1*: getting `details` through to the HTTP body changes the error envelope, which is §1.2 work.

### C2. Event construction (mechanical, ~1–2 days)
- `EventMetadata::from_ctx(ctx, TYPE, VERSION, SOURCE, subject, group)`. The template is
  `scheduled_job/operations/events.rs:23` `meta()`. It replaces 93 ten-argument `new` calls.
- `trait DomainEvent: Serialize { fn metadata(&self) -> &EventMetadata; }`. Have `impl_domain_event!` emit it so
  its ~99 call sites stay unchanged. Delete the 3 hand-written 40-line delegations (`service_account/operations/*`).
- Build events with struct literals. Delete the `.expect()` builders (`UserCreatedBuilder`,
  `EventTypeCreated` builder, `EventMetadataBuilder`).
- The UoW should use `serde_json::to_value(&event)?`, not a round trip through a string.
- The same changes are needed in `fc-sdk/src/usecase/` (**breaking SDK change**; see E).

### C3. Enums at rest: the part of section A that doesn't change behaviour (mechanical, ~1 day)
- Replace the inherent `from_str`s with `impl FromStr` (Err on unknown). Row decoding goes through
  `TryFrom<Row>` and surfaces an error that includes the row id (X-06). The only exception is
  `DispatchMode`, which is lenient with a warning (X-01).
- Use `UserScope`/`PrincipalType` in `AccessTokenClaims` and `AuthContext`. With matching serde renames the JWT
  and wire format are unchanged (`auth_service.rs:89-137`, `authorization_service.rs:21-24,62`).
- Add enums for:
  - `AssignmentSource` (`"SDK_SYNC"`/`"ADMIN"`/`"IDP_SYNC"` literals)
  - `Pkce { challenge, method: PkceMethod }`
  - `GrantType` in `oauth_api.rs`
  - `WarningSeverity: FromStr` (copied 3 times in the router)
  - `HealthStatus: Display`
  - the outbox backend choice
- `OutboxStatus`: CamelCase variants with `#[repr(i32)]`. Delete the aliases and `to_code()`
  (`fc-common/src/lib.rs:706-847`).

### C4. Null injection, null objects and two-phase init (mechanical, ~1 day)
- `Option<Arc<Repo>>` becomes `Arc<Repo>`. The affected state structs are `PrincipalsState` (9 fields) and
  `RolesState`, plus 6 fields in `client/api.rs:172`. This also removes the silent skip of the anchor-domain
  check at `principal/api.rs:494`.
- Delete `DisabledStandbyProcessor`, `NoopTrafficStrategy` and `NoOpNotificationService`, and use `Option` instead.
- Construct at build time instead of calling setters afterwards: `set_notification_service`, `set_strict_routing`,
  `set_warning_service`. The setters `set_circuit_breaker_registry` and `set_oidc_stores` start background tasks
  when called, so rename them to `spawn_*`.
- Replace `""` sentinels with `Option`:
  - `auth_api.rs:224`→`365`
  - `webauthn/api.rs:69`
  - `dispatch_job/api.rs:609,744`
  - `sdk_dispatch_jobs_api.rs:83`
  - `scheduler/auth.rs`
  - `AuditLog.entity_id` (check the column is nullable first)

### C5. Long argument lists and wiring (mechanical, ~1 day)
- Replace positional constructors with struct literals or `*Deps` structs:
  - `OAuthState::new` (12 args)
  - `create_router_with_options` (12 args; also called from fc-server and fc-dev)
  - `OidcLoginApiState::new` and `AuthState::new` (8 args each)
  - `login_attempt::find_with_cursor` (8 args, taking a `LoginAttemptFilter`)
- Add a shared `SessionCookieConfig` with a single `build_cookie()`. It is currently duplicated across three
  state structs, and `same_site` is re-parsed on every request.
- Delete constructors with no callers (`QueueManager::with_limits/with_config`, `ProcessPool::new`→`with_dependencies`).
- fc-stream: replace the 4 start/stop service structs with `async fn run(.., cancel: CancellationToken)` plus a
  `TaskTracker` (fc-stream's API changes, and fc-dev/fc-server need updating).

### C6. Concurrency structure (judgement, ~3 days, fixes A11)
- Router: put one `RwLock<ConsumerRegistry { by_key, by_id, configs }>` around the linked maps. Replace the 3
  lockstep maps in `health.rs:124-136` with one `HashMap<String, ConsumerHealth>`. Hold `ProcessPool`'s 11 `Arc`
  fields in a single `Arc<PoolShared>`.
- Then split `QueueManager` (≈37 fields, ≈90 methods) into `DedupTable`, `ConsumerRegistry` and `PoolRegistry`,
  each owning its own invariants.
- Outbox: one `Mutex<Inner { queue, state }>` per group processor. Drop the `Mutex<VecDeque>` that sits next to
  the mpsc. Use `AtomicU64` for stats.
- Guard: run the fc-router conformance suite and `manager_tests.rs` before and after.

### C7. Deleting dead Java-port code (trivial, do it any time)
- Crates and types:
  - The **fc-secrets** crate (A7).
  - Replace **fc-config** with `SchedulerConfig::from_env()`. Only its scheduler section is read, but it applies
    49 hand-written env overrides.
  - `fc-common` `StandbyConfig`.
- Traits with no callers:
  - `IdpAdapter`
  - `MessageDispatcher`
  - `OutboxRepositoryExt`
  - `HasId::collection_name` (a Mongo leftover)
- `OidcService.providers` and `register_provider`. They are never called, so `?provider=` always errors.
- The duplicate `PasswordResetToken` (`password_service.rs:334`).
- About 50 "matches Java X" / "Java equivalent:" doc comments, and the `# Arguments`/`# Returns` sections.
- `TsidGenerator` is a unit struct with only associated fns, used at 193 call sites. Make it free fns in
  `tsid::` and keep a deprecated re-export for the SDK.

### C8. Low value; do opportunistically or skip
- Rename about 45 `get_x()` methods (router `warning.rs`, `QueueConsumer::get_metrics`).
  `get_or_create` stays.
- Break the `main()` functions in the bins into `build_*`/`spawn_*` functions. `fc-dev` is 710 lines and
  `fc-server` is 450.
- Replace `PlatformRoutes`'s ≈45 fields with `AppState { repos, uow }`. This is large churn for little gain.
- `*_at: String` becoming `DateTime<Utc>` changes the wire format (`+00:00`→`Z`), so it is §1.2 work, not L9.

---

## D. Justified; the reviews recommend leaving these alone
- Traits with several real impls or with mocks:
  - `UnitOfWork` (3 impls, used through generics)
  - `Mediator` (10+ mocks)
  - `ConsumerFactory`
  - `QueueConsumer`/`QueuePublisher`
  - `RateLimitStore`
  - `EmailService`
  - `NotificationService`
  - `OutboxRepository`
  - `Provider`
  - `Cache`, `LockProvider` and `SessionStore` (SDK extension points)
- `Option<Vec<T>>` in `Update*Command`: PATCH semantics.
- SDK builders on large request DTOs, `QueueManagerBuilder`, and `with_*(mut self)` chaining on entities.
- `OnceLock` statics and the `AtomicBool` edge flags in the router.
- `OutboxManager`: the name matches the TS and Laravel SDKs.

## E. Cross-cutting constraints

**Owner ruling (2026-09-24): the Rust SDK (`fc-sdk`) may break. The TS, Laravel and Go SDKs must not.**

- **Breaking fc-sdk changes are allowed.** Ship them in one release:
  - TracingContext (A6)
  - the `DomainEvent` trait (C2)
  - the `OutboxStatus` consts and the `OutboxMessage` field types (C3)
  - `anyhow` return types in `OutboxManager`
  - `TsidGenerator` (C7)
  - the `UseCaseError::*Error` variant names

  These are Rust-only API shapes, so none of them change anything on the wire for the other SDKs.
- **Nothing may change the wire format that TS, Laravel or Go depend on.** In particular, JWT
  `scope`/`principal_type` must stay uppercase (`typescript-sdk/src/fastify/oidc/claims.ts:31` lowercases
  the incoming `"ANCHOR"`). Serde renames in C3 must keep the values byte-identical.
- **Strict enum casing for A2 has been checked and is safe.** 13 server sites accept any case today:
  - connection `status`
  - email-domain `scopeType` (create and update)
  - OAuth `clientType`
  - principal `scope` / `type` / `idpType`
  - role `source` (×3)
  - dispatch-pool and dispatch-job `status` filters
  - subscription `mode`

  No client sends anything but exact uppercase. TS uses uppercase unions, Laravel uses uppercase string-backed
  enums, Go has no lowercase constants, and the frontend uses uppercase unions. Nothing in any SDK lowercases
  an outbound value. So all 13 can require exact serde-derived `SCREAMING_SNAKE_CASE`.

  Only TS callers who pass lowercase through the list filters typed as plain `string`
  (`resources/{dispatch-pools,subscriptions,event-types,processes}.ts`) will see a change: today their value
  either matches or is silently ignored, and afterwards it gets a 400. That is caller input, not SDK code.
  Optionally, narrow those TS types to uppercase unions, which doesn't break anything.
- Delete the `"BLOCKONERROR"` alias in `subscription/api.rs:257` in the same change as A1. It exists only
  so the API accepts its own Debug-form output.
- Still open: whether the Java contract (§2 rule 1) is case-insensitive on these routes. Check
  `../flowcatalyst-javalin` before landing A2, or the parity harness will record a difference.

### Go port comparison (`../flowcatalyst-go`, read-only, 2026-09-24)

Go already implements the owner rulings, so it is the reference for A1–A3.
- **X-06, strict enum reads everywhere** (`flowcatalyst-go/docs/owner-rulings-todo.md:33`): an unknown value is a
  400 at the write boundary and a **loud read error** for stored rows, never a silent default. The ruling
  names one case: an unknown service-account auth type must reject, never become `NONE`. Go uses a
  `ParseX(s) (T, bool)` exact-match helper per enum, and callers reject on `!ok`. **This replaces this doc's
  earlier idea of keeping lenient parsing for DB rows. Stored reads are strict too.**
- **X-01, the one exemption**: dispatch mode. Absent or unknown becomes `NEXT_ON_ERROR`, with a `slog.Warn`
  (`internal/common/message.go:44-69`).
- Rust vs Go, per site:

  | Field | Rust today | Go | Action |
  |---|---|---|---|
  | email-domain `scopeType` (create) | silently CLIENT | 400 | strict |
  | email-domain `scopeType` (update) | silently CLIENT | not updatable | strict (or drop the field; ruling needed) |
  | OAuth `clientType` | silently **PUBLIC** | 400 (a comment calls defaulting a security regression) | strict |
  | dispatch-job create `mode` | silently **IMMEDIATE** | NEXT_ON_ERROR + warn (X-01) | match X-01 |
  | subscription `mode` | rejects `NEXT_ON_ERROR`, accepts `BLOCKONERROR` | X-01 | match X-01, drop the alias |
  | dispatch-pool / role `source` filters | filter ignored / case-insensitive | exact; empty list | strict |
  | dispatch-job `status` filter | 400 | empty list | keep Rust's 400 (stricter is fine) |
  | principal `idpType` | case-insensitive `OIDC` | exact | strict |
  | **stored `WebhookAuthType`** | unknown → `None` (`service_account/entity.rs:43`, read at `repository.rs:376`, and a test asserts it at `entity.rs:491`) | reject (named in X-06) | **strict; fix first** |
  | stored `ConfigValueType` | unknown → `Plain` (a secret shown unmasked, A13c) | — | strict |

- **Wire output values for A1**, from Go:
  - protocol `HTTP_WEBHOOK`
  - mode `IMMEDIATE` / `NEXT_ON_ERROR` / `BLOCK_ON_ERROR`
  - errorType `CONNECTION` / `TIMEOUT` / `HTTP_ERROR` / `VALIDATION` / `UNKNOWN`
  - **retryStrategy lowercase** `immediate` / `fixed` / `exponential`

  Rust spells the retry strategy three different ways today: serde derives `EXPONENTIAL_BACKOFF`
  (`dispatch_job/entity.rs:58`), `as_str()` gives `exponential`, and the API emits Debug `EXPONENTIALBACKOFF`.
  Settle on Go's lowercase for the API and the serde derive.
- fc-platform and fc-sdk each have their own `usecase/` module. Fix C2 and A5/A6 in both, or first
  decide whether fc-platform should depend on the SDK's copy.

## Suggested order
B → A5 and C7 (small, and they unblock later items) → C1 → C2 → C3 → C4 → C5 → C6 → C8.
Section A runs as its own track against the Java contract: A1–A4 first, because A2 is security-relevant
(login scope assignment) and A4 breaks the audit invariant.
