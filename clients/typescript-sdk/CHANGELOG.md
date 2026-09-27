# Changelog — @flowcatalyst/sdk (TypeScript)

Releases are tagged `typescript-sdk/vX.Y.Z` in the source repo. The split
workflow mirrors the source to `github.com/flowcatalyst/typescript-sdk`,
commits a built `dist/` there, and tags it `vX.Y.Z`. The package is not on
the npm registry; consumers install it from that git repo. The version line
continues from the releases cut in `flowcatalyst-go` (last: 0.11.27); see
`docs/sdks.md` in the FlowCatalyst Rust repo.

## Unreleased (next: 0.11.28)

First release built from the FlowCatalyst Rust repo. It is 0.11.27 as
published, plus the changes below. No export or method was removed. Some
resource methods' declared return types changed where they were never true
against the platform (see "Changed": creates answer `{ id }`, `204`
answers resolve `void`); callers relying on those types get compile errors
at code that was reading `undefined`.

### Added
- Audit redaction: `CreateAuditLogDto` redacts `operationData` before it is
  serialised into the outbox. Password-, secret- and token-shaped fields
  become `***`. `redactAuditData` is exported, and the shared vectors are
  in `tests/fixtures/audit-redaction-vectors.json`.
- `withOperationData(data, maskedFields = [])`: an optional second argument
  for extra top-level fields to mask.
- `AuditMasked` / `auditMaskedFieldsOf`: a command can list its own masked
  fields. Both outbox units of work (plain and Effect) honour it.
- A test showing that a `FUNCTION`-sourced subscription round-trips.
  `source` is already typed `string`, so no code change was needed.
- `FlowCatalystClient.accessToken()`: the platform bearer token the client
  authenticates with (the caller's token in user-token mode, else the
  client-credentials token).
- `checkDeliverySignature(params)`: the result-returning form of
  `verifyDeliverySignature` (owner ruling 11 of 2026-09-25). It returns
  neverthrow's `ok(true)` for a genuine delivery, or
  `err(WebhookSignatureError)` with its `code`. The same check, so the two
  never disagree; `verifyDeliverySignature` still throws, unchanged.
- `CreateServiceAccountRequest.allApplications` (optional boolean) for
  `api.createServiceAccount`. A new service account has no application
  access; `allApplications: true` grants every application. The platform
  answers 403 unless the caller itself reaches every application, and 400
  `ALL_APPLICATIONS_WITH_APPLICATION_ID` alongside `applicationId`.
- `passwordHashIgnored` (optional `string[]`) on the principal sync
  results: `principals().sync()`, `principals().syncUsers()` and the
  synchronizer's principals `CategorySyncResult`. A sync uses
  `passwordHash` only to create a user and never changes an existing
  user's password (owner decision 22 of 2026-09-25); the platform lists
  the emails whose hash it ignored.
- The vendored `openapi/openapi.json` carries both fields (hand-added to
  the published spec, as the Java repo did), and `src/generated` is
  regenerated from it.
- `applications().getClientConfig(id, clientId)`: `GET
  /api/applications/{id}/clients/{clientId}` (the platform's
  `ClientConfigResponse`, with `configJson`).
- `applications().list(filters?)`: optional `type` / `active` filters.
- `eventTypes().getByCode(code)` (the README already listed it) and
  `eventTypes().delete(id)`.
- `dispatchPools().archive(id)`: `POST /api/dispatch-pools/{id}/archive`
  (a soft-delete; `delete(id)` removes the row).
- `auditLogs().recent(filters?)`: the same cursor paging and filters as
  `list`.
- Members the platform returns that the hand-written types lacked:
  `ApplicationResponse` (`serviceAccountId`, `hasLoginClient`,
  `defaultBaseUrl`, `iconUrl`, `website`, `logo`, `logoMimeType`),
  `ConnectionDto` (`source`, `applicationCode`), `DispatchPoolDto`
  (`concurrency`, `clientIdentifier`), `ScheduledJob.applicationId`,
  `InPipelineCheckResponse` (`poolCode`, `queueId`), and `FireResponse`
  (`instanceId`, `scheduledJobId`, exported).
- `CreateDispatchJobDto.withDescriptor(descriptor)`: what the job is, in
  words (e.g. "Notify Value of user logins"), shown in the platform's
  dispatch-jobs grid. It travels in the outbox payload as `descriptor` and
  is left out when unset, like `queue`. At most 255 characters
  (`MAX_DISPATCH_JOB_DESCRIPTOR_LENGTH`, exported); a longer one throws,
  where the platform would answer 400 `VALIDATION`.

### Changed
- Hand-written resource calls now send and read what the Go platform's API
  expects (its OpenAPI contract). Where a declared return type was never
  true against Go, it changed; TypeScript callers that read those values
  will get compile errors, which point at code that was reading
  `undefined`:
  - **Creates answer `{ id }`**: `create` on event types, processes,
    subscriptions, roles, clients, applications and dispatch pools now
    resolves `{ id }` (call `get(id)` for the entity). It was typed as the
    full entity. Connections' `create` still returns the entity (the
    platform does).
  - **`204 No Content` resolves `void`**: `update` on event types,
    processes, subscriptions, roles, clients, applications, connections and
    dispatch pools; `subscriptions().pause/resume`;
    `dispatchPools().suspend/activate`; `processes().archive`;
    `clients().updateApplications/enableApplication/disableApplication`;
    `applications().enableForClient/disableForClient`. Refetch after the
    call (see the repo's "Frontend API Response Handling" convention).
  - **Status changes answer `{ message }`**:
    `clients().activate/deactivate/suspend` and
    `principals().activate/deactivate` (were typed as the entity).
  - `applications().listRoles(id)` resolves `{ roles: string[] }`, the
    platform's shape (was typed as an array of role objects). Use
    `roles().listForApplication(id)` for full roles.
- `applications().getServiceAccount(id)` reads the application and then
  `GET /api/service-accounts/{serviceAccountId}`; it fails `not_found`
  when the application has none. The Go platform has no
  `GET …/service-account`.
- `applications().listClients(id)` reads the platform's `items` and each
  config's `configJson`. `clientConfigs`, `total` and `config` are kept
  as deprecated copies.
- `applications().provisionServiceAccount(id)` returns the platform's
  nested `serviceAccount.oauthClient.clientSecret` (the one-time secret);
  the flat `clientId` / `clientSecret` are deprecated copies of it, and
  `serviceAccountId` is optional (the platform does not return it).
- `eventTypes().update` and `connections().update` always send `name`,
  which the platform requires: when a caller omits it, the current name is
  read first and sent unchanged.
- `eventTypes().sync(...)` and the synchronizer send each event type as
  only `code` / `name` / `description`; the platform's strict sync item
  rejects any other member (e.g. `schema`, `clientId`).
- Dispatch pools use the platform's `concurrency` (not `maxConcurrency`)
  on create, update, get and list. A caller's `maxConcurrency` is sent as
  `concurrency`, and responses copy `concurrency` into `maxConcurrency`;
  `rateLimitWindow` / `applicationCode` are not sent. `dispatchPools().sync`
  sends only the members the platform's strict sync item accepts (`code`,
  `name`, `description`, `concurrency`, `rateLimit`) and reads `deleted`
  (`removed` is a deprecated copy).
- `scheduledJobs().list` / `listInstances` fill `totalPages` from the
  platform's `total_pages`. `create` sends `concurrent` and
  `tracksCompletion` (required by the platform) as `false` when omitted;
  `logForInstance` sends `level: "INFO"` when omitted.
- `router().inPipeline(id)` reads `poolCode` / `queueId` from the top
  level of the router's answer (a legacy `detail` object is still lifted).

### Deprecated
- `applications().updateClientConfig(...)`: only the Rust platform serves
  `PUT /api/applications/{id}/clients/{clientId}`.
- `eventTypes().archive(id)`: the platform has no archive route for event
  types, so this sends `DELETE`, which removes the row. Use `delete(id)`.
- Filters and members the platform does not have (documented on each):
  `page`/`size` on event-type, process and subscription lists (the platform
  does not paginate them), `ProcessFilters.search`,
  `ConnectionFilters.serviceAccountId`, `ListInstancesFilters.triggerKind`
  / `from` / `to`, the connection `endpoint`, and the dispatch-pool
  `maxConcurrency` / `rateLimitWindow` / `applicationCode`.

- The Fastify OIDC session refresh is single-flight (owner ruling 5 of
  2026-09-25). The platform rotates refresh tokens and revokes the whole
  family when a rotated-out token is presented again (beyond a 10 s
  leeway). Concurrent requests of one session now join one in-flight
  exchange per refresh token, and a request that read the old token just
  after reuses its result for 10 s. Per process: instances behind a load
  balancer still rely on the platform's leeway. `refreshAccessToken` keeps
  its signature.
- `client.router().inPipeline()` / `inPipelineBatch()` send the platform
  bearer token to the router (`Authorization: Bearer …`). Today's routers
  ignore it; a router that enforces platform tokens (owner ruling 2 of
  2026-09-25) requires it, with `platform:messaging:router:view`, which the
  built-in `platform:application-service` role holds. Ship this release to
  apps **before** any router enforces auth.
- `OutboxManager.createDispatchJob` / `createDispatchJobs`: the outbox
  payload now carries `id`, the outbox row's own id (the id the method
  returns). The platform honours a supplied dispatch-job id, so a batch the
  outbox processor resends after losing the platform's answer is recognised
  instead of creating the job a second time. No signature changed.
- Licence: Apache-2.0, as published (owner decision of 2026-09-25, which
  reverses the earlier move to MPL-2.0). The package now ships the
  Apache-2.0 `LICENSE` text, which the published source lacked.

## 0.11.27 and earlier

Released from `flowcatalyst-go` (`clients/typescript-sdk`); see that repo's
history and its `typescript-sdk/v*` tags.
