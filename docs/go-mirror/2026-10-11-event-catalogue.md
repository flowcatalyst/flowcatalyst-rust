# Event catalogue drift closed, mirrored (2026-10-11)

Go's F9 (`flowcatalyst-go` `8ddaed4`, docs `5c9939d`; owner ruling "catalogue please"): every
event type a platform use case emits is in the seeded catalogue, each with a 1.0 JSON schema of
the data it emits, and the drift test's known-gaps list is empty. Rust's drift test
(`emitted_catalogue_test`, from the second batch's F3, `973801fc`) listed 33 emitted codes the
catalogue lacked; all 33 are catalogued now. Owner ruling for Rust: there are no stale
`platform:iam:client:*` rows to clean up, so nothing beyond migration 068 is needed for those.

Tests named `*_test::` are in `crates/fc-platform/tests/it/` (`cargo test -p fc-platform --test it
<module>::`); none needs a database.

## Summary

| Item | Rust commit |
|---|---|
| Fourteen events wrote their envelope into `data` (found while verifying the schemas) | `df9c8dbf` |
| The 33 uncatalogued codes catalogued; `KNOWN_UNCATALOGUED` empty | `c821a316` |

The catalogue is 159 entries (was 126). Nothing is renamed or dropped.

## How it reaches existing databases

As in Go: `fc-server` and `fc-dev` run `shared::database::seed_platform_event_types` on every
start (`EventTypeRepository::seed_catalogue`), which inserts a missing code with its 1.0 schema and
refreshes the name of an existing one. An existing database gains the 33 rows on the next start;
no migration (no code changes). `POST /bff/event-types/sync-platform` lists the same definitions.

## The envelope in `data`

- **Found**: fourteen event structs had `#[serde(flatten)]` on their `metadata: EventMetadata`
  instead of `#[serde(skip)]`, so the unit of work persisted `event_id`, `event_type`,
  `spec_version`, `source`, `subject`, `time`, `execution_id`, `correlation_id`, `principal_id`,
  `message_group` in `msg_events.data` beside the payload (snake_case; the struct's
  `rename_all` does not reach a flattened struct). For the portal identity events the payload's
  `source` collided with the envelope's. Go's `ToDataJSON` carries no envelope, and
  `DomainEvent`'s own contract says the metadata is skipped. Affected:
  `platform:portal:{app:created,app:updated,app:deleted,identity:ensured,identity:status-set,
  identity:deleted,identity:app-granted,identity:app-revoked}`,
  `platform:iam:user:developer-credential-{set,revoked}`,
  `platform:messaging:dispatch-job:{cancelled,completed}`,
  `platform:messaging:dispatch-jobs:resent`, `platform:admin:connection:synced` (already
  catalogued, so its schema rejected what Rust sent),
  `platform:admin:email-domain-mapping:provider-changed`, `platform:admin:app-docs:synced`,
  `platform:iam:serviceaccount:token-minted`.
- **Fix**: `#[serde(skip)]` on all fourteen. None of these structs is an HTTP response body.
- **Wire impact**: the `data` of new events of those types is the payload only, as Go's. Rows
  already written keep the envelope keys.
- **Test**: `event_data_convention_test` (every event struct's metadata field is
  `#[serde(skip)]`; it failed on the old `app_docs/operations/sync.rs`).
- **Java mirror**: check the same types' `data` carries no envelope.

## The 33 codes

### Same code as Go (14): Go's name and schema

| Code | Name | Data |
|---|---|---|
| `platform:iam:user:developer-credential-set` | User Developer Credential Set | `{userId}` |
| `platform:iam:user:developer-credential-revoked` | User Developer Credential Revoked | `{userId}` |
| `platform:admin:email-domain-mapping:provider-changed` | Email Domain Mapping Provider Changed | `{mappingId, emailDomain, fromIdentityProviderId, toIdentityProviderId}` |
| `platform:messaging:dispatch-job:cancelled` | Dispatch Job Cancelled | `{dispatchJobId}` |
| `platform:messaging:dispatch-job:completed` | Dispatch Job Completed | `{dispatchJobId}` |
| `platform:messaging:dispatch-jobs:resent` | Dispatch Jobs Resent | `{ids}` |
| `platform:portal:app:{created,updated,deleted}` | Portal App Created / Updated / Deleted | `{portalAppId, clientId, code, name}` |
| `platform:portal:identity:ensured` | Portal Identity Ensured | `{identityId, clientId, email, created, source, portalAppId?, portalAppCode?}` |
| `platform:portal:identity:status-set` | Portal Identity Status Set | `{identityId, clientId, status}` |
| `platform:portal:identity:deleted` | Portal Identity Deleted | `{identityId, clientId, email}` |
| `platform:portal:identity:app-granted` | Portal Identity App Granted | `{identityId, clientId, portalAppId, portalAppCode, source}` |
| `platform:portal:identity:app-revoked` | Portal Identity App Revoked | `{identityId, clientId, portalAppId, portalAppCode}` |

### Emitted by Rust only (19): no Go counterpart, named and described here

Go has no event for the app-docs sync, the permission catalogue writes or the admin token mint
(Rust routes them through a unit of work); the function registry and the audit redaction follow
Java (`FunctionEvents`, `AuditLogsRedacted`, `ServiceAccountEvents.TOKEN_MINTED`), which does not
catalogue them either. Names follow the catalogue's style (Go's `groupAs` labels for the function
aggregates).

| Code | Name | Data |
|---|---|---|
| `platform:admin:app-docs:synced` | App Docs Synced | `{applicationCode, created, updated, deleted, syncedCodes}` |
| `platform:admin:audit-log:redacted` | Audit Log Redacted | `{scanned, redacted}` |
| `platform:admin:permission:defined` | Permission Defined | `{permissionId, permission, description?}` |
| `platform:admin:permission:deleted` | Permission Deleted | `{permissionId, permission}` |
| `platform:iam:serviceaccount:token-minted` | Serviceaccount Token Minted | `{serviceAccountId, code}` |
| `platform:function:function:created` | Function Created | `{functionId, address, applicationId, clientId?, runtime}` |
| `platform:function:function:updated` | Function Updated | `{functionId, address, description?, status}` |
| `platform:function:function:deleted` | Function Deleted | `{functionId, address}` |
| `platform:function:version:published` | Function Version Published | `{functionId, address, versionId, version, digest, pool, signerIssuer?, signerSubject?}` |
| `platform:function:version:ready` | Function Version Ready | `{functionId, address, versionId, version, hostId}` |
| `platform:function:version:retired` | Function Version Retired | `{functionId, address, versionId, version}` |
| `platform:function:alias:changed` | Function Alias Changed | `{functionId, address, alias, versionId, version, previousVersionId?}` |
| `platform:function:alias:removed` | Function Alias Removed | `{functionId, address, alias, versionId, version}` |
| `platform:function:policy:updated` | Function Policy Updated | `{owner, signerCount}` |
| `platform:function:config:updated` | Function Config Updated | `{functionId, address, keys}` |
| `platform:function:secret:{set,deleted}` | Function Secret Set / Deleted | `{functionId, address, key}` |
| `platform:function:domain:{claimed,released}` | Function Domain Claimed / Released | `{domainId, hostname, owner}` |

**Verified**: `event_catalogue_schema_test` validates real output against the catalogue schema of
its type: every one of the 33 codes built from its event struct (optional fields both present and
absent), plus all 17 cases of Java's function golden data (which the Rust function events equal,
`function_events_golden_test`). The validator's own negative cases are in the same module. Unlike
Go (which checked 70 of its 79 once and did not commit the check), this check is committed.

## Codes that differ between the ports (not guessed, for the owner)

- **Go's 15 function codes are not catalogued here.** Go's runner emits
  `platform:admin:function:{created,updated,deleted,promoted}`,
  `platform:admin:function-version:{published,retired}`,
  `platform:admin:function-alias:{set,deleted}`, `platform:admin:function-setting:{set,deleted}`,
  `platform:admin:function-route:{set,deleted}`, `platform:admin:function-schedule:wired`,
  `platform:admin:function-domain:{claimed,released}`. Rust's registry is Java's and emits the 14
  `platform:function:*` codes above, with different data. Nothing in Rust emits Go's codes, so
  they are not catalogued (a subscription to one would never fire). Which set is canonical is the
  owner's call.
- **Rust-only codes** (the 19 above): Go would need its own events to emit them.
- The other 50 of Go's 79 were already catalogued here (the earlier batch), same codes.

## Differences left as they are (existing entries, not renamed)

- **Names**: Rust's seeder names `platform:admin:oauth-client:*` "Oauth Client …" and
  `platform:admin:idp-role-mapping:*` "Idp Role Mapping …" (its plain `group`), Go "OAuth Client
  …" and "IdP Role Mapping …" (`groupAs`). Both seeders refresh names on start, so on a database
  both run against the last one to start wins. Aligning is a one-line `group_as` change each
  (the helper exists now); not done, per the brief.
- **Schemas** (both describe the emitted data; the seeder never replaces an existing 1.0 schema):
  `platform:admin:roles:synced` `syncedCodes` admits `null` here, not in Go;
  `platform:admin:oauth-client:secret-rotated` `previousSecretExpiresAt` is `["string","null"]`
  here, Go `string` with `format: date-time`.
- **As in Go, not fixed**: the existing `platform:admin:dispatch-pool:{created,updated,archived,
  deleted}` schemas name `dispatchPoolId` while the events emit `poolId` (the older Go-verbatim
  schemas were not audited); catalogued codes nothing emits (e.g. `platform:iam:role:*` beside
  `platform:admin:role:*`) stay.

## Java mirror

Catalogue the same codes the Java platform emits (it emits the 14 `platform:function:*`, the
audit redaction and the token mint like Rust); check the envelope stays out of `data`.
