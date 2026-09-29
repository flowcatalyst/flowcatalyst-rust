# Typed ids and state types: what the Rust platform changed (for the Go agent)

Rust commits, oldest first (all local to `main`, 2026-09-29/30):
`a105b8a1` reset approvals, `6fb452e4` `Id<K>` + service accounts,
`1e346b72` `Principal.id` + portal tokens, `7135d682` `Client.id`,
`42d3ac17` `Principal.client_id`, then the `client_id` fields below.

The goal is that passing a `PrincipalId` where a `ClientId` belongs is a
compile error, and that a status which carries data cannot be built without it.
Wire and database shapes did not change.

## Go shape

`type ClientID string` protects nothing (any string converts). Use a struct
with an unexported field and a constructor, and guard the zero value:

```go
type ClientID struct{ v string }
func ParseClientID(s string) (ClientID, error) // checks the "clt_" prefix
func (id ClientID) String() string
func (id ClientID) IsZero() bool
```

JSON and SQL stay a plain string: implement `MarshalJSON`/`UnmarshalJSON` and
`driver.Valuer`/`sql.Scanner` (a NULL column scans to a nullable wrapper or
`*ClientID`). One generic type is fine (`ID[K]`) if Go generics keep the
compiler errors readable; if not, one struct per kind.

## What became typed

| Entity | Field | Now |
|---|---|---|
| `Principal` | `id` | `PrincipalId` (`prn_`) |
| `Principal` | `client_id` | `Option<ClientId>` (`clt_`) |
| `Client` | `id` | `ClientId` |
| `ServiceAccount` | `id` | `PrincipalId`: it is the SERVICE **principal's** id, not the account's |
| `ServiceAccount` | `service_account_table_id` | `Option<AccountRow>` (below) |
| `Subscription`, `Connection`, `DispatchPool`, `ScheduledJob` | `client_id` | `Option<ClientId>` |

### The service-account seam (do not skip)

An account has two ids: its own `sac_` id (`iam_service_accounts.id`) and its
SERVICE principal's `prn_` id. Older accounts have **no separate row id**: their
`iam_principals.service_account_id` equals the principal's own `prn_` id (8 of 13
service principals in the Rust nonprod database, 0 in production). Model it, do
not coerce it:

```rust
enum AccountRow { Own(ServiceAccountId), SharedWithPrincipal }
```

`account_id()` returns the row's id string either way. Mapping the legacy case
to "no row" would silently skip updates and deletes of that row.

## What stays a string (deliberately)

- Commands, domain events, audit rows and HTTP DTOs.
- The audit actor id (`"system"`, `"anonymous"` and `""` are legal).
- `assigned_clients` and the caller's accessible-client claims (they contain the
  `"*"` wildcard).
- `client_id` on events, dispatch jobs, audit logs, scheduled-job instances and
  logs: high-volume, and set from external SDK payloads.
- **Naming trap:** `oauth_clients.client_id`, `iam_authorization_codes.client_id`
  and the `portal_*` columns are **OAuth** client ids, not tenants. Do not type
  them as `ClientID`.

## Where a string becomes an id

- Parse once, at the use-case boundary, after the permission check (403 before
  400). Rust answers a malformed client id with 400 `INVALID_CLIENT_ID`
  (previously a database foreign-key failure). Decide whether Go matches that.
- Client-association takes the id from the `Client` the repository returns, so
  its `CLIENT_NOT_FOUND` is unchanged.
- Stored rows are parsed on read; a wrong-prefix value is a loud read error
  naming table, column, value and row id, not a default.

## Data checked before making the parse strict

Read-only prefix scans, nonprod and production (Rust databases, Go-written data):
every `iam_principals.id`, `tnt_clients.id` and admin-plane `client_id` column
matched its prefix (`iam_principals` 455/2,137 rows, `tnt_clients` 35/5).
Only the legacy service-account links above did not. The `fnr_*` (function
registry) tables are not deployed in either database, so their `client_id` is
unscanned. Run the same scan against Go's own environments before making Go's
parse strict.

## Other changes in the same series

- **Reset approvals:** `status` is a typed enum (Go already has
  `resetapproval.Status`), and a decision carries its decider and time:
  `Pending | Approved{by,at} | Denied{by,at} | Expired`. A decided row without a
  decider is a corrupt row. The guarded `UPDATE` is unchanged.
- **Portal set-status:** an unparseable status used to default to ACTIVE in
  `execute`; it now returns `STATUS_INVALID`. Check whether Go does the same.
- **Portal tokens:** the token endpoint built a fake `Principal` from a `ptu_`
  portal identity to feed the token generators. The generators now take the
  portal identity directly. If Go has the same shortcut, a typed `PrincipalID`
  will not accept a `ptu_` id.
