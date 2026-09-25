# FlowCatalyst Java SDK

Plain-Java client SDK for the FlowCatalyst platform. Java 25+, blocking API
(scale with virtual threads), Jackson as the only runtime dependency. Mirrors
the TypeScript SDK's surface: control-plane resources, transactional outbox,
declaration sync, and webhook signature verification.

```java
var client = FlowCatalystClient.builder()
        .baseUrl("https://your-instance.flowcatalyst.io")
        .clientCredentials("oac_your_client_id", "your_client_secret")
        .build();

var eventTypes = client.eventTypes().list(null);
```

## Modules

| Package | What it does |
|---|---|
| `io.flowcatalyst.sdk` | `FlowCatalystClient` — builder config, resource accessors |
| `…sdk.resources` | 15 typed resource families (event types, subscriptions, dispatch pools, connections, roles, permissions, applications, clients, principals, processes, scheduled jobs, audit logs, me, router) |
| `…sdk.error` | `sealed interface SdkError` + `FlowCatalystException` — handle failures with a pattern-matching `switch` |
| `…sdk.outbox` | Transactional outbox: `OutboxManager`, DTO builders, `OutboxDriver` SPI + `JdbcOutboxDriver`, raw SQL migrations in `migrations/` |
| `…sdk.tsid` | TSID generation (13-char Crockford Base32, platform-compatible; collision-free monotonic sequence) |
| `…sdk.sync` | `DefinitionSynchronizer` + `DefinitionSet` — bulk-sync roles / event types / connections / subscriptions / dispatch pools / principals / processes / scheduled jobs / OpenAPI per application |
| `…sdk.annotations` | `@AsEventType` / `@AsConnection` / `@AsSubscription` / `@AsDispatchPool` / `@AsRole` + `DefinitionScanner` (explicit class registration — no classpath scanning) |
| `…sdk.webhook` | `WebhookSignature.verify(...)` — HMAC-SHA256 verification of signed deliveries |

## Auth modes

- **Client credentials** (service account): `.clientCredentials(id, secret)` —
  tokens are cached with a 60s expiry buffer, refreshed single-flight, and
  transparently refreshed once on a 401.
- **User token**: `.accessToken(String)` or `.accessToken(Supplier<String>)` —
  the host app owns refresh; 401s are surfaced, not retried.

Transient statuses (408/429/502/503/504) retry with exponential backoff
(default 3 attempts, 100ms base delay).

## Error handling

Every failure is a `FlowCatalystException` carrying one `SdkError` variant:

```java
try {
    client.eventTypes().get(id);
} catch (FlowCatalystException e) {
    switch (e.error()) {
        case SdkError.NotFound nf -> handleMissing();
        case SdkError.RateLimited rl -> backOff(rl.retryAfter());
        case SdkError.Validation v -> log(v.errors());
        default -> throw e;
    }
}
```

## Transactional outbox

Events are not published over HTTP — they are written to your database's
`outbox_messages` table inside your own transaction (migrations in
`migrations/postgresql` and `migrations/mysql`), and the outbox poller ships
them:

```java
var driver = new JdbcOutboxDriver(dataSource);
var outbox = new OutboxManager(driver, "clt_your_client_tsid");

driver.withTransaction(tx -> {
    // business writes on (Connection) tx ...
    outbox.createEvent(CreateEventDto
            .create("orders:sales:order:placed", Map.of("orderId", orderId))
            .withMessageGroup(orderId), tx);
    return null;
});
```

Event `type`s and dispatch-job `code`s must be fully qualified
`application:subdomain:aggregate:action` strings — the SDK rejects bare codes.

## Declaring definitions

Programmatically:

```java
var set = Definitions.DefinitionSet.define("orders")
        .withEventTypes(List.of(
                Definitions.EventType.of("orders:sales:order:placed", "Order Placed")))
        .withRoles(List.of(Definitions.Role.of("admin")
                .withPermissions(List.of(Definitions.Permission.of("admin", "*", "*")))));

client.definitions().sync(set, SyncOptions.removingUnlisted());
```

Or with annotations and explicit registration:

```java
@AsEventType(code = "orders:sales:order:placed", name = "Order Placed")
public record OrderPlaced(String orderId) {}

var set = DefinitionScanner.scan("orders", List.of(OrderPlaced.class));
client.definitions().sync(set);
```

### The application code

Pass it directly, or inherit it from `FLOWCATALYST_APP_CODE`:

```java
var set = Definitions.DefinitionSet.define("orders");   // explicit
var set = Definitions.DefinitionSet.defineFromEnv();    // FLOWCATALYST_APP_CODE
```

`defineFromEnv()` throws `IllegalStateException` when the variable is unset or
blank, rather than letting a missing code surface later as a request to
`/api/applications/null/…`.

There is no per-definition application override: the set a definition is built
into *is* its application. For several applications, build one set each and
pass them to `client.definitions().syncAll(sets, options)`.

### Connections and subscriptions

A connection is application-owned: the platform assigns its service account
itself (the application's own provisioned one), so `Connection` carries
nothing environment-specific — no service account id, no secret. Connections
sync BEFORE subscriptions, so a subscription's `connectionCode` resolves in
the same run:

```java
var set = Definitions.DefinitionSet.define("orders")
        .withConnections(List.of(
                Definitions.Connection.of("orders-webhook", "Orders Webhook")))
        .withSubscriptions(List.of(Definitions.Subscription.of(
                        "order-shipped-hook", "Order Shipped Hook",
                        "https://app.example.com/webhooks/order-shipped",
                        List.of(Definitions.SubscriptionEventType.of(
                                "orders:fulfillment:shipment:shipped")))
                .withConnectionCode("orders-webhook")));

client.definitions().sync(set, SyncOptions.removingUnlisted());
```

`connectionCode` names a connection in one of two namespaces, with **no
fallback** between them: a bare code names a connection owned by THIS
application; `.withSharedConnection(true)` names a shared (application-less)
one instead. `connectionId` still works but is environment-specific (ids
differ per environment; codes don't).

A subscription's `target` may be a path (`/webhooks/orders`) instead of an
absolute URL — it is resolved at sync time against, in order, the definition
set's own base (see `forClient` below) then a synchronizer-level default:

```java
var synchronizer = new DefinitionSynchronizer(client.transport(), "https://app.example.com");
synchronizer.sync(set);
```

or configure the default once on the client:

```java
var client = FlowCatalystClient.builder()
        .baseUrl("https://your-instance.flowcatalyst.io")
        .clientCredentials("oac_your_client_id", "your_client_secret")
        .subscriptionTargetBaseUrl("https://app.example.com")
        .build();
```

A blank target, or a path with no base available anywhere, fails that
subscription's sync LOCALLY (naming it) and sends nothing for the whole
group — under `removeUnlisted`, sending a partial list would delete the
subscriptions left out.

### Failure handling: `DefinitionSyncException`

`sync`, `syncAll` and `syncGrouped` all **throw `DefinitionSyncException`**
(a `FlowCatalystException`) if ANY category of ANY application they synced
came back failed — a duplicate code, an unresolvable target, or a connection
sync failure that skipped its subscriptions. A caller that doesn't inspect
every category of the returned `SyncResult` (a deploy step that just calls
`sync(set)` and relies on "no exception" to mean success, say) would
otherwise report success while part of the sync silently did not happen.

Every application/scope that COULD run still runs before the exception is
thrown — one tenant's bad definition, or one scope's failed connection sync,
does not stop its siblings from being attempted. What DID sync is never
lost: it's carried on the exception via a typed accessor.

```java
try {
    client.definitions().sync(set, SyncOptions.removingUnlisted());
} catch (DefinitionSyncException e) {
    SyncResult partial = e.result();               // never null when sync() threw
    if (partial.connections() instanceof SyncResult.Category.Failed f) {
        log.warn("connections failed: {}", f.error());
    }
    throw e;   // or handle/report and continue, as your deploy step needs
}
```

`syncAll` and `syncGrouped` run every set/application to completion first,
then throw ONCE at the end — never at the first failing one — carrying every
result, including the ones that synced fully:

```java
try {
    Map<String, SyncResult> results = client.definitions().syncGrouped(sets, options);
} catch (DefinitionSyncException e) {
    e.resultsByApplication().forEach((app, result) -> { /* inspect each */ });
    throw e;
}
```

| Thrown by | Partial result accessor | Type |
|---|---|---|
| `sync` | `e.result()` | `SyncResult` |
| `syncAll` | `e.results()` | `List<SyncResult>`, same order as the input sets |
| `syncGrouped` | `e.resultsByApplication()` | `Map<String, SyncResult>`, keyed by application code |

Only the accessor matching the method that threw is non-null; the others are
null. `e.getMessage()` names every failed category and its error text across
every application, so even an uninspected `catch` block's log line is
actionable.

A genuinely uncaught exception from a category that does not catch its own
HTTP failures (roles, event types, dispatch pools, principals, processes,
scheduled jobs, OpenAPI — connections and subscriptions are the ones that
catch theirs, per above) still propagates immediately and stops `syncAll`/
`syncGrouped` at whichever set was running, exactly as it always has — that
is a real, unrecovered failure (e.g. a network outage), not a partial result
to collect.

A pure success — no category anywhere failed — returns exactly as before;
nothing changes on the happy path.

### Client scoping and multi-tenant applications

Connections and subscriptions may be scoped to a FlowCatalyst **client**,
always by its identifier slug — never its id, since ids differ per
environment. The platform treats each `(application, client)` sync as the
COMPLETE list for that scope, so the SDK issues one platform call per
distinct client (global first, connections before subscriptions within
each) and never merges or splits a scope across calls.

A single-tenant application can set a client per row:

```java
Definitions.Connection.of("orders-webhook", "Orders Webhook").withClient("acme");
```

or via the annotation:

```java
@AsConnection(code = "orders-webhook", name = "Orders Webhook", client = "acme")
```

A **multi-tenant** application should NOT do this per-row — build one
`DefinitionSet` per `(application, client)` instead, the tenant list being
your own runtime data (never an annotation):

```java
var tenantSet = Definitions.DefinitionSet.define("orders")
        .forClient("acme", "https://acme.example.com")   // per-tenant target base URL, optional
        .withConnections(...)
        .withSubscriptions(...);

client.definitions().sync(tenantSet, SyncOptions.removingUnlisted());
```

A row's own `client` wins over its set's; a scanned annotation's own
`client()` wins over the `defaultClient` passed to `DefinitionScanner.scan`:

```java
var set = DefinitionScanner.scan("orders", classes, "acme");   // single-tenant default
```

### Syncing several sets for one application: `syncAll` vs `syncGrouped`

`sync`/`syncAll` keep single-set behaviour and do **not** merge — two sets
targeting the same `(application, client)` scope become two platform calls,
and the second's `removeUnlisted` deletes what the first just created.

`syncGrouped` MERGES every set sharing an application code into one combined
sync before calling the platform — the safe way to combine, say, scanned
annotation definitions with a multi-tenant provider's per-tenant sets:

```java
Map<String, SyncResult> results = client.definitions().syncGrouped(
        List.of(scannedSet, tenantSetAcme, tenantSetBeta),
        SyncOptions.removingUnlisted());
```

The same code appearing twice in one `(application, client)` scope after
merging is a configuration error: that type's sync for that scope fails
LOCALLY, naming the code and the scope, and nothing is sent for it — other
types and other scopes still sync. As with `sync`, a failure anywhere means
`syncGrouped` throws `DefinitionSyncException` — see [Failure
handling](#failure-handling-definitionsyncexception) above for how to read
`e.resultsByApplication()`.

## Webhook verification

```java
WebhookSignature.verify(rawBodyBytes,
        request.header("X-FlowCatalyst-Signature"),
        request.header("X-FlowCatalyst-Timestamp"),
        signingSecret);
```

The body must be the raw bytes as received (sign-then-parse).

## Building

```
make build-java-sdk        # from the repo root: regenerates models + verify
```

Models under `io.flowcatalyst.sdk.generated.model` are generated at build time
from `openapi/openapi.json` (refreshed by `make sdk-spec`) — wire drift
surfaces as compile errors in the hand-written resource layer.

Examples live in `src/test/java/examples/`; run with:

```
mvn -q test-compile org.codehaus.mojo:exec-maven-plugin:3.5.0:java \
    -Dexec.mainClass=examples.ListEventTypes -Dexec.classpathScope=test
```

Releases: `make release-java-sdk BUMP=minor` (tags `java-sdk/vX.Y.Z`).
