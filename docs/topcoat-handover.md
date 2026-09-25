# Topcoat UI: handover

For the agent picking this up. Read `docs/topcoat-trial.md` first: it
explains what was built, why, how it works, the scorecard, the gotchas and
the issues found. This file covers **where things stand and what is left**.

## Where things stand (2026-09-25, second session)

- **Branch:** `trial/topcoat`, rebased on local `main` and fast-forwardable
  from it. Not pushed (the owner hasn't asked for pushes).
- **Task A (land on `main` behind `--features web`) is done:**
  - The platform extractions were re-applied on top of main's Go-parity
    rewrites (login, error envelope, session cookie), keeping main's
    behaviour.
  - `crates/fc-web` is excluded from the workspace: `--workspace` builds,
    CI and the Docker image never compile Topcoat.
  - The asset bundle is written by the process at startup; the
    `fc-web-bundle` tool is gone.
  - The removal recipe is in `docs/topcoat-trial.md` ("Removing the
    trial").
- **Task B (sections, faithful to the SPA's drawers) is done for:** event
  types (reworked as the worked example, with create drawer and platform
  sync), subscriptions, connections, dispatch pools, clients, applications,
  roles, events, dispatch jobs, and the audit log. The shell's navigation
  and sidebar profile follow the SPA.
- **Reference UI:** `frontend/` on main is Go's production SPA. Mirror its
  list pages and `*Drawer.vue` files, not the old Rust Vue pages.

### Running it locally

```sh
docker run -d --name fc-topcoat-trial-pg -p 127.0.0.1:55432:5432 \
  -e POSTGRES_USER=flowcatalyst -e POSTGRES_PASSWORD=flowcatalyst -e POSTGRES_DB=flowcatalyst postgres:17-alpine
cargo build -p fc-dev --features web          # needs frontend/dist (or FC_SKIP_FRONTEND_BUILD=1 and a stub)
FC_DATABASE_URL=postgresql://flowcatalyst:flowcatalyst@localhost:55432/flowcatalyst \
  FC_EMBEDDED_DB=false target/debug/fc-dev --no-functions
# first time only: target/debug/fc-dev init --root /tmp/x --yes \
#   --admin-email admin@flowcatalyst.local --admin-password '…' --code orders --name Orders
```

- Open `http://localhost:8080/ui`.
- fc-web's tests: `CARGO_TARGET_DIR=target cargo test --manifest-path crates/fc-web/Cargo.toml`.
- A fresh worktree needs `frontend/dist` before fc-dev compiles (it embeds
  the SPA); copy one or build the frontend.

## Adding a section

1. **Read the sources:** the SPA's list page and drawers (columns, tag
   severities, labels, empty text, confirm messages) and the API handlers
   they call (checks, use cases, repository reads).
2. **Pages:** `#[page("/ui/(app)/<route>")]` and `…/{id}` render the list
   component; `…/new` is a `[GET, POST]` page that renders the list with
   the create drawer and re-renders it with the error on failure.
3. **Drawer:** `drawer_frame` around a `#[shard("/ui/(app)/<route>/drawer")]`
   taking `id: $(selected.get())` and the `editing` signal. Read view and
   edit form toggled with `:hidden=$(editing.get())`; the form carries
   `data-dirty-form` and `data-dirty-key`.
4. **Writes:** `#[route(POST "/ui/(app)/<route>/{id}/<action>")]`:
   `checks::*` as the handler, then the resource rule (from an
   `<aggregate>::access` module in fc-platform, extracted from the handler
   if it was inlined), then the handler's use case with
   `ExecutionContext::from_auth`, then flash + `see_other`. Never write
   through a repository.
5. **Navigation:** add the SPA route to `nav::PORTED`.
6. **Verify:** fc-web tests; rebuild and restart fc-dev; screenshot list,
   drawer, edit and create; exercise every write with curl (send `Origin`
   and `Sec-Fetch-Site: same-origin`) and check `aud_logs`.

## Left to do

- **Owner decisions / fixes on main** (see "Existing issues" in
  `docs/topcoat-trial.md`), most importantly: dispatch-pool write handlers
  have no permission check; `/bff/roles` reads have no permission check;
  dispatch-job attempts are never read back by the API.
- **Sections not ported:** users (platform and client-scoped), service
  accounts, identity providers, email domains, OAuth clients, reset
  approvals, portal apps and users, CORS origins, login attempts,
  settings, debug grids, scheduled jobs, processes, functions, developer
  pages, the profile page, the event-type add-schema page, the client
  login-theme page and the role editor (these three open the SPA).
- **Kit:** multi-select filters, searchable pickers, column sorting.
- **Not verified by a person:** password entry, passkeys, a real
  click-through of each drawer. Integration tests for `fc_web::service`.
