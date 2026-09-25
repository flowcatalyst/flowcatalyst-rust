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
| Pages | `/ui/login`, `/ui/audit-log`, `/ui/event-types`, `/ui/event-types/{id}` |
| Theme | `crates/fc-web/styles.css`: the Vue app's look (PrimeVue Nora tokens, FlowCatalyst navy chrome) as Tailwind tokens + `.fc-*` component classes |
| FlowCatalyst components | `crates/fc-web/src/ui.rs`: `page_header`, `filter_select`, `search_input`, `cursor_pager`, `tag`, `code_chips`, `confirm_dialog`, `json_block`, `local_time`, `empty_state`, flash messages |
| App shell | `crates/fc-web/src/app/shell.rs`: navy sidebar (themed logo, permission-filtered nav, collapse), user menu, layout |
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

### Fit and finish: matching the Vue app

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

## Not verified yet

- **Clicked through in Chrome:**
  - The event-type list.
  - Opening the drawer from a row (a shard), for both a current and an
    archived type.
  - The schema viewer (native modal).
  - The audit log, and its detail dialog (a shard).
  - The login email step, compared side by side with the Vue login.
- **Not clicked through:**
  - The password step and its show-password toggle. The markup is verified
    with curl; typing passwords has to be done by a person.
  - Passkeys.
  - The user-menu popover and sidebar collapse.
  - The confirm dialogs.
  - The Chrome automation session was unreliable, with repeated timeouts on
    the Vue pages too.
- **Verified with curl against a running fc-dev:**
  - Every page renders (200) with the session cookie.
  - An anonymous request redirects to `/ui/login?next=…`.
  - Anonymous shard and form POSTs are refused with 401.
  - A cross-site POST is refused with 403.
  - A `//evil.com` `next` value is rewritten to `/ui`.
  - Every trial write succeeds (update, finalise, deprecate, archive), and
    the BFF read-back confirms the changes.
  - Unmatched paths (`/`, `/dashboard`, `/event-types`) still get the Vue app.
- **Integration tests aren't written yet.** The plan called for testcontainers
  tests driving `fc_web::service`. The curl checks above cover the same
  scenarios by hand.

## Existing issues found along the way (not fixed here)

- **Audit log filters:**
  - The Vue audit log sends `applicationIds`/`clientIds` filters that the API
    ignores (`AuditLogsQuery` has no such fields).
- **Vue login:**
  - After an interaction login it redirects with a GET to
    `/oidc/interaction/{uid}/login`, and no such route exists.
  - The OIDC path drops `interaction`.
  - `switchClient` posts to `/auth/client/{id}`, but the route is
    `/auth/client/switch`.
- **Adding a schema:** the BFF ignores the version the user types in.
- **Archiving event types:**
  - The "archive only when every schema is deprecated" rule exists only in
    the Vue page's button state. `ArchiveEventTypeUseCase` archives an event
    type with a FINALISING schema, and so do the BFF, the API and fc-web
    (whose button follows the Vue rule).
  - Worth deciding whether the rule belongs in the use case.

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

- Click-through of the unverified pieces (above).
- Embed the asset bundle if a `web` build ever has to be distributed.
- Port the list pages with the most repetition first (events, dispatch jobs,
  subscriptions) to grow the component kit.
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
