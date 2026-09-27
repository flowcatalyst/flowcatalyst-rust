# API parity run 6: Go vs Rust

Date: 2026-09-27. Sixth full run of the Go-vs-Rust API parity harness (`harness/parity`, owner decision #29), after
the cutover fixes (`feat/cutover-fixes`).

| | |
|---|---|
| Go | `flowcatalyst-go` @ `73a6918`, built with `go build -mod=readonly` (Go tree unchanged) |
| Rust | `feat/cutover-fixes` @ `e9ceb88e`, since rewritten as `ca2d9913` with only a clippy borrow fix folded into `bin/fc-server` (`fc-server` debug build; the tree was dirty only in `docs/`) |
| Scenarios | 45 files / 1363 steps, unchanged since run 1 |
| Seed | Go `fcdev init`, first attempt |
| Allow-list | 147 entries, unchanged from run 5 |
| Command | `./target/debug/fc-parity --rust-bin-dir target/debug --report target/parity-run-6` |

## Totals

| status | run 1 | run 2 | run 3 | run 4 | run 5 | run 6 |
|---|---:|---:|---:|---:|---:|---:|
| OK | 89 | 587 | 871 | 1231 | 1231 | **1234** |
| ACCEPTED (allow-listed) | 33 | 35 | 37 | 92 | 113 | **113** |
| DIFF | 870 | 534 | 419 | 40 | 19 | **16** |
| ERROR | 371 | 207 | 36 | 0 | 0 | **0** |

No stale allow-list entry, no false `covers` claim. The run exits 1 on the 16 DIFFs.

Three steps moved from DIFF to OK, none the other way:

| step | run 5 | run 6 | fix |
|---|---|---|---|
| `service-accounts` `list` | DIFF | OK | service-account events carry the account id |
| `portal` `redeem-code-a` | DIFF | OK | the portal access token's `tier` is `""` |
| `portal` `redeem-code-b2` | DIFF | OK | the same |

## What this branch changed

1. **Service-account event ids.** Go builds every service-account event from the account (`sac_…`,
   `subjectFor(sa.ID)` in `serviceaccount/operations/*.go`): `created`, `updated`, `deactivated`, `deleted`,
   `roles-assigned`, `token-regenerated` and `secret-regenerated` carry it as subject, message group and
   `serviceAccountId`, so the audit row's `entityId` is the account too; `service-account-provisioned` names it as
   well (`sa.ID` in provisioning, `cmd.ServiceAccountID` on the attach route). Rust used the SERVICE principal's
   `prn_` id. The event constructors now take the account and read `ServiceAccount::account_id()`; the
   application and the OAuth client still point at the principal (Go: `app.ServiceAccountID = &saPrincipal.ID`);
   the provisioning and create handlers take the principal id from the create result, not from the event. The
   admin token-mint record (a Rust event; Go writes only the `TOKEN_MINTED_BY_ADMIN` audit row) names the account,
   as Go's audit row does. This cleared `service-accounts` `list`, whose masked ids no longer diverge.
2. **Atomic syncs.** Go's `usecaseop.Sync` / `usecasepgx.CommitSync` plans a whole sync and then writes every
   row, a created/updated/deleted event and audit row per row, and the rollup, in one transaction. Rust's
   event-type, process, role, dispatch-pool and subscription syncs wrote each row as they went and emitted the
   events afterwards, so a bad row part-way left the earlier rows written with no events. They now plan first and
   end in the new `UnitOfWork::commit_sync`; validation errors keep their codes, messages and first-bad-row
   order. The OpenAPI spec sync archives the prior spec and inserts the new one inside the event's commit (Go does
   both outside it). No scenario step exercises a failing sync, so the totals do not move.
3. **Portal token `tier`.** Go mints portal tokens from a synthetic principal with no scope, so both carry
   `tier: ""`; Rust's access token said `CLIENT`, and Rust could not decode Go's (the empty tier failed the strict
   decode). The claim is now optional, written as `""`, and accepted empty only on an identity-only token, which
   the platform (and the function host's bearer) refuses as an API credential anyway.
4. **Housekeeping purge** (not visible to the harness). Go's `StartPurger` every minute; see the cutover
   checklist.
5. **Branded emails** (not visible to the harness). Go's `branding.Theme.RenderEmail` layout and login theme on
   the reset, invite and portal emails.

## The 16 remaining DIFFs

| step(s) | n | class | difference |
|---|---:|---|---|
| `audit-logs` `by-principal` | 1 | cascade + ruled + Rust follow-up | As run 5: Rust's sdk-sync rows Go rolled back (#40), a `SyncAppDocsCommand` row per docs sync, casing (#36), and some command member names. The rows still shift against Go's, so this step cannot show the service-account row's `entityId` now matching; `principal_go_parity_test::service_account_events_carry_the_account_id` pins it. |
| `audit-logs` `entity-types-facet-…`, `operations-facet-…` | 2 | cascade + Rust design | The facets of the rows above. |
| `service-accounts` `mint-token` (`claims/name`) | 1 | Go defect | Go's account update does not rename the SERVICE principal; Rust keeps the two in step. |
| `bff` `bff-sync-platform-event-types` (`/schemas/unchanged`) | 1 | Go defect | Go's sync-platform schema tally is always 0; Rust reports 131. |
| `auth-remainder` `oidc-login-unmapped-domain` | 1 | Go defect | Go 500 `OIDC_RESOLVE_FAILED`, Rust 404 `EMAIL_DOMAIN_NOT_MAPPED`. |
| `bff` `developer-get-platform-current-spec`, `developer-get-platform-version`; `me-public-config` `openapi-json`, `openapi-yaml`, `q-openapi-alias` | 5 | open | Platform OpenAPI documents from different generators (huma vs utoipa). |
| `webauthn` ×5 | 5 | open (library) | go-webauthn vs webauthn-rs ceremony options. |

## Follow-ups

1. **Owner:** the three Go defects above (mint-token name, sync-platform tally, unmapped-domain OIDC login) are
   not named by a decision; a ruling would allow-list them as #38/#40 do.
2. **Owner:** the docs sync's event and audit row (run 5 follow-up 2, unchanged).
3. **Rust:** the command member names under `by-principal` (unchanged).
4. **Cutover checklist:** the OpenAPI documents and the webauthn options (unchanged).
