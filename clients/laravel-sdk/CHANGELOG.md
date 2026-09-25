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
- Licence: MPL-2.0 (was MIT), by owner ruling. MPL-2.0 is file-level
  copyleft, so applications that depend on the SDK are not affected.

## 0.10.26 and earlier

Released from `flowcatalyst-go` (`clients/laravel-sdk`); see that repo's
history and its `laravel-sdk/v*` tags.
