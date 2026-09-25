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

## 0.0.10 and earlier

Released from `flowcatalyst-go` (`clients/java-sdk`); see that repo's
history and its `java-sdk/v*` tags.
