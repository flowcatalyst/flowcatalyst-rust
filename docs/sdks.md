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
| Java | `io.flowcatalyst:flowcatalyst-sdk` (Maven) | **nowhere** yet: source tags only. From this repo: **JitPack** (owner decision; set up, see "Java SDK on JitPack") | building from source | **0.0.10** | `flowcatalyst-go` tag `java-sdk/v0.0.10` |
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
  The router needs `platform:messaging:router:view` in the token's `scope`. An application's
  service account holds it through `platform:application-service`, and a client-credentials token
  requested without a `scope` carries every permission the account holds. A caller that narrows
  its scope (the Go SDK's `ClientCredentialsConfig.Scopes`) must include it. The Rust SDK sends
  the token its caller gives it.
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

**Generated code is still not regenerated from this repo's spec.** The vendored
`openapi/openapi.json` and the generated clients (`src/Generated`, `src/generated`, the Java
models) are the published ones, generated from Go's huma spec. Regenerating them with each SDK's
own script from that spec reproduces them byte for byte (checked for TS). This platform's spec now
carries Go's operationIds and schema names, but regenerating from it would still change the
generated code ("Platform OpenAPI document" below), so `just regen-sdks` and `generate-sdks.yml`
still refuse unless explicitly allowed.

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

## SDK calls against Go's API (fixed 2026-09-27)

A read-only audit of `clients/go-sdk` against Go's spec (about 128 calls) found calls that break
on Go or silently lose data. The same drift was in `fc-sdk`, which the Go SDK was ported from,
and some of it in the hand-written layers of the TS, Laravel and Java SDKs. Every SDK was checked
against Go's spec and handlers and fixed on branch `feat/openapi-sdk`, with tests per SDK and an
`Unreleased` CHANGELOG entry (the Go SDK and `fc-sdk` gained a `CHANGELOG.md`).

**The three routes where this platform differed from Go.** The SDKs now use Go's surface:
- `Applications.GetServiceAccount` called `GET …/service-account`, which Go lacks. Every SDK now
  reads the application (`GET /api/applications/{id}`) and, when `serviceAccountId` is set, the
  account (`GET /api/service-accounts/{id}`); no service account is a not-found error.
- `Applications.UpdateClientConfig` called `PUT …/clients/{clientId}`, which Go lacks (Go has
  `GET …/clients/{clientId}` and the enable/disable POSTs). Every SDK gained `GetClientConfig`;
  `UpdateClientConfig` is deprecated as Rust-only.
- `Applications.ListRoles`: Go (and this platform) answer `{roles:[string]}`. The Go, TS and
  Laravel SDKs decoded an array (Laravel keeps `listRoles()` returning `ApplicationRole`s built
  from the names, deprecated for the new `listRoleNames()`).

This platform still serves the two Rust-only routes (undocumented in `/q/openapi`).

**Fixed per SDK** (details in each CHANGELOG):

| Item | Go SDK | fc-sdk | TS | Laravel | Java |
|---|---|---|---|---|---|
| Creates return `{id}`, updates 204: no full-entity decode | fixed | fixed (scheduled-job runner's log/complete callbacks failed after every 204) | fixed (types) | fixed (`applications()->create()`, 204s now `void`) | already right |
| `name` always sent on event-type/connection update | fixed | fixed | fixed (fetches the current name if omitted) | fixed (same) | fixed (same) |
| Processes `body`/`diagramType`/`tags` | fixed | already right | already right | already right | already right |
| Event-type sync sends Go's strict item | fixed | fixed | fixed | already right | already right |
| Dispatch-pool list reads `pools` | fixed | fixed | already right | already right | already right |
| Application client list reads `items`/`configJson` | fixed | fixed | fixed | fixed | already right |
| Provision-service-account keeps the nested one-time secret | fixed | fixed | fixed | fixed (`ProvisionServiceAccountResult`) | already right |
| Principal find-by-email sends `q` | fixed | fixed | already right | fixed (also still sends `email`) | already right |
| Archive: processes/pools `POST …/archive`; event types have none | fixed (`EventTypes.Archive` removed for `Delete`) | fixed (deprecated) | fixed (deprecated) | fixed (deprecated) | fixed (deprecated; pool `archive()` added) |
| Audit logs cursor paging (`after`/`nextCursor`, CSV `clientIds`) | fixed | fixed | `recent()` fixed | fixed | fixed (CSV instead of repeated params) |
| Scheduled-job lists read `total_pages` | fixed | fixed | fixed | fixed | already right |
| Permission / event-type member names | fixed | fixed | already right | fixed | already right |
| Router `inPipeline` reads top-level `poolCode`/`queueId` | fixed | fixed | fixed | fixed | fixed |
| Filters Go lacks | removed | removed | deprecated | applied client-side or documented | documented |

Beyond the list: `AddSchemaVersion` always sends the `version` Go requires (Go SDK, fc-sdk);
bare-array answers (`Roles.ListForApplication`, instance logs) in fc-sdk; URL-encoded list
queries in fc-sdk; Go's required booleans defaulted on role and scheduled-job create (TS, Java);
members Go returns that the hand-written types lacked (all SDKs). Java's generated models come
from Go's spec, so it needed the fewest changes.

**Dispatch-job `descriptor` and `queue`** (with `feat/dispatch-descriptor`): the dispatch-job
create routes take Go's optional `descriptor` (at most 255 characters) and `queue` (`DEFAULT` or
`HIGH_PRIORITY`). The outbox dispatch-job builders carry both into the payload only when set: TS
`withDescriptor()` and Laravel `withDescriptor()` (plus a trailing optional constructor
parameter) beside their existing `queue`; `fc-sdk` (`.queue()`, `.descriptor()` on
`CreateDispatchJobDto` and `DispatchJobPayload`) and Java (`withQueue()`, `withDescriptor()`) had
no `queue` before. TS, Laravel and Java reject a descriptor over 255 characters; the Go SDK has no
dispatch-job builder.

**Platform gaps these fixes expose** (Go documents it, this platform does not do it; handler or
data work, not done here): an application's per-client `baseUrlOverride`/`configJson` are not
stored, so `configJson` reads empty; `clientScoped` on event-type create/update is ignored;
`GET /api/events/{id}` has no `contextData`; service-account role assignments have no
`assignedBy`/`clientId`; service-account update ignores `webhookCredentials`, and create reads only
its `authType`; OAuth-client create takes `clientId` where Go takes `principalId`.

## Platform OpenAPI document (`/q/openapi`) against Go's

Owner decision #11 makes this repo the SDK home, so its platform document should be able to
replace Go's `api/openapi.lock.json` (vendored as `frontend/openapi/openapi.json`) as the source
of the SDKs' generated clients. The document at `/q/openapi` (also `/api/openapi.json`,
`/api/openapi.yaml` and the developer portal's platform spec) is now aligned to Go's:

- **Operations.** Every operation Go documents is documented, under Go's operationId. The
  handlers behind the plain axum routers (applications, service accounts, connections, dispatch
  pools, identity and auth config, CORS, portal, event batch ingest) are added by
  `shared::openapi_contract`; the annotations carry Go's ids.
- **Schemas.** Component schemas carry Go's names through `#[schema(as = GoName)]` (Rust type
  names and serde wire names unchanged), with Go's required members, `date-time` formats, integer
  formats, enums and any-typed members. The dispatch-job schemas include Go's `descriptor`, list
  `metadata` (`MetadataDTO`) and the create request's `descriptor` and `queue`. This also fixed two collisions where two Rust structs
  published under one name and one silently replaced the other (`RegenerateSecretResponse`,
  `ConfigResponse`).
- **Go's document conventions**, applied to the published document by
  `shared::openapi_contract::shape_as_go_contract` (the full `/q/openapi-full` is untouched): one
  success response plus `default: ErrorModel` per operation; `required` on parameters only when
  true, query parameters `explode: false`; optional members, parameters and bodies not nullable;
  `additionalProperties` true on top-level request bodies and false elsewhere; no orphan schemas.
  The document is served as JSON (and YAML through `serde_norway`), because utoipa's model cannot
  read back every schema it writes.

`crates/fc-platform/tests/openapi_go_contract_test.rs` (no database) fails if an operation Go
documents is missing, has another operationId, or names another request or success schema, or if
a Go schema name is missing (Go's two orphan schemas excepted). `OPENAPI_DUMP=<file>` writes the document.

**Measured against Go's lockfile** (`flowcatalyst-go` `73a6918`):

| | before (`main` 6a9fb0e5) | after |
|---|---:|---:|
| Go operations documented | 175 of 256 | **256 of 256** |
| with Go's operationId | 42 | **256** |
| Go schema names present | 119 of 250 | **248 of 250** |
| shared operations naming Go's request schema | 160 of 175 | **256 of 256** |
| shared operations naming Go's success status and schema | 137 of 175 | **255 of 256** |
| query-parameter sets equal | 165 of 175 | **247 of 256** |
| TS client generated from it (`@hey-api/openapi-ts`, the TS SDK's generator), compared with one generated from Go's lockfile with `$schema` removed (#30), JSDoc and whitespace ignored: identical operation functions | 27 of 256 | **250 of 256** |
| … identical types | 144 of 1530 | **1492 of 1530** |

**Remaining differences.** Decided:
- **`$schema`** (#30): Go's response schemas carry a read-only `$schema` member (and the TS
  generator emits a `…Writable` twin of each). Rust does not emit it.
- **`info.version`** (#33): the build version, where Go says `dev`.
- **Rust-only operations**: the function API (about 30 operations; Java is the reference,
  Direction), `/api/monitoring/*` (5), the `/auth` session routes (login, logout, refresh, me,
  check-domain) and `POST /api/dispatch-jobs`. They add functions and types to a generated client
  and change none of Go's.
- **`passwordHashIgnored`** on the principal syncs' results (#22).

Not decided (they keep the parity steps below DIFFs):
- **Documentation text**: operation summaries (about 210 differ in wording), Rust's operation,
  schema and property descriptions and Go's examples. Only the generated JSDoc differs.
- **`security`**: Rust annotates `bearer_auth` per operation (no `securitySchemes`, so generators
  ignore it; Swagger UI shows it).
- **Path parameter names**: `/api/config/{appCode}/…` (Go `{app}`) and `/api/roles/{roleName}`
  for get, update and delete (Go `{id}`). The router shares those segments with other routes,
  whose parameter must have the same name. These are the 6 differing generated functions.
- **`createEvent`** also documents the 200 of an idempotent replay beside Go's 201.
- **Rust extensions** (documented because the handlers accept them): list filters on clients
  (`status`), connections (`serviceAccountId`), IdP role mappings (`idpType`), OAuth clients
  (`active`), principals (`scope`, `email`), processes (`search`), roles (`applicationCode`,
  `clientManaged`, `source`), scheduled-job instances (`triggerKind`, `from`, `to`) and service
  accounts (`active`, `applicationId`, `clientId`); members `AddRoleRequest.clientId`,
  `UpdateMappingRequest.identityProviderId`/`scopeType`, `UpdateOAuthClientRequest.active`,
  `UpdatePrincipalRequest.clientId`/`firstName`/`lastName`/`scope`.
- **Platform gaps**, where the document is truthful and Rust lacks what Go has: see "SDK calls
  against Go's API" above (per-client config, `clientScoped`, event `contextData`, service-account
  role and webhook-credential members, OAuth-client `principalId`); `LoginAttemptResponse.identifier`
  and `RolePermissionListResponse.permissions` may be null.
- **`DispatchJobRead.priority`**: Go's list rows carry it; Rust's do not.
- Go's two orphan schemas (`RawDispatchJobResponse`, `RawEventResponse`) are not reproduced, and a
  few schemas list members or `required` in another order.

**Not regenerated.** Regenerating the SDKs from this document would remove `$schema` from every
response type (and the TS `…Writable` types, 195 of them), change 36 more types and 6 functions,
and add the Rust-only operations. That is not an empty or clearly intended diff, so the generated
clients were left as published. Note also that the SDKs' vendored `openapi.json` is older than
Go's current lockfile (regenerating TS from Go's own lockfile changes about 236 lines).

**API parity harness.** The five OpenAPI steps (`me-public-config` `openapi-json`, `openapi-yaml`,
`q-openapi-alias`; `bff` `developer-get-platform-current-spec`, `developer-get-platform-version`)
compare whole documents. `harness/parity/expected-diffs.json` now accepts, in the four JSON steps
only, the `**/properties/$schema` members (#30) and `info.version` / the stored spec `version`
(#33). The steps stay DIFFs for the undecided classes above; `openapi-yaml` is one text diff and
cannot be scoped. Closing them needs an owner ruling on those classes (or a decision that the
contract test, not a byte comparison, is the gate for the document).

The full run on this branch after merging `main` (`target/parity-openapi`, Go `73a6918`) gives
1227 OK, 113 ACCEPTED, 23 DIFF, 0 ERROR, with no stale allow-list entry. The 16 DIFFs of run 6
(`docs/parity/api-run-6.md`) are unchanged; the five OpenAPI steps among them now differ in 1971
members each (2586 before this branch; 194 `$schema` and the version are accepted, the rest is the
undecided classes above). The 7 others are one token `scope` difference that `main` already has,
from the router-auth merge (`956741d1`), not this branch: Rust's `platform:application-service`
role carries `platform:messaging:router:view` (ruling 2), so client-credentials tokens list it and
Go's do not (`auth/oauth-code-flow` ×3, `authz`, `platform/profile-only`, `router-config` ×2). They
need an allow-list entry citing ruling 2 / decision #43.

## Rust SDK: open item

`fc_sdk::auth::axum` builds a browser session from the **access token** returned by the
authorization-code exchange. On a Go-shaped platform that token is `token_use: identity`: it
carries no roles, clients, applications or scope. A session principal would therefore have no
authority. The published TS and Laravel SDKs read authority from the verified **ID token** (with
`aud` = the OAuth client id). fc-sdk needs the same: validate `id_token` with the client id as
audience, and take the tier and authority lists from it. The claims types now recognise identity
tokens (`is_identity_token()`), but the session flow has not been changed.

## Tests (2026-09-27)

| SDK | Command | Result |
|---|---|---|
| Laravel | `XDEBUG_MODE=off vendor/bin/phpunit` | 264 tests, 0 failures |
| TypeScript | `pnpm run lint && pnpm test` | tsc clean; 180 tests, 0 failures |
| Java | `mvn -o test` (Maven offline; deps cached) | 96 tests, 0 failures, 1 skipped (real-PG, needs `FC_JAVA_SDK_TEST_PG_URL`) |
| Go | `go vet ./... && go test ./...` | all packages pass (167 top-level tests) |
| Rust | `cargo test -p fc-sdk --all-features` | all pass (340 unit tests) |
| Platform document | `cargo test -p fc-platform --test openapi_go_contract_test` | 2 tests pass |

## Publishing from here

This repo now has:
- `.github/workflows/split-laravel-sdk.yml` and `split-typescript-sdk.yml`: the same mechanism as
  Go's, with the same target repos and secret names (`LARAVEL_SDK_TOKEN`,
  `TYPESCRIPT_SDK_TOKEN`).
- `just release-laravel-sdk <bump>`, `just release-ts-sdk <bump>` and `just release-java-sdk <bump>`.
  These are the equivalent of Go's `scripts/release.sh`: they bump from max(`VERSION`, highest
  tag), keep `package.json` / `pom.xml` in lockstep, commit, tag and push.

### Java SDK on JitPack

The owner chose JitPack for the Java SDK. `jitpack.yml` at the repo root builds only
`clients/java-sdk` (`mvn -B -f clients/java-sdk/pom.xml -DskipTests install`). The SDK compiles
for Java 25 (`maven.compiler.release`), so a Temurin 25 is installed with SDKMAN first
(`sdk install java 25.0.1-tem`); if JitPack's SDKMAN lacks that id, the first build log says so
and the id needs changing. Nothing has been built on JitPack yet.

JitPack builds a git ref on the first request for it and uses the ref's name as the Maven version,
which cannot contain `/`. `just release-java-sdk <bump>` therefore tags the release commit twice,
`java-sdk/vX.Y.Z` (the repo's SDK tag scheme) and `java-sdk-vX.Y.Z` (for JitPack), and pushes both.
A commit hash or `<branch>-SNAPSHOT` also works as a version.

Install (first release from here: 0.0.11):

```xml
<repositories>
  <repository>
    <id>jitpack.io</id>
    <url>https://jitpack.io</url>
  </repository>
</repositories>

<dependency>
  <groupId>com.github.flowcatalyst</groupId>
  <artifactId>flowcatalyst-rust</artifactId>
  <version>java-sdk-v0.0.11</version>
</dependency>
```

```kotlin
// Gradle (Kotlin DSL)
repositories { maven("https://jitpack.io") }
dependencies { implementation("com.github.flowcatalyst:flowcatalyst-rust:java-sdk-v0.0.11") }
```

JitPack also serves the artifact under its module coordinates,
`com.github.flowcatalyst.flowcatalyst-rust:flowcatalyst-sdk:java-sdk-v0.0.11`; the build page
(`https://jitpack.io/#flowcatalyst/flowcatalyst-rust`) lists what the first build published. The
pom's own coordinates, `io.flowcatalyst:flowcatalyst-sdk`, are what a Maven Central release would
use. The repository must stay public for JitPack to build it without a token.

### Go SDK module path

`clients/go-sdk/go.mod` declares **`module github.com/flowcatalyst/flowcatalyst/clients/go-sdk`**,
and every package in it imports itself under that path (e.g.
`github.com/flowcatalyst/flowcatalyst/clients/go-sdk/client`). That names the Go platform's repo,
`github.com/flowcatalyst/flowcatalyst`, which has no `clients/go-sdk`, so `go get` cannot resolve
it. Today the SDK is usable only with a `replace` directive pointing at a checkout. It has never
been published, so a rename breaks no one. Options (owner decision pending; nothing changed here):

1. **Subdirectory module in this repo:** `module github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk`.
   Consumers `go get github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk@v0.1.0` and import
   `…/flowcatalyst-rust/clients/go-sdk/client`. Go requires release tags named after the
   subdirectory, `clients/go-sdk/vX.Y.Z` (not `go-sdk/vX.Y.Z` like the other SDKs). No mirror or
   workflow is needed; the module proxy fetches only the subdirectory.
2. **Split mirror repo**, like Laravel and TS: `github.com/flowcatalyst/go-sdk`, module
   `github.com/flowcatalyst/go-sdk`, tags `vX.Y.Z` on the mirror (a `split-go-sdk.yml` like the
   others, pushing a tag per `go-sdk/vX.Y.Z` here). Shortest import path; one more repo to own.
3. **Vanity path**, e.g. `go.flowcatalyst.io/sdk`, served by a `go-import` meta tag that points at
   either layout. Decouples the import path from where the code lives; needs a hosted page.

Any of them is a one-line `go.mod` change plus rewriting the SDK's own imports (a `sed` over
`clients/go-sdk`). From v2 on, the path also needs a `/v2` suffix.

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
   4. `just release-java-sdk patch` → tags `java-sdk/v0.0.11` and `java-sdk-v0.0.11`; JitPack
      builds `java-sdk-v0.0.11` on the first request (no mirror).
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
- **Java distribution (decided: JitPack).** Set up; see "Java SDK on JitPack". The first
  `just release-java-sdk` after this merge publishes 0.0.11 there on first request.
- **Go SDK module path.** Options in "Go SDK module path"; unchanged until decided.
- **Unpublished work in `flowcatalyst-javalin`.** Its TS and Laravel webhook `check()` /
  `WebhookVerification`, router bearer handling and single-flight refresh are now ported here
  (see "SDK rulings of 2026-09-25"), and so is its Java SDK router bearer change. Not merged: a
  Jackson-3 Java SDK at 0.0.4. Its Laravel `CreateAuditLogDto` also puts `maskedFields`
  **before** `applicationCode`/`clientCode`, which breaks positional callers; the copy here puts
  it last.
- **Platform spec (done: Go's operationIds and schema names).** Still open: rulings on the
  undecided differences in "Platform OpenAPI document" (documentation text, `security`, path
  parameter names, Rust extensions), which keep the five OpenAPI parity steps DIFFs, and on
  whether to regenerate the SDKs from this spec once they are settled.
- **fc-sdk session authority** (see "Rust SDK: open item"): fix it before any Rust app uses the
  axum OIDC session against the Go-shaped platform.
