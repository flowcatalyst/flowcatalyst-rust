# Changelog — @flowcatalyst/sdk (TypeScript)

Releases are tagged `typescript-sdk/vX.Y.Z` in the source repo. The split
workflow mirrors the source to `github.com/flowcatalyst/typescript-sdk`,
commits a built `dist/` there, and tags it `vX.Y.Z`. The package is not on
the npm registry; consumers install it from that git repo. The version line
continues from the releases cut in `flowcatalyst-go` (last: 0.11.27); see
`docs/sdks.md` in the FlowCatalyst Rust repo.

## Unreleased (next: 0.11.28)

First release built from the FlowCatalyst Rust repo. It is 0.11.27 as
published, plus the additions below. No export was removed or changed
incompatibly.

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

### Changed
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
- Licence: MPL-2.0 (was Apache-2.0), by owner ruling. MPL-2.0 is
  file-level copyleft, so applications that depend on the SDK are not
  affected.

## 0.11.27 and earlier

Released from `flowcatalyst-go` (`clients/typescript-sdk`); see that repo's
history and its `typescript-sdk/v*` tags.
