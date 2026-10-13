# Go platform bug fixes, seventh batch, mirrored (2026-10-14)

Go's F16, the cookie-session CSRF rule and the SPA serving rules, and its follow-up, login CSRF on
the public sign-in routes, the SSO callback's browser binding and the debug gate
(`flowcatalyst-go/docs/platform-bug-fixes.md` F16, "F16 follow-up" and decisions 23 and 24;
`flowcatalyst-go/docs/wire-contract.md` "Error envelope" and "Authentication"). Go `94a4c79`,
`ff9e495`, `91bc61c` (F16) and `6eb4b0f`, `2cdff75`, `780e406`, `91c355f` (follow-up). Every commit
is on `idiomatic-rust`, not pushed.

The earlier batches are `2026-10-10-platform-bug-fixes.md`, `2026-10-11-platform-bug-fixes-2.md`,
`2026-10-11-event-catalogue.md`, `2026-10-12-platform-bug-fixes-3.md`,
`2026-10-13-platform-bug-fixes-4.md`, `2026-10-14-platform-bug-fixes-5.md` and
`2026-10-14-platform-bug-fixes-6.md`.

Tests named `*_test::` are in `crates/fc-platform/tests/it/` (`FC_TEST_PG_BIN=<postgres bin> cargo
test -p fc-platform --test it -- --include-ignored <module>::`).

## Summary

| Item | Go | Rust commit |
|---|---|---|
| 1. Cookie-session rule: provenance and JSON only for cookie-authenticated writes | `94a4c79` | `1a7ab929` |
| 2. SPA serving: API 404s, asset 404s, revalidated shell, security headers | `ff9e495` | `b7430c13` |
| 2a. The session-cookie test helper sends `Sec-Fetch-Site: same-origin` (item 1's fallout in older tests) | (Go's tests) | `a6847e65` |
| 3. Login CSRF: JSON only and cross-origin refusal on the public sign-in routes; every public unsafe route decided | `6eb4b0f` | `13f099b3` |
| 4. SSO callback bound to the browser that started the login | `2cdff75` | `353b8618` |
| 5. Debug endpoints | `780e406` | none: no such surface (below) |
| 6. CORS without credentials (Rust only: Go has no CORS layer) | none | `f1029b1f` (CORS follow-up, below) |
| Ledger | `91bc61c`, `91c355f` | (this file) |

## Observed before the fixes

Verified empirically first, through the whole platform router (`router::build` under the
`AuthLayer`, as the binaries build it) on the `FC_TEST_PG_BIN` harness, with a stored super-admin's
session cookie and `Sec-Fetch-Site: same-site`, `Origin: https://evil.example.com` (a sibling
subdomain; SameSite=Lax sends the cookie there) unless stated. The SPA authenticates with the
`fc_session` cookie (`SameSite` from `FC_SESSION_COOKIE_SAME_SITE`, Lax by default, `None`
allowed), resolved per request by the extractors (`Authenticated`), with no check of where a write
came from.

Exploitable:

- **Body-less actions**: `POST /api/clients/{id}/activate` (no body) on a deactivated client: **200
  "Client activated"**; the same with `Sec-Fetch-Site: cross-site`, with a foreign `Origin` and no
  `Sec-Fetch-Site`, and with neither header: accepted (409 `ALREADY_ACTIVE` once active, i.e. the
  handler ran).
- **Handlers that read their body as bytes** (`body: Bytes` + `serde_json::from_slice`, or the
  portal `RawBody`) decode JSON whatever the `Content-Type`: `POST /api/function-domains` with a
  `text/plain`, a form-typed, and a `Content-Type`-less JSON body: **201, three domains claimed**;
  `POST /api/functions` and `POST /api/portal-users` with `text/plain` reached their business
  checks (404 for the missing application / portal app); `POST /auth/change-password` with
  `text/plain` was decoded (400 `NO_PASSWORD`, the account had none). The same pattern serves the
  2FA self-service routes, the MFA, password-reset and login routes.
- **`DELETE /api/clients/{id}`: 204** (in a browser a cross-origin DELETE needs a CORS preflight:
  see "CORS" below).
- **Login CSRF**: `POST /auth/login` with a password user's credentials as a `text/plain` body,
  `Sec-Fetch-Site: cross-site`, `Origin: https://attacker.test` (what `<form enctype="text/plain">`
  sends): **200 with an `fc_session` cookie** (`login_csrf_test`, before item 3). Every sign-in route
  decoded such bodies (`body: Bytes`).
- **SSO login CSRF**: with a test identity provider served in-process (discovery, JWKS, a token
  endpoint signing a real RS256 ID token; `sso_binding_test`), `GET /auth/oidc/callback?state=…&code=…`
  for a login started elsewhere **signed the browser in** (303 with `fc_session`): the login state
  lived only in `oauth_oidc_login_states`.
- **SPA serving** (`router::serve_spa`, the `static_dir` server of fc-server and fc-dev's
  `FC_STATIC_DIR`; fixture tree, `tower::oneshot`): `GET /favicon.ico` (missing) and every unknown
  `GET` under `/api`, `/bff`, `/oauth`, `/.well-known`, `/control`, `/q` answered **200 with
  `index.html`** (`no-cache, no-store, must-revalidate`); a missing `/assets/index-OLDHASH.js` 404'd
  but with **`Cache-Control: public, max-age=31536000, immutable`** (a cacheable 404 for a year);
  no ETag (Last-Modified only); no security headers. fc-dev's embedded handler (read, same
  structure) also answered a missing `/assets/*` with the shell.

Not exploitable:

- **Typed `Json<T>` routes** (most of `/api` and `/bff`, e.g. `POST /api/clients`,
  `POST /bff/roles/permissions`, `POST /api/clients/{id}/deactivate`): axum's `Json` extractor
  refuses a `text/plain`, form, multipart or `Content-Type`-less body; the platform's rejection
  layer answers **400 `VALIDATION` "request body is required"**. A cross-origin JSON body needs a
  CORS preflight.
- **GET routes**: the documented GET handlers were scanned for writes (use cases, `unit_of_work`,
  inserts, cookie writes): only the OAuth/OIDC flow (`/oauth/authorize` stores a code with the
  client's `state`/PKCE; `/auth/oidc/login` stores a login state; `/auth/oidc/callback` signs in,
  now bound; `/auth/oidc/session/end` clears the cookie by design, as Go) and in-memory map
  operations. No method override (`X-HTTP-Method-Override`, `_method`) is honoured.
- **Non-simple methods and JSON bodies** need a preflight; fc-dev answers preflights with
  `allow_origin(Any)` and no credentials, so a credentialed one fails (see "CORS" for fc-server).
- **Debug/profiling endpoints**: there is no pprof or dump surface on the platform. The metrics
  port's `/diagnostics/runtime` and `/diagnostics/task-dump` take a platform **bearer** only (no
  cookie, no Basic: `platform_auth_middleware`), so a link cannot carry an ambient credential; the
  router API is bearer-only outside dev mode (dev mode's legacy Basic/OIDC-flow modes aside, below);
  `/bff/debug/*` are cookie-authenticated **GET reads** with no side effect (a cross-site navigation
  shows the victim their own data). Go's `780e406` therefore has nothing to mirror.
- **`/ui` (Topcoat trial, fc-dev `--features web` only)**: Topcoat's router applies its own
  `OriginPolicy` (Sec-Fetch-Site/Origin) to every unsafe request, as Go's `/ui` keeps `guard.CSRF`.

## 1. Cookie-session rule (`1a7ab929`)

`shared::session_csrf::session_csrf` (`crates/fc-platform/src/shared/session_csrf.rs`), an axum
middleware layered in `router::build` outside the profile-only gate, so it runs right after the
binaries' `AuthLayer` (as Go's right after the Authenticator, before `ProfileOnlyWithoutRole`). It
looks at an unsafe request (POST, PUT, PATCH, DELETE) only when it presents the session cookie and
no `Authorization` header (which decides on its own, Go `extractToken`), and authenticates it once
(`resolve_context`, cached for the handler's extractor); it applies when the context's credential
is the session cookie (`AuthContext::via_session_cookie`). The rule (Go `SessionCSRF`):

1. `Sec-Fetch-Site` `same-origin` or `none` passes; any other value is refused unless `Origin` is a
   trusted origin; without it, `Origin` must name the request's `Host` (scheme ignored, as
   `CrossOriginProtection`) or a trusted origin. The trusted origin is the public base URL, the
   JWT issuer (`FC_JWT_ISSUER` / `FC_EXTERNAL_BASE_URL` / `EXTERNAL_BASE_URL`, as Go's
   `cfg.JWTIssuer`); a value that is not an http(s) URL is skipped with a warning.
2. Neither header: refused.
3. Refusal: **403 `CROSS_ORIGIN_REQUEST`**, `Cache-Control: no-store`, Go's message; logged (warn)
   with method, path, `Origin`, `Sec-Fetch-Site`.
4. Then a request with a body (a non-zero `Content-Length`, `Transfer-Encoding`, or a body whose
   size is not known to be zero) must declare `application/json` or `application/*+json`, parsed as
   Go's `mime.ParseMediaType` (a malformed value, e.g. `application/json; =`, is not JSON); else
   **415 `UNSUPPORTED_MEDIA_TYPE`**.

Not affected: safe methods; bearer requests (also with the cookie beside them); an `Authorization`
header of another scheme; a stale cookie (it authenticates nothing; the route answers 403
`UNAUTHENTICATED`); the routes Go serves outside its authenticated group
(`outside_the_authenticated_group`: `/oauth/*`, `/.well-known/*`, `/auth/check-domain`, `/auth/login`,
`/auth/logout`, `/auth/refresh`, the public `/auth/2fa/*` steps, `/auth/password-reset/{request,confirm}`,
`/auth/password-setup/request`, `/api/dispatch/{process,settled}`).

**Tests** (`session_csrf_test`, Go `crossorigin_pg_test.go`; unit `the_rule_matrix`, Go
`TestSessionCSRF`; `a_trusted_origin_must_be_an_http_origin`, `media_types_parse_as_go_parses_them`):
cross-origin cookie writes refused for five provenance variants (sibling, cross-site, foreign
Origin only, neither header, `Origin: null`) across a body-less action, a bytes-reading route with
`text/plain` and with no `Content-Type`, a typed JSON create, a BFF route, a DELETE and a session
self-service route, with nothing written; a cross-origin non-JSON body is 403 first; a role-less
user gets `CROSS_ORIGIN_REQUEST`, not `NO_PLATFORM_ROLE`; same-origin, `none`, Origin-matches-Host,
rewritten Host with the public Origin (with and without same-site fetch metadata) allowed, and a
GET never refused; text/plain, form, multipart, `application/wasm` and no-`Content-Type` bodies 415
on three route kinds, `+json` accepted; bearer requests (foreign Origin, no headers, beside the
cookie) allowed and still decoded as before; the public routes and a stale cookie not treated as a
cookie session.

**Failing first**: `cross_origin_cookie_writes_are_refused` (200 "Client activated"),
`same_origin_cookie_requests_are_allowed` (201 for the wrong-scheme Origin behind a proxy) and
`non_json_cookie_bodies_are_refused` (400 `VALIDATION`) failed with the guard disabled; the two
exemption tests passed (they pin non-refusal). **Mutants** (each killed): neither header passes;
bearer treated as a cookie session; JSON rule dropped; a stale cookie counted as a session; the
issuer not trusted; `/auth/login` or `/oauth/*` not exempt; every request taken to have a body; a
trusted Origin ignored when fetch metadata is present; the Origin-Host match dropped; any Origin
passes. Not wiring the layer is the failing-first run.

## 2. SPA serving (`b7430c13`)

`shared::spa` (`crates/fc-platform/src/shared/spa.rs`): one handler for both ways Rust serves the
SPA, `SpaFiles::Dir` (fc-server's `static_dir`, fc-dev's `FC_STATIC_DIR`, read per request) and
`SpaFiles::Embedded` (fc-dev's `rust-embed` build, its build-time SHA-256). `router::serve_spa_files`
mounts it as the fallback and for the GET of the four client routes that share a path with a POST
API route (`/auth/login`, `/auth/forgot-password`, `/auth/reset-password`, `/auth/set-password`;
fc-dev's embedded path lacked the last). Rules (Go `frontend.Handler`):

1. a path under `/api`, `/bff`, `/oauth`, `/.well-known`, `/control`, `/q` (whole first segment;
   `/apiary` is a client route): **404** `{"error":"NOT_FOUND","message":"No route for <M> <path>"}`,
   `no-store`, any method;
2. other methods than GET and HEAD: **405**, `Allow: GET, HEAD`, `no-store`;
3. an existing file: its MIME type, a content-hash `ETag` (the first 16 bytes of its SHA-256 in hex,
   quoted, as Go's), **304** on a matching `If-None-Match` (weak comparison, lists, `*`);
   `/assets/*` `public, max-age=31536000, immutable`, any other file `no-cache`;
4. a missing file under `/assets/` (or `/assets` itself) or with a static-asset extension (Go's
   allow-list): **404**, `text/plain`, `no-store`;
5. everything else: `index.html`, **`no-cache`** with its ETag (was `no-cache, no-store,
   must-revalidate`);
6. every answer: `X-Content-Type-Options: nosniff`, `Referrer-Policy: same-origin`,
   `X-Frame-Options: DENY`, `Content-Security-Policy: frame-ancestors 'none'; object-src 'none';
   base-uri 'self'` (no script, style or connect policy).

Paths are percent-decoded and cleaned against the root (`..` never climbs out). A frontend with no
`index.html` answers 404 "Frontend not built" (Go `missingFrontend`). A GET to an API route
registered for another method stays 405 (axum's), as Go's `mountFallback` now does.

**Tests** (`router::tests`, Go `frontend/handler_test.go`): client routes get the shell (`/apiary`
included); missing assets 404 `no-store`; unknown API paths 404 JSON for GET, POST and DELETE and
the GET-to-POST-only route 405; unsafe methods on a client route 405 with `Allow`; cache headers,
ETag equality across `/`, `/index.html`, `/auth/login`, 304, HEAD; security headers on every
answer kind; traversal (plain and percent-encoded); fc-dev's `the_embedded_build_is_served_by_the_spa_rules`
against the real embedded build (Go `TestEmbeddedBuild`). **Failing first**: five of the eight
router tests failed against the old `serve_spa` (the shell and traversal ones pass on both).
**Mutants** (each killed): API-prefix branch dropped; missing asset serves the shell; shell
`no-store`; no ETag; non-GET served; no security headers; traversal climbs; 304 not honoured.

Route snapshot: `/assets` nest entries gone (one handler now), the shell's `Content-Type` gains
`; charset=utf-8`, the 405 is `Allow: GET, HEAD` with a `text/plain` body.

## 2a. Test helper (`a6847e65`)

Item 1 refused the older tests' cookie writes (`two_factor_test`, `auth_security_test`): they
replayed the cookie with no browser headers, the case Go's decision 1 refuses.
`TestApp::send_with_session` now sends `Sec-Fetch-Site: same-origin`, as the SPA's browser does.
These four `two_factor_test` and one `auth_security_test` failures were not caught before
`1a7ab929` was committed (only the new module ran); they passed from this commit on, and the whole
suite ran at the end.

## 3. Login CSRF (`13f099b3`)

`SessionCsrf::check_sign_in` (Go `CredentialCSRF`), applied by the same middleware to every unsafe
request to a route decided `SignIn`, authenticated or not: a request a browser marks as from another
origin (the provenance check of item 1) is **403 `CROSS_ORIGIN_REQUEST`** ("Sign-in and credential
requests must be sent by the platform's own pages."), logged; a request with **neither** header
passes (a non-browser caller; login CSRF needs the victim's browser); a body that is not JSON is
**415 `UNSUPPORTED_MEDIA_TYPE`** for every caller ("This endpoint accepts only a JSON body
(Content-Type: application/json)."). On a route both rules cover (the passkey sign-in and portal
routes, which are not outside the cookie group), the cookie rule runs first, as Go's
`SessionCSRF` before `forPaths(credentialCSRF)`.

**Every public unsafe route and its decision** — `session_csrf::PUBLIC_UNSAFE_ROUTES` is the single
source: the middleware's `SignIn` set is read from it, and
`route_table_snapshot_test::every_public_unsafe_route_is_decided` (Go
`TestEveryPublicUnsafeRouteIsDecided`) fails when the pinned route table holds a POST, PUT, PATCH or
DELETE outside `/api` and `/bff` (or under `/api/dispatch/`) with no decision, or the list names a
route the table does not hold. A new route reaches the snapshot through `the_route_table_is_unchanged`
(it fails until the snapshot is regenerated), then fails the decision test until decided.

| Route | Decision |
|---|---|
| `POST /auth/login` | JSON only + provenance (mints `fc_session`) |
| `POST /auth/2fa/verify`, `/auth/2fa/enroll/{totp,email}/confirm` | JSON only + provenance (finish a login, mint `fc_session`) |
| `POST /auth/2fa/challenge/email`, `/auth/2fa/enroll/{totp,email}/begin` | JSON only + provenance (steps of the same flow; mail, factor secrets) |
| `POST /auth/webauthn/authenticate/{begin,complete}` | JSON only + provenance (passkey sign-in mints `fc_session`) |
| `POST /auth/password-reset/confirm` | JSON only + provenance (sets a password; an INVITE confirm mints `fc_session`) |
| `POST /auth/password-reset/request`, `/auth/password-setup/request` | JSON only + provenance (mint tokens, send mail) |
| `POST /portal/auth/login`, `/portal/auth/password-reset`, `/portal/auth/check-domain` | JSON only + provenance (portal password login answers a code redirect in JSON, no cookie; check-domain guarded with its flow, as Go's one registration) |
| `GET /auth/oidc/login`, `GET /portal/auth/oidc/login`, `GET /auth/oidc/callback` | browser binding cookie (item 4) |
| `POST /auth/check-domain` | left: read-only |
| `POST /auth/logout` | left: issues nothing; `GET /auth/oidc/session/end` clears the cookie by design |
| `POST /auth/refresh` | left: refresh token in the body, tokens in the JSON body, no cookie |
| `POST /auth/oidc/interaction/{uid}/login` | left (Rust only): nothing creates an interaction state (`interaction_uid` is never set), typed `Json` body, sets no session; it now sets the binding cookie for the URL it returns, so the callback would accept the browser that follows it |
| `POST /oauth/token`, `/oauth/introspect`, `/oauth/revoke` | left: form-encoded by the spec, client-authenticated; no password grant; no cookie |
| `POST /oauth/userinfo` | left: bearer access token |
| authenticated `/auth/*` writes (`change-password`, `change-password/send-email-code`, `client/switch`, `2fa/methods/*`, `2fa/recovery-codes/regenerate`, `2fa/trusted-devices/{id}`, `webauthn/register/*`, `webauthn/credentials/{id}`) | cookie rule (item 1); bearer exempt |
| `POST /control/functions/{events,heartbeat}` | left: function runner bearer |
| `POST /api/dispatch/{process,settled}` | left: HMAC job token |

**Tests** (`login_csrf_test`, Go `logincsrf_pg_test.go`; unit `the_sign_in_rule_matrix`, Go
`TestCredentialCSRF`; `the_sign_in_routes_are_the_routes_decided_sign_in`): on all 15 guarded
routes, cross-site `text/plain`, sibling JSON, foreign Origin only and `Origin: null` are 403;
same-origin `text/plain`, a non-browser form body, multipart and a `Content-Type`-less body are 415;
same-origin, `none`, Origin-matches-Host and no-header JSON reach the handler; check-domain, logout
and refresh are not guarded; a real password login is refused cross-site, from a sibling and as
same-origin `text/plain` with **no cookie set**, and signs in for the SPA, curl, an old browser on
the host and a proxy that rewrites `Host` (public base URL as Origin). `session_csrf_test` now pins
that a cookie-carrying no-header JSON request to a sign-in route is not refused (the cookie rule's
neither-header refusal does not apply there).

**Failing first**: `the_sign_in_routes_refuse_cross_origin_and_non_json_requests` (401 "Invalid
credentials", the body decoded) and `a_cross_site_form_cannot_sign_a_browser_in` (200) failed with
the table in place and the middleware not yet applying it. **Mutants**: see "Follow-up mutants".

## 4. SSO callback binding (`353b8618`)

`fc_platform_auth::auth::login_binding` (Go `login_binding.go`). The starts — `GET /auth/oidc/login`
(`oidc_login`), `GET /portal/auth/oidc/login` (`portal::oidc::start`) and the inert interaction login
— set `fc_sso_<16 hex of sha256(state)>` (`__Host-` prefixed and `Secure` when the session cookie is
`Secure`, i.e. fc-server), value the state, `HttpOnly`, `SameSite=Lax`, `Path=/`, `Max-Age=600`.
The callback, after consuming the state (`find_and_consume_state`; the portal interceptor's
`consume_portal`), requires the cookie with the state as its value (constant-time); otherwise **400
`{"error":"LOGIN_NOT_STARTED_HERE","message":"This sign-in was not started in this browser. Start
the sign-in again."}`**, the state spent, no login-attempt row, a warn log. Every callback carrying
a state clears the cookie (`oidc_callback` wraps the old body as `complete_callback` and appends the
expiring `Set-Cookie` whatever the outcome; the portal interceptor the same).

**Tests**: `sso_binding_test::an_sso_callback_completes_only_in_the_browser_that_started_it` (a test
IdP in-process: discovery, JWKS and a token endpoint signing an RS256 ID token for the login's
nonce): a callback without the cookie is 400 and signs nothing in, and the starting browser cannot
then finish it (spent); the start's cookie attributes; another login's cookie and a forged value
refused; the full round trip signs in and clears the binding. `portal_identity_test::sso_owned_domains_route_to_their_idp`
gains: the portal start sets the binding, and a portal callback without it is 400
`LOGIN_NOT_STARTED_HERE` with its state spent. Unit (`login_binding::tests`): the name (per login,
`__Host-` over HTTPS), the attributes, only the login's own cookie and value bind, over HTTPS only
the `__Host-` cookie counts. **Failing first**: the SSO test failed at its first assertion (the
browser that never started the login was signed in); the portal additions failed (no binding
cookie).

## Follow-up mutants

Each killed by at least one test (`login_csrf_test`, `sso_binding_test`,
`portal_identity_test::sso_owned_domains_route_to_their_idp`, `session_csrf_test`, the unit tests,
or `every_public_unsafe_route_is_decided`): the sign-in rule not wired; the sign-in rule refusing a
request with neither header; its JSON rule dropped; its provenance check dropped; passkey
`authenticate/complete` decided `Left`; `/auth/logout` missing from the decision table; the
callback not checking the binding; a presence-only binding (any value); the binding not cleared;
no cookie at the platform start; no cookie at the portal start; the portal callback not checking;
no `__Host-` prefix; a 20-minute lifetime; `SameSite=Strict`. Checking before the consume was not
mutated (the spent-state assertions pin the order's effect).

## CORS follow-up (Rust only, `f1029b1f`)

Go has no CORS layer (its SDKs are server-side bearer clients), so there is nothing to mirror; this
closes what the "CORS" divergence below left open.

**Observed before** (`cors_layer_test`, `cors::layer::tests`, the layer as fc-server built it):
`Access-Control-Allow-Credentials: true` on **every** answer, the preflights included, for listed
and unlisted origins alike (tower-http emits it unconditionally); the allowed origin reflected. So a
page on an allow-listed origin could `fetch(…, { credentials: "include" })` the API with the
user's `fc_session` cookie (Lax: same-site siblings; `FC_SESSION_COOKIE_SAME_SITE=None`: any site)
and **read the answer**: the platform answers such a cookie `GET /api/clients` 200 (pinned by
`cors_layer_test`; reads are not refused by item 1) and the layer reflected the origin with
credentials allowed. Item 1 refuses the cookie writes; the reads remained. Unlisted origins got no `Access-Control-Allow-Origin` (no reflection), never `*`;
`Vary: origin, access-control-request-method, access-control-request-headers` was tower-http's
default.

**What relied on credentialed CORS: nothing found.**

- The SPA calls relative URLs on its own origin (`frontend/src/api/client.ts`, `API_BASE_URL =
  "/api"`; `credentials: "include"` there is same-origin). Development serves it through Vite's
  proxy (`frontend/vite.config.ts`: `/api`, `/bff`, `/auth`, `/oauth`, … to the backend), so the
  browser sees one origin and no CORS is involved.
- fc-dev has its own layer (`bin/fc-dev/src/main.rs`): `allow_origin(Any)`, any method and header,
  **no credentials** (`Access-Control-Allow-Origin: *`); unchanged.
- The SDKs (`clients/typescript-sdk`, Laravel, Java, Go) authenticate with bearer tokens (client
  credentials or a user's token), never the cookie; the router dashboard proxies its token exchange
  rather than call the platform cross-origin (`fc-router` `dashboard_sign_in`). A browser OAuth
  public client calling `/oauth/token` or a bearer API cross-origin needs CORS but not credentials.
- No test or doc depended on `Allow-Credentials`. `docs/owner-questions.md` CORS-3 ("`Allow-Credentials`
  emitted?") is open; this answers its credentials half (no) on security grounds. The owner may
  still rule on exact-versus-wildcard matching.

So no dev-mode switch for credentialed CORS was added: there is no legitimate use to keep.

**The change**: `fc_platform::cors::layer` (`crates/fc-platform/src/cors/layer.rs`) now holds the
layer fc-server applies (`cors_layer(AllowedOrigins)`, after `AuthLayer` and `TraceLayer`, as
before) and the allow-list (`AllowedOrigins`: exact entries in a set, `*` entries compiled once per
refresh instead of per request; replaced whole on the 60 s refresh, as before). Rules:

1. **Never `Access-Control-Allow-Credentials`** (`allow_credentials(false)`, explicit), on any
   answer. A browser therefore fails a credentialed preflight and does not expose a response to a
   request that carried the cookie.
2. A listed origin (exact, or a `*` pattern where `*` is one DNS label, anchored) is reflected in
   `Access-Control-Allow-Origin`; any other origin, `null` included, gets no CORS headers. Never `*`.
3. Every answer: `Vary: Origin, Access-Control-Request-Method, Access-Control-Request-Headers`
   (explicit, no longer tower-http's default).
4. Methods, allowed request headers (`Authorization`, `Content-Type`, `Accept`, `Origin`,
   `X-Requested-With`, `X-Client-Id`) and the 24 h preflight cache unchanged.

fc-server's `regex` dependency moved to fc-platform with the code.

**Tests**: `cors::layer::tests` (7, a router behind the layer: a listed origin reflected without
credentials; its preflight allows `Authorization` and `DELETE` without credentials; unlisted
origins — a sibling, a suffix-extended host, another scheme, `null` — get no CORS headers on a GET
or a preflight; no Origin, no CORS headers but `Vary`; a `*` table entry never yields
`Allow-Origin: *`; wildcard = one label, whole-origin match; exact entries exact, replace drops old
ones) and `cors_layer_test::an_allow_listed_origin_reads_with_a_bearer_but_never_with_the_cookie`
(the whole platform router under `AuthLayer` and the layer, the origin added through
`POST /api/platform/cors` and loaded as fc-server loads it: the bearer preflight and the bearer
`GET /api/clients` from the listed origin are allowed and readable, without credentials; the same
GET with the cookie answers 200 with no `Allow-Credentials`; a credentialed DELETE preflight gets
none; an unlisted origin gets no CORS headers with either credential; deleting the origin and
refreshing takes it off the list).

**Failing first**: with the layer extracted unchanged (`allow_credentials(true)`), four of the seven
unit tests (`Allow-Credentials: true`, on the unlisted origins too) and the `it` test (at the
preflight) failed; the three matching tests passed on both. **Mutants** (each killed):
credentials `true`; any origin passes the predicate; `Allow-Origin: *` (`Any`); no `Vary`; the
pattern not anchored; `*` spanning labels (`.+`); an exact entry matched as a prefix; replace
keeping old entries; a refresh that ignores an empty table (killed by the `it` test). Not covered
by a test: fc-server wiring the layer at all (`build_platform_app` is in the binary and needs the
whole server; read, and clippy-clean).

**Client-visible**: a page on an allow-listed origin can no longer use the user's platform session
from the browser (credentialed requests: preflight refused, responses unreadable); bearer-token
calls from it are unaffected. No OpenAPI change, no route change, no SQL change.

**Verified**: `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D
warnings` clean; lib tests fc-platform 99, fc-platform-auth 131, fc-platform-core 134,
fc-platform-iam 452; fc-server binary tests 10; `route_table_snapshot_test` both pass (unchanged: the layer is outside `router::build`);
the whole `it` suite with `--include-ignored` on the `FC_TEST_PG_BIN` harness (embedded PostgreSQL
18.3): **511 passed, 0 failed** (510 before, plus `cors_layer_test`). No dev database, nothing on 8080.

**Not verified**: no browser run (that a real browser refuses the credentialed preflight and hides
the cookie response is the CORS specification, not observed); fc-server not started (the wiring is
read and compiled, not exercised end to end); no deployed environment or proxy; the
allowed-origin list's refresh loop not observed running; fc-dev's `*` layer not re-examined beyond
reading it (it never sent credentials).

## Divergences from Go

- **Where the rules live**: Rust has no authenticated route group (extractors authenticate lazily),
  so one middleware covers the whole platform router and exempts by path the routes Go mounts
  outside its group (`outside_the_authenticated_group`), and takes the sign-in routes from the
  decision table rather than wrapping each registration. Consequences: an **unknown** path is also
  guarded (a cookie-authenticated cross-origin POST to it gets 403 where Go answers its 404/405, a
  refusal either way); refused sign-in requests are refused before the routes' per-IP limiters, so
  they do not spend them.
- **Callback error style**: Rust's OIDC callback answers its other failures with a redirect to
  `/?error=…`; the new refusal is Go's 400 JSON, as Go answers it.
- **Range requests** on SPA files are not supported (Go's `http.ServeContent` honours them); the
  SPA does not use them.
- **`Content-Type` of served files** comes from `mime_guess` (Go: `mime.TypeByExtension`);
  `charset=utf-8` is added to text types and JavaScript as Go's table does.
- **No SPA, no fallback**: fc-server without a `static_dir` still answers unknown paths with axum's
  empty 404 (Go's fc-server always mounts the frontend handler, "frontend not built" when empty), so
  an unknown API path is a JSON `NOT_FOUND` only where the SPA is served.
- **Debug gate** (Go `780e406`): not mirrored; no cookie- or Basic-authenticated debug surface
  exists here.
- **`$schema`**: Go's huma responses carry `$schema` (left by Go's ruling); Rust's never did.
  Unchanged.
- **CORS**: fc-server has a CORS layer for the origins in the CORS table (Go has no CORS
  middleware). It used to send `Access-Control-Allow-Credentials: true`: before item 1 an
  allow-listed origin's page could make credentialed preflighted requests (JSON, DELETE) with the
  user's cookie wherever SameSite let it through, and after item 1 it could still read
  cookie-authenticated responses. The CORS follow-up below drops credentials.

## Client-visible changes

- 403 `CROSS_ORIGIN_REQUEST` / 415 `UNSUPPORTED_MEDIA_TYPE` on cookie-authenticated writes that do
  not come from the platform's origin or carry a non-JSON body; a script replaying a browser's
  cookie must send `Origin: <platform URL>` (or `Sec-Fetch-Site`) or use a bearer token; a
  cookie-authenticated function artifact upload (`application/wasm`) is now 415 (bearer uploads
  unaffected).
- The sign-in routes: a non-JSON body is 415 for every caller; a browser request from another
  origin 403; a non-browser JSON caller unaffected.
- SSO must start and finish in the same browser, on the callback's host; a login in flight across
  the deploy fails once (no binding cookie) and must be restarted; every SSO start sets one
  short-lived cookie.
- SPA: unknown API paths 404 JSON (were 200 `index.html`), missing assets 404 `no-store` (were the
  shell in fc-dev's embedded build, or a cacheable 404), the shell `no-cache` with an ETag (was
  `no-store`), security headers on every SPA answer.
- No OpenAPI change: the middleware, the fallback and the callback's new 400 are not in the
  published document (the route snapshot's document hashes are unchanged); the vendored Go lock is
  untouched (Go did not regenerate it either).

## Verified

All database tests ran on the `FC_TEST_PG_BIN` harness (embedded PostgreSQL 18.3, a private
cluster); no dev database and nothing on 8080 was touched.

- Failing first and mutants: as recorded per item (34 mutants in all, every one killed).
- `cargo fmt --all -- --check`: clean. `cargo clippy --workspace --all-targets -- -D warnings`:
  clean; `cargo clippy -p fc-dev --features web -- -D warnings`: clean.
- Lib tests: fc-platform 92, fc-platform-auth 131, fc-platform-core 134, fc-platform-iam 452;
  fc-dev's binary tests 57 (the real embedded build served by the SPA rules); all passed.
- `route_table_snapshot_test`: both tests pass at `353b8618` (the snapshot regenerated in
  `b7430c13`: the SPA lines only; the document hashes unchanged).
- The whole `it` suite with `--include-ignored`, at `353b8618`'s tree: **510 passed, 0 failed**.
- No SQL changed (`sqlx-prepare --check` not needed); no OpenAPI change.
- Intermediate commits were not each built on their own: `1a7ab929` was committed before the full
  suite ran, which is how item 2a's fallout surfaced; `13f099b3` lists the new test module out of
  alphabetical order in `tests/it/main.rs` (rustfmt reorders it in `353b8618`).
- Builds ran with `CARGO_INCREMENTAL=0`, outside the command sandbox; none stalled.

## Not verified

- No browser run: in particular not that a cross-site `text/plain` form post's response sets a
  Lax cookie (shown at the handler: 200 with `Set-Cookie`), that browsers send the `__Host-` SSO
  cookie on the IdP's redirect back, or SSO against a real IdP. The SPA was not exercised under the
  new headers (the Swagger UI and theme pages included).
- No deployed environment behind a proxy; Vite's dev proxy not tried.
- fc-dev with `--features web` was compiled and linted, not run.
- The `warn` log lines were not observed.
- The router's own dev-mode legacy auth (`AUTH_MODE=BASIC`/`OIDC_FLOW`, dev mode only; its
  `oidc_flow` session cookie also named `fc_session`) and its monitoring mutations were not
  examined for CSRF: outside the platform router, and production resolves to the platform bearer.
