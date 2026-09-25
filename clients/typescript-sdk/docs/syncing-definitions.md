# Syncing Definitions

This guide covers how to sync your application's **roles, event types, connections, subscriptions, dispatch pools, and principals** to the FlowCatalyst platform from TypeScript.

---

## Why sync?

FlowCatalyst is a control-plane for event-driven systems. The platform owns:

- **Roles + permissions** — who can do what
- **Event types** — what kinds of events exist and flow through the pipeline
- **Connections** — the credentials (the application's own provisioned service account) a subscription delivers under
- **Subscriptions** — who consumes which events and how they're delivered
- **Dispatch pools** — how outbound deliveries are throttled and parallelised
- **Principals** — the users allowed to act against your application

Rather than managing these by hand in the admin UI for every environment, **define them in code** next to the feature they belong to, and let a sync step push them to the platform. This keeps code and config in one place, gives you review/history through git, and lets CI enforce drift detection.

---

## Core concepts

### DefinitionSet — one application, all its definitions

Every sync operation is scoped to a single application (`applicationCode`). A `DefinitionSet` carries all of that application's declarations:

```ts
import { sync } from "@flowcatalyst/sdk";

const definitions = sync
	.defineApplication("orders")
	.withRoles([...])
	.withEventTypes([...])
	.withConnections([...])
	.withSubscriptions([...])
	.withDispatchPools([...])
	.withPrincipals([...])
	.build();
```

You can also build the plain object directly if the fluent API isn't your style:

```ts
const definitions: sync.DefinitionSet = {
	applicationCode: "orders",
	roles: [{ name: "admin", displayName: "Administrator" }],
	eventTypes: [{ code: "orders:fulfillment:shipment:shipped", name: "Shipment Shipped" }],
};
```

Omit any category you don't want to touch. The sync call only hits categories that are present.

### Sync order

When you call `client.definitions().sync(...)`, categories are processed in this order — **don't rely on the platform to figure it out**:

1. **Roles** (so principals can reference them)
2. **Event types** (so subscriptions can bind to them)
3. **Connections** (so subscriptions can bind to them)
4. **Subscriptions** (depend on event types, connections, and dispatch pools)
5. **Dispatch pools**
6. **Principals** (depend on roles)

Each category is an **independent HTTP call**. A failure in one category does NOT roll back earlier successes. If you need all-or-nothing, check the `SyncResult` after and decide your own compensation.

Connections and subscriptions are further grouped by **client** (see
[Client scoping](#client-scoping) below): one platform call per
(application, client) scope, global scope first, connections before
subscriptions within each scope. If a scope's connection sync fails, that
scope's subscription sync is skipped (its `connectionCode`s can't resolve),
recorded as an error on `SyncResult.subscriptions` — but a sibling scope
(a different client, or the global scope) still gets its own calls. **A
scope failing this way still fails the overall `sync()` call** — `isErr()`
alone is enough to detect it — see
[What if a sync partially fails?](#what-if-a-sync-partially-fails).

### `removeUnlisted` — drift vs. accumulation

By default, sync is **additive**: rows you omit stay put. Pass `removeUnlisted: true` to also delete SDK-sourced rows that aren't in the submitted list. Admin-UI-created rows are always preserved regardless — sync only touches what it owns.

Use `removeUnlisted: true` when your codebase is the source of truth. Use `false` (the default) when the platform also accepts admin-UI-created rows in the same category.

---

## Roles

```ts
import { sync } from "@flowcatalyst/sdk";

const roles: sync.RoleDefinition[] = [
	{
		name: "admin",
		displayName: "Administrator",
		description: "Full access to orders and fulfillment",
		permissions: [
			"orders:admin:order:read",
			"orders:admin:order:write",
			"orders:admin:shipment:*",
		],
		clientManaged: false,
	},
	{
		name: "viewer",
		displayName: "Viewer",
		permissions: ["orders:admin:order:read"],
		clientManaged: true,
	},
];
```

### Naming

`name` is the **short** role name, without the application prefix. Given `applicationCode: "orders"` and `name: "admin"`, the role is persisted as **`orders:admin`**. Do not include the prefix in `name` yourself — the platform adds it.

### Permissions

Permission strings follow a **4-part format**:

```
<domain>:<area>:<resource>:<action>
```

- **domain** — top-level namespace, usually your `applicationCode` or `platform` for built-ins
- **area** — sub-module within the domain (e.g. `admin`, `fulfillment`, `iam`)
- **resource** — the entity being acted on (e.g. `order`, `shipment`, `user`)
- **action** — the verb (`read`, `write`, `create`, `update`, `delete`, `cancel`, …)

Examples:
- `orders:admin:order:read`
- `orders:fulfillment:shipment:cancel`
- `platform:iam:user:create`

**Wildcards** are supported in any position:

| Pattern | Matches |
|---|---|
| `orders:admin:order:*` | every action on `order` in `orders:admin` |
| `orders:admin:*:read` | read on any resource in `orders:admin` |
| `orders:*:*:*` | everything under `orders` |
| `*:*:*:*` | full superuser (reserved for the platform `super-admin` role) |

Keep permissions fine-grained when you define them; collapse with wildcards only when you truly mean "all of this scope".

#### Reusable permission factories

Since a permission is usually shared across roles, define it **once** with the
`permission()` factory and link roles to it. The `application` segment defaults
to the set's `applicationCode`, so you don't repeat it:

```ts
import { defineApplication, permission } from "@flowcatalyst/sdk/sync";

const ViewPosts = permission({ context: "posts", aggregate: "post", action: "view" });
const EditPosts = permission({ context: "posts", aggregate: "post", action: "edit" });

const set = defineApplication("blog")
  .withPermissions([ViewPosts, EditPosts]) // optional standalone catalogue
  .withRoles([{ name: "editor", permissions: [ViewPosts, EditPosts] }])
  .build();
// role "blog:editor" → ["blog:posts:post:view", "blog:posts:post:edit"]
```

`build()` resolves factories to wire strings. A role's `permissions` accepts a
mix of `permission()` factories and plain 4-part strings. FlowCatalyst has no
standalone "create permission" endpoint — permissions reach the platform via
the roles that grant them, so the standalone catalogue is for reuse and
documentation on the client side.

### `clientManaged`

- `clientManaged: false` — only platform admins can assign this role to users. Use for roles that grant elevated access or span clients.
- `clientManaged: true` — client admins can assign this role to users within their own client. Use for role-per-user-level roles like `editor` or `viewer` that make sense at the customer tenant level.

---

## Event types

```ts
const eventTypes: sync.EventTypeDefinition[] = [
	{
		code: "orders:fulfillment:shipment:shipped",
		name: "Shipment Shipped",
		description: "Emitted when a shipment leaves the warehouse",
	},
	{
		code: "orders:fulfillment:shipment:delivered",
		name: "Shipment Delivered",
	},
];
```

### Code format

Event type codes are **4-part strings**:

```
<application>:<subdomain>:<aggregate>:<event>
```

- **application** — MUST match the `applicationCode` being synced
- **subdomain** — a bounded context inside the app (e.g. `fulfillment`, `billing`, `catalog`)
- **aggregate** — the domain aggregate the event is about (e.g. `shipment`, `invoice`, `product`)
- **event** — a past-tense verb describing what happened (e.g. `shipped`, `issued`, `archived`)

Past-tense matters — events are **facts**, not commands. `orders:fulfillment:shipment:shipped` is good. `orders:fulfillment:shipment:ship` is not.

### JSON schemas

**The SDK sync endpoint does not upload JSON schemas for event types.** If you want schemas attached, use the admin UI or the per-resource API:

```ts
await client.eventTypes().addSchema(eventTypeId, {
	version: "1.0",
	schema: { /* JSON Schema */ },
});
```

This is a deliberate limitation — schemas often evolve separately from the list of events and aren't a good fit for declarative sync.

### Code sourcing

Event types sync'd this way are tagged with `source: "SDK"` on the platform, distinguishing them from admin-UI-created ones. When `removeUnlisted: true`, only SDK-sourced event types are candidates for removal — admin-UI ones are always safe.

---

## Connections

A connection carries nothing environment-specific — the platform assigns the
application's own provisioned service account itself, so one definition
serves every environment. It exists so a subscription's `connectionCode` has
something to resolve; connections are always synced **before** subscriptions
in the same run, so resolution succeeds without a second sync.

```ts
const connections: sync.ConnectionDefinition[] = [
	{
		code: "orders-webhook",
		name: "Orders Webhook",
		description: "Delivers order events to the fulfillment service",
	},
];
```

| Property      | Required | Description |
| ------------- | -------- | ------------ |
| `code`        | Yes      | Unique connection code — stable across environments; what a subscription's `connectionCode` names |
| `name`        | Yes      | Human-readable name |
| `description` | No       | Free text |
| `externalId`  | No       | Your own system's identifier for this connection |
| `client`      | No       | FlowCatalyst client (identifier slug) this connection is scoped to. Omit for global. See [Client scoping](#client-scoping) |

---

## Subscriptions

```ts
const subscriptions: sync.SubscriptionDefinition[] = [
	{
		code: "shipment-tracking",
		name: "Shipment Tracking Webhook",
		description: "Notifies the tracking service of shipment state changes",
		target: "https://tracking.example.com/webhooks/flowcatalyst",
		connectionCode: "orders-webhook",
		eventTypes: [
			{ eventTypeCode: "orders:fulfillment:shipment:shipped" },
			{ eventTypeCode: "orders:fulfillment:shipment:delivered" },
		],
		dispatchPoolCode: "default",
		mode: "BLOCK_ON_ERROR",
		maxRetries: 5,
		timeoutSeconds: 30,
		dataOnly: false,
	},
];
```

### `target`: absolute URL, or a path resolved at sync time

`target` is either:

- **An absolute URL** (`https://tracking.example.com/webhooks/flowcatalyst`) — sent verbatim.
- **A path** (`/webhooks/orders`) — resolved at sync time against, in order:
  1. the owning `DefinitionSet`'s `targetBaseUrl` (set via `.forClient(client, targetBaseUrl)`, or a set-level `targetBaseUrl` after `syncGrouped()` merges — see [Multi-tenant applications](#multi-tenant-applications));
  2. the synchronizer's `subscriptionTargetBaseUrl` option, set once via `client.definitions({ subscriptionTargetBaseUrl: "https://api.example.com" })`.

There is no further fallback — unlike some frameworks, the SDK has no
"app URL" to default to. A blank target, or a path with nothing to resolve
against, fails **that subscription's scope** locally (naming the
subscription) rather than sending a partial list — under `removeUnlisted` an
omitted row would be deleted.

### `connectionCode` / `connectionId` / `sharedConnection`

- **`connectionCode`** (preferred) — the code of a connection that delivers this subscription. By default it names a connection **owned by this application** (defined via `withConnections(...)` on the same `DefinitionSet`, or synced earlier). Set `sharedConnection: true` to instead name a **shared** (application-less) connection.
- **`connectionId`** — a connection's id directly. Environment-specific (ids differ per environment) — prefer `connectionCode` in anything synced to more than one environment. Kept for the (rare) case where you're wiring a subscription to a connection you don't manage in code.
- **`sharedConnection`** — `true` selects the shared namespace for `connectionCode`. There is **no fallback** between "this application's own" and "shared": naming a code that doesn't exist in the chosen namespace 404s (`CONNECTION_NOT_FOUND`) rather than silently resolving to the other connection's credentials.

### `eventTypes` bindings

Each binding is `{ eventTypeCode, filter? }`. `filter` is an optional expression evaluated against the event payload — if it evaluates false, the delivery is skipped. See the platform docs for filter syntax.

### `mode`

- `IMMEDIATE` (default) — every event for this subscription is delivered independently. Failures don't affect other deliveries.
- `BLOCK_ON_ERROR` — within a message group (per-aggregate ordering), a failure holds subsequent deliveries until the failure is retried successfully or expires. Use this for subscriptions where order matters (e.g. "must-see-shipped-before-delivered"). Has throughput cost under failure.

### `dataOnly`

- `false` (default) — POST body is the full event envelope (metadata + `data`).
- `true` — POST body is just the event `data` field. Use when the receiver is a dumb webhook that doesn't care about FlowCatalyst metadata.

---

## Client scoping

Two independent axes — don't conflate them:

| Term | Means |
|---|---|
| **global** | a connection/subscription with no client |
| **client-scoped** | bound to one FlowCatalyst client |
| **shared** connection | owned by no application (`sharedConnection: true` on a subscription) |
| **application-owned** connection | owned by the application being synced (the default) |

Both `ConnectionDefinition` and `SubscriptionDefinition` accept an optional
`client` — the FlowCatalyst client (**identifier slug, never an id** — ids
differ per environment) the row belongs to:

```ts
const connections: sync.ConnectionDefinition[] = [
	{ code: "acme-webhook", name: "Acme Webhook", client: "acme" },
];
```

For a **single-tenant** application (one codebase, at most one client), set
`client` per row, or — more commonly — scope the **whole set** once with
`.forClient(...)` (see [Multi-tenant applications](#multi-tenant-applications)
below; it works just as well for a single client).

The platform treats **each (application, client) scope as a separate call**,
and with `removeUnlisted: true` deletes everything of that scope the call
doesn't list. The SDK therefore:

- groups connections and subscriptions by their effective client (a row's
  own `client`, else the set's) into **one platform call per scope**;
- syncs the **global scope before any client scope** (a client-scoped
  subscription may reference a global connection, so it must exist first);
- **never merges** two scopes into one call, and never splits one scope
  across two calls.

### Multi-tenant applications

A **multi-tenant** application — one codebase, many FlowCatalyst clients —
doesn't hand-write a `client` on every row. Build one `DefinitionSet` per
(application, client) instead:

- `sync.defineApplication("integral")` is the **global** set (no client);
- `.forClient("acme", targetBaseUrl)` produces a set bound to that client.
  The optional `targetBaseUrl` overrides the base URL a path-style
  subscription target resolves against, **for that set only** — tenants
  often have their own host.

```ts
import { sync } from "@flowcatalyst/sdk";

const globalSet = sync
	.defineApplication("integral")
	.withConnections([{ code: "shared-webhook", name: "Shared Webhook" }])
	.build();

const acmeSet = sync
	.defineApplication("integral")
	.forClient("acme", "https://acme.example.com")
	.withConnections([{ code: "acme-webhook", name: "Acme Webhook" }])
	.withSubscriptions([
		{
			code: "acme-orders",
			name: "Acme Orders",
			target: "/webhooks/orders", // resolved against acme.example.com, not the synchronizer default
			connectionCode: "acme-webhook",
			eventTypes: [{ eventTypeCode: "integral:orders:order:created" }],
		},
	])
	.build();
```

**Use `syncGrouped()` — not `syncAll()` — whenever more than one
`DefinitionSet` can target the same application**, most commonly the
per-tenant sets above plus the global one:

```ts
const results = await client
	.definitions()
	.syncGrouped([globalSet, acmeSet, /* ...one set per tenant */], {
		removeUnlisted: true,
	});

results.match(
	// One combined SyncResult per application code, regardless of how many
	// sets (global + N clients) targeted it.
	(byApplication) => console.log("Synced:", byApplication["integral"]),
	(err) => console.error("Sync failed:", err.type, err.message),
);
```

`removeUnlisted` is applied **per (application, client) call**: everything of
that scope a call does not list is deleted. Two consequences:

- **One scope, one call.** `syncGrouped()` **merges** every set you pass for
  the same application code before syncing — so each (application, client)
  scope, and each per-application category (roles, event types, dispatch
  pools, processes, principals), reaches the platform **exactly once**,
  however many sets contributed to it. Calling `sync()` / `syncAll()`
  yourself with two sets for the same scope does **NOT** merge them: under
  `removeUnlisted` the second call deletes what the first just created. This
  is the whole reason `syncGrouped()` exists — always reach for it when more
  than one set can target an application.
- **The same code twice in one scope is a configuration error**, not
  last-one-wins: that category's sync for that scope fails **locally**
  (naming the code and the scope) and nothing is sent for it — sibling
  categories and scopes still sync normally.

A tenant that stops being yielded is simply no longer synced — its rows are
**not** removed, and an empty set doesn't remove them either (a scope with no
definitions produces no call at all). Offboarding a tenant currently means
removing its connections and subscriptions by hand.

---

## Dispatch pools

```ts
const pools: sync.DispatchPoolDefinition[] = [
	{
		code: "fast",
		name: "Fast Webhooks",
		description: "Low-latency webhook delivery (cache lookups, status pings)",
		concurrency: 50,
		rateLimit: 600,
	},
	{
		code: "slow",
		name: "Slow Webhooks",
		description: "High-latency integrations (PDF generation, external APIs)",
		concurrency: 5,
		rateLimit: 60,
	},
];
```

### Why separate pools?

Pools are the platform's **backpressure boundary**. Each pool has its own HTTP client (connection pool) and its own concurrency limiter. If you mix a slow subscription with a fast one in the same pool, a spike on the slow one can starve the fast one.

A good rule: **one pool per nominal latency class**. Segregate cache-speed webhooks from report-speed webhooks even if they both live in your app. Subscriptions pick their pool via `dispatchPoolCode`.

The platform also supports adaptive concurrency (Vegas algorithm) on pools where workload is homogeneous. See the platform's `adaptive-concurrency.md` for preconditions.

### `concurrency` and `rateLimit`

- `concurrency` — hard cap on in-flight requests for this pool. Default 10.
- `rateLimit` — requests per minute ceiling. Default 100.

Both apply simultaneously: a request is dispatched only when both the concurrency slot and a rate-limit token are available.

---

## Principals

```ts
const principals: sync.PrincipalDefinition[] = [
	{
		email: "alice@example.com",
		name: "Alice Johnson",
		roles: ["admin"],
		active: true,
	},
	{
		email: "bob@example.com",
		name: "Bob Smith",
		roles: ["viewer"],
	},
];
```

### Identity model

Principals sync'd this way use the platform's **internal identity provider** (email + password). Users provisioned via OIDC are managed by the IdP's own sync flow and should NOT be listed here — their platform records are reconciled on each OIDC login.

### `roles`

Role references are **short names** only — do NOT include the `<app>:` prefix. Given `applicationCode: "orders"` and `roles: ["admin", "viewer"]`, the principal gets `orders:admin` and `orders:viewer`.

Roles must already exist at sync time (either in the current set, synced earlier in the same batch, or pre-existing on the platform). If a role isn't found, the principal's sync entry fails with `ROLE_NOT_FOUND`.

### `active`

Defaults to `true`. Set to `false` to soft-deactivate a user without deleting the record. Deactivated users can't authenticate but their audit trail is preserved.

---

## Sync orchestration

### Single application

```ts
import { FlowCatalystClient, sync } from "@flowcatalyst/sdk";

const client = new FlowCatalystClient({ /* ... */ });

const definitions = sync
	.defineApplication("orders")
	.withRoles([/* ... */])
	.withEventTypes([/* ... */])
	.build();

const result = await client.definitions().sync(definitions);

result.match(
	(r) => {
		console.log("Synced:", r.applicationCode);
		if (sync.isSynced(r.roles)) {
			console.log(`Roles — created: ${r.roles.created}, updated: ${r.roles.updated}`);
		}
	},
	(err) => {
		// isErr() alone is enough to know the sync failed. If you also want
		// to see what DID sync before/alongside the failure, a
		// "partial_failure" error carries the full SyncResult as `err.partial`
		// — see "What if a sync partially fails?" below.
		console.error("Sync failed:", err.type, err.message);
	},
);
```

### The application code

Pass it directly, or inherit it from `FLOWCATALYST_APP_CODE`:

```ts
const set = sync.defineApplication("orders");        // explicit
const set = sync.defineApplicationFromEnv();         // reads FLOWCATALYST_APP_CODE
```

`defineApplicationFromEnv` throws when the variable is unset or empty, rather than letting a missing code surface later as a request to `/api/applications/undefined/…`.

There is no per-definition application override: the set a definition is built into *is* its application. For several applications, build one set each.

### Multiple applications

```ts
const orders = sync.defineApplication("orders").withRoles([...]).build();
const billing = sync.defineApplication("billing").withEventTypes([...]).build();

const results = await client.definitions().syncAll([orders, billing]);
```

`syncAll` processes sets sequentially. A failure in one set short-circuits the rest; earlier successful sets remain committed. Like `sync()`, `syncAll` does **NOT** merge — see [Multi-tenant applications](#multi-tenant-applications) and `syncGrouped()` for when more than one set can target the same application.

### Options

```ts
await client.definitions().sync(definitions, {
	removeUnlisted: true, // delete SDK-sourced rows not in this submission
	skipPrincipals: true, // don't touch principals this run (stage rollout)
});
```

Per-category skip flags (`skipRoles`, `skipEventTypes`, `skipConnections`, `skipSubscriptions`, `skipDispatchPools`, `skipPrincipals`) force a category to be skipped even if it's present in the set. Useful for staged rollouts or partial syncs.

A subscription's path-style `target` needs a base URL to resolve against
when neither the row nor its `DefinitionSet` supplies one (see
[`target`](#target-absolute-url-or-a-path-resolved-at-sync-time)). Configure
it once, on the synchronizer itself:

```ts
const definitions = client.definitions({
	subscriptionTargetBaseUrl: "https://api.example.com",
});
```

`options` on `client.definitions(...)` only take effect on the **first**
call in a process — the synchronizer instance is created once and cached.

---

## Organising definitions in your codebase

### Colocate definitions with the feature

The whole point of code-first sync is keeping definitions next to the code that uses them. A module layout that works well:

```
src/
  fulfillment/
    events.ts         // EventTypeDefinitions for shipments etc.
    subscriptions.ts  // SubscriptionDefinitions this module owns
    roles.ts          // RoleDefinitions specific to fulfillment
    service.ts
  billing/
    events.ts
    service.ts
  flowcatalyst.ts     // imports + builds the DefinitionSet
```

Then `flowcatalyst.ts`:

```ts
import * as fulfillment from "./fulfillment";
import * as billing from "./billing";
import { sync } from "@flowcatalyst/sdk";

export const appDefinitions = sync
	.defineApplication("orders")
	.withEventTypes([...fulfillment.events, ...billing.events])
	.withSubscriptions([...fulfillment.subscriptions])
	.withRoles([...fulfillment.roles])
	.build();
```

### Running sync

A sync script that can be invoked by CI/CD or a one-off operator command:

```ts
// scripts/sync-flowcatalyst.ts
import { FlowCatalystClient } from "@flowcatalyst/sdk";
import { appDefinitions } from "../src/flowcatalyst";

const client = new FlowCatalystClient({
	baseUrl: process.env.FLOWCATALYST_URL!,
	clientId: process.env.FLOWCATALYST_CLIENT_ID!,
	clientSecret: process.env.FLOWCATALYST_CLIENT_SECRET!,
});

const result = await client.definitions().sync(appDefinitions, {
	removeUnlisted: process.env.FLOWCATALYST_REMOVE_UNLISTED === "true",
});

if (result.isErr()) {
	// isErr() alone is the right check here — it's true for a category/scope
	// failure (see "What if a sync partially fails?") just as much as for a
	// hard transport failure, so this script correctly fails the deploy
	// either way. A "partial_failure" error's `.partial` still shows what
	// DID sync, worth logging before exiting non-zero.
	console.error("Sync failed:", result.error.message);
	if (result.error.type === "partial_failure") {
		console.error("Partial result:", JSON.stringify(result.error.partial, null, 2));
	}
	process.exit(1);
}

console.log("Synced:", JSON.stringify(result.value, null, 2));
```

Wire into `package.json`:

```json
{
	"scripts": {
		"flowcatalyst:sync": "tsx scripts/sync-flowcatalyst.ts"
	}
}
```

---

## FAQ

### What if a sync partially fails?

**`isErr()` always tells you.** If any category, or any (application, client) scope within connections/subscriptions, failed to sync, the whole `sync()` call resolves `Err` — a deploy script doing `if (result.isErr()) process.exit(1)` still catches it. The failure never "reads" as success even though other categories or scopes DID sync successfully.

What differs is how much of the call still ran before it failed:

- **Roles, event types, dispatch pools, principals, processes, scheduled jobs, and the OpenAPI document** are still independent HTTP calls in a fixed order, and a plain HTTP failure in one still short-circuits the categories that would have run after it — a genuine `SdkError` (network, auth, a 5xx from the platform), exactly as it always has.
- **A local configuration error in any category** (most commonly a duplicate code — see [Merging](#multi-tenant-applications)) is caught before the doomed HTTP call and does **not** stop sibling categories: everything else still gets a chance to sync.
- **Connections and subscriptions**, being synced per (application, client) **scope**, go further: one scope's failure — a duplicate code, an unresolvable target, or the HTTP call itself — never stops a **sibling scope** (client B's connections still sync if client A's failed), and a connection-sync failure for one scope specifically skips only *that scope's* subscription sync (its `connectionCode`s can't resolve).

Either way, **nothing successful is thrown away** just because the call reports failure. When `sync()` resolves `Err`, the error carries the complete `SyncResult` — including every category/scope that DID sync — as `error.partial`:

```ts
const result = await client.definitions().sync(definitions, { removeUnlisted: true });

if (result.isErr()) {
	const error = result.error;
	console.error("Sync failed:", error.message);
	if (error.type === "partial_failure") {
		// error.partial is the full SyncResult: read counts for what DID
		// sync, and each category's own `error` for what didn't.
		console.error("Partial result:", JSON.stringify(error.partial, null, 2));
	}
	process.exit(1);
}

console.log("Synced:", result.value.applicationCode);
```

`syncGrouped()`/`syncAll()` apply the same rule to the whole batch: every application/set is synced to completion first (a failure in one never skips a sibling application), and if any of them had a category/scope failure, the aggregate call resolves `Err` with `error.partial` carrying every application's result (`Record<string, SyncResult>` for `syncGrouped`, `SyncResult[]` for `syncAll`) — so the successful applications' results are still there to read. The one exception, matching each method's pre-existing behaviour: a genuine `SdkError` from one set's `sync()` call (not a `partial_failure`) still stops the batch at that point, exactly as it always did.

### Should I sync on every app startup?

**No.** Sync is a CI/CD step, not an application bootstrap step. Running it on every pod start creates unnecessary load and can race between replicas. Run it once per deployment, after migrations, before traffic cutover.

### What about schemas?

SDK sync doesn't upload JSON schemas. Use `client.eventTypes().addSchema(...)` for each schema, or the admin UI. Schemas evolve on their own cadence — conflating them with the event-type list makes both harder to reason about.

### Can I sync across multiple applications from one deployment?

Yes — `syncAll` exists for that. Useful when one codebase owns multiple logical apps (e.g. a monorepo with `orders`, `billing`, `shipping` each mapping to a FlowCatalyst application), **each with its own single `DefinitionSet`**. If more than one set can target the *same* application (typically a multi-tenant app's per-client sets), use `syncGrouped` instead — see [Multi-tenant applications](#multi-tenant-applications).

### How do I know what changed?

The `SyncResult` per category (`roles`, `eventTypes`, `connections`, `subscriptions`, `dispatchPools`, `principals`, `processes`, `scheduledJobs`, `openapi`) has `{ created, updated, deleted, syncedCodes, error? }`. `created + updated + deleted` is the actual change set; `syncedCodes` lists every code that reached the platform (so you can diff against the DB listing if you need a full picture); `error`, when present, names what didn't sync for that category — see [What if a sync partially fails?](#what-if-a-sync-partially-fails).

### Is sync idempotent?

Yes. Submitting the same `DefinitionSet` twice with `removeUnlisted: false` produces zero changes on the second run. This is the intended operator model — sync is declarative.

---

## See also

- [Laravel SDK sync guide](../../laravel-sdk/docs/syncing-definitions.md) — the same patterns in PHP/Laravel.
- Platform admin UI — **Settings → Definitions** shows the full current state of synced vs. admin-managed rows.
