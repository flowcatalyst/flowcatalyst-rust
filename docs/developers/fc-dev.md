# `fc-dev` — Developer CLI

`fc-dev` is the all-in-one developer binary for FlowCatalyst. One executable
contains the platform API, message router (with an embedded SQLite queue),
scheduler, stream processor, frontend, and — optionally — an embedded
PostgreSQL 18, the same cluster Go's and Java's `fcdev` use. It also bundles utilities for bootstrapping a fresh project,
resetting the database, running the MCP server, and polling an external
app's outbox.

This page is the **complete CLI reference**. If you're just trying to get
started, the [Quickstart](quickstart.md) covers the five-minute path.

---

## Why is it called `fc-dev` and not `fc`?

We considered `fc` and `fcdev`. `fc-dev` wins by elimination:

- **`fc` is a POSIX shell builtin.** Bash, zsh, and dash all reserve `fc` for
  recalling and editing history (`fc -l`, `fc -e $EDITOR`). Shell builtins
  shadow PATH binaries, so `fc poll …` would silently invoke the history
  builtin and either error or do something unexpected. Users would need
  `command fc …` or an absolute path to reach our binary. Unworkable.
- **`fc.exe` ships with Windows.** It's the legacy *File Compare* tool, in
  `C:\Windows\System32`. Even users who never type it would see their IT
  department flag the collision.
- **`fcdev` (no hyphen) avoids both collisions** but breaks the workspace's
  `fc-` prefix convention (`fc-server`, `fc-router`, `fc-platform-server`,
  `fc-stream-processor`, `fc-outbox-processor`, `fc-mcp-server`). Renaming
  one binary without renaming the family is inconsistent; renaming the
  family is churn without benefit.

So: keep `fc-dev`. The hyphen is load-bearing.

---

## Installation

Latest release from GitHub:

```sh
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/flowcatalyst/flowcatalyst/main/install.sh | sh

# Windows (PowerShell)
irm https://raw.githubusercontent.com/flowcatalyst/flowcatalyst/main/install.ps1 | iex
```

Full per-platform instructions, manual install, cosign verification, and
troubleshooting in [INSTALL.md](../../INSTALL.md).

Verify:

```sh
fc-dev --version
```

Self-update:

```sh
fc-dev upgrade           # download and replace if newer
fc-dev upgrade --check   # just check
fc-dev upgrade --force   # reinstall even if current
```

---

## Subcommand map

| Command | Purpose |
|---|---|
| `fc-dev` (bare) / `fc-dev start` | Run the dev monolith — API, router, scheduler, stream, frontend, embedded PG. |
| `fc-dev init` | Bootstrap a fresh local app: admin user, client, application, service account, `.env`. |
| `fc-dev fresh` | TRUNCATE every FlowCatalyst-owned table in the DB (keeps schema). |
| `fc-dev stop` | Stop the running fcdev — Rust, Go or Java — via the shared PID file (SIGTERM, then SIGKILL past `--timeout`). |
| `fc-dev outbox init` | One-time setup: writes `FC_OUTBOX_*` keys to the project's `.env`. |
| `fc-dev outbox poll` | Standalone outbox poller. Reads config from `.env` / process env; auto-creates the `outbox_messages` table if missing. |
| `fc-dev mcp` | Read-only MCP server for LLM clients (stdio or HTTP). |
| `fc-dev upgrade` | Replace the running binary with the latest GitHub release. |

`fc-dev --help` and `fc-dev <subcommand> --help` are authoritative for
exact flags and env vars — this page summarises the intent.

---

## `fc-dev` / `fc-dev start` — run the dev monolith

```sh
fc-dev                              # default: shared embedded PG 18 on :15432, API :8080, metrics :9090
fc-dev --api-port 3000              # change the API port
FC_API_PORT=3000 fc-dev             # equivalent via env
fc-dev stop                         # stop the running fcdev (Rust, Go or Java)
fc-dev --embedded-db=false \        # connect to external Postgres instead
       --database-url postgresql://localhost:5432/flowcatalyst
```

What it does on first run:

1. Extracts the PostgreSQL 18 binaries bundled in the executable to
   `<userCacheDir>/flowcatalyst/embedded-pg/theseus/<version>/` (one-time),
   and creates the embedded cluster if there is none yet (see
   [The shared embedded cluster](#the-shared-embedded-cluster)).
2. Runs migrations.
3. Seeds built-in roles, the `platform` application, default processes.
4. Starts the API on `http://localhost:8080`, metrics on
   `http://localhost:9090/metrics`, frontend served from the same port as
   the API.

Optional toggles for the embedded outbox processor and scheduler:

```sh
FC_SCHEDULER_ENABLED=true   # default true — polls PENDING jobs
FC_OUTBOX_ENABLED=true      # default false — embedded outbox processor
FC_OUTBOX_DB_URL=...        # if outbox enabled, where its outbox table lives
```

The embedded outbox processor is for the case where your *app* shares
fc-dev's embedded Postgres (i.e. the same database holds both your app
data and the outbox table). When your app is on a separate database
(e.g. Docker PostGIS, see below), use `fc-dev outbox poll` instead.

### Key flags

| Flag / env | Default | Purpose |
|---|---|---|
| `--api-port` / `FC_API_PORT` | `8080` | API + frontend port |
| `--metrics-port` / `FC_METRICS_PORT` | `9090` | Prometheus + `/health` |
| `--database-url` / `FC_DATABASE_URL` | embedded PG URL | Postgres connection |
| `--embedded-db` / `FC_EMBEDDED_DB` | `true` | Start the shared embedded PG vs. use `--database-url` |
| `--embedded-db-path` / `FC_EMBEDDED_DB_PATH` | `<userDataDir>/flowcatalyst/embedded-pg` | Embedded cluster directory (cluster in `<path>/data`) |
| `--embedded-db-port` / `FC_EMBEDDED_DB_PORT` | `15432` | Embedded PG port (`0` = any free port) |
| `--embedded-db-extensions-from` / `FC_EMBEDDED_DB_EXTENSIONS_FROM` | — | A PG 18 tree to copy PostGIS from (tried first) |
| `--embedded-db-reset` / `FC_EMBEDDED_DB_RESET` (old: `--reset-db` / `FC_RESET_DB`) | `false` | Delete the whole `--embedded-db-path` first; the shared default also needs `--confirm-shared-db-reset` |
| `--pid-file` / `FC_DEV_PID_FILE` | `<userDataDir>/flowcatalyst/fcdev.pid` | PID file shared with Go/Java fcdev |
| `--pool-concurrency` / `FC_POOL_CONCURRENCY` | `10` | Default router pool concurrency |
| `--scheduler-enabled` / `FC_SCHEDULER_ENABLED` | `true` | Dispatch scheduler |
| `--outbox-enabled` / `FC_OUTBOX_ENABLED` | `false` | Embedded outbox processor |

### The shared embedded cluster

fc-dev, Go's `fcdev` and Java's `fcdev` use **one** embedded PostgreSQL
cluster, so a developer can switch between them without losing data. Same
directory, port, credentials, database and PostgreSQL major:

| | |
|---|---|
| Cluster | `<embedded-db-path>/data`; default `<userDataDir>/flowcatalyst/embedded-pg/data` |
| `<userDataDir>` | `$XDG_DATA_HOME`, else macOS `~/Library/Application Support`, Windows `%AppData%`, Linux `$XDG_CONFIG_HOME` or `~/.config` (Go's `os.UserConfigDir`) |
| Port | `15432` |
| URL | `postgresql://postgres:postgres@localhost:15432/flowcatalyst?sslmode=disable` |
| Major | PostgreSQL **18** (pinned; `POSTGRESQL_VERSION` in `.cargo/config.toml`) |
| Keys | `<userDataDir>/flowcatalyst/app-key` (`FLOWCATALYST_APP_KEY`, created if absent) and `jwt-signing-key.pem` (used when present) |
| PID file | `<userDataDir>/flowcatalyst/fcdev.pid` |

Each binary runs the cluster with its own server binaries, all PG 18:
Go's under `<userCacheDir>/flowcatalyst/embedded-pg/bin`, Java's under
`…/embedded-pg/PG-<md5>`, fc-dev's under `…/embedded-pg/theseus/<version>`
(extracted from the fc-dev executable, so a machine without Go or Java
still gets a working cluster; `initdb` runs with Go's arguments). A data
directory of another major is refused with Go's message: upgrade it with
Go's or Java's `fcdev db upgrade`.

**One at a time.** Only one fcdev may serve the cluster. fc-dev refuses to
start while the PID file names a live fcdev, while the cluster's
`postmaster.pid` names a live server, or while Java's `epg-lock` is held,
and says which process it is. Stop the other one first:

```sh
fc-dev stop          # or `fcdev stop` with the Go/Java binary — same PID file
fc-dev
```

While fc-dev runs it holds the PID file and Java's `epg-lock`, so Go's and
Java's `fcdev stop` stop it, and a Java `fcdev` started meanwhile refuses
at once. (Go's `fcdev start` does not check; it fails when its Postgres
cannot start on the busy cluster, without touching it.) `fc-dev init` and
`fc-dev fresh` use the running server when there is one, whichever binary
started it.

**PostGIS.** PostGIS lives only in the tree it was copied into (Go's, per
`flowcatalyst-go/docs/embedded-postgres-postgis.md`). Before starting, fc-dev
copies the PostGIS family (`postgis*`, `rtpostgis*`, `address_standardizer*`
modules, `.control` and `.sql` files) into its own tree from the first
source that has it: `--embedded-db-extensions-from`, Go's tree, Java's
trees, Homebrew (`/opt/homebrew/opt/postgis`, `/usr/local/opt/postgis`),
PGDG (`/usr/lib/postgresql/18`, `/usr/pgsql-18`). It never overwrites a
file. After starting it checks every database's installed extensions and
logs an error naming the database when its tree still lacks one (the
platform's own database needs none, so fc-dev keeps running). Java does
the same; fc-dev runs its own tree rather than Go's so that it never
depends on another tool's cache being present or of a matching version.

**Resetting.** `--embedded-db-reset` deletes the whole
`--embedded-db-path`, as Go does. For the shared default it also needs
`--confirm-shared-db-reset`, because that cluster holds every binary's data.

**Differences from Go.** With `--embedded-db` on (the default) fc-dev uses
the embedded cluster even when `FC_DATABASE_URL` is set (this repository's
`.env.development` sets it); pass `--embedded-db=false` to use the URL. Go
uses the URL whenever it is set. fc-dev has no `db upgrade`.

**Migrations on a database Go and Java migrated.** Each binary keeps its
own tracker beside the others (Go `goose_db_version`, Java
`flyway_schema_history`, fc-dev `_schema_migrations`). On first contact
fc-dev creates its tracker and backfills every migration whose effects are
already visible, then applies only the rest. Verified on a copy of a
Go/Java-migrated dev cluster (goose 54, Flyway V17): fc-dev applied
037 and 038 and nothing else. Its schema changes there:

| DDL | Verdict for Go / Java |
|---|---|
| `CREATE TABLE _schema_migrations` | fc-dev's tracker; neither reads it |
| `ALTER TABLE aud_logs ALTER COLUMN entity_id TYPE VARCHAR(100)` (was 17) | Java's V18 is the same statement; a wider column accepts everything Go writes |
| `fn_functions_runtime_check` re-created allowing `COMPONENT` besides `JVM`, `WASM` | widening only; rows Java writes still pass |
| `ALTER TABLE fn_hosts ADD COLUMN IF NOT EXISTS runtimes JSONB` | nullable; Java's inserts leave it NULL; no Go/Java migration adds the column |
| `CREATE TABLE/INDEX IF NOT EXISTS queue_messages…`, monthly partitions `CREATE TABLE IF NOT EXISTS … PARTITION OF` | no-ops there (already present) |

No existing row is deleted. Seeding and start-up writes: built-in roles
are reset to fc-dev's catalogue, which is Go's (Java's V17 renamed
`platform:admin:config:update` to `…:config:manage`; whichever binary
starts last wins, as between Go and Java today); 58 platform event types Go
emits but had not catalogued are added, existing ones keep their values;
plaintext service-account webhook secrets are encrypted with the shared
`app-key` (Go re-encrypts them the same way on read, and decrypts the
result); the platform application's OpenAPI document becomes fc-dev's (the
previous one is kept as `ARCHIVED`). The developer-portal auto-sync also
records one `platform:admin:eventtype:updated` event per platform event
type on every start. `fc-dev init` only adds rows.

The cluster fc-dev used before (`<userCacheDir>/flowcatalyst-dev/pgdata`,
port 15432, password `flowcatalyst`) is no longer used; fc-dev logs its path
while it exists. Delete it when you no longer need its data.

---

## `fc-dev init` — bootstrap a fresh app

Interactive bootstrap that produces everything a new application needs to
publish events: anchor admin (if none exists yet), Default Client,
Application, Service Account, `client_credentials` OAuth client, and a
`.env` written into the target project root.

```sh
cd ~/code/my-new-app
fc-dev init
```

Interactive prompts cover the application code, name, type, optional
description, and admin email/password (only prompted if no anchor admin
exists yet). Pass flags to skip prompts:

```sh
fc-dev init --yes \
  --code orders \
  --name "Orders" \
  --app-type APPLICATION \
  --admin-email me@example.com \
  --admin-password 's0me-pw'
```

Writes `{root}/.env` with `FLOWCATALYST_BASE_URL`, `FLOWCATALYST_APP_CODE`,
`FLOWCATALYST_CLIENT_ID`, `FLOWCATALYST_CLIENT_SECRET`. Existing keys are
updated in place; new keys are appended.

Idempotent. Re-running picks up existing rows where they exist and only
creates what's missing.

---

## `fc-dev fresh` — truncate every FC table

Reset the local database without re-installing or re-migrating. TRUNCATEs
every table whose name starts with `iam_`, `msg_`, `aud_`, `tnt_`,
`oauth_`, `webauthn_`, `outbox_`, or `fc_`. Preserves the schema and the
`_schema_migrations` tracker.

```sh
fc-dev fresh                # interactive confirmation
fc-dev fresh --yes          # skip confirmation (for scripts)
fc-dev fresh --embedded-db=false \
             --database-url postgresql://localhost:5432/flowcatalyst
```

Built-in roles, the `platform` application, and default processes get
re-seeded the next time `fc-dev` starts (those seeders are idempotent).

Local development only — there is no remote mode.

---

## `fc-dev outbox` — standalone outbox poller

Poll an external app's `outbox_messages` Postgres table and forward
Events / DispatchJobs / AuditLogs to a FlowCatalyst platform API.

**Use this when your app's database can't be (or shouldn't be) fc-dev's
embedded Postgres.** The headline example: an app that uses PostGIS, which
isn't included in the embedded Postgres bundle — so the app runs against
Docker PostGIS, but you still want fc-dev to play the role of "the
outbox processor sidecar."

`outbox` is split into two subcommands so secrets never appear on the
command line: `init` writes config into the project's `.env`, then `poll`
reads from there. Daily use is `fc-dev outbox poll` with no flags.

The poller boots **nothing else**: no embedded Postgres, no platform API,
no queue, no scheduler. Just the outbox processor and an HTTP client.

### `outbox init` — one-time setup

```sh
cd ~/code/my-postgis-app
fc-dev outbox init
# prompts for:
#   Postgres URL (e.g. postgres://user:pass@localhost:5433/myapp)
#   Platform API URL [http://localhost:8080]
#   Bearer token
# writes ./.env (0600 perms on Unix) with:
#   FC_OUTBOX_DB_URL
#   FC_OUTBOX_API_URL
#   FC_OUTBOX_TOKEN
```

Idempotent — re-running with the same values is a no-op. Pass any field
as a flag to skip its prompt (`--db-url=...`), or `--yes` to fail fast if
any required value is missing (for scripted setup, e.g. CI).

### `outbox poll` — daily use

```sh
cd ~/code/my-postgis-app
fc-dev outbox poll
# reads .env → connects → ensures outbox_messages table exists → polls
```

On startup, the poller runs `CREATE TABLE IF NOT EXISTS outbox_messages
(...)` plus the partial pending/stuck indexes. The schema matches the
one the FlowCatalyst SDKs ship in their own migrations, so apps that
already migrated their database are unaffected. Opt out with
`--skip-bootstrap` (or `FC_OUTBOX_SKIP_BOOTSTRAP=true`) if you'd rather
manage the schema entirely from the app side.

Flags (every one also accepts the matching env var; flags override env):

| Flag | Env / default | Purpose |
|---|---|---|
| `--db-url` | `FC_OUTBOX_DB_URL` *(required)* | Postgres URL of the app DB owning `outbox_messages` |
| `--api-url` | `FC_OUTBOX_API_URL` / `http://localhost:8080` | Platform API base URL to forward to |
| `--token` | `FC_OUTBOX_TOKEN` | Bearer token for the platform API (required in practice) |
| `--poll-interval-ms` | `FC_OUTBOX_POLL_INTERVAL_MS` / `1000` | Poll cadence |
| `--max-connections` | `FC_OUTBOX_MAX_CONNECTIONS` / `5` | DB pool size |
| `--skip-bootstrap` | `FC_OUTBOX_SKIP_BOOTSTRAP` / `false` | Don't auto-create the table |

The DB password is redacted before logging.

### Where the token comes from

The platform API rejects unauthenticated batch ingest. Mint a bearer
token from the target platform's service account
(`client_credentials` grant). `fc-dev init` produces such a service
account; exchange its `clientId` + `clientSecret` for a token via the
target's `POST /oauth/token`:

```sh
curl -s -X POST http://localhost:8080/oauth/token \
  -H 'Content-Type: application/x-www-form-urlencoded' \
  -d "grant_type=client_credentials&client_id=$FLOWCATALYST_CLIENT_ID&client_secret=$FLOWCATALYST_CLIENT_SECRET" \
  | jq -r .access_token
```

Tokens are short-lived. In practice you'll either pin them in `.env` and
refresh by re-running the curl + `fc-dev outbox init --token=<new>`, or
wrap `outbox poll` in a script that refreshes on startup.

### Why a separate process

This is the same shape as production: in prod, `fc-outbox-processor` runs
as a sidecar per consumer application — one process, one DB, one token.
`fc-dev outbox poll` is the same shape, locally. You can start and stop
it per project without touching the running `fc-dev` instance.

The architecture deep-dive is at
[../architecture/outbox-processor.md](../architecture/outbox-processor.md).

---

## `fc-dev mcp` — MCP server

Read-only Model Context Protocol server. Lets LLM clients query event
types and subscriptions for code-aware completions.

```sh
fc-dev mcp                       # stdio (for editor integrations)
fc-dev mcp --http                # HTTP server on :3100
fc-dev mcp --http --bind 0.0.0.0:3100
```

Reads `FLOWCATALYST_URL`, `FLOWCATALYST_CLIENT_ID`, and
`FLOWCATALYST_CLIENT_SECRET` from the environment. `fc-dev init` writes
these into your project's `.env`.

For the deep-dive on what MCP can do here, see
[../architecture/shared-crates.md](../architecture/shared-crates.md#fc-mcp).

---

## `fc-dev upgrade` — self-update

Replaces the running binary with the latest GitHub release of `fc-dev`.

```sh
fc-dev upgrade
fc-dev upgrade --check
fc-dev upgrade --force
```

Filters the release feed for `fc-dev/v*` tags (the SDK splits in this
repo also publish tags, so plain "latest" isn't safe). Atomic
rename-then-replace on Windows.

---

## Recipes

### Recipe: vanilla local development

```sh
fc-dev                  # one process, embedded everything
# open http://localhost:8080
```

### Recipe: bootstrap a new app

```sh
cd ~/code/my-new-app
fc-dev init             # interactive — produces .env, admin, app, SA
```

Your SDK reads the resulting `.env` automatically.

### Recipe: reset a wedged local state

```sh
fc-dev fresh            # drops + rebuilds the platform schema (asks first)
# or, wiping the whole shared cluster — every database Go/Java/Rust keep there:
fc-dev --embedded-db-reset --confirm-shared-db-reset
```

### Recipe: app with PostGIS (or any other extension fc-dev can't bundle)

```sh
# 1. Run your DB out-of-band — e.g. PostGIS in Docker
docker run --name pg-postgis -p 5433:5432 \
  -e POSTGRES_PASSWORD=dev -e POSTGRES_DB=myapp \
  postgis/postgis:16-3.4

# 2. Run fc-dev as normal (uses the shared embedded PG for the platform DB)
fc-dev

# 3. One-time: from your project directory, write FC_OUTBOX_* into .env
cd ~/code/my-postgis-app
fc-dev outbox init
#   Postgres URL: postgres://postgres:dev@localhost:5433/myapp
#   Platform API URL [http://localhost:8080]:
#   Bearer token: <paste from /oauth/token>

# 4. Daily: poll. Auto-creates `outbox_messages` on first run.
fc-dev outbox poll
```

Why not just point fc-dev at the PostGIS instance with
`--embedded-db=false`? Because fc-dev's own platform tables don't need
PostGIS — only your app does. Splitting them keeps fc-dev's embedded PG
in play (zero-config restart) and matches the production topology where
the platform's DB and the app's DB are separate.

### Recipe: connect fc-dev to a long-running Postgres you already use

If you keep one Postgres around for everything (e.g. your team's
shared dev DB), skip the embedded PG:

```sh
FC_DATABASE_URL=postgresql://user:pass@db.local:5432/flowcatalyst \
  fc-dev --embedded-db=false
```

### Recipe: drive fc-dev from an editor / agent

```sh
fc-dev mcp                    # stdio — Cursor, Claude Code, etc.
fc-dev mcp --http             # HTTP — generic MCP clients
```

---

## Environment variables (summary)

The full list lives in `fc-dev <subcommand> --help`. The most common:

| Variable | Default | Used by |
|---|---|---|
| `FC_API_PORT` | `8080` | `start` |
| `FC_METRICS_PORT` | `9090` | `start` |
| `FC_DATABASE_URL` | (embedded PG) | `start`, `init`, `fresh` |
| `FC_EMBEDDED_DB` | `true` | `start`, `init`, `fresh` |
| `FC_EMBEDDED_DB_PATH` | `<userDataDir>/flowcatalyst/embedded-pg` | `start`, `init`, `fresh` |
| `FC_EMBEDDED_DB_PORT` | `15432` | `start`, `init`, `fresh` |
| `FC_EMBEDDED_DB_EXTENSIONS_FROM` | — | `start`, `init`, `fresh` |
| `FC_EMBEDDED_DB_RESET` (old: `FC_RESET_DB`) | `false` | `start` |
| `FC_CONFIRM_SHARED_DB_RESET` | `false` | `start` |
| `FC_DEV_PID_FILE` | `<userDataDir>/flowcatalyst/fcdev.pid` | `start`, `stop` |
| `FLOWCATALYST_APP_KEY` | `<userDataDir>/flowcatalyst/app-key` (embedded); a fixed dev key otherwise | all |
| `FC_SCHEDULER_ENABLED` | `true` | `start` |
| `FC_OUTBOX_ENABLED` | `false` | `start` (embedded outbox) |
| `FC_OUTBOX_DB_URL` | — | `start`, `outbox poll`, `outbox init` |
| `FC_OUTBOX_API_URL` | `http://localhost:8080` | `outbox poll`, `outbox init` |
| `FC_OUTBOX_TOKEN` | — | `outbox poll`, `outbox init` |
| `FC_OUTBOX_POLL_INTERVAL_MS` | `1000` | `start`, `outbox poll` |
| `FC_OUTBOX_SKIP_BOOTSTRAP` | `false` | `outbox poll` — opt out of `CREATE TABLE IF NOT EXISTS` |
| `FC_DEV_MODE` | `true` (auto-set) | global behaviour gate |
| `FC_STATIC_DIR` | — | `start` — serve frontend from filesystem (live reload) |
| `FC_JWT_PRIVATE_KEY_PATH` / `FC_JWT_PUBLIC_KEY_PATH` / `FC_JWT_SIGNING_KEY_PATH` | `<userDataDir>/flowcatalyst/jwt-signing-key.pem` when it exists (Go/Java's), else `<userCacheDir>/flowcatalyst-dev/jwt-keys/` | `start` |
| `FC_WEBAUTHN_RP_ID` | `localhost` | `start` |
| `FC_WEBAUTHN_ORIGINS` | `http://localhost:5173,http://localhost:8080` | `start` |

`fc-dev` honours a project-local `.env.development` (then `.env`) on
startup — useful for pinning per-project overrides without exporting in
your shell.

---

## Troubleshooting

| Symptom | Most likely cause |
|---|---|
| `fc-dev: command not found` after install | Shell hasn't reloaded — see [INSTALL.md](../../INSTALL.md#troubleshooting) |
| First run slow | The bundled PG binaries unpack into `<userCacheDir>/flowcatalyst/embedded-pg/theseus/` (~45 MB, once) |
| "… is already running (pid N …)" / "the embedded cluster … is already being served" | Go's, Java's or another fc-dev is using the shared cluster; `fc-dev stop` (or `fcdev stop`), then start again |
| "the embedded cluster is locked by Java fcdev" | Java's fcdev holds the cluster's `epg-lock`; stop it first |
| "data dir … is PG17 but this fc-dev embeds PG18" | The cluster is another major; `fcdev db upgrade` (Go/Java) or `--embedded-db-reset` |
| Error log "Database uses extension(s) fc-dev's Postgres tree does not provide" | A database uses PostGIS but no source tree has it; `brew install postgis` or `--embedded-db-extensions-from <PG 18 tree>` |
| `error: failed to run custom build command for postgresql_embedded` | Building from source behind a proxy; use the release binary instead |
| Outbox poll prints "no token" warning | Set `FC_OUTBOX_TOKEN` — batch endpoints reject unauthenticated requests |
| Outbox poll connects but no items flow | Confirm your app writes to a table the processor recognises (default `outbox_messages`) and at least one row has `status = 1` (PENDING) |
| Embedded PG fails to start | The last lines of `<userCacheDir>/flowcatalyst/embedded-pg/theseus/postgres.log` are in the error |

---

## Related docs

- [Quickstart](quickstart.md) — five-minute getting-started.
- [Concepts](concepts.md) — events, subscriptions, pools, dispatch modes.
- [Publishing events](publishing-events.md) — the outbox pattern and batch API.
- [Outbox processor architecture](../architecture/outbox-processor.md) —
  what `outbox poll` actually does, in depth.
- [INSTALL.md](../../INSTALL.md) — install paths, cosign verification,
  troubleshooting.
- [Topologies](../operations/topologies.md) — production binaries and
  deployment shapes.
