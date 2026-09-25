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

## 0.0.10 and earlier

Released from `flowcatalyst-go` (`clients/java-sdk`); see that repo's
history and its `java-sdk/v*` tags.
