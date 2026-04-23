# Multi-Outbox for fc-dev (Deferred)

Dev-mode only. Lets a developer register N outbox sources via the admin UI and
have fc-dev supervise one outbox processor per source.

## Current state

`bin/fc-dev/src/main.rs` wires **one** outbox processor from CLI/env flags
(`FC_OUTBOX_*`). Only the Postgres path is implemented; sqlite/mongo are
advertised in the arg enum but not built. To process a second outbox a dev
must rebuild main.rs.

Relevant code:

- `bin/fc-dev/src/main.rs:74–103` — CLI args for the single outbox
- `bin/fc-dev/src/main.rs:348–364` — pool construction
- `bin/fc-dev/src/main.rs:415–457` — processor spawn + shutdown wiring

## Goal

Developers running fc-dev locally can:

1. Open the admin UI, add outbox sources (Postgres / Mongo / SQLite, URL, table,
   poll interval).
2. Restart fc-dev (or hit a "restart" button per source) and see one processor
   running per enabled source, each reporting health + lag.

Production continues to use dedicated `fc-outbox-processor` binaries; this
feature does not ship there.

## Shape

### 1. Schema

New migration `NNN_dev_outbox_configs.sql`:

```sql
CREATE TABLE dev_outbox_configs (
    id                 VARCHAR(17) PRIMARY KEY,
    name               VARCHAR(128) NOT NULL UNIQUE,
    enabled            BOOLEAN NOT NULL DEFAULT true,
    driver             VARCHAR(16) NOT NULL,   -- 'postgres' | 'mongo' | 'sqlite'
    connection_url     TEXT NOT NULL,
    table_or_collection VARCHAR(128) NOT NULL DEFAULT 'outbox_messages',
    poll_interval_ms   INTEGER NOT NULL DEFAULT 1000,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

Dev-only: gate the migration behind a feature flag so it doesn't run in prod.
Connection strings in cleartext are acceptable for dev; any non-dev use must
route them through `fc-secrets` at spawn time.

### 2. Backend

`crates/fc-platform/src/dev_outbox/`

- `entity.rs` — `DevOutboxConfig` aggregate implementing `Aggregate` + `HasId`.
- `repository.rs` — `DevOutboxConfigRepository` with standard CRUD + `find_enabled()`.
- `operations/` — Create / Update / Delete / Toggle use cases. Each commits
  via `UnitOfWork` with a `DevOutboxConfigChanged` domain event.
- `api.rs` — `/bff/dev-outbox-configs` CRUD routes + a `POST /:id/test` handler
  that opens a throwaway pool and runs `SELECT 1` / equivalent Mongo ping.

Gate the whole module behind `cfg(feature = "dev-tools")` so prod builds drop
it entirely.

### 3. Frontend

- `pages/dev/OutboxConfigListPage.vue` — list with enabled toggle, driver tag,
  last-error column. Uses the cross-page `useListState` + `useReturnTo` already
  adopted by the other admin lists.
- `pages/dev/OutboxConfigDetailPage.vue` — form + **Test Connection** button +
  **Restart Processor** button.
- Nav entry under a "Development" section; hide when the backend `dev-tools`
  feature is off.

### 4. Supervisor

New `crates/fc-dev-support/src/outbox_supervisor.rs` (or a module inside
`bin/fc-dev/`). Replace the single `outbox_handle` block in main.rs with:

```rust
let supervisor = OutboxSupervisor::new(
    pg_pool.clone(),
    auth_services.auth.clone(),
    format!("http://localhost:{}", args.api_port),
).await?;
supervisor.start_all_enabled().await?;
```

Supervisor responsibilities:

- Load all enabled rows from `dev_outbox_configs`, build the right
  `OutboxRepository` impl per driver, spawn an `EnhancedOutboxProcessor`
  per row, keep the `JoinHandle` in a `HashMap<String, ProcessorHandle>`
  keyed by config ID.
- Generate one internal service token at supervisor start (same pattern as
  today) and pass it to every processor.
- Expose `restart(id)` / `stop(id)` / `start(id)` methods the REST handlers
  can call. No live-watch on the table — "Restart Processor" button + full
  fc-dev restart is the v1 contract.
- Per-processor status surface (`status(id) -> { running, last_polled_at,
  last_error, items_processed }`) rendered on the detail page.

### 5. Metrics

Per processor, tag existing outbox metrics with `outbox_config_id` / `name`.
Grafana queries already use `outbox_config_id` → no schema change, just a
consistent label.

## Tradeoff recap

- **Load-on-startup + restart button** (proposed): ~300 LOC of supervisor, 0
  live-watch complexity, no silent-stop failure mode.
- **Full live-watch**: ~600 LOC, config can change without restart, but a
  broken save can leave a processor stopped with no obvious signal until
  somebody notices events aren't draining. Not worth it for dev.

## Out of scope

- Authz on `/bff/dev-outbox-configs/*` — fc-dev is single-user, reuse the
  admin auth layer unchanged.
- Prod support — this lives in `dev-tools`; prod keeps `fc-outbox-processor`.
- Historical activity per config — health snapshot + last-error is enough.

## Estimated effort

- Migration + entity + repo + use cases + routes: ~1 day.
- Frontend list/detail + test-connection + restart wiring: ~0.5 day.
- Supervisor + metrics labelling: ~0.5 day.
- Total ~2 days including basic tests.
