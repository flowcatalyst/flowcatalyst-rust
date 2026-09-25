# Topcoat trial: a server-rendered Rust admin UI

**Status:** trial, 2026-09-25, on `main` behind a feature. Nothing here ships
in `fc-server`/`fc-platform-server` or the Docker image; the UI only mounts in
`fc-dev` built with `--features web`. `crates/fc-web` is excluded from the
Cargo workspace, so `cargo build`/`test`/`clippy --workspace` (and CI) never
compile it. To drop the trial, see [Removing the trial](#removing-the-trial).

**Question:** can [Topcoat](https://github.com/tokio-rs/topcoat) (tokio-rs,
v0.9: server-rendered components, a small browser runtime, Tailwind via the
standalone CLI) replace the Vue 3 + PrimeVue frontend, so the platform needs
no Node/npm toolchain?

## What was built

| Piece | Where |
|---|---|
| UI crate | `crates/fc-web` (edition 2024) |
| Pages | `/ui/login`; list + drawers for event types, subscriptions, connections, dispatch pools, clients, applications, roles (`/ui/authorization/roles`), events, dispatch jobs; the audit log (`/ui/platform/audit-log`). Each section's URL is the SPA's route under `/ui` |
| Theme | `crates/fc-web/styles.css`: the SPA's look (PrimeVue Nora tokens, the FlowCatalyst density preset, navy chrome) as Tailwind tokens + `.fc-*` component classes |
| FlowCatalyst components | `crates/fc-web/src/ui.rs` and `src/ui/`: `page_header`, `table_toolbar` (FcTableToolbar), `paginator`, `drawer_frame` / `drawer_header` (EntityDrawer), `form_field` / `detail_field` / `detail_value` (FcFormField, FcDetailField), `filter_select`, `tag`, `code_chips`, `confirm_dialog`, `json_block`, `local_time`, `empty_state`, flash messages |
| App shell | `crates/fc-web/src/app/shell.rs` + `nav.rs`: navy sidebar (themed logo, the SPA's `navigation.ts` gated by its own `canAccessPath` rule on the server, collapse), SidebarProfile menu, layout |
| Auth convention test | `crates/fc-web/tests/auth_convention_test.rs` |
| Asset bundle | written by the process itself on the first start after a build: `crates/fc-web/src/assets.rs` |

How it fits together:

```
axum Router (fc-dev --features web)
 ├─ /api, /bff, /auth, /oauth, …   existing handlers, unchanged
 └─ fallback_service → fc_web::service(deps, spa)   (Topcoat TowerService)
      ├─ /ui/login, /ui/logout              public
      ├─ /ui/(app)/…  pages, routes, shards  ← #[layer] authenticates
      ├─ /_topcoat/…  assets + runtime
      └─ "/" and "/{*rest}" → the existing Vue SPA service
```

- **Reads** go straight to the repositories, just as the BFF handlers do.
  There is no JSON layer in between and no BFF endpoint per screen.
- **Writes** are plain HTML form POSTs to `#[route]` handlers. Each one runs
  the same use case the BFF handler runs (`UpdateEventTypeUseCase`, …)
  through `UnitOfWork`, so the domain events and audit rows are identical.
  After the write, the handler redirects back with a flash message
  (Post/Redirect/Get).
- **Authentication** is a `#[layer("/ui/(app)")]` that calls
  `fc_platform::shared::middleware::authenticate_headers`, the same code as
  the `Authenticated` extractor.
- **Authorization** is `permit(checks::…)` inside every handler.
- **Shards** get explicit paths under `/ui/(app)`. Their default
  `/_topcoat/runtime/<hash>` path would sit outside the layer, and a shard
  endpoint runs without its page's checks. The convention test enforces
  both rules, and a mutation check confirmed it catches each violation.
- **CSRF** is handled by Topcoat's origin policy (`Sec-Fetch-Site`/`Origin`)
  on every non-GET request.

### Fit and finish: matching the SPA

The reference is the SPA in `frontend/`, which is now Go's production UI:
list pages whose rows open a non-modal right-hand drawer (`EntityDrawer`),
`FcTableToolbar` (quick search, a Filters popover with a count badge, Clear
All), `Fc*` form components, `SidebarProfile`. fc-web mirrors each piece:

- **List + drawer pattern** (`app/event_types.rs` is the worked example):
  every section URL renders the list; `/{id}` also opens the drawer (so
  writes redirect back to it and it is linkable), `?edit=true` opens it
  editing, `/new` (or `/create` for event types, as the SPA) opens the
  create drawer. The drawer body is a shard keyed by a `selected` signal,
  so clicking another row swaps it in place and the list keeps its scroll.
- **Read view and edit form** are both rendered; an `editing` signal
  toggles them in the browser. Save stays disabled and Discard hidden
  until the form is dirty (`ui.js`, `data-dirty-form`), as `useDirtyForm`.
- **Toolbar and paging:** the toolbar is the list's GET form; filters live
  in a native `popover`; the paginator is links plus a rows-per-page
  select ("Showing x to y of n …"). Events and dispatch jobs take a result
  `size` only, no paging (owner rule).
- **Navigation:** `nav.rs` is `navigation.ts` with the SPA's own access
  rule (`ROUTE_PERMISSIONS`, `ANCHOR_ROUTES`, audience scope, roleless
  users) evaluated on the server; ported routes open at `/ui<route>`, the
  rest open the SPA.

The first pass used Topcoat UI's vendored components and its neutral theme.
It worked, but it looked like a different product. The second pass replaced
that with a theme copied from the Vue app's actual values, so the two UIs sit
side by side without a seam:

- **Tokens and control styles:**
  - PrimeVue Nora: emerald primary, 2px form and tag radii, solid bold tags,
    slate borders.
  - The FlowCatalyst density overrides from `main.ts`.
  - The chrome from `main.css`, `AppSidebar.vue` and `LoginPage.vue`.
- **Sidebar:** the theme's logo (`logoUrl` / `logoSvg`, else the default
  bolt), navigation filtered by permission on the server, and a user card
  that opens the profile / reset password / sign out menu.
- **Event types:** the list plus a right-hand detail drawer (URL
  `/ui/event-types/{id}`, or opened in place from a row), with coloured code
  segments.
- **Login:** the themed login with the email step, then the password step
  (with a show/hide toggle) or SSO, and passkeys.
- **Timestamps:** localised in the browser, like the Vue app's
  `toLocaleString()`.

Topcoat UI itself was dropped. Its components are shadcn-style, and
restyling them to Nora would have meant rewriting them anyway. Our kit is
about 300 lines of components plus about 800 lines of CSS.

Interactivity now comes mostly from the platform, not the Topcoat runtime:

- Native `<dialog>` opened with invoker commands
  (`commandfor`/`command="show-modal"`), so modals get a focus trap, Escape
  and a backdrop with no script.
- `popover` for the user menu.
- `<details>` for nav sub-menus.

The runtime is used where it pays off:

- The audit detail and event-type drawer are shards, so opening a row keeps
  the list's scroll position.
- The show-password toggle is a signal.

### Changes to `fc-platform` (behaviour-preserving)

These lift logic out of the axum handlers so fc-web can call it instead of
copying it:

- `shared::middleware::authenticate_headers` (the extractors now call it).
- `auth::auth_api::password_login` (the body of `POST /auth/login`).
- `auth::oidc_login_api::resolve_auth_method` (the body of `POST /auth/check-domain`).
- `shared::public_api::load_login_theme`.
- `event_type::access::{ensure_visible, ensure_modifiable}`. These are the
  client/anchor rules the BFF event-type handlers had inlined 7 times; the
  BFF handlers now call them.
- `PlatformError::status_code()` and `SessionCookieConfig::password_login()`.
- `audit::api::enrich_principal_names` and
  `audit::api::enrich_single_principal_name` are now `pub`.
- `event_type::access::ensure_can_create` and
  `bff_event_types_api::platform_sync_command`.
- `subscription::access`, `connection::access`, `client::access`: the
  client/anchor rules the API handlers had inlined (the handlers call them).
- `dispatch_pool::access` (the handlers keep their inline copies: switching
  them over tripped `permission_convention_test`, which counts the inline
  `is_anchor()` as their only check) and `checks::can_write_dispatch_pools`
  (Go's `CanWriteDispatchPools`, used by fc-web only).
- `application::api`: the delete / deactivate cascades and the service
  account / login client provisioning bodies.
- `shared::caller_reach::{read_client_filter, ensure_row_visible}` (the
  events and dispatch-job read handlers) and
  `DispatchJobRepository::find_attempts` (a new read; see below).

## Running it

```sh
cargo build -p fc-dev --features web
target/debug/fc-dev          # then open http://localhost:8080/ui
```

- **Assets bundle themselves.** On the first start after a build, fc-web
  scans its own executable for Topcoat's `asset!` declarations and writes
  the bundle to `target/debug/fc-web-assets/<key>` (the key derives from the
  executable's path, size and mtime; older bundles are removed). That takes
  well under a second, and a rebuild can never serve a stale stylesheet.
  `topcoat asset bundle` can't be used: it builds the binary itself and has
  no `--features` flag.
- The bundle copies files from where the build left them (the Tailwind
  output under `target/`, the Topcoat runtime in `~/.cargo/registry`), so a
  `web` binary works on the machine that built it, which is the only way
  fc-dev is meant to use it. `FC_WEB_ASSETS_DIR` points at a prebuilt bundle
  instead.
- **Tests:** fc-web is outside the workspace, so run its tests against its
  own manifest (sharing the workspace's target dir and, if you like, its
  lockfile, `cp Cargo.lock crates/fc-web/`; both are git-ignored):

  ```sh
  CARGO_TARGET_DIR=target cargo test --manifest-path crates/fc-web/Cargo.toml
  ```
- Offline builds need two things:
  - `TAILWIND_CLI=/path/to/tailwindcss`. Otherwise the build downloads Tailwind
    v4.3.2 into `target/topcoat/cache`.
  - A warm cache for the Lucide icon set, which `build.rs` downloads the same way.

## Scorecard

All measurements are from a debug build on an M-series Mac, against a local
Postgres.

| | Topcoat (`fc-web`) | Vue (`frontend/`) |
|---|---|---|
| **Toolchain** | cargo + a pinned Tailwind binary (downloaded by build.rs) | Node ≥ 24, pnpm, 413 packages |
| **Audit log page (lines)** | 358 (`audit_log.rs`, list + detail dialog) | 604 (`AuditLogListPage.vue`) + `useListState` 352 + `useCursorPagination` 130 |
| **Event types list + drawer + writes (lines)** | 661 (`event_types.rs`) | 1,007 (`EventTypeListPage.vue` + `EventTypeDetailPage.vue`), plus the BFF endpoints they need |
| **Login (lines)** | 283 + 100 JS (passkey) | 667 (`LoginPage.vue`) + 111 theme store + 170 WebAuthn client |
| **Shell + UI kit (lines)** | 424 shell + 311 components + 820 CSS | `MainLayout`/`AppSidebar`/`UserMenu` ~900 + `main.css` 151 + PrimeVue (a dependency) |
| **First load, shared assets (gzip)** | CSS 6.7 KB + runtime JS 10.8 KB + `ui.js` 0.5 KB, no web fonts | Entry bundle 117 KB (index JS + CSS), before route chunks |
| **Per page (gzip)** | Audit log 6.2 KB, event types with drawer 9.7 KB (75 rows) | Route chunk 4–9 KB JS + JSON API calls |
| **Server time to first byte (debug build)** | 4–14 ms per page, data included | n/a (the SPA shell is static; data comes from later API calls) |
| **Edit one page → new fc-dev binary** | ~4 s (incremental) | Vite HMR, under 1 s |
| **Cold build of fc-web + Topcoat** | ~4.5 min (first time, with downloads) | `pnpm install` + `vite build` |

## What worked well

- **Plain HTML forms plus Post/Redirect/Get cover almost all CRUD.** With
  native `<dialog>`, `popover` and `<details>`, the browser handles most of
  the interactivity itself. The Topcoat runtime is needed only for in-place
  shards and the show-password toggle.
- **Matching the existing look is ordinary CSS work.** The PrimeVue Nora
  values copy straight into CSS custom properties and component classes.
- **The URL holds the filter state.** A GET form re-renders on the server, so
  lists are bookmarkable and survive a refresh. The Vue app's
  `useListState` (352 lines) and cursor stack (lost on refresh) have no
  counterpart because nothing is needed.
- **No per-screen API.** Pages read repositories directly and write through
  use cases. Nothing had to be added to `/bff`.
- **Security lines up with the backend.** Authentication and permission checks
  are the platform's own functions. The origin policy covers CSRF. The
  navigation is filtered on the server by permission, which the Vue sidebar
  doesn't do.
- **The auth convention test is cheap** and closes the "shards bypass page
  guards" hole structurally.

## What hurt

1. **The runtime's expression vocabulary is small.**
   - An expression can read an `Option` but can't construct `Some(x)` or
     `None`. The selected-row signal had to become a `String` where empty
     means "none".
   - `String::new()` isn't allowed either; `"".to_owned()` is.
   - Your own structs and enums can't cross into the browser.
2. **`view!` type errors take a while to read.**
   - Every `view!` is its own anonymous type, so a page can't return
     different views from different branches. The fix is to compute state
     first and render once.
   - Borrowing a value in one component call and moving it in another fails
     because views are lazy.
   - Component props can't be borrowed slices; they have to be owned.
   - Each of these cost a compile cycle to understand.
3. **Asset bundling in an embedded setup.**
   - The bundler scans the final binary, and `topcoat asset bundle` can't
     pass cargo features. The first pass needed a separate `fc-web-bundle`
     tool and a manual re-bundle after every class change; fc-web now
     bundles itself at startup (`src/assets.rs`).
   - `topcoat-asset` as a direct dependency switches Topcoat's asset macros
     to `::topcoat_asset` paths. As an *optional* dependency that broke the
     font macros (the path was emitted while the crate wasn't linked); as a
     normal dependency it is fine.
   - The bundle is files on disk, copied from the build machine's paths, so
     a `web` build is not a distributable single binary. Shipping it would
     need the bundle embedded (or Topcoat's hosted-manifest mode).
4. **The dev loop is slower than Vite.**
   - It takes about 4 s to rebuild, then an fc-dev restart (the bundle is
     rewritten on that start).
   - `topcoat dev` would automate this but can't pass features either.
     fc-web does call `notify_ready`, so it works once that is solved.
5. **Build-script pitfall.** Printing `cargo:rerun-if-env-changed` (for
   `TAILWIND_CLI`) turns off Cargo's default change detection. The Tailwind
   stylesheet then silently stopped updating when classes changed. The fix is
   to name the inputs explicitly (`styles.css`, `src`). Nothing warns you.
6. **Topcoat UI doesn't match the product.** Its shadcn look needed
   replacing, not theming. Budget for a house component kit from day one.
7. **Small API gotchas.**
   - A `view!` outside a component needs the context passed explicitly.
   - Icon sizes take a `Length`, not a string.
   - `IconData` isn't `Copy`.
   - The runtime renders a bound attribute's initial value too, so a static
     attribute next to it gets duplicated.
   - Each one is a compile or render cycle to discover.
8. **Maturity.** v0.9, released 2026-09-24. The README says to expect
   breaking changes, and the runtime docs call it "highly experimental".

## Verification

- **Tests:** `cargo test -p fc-platform` (including the route-auth,
  permission and UoW convention tests) and fc-web's auth convention test.
- **Screenshots** of every section (list, drawer, edit mode, create
  drawer, filters) from headless Chrome against a local fc-dev, compared
  with the Vue templates.
- **curl against a local fc-dev**, per section: every write succeeds and
  leaves its `aud_logs` row; refusals re-render with the platform's
  message; anonymous pages redirect to `/ui/login?next=…`, anonymous shard
  and form POSTs get 401, cross-site POSTs get 403.
- **Not verified by a person:** password entry and the show-password
  toggle, passkeys, and a click-through of every drawer in a real browser
  (automation drove the clicks). Integration tests driving
  `fc_web::service` are still not written.

## Differences from the SPA

- Multi-selects are single selects (applications, facets). Searchable
  pickers are plain selects; chip inputs are one-per-line text areas.
- Buttons the caller can't use are hidden or disabled; the SPA shows them
  and lets the server refuse.
- No column sorting.
- Where the Rust API can't do what the SPA sends, fc-web leaves the field
  out rather than pretend (listed under "Existing issues").
- Subscriptions: fc-web lists paused subscriptions (the API doesn't, so
  the SPA's Paused filter is empty against Rust) and its drawer saves the
  dispatch mode through the use case (the API handler drops it).
- Dispatch pools: fc-web lists every status and requires
  `can_write_dispatch_pools` for writes, as Go (see below).

## Existing issues found along the way (not fixed here)

Backend (main):

- **Dispatch-pool writes have no permission check.** Create, update,
  archive, suspend and activate in `dispatch_pool/api.rs` check only anchor
  scope or client reach, and their use cases' `authorize` is empty. Go
  requires `CanWriteDispatchPools`. `permission_convention_test` passes
  because it counts the inline `is_anchor()`.
- `GET /bff/roles` and `/bff/roles/{name}` need only a login (no
  `can_read_roles`); the BFF role list ignores `source` when an
  application is also given.
- `GET /api/subscriptions` never returns paused subscriptions. The
  subscription update handler drops the dispatch mode; queue, max age,
  delay, sequence and client-scoped are ignored on create and update;
  bindings carry no spec version.
- `GET /api/dispatch-pools` without a client returns only ACTIVE pools.
- Connection and dispatch-pool creates never store `client_identifier`.
- Nothing reads dispatch-job attempts back: `find_by_id` leaves `attempts`
  empty, so `GET /api/dispatch-jobs/{id}/attempts` always answers `[]`
  (fc-web uses the new `find_attempts`; the handler should too).
- Missing routes the SPA calls: dispatch-job requeue and sign, docs sync.
- The dispatch-job read model has no descriptor or client identifier; its
  list takes no message group, date range or sort.
- Application website / logo are dropped on create and update.
- The client list ignores its page parameter; client deactivate is refused
  while a *disabled* application config row still references the client.
- `/oauth/token` rejects client credentials sent with HTTP Basic auth
  (possible Go parity gap; not checked against Go).
- The SPA's audit log sends `applicationIds` / `clientIds`, which
  `AuditLogsQuery` ignores.
- The BFF add-schema handler ignores the version the user enters (Go uses
  it).
- The SPA's interaction login redirects with a GET to
  `/oidc/interaction/{uid}/login`, a route that doesn't exist (already on
  the cutover checklist).

Dropped from the first trial's list (no longer apply): the OIDC path now
forwards `interaction`; `switchClient` posts to `/auth/client/switch`;
archiving an event type with a FINALISING schema matches Go (its use case
has no such rule either).

## Recommendation

**Go, as a gradual migration, with two conditions.**

The fit is good. The admin UI is forms and tables, and server rendering with
form posts is less code than the Vue pages: roughly half the lines, with no
API layer per screen. It also sends a fraction of the JavaScript. Security
rests on the platform's own authorization functions.

The risk is Topcoat's youth, not the model:

1. **Pin Topcoat to an exact version and keep our own component kit**
   (already the case). Treat an upgrade as a planned task.
2. **Keep the browser runtime optional.** Pages should work as plain HTML
   first (forms, links, GET filters), with signals, shards and dialogs as
   enhancements. If the runtime breaks or stalls, the fallback is htmx or
   plain JS on the same server-rendered pages. Topcoat ships htmx and
   Alpine integrations. Nothing on the server side has to change.

**Next steps if we continue:**

- A person clicks through the password step, passkeys and each drawer.
- Embed the asset bundle if a `web` build ever has to be distributed.
- Port the remaining sections (users, service accounts, identity
  providers, email domains, OAuth clients, CORS, login attempts, scheduled
  jobs, processes, functions, settings), and multi-select filters.
- Retire the matching Vue routes and BFF endpoints one by one.

## Removing the trial

fc-web touches nothing outside these places, so dropping it is a small,
mechanical diff:

1. Delete `crates/fc-web`.
2. In `bin/fc-dev/Cargo.toml`, remove the `fc-web` optional dependency and
   the `web` feature.
3. In `bin/fc-dev/src/main.rs`, remove every `#[cfg(feature = "web")]` item
   (`grep -n 'feature = "web"'`): the `WebDeps` construction and its log
   line, the two `fallback_service(fc_web::service(…))` branches (keep the
   `#[cfg(not(feature = "web"))]` fallback, minus its attribute, in the
   embedded branch), and the `notify_dev_ready` spawn.
4. In the root `Cargo.toml`, remove `"crates/fc-web"` (and its comment) from
   `exclude`; in `.gitignore`, the two `crates/fc-web/` lines.
5. Remove the "fc-web (Topcoat UI trial)" section and the fc-web mention in
   "Frontend UI Conventions" from `CLAUDE.md`, and delete
   `docs/topcoat-trial.md` and `docs/topcoat-handover.md`.
6. `cargo build -p fc-dev` to refresh `Cargo.lock` (Topcoat's packages drop
   out of it).

The `fc-platform` extractions stay: `authenticate_headers`,
`password_login`, `resolve_auth_method`, `load_login_theme`,
`event_type::access` and `PlatformError::status_code` are ordinary platform
functions that the axum handlers call themselves.
