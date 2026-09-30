# Quarkus SDK — Plan

## Status

Plan / not yet implemented. This document is the design brief for adding a
fifth wire-compatible SDK alongside the existing Rust, TypeScript, Laravel,
and Go SDKs.

## Purpose

A FlowCatalyst SDK for consumer apps built on **Quarkus (Java)** — for teams
who want imperative business code with type-safe domain modelling, single
native binaries via GraalVM, and the Java ecosystem's depth of libraries.

Target user: a backend engineer building a service that subscribes to
FlowCatalyst events, calls FlowCatalyst APIs, and persists state — using
Quarkus's CDI-driven architecture and virtual threads for blocking-style
code.

## Constraints — wire compatibility

The SDK must be byte-compatible with the existing four SDKs. The Rust
implementation (`crates/fc-sdk`) is the canonical spec. Any value minted by
this SDK — TSID, JWT, webhook signature, CloudEvent envelope, outbox row
— must be indistinguishable from one minted by the Rust SDK.

Specifically:

- **TSID format**: 13-character Crockford Base32, layout `42-bit ms |
  10-bit random | 12-bit counter`. See `crates/fc-common-types/src/tsid.rs`.
- **CloudEvent envelope**: `{ eventId, specVersion, source, type, subject,
  time, correlationId, causationId, principalId, executionId,
  messageGroup, data: { ... } }`. See `crates/fc-common/src/cloud_event.rs`.
- **Webhook signature**: HMAC-SHA256 over `timestamp || body`, hex-encoded,
  Unix-second timestamp. Headers: `X-FlowCatalyst-Signature`,
  `X-FlowCatalyst-Timestamp`. Tolerance 300s, future-grace 60s.
- **Outbox row shape**: see `crates/fc-outbox/src/repository.rs` for
  column names and types.
- **JWT claims model**: `clients[]`, `roles[]`, `applications[]`,
  `permissions[]`. See `crates/fc-sdk/src/auth/claims.rs`.

Cross-SDK parity is verified by shared golden test vectors under
`crates/fc-router/tests/golden/` and the SDK-specific test suites that
consume them.

## Locked decisions

The following are decided. Do not re-litigate in implementation.

| Decision           | Choice                                                       |
|--------------------|--------------------------------------------------------------|
| Language           | Java 21 LTS minimum; Java 25 LTS where available             |
| Framework          | Quarkus 3.x (latest stable at implementation time)           |
| Programming style  | Imperative, always on virtual threads (`@RunOnVirtualThread`) |
| Reactive support   | Opt-in via Mutiny wrappers (Phase 4)                         |
| Distribution shape | Single umbrella extension: `io.flowcatalyst:flowcatalyst-quarkus` |
| Build tool         | Maven (for the SDK itself); consumers may use either         |
| ORM                | Hibernate ORM (not Reactive)                                 |
| HTTP client        | Hand-written using Quarkus's vert.x http-client (blocking facade) |
| JSON               | Jackson (Quarkus default)                                    |
| DB drivers         | Postgres primary; MySQL Phase 4                              |
| Native image       | First-class from day 1                                       |
| License            | Same as other FlowCatalyst SDKs                              |

## Reference SDKs to copy from

Read all four before starting. They're equivalent at the contract level;
they differ in idiom.

- **Rust**: `crates/fc-sdk/` — canonical spec.
- **Go**: `../flowcatalyst-go/pkg/fcsdk/` — most recent, easiest to read,
  same imperative style this SDK will adopt. Especially:
  - `pkg/fcsdk/doc.go` — package-map template
  - `pkg/fcsdk/usecase/` — UseCase + Result + sealed-token pattern
  - `pkg/fcsdk/usecasepgx/` — UoW driver
  - `pkg/fcsdk/examples/` — five runnable examples
- **TypeScript**: `clients/typescript-sdk/` — wire format and DTO shapes.
- **Laravel**: `clients/laravel-sdk/` — PHP idiom; useful for "how do
  framework-natural integrations look."

## Package structure

Mirror the Go SDK's package layout. Java package = Go package, one-to-one:

```
io.flowcatalyst.sdk/
├── usecase/        ← UseCase, Result, DomainEvent, ExecutionContext, Error
├── outbox/         ← Sink interface + Hibernate/JDBC implementations
├── client/         ← FlowCatalystClient + typed accessors per aggregate
├── auth/           ← JWT validation, OAuth client_credentials, claims
├── webhook/        ← HMAC validator (JAX-RS filter + standalone)
├── cache/          ← Cache interface, MemoryCache; redis/postgres sub-packages
├── lock/           ← Lock interface, MemoryLock; redis/postgres sub-packages
├── sync/           ← DefinitionSet builder + Synchronizer
├── scheduledjobs/  ← JobHandler interface + Runner JAX-RS endpoint
└── tsid/           ← TSID generation, EntityType enum
```

Internal-only packages live under `io.flowcatalyst.sdk.internal.*` and use
`module-info.java` (JPMS) to prevent external import — the equivalent of
Go's `internal/sealed`.

## The sealed Result pattern in Java

Java 17+ has sealed interfaces — this maps better than it does in Go.

```java
package io.flowcatalyst.sdk.usecase;

public sealed interface Result<E> permits Success, Failure { }

public final class Success<E> implements Result<E> {
    private final E event;
    // Package-private constructor — only the SDK module can construct.
    Success(E event) { this.event = event; }
    public E event() { return event; }
}

public final class Failure<E> implements Result<E> {
    private final UseCaseError error;
    public Failure(UseCaseError error) { this.error = error; }
    public UseCaseError error() { return error; }
}
```

External code can construct `Failure` (validation errors, business rule
violations, etc.) but cannot construct `Success`. The only path to a
`Success` is through `UnitOfWork.commit(...)`, which is in the SDK module
and can reach the package-private constructor.

Compiler enforces this via `permits` + module visibility. Cleaner than
Go's `internal/sealed` token because Java has language-level support for
exactly this pattern.

Consumer code handles the result with pattern matching (Java 21+):

```java
switch (uc.run(cmd, ec)) {
    case Success<OrderPlaced> s -> response.ok(s.event());
    case Failure<OrderPlaced> f -> response.error(f.error());
}
```

## The use case pattern

The Java equivalent of the Go SDK's `usecase.UseCase[Cmd, Evt]`:

```java
package io.flowcatalyst.sdk.usecase;

public interface UseCase<Cmd, Evt> {
    /** Shape validation. Pure function; no I/O. */
    void validate(Cmd command) throws UseCaseError;

    /** Access checks. Returns void on success, throws on denial. */
    void authorize(Cmd command, ExecutionContext ec) throws UseCaseError;

    /** Business work. Returns Result<Evt>; only Success path is via UoW. */
    Result<Evt> execute(Cmd command, ExecutionContext ec);
}
```

The orchestrator (`UseCaseRunner`) calls `validate` → `authorize` →
`execute` and short-circuits on the first failure. Mirrors `usecase.Run`
in the Go SDK.

Errors are typed:

```java
public class UseCaseError extends RuntimeException {
    public enum Kind { VALIDATION, AUTHORIZATION, NOT_FOUND, CONFLICT,
                       BUSINESS_RULE, INTERNAL }
    public final Kind kind;
    public final String code;
    public final String message;

    public static UseCaseError validation(String code, String message) { ... }
    public static UseCaseError authorization(String code, String message) { ... }
    public static UseCaseError notFound(String code, String message) { ... }
    public static UseCaseError conflict(String code, String message) { ... }
    public static UseCaseError businessRule(String code, String message) { ... }
    public static UseCaseError internal(String code, String message, Throwable cause) { ... }

    public int httpStatus() { /* maps kind → 400/403/404/409/422/500 */ }
}
```

## CDI shape

The SDK is a single Quarkus extension. Consumer apps add one Maven
dependency:

```xml
<dependency>
    <groupId>io.flowcatalyst</groupId>
    <artifactId>flowcatalyst-quarkus</artifactId>
    <version>${flowcatalyst.version}</version>
</dependency>
```

And configure via `application.properties`:

```properties
quarkus.flowcatalyst.base-url=https://api.flowcatalyst.io
quarkus.flowcatalyst.client-id=my-app
quarkus.flowcatalyst.client-secret=${FC_CLIENT_SECRET}
quarkus.flowcatalyst.webhook.signing-secret=${FC_WEBHOOK_SECRET}
quarkus.flowcatalyst.webhook.tolerance-secs=300
quarkus.flowcatalyst.outbox.table-name=outbox_messages
```

The extension produces these CDI beans automatically (consumers `@Inject`
them):

| Bean                       | Purpose                                                |
|----------------------------|--------------------------------------------------------|
| `FlowCatalystClient`       | Typed platform HTTP client. Bearer auth wired from config. |
| `WebhookValidator`         | HMAC-SHA256 validator. Signing secret from config.     |
| `UnitOfWork`               | Hibernate-backed UoW. Wired to the datasource.         |
| `Sink`                     | Outbox sink. Writes to `outbox_messages`.              |
| `TokenProvider`            | OAuth2 client_credentials. Fetches and caches tokens.  |
| `Cache`                    | Memory by default; swap via `@Alternative`.            |
| `Lock`                     | Memory by default; swap via `@Alternative`.            |

Consumer code:

```java
@ApplicationScoped
public class CreateOrderUseCase implements UseCase<CreateOrderCommand, OrderPlaced> {

    @Inject OrderRepository repo;
    @Inject UnitOfWork uow;

    @Override
    public void validate(CreateOrderCommand cmd) {
        if (cmd.customerId() == null || cmd.customerId().isBlank()) {
            throw UseCaseError.validation("CUSTOMER_REQUIRED", "customerId is required");
        }
    }

    @Override
    public void authorize(CreateOrderCommand cmd, ExecutionContext ec) {
        // empty — handler enforced the tenant gate
    }

    @Override
    public Result<OrderPlaced> execute(CreateOrderCommand cmd, ExecutionContext ec) {
        var order = Order.fromCommand(cmd, ec.principalId());
        var event = new OrderPlaced(ec, order.id(), order.customerId(), order.amount());
        return uow.commit(order, repo, event, cmd);
    }
}
```

## Virtual threads — always

Every entry point uses `@RunOnVirtualThread`. The SDK's blocking calls
(HTTP, JDBC) are fine on virtual threads — they unmount the carrier when
they block, which is the entire point.

JAX-RS endpoints in consumer apps:

```java
@Path("/orders")
public class OrderResource {

    @Inject CreateOrderUseCase createUseCase;
    @Inject UseCaseRunner runner;

    @POST
    @RunOnVirtualThread
    public Response create(CreateOrderCommand cmd, @Context SecurityContext sec) {
        var ec = ExecutionContext.from(sec);
        var result = runner.run(createUseCase, cmd, ec);
        return switch (result) {
            case Success<OrderPlaced> s -> Response.status(201)
                                                   .entity(Map.of("id", s.event().orderId()))
                                                   .build();
            case Failure<OrderPlaced> f -> Response.status(f.error().httpStatus())
                                                   .entity(f.error()).build();
        };
    }
}
```

The webhook receiver:

```java
@Path("/webhooks")
public class WebhookResource {

    @Inject WebhookValidator validator;

    @POST
    @Path("/orders")
    @RunOnVirtualThread
    public Response receive(@Context HttpHeaders headers, byte[] body) {
        var sig = headers.getHeaderString("X-FlowCatalyst-Signature");
        var ts = headers.getHeaderString("X-FlowCatalyst-Timestamp");
        try {
            validator.validate(sig, ts, body);
        } catch (WebhookException e) {
            return Response.status(e.httpStatus()).build();
        }
        // body is now trusted — parse and process
        return Response.ok().build();
    }
}
```

The SDK's `Runner` for scheduled jobs (the JAX-RS endpoint that handles
inbound webhooks from the FlowCatalyst scheduler) uses
`Executors.newVirtualThreadPerTaskExecutor()` internally for handler
dispatch.

**Do not** use blocking calls from `@Blocking` JAX-RS endpoints unless
you also annotate `@RunOnVirtualThread`. Without the annotation, blocking
calls land on the worker thread pool, which is a *much* smaller
resource than the virtual-thread pool.

## UnitOfWork (Hibernate-backed)

```java
package io.flowcatalyst.sdk.usecase;

public interface UnitOfWork {
    <T, E> Result<E> commit(T aggregate, Persistable<T> repo, E event, Object command);
    <T, E> Result<E> commitDelete(T aggregate, Persistable<T> repo, E event, Object command);
    <E> Result<E> emitEvent(E event);
    // ...
}
```

Hibernate implementation uses `@Transactional` on the commit method so
JTA manages the transaction. Within the transaction:

1. `repo.persist(aggregate, em)` — saves the aggregate via EntityManager.
2. `sink.writeEvent(event, em)` — writes one row to `outbox_messages`.
3. `sink.writeAudit(event, command, em)` — writes audit row (if enabled).

If any step fails, the transaction rolls back and `Failure` is returned.
Success is constructed only inside the SDK module after all three steps
succeed.

The `Persistable<T>` interface is the Java equivalent of
`usecase.Persist[T]`:

```java
public interface Persistable<T> {
    void persist(T entity, EntityManager em);
    void delete(T entity, EntityManager em);  // optional
}
```

## Auth

Three pieces.

### Token validation (resource server)

SmallRye JWT does the heavy lifting. The SDK wraps it to produce a
`TokenContext` with FlowCatalyst-specific semantics:

```java
@ApplicationScoped
public class TokenValidator {
    @Inject JsonWebToken jwt;

    public TokenContext validate() {
        return new TokenContext(
            jwt.getSubject(),
            jwt.getClaim("clients"),
            jwt.getClaim("roles"),
            jwt.getClaim("applications"),
            jwt.getClaim("permissions")
        );
    }
}
```

JWKS discovery is configured via `mp.jwt.verify.publickey.location` —
SmallRye handles caching and rotation.

### OAuth2 client_credentials (service-to-service)

The SDK ships a `TokenProvider` bean that fetches and caches a bearer
token using client_credentials:

```java
@ApplicationScoped
public class ClientCredentialsTokenProvider implements TokenProvider {
    // Reads quarkus.flowcatalyst.client-id, .client-secret, .token-url
    // from config. Caches the token until ~60s before exp.
    public String token() throws AuthException { ... }
}
```

`FlowCatalystClient` injects this and calls `provider.token()` before
each request. Implementations are thread-safe (synchronized refresh).

### Authorization code flow (web app)

For consumer apps with user-facing OAuth login, prefer
**Quarkus OIDC** (`quarkus-oidc` extension) directly. Don't reimplement
this in the SDK — the OIDC extension already handles PKCE, refresh,
session cookies, etc. Document this in the README as the recommended
approach.

## HTTP client

Hand-written using vert.x http-client (the engine under Quarkus REST
Client). Imperative façade — every method blocks (virtual-thread safe).

```java
@ApplicationScoped
public class FlowCatalystClient {

    @Inject @ConfigProperty(name = "quarkus.flowcatalyst.base-url") String baseUrl;
    @Inject TokenProvider tokenProvider;
    @Inject Vertx vertx;

    private final WebClient httpClient = WebClient.create(vertx);

    // Typed accessors per aggregate. Mirrors the Go SDK shape.
    public EventTypes eventTypes()         { return new EventTypes(this); }
    public Subscriptions subscriptions()   { return new Subscriptions(this); }
    public DispatchPools dispatchPools()   { return new DispatchPools(this); }
    public Applications applications()     { return new Applications(this); }
    public Processes processes()           { return new Processes(this); }
    // ...

    // Internal HTTP plumbing — package-private; called by accessors.
    <T> T get(String path, Class<T> responseType) throws ApiException { ... }
    <T, B> T post(String path, B body, Class<T> responseType) throws ApiException { ... }
    // ...
}

public class EventTypes {
    EventTypes(FlowCatalystClient c) { this.c = c; }
    public List<EventType> list() throws ApiException { ... }
    public EventType get(String id) throws ApiException { ... }
    public EventType create(CreateEventTypeRequest req) throws ApiException { ... }
    public EventType update(String id, UpdateEventTypeRequest req) throws ApiException { ... }
    public SyncResult sync(SyncEventTypesRequest req) throws ApiException { ... }
}
```

`ApiException` carries the status code + parsed error envelope. Retry on
transient 5xx is built in (3 attempts, exponential backoff).

## Webhook validator

```java
@ApplicationScoped
public class WebhookValidator {
    @Inject @ConfigProperty(name = "quarkus.flowcatalyst.webhook.signing-secret")
    String signingSecret;
    @Inject @ConfigProperty(name = "quarkus.flowcatalyst.webhook.tolerance-secs",
                             defaultValue = "300")
    int toleranceSecs;

    public void validate(String signature, String timestamp, byte[] payload)
            throws WebhookException {
        // 1. Check headers present.
        // 2. Parse timestamp; reject if expired or in future.
        // 3. Compute HMAC-SHA256 over timestamp || body.
        // 4. Constant-time comparison.
    }
}
```

`WebhookException` extends `RuntimeException` with a `kind` enum
(`MISSING_SIGNATURE`, `MISSING_TIMESTAMP`, `INVALID_TIMESTAMP`,
`TIMESTAMP_EXPIRED`, `TIMESTAMP_IN_FUTURE`, `INVALID_SIGNATURE`,
`MISSING_SECRET`). Maps to HTTP status via `kind.httpStatus()`.

Also ship a JAX-RS `ContainerRequestFilter` that auto-validates any
endpoint annotated `@FlowCatalystWebhook`:

```java
@POST
@Path("/orders")
@FlowCatalystWebhook
@RunOnVirtualThread
public Response receive(OrderEvent event) {
    // Body is already validated by the filter; if we got here, it's trusted.
    process(event);
    return Response.ok().build();
}
```

## Cache + Lock

Same pluggable shape as the Go SDK:

```java
public interface Cache {
    Optional<byte[]> getBytes(String key);
    void setBytes(String key, byte[] value, Duration ttl);
    void delete(String key);
}

public interface Lock {
    Optional<Handle> acquire(String key, Duration ttl);

    interface Handle extends AutoCloseable {
        void release();  // safe to call multiple times
        @Override default void close() { release(); }
    }
}
```

`MemoryCache` and `MemoryLock` ship in the main extension. Redis and
Postgres implementations live in sub-packages with their own Quarkus
extension qualifiers — but for distribution purposes, they're inside the
same umbrella jar (consumers don't need to pull a second dependency).
Driver dependencies are `<optional>true</optional>` so apps that only
use memory don't drag Redis client onto the classpath.

Generic helpers for JSON value caching:

```java
public class Caches {
    public static <T> Optional<T> get(Cache c, String key, Class<T> type) { ... }
    public static <T> void set(Cache c, String key, T value, Duration ttl) { ... }
    public static <T> T getOrSet(Cache c, String key, Class<T> type, Duration ttl,
                                  Supplier<T> compute) { ... }
}
```

## Sync (declarative reconciliation)

Mirror the Go SDK's `sync.DefinitionSet` builder + `Synchronizer`:

```java
var set = DefinitionSet.forApplication("orders")
    .addRole(Role.make("admin")
                 .withDisplayName("Orders Admin")
                 .withPermissions("orders:read", "orders:write"))
    .addRole(Role.make("viewer")
                 .withPermissions("orders:read")
                 .clientManagedEnabled())
    .addEventType(EventType.make("orders:sales:order:placed", "Order placed"))
    .addEventType(EventType.make("orders:sales:order:cancelled", "Order cancelled"))
    .addDispatchPool(DispatchPool.make("default", "Default pool")
                                 .withConcurrency(8))
    .addSubscription(Subscription.make("order-billing", "Forward placed orders",
                                       "https://billing.example.com/webhooks/orders")
                                 .withDispatchPool("default")
                                 .withMode("Immediate")
                                 .withMaxRetries(5)
                                 .bind("orders:sales:order:placed", null));

var result = synchronizer.sync(set, SyncOptions.removeUnlisted());
```

One platform call per category (roles, event types, subscriptions,
dispatch pools, principals, processes). Per-category errors captured on
the result; no cross-category abort.

## ScheduledJobs Runner

The runner is a JAX-RS resource the consumer mounts at a known path. The
FlowCatalyst scheduler posts to it; the runner dispatches to registered
`JobHandler` beans.

```java
@Path("/scheduled-jobs")
public class ScheduledJobsResource {
    @Inject ScheduledJobsRunner runner;

    @POST
    @Path("/run")
    @FlowCatalystWebhook
    @RunOnVirtualThread
    public Response run(JobInvocation invocation) {
        return runner.handle(invocation);
    }
}
```

Consumer registers handlers via CDI:

```java
@ApplicationScoped
@JobHandlerFor("orders:reconcile-daily")
public class ReconcileDailyHandler implements JobHandler {
    @Inject FlowCatalystClient platform;
    @Inject OrderRepository orders;

    @Override
    public JobResult run(JobContext ctx) throws Exception {
        ctx.log(LogLevel.INFO, "starting daily reconciliation");
        // ... work ...
        return JobResult.success("processed " + count + " orders");
    }
}
```

The runner:
1. Validates the webhook signature.
2. Acquires a lock via `Lock` provider (so a single job code runs once
   even if the scheduler delivers twice).
3. Dispatches to the registered handler.
4. Streams log lines back to the platform via
   `client.scheduledJobs().log(instanceId, level, message)`.
5. Reports completion via
   `client.scheduledJobs().complete(instanceId, status, message)`.
6. Surfaces errors via an `OnError` callback (CDI event or builder hook).

## TSID

Direct port of the Go SDK's `tsid` package. Same alphabet, same layout,
same prefixes:

```java
public final class Tsid {
    public static String generate(EntityType type) { ... }
    public static String generateWithPrefix(String prefix) { ... }
    public static String generateUntyped() { ... }
    public static OptionalLong toLong(String s) { ... }
    public static String fromLong(long v) { ... }
}

public enum EntityType {
    CLIENT("clt"), PRINCIPAL("prn"), APPLICATION("app"),
    SERVICE_ACCOUNT("sac"), ROLE("rol"), PERMISSION("prm"),
    // ... 35 total, same as Go SDK ...
    ;
    private final String prefix;
    EntityType(String prefix) { this.prefix = prefix; }
    public String prefix() { return prefix; }
}
```

A counter (`AtomicInteger`) and `SecureRandom` instance live as static
fields. Generation is thread-safe.

**Cross-SDK parity test**: shared test vectors verify the same numeric
input produces the same encoded output across all five SDKs.

## Native image readiness

Day-1 requirement. The Quarkus extension's deployment module registers:

- Reflection for all DTO classes (Jackson serialization).
- Resource bundles needed (none expected).
- Substitutions for any platform-specific reflection (none expected).
- Health-check beans (`/q/health`).
- OpenAPI integration so consumer endpoints get documented automatically.

CI builds a native binary on every commit. Native test suite is a subset
of the JVM test suite — the integration tests that exercise HTTP, DB,
and the use-case lifecycle.

```bash
mvn package -Pnative
./target/flowcatalyst-quarkus-runner --help  # native binary
```

Expected binary size: ~80-120MB for an example app with full SDK + Postgres
driver + Redis client. Native startup: ~50-100ms.

## Build infrastructure

- **Maven** for the SDK itself. Multi-module:
  - `flowcatalyst-quarkus-parent` — version + dependency management
  - `flowcatalyst-quarkus` — runtime module (consumer dependency)
  - `flowcatalyst-quarkus-deployment` — build-time processor
  - `flowcatalyst-quarkus-integration-tests` — full-stack tests
- **Gradle support** for consumers — the umbrella extension works with
  both Quarkus Maven and Gradle plugins. No special handling needed.
- **GitHub Actions** CI matrix: JDK 21 + JDK 25; JVM + native; Postgres +
  H2 in-memory.

## Examples

Five runnable examples, mirroring the Go SDK's `examples/` directory:

1. **`order-service`** — end-to-end UoW flow. Single best "how do I write a
   consumer app" reference. Hibernate ORM, `@Transactional`, sealed
   `Result` pattern.
2. **`fc-sync`** — declarative reconciliation. A CLI Quarkus app
   (`@QuarkusMain`) that builds a `DefinitionSet` and pushes it.
3. **`webhook-receiver`** — JAX-RS app demonstrating
   `@FlowCatalystWebhook` annotation + native build.
4. **`scheduled-jobs-runner`** — Quarkus app exposing the runner endpoint
   with two registered handlers and a memory lock provider.
5. **`list-event-types`** — minimal client + auth wiring; smallest
   possible "hello SDK" example.

Each is its own Maven module under `examples/`, runnable with
`mvn quarkus:dev` for live reload or `mvn package -Pnative` for native
build.

## Testing strategy

- **Unit tests** for pure logic (TSID, webhook validator, sync builder)
  — plain JUnit 5.
- **Integration tests** using `@QuarkusTest` — boot the test app, hit the
  HTTP API, verify DB state. Uses Testcontainers for Postgres (no
  embedded DB; we want the real driver behaviour).
- **Native image tests** using `@QuarkusIntegrationTest` — subset of
  integration tests verified against the native binary.
- **Cross-SDK parity tests** — consume the golden vectors at
  `crates/fc-router/tests/golden/` and verify byte-identical output:
  - TSID generation from seed inputs
  - Webhook signature computation
  - CloudEvent JSON serialization
- **Mutation testing** via PIT — optional, for the use-case orchestration
  code where correctness matters most.

## Phased delivery

Five phases. Each delivers something useful in isolation; later phases
build on earlier ones.

### Phase 1 — Foundation (no platform integration yet)

- `tsid` package + tests + cross-SDK parity vectors
- `webhook.WebhookValidator` + tests
- `auth.TokenValidator` + `TokenContext` (SmallRye JWT integration)
- `client.FlowCatalystClient` skeleton + a few resource accessors
  (event_types, subscriptions, applications)
- Build setup, native image proof-of-life
- `list-event-types` example (the smallest one)

Verifies: native build works; SmallRye integration works; HTTP client
shape is right.

### Phase 2 — Use case + outbox

- `usecase` package — `UseCase`, `Result`, `Success`, `Failure`,
  `UseCaseRunner`, `ExecutionContext`, `UseCaseError`, `DomainEvent`
- `usecase.UnitOfWork` interface
- Hibernate implementation of UoW (`HibernateUnitOfWork`)
- `outbox.Sink` interface + Hibernate-based `OutboxSink` (writes to
  `outbox_messages`)
- `order-service` example

Verifies: the sealed Result pattern works; transactions commit
atomically; outbox rows match Rust SDK byte-for-byte.

### Phase 3 — Sync + ScheduledJobs

- Remaining `client` accessors (audit_logs, dispatch_pools, principals,
  roles, permissions, processes, scheduled_jobs)
- `sync.DefinitionSet` builder + `Synchronizer`
- `scheduledjobs.JobHandler` + `Runner` + `@JobHandlerFor` annotation
- `lock.Lock` interface + `MemoryLock` (Redis impl in Phase 4)
- `auth.ClientCredentialsTokenProvider`
- `fc-sync` and `scheduled-jobs-runner` examples

Verifies: declarative reconciliation; runner serialises via lock;
client_credentials token caching.

### Phase 4 — Drivers + Reactive

- `cache` package — `Cache` interface, `MemoryCache`, Redis impl,
  Postgres impl
- `lock` Redis impl
- MySQL driver support (Phase 1-3 are Postgres-only)
- Optional Mutiny wrappers for the HTTP client (`asyncList()`,
  `asyncGet()`) — for consumers who want reactive composition
- `webhook-receiver` example with native build verification

### Phase 5 — Polish

- README + docs site (mdBook or similar)
- ERRORS.md (mirror Go SDK)
- Per-package JavaDoc audit (`@since`, `@see` cross-refs, examples in
  `{@code}` blocks)
- Performance baseline: throughput numbers for HTTP fan-out, outbox
  write rate, use-case orchestration overhead
- Migration guide: "moving an app from the Go SDK or TS SDK to this one"

## Open decisions (defer to implementation)

These are not blockers — pick at the time and document the choice in
the implementation PR:

1. **Module system**: JPMS (`module-info.java`) for hard internal-only
   boundaries, OR rely on package-private visibility + Maven Enforcer
   rules? JPMS is stricter but adds complexity.
2. **DTO style**: Records (Java 16+) for all DTOs, OR Lombok value
   classes for compatibility with older Java? Recommend records; revisit
   if a consumer pushes back on Java 21 minimum.
3. **Validation**: Use Jakarta Bean Validation (`@NotBlank`, `@NotNull`)
   on command DTOs, OR keep validation explicit inside `validate()`?
   Recommend explicit (matches Go SDK shape; no annotation magic for the
   business rule check).
4. **Logging**: stdout via JBoss Logging (Quarkus default) or shadowed
   to Log4j2? Recommend JBoss Logging — Quarkus-native, no extra
   dependency.
5. **JSON**: Jackson (default) or JSON-B? Recommend Jackson — broader
   ecosystem support, faster native compilation.
6. **Reactive entry point**: how reactive façades expose blocking calls.
   Recommend `Uni.createFrom().item(() -> blockingCall())` rather than
   maintaining two implementations.

## What NOT to do

- **Don't reach for Spring concepts** (`@Service`, `@Repository`,
  `@Component`). Use Quarkus-native: `@ApplicationScoped`. Spring users
  will need to adjust; that's fine.
- **Don't use `@Transactional` from Spring TX**. Use Jakarta Transactions
  `@Transactional`. Same name, different import.
- **Don't add Spring Boot starters as transitive dependencies**. Anything
  for Quarkus comes from the Quarkus ecosystem (SmallRye, MicroProfile,
  Mutiny).
- **Don't wrap every blocking call in a thread pool**. Virtual threads
  eliminate the need; the SDK should let blocking happen on the
  virtual-thread carrier and let the runtime handle it.
- **Don't break wire compatibility** without coordinating across all
  five SDKs (Rust, TS, Laravel, Go, Quarkus). The Rust SDK is canonical;
  changes propagate from there.
- **Don't ship before the cross-SDK parity tests pass**.

## What to skip / leave alone

- **Spring Boot integration**. Not a goal. Spring users can wrap the
  SDK's JAX-RS-free utility classes (validator, client) inside their own
  beans if they want — but the umbrella extension is Quarkus-only.
- **Android / JVM-on-mobile support**. Out of scope.
- **Pre-Java 17**. Sealed types are not optional; records are not
  optional; pattern matching is not optional. Minimum Java 21 (LTS).

## Working agreement

- One Phase = one milestone, not one PR. Each phase opens multiple PRs
  reviewed independently.
- Cross-SDK parity tests are gating. A PR that breaks a parity vector
  is reverted, not "fixed in a follow-up."
- Native image must build and run on every PR (CI matrix).
- Wire-format-affecting changes require coordination across SDKs —
  raise an issue in the canonical Rust repo first.
- Don't add new transitive dependencies without consideration of native
  image impact. Each dependency added to the umbrella extension is
  evaluated for native compatibility before merge.
