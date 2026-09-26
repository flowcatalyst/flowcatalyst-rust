# Quickstart

Get FlowCatalyst running locally in five minutes. After this, you'll have a working platform you can publish events to, configure subscriptions in, and watch dispatch jobs flow through.

This is the developer-side view. For deploying FlowCatalyst yourself in production, see [../operations/](../operations/).

---

## Install fc-dev

`fc-dev` is the all-in-one development binary. It bundles an embedded PostgreSQL, the platform API, message router with embedded SQLite queue, scheduler, stream processor, and frontend — one process, no external dependencies.

### macOS / Linux

```sh
curl -fsSL https://raw.githubusercontent.com/flowcatalyst/flowcatalyst/main/install.sh | sh
```

### Windows (PowerShell)

```powershell
irm https://raw.githubusercontent.com/flowcatalyst/flowcatalyst/main/install.ps1 | iex
```

Full per-platform instructions in [INSTALL.md](../../INSTALL.md).

Verify:

```sh
fc-dev --version
```

---

## First run

```sh
fc-dev
```

That's it. On first run, fc-dev:

1. Starts the embedded PostgreSQL 18 cluster it shares with Go's and Java's `fcdev` (`~/Library/Application Support/flowcatalyst/embedded-pg` on macOS, port 15432), creating it on a machine that has none. Only one of the three runs at a time: `fc-dev stop` (or `fcdev stop`) stops whichever is running.
2. Runs all migrations.
3. Seeds default data: an `admin@flowcatalyst.local` user, built-in roles, the platform application.
4. Starts the API server on `http://localhost:8080`.
5. Starts the metrics server on `http://localhost:9090`.

Open `http://localhost:8080` in a browser. Log in with `admin@flowcatalyst.local` (first-run password is logged to the console; check the startup output for `Seeded admin user with password: …`).

---

## What you're looking at

The frontend at `http://localhost:8080` is the platform admin UI. From here you can:

- **Events** — see incoming events, drill into any event's payload and dispatch jobs.
- **Subscriptions** — configure which events fan out to which webhook endpoints.
- **Connections** — define webhook endpoints (URL, auth, signing).
- **Dispatch pools** — set concurrency and rate-limit policies per workload.
- **Dispatch jobs** — see deliveries in flight, completed, failed; retry, cancel, ignore.
- **Event types** — declare your event schema versions.
- **Identities** — manage principals, roles, applications.
- **Scheduled jobs** — cron-triggered events / webhooks.

Quick orientation: the dashboard at `/bff/dashboard` shows pipeline-wide throughput. The dispatch jobs list at `/dispatch-jobs` is where you'll spend most debugging time.

---

## Publishing your first event

The simplest path — POST directly to the platform API:

```sh
# Get a session token (use the admin password from the console output)
TOKEN=$(curl -s -X POST http://localhost:8080/auth/login \
  -H 'Content-Type: application/json' \
  -d '{"email":"admin@flowcatalyst.local","password":"<password-from-console>"}' \
  | jq -r .accessToken)

# Publish an event
curl -X POST http://localhost:8080/api/events/batch \
  -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{
    "items": [
      {
        "type": "demo.order.created",
        "source": "/demo/orders",
        "data": { "orderId": "ord_001", "total": 99.99 }
      }
    ]
  }'
```

You should see the event appear in the Events list within a second. Without a subscription configured, nothing else happens — the event is stored but no dispatch jobs are created.

---

## Wire up a webhook receiver

In a separate terminal, start a webhook receiver. Anything that accepts a POST works; here's the easiest with `webhook-test`-style or a one-liner:

```sh
# Listens on :3001, echoes everything to stdout
python3 -c '
import http.server, json, sys
class H(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        body = self.rfile.read(int(self.headers["Content-Length"]))
        print("---")
        print("Headers:", dict(self.headers))
        print("Body:", body.decode())
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(b"{\"ack\":true}")
http.server.HTTPServer(("127.0.0.1", 3001), H).serve_forever()
'
```

In the platform UI:

1. **Create a connection.** `/connections` → "New connection". Set the URL to `http://localhost:3001`, leave auth blank (or set HMAC and copy the signing secret for the receiver).
2. **Create a subscription.** `/subscriptions` → "New subscription". Pick the event type `demo.order.created` (the platform discovered it from the event you just posted), pick the connection from step 1, leave dispatch mode as `IMMEDIATE`.
3. **Publish another event** (using the curl from above).

Within ~1 second:

1. Stream processor's event_fan_out matches the event to your new subscription.
2. A row appears in `msg_dispatch_jobs` with status `PENDING`.
3. Scheduler polls, publishes to the SQLite queue, status → `QUEUED`.
4. Router consumes from the queue, calls the platform's dispatch-process endpoint.
5. Platform loads the dispatch job, POSTs to `http://localhost:3001`.
6. Your receiver prints the body, returns `{"ack":true}`.
7. Status → `COMPLETED`.

You'll see the dispatch job appear in `/dispatch-jobs` with status COMPLETED and an attempt record showing the HTTP 200 response.

---

## The end-to-end flow you just exercised

```
1. POST /api/events/batch
        │
        ▼
2. INSERT INTO msg_events
        │
        │  (stream processor's event_fan_out)
        ▼
3. INSERT INTO msg_dispatch_jobs (PENDING)
        │
        │  (scheduler polls)
        ▼
4. Publish to SQLite queue, mark QUEUED
        │
        ▼
5. Router consumes, calls /api/dispatch/process
        │
        ▼
6. Platform POSTs your webhook (localhost:3001)
        │
        ▼
7. Receiver returns 200 → COMPLETED
```

In production, the SQLite queue is replaced with SQS, embedded Postgres with RDS, fc-dev with fc-server — but the flow is identical.

---

## What to read next

- [concepts.md](concepts.md) — event types, subscriptions, dispatch modes, message groups, what each piece does.
- [publishing-events.md](publishing-events.md) — proper publishing patterns: the outbox, batch endpoints, schema management.
- [receiving-webhooks.md](receiving-webhooks.md) — webhook contract: HMAC signing, ack/nack body, retry semantics.
- [subscriptions-and-pools.md](subscriptions-and-pools.md) — subscriptions, dispatch modes, pool configuration.
- [scheduled-jobs.md](scheduled-jobs.md) — cron-driven workflows.
- [debugging.md](debugging.md) — when something goes wrong, where to look.

---

## Common fc-dev commands

```sh
# Run normally
fc-dev

# Pick a port (default 8080)
FC_API_PORT=3000 fc-dev

# Bootstrap a fresh app (admin + client + application + service account + .env)
fc-dev init

# TRUNCATE every FC table (keeps schema; built-in roles re-seed on next start)
fc-dev fresh

# Stop the running fcdev (Rust, Go or Java — they share one PID file)
fc-dev stop

# Wipe the entire shared embedded cluster (every database in it) and start over
fc-dev --embedded-db-reset --confirm-shared-db-reset

# Connect to an existing Postgres instead of the embedded one
fc-dev --embedded-db=false --database-url postgresql://localhost:5432/flowcatalyst

# Sidecar an external app's outbox (e.g. PostGIS in Docker) into a local fc-dev:
#   one-time setup writes FC_OUTBOX_* to ./.env (0600 perms)
fc-dev outbox init
#   then run the poller — reads .env, auto-creates the outbox table
fc-dev outbox poll

# Run the MCP server (read-only access for LLM clients)
fc-dev mcp           # stdio
fc-dev mcp --http    # HTTP server on :3100

# Upgrade to the latest release
fc-dev upgrade           # download and install if newer
fc-dev upgrade --check   # just check
```

Full reference (every subcommand, every flag, every env var):
**[fc-dev.md](fc-dev.md)**.

---

## SDKs

For application integration, use the SDK appropriate to your language:

- **TypeScript / JavaScript** — `npm install @flowcatalyst/sdk`. See [clients/typescript-sdk/README.md](../../clients/typescript-sdk/README.md).
- **Laravel / PHP** — `composer require flowcatalyst/laravel-sdk`. See [clients/laravel-sdk/README.md](../../clients/laravel-sdk/README.md).
- **Rust** — `fc-sdk` crate in this workspace. See [../architecture/shared-crates.md#fc-sdk](../architecture/shared-crates.md#fc-sdk---application-sdk).

The SDKs cover the **outbox pattern** for atomic event publishing, **definition syncing** for declaring your application's event types and roles, and **webhook signature verification** for receiving.
