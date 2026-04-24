# Syncing Definitions

This guide covers how to sync your application's **roles, event types, subscriptions, dispatch pools, and principals** to the FlowCatalyst platform from TypeScript.

---

## Why sync?

FlowCatalyst is a control-plane for event-driven systems. The platform owns:

- **Roles + permissions** — who can do what
- **Event types** — what kinds of events exist and flow through the pipeline
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
3. **Subscriptions** (depend on event types + dispatch pools)
4. **Dispatch pools**
5. **Principals** (depend on roles)

Each category is an **independent HTTP call**. A failure in one category does NOT roll back earlier successes. If you need all-or-nothing, check the `SyncResult` after and decide your own compensation.

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

## Subscriptions

```ts
const subscriptions: sync.SubscriptionDefinition[] = [
	{
		code: "shipment-tracking",
		name: "Shipment Tracking Webhook",
		description: "Notifies the tracking service of shipment state changes",
		target: "https://tracking.example.com/webhooks/flowcatalyst",
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

### `target` vs. `connectionId`

- **`target`** — a webhook URL. The subscription POSTs directly to it.
- **`connectionId`** — a reference to a pre-configured Connection on the platform. Use this when you want centralised auth (shared signing secret, bearer token) across multiple subscriptions.

Supply one or the other, not both.

### `eventTypes` bindings

Each binding is `{ eventTypeCode, filter? }`. `filter` is an optional expression evaluated against the event payload — if it evaluates false, the delivery is skipped. See the platform docs for filter syntax.

### `mode`

- `IMMEDIATE` (default) — every event for this subscription is delivered independently. Failures don't affect other deliveries.
- `BLOCK_ON_ERROR` — within a message group (per-aggregate ordering), a failure holds subsequent deliveries until the failure is retried successfully or expires. Use this for subscriptions where order matters (e.g. "must-see-shipped-before-delivered"). Has throughput cost under failure.

### `dataOnly`

- `false` (default) — POST body is the full event envelope (metadata + `data`).
- `true` — POST body is just the event `data` field. Use when the receiver is a dumb webhook that doesn't care about FlowCatalyst metadata.

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
		console.error("Sync failed:", err.type, err.message);
	},
);
```

### Multiple applications

```ts
const orders = sync.defineApplication("orders").withRoles([...]).build();
const billing = sync.defineApplication("billing").withEventTypes([...]).build();

const results = await client.definitions().syncAll([orders, billing]);
```

`syncAll` processes sets sequentially. A failure in one set short-circuits the rest; earlier successful sets remain committed.

### Options

```ts
await client.definitions().sync(definitions, {
	removeUnlisted: true, // delete SDK-sourced rows not in this submission
	skipPrincipals: true, // don't touch principals this run (stage rollout)
});
```

Per-category skip flags (`skipRoles`, `skipEventTypes`, `skipSubscriptions`, `skipDispatchPools`, `skipPrincipals`) force a category to be skipped even if it's present in the set. Useful for staged rollouts or partial syncs.

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
	console.error("Sync failed:", result.error);
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

Each category is an independent HTTP call. If roles succeed but subscriptions fail, **roles stay committed**. The `ResultAsync` you get back carries the error from the first failing category; earlier successes are lost (they're inside the abandoned chain).

If you need all-or-nothing semantics, run each category separately and roll back manually on failure — the SDK deliberately doesn't hide partial state from you.

### Should I sync on every app startup?

**No.** Sync is a CI/CD step, not an application bootstrap step. Running it on every pod start creates unnecessary load and can race between replicas. Run it once per deployment, after migrations, before traffic cutover.

### What about schemas?

SDK sync doesn't upload JSON schemas. Use `client.eventTypes().addSchema(...)` for each schema, or the admin UI. Schemas evolve on their own cadence — conflating them with the event-type list makes both harder to reason about.

### Can I sync across multiple applications from one deployment?

Yes — `syncAll` exists for that. Useful when one codebase owns multiple logical apps (e.g. a monorepo with `orders`, `billing`, `shipping` each mapping to a FlowCatalyst application).

### How do I know what changed?

The `SyncResult` per category has `{ created, updated, deleted, syncedCodes }`. `created + updated + deleted` is the actual change set; `syncedCodes` lists every code in the submitted set so you can diff against the DB listing if you need a full picture.

### Is sync idempotent?

Yes. Submitting the same `DefinitionSet` twice with `removeUnlisted: false` produces zero changes on the second run. This is the intended operator model — sync is declarative.

---

## See also

- [Laravel SDK sync guide](../../laravel-sdk/docs/syncing-definitions.md) — the same patterns in PHP/Laravel.
- Platform admin UI — **Settings → Definitions** shows the full current state of synced vs. admin-managed rows.
