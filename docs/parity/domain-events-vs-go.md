# Domain events: Rust vs Go

Status as of 2026-09-25, branch `feat/go-events`. Go (`../flowcatalyst-go`) is the reference: production rows in
`msg_events` and `aud_logs` were written by Go, and subscribers (and the apps) match on the event type and read
`data`. Owner decisions 2026-09-25, "Follow-ups … Cutover blocker".

Go sources: each domain's `internal/platform/*/operations/events.go` (type constants, `Source`, `subjectFor`,
`groupFor`, `ToDataJSON`), `internal/platform/shared/platformsink/sink.go` (the row writer),
`pkg/fcsdk/usecaseop` (`Sync`), `internal/platform/seed/event_types.go` (the catalogue).

## 1. What every event row now carries (all events)

| Column | Rust before | Go / Rust now |
|---|---|---|
| `msg_events.data` | the whole event struct, including the envelope in snake case (`event_id`, `event_type`, `spec_version`, `source`, `subject`, `time`, `execution_id`, `correlation_id`, `causation_id`, `principal_id`, `message_group`) plus the payload | the payload only (`ToDataJSON`); `{}` for an empty payload |
| `correlation_id`, `causation_id`, `message_group` | `''` when empty | `NULL` when empty (`nullIfEmpty`) |
| `aud_logs.id` | untyped TSID | `aud_` TSID (`tsid.Generate(tsid.AuditLog)`) |
| `aud_logs.entity_type` / `entity_id` | derived from the subject | unchanged rule, but the subjects are now Go's, so the values are too (e.g. `Principal`, `Identityprovider`, `Emaildomainmapping`, `Passkey`, `Dispatchpools`) |
| sync operations | one rollup event | a created/updated/deleted (or archived) event **per synced row**, each with its audit row, then the rollup, in one transaction (Go's `usecaseop.Sync`) |

Unchanged and already equal: `spec_version` `1.0`, `deduplication_id` `{type}-{eventId}`, `context_data`
`[{principalId},{aggregateType}]`, `client_id` NULL, event id an untyped 13-char TSID.

## 2. Type codes renamed (37)

| Rust before | Go / Rust now |
|---|---|
| `platform:iam:client:created` | `platform:admin:client:created` |
| `platform:iam:client:updated` | `platform:admin:client:updated` |
| `platform:iam:client:activated` | `platform:admin:client:activated` |
| `platform:iam:client:suspended` | `platform:admin:client:suspended` |
| `platform:iam:client:deleted` | `platform:admin:client:deleted` |
| `platform:iam:client:note-added` | `platform:admin:client:note-added` |
| `platform:iam:role:created` | `platform:admin:role:created` |
| `platform:iam:role:updated` | `platform:admin:role:updated` |
| `platform:iam:role:deleted` | `platform:admin:role:deleted` |
| `platform:iam:roles:synced` | `platform:admin:roles:synced` |
| `platform:iam:anchor-domain:created` | `platform:admin:anchor-domain:created` |
| `platform:iam:anchor-domain:updated` | `platform:admin:anchor-domain:updated` |
| `platform:iam:anchor-domain:deleted` | `platform:admin:anchor-domain:deleted` |
| `platform:iam:auth-config:created` | `platform:admin:auth-config:created` |
| `platform:iam:auth-config:updated` | `platform:admin:auth-config:updated` |
| `platform:iam:auth-config:deleted` | `platform:admin:auth-config:deleted` |
| `platform:iam:idp-role-mapping:created` | `platform:admin:idp-role-mapping:created` |
| `platform:iam:idp-role-mapping:deleted` | `platform:admin:idp-role-mapping:deleted` |
| `platform:admin:idp:created` | `platform:admin:identity-provider:created` |
| `platform:admin:idp:updated` | `platform:admin:identity-provider:updated` |
| `platform:admin:idp:deleted` | `platform:admin:identity-provider:deleted` |
| `platform:admin:edm:created` | `platform:admin:email-domain-mapping:created` |
| `platform:admin:edm:updated` | `platform:admin:email-domain-mapping:updated` |
| `platform:admin:edm:deleted` | `platform:admin:email-domain-mapping:deleted` |
| `platform:admin:config:property-set` | `platform:admin:platform-config:property-set` |
| `platform:admin:config-access:granted` | `platform:admin:platform-config:access-granted` |
| `platform:admin:config-access:revoked` | `platform:admin:platform-config:access-revoked` |
| `platform:admin:scheduledjob:created` | `platform:admin:scheduled-job:created` |
| `platform:admin:scheduledjob:updated` | `platform:admin:scheduled-job:updated` |
| `platform:admin:scheduledjob:paused` | `platform:admin:scheduled-job:paused` |
| `platform:admin:scheduledjob:resumed` | `platform:admin:scheduled-job:resumed` |
| `platform:admin:scheduledjob:archived` | `platform:admin:scheduled-job:archived` |
| `platform:admin:scheduledjob:deleted` | `platform:admin:scheduled-job:deleted` |
| `platform:admin:scheduledjob:firedManually` | `platform:admin:scheduled-job:fired-manually` |
| `platform:iam:passkey:registered` | `platform:admin:passkey:registered` |
| `platform:iam:passkey:revoked` | `platform:admin:passkey:revoked` |
| `platform:iam:user:logged-in-with-passkey` | `platform:admin:passkey:authenticated` |

Re-check of `69e69f47` (OAuth client events "as Java"): the seven `platform:admin:oauth-client:*` types, source
`platform:admin`, subject `platform.oauthclient.{id}`, group `platform:oauthclient:{id}` and payloads are exactly
Go's (`auth/operations/events.go`). No change needed.

Unchanged type codes (already Go's): `platform:iam:user:*` (except as above), `platform:iam:principals:synced`,
`platform:iam:serviceaccount:*`, `platform:iam:application:*`, `platform:iam:client:applications-updated`,
`platform:admin:cors:*`, `platform:admin:connection:*`, `platform:admin:dispatch-pool:*`,
`platform:admin:dispatch-pools:synced`, `platform:admin:eventtype:*`, `platform:admin:eventtypes:synced`,
`platform:admin:process:*`, `platform:admin:processes:synced`, `platform:admin:scheduledjobs:synced`,
`platform:admin:subscription:*`, `platform:admin:oauth-client:*`, `platform:developer:application-openapi:synced`.

## 3. Source, subject and message group changes

| Events | Rust before | Go / Rust now |
|---|---|---|
| client | source `platform:iam` | `platform:admin` |
| application (all) | source `platform:application` | `platform:iam` |
| `client:applications-updated` | source `platform:client` | `platform:iam` |
| service account (all) | source `platform:serviceaccount` | `platform:iam` |
| role, anchor-domain, auth-config, idp-role-mapping | source `platform:iam` | `platform:admin` |
| user (all except logged-in) | subject/group `platform.user.{id}` / `platform:user:{id}` | `platform.principal.{id}` / `platform:principal:{id}` (logged-in keeps `platform.user.*`, as Go) |
| identity provider | `platform.idp.{id}` / `platform:idp:{id}` | `platform.identityprovider.{id}` / `platform:identityprovider:{id}` |
| email domain mapping | `platform.edm.{id}` / `platform:edm:{id}` | `platform.emaildomainmapping.{id}` / `platform:emaildomainmapping:{id}` |
| platform-config access | `platform.platformconfigaccess.{id}` | `platform.platformconfig.{id}` (same as a property, as Go) |
| passkey | source `platform:iam`, `platform.webauthncredential.{id}` | source `platform:admin`, `platform.passkey.{id}` / `platform:passkey:{id}` |
| subscription | group `platform:admin:subscription:{id}` | `platform:subscription:{id}` |
| event type (all but synced) | group `platform:eventtype:{id}` | no group (NULL): Go never sets it |
| `principals:synced` | `platform.application.{app}` | `platform.principals.{app}` / `platform:principals:{app}` |
| `roles:synced` | `platform.application.{app}` | subject `platform.roles` (fixed), group `platform:roles:{app}` |
| `eventtypes:synced` | `platform.application.{app}` | `platform.eventtypes.{app}` / `platform:eventtypes:{app}` |
| `dispatch-pools:synced` | `platform.application.{app}` | `platform.dispatchpools.{app}` / `platform:dispatchpools:{app}` |
| `processes:synced` | `platform.application.{app}` | `platform.processes.{app}` / `platform:processes:{app}` |
| `subscription:synced` | `platform.application.{app}` | `platform.subscriptions.{app}` / `platform:subscriptions:{app}` |
| `scheduledjobs:synced` | `platform.scheduledjobs.synced.sync:{scope}:{client}` | `platform.scheduledjobs.synced.{app}` / `platform:scheduledjobs:{app}` |

## 4. `data` payload changes (beyond dropping the envelope)

| Event | Rust before | Go / Rust now |
|---|---|---|
| client created | + `description` | `{clientId, name, identifier}` |
| client updated | `name?`, `description?` | `{clientId, name}` (name after the update) |
| client activated | + `previousStatus` | `{clientId}` |
| client deleted | + `name` | `{clientId, identifier}` |
| client note-added | + `author` | `{clientId, category, text}` |
| application created | + `applicationType` | `{applicationId, code, name}` |
| application updated | `name?`, `description?` | `{applicationId, name}` |
| application activated / deactivated | + `code` | `{applicationId}` |
| application deleted | + `name` | `{applicationId, code}` |
| client applications-updated | lists always arrays | `enabledApplicationIds`, `enabledAdded`, `disabledRemoved` are `null` when empty (Go appends to nil) |
| service account created | + `applicationId`, `clientIds` | `{serviceAccountId, code, name}` |
| service account updated | `name?`, `description?`, `clientIdsAdded`, `clientIdsRemoved` | `{serviceAccountId, name}` |
| service account deactivated | + `code` | `{serviceAccountId}` |
| role created | `{roleId, code, displayName, applicationCode, permissions}` | `{roleId, name}` |
| role updated | `{roleId, displayName?, description?, permissionsAdded, permissionsRemoved}` | `{roleId, name}` |
| role deleted | `{roleId, code}` | `{roleId, name}` |
| roles synced | `{applicationCode, created, updated, deleted, syncedNames}` | `{created, updated, removed, total, applicationCode, syncedCodes}` (last two omitted when empty) |
| user created | `{principalId, email, emailDomain, name, scope, clientId?, isAnchorUser}` | `{principalId, email}` |
| user updated | `name?`, `email?` | `{principalId, name}` |
| user deactivated | + `reason?` | `{principalId}` |
| user deleted | `{principalId}` | `{principalId, email}` |
| password-reset-completed | + `email` | `{principalId}` |
| auth-config created | + `configType` | `{authConfigId, emailDomain}` |
| idp-role-mapping created | `{idpRoleMappingId, idpRole: "type:name", mappedRole}` | `{mappingId, idpType, idpRoleName, platformRoleName}` |
| idp-role-mapping deleted | `{idpRoleMappingId}` | `{mappingId, idpRoleName}` |
| identity-provider created | `{idpId, code, name, idpType}` | `{identityProviderId, code}` |
| identity-provider updated | `{idpId, name?}` | `{identityProviderId, code}` |
| identity-provider deleted | `{idpId, code}` | `{identityProviderId, code}` |
| email-domain-mapping created | + `identityProviderId`, `scopeType` | `{mappingId, emailDomain}` |
| connection created | + `serviceAccountId`, `clientId?` | `{connectionId, code, name}` |
| connection updated | `{connectionId, code, name?, status?}` | `{connectionId, name}` |
| connection deleted | + `clientId?` | `{connectionId, code}` |
| dispatch pool (all) | id key `dispatchPoolId`; created + `clientId?`; updated `name?, rateLimit?, concurrency?` | id key `poolId`; created `{poolId, code, name}`; updated `{poolId, name}` |
| event type updated | `name?`, `description?` | `{eventTypeId, name, description?}` (after the update) |
| schema-added | `{eventTypeId, version, mimeType, schemaType}` | `{eventTypeId, specVersion}` |
| schema-finalised / -deprecated | `version` | `specVersion` |
| eventtypes synced | + `schemasCreated`, `schemasUpdated`, `schemasUnchanged` | `{applicationCode, created, updated, deleted, syncedCodes}` |
| platform-config property-set | + `scope`, `clientId?`, `valueType`, `wasCreated` | `{configId, applicationCode, section, property}` |
| platform-config access-granted | + `canRead`, `wasCreated` | `{accessId, applicationCode, roleCode, canWrite}` |
| process created | + `description?`, `application`, `subdomain`, `processName` | `{processId, code, name}` |
| process updated | `name?`, `description?`, `bodyChanged?`, `tags?` | `{processId, name}` |
| scheduled job (all) | + `clientId?`; created + `name, crons, timezone, concurrent, tracksCompletion`; updated + `changedFields, version` | `{scheduledJobId, code}` (+ `instanceId` for fired-manually) |
| scheduledjobs synced | `{scope, clientId?, created, updated, archived}` | `{applicationCode, created, updated, archived}`, each `null` when empty |
| subscription created | + `endpoint`, `eventTypes`, `clientId?` | `{subscriptionId, code, name}` |
| subscription updated | `name?`, `eventTypesAdded`, `eventTypesRemoved` | `{subscriptionId, name}` |
| subscription paused / resumed | + `code` | `{subscriptionId}` |
| subscription synced | — | + `clientId?` (omitted; the Rust sync is application-scoped) |
| passkey registered / revoked / authenticated | `principalId`; `name` always present | `userId`; `name` omitted when absent |

Payloads that were already Go's apart from the envelope: user activated / roles-assigned / application-access-assigned /
client-access-granted / -revoked / logged-in, principals synced, service account deleted / roles-assigned /
token- and secret-regenerated, application service-account-provisioned / enabled- and disabled-for-client, cors,
anchor-domain, auth-config updated / deleted, all oauth-client, event type created / archived / deleted,
dispatch-pools / processes / subscription synced, process archived / deleted, subscription deleted,
platform-config access-revoked, application-openapi synced.

## 5. Now emitted as Go does (were missing or different)

| Go event | Rust before |
|---|---|
| per-row `role:created/updated/deleted` in the SDK role sync | rollup only |
| per-row `eventtype:created/updated/deleted` in the event-type sync | rollup only |
| per-row `subscription:created/updated/deleted` in the subscription sync | rollup only |
| per-row `dispatch-pool:created/updated/archived` in the pool sync | rollup only |
| per-row `process:created/updated/deleted` in the process sync | rollup only |
| per-row `scheduled-job:created/updated/archived` in the scheduled-job sync | rollup only |
| per-row `user:created/updated` in the principal syncs | emitted, but in separate transactions before the rows |
| `dispatch-pool:suspended` / `:activated` | suspend archived the pool (`dispatch-pool:archived`); activate changed nothing |
| `role:permission-granted` / `:permission-revoked` | `role:updated` (and 400 `NO_CHANGES` on a repeat) |

## 6. Unmatched

### Go emits, Rust does not (the feature is not in Rust)

| Go event | Why |
|---|---|
| `platform:admin:connection:synced` | no `connections/sync` route (owner follow-up "Missing Go routes") |
| `platform:iam:user:developer-credential-set` / `-revoked` | no self-service developer credential |
| `platform:admin:email-domain-mapping:provider-changed` | no move-provider operation |
| `email-domain-mapping:created/updated` inside an IdP create/update | Rust's IdP create/update does not claim or release domains |
| `platform:messaging:dispatch-job:cancelled` / `:completed`, `platform:messaging:dispatch-jobs:resent` | no human dispatch-job cancel/complete/resend use case (dispatch-job area, other workstream) |
| `platform:portal:identity:ensured` / `status-set` / `deleted` / `app-granted` / `app-revoked`, `platform:portal:app:created/updated/deleted` | no portal identity plane |

### Rust emits, Go does not (kept)

| Rust event | Note |
|---|---|
| `platform:function:*` (14 types) | function runner; Java is the reference (owner decisions) |
| `platform:admin:audit-log:redacted` | Java's audit redaction backfill |
| `platform:iam:application:client-config-updated` | per-client application config; Go has no such operation. Now on Go's application source and subject |
| `platform:iam:user:password-reset-requested` | self-service reset request; Go emits nothing on a request (its catalogue does list the code). Subject now `platform.principal.{id}` |

## 7. Event-type catalogue

Go seeds its catalogue on every start (`seed/event_types.go`); Rust did not seed it at all (only the BFF
sync-platform used it). Rust now:

- seeds at startup exactly as Go (insert missing codes as source `UI`, status `CURRENT`; refresh names; attach the
  catalogue schema as spec version `1.0` when none exists; never delete);
- carries Go's catalogue code for code, name for name, schema for schema (it gains
  `platform:admin:connection:synced`, the one code Rust lacked);
- then adds every type the platform emits that Go's catalogue lacks, each with a schema of the real payload.

Go's catalogue is itself out of step with Go's emitters: it seeds `platform:iam:client:*`, `platform:iam:role:*`,
`platform:iam:roles:synced`, `platform:iam:anchor-domain:*`, `platform:iam:auth-config:*`, `platform:admin:idp:*`,
`platform:admin:edm:*` (types nothing emits), and its schemas describe older payloads (e.g. `dispatchPoolId`,
`emailDomain`/`scope` on user created). These rows exist in production, so they are kept as they are; the emitted
names are added beside them.

## 8. Known remaining differences

- **Audit `operation`**: Rust writes the command's type name (`CreateClientCommand`); Go writes its own command
  struct's name, which is often bare (`CreateCommand`, `UpdateCommand`). Not aligned: the Go names are ambiguous
  without `entity_type`, and nothing consumes the column but the audit UI.
- **Correlation and execution ids**: Go's are UUIDs, Rust's are `exec-{TSID}`. Opaque tracing values.
- **Timestamps inside `data`** (`previousSecretExpiresAt` only): Go writes RFC 3339 with trimmed nanoseconds, Rust
  with 0/3/6/9 fractional digits. Both parse as the same instant.
- **Sync semantics** (found while adding per-row events, not changed here): Rust's syncs write rows directly and emit
  their events afterwards (the events are atomic with each other, not with the rows, except the principal and
  scheduled-job syncs); Rust's event-type sync updates only API/CODE-sourced rows while Go updates any existing row;
  Rust's SDK role sync replaces a role's permissions with an empty list where Go preserves them.
