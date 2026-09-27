# Changelog — io.flowcatalyst:flowcatalyst-sdk (Java)

Releases are tagged `java-sdk/vX.Y.Z`. There is no registry or split repo
yet (see `docs/sdks.md` in the FlowCatalyst Rust repo). The version line
continues from the releases cut in `flowcatalyst-go` (last: 0.0.10).

## Unreleased (next: 0.0.11)

First release built from the FlowCatalyst Rust repo. It is 0.0.10 as
published, plus the additions below.

### Added
- Audit redaction: `CreateAuditLogDto` redacts `operationData` before it is
  serialised into the outbox. Password-, secret- and token-shaped fields
  become `***`; the rules are in `io.flowcatalyst.sdk.outbox.AuditRedaction`,
  the same rule the TypeScript, Laravel and Rust SDKs apply. The shared
  vectors are in `src/test/resources/audit-redaction-vectors.json`.
- `CreateAuditLogDto.withOperationData(Map, Set<String> maskedFields)`: an
  overload for extra top-level fields to mask. The one-argument form is
  unchanged.

### Changed
- `client.router().inPipeline()` / `inPipelineBatch()` send the platform
  bearer token to the router (with the same one-shot refresh on 401). Today's
  routers ignore it; a router that enforces platform tokens (owner ruling 2
  of 2026-09-25) requires it. `Transport.rawAuthenticated` is new;
  `rawUnauthenticated` stays for callers.

### Fixed: calls against Go's platform API
The resources already used models generated from Go's spec; these calls
still sent or read something Go's API does not. No signature changed.
- `applications().getServiceAccount()`: the platform has no
  `GET …/service-account`. It now reads the application and then
  `GET /api/service-accounts/{serviceAccountId}`, and throws
  `FlowCatalystException` with `SdkError.NotFound` when the application has
  none.
- `eventTypes().update()` and `connections().update()` always send `name`,
  which the platform requires (a null name was sent as `"name": null` and
  rejected): when it is null, the current name is read first and set on the
  request.
- `roles().create()` and `scheduledJobs().create()` send the platform's
  required booleans: a null `clientManaged` / `concurrent` /
  `tracksCompletion` is set to `false` on the request.
- `auditLogs().list()` sends `applicationIds` / `clientIds`, and
  `principals().list()` sends `roles`, as one comma-separated value. The
  platform reads each as a single CSV value, so a repeated parameter kept
  only the first id.
- `router().inPipeline()`: the router puts `poolCode` / `queueId` at the
  top level, so `detail()` was always null. `InPipelineCheckResponse` gains
  `poolCode` and `queueId`, and `detail()` is built from them when the router
  sends no `detail`. The three-argument constructor is kept.

### Deprecated
- `eventTypes().archive()`: the platform has no archive for event types; it
  sends `DELETE`, which deletes. Use the new `eventTypes().delete()`.
- `applications().updateClientConfig()`: only the Rust platform serves that
  `PUT`. Use `enableForClient()` / `disableForClient()` and the new
  `getClientConfig()`.

### Added
- `applications().getClientConfig(id, clientId)`,
  `applications().list(type, active)`, `dispatchPools().archive(id)`
  (`POST …/archive`), `eventTypes().delete(id)`,
  `scheduledJobs().getByCode(code, clientId)`.
- `ScheduledJobsResource.InstanceFilters`: documented that the platform
  ignores `triggerKind`, `from` and `to`.

## 0.0.10 and earlier

Released from `flowcatalyst-go` (`clients/java-sdk`); see that repo's
history and its `java-sdk/v*` tags.
