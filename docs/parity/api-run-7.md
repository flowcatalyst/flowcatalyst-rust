# API parity run 7: Go vs Rust

Date: 2026-09-30. Seventh full run of the Go-vs-Rust API parity harness (`harness/parity`, owner decision #29), the
first against Go `main` after `5ffb009`, with the service-account delete fix (`fix/sa-delete-grants`) and the
allow-list refreshed.

| | |
|---|---|
| Go | `flowcatalyst-go` `main` @ `5ffb009`, taken read-only with `git archive main` (the Go checkout has uncommitted work) and built with `go build -mod=readonly` into prebuilt binaries; `frontend/dist`, which `git archive` omits, copied in so the SPA embed compiles |
| Rust | `fix/sa-delete-grants` on `main` @ `4e401af4`, with the change committed as `0d27ded0` (`fc-server` debug build; the tree was dirty only with that change) |
| Scenarios | 45 files / 1363 steps, unchanged since run 1 |
| Seed | Go `fcdev init`, first attempt |
| Allow-list | 173 entries before, 56 stale removed, 117 after |
| Command | `./target/debug/fc-parity --go-bin-dir <scratch>/go-bin --rust-bin-dir target/debug --report <scratch>/parity-run-7` |

## Totals

| status | run 1 | run 2 | run 3 | run 4 | run 5 | run 6 | run 7 |
|---|---:|---:|---:|---:|---:|---:|---:|
| OK | 89 | 587 | 871 | 1231 | 1231 | 1234 | **1260** |
| ACCEPTED (allow-listed) | 33 | 35 | 37 | 92 | 113 | 113 | **87** |
| DIFF | 870 | 534 | 419 | 40 | 19 | 16 | **16** |
| ERROR | 371 | 207 | 36 | 0 | 0 | 0 | **0** |

Two runs on the same binaries:

| | allow-list | OK / ACCEPTED / DIFF / ERROR | stale entries |
|---|---:|---|---:|
| before the refresh | 173 | 1260 / 87 / 16 / 0 | 56 |
| after the refresh | 117 | 1260 / 87 / 16 / 0 | **0** |

The per-file table and the DIFF steps are identical between the two runs: removing the stale entries turned no step
into a DIFF. No false `covers` claim. Coverage is unchanged: Go lockfile 252 / 256, outside-lockfile surface
131 / 135. The run exits 1 on the 16 DIFFs.

## What changed

1. **Service-account delete removes the principal's grants** (Rust fix). Go's `serviceaccount.Repository.Delete`
   deletes, in one transaction, the account's OAuth clients, its SERVICE principal's
   `iam_principal_application_access` and `iam_client_access_grants` rows, the principal (its
   `iam_principal_roles` rows cascade) and the account row. Rust's `Persist<ServiceAccount>::delete` skipped the two
   grant tables, which have no FK on `principal_id`, so the rows outlived the principal. The application delete
   guard (#34) then counted the dangling access grant and refused the authz scenario's `cleanup-application` with
   409 `APPLICATION_HAS_REFERENCES` "1 access grants", where Go answered 204. Rust now deletes both, in Go's order,
   right after the OAuth clients. `service_account_delete_test` provisions an account with application and client
   access, deletes it, asserts no grant row remains and deletes the application (204); it failed before the fix
   with one application-access and two client-access rows left. The 16 #34 entries went stale: `cleanup-application`
   and the application lists after it are now OK. Their `reason` blamed the scenario's anchor viewer; the grant was
   the service account's.
2. **Go `main` @ `5ffb009` fixed the #31, #38 and #40 behaviours** the allow-list recorded, and those 40 entries went
   stale:
   - the app-scoped and code-first syncs no longer answer 500 `AUDIT_WRITE` and roll back for an application code
     over 17 characters (`aud_logs.entity_id` `VARCHAR(17)`). With them go the consequences: the
     `list-by-status` rows, the router-config pool, the masked `applicationCode`s in `docs` and `roles`, and the
     synced user in the principal lists;
   - a rename onto an existing anchor domain and a duplicate IdP role mapping answer 409, not 500 `PERSIST`;
   - an email-domain mapping update that omits `primaryClientId` no longer clears it;
   - the service-account delete no longer leaves the SERVICE principal behind in the principal lists.

### Removed entries (56)

| ruling | n | scenarios |
|---|---:|---|
| #34 | 16 | `authz` `cleanup-application` (3), `bff` (3), `clients` (8), `me-public-config` `me-applications` (2) |
| #40 | 23 | `applications` sdk-sync (11), `principals` lists (6) and `sync-users`, `docs` (3), `roles` `filters-applications` (2) |
| #31 | 10 | `code-first-connections` (3), `connections` / `subscriptions` / `dispatch-pools` `list-by-status` (6), `router-config` `/processingPools/**` |
| #38 | 7 | `anchor-domains` `update-into-existing-domain` (3), `idp-role-mappings` `create-duplicate-idp-role-name` (3), `email-domain-mappings` `get-updated` |

(An entry citing two rulings is counted under its first.) The rulings remaining in the allow-list: #5 (33), #21 (24),
#33 (16), #43 (12), #35 (5), #30 (4), #46 (4), #8 (2), and one each of #31, #34 and #40 (below).

## Steps that moved since run 6

Run 6 was against Go `73a6918`, so the moves mix Go's fixes and Rust's.

- Cleared: `service-accounts` `mint-token` `claims/name` (Go `5ffb009` renames the SERVICE principal with its
  account), `auth-remainder` `oidc-login-unmapped-domain`, `audit-logs` `entity-types-facet-contains-our-entity-type`.
- New: `docs` `list-before-any-app-docs`, `list-after-sync-includes-application-group`,
  `confinement-read-succeeds-with-docs-view`: Go's embedded platform docs index has a `functions` page
  (`/platform/5`, `{"slug":"functions","title":"Functions"}`) that Rust's lacks.

## The 16 remaining DIFFs

| step(s) | n | class | difference |
|---|---:|---|---|
| `audit-logs` `by-principal`, `operations-facet-contains-create-and-update` | 2 | Rust design + ruled | Rust's `SyncAppDocsCommand` row per docs sync (run 5 follow-up 2), which shifts the rows and facets after it; command member casing (#36) and names. |
| `bff` `bff-sync-platform-event-types` (`/schemas/unchanged`, `/total`, `/updated`) | 1 | Go defect | Go's sync-platform tally is 73, Rust's 131. |
| `docs` ×3 | 3 | new, open | Go's platform docs index has a `functions` page Rust lacks. |
| `bff` `developer-get-platform-current-spec`, `developer-get-platform-version`; `me-public-config` `openapi-json`, `openapi-yaml`, `q-openapi-alias` | 5 | open | Platform OpenAPI documents from different generators (huma vs utoipa). |
| `webauthn` ×5 | 5 | open (library) | go-webauthn vs webauthn-rs ceremony options. |

## Follow-ups

1. **Allow-list, entries that still match a different diff.** Three entries are not stale but now accept a
   difference their `reason` does not describe:
   - `principals` `list-available-applications` `/applications/**` (#34): no longer the authz application; Go and
     Rust order the functions scenario's `pf-host-<run>` and `pf_bad_<run>` applications differently
     (`/applications/11`, `/applications/12` swapped).
   - `oauth-clients` `list` `/clients/**` (#40): no longer the deleted account's client; Go lists
     `CFC App Service Account Client` with `applicationIds` / `applications` for the code-first-connections
     application, Rust `CFC shared signer Client` with none.
   - `router-config` `router-config-with-router-role` `/queues/**` (#31): unchanged and still accurate.
   The first two want a look and a new reason or a fix.
2. **Rust:** the `functions` page in the platform docs index (Go `5ffb009`).
3. **Owner:** the sync-platform schema tally (Go defect, not named by a decision) and the docs sync's event and
   audit row (unchanged).
4. **Cutover checklist:** the OpenAPI documents and the webauthn options (unchanged).
