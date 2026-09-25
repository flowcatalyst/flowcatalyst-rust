# SDKs: what is published, and how it is published from here

Owner decision #11 (`docs/owner-decisions-2026-09-25.md`) makes this repo the home of the SDKs. Until
now the published SDKs were built from `flowcatalyst-go/clients/*`. This page records what is
published, what this repo's copies now contain, and the cutover steps. The owner does the actual
switch.

## What is published (as of 2026-09-25)

| SDK | Package | Where it's published | Consumers install via | Latest published | Source of that release |
|---|---|---|---|---|---|
| Laravel | `flowcatalyst/laravel-sdk` (composer) | `github.com/flowcatalyst/laravel-sdk`, a split mirror tagged `vX.Y.Z` | Composer `vcs` repository on that GitHub repo; **not on Packagist** (packagist.org 404s) | **0.10.26** (2026-09-22) | `flowcatalyst-go` tag `laravel-sdk/v0.10.26` |
| TypeScript | `@flowcatalyst/sdk` | `github.com/flowcatalyst/typescript-sdk`, a split mirror with a built `dist/` commit, tagged `vX.Y.Z` | git dependency on that repo; **not on the npm registry** (`npm view` 404s) | **0.11.27** (2026-09-22) | `flowcatalyst-go` tag `typescript-sdk/v0.11.27` |
| Java | `io.flowcatalyst:flowcatalyst-sdk` (Maven) | **nowhere**: source tags only; no split repo, no Maven registry | building from source | **0.0.10** | `flowcatalyst-go` tag `java-sdk/v0.0.10` |
| Go | module `github.com/flowcatalyst/flowcatalyst/clients/go-sdk` | **never published** | n/a | none | exists only in this repo |
| Rust | crate `fc-sdk` | **never published** (workspace version, no crates.io metadata) | git dependency on this repo | none | this repo |

The mechanism in `flowcatalyst-go` is `make release-<sdk> BUMP=…`, which runs `scripts/release.sh`.
It bumps `clients/<sdk>/VERSION` (plus `package.json` / `pom.xml`), commits, tags `<sdk>/vX.Y.Z`
and pushes. The tag fires `.github/workflows/split-<sdk>.yml`: `splitsh-lite --prefix=clients/<sdk>`,
a force-push to the standalone repo's `main`, and a re-tag there as `vX.Y.Z` (for TS, after
committing `dist/`). The Go repo's split workflows have been **disabled** since 2026-09-24
(`if: false`, commit `dac0c80 disable sdks`). There is no Java split workflow.

Version numbering has one line across the repos. This repo's `origin` has SDK tags only up to
**0.6.15**, the point where the SDKs moved to the Go repo on 2026-06-01 (`flowcatalyst-go`
`cc420c5`). The Go repo continued from 0.6.16. A local clone of this repo may also hold stale
`laravel-sdk/v0.10.x` / `typescript-sdk/v0.11.x` tags fetched from Go; they are harmless because
the bump base is max(VERSION, tags).

### Apps that depend on them

- `inhance/InhanceMono/apps/integral`: laravel-sdk 0.10.26 (`^0.10.12`)
- `inhance/InhanceMono/apps/rfp`: laravel-sdk 0.10.23 (`^0.10`)
- `inhance/InhanceMono/apps/hr`: laravel-sdk 0.8.19 (`^0.8`). The next release (0.10.27) will
  **not** reach hr without a constraint bump.
- `inhance/AgentPlanner`: Python, no SDK.

## What the copies in this repo now are

The Go repo's SDK copies forked from this repo's at `07259bb5` (2026-05-31). That fork point is
byte-identical to Go's `cc420c5` apart from `VERSION`. Each merge takes the Go repo's tree at the
published tag as the base, then replays this repo's changes since the fork with a 3-way merge.
Where the two conflict, the published code wins.

| SDK | Base | Added from this repo | Next version |
|---|---|---|---|
| `clients/laravel-sdk` | Go 0.10.26 | `SubscriptionSource::FUNCTION`; audit redaction (`AuditRedaction`, `AuditMasked`, `$maskedFields` as the **last**, optional constructor parameter); the 2026-09-25 SDK rulings (below); MIT licence as published | **0.10.27** |
| `clients/typescript-sdk` | Go 0.11.27 | audit redaction (`redactAuditData`, `AuditMasked`, `auditMaskedFieldsOf`, optional `maskedFields` on `withOperationData`), wired into both outbox units of work; FUNCTION round-trip test; the 2026-09-25 SDK rulings (below); Apache-2.0 licence as published | **0.11.28** |
| `clients/java-sdk` | Go 0.0.10, as-is | audit redaction (`AuditRedaction`, `withOperationData(Map, Set)` overload) | **0.0.11** |
| `clients/go-sdk` | this repo | token claims in Go's shape (below); the 2026-09-25 SDK rulings (below); Apache-2.0 licence | first release |
| `crates/fc-sdk` | this repo | token claims in Go's shape (below); the 2026-09-25 SDK rulings (below); Apache-2.0 licence | n/a |

Every SDK has a `CHANGELOG.md` with the unreleased entry. No published public API was removed or
changed incompatibly.

**Token claims.** Every SDK reads Go's shape (owner decision #3):
- `tier` is the tenancy tier;
- `scope` is the space-delimited permission list;
- `token_use` is `api` or `identity`;
- `clients` entries are `"*"` or `"{id}:{identifier}"`;
- `applications` entries are `"*"` or `"{id}:{code}"`, with `all_applications` alongside.

The Laravel and TS SDKs got this from the Go repo. Before the merge, this repo's Laravel copy still
read `scope` as the tier. The Go and Rust SDKs were fixed here. They fall back to a tier value in
`scope` for tokens minted before `tier` existed.

**SDK rulings of 2026-09-25** (owner decision #21; Java `docs/backlog.md` rulings 2, 5 and 11):
- **Router calls carry the platform bearer token** (ruling 2, Java 714f3f2d). TS, Laravel and Java
  now send it on `inPipeline` / `inPipelineBatch`; the Go and Rust SDKs already did (now pinned by
  tests).
  These releases must reach integral, hr and rfp **before** any router enforces auth.
- **Single-flight session refresh** (ruling 5, Java e0a9fd13). TS joins an in-flight exchange per
  refresh token and reuses its result for 10 s; Laravel does the same under a cache lock across
  PHP workers, with the token set memoised encrypted for 10 s. The Rust and Go `OAuthClient`
  refresh calls did not single-flight; they now do (join, 10 s memo, failures never remembered).
- **Result-returning webhook check** (ruling 11, Java f55afed6): TS `checkDeliverySignature`,
  Laravel `WebhookValidator::check` / `checkRequest` returning a `WebhookVerification`. The
  throwing forms are unchanged. Rust `WebhookValidator::validate` and Go `Validator.Validate`
  already return a result.
- **Platform additions:** service-account create takes `allApplications` (every SDK; Go and Rust
  gained a `ServiceAccounts` create for it), and both principal syncs report
  `passwordHashIgnored` (decision #22). Go and Rust principal-sync items also gained the optional
  `passwordHash` the TS and Laravel SDKs already sent.

**Audit redaction.** Every SDK runs the shared vectors, `docs/spec/audit-redaction-vectors.json`.
Each SDK carries a byte-identical copy because the SDKs are split into their own repos, and
`crates/fc-common/tests/audit_redaction_vectors_parity.rs` fails if any copy drifts.

**Generated code is not regenerated from this repo's spec (yet).** The vendored
`openapi/openapi.json` and the generated clients (`src/Generated`, `src/generated`, the Java
models) are the published ones, generated from Go's huma spec. Regenerating them with each SDK's
own script from that spec reproduces them byte for byte (checked for TS). This platform's spec uses
path-derived operationIds, e.g. `postApiPrincipalsUsers` where Go has `createUser`. It also
documents 140 operations where Go documents 254. Regenerating from it would rename every generated
class, which is a breaking change. `just regen-sdks` and `generate-sdks.yml` therefore refuse to
regenerate the SDKs unless explicitly allowed. The fix belongs on the platform side: give the Rust
routes Go's operationIds and schema names, then regenerate and diff.

## Compatibility with the apps

These were checked read-only against the merged Laravel SDK, covering every `FlowCatalyst\` class,
method, attribute, guard, middleware alias, config key and artisan command used by the apps and
their shared `packages_root` packages.

- **integral** (0.10.26) and **rfp** (0.10.23): **no gaps**. Everything they use exists with the
  same signature. The only differences from 0.10.26 are additive: `SubscriptionSource::FUNCTION`,
  an optional `$maskedFields`, and audit `operationData` now redacted. That last one is the one
  behaviour change.
- **hr** (0.8.19, `^0.8`): **no signature gaps**, but it needs its constraint raised to `^0.10`
  to receive any new release. Moving from 0.8.19 changes behaviour in three ways:
  - OIDC ID tokens are now verified (signature, issuer, `aud` = `FLOWCATALYST_OIDC_CLIENT_ID`,
    `exp`);
  - `return_url` must be relative;
  - audit rows are redacted, and HTTP calls retry on 408/429/502/503/504.
  
  0.8.19 already reads `tier` for the tier and `scope` as permissions, so it works with Go's token
  shape as it stands.
- No app reads `tier`, `scope` or `roles` straight from a JWT. They go through the SDK.

## Go SDK: remaining drift from Go's API

A read-only audit of `clients/go-sdk` against Go's spec checked about 128 calls. About 93 match,
including all auth, sync and token paths. Fixed in this branch: `Roles.ListForApplication` and
`ScheduledJobs.ListInstanceLogs` (bare arrays), and the always-sent required booleans on role and
scheduled-job create. Also fixed: log `level` now defaults to INFO, and `AddSchemaVersionRequest`
gained an optional `version`. Still open:

- **Divergences between this platform and Go**, where the SDK follows this platform today. Align
  the platform to Go first, then the SDK:
  - `Applications.GetServiceAccount` uses `GET …/service-account`; Go has only `POST`.
  - `Applications.UpdateClientConfig` uses `PUT …/clients/{clientId}`; Go has only `GET`.
  - `Applications.ListRoles`: Go returns `{roles:[string]}`, this platform returns an array.
- **Breaks on Go:**
  - `EventTypes.Update` and `Connections.Update` send an optional `name`, but Go requires it (full
    replacement).
  - `Processes` model `steps`, but Go has `body`, `diagramType` and `tags`, and its strict sync
    item rejects `steps`.
  - Direct `EventTypes.Sync` callers can send `schema`/`clientId`, which the strict item rejects.
- **Silent on Go** (a field or filter is dropped, or a response decodes as empty):
  - `DispatchPools.List` reads `items`, but Go returns `pools`.
  - `Applications.ListClients` reads `clientConfigs`/`config`, but Go returns
    `items`/`configJson`.
  - `ProvisionServiceAccount` expects a flat body, but Go nests
    `serviceAccount.oauthClient.clientSecret`, so the one-time secret is lost.
  - `Principals.FindByEmail` sends `?email=`, but Go reads `q`.
  - `Archive` on processes, pools and event types sends `DELETE`, a hard delete. Go archives
    processes and pools via `POST …/archive`, and has no archive route for event types.
  - Audit logs use page/`from`/`to`, but Go uses cursors (`after`/`nextCursor`) and `clientIds`.
  - Scheduled-job lists read `totalPages`, but Go returns `total_pages`.
  - Permission and event-type response field names differ (`eventName`, `name`/`category`).
  - Router `InPipeline` reads a `detail` object, but Go puts the fields at the top level.
  - Several list filters don't exist on Go.
  - Creates return `{id}` only, and most updates return 204, but the SDK decodes full entities,
    so callers get zero-value structs.

## Rust SDK: open item

`fc_sdk::auth::axum` builds a browser session from the **access token** returned by the
authorization-code exchange. On a Go-shaped platform that token is `token_use: identity`: it
carries no roles, clients, applications or scope. A session principal would therefore have no
authority. The published TS and Laravel SDKs read authority from the verified **ID token** (with
`aud` = the OAuth client id). fc-sdk needs the same: validate `id_token` with the client id as
audience, and take the tier and authority lists from it. The claims types now recognise identity
tokens (`is_identity_token()`), but the session flow has not been changed.

## Tests (2026-09-25)

| SDK | Command | Result |
|---|---|---|
| Laravel | `XDEBUG_MODE=off vendor/bin/phpunit` | 228 tests, 0 failures |
| TypeScript | `pnpm run lint && pnpm test` | tsc clean; 142 tests, 0 failures |
| Java | `mvn -o clean verify` (Maven offline; deps cached) | 80 tests, 0 failures, 1 skipped (real-PG, needs `FC_JAVA_SDK_TEST_PG_URL`) |
| Go | `go vet ./... && go test ./...` | all packages pass (10 new cases) |
| Rust | `cargo test -p fc-sdk` (and `--all-features`) | all pass (291 unit tests with all features) |

## Publishing from here

This repo now has:
- `.github/workflows/split-laravel-sdk.yml` and `split-typescript-sdk.yml`: the same mechanism as
  Go's, with the same target repos and secret names (`LARAVEL_SDK_TOKEN`,
  `TYPESCRIPT_SDK_TOKEN`).
- `just release-laravel-sdk <bump>`, `just release-ts-sdk <bump>` and `just release-java-sdk <bump>`.
  These are the equivalent of Go's `scripts/release.sh`: they bump from max(`VERSION`, highest
  tag), keep `package.json` / `pom.xml` in lockstep, commit, tag and push.

### Cutover steps (owner), in order

1. **Stop every other source.** Only one repo may force-push a standalone SDK repo's `main`.
   - `flowcatalyst-go`: `split-laravel-sdk.yml` and `split-typescript-sdk.yml` are already disabled
     (`if: false`). Leave them disabled, or delete them.
   - `flowcatalyst-javalin`: its `split-laravel-sdk.yml` and `split-typescript-sdk.yml` are
     **enabled** and target the same standalone repos. Disable them (`if: false`) before step 3.
2. **Secrets.** Add `LARAVEL_SDK_TOKEN` and `TYPESCRIPT_SDK_TOKEN` to this repo's Actions secrets.
   Use the same fine-grained tokens with write access to `flowcatalyst/laravel-sdk` and
   `flowcatalyst/typescript-sdk`.
3. **Merge** this branch to `main`, then cut the first releases from `main`, one SDK at a time:
   1. In `clients/<sdk>/CHANGELOG.md`, move the "Unreleased" entry under the version.
   2. `just release-laravel-sdk patch` → tags `laravel-sdk/v0.10.27` → mirror tag `v0.10.27`.
   3. `just release-ts-sdk patch` → tags `typescript-sdk/v0.11.28` → mirror tag `v0.11.28`.
   4. `just release-java-sdk patch` → tags `java-sdk/v0.0.11`. There is no mirror; nothing
      publishes it.
4. **Verify.** Check that the mirror repos' `main` and new tags exist, that `v0.10.26`,
   `v0.11.27` and older tags still resolve, and that `composer update flowcatalyst/laravel-sdk` in
   `apps/integral` picks up 0.10.27.
5. **Retire** `flowcatalyst-go/clients/*` (and the Java repo's copies) as read-only history, and say
   in their READMEs where the SDKs now live.

**History.** Splitting from this repo produces a different commit history than the Go repo's
split, so the first push force-replaces the mirrors' `main`. This already happened once, when
publishing moved from this repo to Go. Old tags keep pointing at their old commits, so every
`composer.lock` reference stays resolvable. Do not delete the old tags.

### Needs an owner decision

- **Licence (decided 2026-09-25).** The SDKs keep their published licences: TS Apache-2.0,
  Laravel MIT. The never-published Go SDK and `fc-sdk` follow the TS SDK (Apache-2.0). The
  2026-09-01 move to MPL-2.0 is reverted for the SDKs; the function guest crates stay MPL-2.0
  (decision #10). The Java SDK still has no licence file or pom `<licenses>` at all.
- **Java distribution.** Maven Central, GitHub Packages or JitPack: Go's `docs/java-sdk-plan.md`
  left this open. Until it's decided, a Java release is only a tag.
- **Go SDK module path.** `github.com/flowcatalyst/flowcatalyst/clients/go-sdk` names the **Go
  platform** repo (`flowcatalyst/flowcatalyst`), where no `clients/go-sdk` exists, so `go get`
  cannot resolve it. It needs either `github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk`
  with `clients/go-sdk/vX.Y.Z` tags, or a split repo `github.com/flowcatalyst/go-sdk` with its own
  module path. The Go SDK has never been published, so the rename breaks no one.
- **Unpublished work in `flowcatalyst-javalin`.** Its TS and Laravel webhook `check()` /
  `WebhookVerification`, router bearer handling and single-flight refresh are now ported here
  (see "SDK rulings of 2026-09-25"), and so is its Java SDK router bearer change. Not merged: a
  Jackson-3 Java SDK at 0.0.4. Its Laravel `CreateAuditLogDto` also puts `maskedFields`
  **before** `applicationCode`/`clientCode`, which breaks positional callers; the copy here puts
  it last.
- **Platform spec.** Should the Rust platform adopt Go's operationIds and schema names, so the
  SDKs can be regenerated from this repo's spec? This goes with the platform divergences listed
  under "Go SDK: remaining drift".
- **fc-sdk session authority** (see "Rust SDK: open item"): fix it before any Rust app uses the
  axum OIDC session against the Go-shaped platform.
