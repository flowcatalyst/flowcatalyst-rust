# Branded (value) types — deferred backlog

Owner, 2026-09-28: "use branded types more … lots of fairly basic validation
like not null going on." Deferred; do after platform-uniformity phase 2
(authorization placement) and build-speed part B (crate split), which touch
every command and entity.

## Where the Rust platform is (measured 2026-09-28, fc-platform)

- Real value types: `EventTypeCode`, `ProcessCode`, `SecretValue` (plus a few
  query markers). Everything else is a checked `String`.
- ~375 raw `String` / `Option<String>` id fields in commands and entities.
- ~286 hand-written blank/empty checks in use cases.
- Java and Go have the same shape (Java also parses `EventTypeCode`).

## Priority

1. **Typed ids per aggregate** — `ClientId`, `PrincipalId`, `ServiceAccountId`,
   `ApplicationId`, … each parsing its TSID prefix (`clt_`, `prn_`, `sac_`).
   Makes "passed the wrong id" a compile error. (Real precedent: the
   service-account events carried the principal's `prn_` id instead of the
   account's `sac_` id.) Pairs with phase 2's `Caller` checks
   (`caller.can_reach(ClientId)`).
2. **Formatted codes** — application code, role name, permission string, pool
   code, connection code, email, endpoint URL, cron expression, hostname.
   Parse once, never re-check.
3. **Non-blank names/descriptions** — replaces most of the blank checks.

## Rules (from the `EventTypeCode` pilot)

- Parse where the handler builds the command, **after** its permission check
  (Go answers 403 before any 400).
- Reproduce Go's exact error codes, messages and order; HTTP DTOs stay
  `String` so a bad value still answers the specific code (e.g.
  `INVALID_CODE_FORMAT`), not a generic extractor 400.
- JSON and DB shapes unchanged: `#[serde(transparent)]`,
  `#[sqlx(transparent)]`.
- Hand-written newtypes are fine; `nutype` only where it reproduces the error
  codes exactly.
- One aggregate group per commit, gated by the API parity harness and the
  event persistence snapshots.

## Per language

- **Rust:** private-field newtypes with `parse` constructors (zero cost). No
  null in Rust, so the checks being replaced are blank/format checks.
- **Java:** records with compact constructors
  (`record ClientId(String value) { ClientId { Ids.require("clt_", value); } }`),
  NullAway + JSpecify for not-null; Valhalla value classes later.
- **Go:** `type ClientID string` protects nothing (any string converts). Use a
  struct with an unexported field plus a constructor
  (`type ClientID struct{ v string }`, `NewClientID(s) (ClientID, error)`),
  and guard the zero value (`IsZero` + a linter rule). Java and Go work would
  go on branches in their own repos, like the 2026-09-28 fixes.
