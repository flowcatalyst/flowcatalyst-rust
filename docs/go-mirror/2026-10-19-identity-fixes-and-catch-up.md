# Identity review fixes and the F28-F34 catch-up (2026-10-19)

Rust side of Go's `docs/identity-go-vs-rust-2026-10-19.md` (the review at Go `b893efd4`), plus the Go ledger items F28
to F34 and the other platform behaviours `PENDING.md` still listed, mirrored in the order the owner set. Go was read,
never changed. Nothing is pushed. The Go agent fixing the Go side in parallel landed F36-F45 (`377fe770` to
`e64ac802`); the ones that touch identity are mirrored below with Go's switch names, so one environment file drives both
implementations.

The rule for the whole batch: nothing a valid client, SDK, the vendored SPA, `fcdev login`, a service account or the Go
parity contract does today may start failing without a switch that keeps today's behaviour as the default.

## Part A: the review's Rust findings

| # | Finding | Commit | What changed | Default |
|---|---|---|---|---|
| R1 | wildcard redirect URI matcher is a string matcher (HIGH) | `7c960868` | New `fc-platform-auth/src/auth/redirect_match.rs`. The incoming URI is taken apart by hand (scheme, authority, path) and cross-checked against the WHATWG `url` parser (host, effective port, no userinfo). Refused outright: userinfo, any `%` or non-DNS character in the authority, backslash, whitespace or a control character anywhere, an empty or trailing-dot host. `*` is confined to parts of whole host labels; the last two labels of a host pattern must be literal (`https://*`, `*.com`, `app.example.*` match nothing). Paths with `.`/`..` segments or an encoded dot, slash or backslash never match a wildcard pattern. Reproduced first: the old function returns `true` for `https://evil%2Ecom/.example.com/callback` against `https://*.example.com/callback`. Go's `TestMatchRedirectURI` matrix runs row for row. | on |
| R2 | no timing equalisation on the main password login | `f3305654` | `PasswordService::equalize_timing` (one Argon2id verification of a fixed per-service hash) on the unknown / inactive / password-less path of `password_login`; the portal login's private copy now calls it. Measured before the fix: unknown email 1 ms, wrong password on a real account 78 ms. | on |
| R3 | F34 `token_use` denylist | `f0b06002` | `AccessTokenClaims::is_api_credential` (`api`, none or empty) in `build_context`; the function-endpoint verifier (`fc-platform-jwks`) is an allowlist too, and a non-string marker is refused. The session-cookie path already refused any token carrying `token_use`, `type` or `jti` (`validate_session_token`), so no change there. `fc-router` was already an allowlist (`api` only). | on |
| R4 | 60 s JWT leeway | `4292eef3` | `FC_JWT_LEEWAY_SECS` (0 to 600), default **0** on the platform's token validation and the upstream id_token check. Decision: lowering is safe because the platform mints and checks its own tokens and `nbf` is not validated, so nothing depends on post-expiry acceptance; the switch exists for a deployment with skewed consumers. `fc-router/src/api/auth.rs` (its own OIDC validation) and `fc-sdk` keep their settings; `fc-platform-jwks` was already exact. | 0 |
| R5 | unbounded bearer cache | `f96e544b` | Entries older than the 30 s TTL are swept on insert (once per TTL), and the cache is emptied if it reaches 10,000 entries inside one TTL. | on |
| R6 | upstream OIDC verifier | `345d62a5` | A token naming a `kid` the cached JWKS lacks refetches it, at most every 30 s per issuer (a failed refetch keeps the cached set); a kid-less token is verified against each published key instead of only the first. Go supports RS256 only here (go-oidc's default), so ES256 was not added; RSA behaviour is unchanged. | on |
| B1 | introspect / revoke authorise any caller | `b16378a8`, `d53a827d` | Same switches and defaults as Go's `81a6f588`: `FC_OAUTH_REVOKE_ENFORCE_CLIENT` (default true: a client revokes only refresh tokens issued to it; a Bearer caller and an unbound token are unchecked) and `FC_OAUTH_INTROSPECT_CLIENT_AUTH_ONLY` (default false). Every shipped SDK (`fc-sdk`, `go-sdk`) revokes its own client's tokens with that client's credentials; no in-repo caller revokes another client's. | revoke on, introspect off |
| B2 | `email_verified` ignored | `0ecf4a07` | `FC_OIDC_REJECT_UNVERIFIED_EMAIL`; rejects only an explicit `false` (bool or the string) on the `email` claim; covers the employee and the portal SSO sinks (one validator). | off |
| B3 | unanchored issuer regex | `0ecf4a07` | `FC_OIDC_ISSUER_PATTERN_STRICT` makes it a full match; off, an unanchored pattern logs a warning once. One matcher serves the login id_token check and `ClientAuthConfig::is_valid_issuer`. | off |
| Go a24f549b | return URL accepts control characters | `819e6ead` | `/<TAB>/evil.com` passed `safe_relative_return_url` (password-reset redirect). Now refused; `determine_redirect_url` only honours a same-site path (nothing stores a return URL today). `fc-web/src/app/login.rs` has a `/ui`-prefix guard that a tab cannot defeat; fc-web is outside the workspace and was not touched. | on |

Already true here, verified in code: atomic refresh rotation with the 10 s replay leeway and atomic TOTP claim (Go's
F36/F37 copy these), one session-cookie constructor with `FC_SESSION_COOKIE_SAME_SITE` (F39), webauthn-rs refuses a
counter regression and the authenticate use case persists the counter in its transaction (F43), no test-header bypass
(F38 is Go-only), no PKCE or client-id-message change (B4, B6: by design, same as Go).

### Not enabled by default, and why

| Switch | Why it is not on |
|---|---|
| `FC_OIDC_REJECT_UNVERIFIED_EMAIL` | Keycloak realms create users with `email_verified=false` as a matter of course; turning it on locks them out of SSO. Absent claims (Entra) always pass. |
| `FC_OIDC_ISSUER_PATTERN_STRICT` | An operator may have registered a prefix pattern that only works as a partial match. Warn-once names every unanchored pattern so they can be fixed first. |
| `FC_OAUTH_INTROSPECT_CLIENT_AUTH_ONLY` | Resource servers introspect with a Bearer today. |
| `FC_SERVICE_ACCOUNT_SCOPE_REACH_STRICT` (F28) | The vendored SPA's service-account drawer sent the old links beside `scope: ANCHOR` (the bug Go's SPA fix addresses) and a client may send `CLIENT` with no links; both are accepted today. The drawer in `frontend/src` now carries Go's fix (`vue-tsc` clean); rebuild the frontend (`make frontend`-style, the embedded copy goes stale otherwise), then turn it on. |
| `FC_IDP_MAPPING_REQUIRE_ROLE` (F33 write check) | A mapping may be written before the application that owns the role has synced. The login-time half (skip a stale mapping) is on. |
| `FC_JWT_LEEWAY_SECS` is the one switch whose default (0) is stricter than before: set `60` to get the old behaviour. |

## Part B: Go ledger catch-up

| Item | Go | Rust commit | Notes |
|---|---|---|---|
| F28 `SCOPE_REACH_MISMATCH` | `ebb18542` | `5592932a` | `require_scope_matches_reach` in create and update; update compares the merged result and only when `scope` or `clientIds` is in the request; the token never follows the stored scope. Behind the switch above. |
| F29 router-config ETag/304, 30 s poll, nudge | `0fb7f0c7` | `8a6f1a0e` | Strong ETag (sha-256 of the rendered JSON, `If-None-Match` as a list, weak validators and `*`); `ConfigSyncService` keeps a validator per URL beside the cached document, reuses it on 304, sends none for a source without an ETag; default poll `DEFAULT_SYNC_INTERVAL` = 30 s (`FLOWCATALYST_CONFIG_INTERVAL` still overrides; the production task definition sets 300 explicitly, so it keeps 300 until the IaC is changed); missing-queue recheck paced (young under 10 min every tick, all every 5 min); `set_router_config_hook` called when a client, dispatch-pool or subscription event is written, wired to `ConfigSyncService::nudge` in `fc-server` (1 s delay, a confirming second fetch). `fc-dev` runs the built-in document and has no watcher. A separately run router relies on the poll. |
| F30 no rewrite / `updated` for unchanged rows | `87cbc30f` | `9725441f` | SDK roles, catalogue roles, connections, subscriptions. Permissions and binding codes compare as sets; an omitted permission list still keeps the stored ones; an adopted id-less role or a healed application id counts. Rust stores no binding filter, so none is compared. |
| F31 `attemptCount`, rollup subject | `4f2b3908` | `fa6dae50` | `lifecycle::complete` and `fail` bump `attempt_count`; the scheduled-job rollup subject is `platform.scheduledjobs.<app>`, so the audit row reads `Scheduledjobs` / `<application code>`. |
| F32 `{appCode}` routes 404 | `49ba1494` | `e8410ba6` | Already the behaviour since F20 (owner ruling); the test pins the body code too. |
| F33 stale IdP mapping | `281d40d3` | `8e71db64` | `OidcSyncService::with_role_repo` (wired in `auth_init`); both sync paths skip a mapping whose role is missing with a warning. Write-time check behind the switch. |
| F34 token_use | `8165e614` | `f0b06002` | See R3. |
| F35 function service account | `f0e70ee4` | none | Go function model (`fng_functions.service_account_id`, Go 084); left with F22-F24 for P4. |
| 429 for scheduled jobs and the terminal `reject` outcome | `6e6b9307` | `7748677d` | `defer_delivery` + `not_before` filter in the dispatcher's queue query; `Retry-After` in seconds or HTTP date (default 30 s, at most 1 h); `FlowCatalyst-Outcome: reject` on a 422 fails a dispatch job or a firing on that attempt (a plain 422 still retries). This is the only change with a new `.sqlx` entry (regenerated with `scripts/sqlx-prepare.sh`; 700 to 701 files, none deleted). |
| requeue WHERE-builder unification | (parity-plan batch D) | `a34dfc92` | `dispatch_job::read_conditions` is the one builder; the list, the preview count and the id walk all use it. The shared builder carries `accessible_applications` (the list's application confinement); the requeue match does not set it, because the actions state has no application scope and per-job reach stays in the use case, so behaviour is unchanged. |
| `b591d703` race on cancel / complete / edit payload | `b591d703` | none | Already a 409 here (`NOT_FAILED`, `NOT_EDITABLE`), not a 500; Go's new code `STATUS_CHANGED` was not adopted because the existing codes are documented and tested. |

## Left, and exactly what would break

- **Router head-of-line reservation schedule** (Go `1c37a339`, 2026-09-22). An L router redesign: the consumer keeps polling
  while any pool has room or a per-queue deferral budget lasts, a full pool Defers with a completion-rate reservation, SQS
  Defer becomes a real `ChangeMessageVisibility`, NATS `max-deliver` and `max-ack-pending` go unlimited, buffers double.
  Rust defers by the fixed `CAPACITY_DEFER_SECONDS`. Rushing it without a load rig risks reordering within a message
  group and unbounded NATS redelivery; it needs its own batch and the bench rig.
- **R7** (`AuthService::new` falls back to HS256 on a bad RSA key, with the default empty secret). Production boot calls
  `new_with_rsa` and fails closed (`auth_init.rs`); `AuthService::new` is what many unit and integration tests build with.
  Removing the public fallback means rewriting all of them.
- **R8** key bootstrap into `./.jwt-keys/` stays: dev convenience, documented as dev only in `docs/operations/configuration.md`.
- **R6 tail**: `decode_jwt_payload_unsafe` still reads the IdP access token's payload unverified into the login event
  (data quality, not an authorisation decision).
- **B4, B6, B7**: distinct "Unknown client" text, PKCE only when the client's flag says so, and the 2FA step's backoff
  failing open on a repository error are the same on both sides and by design.
- **F22-F24, F35, version pruning, the SPA vendoring (P5), `fcdev`, the licence**: out of scope as briefed.

## Found, Go should also look

- Go's `safeRelativeReturnURL` fix covers `bridge` and `passwordreset`; check `fcdev`'s loopback-callback landing page
  and the templ UI's `next` handling for the same tab/CR/LF trick (Rust's `fc-web` `safe_next` only accepts `/ui...`).
- Go's `FC_OAUTH_REVOKE_ENFORCE_CLIENT` leaves a Bearer caller unchecked, so any valid access token can still revoke any
  refresh token it can name. Rust copies Go; if the Go owner wants it closed, the rule that fits both is "a Bearer
  caller revokes its own principal's tokens, an anchor anyone's" (written and tested here first, then reverted to match
  Go; see `b16378a8`).
- The review's R3 note that `fc-router/src/api/platform_auth.rs` refuses cookie tokens while Go admits "api or absent":
  unchanged, the router is stricter.
- `cors/layer.rs` has its own `*` origin matcher (one DNS label). The origin comes from the browser's `Origin` header
  (already normalised), so the string tricks do not apply, but it is the same shape of code and worth the same table test.

## Tests

- New unit tests: `redirect_match` (5 incl. Go's 24-row matrix), `equalize_timing`, token_use allowlist (iam middleware
  and jwks), leeway parse and expiry, bearer cache bound, JWKS refetch / rate limit / kid-less (3 against a fake IdP),
  ownership, issuer pattern, `email_verified`, return URLs, scope/reach table, router-config ETag, paced recheck,
  ConfigSync 304 / no-ETag / nudge, hook family, `read_conditions`, `retry_after`, `is_delivery_rejected`.
- New integration tests (`FC_TEST_PG_BIN=~/.theseus/postgresql/18.3.0/bin`): `identity_hardening_test` (timing, wildcard
  redirect end to end, revoke, introspection default, F28 on both use cases), `go_catchup_test` (router-config 304, the
  F30 syncs, F31 audit key, F33 both halves), `scheduled_job_delivery_test`, plus changes to `dispatch_process_test`
  (attempt counts, reject), `requeue_by_filter_test` (list = count = walk), `application_users_test`.
- Mutation checks run for R2 (equalise removed: the test fails, 1 ms against 78 ms).
- Known failures in this environment, none new: Docker-only tests (`api_integration_tests`, `function_table_prefix_test`,
  `id_roundtrip_test`, `dispatch_scheduler_test::the_scheduler_publishes_to_its_tenants_sqs_fifo_queue`,
  webauthn / backoff Docker tests) and the three 200k timing bounds in `dispatch_claim_plan_test` (documented in
  `2026-10-12-platform-bug-fixes-3.md` and `-8.md`).
