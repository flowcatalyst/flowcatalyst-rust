# Topcoat UI components in fc-web

**Status:** 2026-09-26. The users section (`/ui/users`) is the first fc-web
section built from Topcoat UI's own components. The older sections still
run on the hand-built `.fc-*` kit. This page is the catalogue, how fc-web
installs and themes the components, how the SPA's patterns map onto them,
what is still hand-built and why, and what moving the rest over would take.

**The rule:** Tailwind and Topcoat UI's components are the intended tools
in fc-web. The no-Tailwind rule is the Vue SPA's only. The first trial pass
used the components with Topcoat's unmodified neutral theme, and the owner
called the result "off". That was about fit and finish: the theme below
fixes it without hand-building the components.

## What Topcoat UI is

- A **copy-in component registry**, shadcn/ui's model in Rust
  (`topcoat-ui` 0.9.0 manages it, `topcoat-ui-registry` 0.9.0 ships it).
  `topcoat ui add <name>` copies a component's source into the crate
  (default `src/components/`), declares the module, and records a source
  hash in `components.toml`. The copy is ours to edit.
- **31 components and one theme** (`neutral`). The theme is a stylesheet
  of CSS custom properties that components read through Tailwind colour
  utilities (`bg-primary`, `text-muted-foreground`, `border-border`, …).
- **Commands:** `topcoat ui init` (writes `components.toml` and the theme
  stylesheet), `add`, `list` (shows when the registry has a newer source
  than the installed hash), `remove`. `add --overwrite` pulls a new
  version and drops local edits. Topcoat's `ui` feature must be on for the
  CLI to find the registry. It adds only `topcoat-ui-registry`, a crate
  with no dependencies.
- **Custom registries:** any crate with `[package.metadata.topcoat-ui]`
  and a `registry.toml` can publish components and themes. A FlowCatalyst
  registry (the SPA theme and the components below) is an option if more
  than one crate ever needs them.

### Conventions every component follows

- `#[component] pub async fn name(...)`, called as
  `name(prop: value, attrs: attributes! { … }, children…)`.
- **Variants and sizes are enum props** (`ButtonVariant::Destructive`).
- **`attrs`** goes to the component's main element. Classes are appended
  **without conflict resolution**: to override a default reliably, use a
  selector Tailwind emits later (`[&]:max-w-xl`, `[&_th]:uppercase`) or
  edit the component. Form controls (`select`, `checkbox`, `switch`,
  `radio_group_item`, `toggle`) put `class` on their wrapper and every
  other attribute on the `<input>` / `<select>`.
- **Class helpers** style other elements: `button_variants(variant, size)`
  (links as buttons), `badge_variants`, `sidebar_menu_button_variants`.
- **Interactivity is the browser's first:** `<details>` for menus and
  accordions, native inputs for toggles, CSS `:hover` / `:focus-within`
  for tooltips and hover cards, the customizable `<select>`
  (`appearance: base-select`) where the browser supports it. Only
  `dialog`, `alert_dialog`, `sheet`, `sidebar`, `tabs_trigger` and the
  sidebar's menu buttons take a runtime `Expr<bool>` (a constant or a
  signal expression): `open` or `active`.
- **The overlays are non-modal.** `dialog` / `sheet` render
  `<dialog :open=…>`, which the browser shows without a focus trap,
  Escape or backdrop (the docs say "focus trapping and Escape dismissal
  require application scripting"). fc-web renders them with `open: false`
  and opens them with invoker commands (`commandfor` +
  `command="show-modal"`), which gives all three.

## Catalogue

Parts are the extra components that compose with the main one. "Native"
means the browser does the interaction with no script.

| Component | Parts | Props | Element, interactivity | Notes |
|---|---|---|---|---|
| `accordion` | `accordion_item`, `accordion_trigger`, `accordion_content` | attrs, children | `<details>`/`<summary>`, native; animated height | One item open at a time needs `name` on the items |
| `alert` | `alert_title`, `alert_description` | `variant: Neutral \| Destructive`, attrs | `<div>` (add `role="alert"` for live messages) | An `svg` first child becomes the icon column |
| `alert_dialog` | uses the `dialog_*` parts | `open: Expr<bool>`, attrs | `<dialog role="alertdialog">` | Confirmations |
| `avatar` | `avatar_image`, `avatar_fallback` | `size: Sm \| Md \| Lg` | `<span>` | Initials fallback |
| `badge` | – | `variant: Primary \| Secondary \| Outline \| Destructive` (+ ours, below) | `<span>` | PrimeVue's `Tag` |
| `breadcrumb` | `_list`, `_item`, `_link`, `_page`, `_separator`, `_ellipsis` | attrs | `<nav><ol>` | |
| `button` | `button_variants()` | `variant: Primary \| Secondary \| Outline \| Ghost \| Destructive` (+ ours), `size: Sm \| Md \| Lg \| Icon` | `<button>` | Pass `type` in attrs |
| `card` | `card_header`, `card_title`, `card_description`, `card_content`, `card_footer` | attrs | `<div>` | 24px vertical padding, 20px gaps by default |
| `checkbox` | – | attrs | native `<input type=checkbox>` + overlaid check icon | No indeterminate styling |
| `dialog` | `dialog_content`, `dialog_header`, `dialog_title`, `dialog_description`, `dialog_footer` | `open: Expr<bool>`, attrs | `<dialog :open>`: full-viewport overlay, centred panel, fade/scale | No close button part |
| `dropdown_menu` | `_trigger`, `_content`, `_item`, `_sub`, `_sub_trigger`, `_sub_content`, `_label`, `_separator` | attrs | `<details>`/`<summary>`, native; absolutely positioned panel | No light dismiss, no flip/collision handling |
| `field` | `field_set`, `field_legend`, `field_group`, `field_content`, `field_label`, `field_title`, `field_description`, `field_separator`, `field_error` | `orientation: Vertical \| Horizontal \| Responsive`, legend `variant` | `<div role=group>`; label turns red when the control is `aria-invalid` | The form-layout kit |
| `hover_card` | `hover_card_content` | attrs | CSS hover / focus-within, 300ms delay | |
| `input` | – | attrs | `<input>` | `aria-invalid="true"` for the error border |
| `kbd` | `kbd_group` | attrs | `<kbd>` | |
| `label` | – | attrs | `<label>` | Follows its control's disabled state |
| `pagination` | `_content`, `_item`, `_link`, `_previous`, `_next`, `_ellipsis` | link `active: bool`; prev/next `label` | `<nav><ul>` of links | Links only: no page-size select, no report |
| `progress` | – | `value: Option<f32>`, `max` | native `<progress>` | |
| `radio_group` | `radio_group_item` | attrs | native radios | |
| `select` | – | attrs, `<option>` children | native `<select>`, styled picker where `base-select` is supported | Single select only |
| `separator` | – | `orientation` | `<hr>` | |
| `sheet` | `sheet_content` | `open: Expr<bool>`; content `side: Left \| Right \| Top \| Bottom` | `<dialog :open>` with an edge panel sliding in | An overlay; see the drawer below |
| `sidebar` | `sidebar_provider`, `_inset`, `_header`, `_footer`, `_content`, `_group`, `_group_label`, `_group_action`, `_group_content`, `_menu`, `_menu_item`, `_menu_button`, `_menu_action`, `_menu_badge`, `_menu_skeleton`, `_menu_sub`, `_menu_sub_item`, `_menu_sub_button`, `_input`, `_separator`, `_trigger`, `_rail` | `open`, `mobile_open: Expr<bool>`, `side`, `variant: Sidebar \| Floating \| Inset`, `collapsible: Offcanvas \| Icon \| None` | Desktop panel that becomes a sheet below 48rem; `--sidebar-*` tokens | The app shell |
| `skeleton` | – | attrs | pulsing `<div>` | |
| `spinner` | – | `size: Length`, `label` | spinning Lucide icon | |
| `switch` | – | attrs | native `<input type=checkbox role=switch>` | |
| `table` | `table_header`, `table_body`, `table_footer`, `table_row`, `table_head`, `table_cell`, `table_caption` | attrs | `<table>` in a scroll container; row hover | Markup only: no sorting, selection or paging |
| `tabs` | `tabs_list`, `tabs_trigger`, `tabs_content` | trigger `active: Expr<bool>` | links (server tabs) or signal-driven panels | No arrow-key tab pattern |
| `textarea` | – | attrs | `<textarea>`, grows with content | |
| `toggle` | `toggle_group` | `kind: Independent \| Exclusive`, `size` | native checkbox/radio inside a `<label>` | Toggle buttons |
| `tooltip` | `tooltip_content` | attrs | CSS hover / focus-within bubble above | Text only, no placement options |

**Theme tokens** (neutral): `--background`, `--foreground`, `--card(-foreground)`,
`--popover(-foreground)`, `--muted-foreground`, `--primary(-foreground)`,
`--destructive(-foreground)`, `--border`, `--ring`, `--sidebar`,
`--sidebar-foreground`, `--sidebar-primary(-foreground)`,
`--sidebar-accent(-foreground)`, `--sidebar-border`, `--sidebar-ring`,
`--shadow-xs`, `--shadow-sm`; dark values under `.dark`. Geometry is
Tailwind's own theme: `--radius-*`, `--text-*`, `--spacing`.

## How fc-web installs and themes them

- `crates/fc-web/Cargo.toml` enables Topcoat's `ui` feature. The install
  state is `crates/fc-web/components.toml`. Run `topcoat ui add <name>`
  from `crates/fc-web`. `init` was run once with
  `--components-dir src/components`. It rewrites `styles.css`, so keep a
  copy if you ever re-run it.
- **Installed:** alert, alert_dialog, badge, button, checkbox, dialog,
  dropdown_menu, field, input, label, pagination, select, sheet, switch,
  table, tooltip: the ones the users section uses. `mod components` is
  `#[allow(dead_code)]`, because each component keeps its full API.
- **Tokens:** `styles.css` sets the neutral theme's tokens on `:root` to the
  SPA's values: PrimeVue Nora's emerald primary, slate text and borders,
  red destructive. The hand-built kit uses the same variables, so the two
  can't drift. fc-web adds four tokens: `--success`, `--info`,
  `--warning` (PrimeVue's Tag and button severities) and `--input` (the
  darker border of form controls).
- **Geometry: the `.tc-theme` scope.** The users page's root carries
  `class="tc-theme"`, which sets Tailwind's theme variables for the
  components inside it (modals included, since CSS custom properties
  inherit through the top layer):
  - `--radius-md` and `--radius-lg`: 2px, Nora's control and tag radius.
  - `--radius-xl`: 8px, for cards and dialogs.
  - The type scale for the SPA's 12.6px base: `text-xs` 12px (tags),
    `text-sm` 12.6px (controls and table text), `text-base` 14px,
    `text-lg` 18px (titles).

  It is a scope rather than `@theme` so that the hand-built sections,
  which use `rounded-md` and `text-sm` with the defaults, look the same as
  before. When they migrate, these values move into `@theme` and the scope
  goes.
- **Local edits to the installed sources** (lost on `add --overwrite`,
  so reapply them):
  - `badge.rs`: variants `Success`, `Info`, `Warn` (the new tokens); the
    base weight is bold, as PrimeVue's Tag.
  - `button.rs`: variants `Text` (PrimeVue's primary-coloured text
    button), `DestructiveOutline` and `SuccessOutline` (outlined
    severities).
  - `input.rs`, `select.rs`, `checkbox.rs`: `border-input` instead of
    `border-border`, and a white field background.
- **Per-use overrides** (`attrs` with `[&]:…`): the drawer's
  width, padding and shadow; the modal mask colour; the users table's
  uppercase headings and striping; the current page link's primary fill.
  These are listed as constants (`DRAWER_PANEL`, `MODAL`, …) in
  `app/users.rs` / `app/users_drawer.rs`.

## The SPA's patterns, mapped

| SPA piece | Topcoat UI | In the users section |
|---|---|---|
| Sidebar navigation (`AppSidebar`, `SidebarProfile`) | `sidebar` (+ menu parts, `--sidebar-*` tokens for the navy palette, `collapsible: Icon`) | Not migrated: the shell stays hand-built (see below) |
| Page header (`page-header`, title, subtitle, actions) | none | Tailwind markup; the action is a link with `button_variants` |
| `DataTable` list | `table` + parts | Used, with sort links in the headings and striped rows |
| `FcTableToolbar` search | `input` (no icon slot) | `input` with a Lucide icon positioned over it |
| Filters popover + count badge + Clear All | `dropdown_menu` (`<details>`), `field` + `select`, `badge`, `button` Ghost | Used. The panel stays open across a filter change (`fo=1`) |
| `MultiSelect` (roles filter) | none | A scrolling list of `checkbox`es in the Filters panel |
| `Paginator` | `pagination` + parts | Used, plus a `select` for rows per page and the report text |
| `EntityDrawer` (non-modal right panel, header, tags, close) | `sheet` + `sheet_content(side: Right)` + `dialog_header` / `dialog_title` / `dialog_description` | Used. The sheet's overlay is made click-through (`[&]:pointer-events-none [&]:bg-transparent`), so the list stays usable, as in the SPA |
| `FcFormSection` (title, actions, body) | none (`card` is a bordered panel) | Tailwind markup |
| `FcDetailField` read view | none | A `<dl>` grid |
| `FcFormField` / edit form | `field`, `field_label`, `field_description`, `field_error`, `input`, `select` | Used; `aria-invalid` turns the label red |
| Save enabled on change (`useDirtyForm`) | none | `ui.js` `data-dirty-form` (shared with the kit) |
| `Select` | `select` | Used |
| Searchable select / `AutoComplete` (client picker) | none | Native `select` (the browser's type-ahead) |
| `ToggleSwitch` | `switch` | Used (all-applications, invitation options) |
| Checkbox | `checkbox` | Used |
| `Dialog` | `dialog` + parts | Used (grant client access, password dialogs, pickers, one-time secret) |
| `ConfirmDialog` | `alert_dialog` + parts | Used (reset 2FA, revoke credential, delete) |
| Dialog close X | none | `button` Ghost Icon, `commandfor … command="close"` |
| Dual-pane pick list (Manage Roles / Applications) | none | Composed: `checkbox` list, filter `input`, mirrored "Selected" pane (`ui.js`) |
| `Tag` | `badge` (+ the three added variants) | Used |
| Tooltip (`v-tooltip`) | `tooltip` | Used (type tag, edit button) |
| `Message` (info / warn / error) | `alert` (`Neutral`, `Destructive`) | Used, with info / warn colours per use |
| Toast (`errorBus`) | none | The shell's flash banner (hand-built, shared) |
| Empty state (`#empty` slot) | none | Tailwind markup |
| Account actions / danger zone rows | `field(orientation: Horizontal)` + `field_content` / `field_title` / `field_description` | Used, on a grey card |
| Tabs | `tabs` | Not needed on this page |
| Spinner / skeleton | `spinner`, `skeleton` | Not needed (server-rendered) |

## Gaps: where Topcoat UI has nothing suitable

Each of these is hand-built in the users section, and why:

- **Toasts.** There is no toast or notification stack. The shell's flash
  banner still shows post/redirect messages. `alert` in a fixed stack with
  an auto-dismiss in `ui.js` is the obvious build.
- **Multi-select and searchable select.** `select` is a single native
  select. The roles filter is a checkbox list, and the client pickers
  are plain selects.
- **Popover.** There is no anchored popover with light dismiss. The
  `<details>` dropdown menu does for the Filters panel, but it closes
  only when its trigger is clicked again. The older sections use a native
  `popover` positioned by `ui.js`.
- **A non-modal drawer.** `sheet` is an overlay. Making it click-through
  takes class overrides, and Escape needs `ui.js`, which now treats a
  `dialog[data-drawer]` as the drawer.
- **Modal focus trap and Escape.** Not built in (see above). fc-web opens
  dialogs with invoker commands instead of their `open` expression. The
  dialog also fills the viewport with its own mask, so the browser never
  sees a backdrop click. `ui.js` closes a `data-light-dismiss` dialog on a
  click on the mask.
- **Dialog close button, page header, section header, detail grid, empty
  state, input with icon.** These are small Tailwind markup, not worth
  their own components yet.
- **Pick list (dual pane).** Composed from checkboxes plus about 30 lines of
  `ui.js`.
- **Data-table behaviour.** `table` is markup only. Sorting is links in the
  headings, and paging is `pagination` plus a `select`, all server-side.
- **Tag severities.** Four badge variants are missing. They were added
  to the local copy.

## Moving the hand-built kit to Topcoat UI

**What exists.**
- The kit: `src/ui.rs` and `src/ui/*.rs`, about 880 lines. It has
  `page_header`, `table_toolbar`, `paginator`, `cursor_pager`,
  `filter_select`, `search_input`, `drawer_frame` / `drawer_header`,
  `form_field` / `detail_field` / `detail_value`, `tag`, `confirm_dialog`,
  `empty_state`, `flash_banner`, `json_block`, `code_chips` and
  `local_time`.
- About 1,000 lines of `.fc-*` CSS in `styles.css`, the sidebar and login
  included.
- Ten sections, the login page and the shell call the kit about 460 times
  and use about 490 `.fc-*` classes directly.

**The plan**, in order:

1. **Swap the kit's insides for Topcoat components, keeping its
   signatures.** `tag` becomes `badge`, `confirm_dialog` becomes
   `alert_dialog`, `drawer_frame` becomes `sheet`, `form_field` becomes
   `field`, `table_toolbar` / `filter_select` become `dropdown_menu`,
   `field` and `select`, `paginator` becomes `pagination`, and `Btn`
   becomes `button_variants`. The ten sections pick this up without
   edits. Move the `.tc-theme` values into `@theme` in the same change.
   About 1 day, most of it checking screenshots.
2. **Sweep the sections' own markup.** Replace inline `.fc-table`,
   `.fc-input`, `.fc-select`, `.fc-btn`, `.fc-form-grid` and
   `.fc-danger-item` with the components, as `users.rs` does. Then delete
   the `.fc-*` CSS they used. About a third of a day per large section
   (event types, subscriptions, applications, connections, dispatch
   pools, clients, roles) and less for the read-only ones: about 3 days
   in all.
3. **Shell last.** Move to `sidebar` + `sidebar_inset` with the navy
   palette as `--sidebar-*` tokens, and the user menu as `dropdown_menu`.
   The collapse state would become a signal instead of the cookie and
   class it is today. Keep the cookie for first paint. About 1 day,
   mainly because `sidebar` renders through `sheet` and needs a careful
   check on narrow screens.
4. **Login** is its own layout. It moves last, or not at all (half a day).

That is about 5–6 working days by hand (much less for an agent working
section by section), with no platform changes.

**What Topcoat can't express, and stays ours:** toasts, multi-select and
searchable selects, the pick list, an anchored popover with light dismiss,
code chips, a JSON view, and localised timestamps. Keep these as small
FlowCatalyst components next to the installed ones, or publish them as a
custom registry.

**Recommendation: do it, in the order above, and treat Topcoat UI's
components as the base layer from now on.** The users section shows the
SPA's look is reachable through tokens, Tailwind's theme and a few local
edits. That leaves less of our own CSS to maintain than the kit (the users
section uses no `.fc-*` class; the older sections use 10–75 each), and new sections get
Topcoat's accessibility work (labels following `aria-invalid`,
`role=alertdialog`, focus rings) for free. Keep two guards:
- List the local edits (above) and re-check them whenever Topcoat is
  upgraded. `topcoat ui list` shows which components changed upstream.
- Keep pages working as plain HTML first. Topcoat's own components are
  built that way.
