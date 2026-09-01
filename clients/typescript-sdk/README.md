# @flowcatalyst/sdk

Official TypeScript/JavaScript SDK for FlowCatalyst platform.

## Installation

```bash
npm install @flowcatalyst/sdk
# or
yarn add @flowcatalyst/sdk
# or
bun add @flowcatalyst/sdk
```

## Local development with `fc-dev`

For local work you need a FlowCatalyst control plane to talk to.
`fc-dev` is the official one-binary dev environment — bundled
PostgreSQL, platform API, message router, scheduler, and frontend
in a single process.

```bash
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/flowcatalyst/flowcatalyst/main/install.sh | sh

# Windows (PowerShell)
irm https://raw.githubusercontent.com/flowcatalyst/flowcatalyst/main/install.ps1 | iex

fc-dev          # starts API on http://localhost:8080
```

If you publish events via the **outbox pattern**, you also need
`fc-dev outbox` running as a sidecar — it polls your app's
`outbox_messages` table and forwards events to the platform:

```bash
# In your project directory (where this SDK is installed):

# Once: write FC_OUTBOX_DB_URL / FC_OUTBOX_API_URL / FC_OUTBOX_TOKEN
# into ./.env (0600 perms; no secrets on argv or shell history).
fc-dev outbox init

# Daily: reads .env, auto-creates the `outbox_messages` table on
# first run, then polls.
fc-dev outbox poll
```

The SDK's own migration at
[`migrations/postgresql/001_create_outbox_messages.sql`](migrations/postgresql/001_create_outbox_messages.sql)
and `fc-dev outbox poll`'s built-in `CREATE TABLE IF NOT EXISTS`
produce the same schema, so it doesn't matter which one runs first.

Complete reference: [fc-dev CLI docs](https://github.com/flowcatalyst/flowcatalyst-rust/blob/main/docs/developers/fc-dev.md).

## Usage

```typescript
import { FlowCatalystClient } from '@flowcatalyst/sdk';

// Initialize the client
const client = new FlowCatalystClient({
  baseUrl: 'http://localhost:8080',
  apiKey: 'your-api-key', // optional
  timeout: 30000, // optional, defaults to 30s
});

// Get all event types
const { data: eventTypes, error } = await client.getEventTypes();
if (error) {
  console.error('Error:', error);
} else {
  console.log('Event types:', eventTypes);
}

// Create a new event type
const { data: newEventType, error: createError } = await client.createEventType({
  name: 'user.created',
  version: '1.0.0',
  schema: {
    type: 'object',
    properties: {
      userId: { type: 'string' },
      email: { type: 'string' },
    },
  },
});

// Create a subscription
const { data: subscription } = await client.createSubscription({
  eventTypeId: 'event-type-id',
  endpoint: 'https://myapp.com/webhooks',
  status: 'active',
});

// Get dispatch jobs
const { data: jobs } = await client.getDispatchJobs();
```

## API Reference

### FlowCatalystClient

#### Constructor

```typescript
new FlowCatalystClient(config: FlowCatalystConfig)
```

**Config Options:**

- `baseUrl` (required): Base URL of the FlowCatalyst platform
- `apiKey` (optional): API key for authentication
- `timeout` (optional): Request timeout in milliseconds (default: 30000)

#### Event Types

- `getEventTypes()`: Get all event types
- `getEventType(id)`: Get a specific event type
- `createEventType(eventType)`: Create a new event type

#### Subscriptions

- `getSubscriptions()`: Get all subscriptions
- `getSubscription(id)`: Get a specific subscription
- `createSubscription(subscription)`: Create a new subscription

#### Dispatch Jobs

- `getDispatchJobs()`: Get all dispatch jobs
- `getDispatchJob(id)`: Get a specific dispatch job

## Syncing Definitions

Declare your application's roles, permissions, event types, subscriptions,
dispatch pools, and principals in code, then push them to the platform with
a single call:

```typescript
import { FlowCatalystClient, sync } from "@flowcatalyst/sdk";

const definitions = sync
  .defineApplication("orders")
  .withRoles([{ name: "admin", displayName: "Administrator" }])
  .withEventTypes([
    { code: "orders:fulfillment:shipment:shipped", name: "Shipment Shipped" },
  ])
  .build();

await client.definitions().sync(definitions);
```

See **[docs/syncing-definitions.md](./docs/syncing-definitions.md)** for the
full structure guide — how to name roles, the 4-part permission format,
event-type code conventions, subscription modes, dispatch pool sizing, and
principal management.

## Fastify integration

If you're running a Fastify app, `@flowcatalyst/sdk/fastify` is a drop-in
plugin that handles both the browser OIDC flow (redirect + encrypted
session cookie) and Bearer-token validation for API callers — exposing the
same `request.principal` on both. Roles come from the FlowCatalyst access
token; permissions are resolved locally from a catalogue you declare in code.

Optional peer dependencies: `fastify@^5`, `@fastify/cookie`, `jose`. Install
the ones you need:

```bash
pnpm add fastify @fastify/cookie jose
```

```typescript
import Fastify from "fastify";
import {
  flowcatalystAuth,
  defineRbac,
  generateSessionSecret,
} from "@flowcatalyst/sdk/fastify";

const rbac = defineRbac()
  .role("operant:admin").grants("operant:*")
  .role("operant:viewer").grants("operant:read")
  .role("support").grants("ticket:*")
  .build();

const app = Fastify();

await app.register(flowcatalystAuth, {
  baseUrl: "https://platform.example.com",
  clientId: process.env.FC_CLIENT_ID!,
  clientSecret: process.env.FC_CLIENT_SECRET!,
  cookie: { secret: process.env.SESSION_SECRET! }, // 32B base64url
  rbac,
});

// Web route: 302s to /auth/login if no session cookie.
app.get("/dashboard", { preHandler: app.fc.requireSession() }, async (req) => {
  return { hello: req.principal!.name };
});

// API route: 401 JSON if no valid Bearer token.
app.post("/api/orders", { preHandler: app.fc.requireBearer() }, async (req) => {
  if (!req.principal!.hasPermissionTo(["operant:write"])) {
    throw app.httpErrors.forbidden();
  }
  // ...
});

// Either: redirects browsers, 401s machines.
app.get("/api/me", { preHandler: app.fc.requireAuth() }, async (req) => ({
  id: req.principal!.id,
  roles: req.principal!.roles,
  permissions: req.principal!.hasAnyPermissionTo(["operant:read"]),
}));
```

Generate a session secret with `node -e "console.log(require('@flowcatalyst/sdk/fastify').generateSessionSecret())"`.

### Principal helpers

`request.principal` is the same shape for cookie and Bearer callers:

```typescript
principal.id;                                    // "prn_..." or service principal id
principal.scope;                                 // "anchor" | "partner" | "client"
principal.clients;                               // ["clt_abc", ...] or ["*"] for anchors
principal.roles;                                 // ["operant:admin", ...]
principal.applications;                          // ["operant", ...]
principal.mechanism;                             // "session" | "bearer"

principal.hasRole("operant:admin");              // single
principal.hasRoles(["a", "b"]);                  // ALL
principal.hasAnyRole(["a", "b"]);                // ANY
principal.hasPermissionTo(["operant:read"]);     // ALL (wildcard-aware)
principal.hasAnyPermissionTo(["a", "b"]);        // ANY (wildcard-aware)
principal.isAnchor();
principal.canAccessClient("clt_abc");

principal.sessionData;                           // app's bag, persisted in the session
```

Permission wildcards use `:` as separator with `*` as a suffix at any
segment boundary. `operant:*` matches `operant:read` and `operant:reports:export`;
`*` matches anything. Mid-string globs (e.g. `operant:r*`) are not supported.

### Custom post-auth logic

The plugin doesn't expose a special hook — once it has populated
`request.principal`, register a normal Fastify `preHandler` to enrich it
or enforce app-specific checks:

```typescript
app.addHook("preHandler", async (req) => {
  if (!req.principal) return;
  const user = await db.user.upsert({
    where: { fcId: req.principal.id },
    update: { lastSeenAt: new Date() },
    create: { fcId: req.principal.id, email: req.principal.email },
  });
  req.principal.sessionData.localUserId = user.id;
});
```

For cookie sessions, anything you write to `principal.sessionData` from a
post-auth hook is **not** persisted automatically — the session is read
from the store on each request. Persist via `app.fc.logout` / re-issue if
you need a write path; or use the `PgSessionStore` / `RedisSessionStore`
backends if you need mutable server-side state.

### Logout

Default is local-only — clear the session cookie. To also terminate the
OIDC session at the platform, redirect to FlowCatalyst's logout page:

```typescript
app.post("/auth/logout", async (req, reply) => {
  await app.fc.logout(req, reply, {
    redirectTo: "https://platform.example.com/logout",
  });
});
```

### Server-side session storage

The default `CookieSessionStore` puts the encrypted session in the cookie
itself — zero infra, but limited to ~4KB. Swap to Postgres or Redis once
you want to store more per-session data or revoke sessions server-side:

```typescript
import { PgSessionStore, initSessionSchema } from "@flowcatalyst/sdk/fastify";
import pg from "pg";

const pool = new pg.Pool({ connectionString: process.env.DATABASE_URL });
await initSessionSchema(pool);

await app.register(flowcatalystAuth, {
  // ...
  sessionStore: new PgSessionStore({
    executor: pool,
    cookieName: "fc_session",
    cookieOptions: {
      path: "/",
      httpOnly: true,
      secure: true,
      sameSite: "lax",
      maxAge: 60 * 60 * 8,
    },
  }),
});
```

`RedisSessionStore` follows the same shape against an ioredis-compatible
client. With a server-side store, the cookie contains an opaque session id
only and the payload (including any size of `sessionData`) lives in the
backend.

## Using with Effect

If your project uses [Effect](https://effect.website/), the SDK ships an
optional Effect-flavored surface at `@flowcatalyst/sdk/effect/usecase` that
gives the write path (events, dispatch jobs, audit logs) compile-time
invariant guarantees — a use case that doesn't go through `UnitOfWork`
fails to compile, not at runtime. Effect is an optional peer dependency:
the default neverthrow surface is unchanged for everyone else.

See **[docs/effect-usage.md](./docs/effect-usage.md)** for the full
worked example, layer wiring, error handling with `Effect.catchTag`, and
testing with `TestUnitOfWork`.

## TypeScript Support

This SDK is written in TypeScript and provides full type definitions. All API responses are properly typed.

```typescript
import type { EventType, Subscription, DispatchJob } from '@flowcatalyst/sdk';
```

## Error Handling

All API methods return a response object with either `data` or `error`:

```typescript
const { data, error } = await client.getEventTypes();

if (error) {
  // Handle error
  console.error('API Error:', error);
} else {
  // Use data
  console.log('Event types:', data);
}
```

## AI Agent Access (MCP Server)

If you're using an AI coding agent (Claude Code, Cursor, Windsurf, etc.), you can give it read-only access to your FlowCatalyst event types, schemas, and subscriptions via the MCP server. This lets the agent explore your event catalog and generate typed code for you.

### Quick setup (Claude Code)

```bash
claude mcp add flowcatalyst -- npx @flowcatalyst/mcp-server
```

### Quick setup (Cursor / Windsurf / Claude Desktop)

Add to your MCP config file (`.cursor/mcp.json`, Claude Desktop config, etc.):

```json
{
  "mcpServers": {
    "flowcatalyst": {
      "command": "npx",
      "args": ["@flowcatalyst/mcp-server"],
      "env": {
        "FLOWCATALYST_URL": "https://your-instance.flowcatalyst.io",
        "FLOWCATALYST_CLIENT_ID": "svc_abc123",
        "FLOWCATALYST_CLIENT_SECRET": "your_secret"
      }
    }
  }
}
```

You need a service account with the `AI Agent Read-Only` role. See the [MCP server README](../mcp-server/README.md) for full details.

## Development

```bash
# Install dependencies
npm install

# Build
npm run build

# Watch mode
npm run dev

# Type check
npm run lint
```

## License

Copyright (c) 2026 Belac (FlowCatalyst).

[MPL-2.0](https://www.mozilla.org/en-US/MPL/2.0/) — see [LICENSE](LICENSE).
File-level copyleft: changes to the SDK's own files must be shared; your
application that depends on it is not affected.
