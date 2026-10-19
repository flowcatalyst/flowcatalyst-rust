# Go F25 (platform half) and the outside-ledger small items mirrored (P2 batch D, 2026-10-19)

Go's F25 (`flowcatalyst-go/docs/platform-bug-fixes.md` F25, commit `8254cf0c`) plus the five Go changes
that have no ledger entry: requeue by filter, the last all-applications administrator guard, `x-sdk`
tagging, logging every 5xx with its cause, and the S3 startup self-check. Nothing here is pushed. The
Go repo was only read. The `fcdev` CLI stays Go-only (owner decision D6); this is the platform half.

| Item | Go | Rust |
|---|---|---|
| 1. F25: built-in public client `fcdev-cli`, loopback redirect, `FC_FCDEV_LOGIN_REFRESH_TTL_SECS`, `refresh_expires_in` | `8254cf0c` | `123247c1` |
| 2. Requeue by filter (`/api` and `/bff`, preview and run) | `165feb6d`, `5496c3ff` (SPA half: P5) | `3aa51476` |
| 3. Last all-applications administrator guard, `confirmSelfRestriction` | `9966a2fa`, `01fead7e` (SPA half: P5) | `dab7e18e` |
| 4. `x-sdk` tagging | `21f9734d` (tagging only; stubs and per-SDK split: P5) | `98a2ae89` |
| 5. Every 5xx logged with its cause; artifact store failures annotated; S3 startup self-check | `c0d27994` | `aaf78687` |
| 6. OpenAPI ratchet baseline: 2 operations and 4 schemas closed | | with item 2 |

Tests: `crates/fc-platform/tests/it/{fcdev_login,requeue_by_filter,last_admin,sdk_class}_test.rs`,
the unit tests in `fc-platform-auth/src/auth/fcdev.rs`, `fc-platform-core/src/shared/error.rs`
(`five_xx_logging`), `fc-platform-functions/src/function/artifact/{mod,s3}.rs`, and
`fc-platform/src/shared/sdk_class.rs`. The integration tests need a Postgres 15+:
`FC_TEST_DATABASE_URL=postgres://user:pw@host:port/postgres cargo test -p fc-platform --test it -- --include-ignored fcdev_login_test:: requeue_by_filter_test:: last_admin_test:: sdk_class_test::`.

## 1. F25, the platform half

What Go does, and what Rust now does the same way:

- **The client `fcdev-cli`** is seeded at every start (`database::seed_fcdev_cli_client`, called by
  fc-server and fc-dev beside the role and application seeds) and its security-relevant columns and
  junctions are overwritten each time, so a console edit cannot widen it: PUBLIC, no secret, PKCE
  required, `api_access` (the access token is `token_use=api` and carries the user's own roles and
  `scope`), grants `authorization_code` and `refresh_token` only, redirects `http://127.0.0.1/callback`
  and `http://[::1]/callback`, no application scoping, no allowed origins, not a portal client. Only
  `active` is left alone: deactivating the client switches fcdev logins off and a restart does not
  undo it.
- **Loopback redirect (RFC 8252 section 7.3).** `auth::fcdev::match_loopback_redirect_uri`, consulted
  by `/oauth/authorize` only when `client_id` is `fcdev-cli`: the redirect must be the registered
  port-less URI for the same host with a canonical decimal port 1-65535 added and nothing else (host
  literally `127.0.0.1` or `[::1]`; never `localhost`, `https`, userinfo, query, fragment, another path).
  Go closes the rule by rebuilding the URI from its parsed parts and demanding byte equality. Rust
  parses positionally (scheme prefix, host literal, `:port`, then a path that must be built of
  unreserved characters and equal a registered path), so there is no URL-parser differential to
  exploit. Go's 50-row table is ported row for row as a unit test. The token endpoint still demands the
  `redirect_uri` the code was issued for.
- **Lifetime.** `FC_FCDEV_LOGIN_REFRESH_TTL_SECS` (default 604800, unset, junk or non-positive means the
  default), independent of `FC_REFRESH_TOKEN_EXPIRY_SECS`. Applied when the code is redeemed
  (`OAuthState::fcdev_refresh_ttl_secs`); `refresh_rotation::rotate` already copies the presented
  token's `expires_at` forward, which is what makes it absolute. Read when the router is built.
- **`refresh_expires_in`.** One additive optional field on the `/oauth/token` success body, seconds
  until the refresh family's absolute deadline, present with `refresh_token` on both the code and the
  refresh grants (Go `secsUntil`, floored at zero). `int64` in the schema. The ratchet compares schema
  names only, so no baseline line moves.
- **Account cut-off.** The authorization-code grant already refused an inactive principal here
  (`Account is not active`, `invalid_grant`; Java S2.2, `oauth_api.rs`); verified, and Go's test ported
  (`removed_roles_narrow_and_a_deactivated_user_is_cut_off`: a code issued before a deactivation cannot
  be redeemed, and neither can the refresh token).

Ported from Go's `fcdev_login_pg_test.go` (Go's 11 tests as 10: "login required" and "PKCE enforced" share one): seed shape and re-assertion
(including the console-widening edit and the deactivation), happy path with claims, login required,
PKCE missing, `plain`, wrong and missing verifier and a secret on a public client, the redirect matrix and
"other clients get no port variance", rotation with absolute expiry and reuse detection, the
configurable TTL, client binding both ways, removed roles and a deactivated user, revoke, and a
deactivated client. All passed on the first run against Postgres 18.

Difference kept: Go's clients repository does not cache, Rust's `OAuthClientRepository` caches by
`client_id` for 5 minutes (invalidated by console writes in the same process). A client deactivated by raw
SQL in another process is honoured within 5 minutes; a console deactivation is immediate. Unchanged by
this batch, and it applies to every client.

## 2. Requeue by filter

Go: `POST /api/dispatch-jobs/requeue-by-filter/preview` takes the list's filter and answers
`{count, byStatus, eligibleCount}`; `POST /api/dispatch-jobs/requeue-by-filter` takes the same filter plus
`confirmCount` (the `eligibleCount` the caller saw) and an optional `limit` (default and maximum 10,000).
If more jobs are eligible now than were confirmed it resets nothing and answers
`409 FILTER_MATCH_CHANGED` with the new counts in `details`; otherwise it walks the matching ids in id order
(keyset, 500 a page) and hands each page to the existing requeue use case: one transaction, event and audit
row per page. It never resets more than `min(limit, confirmCount)` jobs and answers
`{requeued, truncated, remaining}`. An unparseable `since` or `until` is a 400 `INVALID_TIMESTAMP` on the
requeue routes (the list still ignores it). Both `/bff` twins exist. Same gate as requeue
(`dispatch-job:view`); per-job tenant reach stays in the use case, and `eligibleCount` counts the jobs the
caller's reach lets the use case reset.

Rust: `dispatch_job_actions/by_filter.rs` and `MatchFilter`, `count_matches`, `matching_ids` in
`dispatch_job_actions/repository.rs`. The walk reuses `RequeueDispatchJobsUseCase`, so every page is one
UoW run with its `DispatchJobsResent` event and `ResendCommand` audit row, as in Go.

Difference, to unify later: Go builds the list, the count and the id walk from one `FilterParams.where()`.
Rust's list builds its WHERE in `dispatch_job/repository.rs::find_read_filtered`, which batch C is editing
(application confinement), so the count and walk have their own `push_conditions` with the same
conditions, in the same order. They should share one function once batch C has landed; the tests assert
the walk and the preview against the same seeded rows. Ported 9 of Go's 10 tests. Not ported: Go's
`TestRequeueByFilter_ConcurrentSchedulerClaim` (the scheduler claim racing a run) and the plan test
`requeue_filter_plan_pg_test.go` (EXPLAIN shapes on Go's indexes); Rust's `msg_dispatch_jobs_read` indexes
are Go's, but the plan was not re-checked.

## 3. The last all-applications administrator guard

Go: any write that leaves no ACTIVE USER of tier ANCHOR with `all_applications` set is refused with
`409 BUSINESS_RULE LAST_ALL_APPLICATIONS_ADMIN`, serialised by a transaction-scoped advisory lock
(`pg_advisory_xact_lock(0x464341444d494e01)`) so two administrators demoting each other cannot both
succeed. It covers assign application access, deactivate, update, delete, scope change and sync, because it
sits in the principal repository's `Persist` and `Delete`. Service principals never count. Removing your own
flag needs `confirmSelfRestriction: true` or the request is `409 SELF_RESTRICTION_REQUIRES_CONFIRMATION`.

Rust: `fc-platform-iam/src/principal/last_admin.rs`, called from `Persist<Principal>`, `Persist<Principal>::delete`
and `Persist<PrincipalSyncBatch>`; the use-case unit of work already passes a `BusinessRule` persist error
through with its code. `confirmSelfRestriction` is on `SetApplicationAccessRequest` (the schema
`AssignApplicationAccessRequest`). Go persists a sync's principals one at a time; Rust's sync is one UNNEST
statement, so the batch guard checks the end state (administrators outside the batch plus the batch's own
incoming administrators) rather than each principal in turn. The outcomes agree: a batch is refused exactly
when it would leave none.

Ported: all nine tests of `lastadmin_pg_test.go` (assign refused then allowed; nil flag and grant
unaffected; service principal and inactive admin do not count; deactivate, update and delete refused;
scope change both modes; sync; 15 rounds of concurrent mutual demotion; the deterministic lock test, done
here by holding the advisory lock from another transaction and requiring the demotion to block) and the
three of `self_restriction_pg_test.go`. Go's `storeFailure` change (a repository's non-internal errors pass
through the unit of work) is already how `write_failure` behaves here.

## 4. `x-sdk` tagging

`shared/sdk_class.rs` holds Go's three lists verbatim (`APP_OPS`, `INTERNAL_OPS`, `ADMIN_TAGS`) and
`classify` stamps `x-sdk: app|admin` on the published document; absent means internal. Added for what the
Rust platform has and Go lacks: `INTERNAL_TAGS` (`auth`, the session endpoints; `monitoring`, the router
dashboard reads), the `function-policies` tag as admin, and `postApiDispatchJobs` (the Rust spelling of
`createDispatchJob` while that operation-id gap is open). The pass runs in `router::build` after
`shape_as_go_contract` and only warns on an operation no list decides; `sdk_class_test.rs` fails on one.
Tests: every operation is classified; the class of the 276 operations both platforms document equals Go's;
every listed id exists in this document or Go's; the extension is in the served document.

Not done here (plan P5): the 13 spec-only stubs for Go's plain-handler operations (`/oauth/*`, `/api/me*`,
`/.well-known/*`, `audit-logs/batch`, `dispatch-jobs/batch`), which are still `go-only` lines of the
baseline, and the per-SDK `openapi.app.json` / `openapi.admin.json` split (`make sdk-spec` is Go tooling).

## 5. Every 5xx logged, store failures annotated, S3 self-check

- `PlatformError::into_response` logged only `500`; it now logs every 5xx once, at ERROR, with `code`,
  `status` and the cause, with any URL's query string replaced by `?REDACTED` (a presigned URL's
  signature). The request's method and path come from the request span the binaries' `TraceLayer` opens.
- `DiagnosedBlobStore` wraps both artifact stores (`file://` and `s3://`): a transport failure now reads
  `artifact store store=s3 bucket=B prefix=P op=exists digest=D: <cause>` (Go `StoreError`), sentinels
  (`NotFound`, `BadRef`) pass through. The S3 store adds Go's two hints (credentials not resolvable by the
  default chain; access denied, check the task role's s3 permissions) and names the SDK operation.
- **S3 startup self-check.** When `FC_FN_ARTIFACT_STORE` is `s3://`, a background task logs the effective
  bucket, prefix and region and whether the credential chain resolves (`credentials_resolved`,
  `credentials_expire`; values are never logged). A chain that does not resolve is an ERROR line, never a
  failed start. Go logs the provider's `Source` name; the Rust SDK exposes none, so that field is absent.

Env name: Go calls the variable `FC_FUNCTIONS_ARTIFACT_STORE`; the Rust platform's function model reads
`FC_FN_ARTIFACT_STORE` (Java's). The name belongs to the function-model work (plan P4, decision D1), so it
is unchanged here and the self-check follows whichever the store is configured from.

## Inferred versus verified

Verified by a test that failed under a mutation (19 mutations, each caught; listed in the commit messages): the loopback
rule is fcdev-only, the fcdev TTL is applied, the seed re-asserts PKCE, the inactive check on the code grant,
the eligible count follows tenant reach, `confirmCount` is enforced, the last-admin lock and delete guard,
the self-restriction check, the shared-operations class equality and the 5xx log scope. Inferred from
reading Go and not exercised: the S3 hint wording on a real AWS failure (the SDK's error text was matched by
substring on a synthetic string), the self-check against a real credential chain.
