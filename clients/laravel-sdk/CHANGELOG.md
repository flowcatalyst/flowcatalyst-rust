# Changelog — flowcatalyst/laravel-sdk

Releases are tagged `laravel-sdk/vX.Y.Z` in the source repo and mirrored to
`github.com/flowcatalyst/laravel-sdk` as `vX.Y.Z` (apps install it as a
Composer `vcs` repository). The version line continues from the releases cut
in `flowcatalyst-go` (last: 0.10.26); see `docs/sdks.md` in the FlowCatalyst
Rust repo.

## Unreleased (next: 0.10.27)

First release built from the FlowCatalyst Rust repo. It is 0.10.26 as
published, plus the additions below. No public API was removed. Some
return types changed, under "Fixed: calls against Go's platform API"; each
method changed there threw or returned wrong data against the platform
before, so no working call breaks.

### Added
- `SubscriptionSource::FUNCTION`: subscriptions created when a function is
  promoted carry this source. Before, `SubscriptionSource::from('FUNCTION')`
  threw.
- Audit redaction: `CreateAuditLogDto` redacts `operationData` before it is
  serialised into the outbox. Password-, secret- and token-shaped fields
  become `***`; the rules are in `FlowCatalyst\Outbox\AuditRedaction`, and
  the shared vectors are in `tests/Fixtures/audit-redaction-vectors.json`.
- `CreateAuditLogDto::withOperationData($data, $maskedFields = [])` and a
  trailing `$maskedFields` constructor argument. Both are optional, and the
  constructor argument comes last, so existing positional and named calls
  are unchanged.
- `FlowCatalyst\UseCase\AuditMasked` interface: a command can list extra
  top-level fields to mask. `OutboxUnitOfWork` honours it.
- `WebhookValidator::check()` / `checkRequest()`: the result-returning forms
  of `validate()` / `validateRequest()` (owner ruling 11 of 2026-09-25).
  They return a `FlowCatalyst\Webhook\WebhookVerification` (`valid`, and
  the `reason` when invalid) instead of throwing. The same check; the
  throwing methods are unchanged.
- `CreateServiceAccountRequest::setAllApplications()` (generated model,
  for `$client->generated()->createServiceAccount()`). A new service
  account has no application access; `allApplications: true` grants every
  application. The platform answers 403 unless the caller itself reaches
  every application, and 400 `ALL_APPLICATIONS_WITH_APPLICATION_ID`
  alongside `applicationId`.
- `passwordHashIgnored` on the principal sync results: the
  `SyncResult::$passwordHashIgnored` DTO property (last, optional
  constructor argument, default `[]`) returned by `principals()->sync()` and
  `principals()->syncUsers()`, the generated `SyncResultResponse` /
  `SyncUsersResponse`, and the synchronizer's `principals` result array. A
  sync uses `passwordHash` only to create a user and never changes an
  existing user's password (owner decision 22 of 2026-09-25); the platform
  lists the emails whose hash it ignored, and `flowcatalyst:sync` prints
  them as a warning.
- The vendored `openapi/openapi.json` carries both fields. The generated
  models were edited by hand to match: regenerating with today's Jane
  rewrites ~530 generated files, so it is left for a deliberate regen.
- `CreateDispatchJobDto::withDescriptor($descriptor)` and a trailing
  optional `descriptor` constructor parameter (after `queue`, so positional
  callers are unaffected): what the job is, in words (e.g. "Notify Value of
  user logins"), shown in the platform's dispatch-jobs grid. It travels in
  the outbox payload as `descriptor` and is left out when unset, like
  `queue`. At most 255 characters (`CreateDispatchJobDto::MAX_DESCRIPTOR_LENGTH`);
  a longer one throws `InvalidArgumentException`, where the platform would
  answer 400 `VALIDATION`.

### Changed
- The OIDC session refresh (`TokenRefresher`, used by the refresh route and
  `AuthenticateFc`'s automatic refresh) is single-flight (owner ruling 5 of
  2026-09-25). The platform rotates refresh tokens and revokes the whole
  family when a rotated-out token is presented again (beyond a 10 s
  leeway). One exchange now runs per refresh token under a cache lock
  across PHP workers, and its token set is kept encrypted in the cache for
  10 s so concurrent requests of the same session reuse it. A cache store
  without locks still gets the memo. Use a shared cache store (redis,
  memcached, database) for this to hold across servers.
- `Router::inPipeline()` / `inPipelineBatch()` send the platform bearer
  token (the client's token provider) to the router. Today's routers ignore
  it; a router that enforces platform tokens (owner ruling 2 of 2026-09-25)
  requires it, with `platform:messaging:router:view`, which the built-in
  `platform:application-service` role holds. Ship this release to apps
  **before** any router enforces auth. `Router`'s constructor takes an
  optional Guzzle client for the router's origin as a second argument.
- `OutboxManager::createDispatchJob` / `createDispatchJobs`: the outbox
  payload now carries `id`, the outbox row's own id (the id the method
  returns). The platform honours a supplied dispatch-job id, so a batch the
  outbox processor resends after losing the platform's answer is recognised
  instead of creating the job a second time. No signature changed.
- Licence: MIT, as published (owner decision of 2026-09-25, which reverses
  the earlier move to MPL-2.0). `LICENSE` is the published file.

### Fixed: calls against Go's platform API
The hand-written resources now send and read what Go's platform API
(`api/openapi.lock.json`) expects, checked against Go's spec and handlers.
The Rust platform answers the calls changed here with the same shapes.

Return types changed. Each old type was built from a body the platform
never sends, so the call threw (`Undefined array key` / `TypeError`):
- `applications()->create()` returns the new id (`string`), like every
  other create. The platform answers `{id}`.
- `applications()->update()`, `clients()->update()`,
  `connections()->update()`, `applications()->enableForClient()` /
  `disableForClient()`, `dispatchPools()->archive()` / `suspend()` /
  `activate()` and `subscriptions()->pause()` / `resume()` return `void`.
  The platform answers 204; call `get()` for the record.
- `applications()->provisionServiceAccount()` returns a
  `ProvisionServiceAccountResult`: `{message, serviceAccount: {principalId,
  name, oauthClient: {id, clientId, clientSecret}}}`, as the platform nests
  it. The one-time client secret was lost before.

Behaviour fixed, signatures unchanged:
- `applications()->getServiceAccount()`: the platform has no
  `GET …/service-account`. It now reads the application and then
  `GET /api/service-accounts/{serviceAccountId}`, and throws a
  `FlowCatalystException` with code 404 when the application has none.
- `applications()->listRoles()`: the platform returns role names
  (`{roles: [string]}`); each comes back as an `ApplicationRole` with only
  `code` set. Deprecated for the new `listRoleNames()`.
- `applications()->listClients()` reads `items` and each row's
  `configJson` (into `ClientConfig::$config`). It was always empty.
- `connections()->update()` and `eventTypes()->update()` always send
  `name`, which the platform requires: when the request's `name` is null,
  the current name is read first and sent unchanged.
- `principals()->findByEmail()` and `list(email:)` send the search as `q`
  (the platform has no `email` filter and returned every principal).
  `findByEmail()` still returns only an exact, case-insensitive match.
- `auditLogs()->list()` pages by cursor: new `after`, `applicationIds`
  and `clientIds` arguments, and `AuditLogList::$hasMore` / `$nextCursor`.
  `clientId` is sent as `clientIds`. `from`, `to` and `page` have no
  effect on the platform and are deprecated.
- `scheduledJobs()->list()` / `listInstances()` read `total_pages` into
  `totalPages` (it was always 0).
- `EventType::$event` reads the platform's `eventName` (it was always
  empty). `Permission` reads `name` and `category`, and parses
  `application` / `context` / `aggregate` / `action` from the permission
  string when the platform does not send them (they were always empty).
  `Role::$shortName` is derived from `name` when absent.
- `router()->inPipeline()`: the router puts `poolCode` / `queueId` at the
  top level. The result now also carries them as `detail`, the shape the
  docblock promised.
- List filters the platform does not have: `clients()->list($status)`,
  `connections()->list(serviceAccountId:)` and `roles()->list(...)` are
  now applied to the returned rows (they had no effect, so every row came
  back). `processes()->list(search:)` and `scheduledJobs()->listInstances()`
  `triggerKind` / `from` / `to` are documented as having no effect.

Deprecated: `eventTypes()->archive()`. The platform has no archive for
event types; it sends `DELETE`, which deletes. Use the new `delete()`.
`applications()->updateClientConfig()`: only the Rust platform serves that
`PUT`; use `enableForClient()` / `disableForClient()` and the new
`getClientConfig()`.

Added: `applications()->getClientConfig()`, `listRoleNames()`, `list()`'s
`type` filter; `eventTypes()->delete()`, `list()`'s `subdomain` /
`aggregate` filters; `principals()->list()`'s `q`;
`UpdateEventTypeRequest::$clientScoped`;
`UpdateConnectionRequest::$applicationCode`; response members the platform
sends (`ClientConfig::$createdAt` / `$updatedAt`, `ServiceAccount::$principalId`
/ `$oauthClientId`, `Connection::$applicationCode` / `$source`,
`EventType::$source` / `$clientId`, `AuditLog::$operationJson`). All are
optional trailing arguments.

## 0.10.26 and earlier

Released from `flowcatalyst-go` (`clients/laravel-sdk`); see that repo's
history and its `laravel-sdk/v*` tags.
