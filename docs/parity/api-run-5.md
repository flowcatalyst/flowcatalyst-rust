# API parity run 5: Go vs Rust

Date: 2026-09-26. Fifth full run of the Go-vs-Rust API parity harness (`harness/parity`, owner decision #29), after
the principal follow-ups (`feat/followups-principals`) and owner decision #40.

| | |
|---|---|
| Go | `flowcatalyst-go` @ `73a6918`, built with `go build -mod=readonly` (Go tree unchanged) |
| Rust | `feat/followups-principals` @ `b841ee00` (`fc-server` debug build, clean tree) |
| Scenarios | 45 files / 1363 steps, unchanged since run 1 |
| Seed | Go `fcdev init`, first attempt |
| Allow-list | 147 entries (123 in run 4, 24 added citing #40) |
| Command | `./target/debug/fc-parity --rust-bin-dir target/debug --report target/parity-run-5` |

## Totals

| status | run 1 | run 2 | run 3 | run 4 | run 5 |
|---|---:|---:|---:|---:|---:|
| OK | 89 | 587 | 871 | 1231 | **1231** |
| ACCEPTED (allow-listed) | 33 | 35 | 37 | 92 | **113** |
| DIFF | 870 | 534 | 419 | 40 | **19** |
| ERROR | 371 | 207 | 36 | 0 | **0** |

No stale allow-list entry, no false `covers` claim. Coverage is unchanged: Go lockfile 252 / 256, outside-lockfile
surface 131 / 135. The run exits 1 on the 19 DIFFs.

The same build before the allow-list entries gave 1231 / 92 / 40 / 0, the run-4 totals: the principal fixes below
change behaviour no scenario step exercises (a no-op update, grant dates after a later save, query counts), and the
provisioning audit rows now match Go's inside a step that still differs for other reasons (`audit-logs`
`by-principal`, below). The 21 steps that moved are the #40 entries.

## Per group

| group | steps | run 4 OK / ACC / DIFF / ERR | run 5 OK / ACC / DIFF / ERR |
|---|---:|---|---|
| `applications/*` | 72 | 61 / 0 / 11 / 0 | 61 / 11 / 0 / 0 |
| `docs/*` | 21 | 18 / 0 / 3 / 0 | 18 / 3 / 0 / 0 |
| `oauth-clients/*` | 32 | 31 / 0 / 1 / 0 | 31 / 1 / 0 / 0 |
| `principals/*` | 81 | 74 / 2 / 5 / 0 | 74 / 7 / 0 / 0 |
| `roles/*` | 46 | 41 / 4 / 1 / 0 | 41 / 5 / 0 / 0 |
| every other group | 1111 | 1006 / 86 / 19 / 0 | 1006 / 86 / 19 / 0 |
| **total** | **1363** | **1231 / 92 / 40 / 0** | **1231 / 113 / 19 / 0** |

## What this branch changed

Each fix follows Go's handler and use case, with a Docker test in
`crates/fc-platform/tests/principal_go_parity_test.rs`.

1. **No-op principal update.** Go's `UpdateUser` (`principal/operations/update.go`) applies whatever was sent, saves
   and records `UserUpdated` even when nothing changed; it refuses only a blank name (`NAME_REQUIRED`) and an email
   other than the stored one (`EMAIL_IMMUTABLE`, the email is an identity assertion, never a rename). Rust answered
   400 `NO_CHANGES` "No changes detected" for an unchanged name and 400 `NO_UPDATES` for an empty body. The SPA
   sends the name first on every save and then changes the tier or client through `/client-association`, so a
   Type- or Client-only change failed against Rust. Rust now does as Go (200, the principal, one `UserUpdated`),
   accepts and asserts `email`, and fc-web sends the name on every save as the SPA does.
2. **N+1 in application access.** The `GET` and `PUT /api/principals/{id}/application-access` handlers and the
   `AssignApplicationAccess` use case read one application per granted id (Go does too); they now read the set in
   one `WHERE id = ANY($1)` query (`ApplicationRepository::find_by_ids`), keeping Go's order and skipping ids that
   no longer resolve. The principal's transactional save wrote its role, client-grant and application-access
   junctions one `INSERT` per row; each is now one `UNNEST` statement.
3. **Client grant dates.** `GET /api/principals/{id}/client-access` answered every grant with the user's creation
   time and a synthetic `"{principalId}-{n}"` id, and `POST` with the request time. Go answers each grant row's own
   `id` and `grantedAt` (`clientAccessGrantFromEntity`), oldest first. Rust now reads the grant rows. Two causes
   went with it: grant ids were minted with the principal prefix (`prn_`), now `gnt_` as Go's
   `tsid.ClientAccessGrant`; and every save of the principal deleted and re-inserted its grants (new ids, grant date
   = now), so a name change reset every grant date. The save now keeps the grants the principal still holds, removes
   the dropped ones and adds the new ones.
4. **All-applications default.** Go's `principal.NewUser` sets `AllApplications: true` for every user (create,
   `createUser`, bulk import, the principal syncs); only `CreatePortalUser` and service accounts start without.
   Rust already matched (`Principal::new_user`), and the service-account ruling (no application access) is
   untouched. The test pins create and sync.
5. **Provision-service-account audit.** Go runs provisioning as one operation and hands its single
   `ProvisionServiceAccountCommand {applicationId}` to each scoped commit, so its three audit rows (service account,
   application, OAuth client) all record that command; Rust recorded `CreateCommand`, `AttachServiceAccountCommand`
   and `CreateOAuthClientCommand`. `PgUnitOfWork::run_as(&command, …)` runs the same three use cases in the same
   transaction, each writing its own event, with every audit row recorded under the orchestration's command
   (camelCase JSON per #36). The run shows the three rows now match Go's operation and JSON.
6. **Allow-list (#40).** 24 entries, each a step and the members that differ:
   - the eleven `applications/sdk-sync` sync steps where Go answers 500 `AUDIT_WRITE` (whole response, `/**`: the
     Go response is the 500);
   - their cascades: `docs` ×3 and `roles` `filters-applications` (the application code masked as
     `«auto:entityId»` on Rust only, because Rust's rollup audit rows show it under `entityId` and Go rolled them
     back), `principals-access` `sync-users` (the same string as an email local part), and `principals-core`
     `list-all`, `list-by-active`, `list-by-type`, `list-sorted-desc` (`/principals/**`, plus `/total` where it
     differs): checked row by row, the lists differ only in Rust's synced user and Go's leftover SERVICE principal;
   - `oauth-clients` `list` (`/clients/**`): Go still lists the deleted service account's OAuth client.

   #36 is untouched: no casing entry, no change to command JSON.

## The 19 remaining DIFFs

| step(s) | n | class | difference |
|---|---:|---|---|
| `audit-logs` `by-principal` | 1 | cascade + ruled + Rust follow-up | Rust has the sdk-sync audit rows Go rolled back (#40) and a `SyncAppDocsCommand` row per docs sync (Go writes documentation without an event or audit row); casing (#36); some command member names (`applicationId` vs `id` on application delete, `anchorDomainId` vs `id` on anchor-domain delete/update, the OAuth-client create command's members, the anchor-domain create command's domain as sent vs normalised); and the service-account row's `entityId` (below). The provisioning rows' operation and JSON now match. The rows shift, so no entry can be scoped tighter than the whole list. |
| `audit-logs` `entity-types-facet-…`, `operations-facet-…` | 2 | cascade + Rust design | The facets list the entity types and operations of the rows above: the #40 sync rollups and `SyncAppDocsCommand`. |
| `service-accounts` `list` | 1 | Rust follow-up (was "harness") | Byte-identical bodies once ids are set aside, but the cause is not the sdk-sync cascade: Rust's service-account events carry the SERVICE principal's id (`prn_…`) as subject and `serviceAccountId`, Go's the account's (`sac_…`, `NewServiceAccountCreatedEvent(ec, sa.ID, …)`). Go's provisioning audit row therefore shows the `sac_` id under `entityId` and Rust's does not, and the normaliser masks the ids differently. See follow-up 1. |
| `service-accounts` `mint-token` (`claims/name`) | 1 | Go defect | Go's account update does not rename the SERVICE principal, so its token keeps the old name; Rust keeps the two in step. No decision names it. |
| `bff` `bff-sync-platform-event-types` (`/schemas/unchanged`) | 1 | Go defect | Go's sync-platform schema tally is always 0 ("not instrumented"); Rust reports 131. |
| `auth-remainder` `oidc-login-unmapped-domain` | 1 | Go defect | Go 500 `OIDC_RESOLVE_FAILED`, Rust 404 `EMAIL_DOMAIN_NOT_MAPPED`; a login route, so #38 does not name it. |
| `bff` `developer-get-platform-current-spec`, `developer-get-platform-version`; `me-public-config` `openapi-json`, `openapi-yaml`, `q-openapi-alias` | 5 | open | Platform OpenAPI documents from different generators (huma vs utoipa). Cutover checklist. |
| `portal` `redeem-code-a`, `redeem-code-b2` | 2 | open (auth owner) | A portal token's `tier` is `""` on Go and `CLIENT` on Rust. |
| `webauthn` ×5 | 5 | open (library) | go-webauthn vs webauthn-rs ceremony options. |

## Follow-ups

1. **Rust (events, cutover-relevant):** Rust builds every service-account event from `ServiceAccount.id`, the
   SERVICE principal's `prn_` id; Go builds `created`, `updated`, `deactivated`, `deleted` and `roles-assigned`
   from the account's `sac_` id (`subjectFor(sa.ID)`, `serviceaccount/operations/*.go`). The subject, the payload's
   `serviceAccountId` and so the audit row's `entityId` differ, and likely the application's
   `service-account-provisioned` payload too (Go: `sa.ID`). Subscribers matching on these see different ids. Aligning
   them needs the provisioning handler (which reads the principal id from the created event) to take it from the
   result instead. This would also clear `service-accounts` `list`.
2. **Owner:** the docs sync writes an event and an audit row in Rust (every write goes through a use case,
   `CLAUDE.md`) where Go writes neither; it shows in the audit facets. A ruling would allow-list those members.
3. **Rust:** the command member names listed under `by-principal` (follow-up 3 of run 4, unchanged).
4. **Cutover checklist:** the OpenAPI documents and the portal token's `tier` (unchanged).
