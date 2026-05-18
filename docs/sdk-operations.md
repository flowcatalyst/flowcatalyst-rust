# SDK Operation Surface — Rust / TypeScript / Laravel

Side-by-side catalogue of every public operation across the three SDKs:

- **Rust** — `crates/fc-sdk/` (flat methods on `FlowCatalystClient`, snake_case)
- **TS** — `clients/typescript-sdk/` (resource accessors: `sdk.eventTypes().list()`)
- **Laravel** — `clients/laravel-sdk/` (resource classes: `$sdk->eventTypes()->list()`)

`—` means the operation is missing from that SDK.

> **Sync surface aligned (2026-05).** Every sync endpoint is now
> `POST /api/applications/{app_code}/{resource}/sync` — one shape across
> roles, event-types, subscriptions, dispatch-pools, principals, processes,
> scheduled-jobs, openapi. The duplicate collection-level handlers
> (`/api/event-types/sync` etc.) and the duplicate roles router
> (`application_roles_sdk_api::sync_roles`) have been removed. Each
> app-scoped sync handler now does an explicit `can_sync_*` permission
> check at the handler layer (was previously delegated to the use case,
> a CLAUDE.md violation). All three SDKs target the canonical URLs.

> **SDK parity sweep (2026-05).** All numbered recommendations below
> have shipped. Highlights:
> - **Rust SDK** now uses resource accessors (`client.applications().list()`)
>   throughout — the flat `client.list_applications()` form is gone. Adds
>   `ScheduledJobs` (15+ operations) and `Permissions` accessors that
>   didn't exist before. `is_message_in_pipeline` → `in_pipeline`,
>   `assign_principal_role*` → `add_role` / `set_roles`,
>   `archive_event_type` → `archive`, etc.
> - **Laravel SDK** closes the Applications gap (`delete`,
>   `provisionServiceAccount`, `getServiceAccount`, `listRoles`,
>   `listClients`, `updateClientConfig`, `enableForClient`,
>   `disableForClient`), adds `AuditLogs` resource, exposes
>   `DefinitionSynchronizer` via `$client->definitions()`,
>   `assignRole`/`assignRoles` → `addRole`/`setRoles`,
>   `isInPipeline`/`areInPipeline` → `inPipeline`/`inPipelineBatch`,
>   `eventTypes->delete` → `eventTypes->archive`.
> - **TS SDK** closes the Roles permission/sync gap (`getByCode`,
>   `grantPermission`, `revokePermission`, `sync`), adds `principals().sync`,
>   `clients().search` and `clients().addNote`, plus a full `auditLogs()`
>   resource. Same renames as Laravel.
> - **Cosmetic regen (#8).** The `Admin` infix has been stripped from server
>   operationIds; regenerated bindings are now `getApiClients` etc. rather
>   than `getApiAdminClients`.
>
> Pre-existing bug fixed along the way: `applications.listRoles()` in
> Rust and TS was calling `/api/applications/{id}/roles`, but the
> server routes that under `/by-id` to avoid colliding with the
> `/{app_code}/roles/sync` SDK route. All three SDKs now hit
> `/api/applications/by-id/{id}/roles`.

---

## Executive summary — what's not parity

### Structural mismatch
- **Rust has no resource accessors.** Every operation is a flat method on the client (`client.list_clients()`, `client.get_principal()`). TS and Laravel both group by resource (`sdk.clients().list()`, `$sdk->principals()->get()`). This means every Rust method has to carry the resource name as a prefix — that's where most of the awkward names come from.

### Resource gaps (entire resources missing)
| Resource | Rust | TS | Laravel |
|---|---|---|---|
| **ScheduledJobs** | ❌ missing | ✅ | ✅ |
| **AuditLogs** | ✅ (2 ops) | ❌ no resource class¹ | ❌ |
| **DefinitionSynchronizer / definitions.sync()** | ✅ (`DefinitionSynchronizer`) | ✅ (`sdk.definitions().sync()`) | ❌ |
| **ScheduledJobRunner (handler registration)** | ❌ | ✅ (`sdk.scheduledJobRunner()`) | ❌ |

¹ TS has audit-log functions in the generated client but no `AuditLogs` resource class on the SDK facade.

### Operation gaps within resources
Notable per-resource omissions are flagged inline in the tables below. The biggest ones:

- **Laravel — Applications** is missing 7 operations the other two have (`delete`, `provisionServiceAccount`, `getServiceAccount`, `listRoles`, `listClients`, `updateClientConfig`, `enableForClient`, `disableForClient`).
- **TS — Roles** is missing `getByCode`, `grantPermission`, `revokePermission`, `sync`.
- **TS — Principals** is missing `sync`.
- **TS — Clients** is missing `search` and `addNote`.
- **Rust — DispatchPools** is missing the explicit `archive` action (Laravel has it; the operation is folded into `delete` in Rust).

### Names that don't read well (flagged for review)

| Current Rust name | Why it's awkward | Suggested form |
|---|---|---|
| `client.me_get_clients()` / `me_get_client(id)` / `me_get_client_applications(id)` | `me_` is a stuttering prefix forced by the flat namespace | `client.me().clients()` etc. via a `me()` accessor |
| `client.is_message_in_pipeline(id)` / `are_messages_in_pipeline(ids)` | English-grammar split (`is`/`are`) reads like a unit test | `client.router().in_pipeline(id)` / `in_pipeline_batch(ids)` |
| `client.find_principal_by_email(email)` | "find_X_by_Y" pattern doesn't appear anywhere else in Rust SDK; TS/Laravel both use `findByEmail` | `client.principals().find_by_email(email)` |
| `client.assign_principal_role(id, r)` vs `assign_principal_roles(id, rs)` | Singular vs plural with different semantics (additive vs replace) is invisible at the call site | `principals().add_role()` / `principals().set_roles()` (verbs encode intent) |
| `client.add_schema_version()` vs TS `addSchema` vs Laravel `addSchemaVersion` | Three different names for the same call | Pick one — `addSchemaVersion` is the most precise |
| `client.get_role_by_code(code)` | Same pattern as `get_application_by_code` — fine in isolation, but missing on TS | Add `getByCode` to TS `Roles` resource |
| `client.list_application_clients(id)` / `update_application_client_config(...)` / `enable_application_for_client(...)` | Long name from chaining the resource and the related resource | A nested accessor (`applications(id).clients()...`) would clean this up |
| `client.create_user(...)` posts to `/api/principals/users` | The verb implies "create a User entity", but `User` is a kind of `Principal` here | `principals().create_user(...)` makes the nesting obvious |
| Rust `delete_subscription` vs `delete_connection` vs Rust `archive_event_type` (uses DELETE) | Inconsistent verb for the DELETE verb across resources | Settle on `delete` everywhere and let the server decide if it's a soft delete |
| Rust dispatch-pools: only `delete` (no separate `archive`) | Laravel exposes both `archive` (soft) and `delete` (hard); same server, two intents | Mirror Laravel: `archive` + `delete` |

### Sync endpoint shape (resolved — single URL convention)

All sync endpoints now live at `POST /api/applications/{app_code}/{resource}/sync`. Body shape is `{ <resourcePlural>: [...] }` with `applicationCode` no longer in the body — it travels in the URL path. The `removeUnlisted` query param is camelCase. Routes:

```
POST /api/applications/{app_code}/roles/sync
POST /api/applications/{app_code}/event-types/sync
POST /api/applications/{app_code}/subscriptions/sync
POST /api/applications/{app_code}/dispatch-pools/sync
POST /api/applications/{app_code}/principals/sync
POST /api/applications/{app_code}/processes/sync
POST /api/applications/{app_code}/scheduled-jobs/sync
POST /api/applications/{app_code}/openapi/sync
```

All three SDKs target these URLs.

---

## Side-by-side operation tables

Logical operation → method name in each SDK. The HTTP path is shown once per row (it's the same across SDKs unless noted).

### Applications — `/api/applications`

| Operation | Rust | TS | Laravel |
|---|---|---|---|
| List | `list_applications()` | `applications().list()` | `applications()->list()` |
| Get by id | `get_application(id)` | `applications().get(id)` | `applications()->get(id)` |
| Get by code | `get_application_by_code(code)` | `applications().getByCode(code)` | `applications()->getByCode(code)` |
| Create | `create_application(req)` | `applications().create(data)` | `applications()->create(req)` |
| Update | `update_application(id, req)` | `applications().update(id, data)` | `applications()->update(id, req)` |
| Delete (hard) | `delete_application(id)` | `applications().delete(id)` | **— missing** |
| Activate | `activate_application(id)` | `applications().activate(id)` | `applications()->activate(id)` |
| Deactivate | `deactivate_application(id)` | `applications().deactivate(id)` | `applications()->deactivate(id)` |
| Provision service account | `provision_service_account(id)` | `applications().provisionServiceAccount(id)` | **— missing** |
| Get service account | `get_service_account(id)` | `applications().getServiceAccount(id)` | **— missing** |
| List roles | `list_application_roles(id)` | `applications().listRoles(id)` | **— missing** |
| List per-client configs | `list_application_clients(id)` | `applications().listClients(id)` | **— missing** |
| Update per-client config | `update_application_client_config(id, cid, data)` | `applications().updateClientConfig(id, cid, data)` | **— missing** |
| Enable for client | `enable_application_for_client(id, cid)` | `applications().enableForClient(id, cid)` | **— missing** |
| Disable for client | `disable_application_for_client(id, cid)` | `applications().disableForClient(id, cid)` | **— missing** |

### Clients (tenants) — `/api/clients`

| Operation | Rust | TS | Laravel |
|---|---|---|---|
| List | `list_clients(...)` | `clients().list()` | `clients()->list(status?)` |
| Search | `search_clients(term)` | **— missing** | `clients()->search(term)` |
| Get by id | `get_client(id)` | `clients().get(id)` | `clients()->get(id)` |
| Get by identifier | `get_client_by_identifier(ident)` | `clients().getByIdentifier(ident)` | `clients()->getByIdentifier(ident)` |
| Create | `create_client(req)` | `clients().create(data)` | `clients()->create(req)` |
| Update | `update_client(id, req)` | `clients().update(id, data)` | `clients()->update(id, req)` |
| Delete | `delete_client(id)` | **— missing** | **— missing** |
| Activate | `activate_client(id)` | `clients().activate(id)` | `clients()->activate(id)` |
| Suspend | `suspend_client(id, reason)` | `clients().suspend(id, reason)` | `clients()->suspend(id, reason)` |
| Deactivate | `deactivate_client(id, reason)` | `clients().deactivate(id, reason)` | `clients()->deactivate(id, reason)` |
| Add note | `add_client_note(id, cat, text)` | **— missing** | `clients()->addNote(id, cat, text)` |
| List apps for client | `list_client_applications(id)` | `clients().getApplications(id)` | `clients()->getApplications(id)` |
| Update apps (declarative) | `update_client_applications(id, req)` | `clients().updateApplications(id, data)` | `clients()->updateApplications(id, req)` |
| Enable app for client | `enable_client_application(cid, aid)` | `clients().enableApplication(cid, aid)` | `clients()->enableApplication(cid, aid)` |
| Disable app for client | `disable_client_application(cid, aid)` | `clients().disableApplication(cid, aid)` | `clients()->disableApplication(cid, aid)` |

> **Naming note:** TS uses `getApplications` for what Rust calls `list_application*` and Laravel calls `getApplications`. TS could be `listApplications` for consistency with the verb pattern elsewhere in the SDK.

### Event Types — `/api/event-types` (sync at `/api/applications/{app_code}/event-types/sync`)

| Operation | Rust | TS | Laravel |
|---|---|---|---|
| List | `list_event_types(...)` | `eventTypes().list(filters?, page?)` | `eventTypes()->list(...)` |
| Get by id | `get_event_type(id)` | `eventTypes().get(id)` | `eventTypes()->get(id)` |
| Get by code | `get_event_type_by_code(code)` | **— missing** | `eventTypes()->getByCode(code)` |
| Create | `create_event_type(req)` | `eventTypes().create(data)` | `eventTypes()->create(req)` |
| Update | `update_event_type(id, req)` | `eventTypes().update(id, data)` | `eventTypes()->update(id, req)` |
| Add schema version | `add_schema_version(id, ...)` | `eventTypes().addSchema(id, schema)` | `eventTypes()->addSchemaVersion(id, ...)` |
| Archive / Delete | `archive_event_type(id)` | `eventTypes().delete(id)` | `eventTypes()->delete(id)` |
| Sync | `sync_event_types(app, ...)` | `eventTypes().sync(app, items, remove?)` | `eventTypes()->sync(app, items, remove?)` |

> **Naming notes:** Rust calls it `archive_event_type` (semantically accurate — it's a soft delete); TS/Laravel call it `delete`. Pick one. Also `addSchema` (TS) vs `addSchemaVersion` (Laravel/Rust) — the second is more precise.

### Subscriptions — `/api/subscriptions` (sync at `/api/applications/{app_code}/subscriptions/sync`)

| Operation | Rust | TS | Laravel |
|---|---|---|---|
| List | `list_subscriptions(...)` | `subscriptions().list(...)` | `subscriptions()->list(...)` |
| Get | `get_subscription(id)` | `subscriptions().get(id)` | `subscriptions()->get(id)` |
| Create | `create_subscription(req)` | `subscriptions().create(data)` | `subscriptions()->create(req)` |
| Update | `update_subscription(id, req)` | `subscriptions().update(id, data)` | `subscriptions()->update(id, req)` |
| Delete | `delete_subscription(id)` | `subscriptions().delete(id)` | `subscriptions()->delete(id)` |
| Pause | `pause_subscription(id)` | `subscriptions().pause(id)` | `subscriptions()->pause(id)` |
| Resume | `resume_subscription(id)` | `subscriptions().resume(id)` | `subscriptions()->resume(id)` |
| Sync | `sync_subscriptions(app, ...)` | `subscriptions().sync(app, items, remove?)` | `subscriptions()->sync(app, items, remove?)` |

### Connections — `/api/connections`

| Operation | Rust | TS | Laravel |
|---|---|---|---|
| List | `list_connections(...)` | `connections().list(...)` | `connections()->list(...)` |
| Get | `get_connection(id)` | `connections().get(id)` | `connections()->get(id)` |
| Create | `create_connection(req)` | `connections().create(data)` | `connections()->create(req)` |
| Update | `update_connection(id, req)` | `connections().update(id, data)` | `connections()->update(id, req)` |
| Delete | `delete_connection(id)` | `connections().delete(id)` | `connections()->delete(id)` |
| Pause | `pause_connection(id)` | `connections().pause(id)` | `connections()->pause(id)` |
| Activate | `activate_connection(id)` | `connections().activate(id)` | `connections()->activate(id)` |

### Dispatch Pools — `/api/dispatch-pools` (sync at `/api/applications/{app_code}/dispatch-pools/sync`)

| Operation | Rust | TS | Laravel |
|---|---|---|---|
| List | `list_dispatch_pools(...)` | `dispatchPools().list(...)` | `dispatchPools()->list(...)` |
| Get | `get_dispatch_pool(id)` | `dispatchPools().get(id)` | `dispatchPools()->get(id)` |
| Create | `create_dispatch_pool(req)` | `dispatchPools().create(data)` | `dispatchPools()->create(req)` |
| Update | `update_dispatch_pool(id, req)` | `dispatchPools().update(id, data)` | `dispatchPools()->update(id, req)` |
| Delete (hard) | `delete_dispatch_pool(id)` | `dispatchPools().delete(id)` | `dispatchPools()->delete(id)` |
| Archive (soft) | **— missing** | **— missing** | `dispatchPools()->archive(id)` |
| Suspend | `suspend_dispatch_pool(id)` | `dispatchPools().suspend(id)` | `dispatchPools()->suspend(id)` |
| Activate | `activate_dispatch_pool(id)` | `dispatchPools().activate(id)` | `dispatchPools()->activate(id)` |
| Sync | `sync_dispatch_pools(app, ...)` | `dispatchPools().sync(app, items, remove?)` | `dispatchPools()->sync(app, items, remove?)` |

> **Naming note:** Only Laravel distinguishes `archive` (soft, keeps row) from `delete` (hard). Rust/TS conflate them under `delete`. The server supports both intents — the other two SDKs should match.

### Processes — `/api/processes` (sync at `/api/applications/{app_code}/processes/sync`)

| Operation | Rust | TS | Laravel |
|---|---|---|---|
| List | `list_processes(...)` | `processes().list(...)` | `processes()->list(...)` |
| Get by id | `get_process(id)` | `processes().get(id)` | `processes()->get(id)` |
| Get by code | `get_process_by_code(code)` | `processes().getByCode(code)` | `processes()->getByCode(code)` |
| Create | `create_process(req)` | `processes().create(data)` | `processes()->create(data)` |
| Update | `update_process(id, req)` | `processes().update(id, data)` | `processes()->update(id, data)` |
| Archive (soft) | `archive_process(id)` | `processes().archive(id)` | `processes()->archive(id)` |
| Delete (hard, archived only) | `delete_process(id)` | `processes().delete(id)` | `processes()->delete(id)` |
| Sync | `sync_processes(app, ...)` | `processes().sync(app, items, remove?)` | `processes()->sync(app, items, remove?)` |

> Processes is the cleanest of the bunch — both `archive` and `delete` are exposed everywhere and have the same semantics.

### Principals — `/api/principals`

| Operation | Rust | TS | Laravel |
|---|---|---|---|
| List | `list_principals(...)` | `principals().list(...)` | `principals()->list(...)` |
| Get | `get_principal(id)` | `principals().get(id)` | `principals()->get(id)` |
| Find by email | `find_principal_by_email(email)` | `principals().findByEmail(email)` | `principals()->findByEmail(email)` |
| Create user | `create_user(req)` | `principals().createUser(data)` | `principals()->createUser(req)` |
| Update | `update_principal(id, req)` | `principals().update(id, data)` | `principals()->update(id, req)` |
| Activate | `activate_principal(id)` | `principals().activate(id)` | `principals()->activate(id)` |
| Deactivate | `deactivate_principal(id)` | `principals().deactivate(id)` | `principals()->deactivate(id)` |
| Reset password | `reset_principal_password(id, ...)` | `principals().resetPassword(id, data)` | `principals()->resetPassword(id, pw, enforce?)` |
| Get roles | `get_principal_roles(id)` | `principals().getRoles(id)` | `principals()->getRoles(id)` |
| Assign role (additive) | `assign_principal_role(id, name)` | `principals().assignRole(id, name)` | `principals()->assignRole(id, name)` |
| Remove role | `remove_principal_role(id, name)` | `principals().removeRole(id, name)` | `principals()->removeRole(id, name)` |
| Replace all roles | `assign_principal_roles(id, names)` | `principals().assignRoles(id, names)` | `principals()->assignRoles(id, names)` |
| Get client access | `get_principal_client_access(id)` | `principals().getClientAccessGrants(id)` | `principals()->getClientAccessGrants(id)` |
| Grant client access | `grant_principal_client_access(id, cid)` | `principals().grantClientAccess(id, cid)` | `principals()->grantClientAccess(id, cid)` |
| Revoke client access | `revoke_principal_client_access(id, cid)` | `principals().revokeClientAccess(id, cid)` | `principals()->revokeClientAccess(id, cid)` |
| Sync | `sync_principals(app, ...)` | **— missing** | `principals()->sync(app, items, remove?)` |

> **Naming note:** `assignRole` (singular = additive) vs `assignRoles` (plural = replace-all) is identical in all three SDKs but is a footgun — the difference is invisible at the call site. Renaming to `addRole`/`setRoles` would make the semantics obvious. Also Rust's `get_principal_client_access` is shorter than TS/Laravel's `getClientAccessGrants` — pick one.

### Roles — `/api/roles`

| Operation | Rust | TS | Laravel |
|---|---|---|---|
| List | `list_roles()` | `roles().list(page?)` | `roles()->list(...)` |
| Get by name | `get_role(name)` | `roles().get(name)` | `roles()->get(name)` |
| Get by code | `get_role_by_code(code)` | **— missing** | `roles()->getByCode(code)` |
| Create | `create_role(req)` | `roles().create(data)` | `roles()->create(req)` |
| Update | `update_role(name, req)` | `roles().update(name, data)` | `roles()->update(name, req)` |
| Delete | `delete_role(name)` | `roles().delete(name)` | `roles()->delete(name)` |
| List for application | `list_roles_for_application(app_id)` | `roles().listForApplication(app_id)` | (covered via `list(applicationCode)`) |
| Grant permission | `grant_permission(name, perm)` | **— missing** | `roles()->grantPermission(name, perm)` |
| Revoke permission | `revoke_permission(name, perm)` | **— missing** | `roles()->revokePermission(name, perm)` |
| Sync | `sync_roles(app, ...)` | **— missing** | `roles()->sync(app, items, remove?)` |

### Permissions — `/api/roles/permissions`

| Operation | Rust | TS | Laravel |
|---|---|---|---|
| List | `list_permissions()` | `permissions().list()` | `permissions()->list()` |
| Get | `get_permission(name)` | `permissions().get(name)` | `permissions()->get(perm)` |

### Audit Logs — `/api/audit-logs`

| Operation | Rust | TS | Laravel |
|---|---|---|---|
| List | `list_audit_logs(...)` | **— missing** | **— missing** |
| Get | `get_audit_log(id)` | **— missing** | **— missing** |

> Only Rust exposes this as a first-class resource. TS has generated bindings (`getApiAdminAuditLogs*`) but no `AuditLogs` resource class.

### Me (current-user context) — `/api/me`

| Operation | Rust | TS | Laravel |
|---|---|---|---|
| Accessible clients | `me_get_clients()` | `me().getClients()` | `me()->getClients()` |
| Get one accessible client | `me_get_client(id)` | `me().getClient(id)` | `me()->getClient(id)` |
| Apps in a client | `me_get_client_applications(id)` | `me().getClientApplications(id)` | `me()->getClientApplications(id)` |

> **Naming note:** The Rust `me_` prefix is the clearest example of what a `me()` accessor would clean up.

### Router (in-flight message check) — `/monitoring/in-flight-messages`

| Operation | Rust | TS | Laravel |
|---|---|---|---|
| Check one | `is_message_in_pipeline(id)` | `router().isInPipeline(id)` | `router()->isInPipeline(id)` |
| Check batch | `are_messages_in_pipeline(ids)` | `router().areInPipeline(ids)` | `router()->areInPipeline(ids)` |

> **Naming note:** `is_*` / `are_*` reads strangely. Even TS/Laravel could rename to `inPipeline(id)` / `inPipelineBatch(ids)`.

### Scheduled Jobs — `/api/scheduled-jobs`

| Operation | Rust | TS | Laravel |
|---|---|---|---|
| Create | **— missing** | `scheduledJobs().create(req)` | `scheduledJobs()->create(req)` |
| List | **— missing** | `scheduledJobs().list(filters?)` | `scheduledJobs()->list(...)` |
| Get | **— missing** | `scheduledJobs().get(id)` | `scheduledJobs()->get(id)` |
| Get by code | **— missing** | `scheduledJobs().getByCode(code, cid?)` | `scheduledJobs()->getByCode(code, cid?)` |
| Update | **— missing** | `scheduledJobs().update(id, req)` | `scheduledJobs()->update(id, req)` |
| Pause | **— missing** | `scheduledJobs().pause(id)` | `scheduledJobs()->pause(id)` |
| Resume | **— missing** | `scheduledJobs().resume(id)` | `scheduledJobs()->resume(id)` |
| Archive | **— missing** | `scheduledJobs().archive(id)` | `scheduledJobs()->archive(id)` |
| Delete | **— missing** | `scheduledJobs().delete(id)` | `scheduledJobs()->delete(id)` |
| Fire manually | **— missing** | `scheduledJobs().fire(id, req?)` | `scheduledJobs()->fire(id, corr?)` |
| List instances | **— missing** | `scheduledJobs().listInstances(jid, ...)` | `scheduledJobs()->listInstances(jid, ...)` |
| Get instance | **— missing** | `scheduledJobs().getInstance(iid)` | `scheduledJobs()->getInstance(iid)` |
| List instance logs | **— missing** | `scheduledJobs().listInstanceLogs(iid)` | `scheduledJobs()->listInstanceLogs(iid)` |
| Log on instance | **— missing** | `scheduledJobs().logForInstance(iid, req)` | `scheduledJobs()->logForInstance(iid, ...)` |
| Complete instance | **— missing** | `scheduledJobs().completeInstance(iid, req)` | `scheduledJobs()->completeInstance(iid, ...)` |
| Sync | **— missing** | (n/a on TS) | `scheduledJobs()->sync(app, jobs, cid?, archive?)` |
| Handler runner | **— missing** | `sdk.scheduledJobRunner(opts?)` | **— missing** |

> Rust is missing the entire ScheduledJobs surface — 15+ operations. This is the largest single gap.

### Bulk synchronization

| Operation | Rust | TS | Laravel |
|---|---|---|---|
| Multi-resource sync orchestrator | `DefinitionSynchronizer::new(c).sync(set, opts)` / `sync_all(sets, opts)` | `definitions().sync(set)` | **— missing** |

---

## Recommendations — status

All eight items below have shipped. The tables further up in this doc still
reflect the *pre-sweep* state of the Rust method names (snake_case flat
form) — they're kept as a record of where things were before the refactor.
Today the Rust SDK uses resource accessors throughout.

1. ~~Add resource accessors to Rust~~ — **done (2026-05).** `crates/fc-sdk/src/client/` restructured: each resource is `pub struct X<'a> { client: &'a FlowCatalystClient }` with methods accessed via `client.applications()`, `client.principals()`, etc. ~100 methods renamed (`list_applications` → `applications().list()`, `me_get_clients` → `me().clients()`, etc.). `DefinitionSynchronizer` updated to use new accessor paths.
2. ~~Close the ScheduledJobs gap in Rust~~ — **done (2026-05).** `crates/fc-sdk/src/client/scheduled_jobs.rs` added with all 17 operations (CRUD + state transitions + instance reads + SDK callbacks + sync).
3. ~~Close the Applications gap in Laravel~~ — **done (2026-05).** Added `delete`, `provisionServiceAccount`, `getServiceAccount`, `listRoles`, `listClients`, `updateClientConfig`, `enableForClient`, `disableForClient` plus supporting DTOs (`ServiceAccount`, `ApplicationRole`, `ClientConfig`, `ClientConfigRequest`, `ClientConfigList`).
4. ~~Close the Roles permission/sync gap in TS~~ — **done (2026-05).** Added `getByCode`, `grantPermission`, `revokePermission`, `sync` on the TS `RolesResource`. Also added `principals().sync`, `clients().search`, `clients().addNote`, and the full `auditLogs()` resource for completeness.
5. ~~Canonical naming sweep~~ — **done (2026-05).**
   - `addSchemaVersion` is the canonical name across all three SDKs (TS `addSchema` renamed).
   - `delete` vs `archive`: event-types is `archive` everywhere (it's semantically a soft archive); processes keeps both `archive`/`delete` (which differ).
   - `getClientAccessGrants` (TS/Laravel) vs `client_access_grants()` (Rust) — Rust drops the `get_` for idiomatic style; TS/Laravel match each other.
   - `assignRole`/`assignRoles` → `addRole`/`setRoles` everywhere — makes additive-vs-replace visible.
6. ~~Rename `is_message_in_pipeline` / `are_messages_in_pipeline`~~ — **done (2026-05).** All three SDKs now use `in_pipeline` / `in_pipeline_batch` (Rust) and `inPipeline` / `inPipelineBatch` (TS, Laravel).
7. ~~Server-side: unify sync URL shape~~ — **done (2026-05, earlier pass).** All sync endpoints unified to `/api/applications/{app}/{resource}/sync`.
8. ~~Cosmetic: drop the `Admin` infix in TS/frontend bindings~~ — **done (2026-05).** Server operationIds renamed (`postApiAdminEventTypes` → `postApiEventTypes`); openapi.json snapshots patched; TS resource files swept; frontend bindings regenerated.
