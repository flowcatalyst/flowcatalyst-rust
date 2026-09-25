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
