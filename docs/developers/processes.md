# Processes

Process documentation is a free-form workflow record stored in the
platform. The `body` field holds diagram source verbatim (typically
Mermaid) and the platform renders it client-side and exposes
SVG/PNG export. There is no validation, parsing, or interpretation of the
body — it is opaque text storage with metadata.

## Why this exists

Apps document their internal workflows — "fulfilment created → reactive
aggregate creates shipment if geocoded → locations enriched via dispatch
job" — and the platform should be the single place where those diagrams
live alongside the event types and subscriptions they describe. The
alternative (READMEs, Confluence, scattered Mermaid in slide decks) goes
stale within weeks.

## Identity

Processes are identified by a three-segment colon-separated code:

```
{application}:{subdomain}:{process-name}
```

Examples:

- `orders:fulfilment:shipment-flow`
- `billing:invoicing:subscription-renewal`
- `platform:auth:oidc-login`

This mirrors `EventType` (which uses four segments
`app:subdomain:aggregate:event`) — Processes drop the `aggregate` segment
because a process typically spans several aggregates by definition.

The first segment must match the application's code. Codes are
case-sensitive and unique across the platform.

## Data model

### Schema (migration `026_processes.sql`)

| Column | Type | Notes |
|---|---|---|
| `id` | `VARCHAR(17)` | `prc_` prefixed TSID |
| `code` | `VARCHAR(255) UNIQUE` | Full colon-separated code |
| `name` | `VARCHAR(255)` | Human-readable label |
| `description` | `TEXT NULL` | Short summary, optional |
| `status` | `VARCHAR(20)` | `CURRENT` / `ARCHIVED` |
| `source` | `VARCHAR(20)` | `CODE` (SDK sync) / `API` (programmatic) / `UI` (admin UI) |
| `application` | `VARCHAR(100)` | First segment of `code` |
| `subdomain` | `VARCHAR(100)` | Second segment |
| `process_name` | `VARCHAR(100)` | Third segment |
| `body` | `TEXT` | Diagram source, verbatim |
| `diagram_type` | `VARCHAR(20)` | Default `mermaid` |
| `tags` | `TEXT[]` | Free-form tags for grouping |
| `created_at`, `updated_at` | `TIMESTAMPTZ` | Standard |

Indexes on `status`, `source`, `application`, `subdomain`. The DB-level
`UNIQUE(code)` is the only integrity constraint beyond standard
timestamps.

### Domain entity

`crates/fc-platform-messaging/src/process/entity.rs` — `Process` aggregate. Pure
data + behavior (`archive()`, `Process::new(code, name)` with code
parsing). No `sqlx` imports. Status enum: `Current` / `Archived`.
Source enum: `Code` / `Api` / `Ui`.

### Repository

`crates/fc-platform-messaging/src/process/repository.rs` — SQLx repository.
Implements `Persist<Process>` for `UnitOfWork`. Methods: `insert`,
`update`, `delete`, `find_by_id`, `find_by_code`, `find_all`,
`find_by_application`, `find_with_filters`, `exists_by_code`.

## Use cases

All writes route through `UnitOfWork::commit`. Location:
`crates/fc-platform-messaging/src/process/operations/`.

| Use case | Command | Event |
|---|---|---|
| `CreateProcessUseCase` | `CreateProcessCommand { code, name, description, body, diagram_type, tags }` | `ProcessCreated` |
| `UpdateProcessUseCase` | `UpdateProcessCommand { process_id, name?, description?, body?, diagram_type?, tags? }` | `ProcessUpdated` |
| `ArchiveProcessUseCase` | `ArchiveProcessCommand { process_id }` | `ProcessArchived` |
| `DeleteProcessUseCase` | `DeleteProcessCommand { process_id }` (archived only) | `ProcessDeleted` |
| `SyncProcessesUseCase` | `SyncProcessesCommand { application_code, processes, remove_unlisted }` | `ProcessesSynced` (single summary event) |

Validation rules enforced by use cases:

- Code must be three colon-separated segments, no empty segments.
- Update requires at least one mutated field (`NO_CHANGES` otherwise).
- Cannot update an archived process.
- Delete only allowed on archived processes.
- Sync only mutates `CODE`/`API`-sourced processes; UI-sourced are left
  untouched even on `removeUnlisted=true`.

## HTTP surface

Mounted under both `/api/processes` (bearer auth, SDK consumers) and
`/bff/processes` (cookie auth, SPA). Both point at the same router
defined in `crates/fc-platform-messaging/src/process/api.rs`.

| Method | Path | Permission | Use case |
|---|---|---|---|
| `POST` | `/` | `can_create_processes` | Create |
| `GET` | `/` | `can_read_processes` | List with filters (`application`, `subdomain`, `status`, `search`) |
| `GET` | `/{id}` | `can_read_processes` | Get by ID |
| `GET` | `/by-code/{code}` | `can_read_processes` | Get by code |
| `PUT` | `/{id}` | `can_update_processes` | Update — returns 204 |
| `POST` | `/{id}/archive` | `can_write_processes` | Archive |
| `DELETE` | `/{id}` | `can_delete_processes` | Hard delete (archived only) |
| `POST` | `/sync` | `can_sync_processes` | Bulk sync per application |

The list endpoint defaults to `status=CURRENT` when no filters are set,
matching the EventType convention.

## Permissions

Defined in `crates/fc-platform-iam/src/role/entity.rs::permissions::admin`
(messaging context) and `permissions::application_service` (SDK):

**Admin (full management):**
- `platform:messaging:process:view`
- `platform:messaging:process:create`
- `platform:messaging:process:update`
- `platform:messaging:process:delete`
- `platform:messaging:process:manage`
- `platform:messaging:process:archive`
- `platform:messaging:process:sync`

**Application service (SDK push):**
- `platform:application-service:process:view`
- `platform:application-service:process:sync`

### Built-in role wiring

| Role | Process permissions |
|---|---|
| `messaging-admin` | All seven admin permissions |
| `developer` | view, create, update, delete, archive (own portal use; sync deferred to service accounts) |
| `viewer` | view only |
| `application-service` (auto-assigned to app service accounts) | view, sync |

Permission check helpers in
`crates/fc-platform-core/src/shared/authorization_service.rs::checks`:
`can_read_processes`, `can_create_processes`, `can_update_processes`,
`can_delete_processes`, `can_write_processes` (any write),
`can_sync_processes`.

## Domain events

| Event type | Spec version | Subject pattern |
|---|---|---|
| `platform:admin:process:created` | 1.0 | `platform.process.{id}` |
| `platform:admin:process:updated` | 1.0 | `platform.process.{id}` |
| `platform:admin:process:archived` | 1.0 | `platform.process.{id}` |
| `platform:admin:process:deleted` | 1.0 | `platform.process.{id}` |
| `platform:admin:processes:synced` | 1.0 | `platform.application.{app}` (one summary event per sync call) |

Subscribers can wire to any of these in the normal way.

## SDK surface

### Rust SDK (`crates/fc-sdk/`)

File: `src/client/processes.rs`. Methods on `FlowCatalystClient`:

```rust
client.create_process(&CreateProcessRequest { ... }).await?;
client.get_process(id).await?;
client.get_process_by_code("orders:fulfilment:shipment-flow").await?;
client.list_processes(Some("orders"), None, Some("CURRENT"), None).await?;
client.update_process(id, &UpdateProcessRequest { ... }).await?;
client.archive_process(id).await?;
client.delete_process(id).await?;
client.sync_processes("orders", processes, /* remove_unlisted */ true).await?;
```

### TypeScript SDK (`clients/typescript-sdk/`)

File: `src/resources/processes.ts`. Mounted as `client.processes()`:

```ts
await client.processes().list({ application: "orders" });
await client.processes().get(id);
await client.processes().getByCode("orders:fulfilment:shipment-flow");
await client.processes().create({ code, name, body, tags });
await client.processes().update(id, { body });
await client.processes().archive(id);
await client.processes().delete(id);
await client.processes().sync("orders", processes, /* removeUnlisted */ true);
```

Generated bindings materialise on `pnpm generate` against a running
platform exposing `/q/openapi`.

### Laravel SDK (`clients/laravel-sdk/`)

Partial — DTOs/Enums/Attribute/Sync-definition landed; the
`Resources/Processes.php` HTTP wrapper and scanner wiring are tracked in
`docs/sdk-parity-plan.md` (workstream L1). What exists today:

- `src/DTOs/Process.php`
- `src/Enums/ProcessStatus.php`, `src/Enums/ProcessSource.php`
- `src/Sync/ProcessDefinition.php`
- `src/Attributes/AsProcess.php`

Once L1 lands, the typical Laravel usage will be:

```php
#[AsProcess(
    subdomain: 'fulfilment',
    processName: 'shipment-flow',
    name: 'Shipment Flow',
    body: <<<MERMAID
        graph TD
          A[Fulfilment Created] --> B[Build shipment]
        MERMAID,
)]
class ShipmentFlow {}

// Bundled with other definitions, pushed via DefinitionSynchronizer:
$result = $synchronizer->sync($definitionSet);
```

## Frontend

Vue 3 pages under `frontend/src/pages/processes/`:

- `ProcessListPage.vue` — filterable list, mounted at `/processes`.
- `ProcessDetailPage.vue` — renders Mermaid client-side, SVG/PNG
  download, archive/delete actions.
- `ProcessCreatePage.vue` — form with code + name + body editor.
- `ProcessEditPage.vue` — edits all fields; body is plain `<Textarea>`.

Route registration in `frontend/src/router/index.ts`. Nav placement in
`frontend/src/config/navigation.ts`: under the **Developer** section,
sibling to "Applications".

### Diagram rendering

`mermaid ^11.4.0` is imported dynamically only when a Process detail
page mounts — list and create/edit views don't pay the bundle cost.
Mermaid initializes with `securityLevel: "strict"` so the source can't
inject HTML. PNG export rasterises the SVG via a hidden `<canvas>` at
2× scale.

If `diagramType !== "mermaid"`, the detail page shows
`"Unsupported diagram type"` rather than attempting to render. The
schema reserves the field but no other renderers ship today.

## Code conventions

When adding a new Process-related endpoint (per workstream L1, or any
future change):

- Write path goes through `UseCase::run` → `UnitOfWork::commit` per the
  platform-wide convention. No direct repo calls from handlers.
- Permission checks always at the handler boundary using the
  `can_*_processes` helpers — never trust the URL tier.
- New permissions go into `permissions::admin` AND the `ALL` list in the
  same file, then into every built-in role that should receive them.
- Frontend stays in the `Developer` nav section. Don't reach for
  Tailwind classes — this codebase ships with no Tailwind. Mirror
  `ProcessListPage.vue`'s scoped CSS.

## Source of truth

- Migration: `migrations/026_processes.sql`
- Aggregate: `crates/fc-platform/src/process/`
- HTTP routes: `crates/fc-platform-messaging/src/process/api.rs` + `router.rs`
- Permissions: `crates/fc-platform-iam/src/role/entity.rs::permissions`
- SDK (Rust): `crates/fc-sdk/src/client/processes.rs`
- SDK (TS): `clients/typescript-sdk/src/resources/processes.ts`
- SDK (Laravel): `clients/laravel-sdk/src/{DTOs,Enums,Sync,Attributes}/Process*.php` (partial — L1 in progress)
- Frontend: `frontend/src/pages/processes/`
