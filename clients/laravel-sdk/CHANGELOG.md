# Changelog — flowcatalyst/laravel-sdk

Releases are tagged `laravel-sdk/vX.Y.Z` in the source repo and mirrored to
`github.com/flowcatalyst/laravel-sdk` as `vX.Y.Z` (apps install it as a
Composer `vcs` repository). The version line continues from the releases cut
in `flowcatalyst-go` (last: 0.10.26); see `docs/sdks.md` in the FlowCatalyst
Rust repo.

## Unreleased (next: 0.10.27)

First release built from the FlowCatalyst Rust repo. It is 0.10.26 as
published, plus the additions below. No public API was removed or changed
incompatibly.

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
- Licence: MPL-2.0 (was MIT), by owner ruling. MPL-2.0 is file-level
  copyleft, so applications that depend on the SDK are not affected.

## 0.10.26 and earlier

Released from `flowcatalyst-go` (`clients/laravel-sdk`); see that repo's
history and its `laravel-sdk/v*` tags.
