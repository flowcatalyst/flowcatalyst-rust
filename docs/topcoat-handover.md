# Topcoat UI: handover

For the agent picking this up. Read `docs/topcoat-trial.md` first: it
explains what was built, why, how it works, the scorecard and the gotchas.
This file covers **what is left to do**.

## Where things stand (2026-09-25)

- **Branch:** `trial/topcoat`, pushed to origin. It is two commits on top of
  `main` at `f7bcb827`:
  - `f80d51ed` **platform:** behaviour-preserving extractions in
    `fc-platform` (header auth, password login, domain check, login theme,
    event-type access rules). All 1,088 fc-platform tests pass.
  - `466305be` **fc-web:** the Topcoat UI crate (`crates/fc-web`), the fc-dev
    `web` feature, the asset-bundle tool (`crates/fc-web/bundle`), docs, and a
    CLAUDE.md section.
- **Worktree:** `../flowcatalyst-rust-wt/topcoat`.
- **Ported pages:** `/ui/login`, `/ui/audit-log`, `/ui/event-types` (list + side
  panel at `/ui/event-types/{id}`). All other sidebar links open the Vue app.
- **Owner decisions:**
  - The UI must look like the existing Vue app.
  - Tailwind is fine in fc-web (the no-Tailwind rule is only for the PrimeVue app).
  - Merge it to `main`, but only behind `--features web`, and it must be easy
    to remove if the trial is dropped.
  - No pull request is needed.

### Running it locally

```sh
docker run -d --name fc-topcoat-trial-pg -p 127.0.0.1:55432:5432 \
  -e POSTGRES_USER=flowcatalyst -e POSTGRES_PASSWORD=flowcatalyst -e POSTGRES_DB=flowcatalyst postgres:17-alpine
FC_SKIP_FRONTEND_BUILD=1 cargo build -p fc-dev --features web
cargo run -p fc-web-bundle -- target/debug/fc-dev     # re-run after CSS/class changes
FC_DATABASE_URL=postgresql://flowcatalyst:flowcatalyst@localhost:55432/flowcatalyst \
  FC_EMBEDDED_DB=false target/debug/fc-dev --no-functions
# first time only: target/debug/fc-dev init --root /tmp/x --yes \
#   --admin-email admin@flowcatalyst.local --admin-password '…' --code orders --name Orders
```

- Open `http://localhost:8080/ui`.
- To see the login page without signing out, use `http://127.0.0.1:8080/ui/login`
  (a different cookie host).
- The container above may already be running on the owner's machine. Its
  admin user is `admin@flowcatalyst.local` / `Trial-Passw0rd!x`.

---

## Task A: land it on `main` behind `--features web`, easy to remove

**Goal:** `main` carries fc-web, but nothing changes for anyone who doesn't
pass `--features web`:

- default builds,
- CI,
- the Docker image,
- `fc-server` and `fc-platform-server`.

Dropping the trial must be a small, mechanical diff.

### Steps

1. **Rebase `trial/topcoat` onto current `origin/main`.** `main` moves fast.
   Conflicts are most likely in the platform commit's files:
   - `auth/auth_api.rs` (`login` → `password_login`)
   - `auth/oidc_login_api.rs` (`check_domain` → `resolve_auth_method`)
   - `shared/middleware.rs`
   - `shared/bff_event_types_api.rs` (the seven access checks →
     `event_type::access`)
   - `shared/error.rs` (`status_and_code`)
   - `shared/public_api.rs`, `server_setup/platform_routes.rs`

   Re-apply the same extractions on top of whatever `main` did there. Then
   run `cargo test -p fc-platform`, which includes
   `permission_convention_test` and `uow_convention_test`.
2. **Take fc-web out of default workspace builds.**
   - Today `crates/fc-web` and `crates/fc-web/bundle` are listed in the root
     `Cargo.toml` `members`. That means `cargo build --workspace` /
     `cargo test --workspace` (and CI, if it uses them) compile Topcoat.
   - Topcoat's build script downloads the Tailwind CLI and the Lucide icon set
     from the network.
   - Check `.github/workflows/*.yml` for `--workspace` / `--all-features`.
     `--all-features` on fc-dev would turn `web` on.
   - Preferred: move fc-web to the root `exclude` list. It stays reachable as
     fc-dev's optional path dependency. Then replace its `*.workspace = true`
     keys with explicit values; an excluded crate can't inherit.
   - If CI does pass `--all-features`, change it to list features explicitly,
     or gate `web` so CI doesn't enable it.
3. **Drop the separate bundle step. Prefer self-bundling.**
   - `topcoat asset bundle` can't pass cargo features. That is why
     `crates/fc-web/bundle` exists, and why a manual re-bundle is needed after
     every class change.
   - Better: in `fc_web::service`, when no bundle directory is found, bundle
     from `std::env::current_exe()` into a cache directory keyed by the
     executable's hash (`topcoat_asset::Bundler`, feature `bundler`). That
     removes the tool and the manual step, and keeps fc-dev a single binary.
   - Caveat: making `topcoat-asset` a direct dependency changes how some
     Topcoat macros resolve paths. It broke `fontsource_font!` when the
     dependency was *optional*. fc-web no longer uses web fonts; if you do
     this, make the dependency non-optional and check that everything builds.
4. **Write the removal recipe into `docs/topcoat-trial.md`.** It should be
   roughly:
   - delete `crates/fc-web`;
   - remove the `fc-web` optional dependency and the `web` feature from
     `bin/fc-dev/Cargo.toml`;
   - remove the three `#[cfg(feature = "web")]` blocks in
     `bin/fc-dev/src/main.rs` (the WebDeps construction, the two
     fallback-service branches, and `notify_dev_ready`);
   - remove the CLAUDE.md "fc-web (Topcoat UI trial)" section, the docs, and
     the workspace entry.

   The platform commit stays either way; it is useful on its own.
5. **Verify, then merge to `main`.**
   - `cargo build -p fc-dev` (no feature) behaves exactly as before, including
     the SPA fallback.
   - `cargo test -p fc-platform` and `cargo test -p fc-web` pass.
   - `cargo build -p fc-dev --features web` works.
   - CI is green.
   - Merge to `main` (no PR, per the owner) and push.

---

## Task B: port more sections, faithful to the side panels

The owner's bar is **"the same fit and finish as the Vue app"**. The first
styling pass was rejected as "off". The reference is the owner's
**production** UI:

- Dark navy sidebar with the theme-configured logo (never hard-code a logo).
- User card at the bottom of the sidebar that opens Profile / Reset Password
  / Sign Out.
- List pages with a search box, labelled filters and a table.
- Details in a **right-hand side panel** (drawer). The panel has:
  - a header with the name, a status tag, the code or id underneath, and a
    close button;
  - an inline editable form, with Save in the panel footer;
  - related tables with icon actions (eye / check / ban);
  - a "Danger Zone" of outlined Archive/Delete actions, each with its own
    confirm dialog.

`/ui/event-types` is the worked example of this pattern. Copy it.

### Before porting: confirm which frontend is canonical

The production screenshots show side panels (for example
`/event-types/{id}?edit=true` opens a drawer over the list). The Vue code on
`main` renders event-type details as a **full page**
(`EventTypeDetailPage.vue`). So production may run a newer or different
frontend than `main`.

- Grep `frontend/src` for drawers (`<Drawer`, `?edit`, side panel) to see
  which pages use them on `main`.
- **Ask the owner** which pages should get side panels, or for screenshots of
  each, before building them. Faithful means matching what they see.

### Suggested order

Start with the CRUD lists (small, and they build out the kit), then the high
volume pages:

1. Subscriptions
2. Connections
3. Dispatch Pools
4. Clients
5. Applications
6. Roles
7. Events and Dispatch Jobs (high volume)

For events and dispatch jobs, use a `?size=` selector with **no pagination**.
That is a standing owner rule for append-only firehose tables (see
CLAUDE.md "Frontend UI Conventions").

### Per-section checklist

1. **Read the sources.**
   - The Vue list and detail pages, and copy their values: columns, tag
     severities, labels, empty-state text, subtitles.
   - The axum/BFF handlers they call: which repository methods, which
     `checks::*`, which use cases.
2. **Pages.**
   - `#[page("/ui/(app)/<section>")]` and `#[page("/ui/(app)/<section>/{id}")]`,
     both rendering the list component.
   - The `{id}` route opens the side panel from the server, so writes can
     redirect back to it and the panel can be linked to.
3. **Side panel.**
   - A `#[shard("/ui/(app)/<section>/panel")]` keyed by a
     `signal(cx, || open_id)`. Clicking a row sets the signal, which keeps
     the list's scroll position.
   - Keep the shard's explicit path under `/ui/(app)`. The convention test
     enforces this.
4. **Writes.**
   - `#[route(POST "/ui/(app)/<section>/{id}/<action>")]` that:
     - checks the permission with `checks::*`, then the resource-level rule;
     - runs the **same use case** the BFF handler runs, via
       `ExecutionContext::from_auth`;
     - calls `set_flash` and `see_other` back to `/ui/<section>/{id}`.
   - Never write through a repository from fc-web.
   - If a resource rule is inlined in a BFF handler, extract it into
     fc-platform the way `event_type::access` was, and use it from both
     sides.
5. **UI kit.**
   - Reuse `src/ui.rs` (`page_header`, `filter_select`, `search_input`,
     `tag`, `confirm_dialog`, `json_block`, `local_time`, `empty_state`,
     `code_chips`) and the `.fc-*` classes in `styles.css`.
   - Add to the kit instead of writing one-off markup.
   - Modals are native `<dialog>` with `commandfor`/`command="show-modal"`.
6. **Navigation.** Point the section's entry in `app/shell.rs` `NAV` at
   `/ui/<section>`, and gate it with the same check the page uses.
7. **Verify.**
   - `cargo test -p fc-web`.
   - Rebuild, re-bundle, restart.
   - Screenshot the Vue page and the fc-web page side by side and compare.
   - Exercise every write with curl or a browser, and confirm the audit-log
     row appears.

### Known gaps to close along the way

- "Create Event Type" and "Add Schema" still open the Vue screens. Port them
  as side panels or dialogs.
- Not clicked through in a browser yet:
  - the password step and its show-password toggle;
  - passkey sign-in;
  - the user-menu popover and sidebar collapse;
  - the confirm dialogs.

  The markup is verified with curl. Password entry has to be done by a person.
- Integration tests (testcontainers, driving `fc_web::service` with
  `tower::ServiceExt::oneshot`) are not written. See `docs/topcoat-trial.md`
  for the scenarios.

---

## Gotchas (full list in `docs/topcoat-trial.md`)

- **`build.rs` rerun rule:** any `cargo:rerun-if-*` line turns off Cargo's
  default change detection. `build.rs` lists `styles.css` and `src`. Keep it
  that way, or the Tailwind CSS silently goes stale.
- **Runtime expressions (`$(...)`)** can't build an `Option` or use
  `String::new()`. Use an empty `String` for "none" and `"".to_owned()`.
- **One view per page:**
  - Every `view!` has its own type, so compute the state first and render once.
  - A `view!` outside a component needs `cx =>`, or make it a `#[component]`.
- **Icon sizes** take `Length::rem(..)` or `Length::px(..)`, and `IconData`
  must be `.clone()`d.
- **Bound attributes** (`:type=...`) already render their initial value. Don't
  also write the static attribute.
- **Shards run without their page's checks.** Every shard and route calls
  `auth(cx)?` and `permit(checks::…)?`, and
  `tests/auth_convention_test.rs` enforces it.
- **Chrome automation** against localhost was unreliable in this session
  (timeouts on the Vue pages too). Use screenshots when it works, and curl
  for everything else.

## Existing bugs found (not fixed; report or fix separately)

- The Vue audit log sends `applicationIds`/`clientIds`, which the API ignores
  (`AuditLogsQuery`).
- The Vue login:
  - After an interaction login it redirects with a GET to
    `/oidc/interaction/{uid}/login`, and no such route exists.
  - The OIDC path drops `interaction`.
  - `switchClient` posts to `/auth/client/{id}`, but the route is
    `/auth/client/switch`.
- The BFF add-schema handler ignores the version the user enters.
- `ArchiveEventTypeUseCase` archives an event type that still has a
  FINALISING schema. The "all schemas deprecated" rule exists only in the UI.
  Decide whether it belongs in the use case.
