//! `/ui/users`: the SPA's `UserListPage.vue` with its drawers,
//! `UserDetailDrawer.vue` + `UserDetailBody.vue` (`/ui/users/{id}`,
//! `?edit=true` opens it editing; see `users_drawer.rs`) and
//! `UserCreateDrawer.vue` (`/ui/users/new`).
//!
//! The first section built from Topcoat UI's own components
//! (`crate::components`, installed with `topcoat ui add`) rather than the
//! hand-built kit in `crate::ui`: table, input, select, checkbox, switch,
//! badge, button, dropdown menu (the Filters panel), pagination, sheet (the
//! drawers), dialog / alert dialog, alert and field. The page root carries
//! `tc-theme`, which sets Tailwind's radius and type scale to the SPA's
//! (see `styles.css`). `docs/topcoat-components.md` lists what Topcoat
//! has, what this page uses, and what is still hand-built.
//!
//! Reads and writes call the principal API's own handler bodies
//! (`fc_platform::principal::admin`, `go_api::client_association`,
//! `mfa::admin_api::reset_user_two_factor`, `developer_credential::api`),
//! with the states fc-dev built them with, so the checks, reach rules, use
//! cases, events and audit rows are the API's.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use fc_platform::principal::admin;
use fc_platform::principal::api::{CreateUserRequest, PrincipalResponse, PrincipalsQuery};
use fc_platform::{AuthContext, Client, PlatformError, checks};
use serde::Deserialize;
use topcoat::{
    Result,
    context::Cx,
    icon::{icon, iconify::iconify_icon},
    router::{
        Method,
        content::Form,
        error::{SeeOther, see_other},
        page,
        request::{method, uri},
    },
    runtime::{Event, shard, signal},
    view::{Length, View, attributes, class, component, view},
};

use super::users_drawer::{InviteLink, Overlay, overlay_dialog, user_drawer};
use crate::auth::{auth, permit, platform_error};
use crate::components::alert::{AlertVariant, alert, alert_description};
use crate::components::badge::{BadgeVariant, badge};
use crate::components::button::{ButtonSize, ButtonVariant, button, button_variants};
use crate::components::dialog::{dialog_description, dialog_header, dialog_title};
use crate::components::dropdown_menu::{
    dropdown_menu, dropdown_menu_content, dropdown_menu_trigger,
};
use crate::components::field::{
    field, field_description, field_error, field_group, field_label, field_title,
};
use crate::components::input::input;
use crate::components::pagination::{
    pagination, pagination_content, pagination_ellipsis, pagination_item, pagination_link,
    pagination_next, pagination_previous,
};
use crate::components::select::select;
use crate::components::sheet::{SheetSide, sheet, sheet_content};
use crate::components::switch::switch;
use crate::components::table::{
    table, table_body, table_cell, table_head, table_header, table_row,
};
use crate::components::tooltip::{tooltip, tooltip_content};
use crate::ui::{FlashKind, set_flash};

pub(crate) const LIST: &str = "/ui/users";
const FORM_ID: &str = "users-list";

/// The SPA's `:rowsPerPageOptions`, default 100 (`useListState` pageSize).
const ROWS_OPTIONS: [usize; 4] = [50, 100, 250, 500];
const DEFAULT_ROWS: usize = 100;

pub(crate) fn detail_href(id: &str) -> String {
    format!("{LIST}/{id}")
}

// ------------------------------------------------------------ shared bits

/// `getUserType` / the drawer's `userType`: the tag for a principal's tier.
pub(crate) struct UserType {
    pub label: &'static str,
    pub variant: BadgeVariant,
    pub star: bool,
}

pub(crate) fn user_type(scope: &str) -> UserType {
    match scope {
        "ANCHOR" => UserType {
            label: "Anchor",
            variant: BadgeVariant::Warn,
            star: true,
        },
        "PARTNER" => UserType {
            label: "Partner",
            variant: BadgeVariant::Info,
            star: false,
        },
        _ => UserType {
            label: "Client",
            variant: BadgeVariant::Secondary,
            star: false,
        },
    }
}

/// A PrimeVue `Tag`: Topcoat's badge with the FlowCatalyst severities.
#[component]
pub(crate) async fn tag(
    #[into] label: String,
    variant: BadgeVariant,
    #[default] star: bool,
) -> Result<impl View> {
    Ok(view! {
        badge(variant: variant,
            if star {
                icon(data: iconify_icon!("lucide:star"), size: Length::rem(0.85))
            }
            (label)
        )
    })
}

/// "Active" / "Inactive", as the SPA's status column.
#[component]
pub(crate) async fn status_tag(active: bool) -> Result<impl View> {
    Ok(view! {
        if active {
            badge(variant: BadgeVariant::Success, "Active")
        } else {
            badge(variant: BadgeVariant::Destructive, "Inactive")
        }
    })
}

/// `formatDate`: the date in the viewer's locale (`ui.js` rewrites the
/// UTC date the server renders). Absent or unreadable is "—".
#[component]
pub(crate) async fn local_date(at: Option<String>) -> Result<impl View> {
    let at: Option<DateTime<Utc>> = at
        .as_deref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&Utc));
    Ok(view! {
        match at {
            Some(at) => <time datetime=(at.to_rfc3339()) data-local-date="">(at.format("%Y-%m-%d").to_string())</time>,
            None => <span>"—"</span>,
        }
    })
}

/// Clients by id, for labels (`useClientOptions`).
pub(crate) fn client_names(clients: &[Client]) -> HashMap<String, (String, String)> {
    clients
        .iter()
        .map(|c| (c.id.clone(), (c.name.clone(), c.identifier.clone())))
        .collect()
}

/// The clients the caller reaches: every client for an anchor.
pub(crate) async fn reachable_clients(cx: &Cx, auth: &AuthContext) -> Result<Vec<Client>> {
    let mut clients = crate::deps(cx)
        .users
        .principals
        .client_repo
        .find_all()
        .await
        .map_err(platform_error)?;
    clients.retain(|c| auth.is_anchor() || auth.can_access_client(&c.id));
    clients.sort_by_key(|c| c.name.to_lowercase());
    Ok(clients)
}

// ---------------------------------------------------------------- list

/// The list's URL state, as the SPA's `useListState` keeps it: `q`,
/// `clientId`, `active`, `scope`, `roles` (comma-joined, or repeated by the
/// filter form's checkboxes), `page` (0-based), `pageSize`, `sortField`,
/// `sortOrder`. `fo` keeps the Filters panel open across a filter change.
#[derive(Clone, Default)]
struct ListState {
    q: String,
    client_id: String,
    active: String,
    scope: String,
    roles: Vec<String>,
    page: usize,
    page_size: usize,
    sort_field: String,
    sort_order: String,
    filters_open: bool,
    edit: bool,
}

impl ListState {
    fn from_request(cx: &Cx) -> Self {
        let mut s = ListState {
            page_size: DEFAULT_ROWS,
            ..Default::default()
        };
        let query = uri(cx).query().unwrap_or_default();
        for (k, v) in form_urlencoded::parse(query.as_bytes()) {
            let v = v.trim().to_owned();
            match k.as_ref() {
                "q" => s.q = v,
                "clientId" => s.client_id = v,
                "active" if v == "true" || v == "false" => s.active = v,
                "scope" if matches!(v.as_str(), "ANCHOR" | "PARTNER" | "CLIENT") => s.scope = v,
                "roles" => {
                    for role in v.split(',').map(str::trim).filter(|r| !r.is_empty()) {
                        if !s.roles.iter().any(|r| r == role) {
                            s.roles.push(role.to_owned());
                        }
                    }
                }
                "page" => s.page = v.parse().unwrap_or(0),
                "pageSize" => {
                    s.page_size = v
                        .parse()
                        .ok()
                        .filter(|n| ROWS_OPTIONS.contains(n))
                        .unwrap_or(DEFAULT_ROWS)
                }
                "sortField" if matches!(v.as_str(), "name" | "email" | "createdAt") => {
                    s.sort_field = v
                }
                "sortOrder" if v == "asc" || v == "desc" => s.sort_order = v,
                "fo" => s.filters_open = v == "1",
                "edit" => s.edit = v == "true",
                _ => {}
            }
        }
        s
    }

    fn active_filter_count(&self) -> usize {
        [
            !self.client_id.is_empty(),
            !self.active.is_empty(),
            !self.scope.is_empty(),
            !self.roles.is_empty(),
        ]
        .iter()
        .filter(|b| **b)
        .count()
    }

    fn has_active_filters(&self) -> bool {
        !self.q.is_empty() || self.active_filter_count() > 0
    }

    /// This state as a query string on `path`, with `page` / sort
    /// overridden; empty values and defaults are left out, as the SPA does.
    fn href(&self, path: &str, page: usize, sort: Option<(&str, &str)>) -> String {
        let (sort_field, sort_order) = sort.unwrap_or((&self.sort_field, &self.sort_order));
        let page = if page > 0 {
            page.to_string()
        } else {
            String::new()
        };
        let size = if self.page_size != DEFAULT_ROWS {
            self.page_size.to_string()
        } else {
            String::new()
        };
        let roles = self.roles.join(",");
        crate::ui::list_query(
            path,
            &[
                ("q", &self.q),
                ("clientId", &self.client_id),
                ("active", &self.active),
                ("scope", &self.scope),
                ("roles", &roles),
                ("page", &page),
                ("pageSize", &size),
                ("sortField", sort_field),
                ("sortOrder", sort_order),
            ],
        )
    }
}

#[page("/ui/(app)/users")]
async fn users(cx: &Cx) -> Result<impl View> {
    permit(checks::can_read_principals(auth(cx)?))?;
    Ok(
        view! { user_list(open_id: String::new(), create: None, overlay: None, dialog_error: String::new()) },
    )
}

#[page("/ui/(app)/users/{id}")]
async fn user_detail(cx: &Cx) -> Result<impl View> {
    permit(checks::can_read_principals(auth(cx)?))?;
    let id = super::users_drawer::target(cx);
    Ok(view! { user_list(open_id: id, create: None, overlay: None, dialog_error: String::new()) })
}

/// One row of the table, precomputed (views can't branch on types).
#[derive(Clone)]
struct Row {
    id: String,
    name: String,
    email: String,
    kind: &'static str,
    kind_variant: BadgeVariant,
    star: bool,
    kind_tooltip: String,
    client: ClientCell,
    active: bool,
    roles: Vec<String>,
    more_roles: usize,
    created_at: String,
}

#[derive(Clone)]
enum ClientCell {
    All,
    One { name: String, more: usize },
    None,
}

/// The table cells the SPA derives per user (`getUserType`, the client
/// column, the first two roles).
fn row(p: PrincipalResponse, names: &HashMap<String, (String, String)>) -> Row {
    let client_name = |id: &str| {
        names
            .get(id)
            .map_or_else(|| id.to_owned(), |(name, _)| name.clone())
    };
    let granted = p.granted_client_ids.len();
    // `getUserType`: the list derives the tier from the grants, as the SPA.
    let (kind, kind_variant, star, kind_tooltip) = if p.is_anchor_user {
        (
            "Anchor",
            BadgeVariant::Warn,
            true,
            "Has access to all clients via anchor domain".to_owned(),
        )
    } else if granted > 0 || p.client_id.is_none() {
        let tip = match p.client_id.as_deref() {
            Some(home) => format!("Home: {}, +{granted} granted", client_name(home)),
            None => format!("Access to {granted} client(s)"),
        };
        ("Partner", BadgeVariant::Info, false, tip)
    } else {
        let home = p.client_id.as_deref().map(&client_name).unwrap_or_default();
        (
            "Client",
            BadgeVariant::Secondary,
            false,
            format!("Home client: {home}"),
        )
    };
    let client = if p.is_anchor_user {
        ClientCell::All
    } else if let Some(home) = p.client_id.as_deref() {
        ClientCell::One {
            name: client_name(home),
            more: granted,
        }
    } else if let Some(first) = p.granted_client_ids.first() {
        ClientCell::One {
            name: client_name(first),
            more: granted - 1,
        }
    } else {
        ClientCell::None
    };
    let short: Vec<String> = p
        .roles
        .iter()
        .take(2)
        .map(|r| r.rsplit(':').next().unwrap_or(r).to_owned())
        .collect();
    Row {
        id: p.id,
        name: p.name,
        email: p.email.unwrap_or_default(),
        kind,
        kind_variant,
        star,
        kind_tooltip,
        client,
        active: p.active,
        more_roles: p.roles.len().saturating_sub(2),
        roles: short,
        created_at: p.created_at,
    }
}

#[component]
pub(crate) async fn user_list(
    cx: &Cx,
    open_id: String,
    create: Option<CreateState>,
    overlay: Option<Overlay>,
    /// A refused direct password reset: its dialog reopens with this.
    dialog_error: String,
) -> Result<impl View> {
    let auth = auth(cx)?;
    let can_create = checks::can_write_principals(auth).is_ok();
    let can_read_roles = checks::can_read_roles(auth).is_ok();
    let state = ListState::from_request(cx);
    let deps = crate::deps(cx);
    let principals = &deps.users.principals;

    // `usersApi.list({ type: USER, … })` through the handler body.
    let query = PrincipalsQuery {
        page: Some(state.page.to_string()),
        page_size: Some(state.page_size.to_string()),
        principal_type: Some("USER".to_owned()),
        scope: Some(state.scope.clone()).filter(|s| !s.is_empty()),
        client_id: Some(state.client_id.clone()).filter(|s| !s.is_empty()),
        q: Some(state.q.clone()).filter(|s| !s.is_empty()),
        active: Some(state.active.clone()).filter(|s| !s.is_empty()),
        roles: Some(state.roles.join(",")).filter(|s| !s.is_empty()),
        sort_field: Some(if state.sort_field.is_empty() {
            "createdAt".to_owned()
        } else {
            state.sort_field.clone()
        }),
        sort_order: Some(if state.sort_order.is_empty() {
            "asc".to_owned()
        } else {
            state.sort_order.clone()
        }),
        email: None,
    };
    let roles_fut = async {
        if can_read_roles {
            principals.role_repo.find_all().await
        } else {
            Ok(Vec::new())
        }
    };
    let (listed, clients, roles) = tokio::try_join!(
        async {
            admin::list(principals, auth, &query)
                .await
                .map_err(platform_error)
        },
        reachable_clients(cx, auth),
        async { roles_fut.await.map_err(platform_error) },
    )?;
    let names = client_names(&clients);
    let total = listed.total;
    let rows: Vec<Row> = listed
        .principals
        .into_iter()
        .map(|p| row(p, &names))
        .collect();

    // "Showing 1 to 100 of 250 users" and the page links around the
    // current one, as PrimeVue's paginator.
    let pages = total.div_ceil(state.page_size).max(1);
    let page = state.page.min(pages - 1);
    let report = if total == 0 {
        "Showing 0 to 0 of 0 users".to_owned()
    } else {
        format!(
            "Showing {} to {} of {total} users",
            page * state.page_size + 1,
            ((page + 1) * state.page_size).min(total)
        )
    };
    let start = page.saturating_sub(2).min(pages.saturating_sub(5));
    let end = (start + 5).min(pages);
    let page_links: Vec<(usize, String)> = (start..end)
        .map(|p| (p, state.href(LIST, p, None)))
        .collect();
    let prev_href = (page > 0).then(|| state.href(LIST, page - 1, None));
    let next_href = (page + 1 < pages).then(|| state.href(LIST, page + 1, None));
    let gap_before = start > 0;
    let gap_after = end < pages;

    // Sortable headings: a click sorts ascending, a second descending.
    let sort_link = |sort_field: &str| {
        let current = if state.sort_field.is_empty() {
            "createdAt"
        } else {
            state.sort_field.as_str()
        };
        let order = if current == sort_field && state.sort_order != "desc" {
            "desc"
        } else {
            "asc"
        };
        let marker = if current == sort_field {
            if state.sort_order == "desc" {
                SortMark::Desc
            } else {
                SortMark::Asc
            }
        } else {
            SortMark::None
        };
        (state.href(LIST, 0, Some((sort_field, order))), marker)
    };
    let (name_sort, name_mark) = sort_link("name");
    let (email_sort, email_mark) = sort_link("email");
    let (created_sort, created_mark) = sort_link("createdAt");

    let client_options: Vec<(String, String)> = clients
        .iter()
        .map(|c| (c.id.clone(), c.name.clone()))
        .collect();
    let role_options: Vec<(String, String)> = {
        let mut r: Vec<(String, String)> = roles
            .into_iter()
            .map(|r| (r.name, r.display_name))
            .collect();
        r.sort_by_key(|(_, label)| label.to_lowercase());
        r
    };
    let selected_roles: HashSet<String> = state.roles.iter().cloned().collect();
    let active_count = state.active_filter_count();
    let has_active = state.has_active_filters();
    let size = state.page_size;
    let clear_href = LIST.to_owned();

    let start_editing = state.edit && !open_id.is_empty();
    let selected = signal(cx, move || open_id.clone());
    let editing = signal(cx, move || start_editing);
    // The reset-password dialog reopens with the refusal after a failed
    // POST (`reset_password` renders the page with it set).
    let dialog_error = signal(cx, move || dialog_error.clone());
    let (sheet_selected, drawer_selected, drawer_editing, drawer_error, close) = (
        selected.clone(),
        selected.clone(),
        editing.clone(),
        dialog_error.clone(),
        selected.clone(),
    );
    let close_href = state.href(LIST, state.page, None);
    let create_href = state.href(&format!("{LIST}/new"), 0, None);

    Ok(view! {
        <div class="tc-theme">
            <header class="mb-4 flex items-start justify-between gap-4">
                <div>
                    <h1 class="text-[24px] font-semibold text-navy-900">"Users"</h1>
                    <p class="mt-1 text-[14px] text-navy-500">"Manage platform users and their access"</p>
                </div>
                if can_create {
                    <a href=(create_href) class=(button_variants(ButtonVariant::Primary, ButtonSize::Md))>
                        icon(data: iconify_icon!("lucide:user-plus"), size: Length::rem(1.0))
                        "Add User"
                    </a>
                }
            </header>

            <div class="overflow-hidden rounded-xl border border-border bg-card">
                // FcTableToolbar: the list's GET form. Search on the left;
                // Clear All and the Filters panel (Topcoat's dropdown menu)
                // on the right. A filter change submits it.
                <form id=(FORM_ID) method="get" action=(LIST) class="flex flex-wrap items-center justify-between gap-3 border-b border-border px-4 py-3">
                    <input type="hidden" name="fo" value="">
                    if !state.sort_field.is_empty() {
                        <input type="hidden" name="sortField" value=(state.sort_field.clone())>
                    }
                    if !state.sort_order.is_empty() {
                        <input type="hidden" name="sortOrder" value=(state.sort_order.clone())>
                    }
                    <label class="relative block w-[260px] max-w-full">
                        <span class="sr-only">"Search by name or email"</span>
                        icon(data: iconify_icon!("lucide:search"), size: Length::rem(1.0), attrs: attributes! { class="pointer-events-none absolute top-1/2 left-2.5 -translate-y-1/2 text-muted-foreground" })
                        input(attrs: attributes! {
                            type="search" name="q" value=(state.q.clone())
                            placeholder="Search by name or email..." class="pl-8"
                        })
                    </label>
                    <div class="flex flex-wrap items-center gap-2">
                        if has_active {
                            <a href=(clear_href.clone()) class=(button_variants(ButtonVariant::Ghost, ButtonSize::Md))>
                                icon(data: iconify_icon!("lucide:funnel-x"), size: Length::rem(1.0))
                                "Clear All"
                            </a>
                        }
                        dropdown_menu(attrs: attributes! { open=(state.filters_open) },
                            dropdown_menu_trigger(attrs: attributes! { class=(class!(button_variants(ButtonVariant::Outline, ButtonSize::Md), "[&]:border-primary [&]:text-primary")) },
                                icon(data: iconify_icon!("lucide:funnel"), size: Length::rem(1.0))
                                "Filters"
                                if active_count > 0 {
                                    badge(attrs: attributes! { class="rounded-full" }, (active_count.to_string()))
                                }
                            )
                            dropdown_menu_content(attrs: attributes! { class="[&]:right-0 [&]:left-auto w-[360px] max-w-[calc(100vw-2rem)] p-4" },
                                field_group(attrs: attributes! { class="gap-4" },
                                    filter_select(name: "clientId", label: "Client", placeholder: "All clients", options: client_options, selected: state.client_id.clone())
                                    filter_select(name: "active", label: "Status", placeholder: "All statuses",
                                        options: vec![("true".to_owned(), "Active".to_owned()), ("false".to_owned(), "Inactive".to_owned())],
                                        selected: state.active.clone())
                                    filter_select(name: "scope", label: "Type", placeholder: "All types",
                                        options: vec![
                                            ("ANCHOR".to_owned(), "Anchor".to_owned()),
                                            ("PARTNER".to_owned(), "Partner".to_owned()),
                                            ("CLIENT".to_owned(), "Client".to_owned()),
                                        ],
                                        selected: state.scope.clone())
                                    if can_read_roles {
                                        field(
                                            field_title("Roles")
                                            <div class="max-h-48 overflow-y-auto rounded-lg border border-border p-2" role="group" aria-label="Roles">
                                                for (value, label) in role_options {
                                                    let id = format!("filter-role-{value}");
                                                    <label for=(&id) class="flex cursor-pointer items-center gap-2 rounded-md px-1.5 py-1 text-sm hover:bg-foreground/5">
                                                        crate::components::checkbox::checkbox(attrs: attributes! {
                                                            id=(&id) name="roles" value=(&value) checked=(selected_roles.contains(&value))
                                                            onchange="this.form.elements.fo.value='1';this.form.requestSubmit()"
                                                        })
                                                        <span class="truncate">(label)</span>
                                                    </label>
                                                }
                                            </div>
                                        )
                                    }
                                )
                            )
                        )
                    </div>
                </form>

                table(attrs: attributes! { class="[&_td]:py-[0.4rem] [&_td]:px-3 [&_th]:px-3" },
                    table_header(attrs: attributes! { class="bg-[#f8fafc] [&_th]:h-9 [&_th]:text-[12px] [&_th]:font-semibold [&_th]:tracking-wider [&_th]:text-[#475569] [&_th]:uppercase" },
                        table_row(attrs: attributes! { class="hover:bg-transparent" },
                            table_head(attrs: attributes! { class="w-[20%]" }, sort_heading(label: "Name", href: name_sort, mark: name_mark))
                            table_head(attrs: attributes! { class="w-[25%]" }, sort_heading(label: "Email", href: email_sort, mark: email_mark))
                            table_head(attrs: attributes! { class="w-[12%]" }, "Type")
                            table_head(attrs: attributes! { class="w-[15%]" }, "Client")
                            table_head(attrs: attributes! { class="w-[10%]" }, "Status")
                            table_head(attrs: attributes! { class="w-[15%]" }, "Roles")
                            table_head(attrs: attributes! { class="w-[10%]" }, sort_heading(label: "Created", href: created_sort, mark: created_mark))
                            table_head(attrs: attributes! { class="w-[60px]" }, "Actions")
                        )
                    )
                    table_body(attrs: attributes! { class="[&_tr:nth-child(even)]:bg-[#f8fafc]" },
                        if rows.is_empty() {
                            <tr>
                                <td colspan="8">
                                    // The DataTable `#empty` slot.
                                    <div class="flex flex-col items-center gap-3 px-6 py-12 text-muted-foreground">
                                        icon(data: iconify_icon!("lucide:users"), size: Length::px(48.0), attrs: attributes! { class="text-[#cbd5e1]" })
                                        <span>"No users found"</span>
                                        if has_active {
                                            <a href=(clear_href.clone()) class=(button_variants(ButtonVariant::Text, ButtonSize::Sm))>"Clear filters"</a>
                                        }
                                    </div>
                                </td>
                            </tr>
                        }
                        for r in rows {
                            let (open_id, edit_id) = (r.id.clone(), r.id.clone());
                            table_row(attrs: attributes! {
                                class="cursor-pointer"
                                @click=$(|_e: Event| { selected.set(open_id.clone()); editing.set(false); dialog_error.set("".to_owned()) })
                            },
                                table_cell(
                                    <a href=(detail_href(&r.id)) onclick="event.preventDefault()" class="font-medium text-foreground no-underline">(r.name)</a>
                                )
                                table_cell(attrs: attributes! { class="text-[13px] text-muted-foreground" },
                                    if r.email.is_empty() { "—" } else { (r.email) }
                                )
                                table_cell(
                                    tooltip(
                                        tag(label: r.kind, variant: r.kind_variant, star: r.star)
                                        tooltip_content((r.kind_tooltip))
                                    )
                                )
                                table_cell(attrs: attributes! { class="text-[13px]" },
                                    match r.client {
                                        ClientCell::All => <span class="font-medium text-[#f59e0b]">"All Clients"</span>,
                                        ClientCell::One { name, more } => <span class="flex flex-col gap-0.5">
                                            <span>(name)</span>
                                            if more > 0 {
                                                <span class="text-[11px] text-muted-foreground">"+" (more.to_string()) " more"</span>
                                            }
                                        </span>,
                                        ClientCell::None => <span class="text-[#94a3b8] italic">"No Client"</span>,
                                    }
                                )
                                table_cell(status_tag(active: r.active))
                                table_cell(
                                    <span class="flex flex-wrap items-center gap-1">
                                        for role in r.roles {
                                            badge(variant: BadgeVariant::Secondary, attrs: attributes! { class="text-[11px]" }, (role))
                                        }
                                        if r.more_roles > 0 {
                                            <span class="text-[12px] text-muted-foreground">"+" (r.more_roles.to_string()) " more"</span>
                                        }
                                    </span>
                                )
                                table_cell(attrs: attributes! { class="text-[13px] text-muted-foreground" }, local_date(at: Some(r.created_at)))
                                table_cell(
                                    tooltip(
                                        <a
                                            href=(format!("{}?edit=true", detail_href(&r.id)))
                                            class=(class!(button_variants(ButtonVariant::Ghost, ButtonSize::Icon), "[&]:rounded-full text-muted-foreground"))
                                            aria-label="Edit"
                                            onclick="event.preventDefault(); event.stopPropagation()"
                                            @click=$(|_e: Event| { selected.set(edit_id.clone()); editing.set(true); dialog_error.set("".to_owned()) })
                                        >
                                            icon(data: iconify_icon!("lucide:pencil"), size: Length::rem(1.0))
                                        </a>
                                        tooltip_content("Edit")
                                    )
                                )
                            )
                        }
                    )
                )

                // PrimeVue's paginator: page links, rows per page, report.
                <div class="flex flex-wrap items-center justify-center gap-3 border-t border-border px-4 py-2 text-sm text-muted-foreground">
                    pagination(attrs: attributes! { class="[&]:mx-0 [&]:w-auto" },
                        pagination_content(attrs: attributes! { class="[&]:flex-nowrap" },
                            pagination_item(
                                match prev_href {
                                    Some(href) => pagination_previous(attrs: attributes! { href=(href) }),
                                    None => pagination_previous(attrs: attributes! { aria-disabled="true" class="pointer-events-none opacity-50" }),
                                }
                            )
                            if gap_before { pagination_item(pagination_ellipsis()) }
                            for (p, href) in page_links {
                                // PrimeVue marks the current page with the primary fill.
                                pagination_item(pagination_link(active: p == page, attrs: attributes! {
                                    href=(href)
                                    class=(if p == page { "[&]:border-primary [&]:bg-primary [&]:text-primary-foreground" } else { "" })
                                }, ((p + 1).to_string())))
                            }
                            if gap_after { pagination_item(pagination_ellipsis()) }
                            pagination_item(
                                match next_href {
                                    Some(href) => pagination_next(attrs: attributes! { href=(href) }),
                                    None => pagination_next(attrs: attributes! { aria-disabled="true" class="pointer-events-none opacity-50" }),
                                }
                            )
                        )
                    )
                    select(attrs: attributes! {
                        name="pageSize" form=(FORM_ID) aria-label="Rows per page" class="w-24"
                        onchange="this.form.requestSubmit()"
                    },
                        for option in ROWS_OPTIONS {
                            <option value=(option.to_string()) selected=(option == size)>(option.to_string())</option>
                        }
                    )
                    <span>(report)</span>
                </div>
            </div>

            // EntityDrawer (non-modal, `size="wide"`): Topcoat's sheet with
            // its overlay made click-through, so the list stays usable.
            sheet(
                open: $(!sheet_selected.get().is_empty()),
                attrs: attributes! {
                    aria-label="User"
                    data-drawer=""
                    class="[&]:pointer-events-none [&]:bg-transparent [&]:backdrop-blur-none"
                },
                sheet_content(side: SheetSide::Right, attrs: attributes! {
                    class=(class!(DRAWER_PANEL, DRAWER_WIDE))
                },
                    button(variant: ButtonVariant::Ghost, size: ButtonSize::Icon, attrs: attributes! {
                        type="button" aria-label="Close" data-drawer-close=""
                        class="absolute top-4 right-4 z-10 [&]:rounded-full text-muted-foreground"
                        @click=$(|_e: Event| close.set("".to_owned()))
                    },
                        icon(data: iconify_icon!("lucide:x"), size: Length::rem(1.2))
                    )
                    user_drawer(id: $(drawer_selected.get()), editing: drawer_editing, dialog_error: $(drawer_error.get()))
                )
            )

            if let Some(create) = create {
                create_drawer(state: create, close_href: close_href)
            }
            if let Some(overlay) = overlay {
                overlay_dialog(overlay: overlay)
            }
        </div>
    })
}

/// The drawer panel: no padding (the header, body and footer pad
/// themselves) and the SPA's shadow. Its width is EntityDrawer's size:
/// [`DRAWER_WIDE`] or [`DRAWER_DEFAULT`].
pub(crate) const DRAWER_PANEL: &str = "pointer-events-auto [&]:max-w-none [&]:gap-0 [&]:p-0 [&]:shadow-[-8px_0_32px_rgb(15_23_42/0.18)]";
/// EntityDrawer `size="wide"`: 800px.
pub(crate) const DRAWER_WIDE: &str = "[&]:w-[min(800px,calc(100vw-24px))]";
/// EntityDrawer's default size: 560px.
pub(crate) const DRAWER_DEFAULT: &str = "[&]:w-[min(560px,calc(100vw-24px))]";

#[derive(Clone, Copy, PartialEq, Eq)]
enum SortMark {
    None,
    Asc,
    Desc,
}

#[component]
async fn sort_heading(label: &'static str, href: String, mark: SortMark) -> Result<impl View> {
    Ok(view! {
        <a href=(href) class="inline-flex items-center gap-1 text-inherit no-underline hover:text-foreground">
            (label)
            match mark {
                SortMark::Asc => icon(data: iconify_icon!("lucide:arrow-up"), size: Length::rem(0.85)),
                SortMark::Desc => icon(data: iconify_icon!("lucide:arrow-down"), size: Length::rem(0.85)),
                SortMark::None => icon(data: iconify_icon!("lucide:arrow-up-down"), size: Length::rem(0.85), attrs: attributes! { class="opacity-40" }),
            }
        </a>
    })
}

/// One filter in the Filters panel: Topcoat's field + select. A change
/// submits the list form and keeps the panel open.
#[component]
async fn filter_select(
    name: &'static str,
    label: &'static str,
    placeholder: &'static str,
    options: Vec<(String, String)>,
    selected: String,
) -> Result<impl View> {
    let id = format!("filter-{name}");
    Ok(view! {
        field(
            field_label(attrs: attributes! { for=(&id) }, (label))
            select(attrs: attributes! {
                id=(&id) name=(name)
                onchange="this.form.elements.fo.value='1';this.form.requestSubmit()"
            },
                <option value="" selected=(selected.is_empty())>(placeholder)</option>
                for (value, text) in options {
                    <option value=(&value) selected=(value == selected)>(text)</option>
                }
            )
        )
    })
}

// -------------------------------------------------------------- create

#[derive(Clone, Default, Deserialize)]
pub(crate) struct CreateForm {
    #[serde(default)]
    name: String,
    #[serde(default)]
    email: String,
    #[serde(default)]
    scope: String,
    #[serde(default)]
    client_id: String,
    /// Checkboxes: present ("on") when ticked.
    #[serde(default)]
    send_invitation: Option<String>,
    #[serde(default)]
    return_invite_link: Option<String>,
    #[serde(default)]
    invite_redirect_uri: String,
}

#[derive(Clone, Default)]
pub(crate) struct CreateState {
    form: CreateForm,
    /// Whether the form was posted (a fresh form sends invitations).
    posted: bool,
    error: Option<String>,
}

/// `UserCreateDrawer.vue`: `POST /api/principals/users` through its
/// handler body. The email-domain check (`checkEmailDomain`) renders as a
/// shard when the email changes; without the runtime the POST answers the
/// same refusal the API would and the re-rendered form shows the check.
#[page([GET, POST] "/ui/(app)/users/new")]
async fn create_user(cx: &Cx, form: Option<Form<CreateForm>>) -> Result<impl View> {
    let auth = auth(cx)?;
    permit(checks::can_write_principals(auth))?;
    let deps = crate::deps(cx);
    let mut state = CreateState::default();
    let mut overlay = None;
    let mut open_id = String::new();

    if method(cx) == Method::POST {
        let form = form.map(|Form(f)| f).unwrap_or_default();
        let trimmed = |s: &str| Some(s.trim().to_owned()).filter(|s| !s.is_empty());
        let outcome = if form.name.trim().is_empty() {
            Err(PlatformError::validation("Name is required"))
        } else if form.email.trim().is_empty() {
            Err(PlatformError::validation("Email is required"))
        } else {
            admin::create_user(
                &deps.users.principals,
                auth,
                CreateUserRequest {
                    email: form.email.trim().to_owned(),
                    password: None,
                    name: form.name.trim().to_owned(),
                    client_id: trimmed(&form.client_id),
                    scope: trimmed(&form.scope),
                    enforce_password_complexity: None,
                    send_invitation: Some(form.send_invitation.is_some()),
                    return_invite_link: Some(form.return_invite_link.is_some()),
                    invite_redirect_uri: trimmed(&form.invite_redirect_uri),
                },
            )
            .await
        };
        match outcome {
            Ok(created) => {
                // The SPA's toast: internal users get a sign-in link by
                // email, federated ones sign in at their IdP.
                let internal = created.idp_type.as_deref() == Some("INTERNAL");
                match created.invite_link {
                    // Shown once, in a dialog: the link never travels
                    // through a cookie or a URL.
                    Some(link) => {
                        open_id = created.id.clone();
                        overlay = Some(Overlay::Invite(InviteLink {
                            email: created.email.clone().unwrap_or_default(),
                            link,
                        }));
                    }
                    None => {
                        let message = if !internal {
                            "User created. They can sign in via their identity provider."
                        } else if form.send_invitation.is_some() {
                            "User created. We've emailed them a one-time sign-in link to set their password."
                        } else {
                            "User created. No invitation was sent."
                        };
                        set_flash(cx, FlashKind::Success, message);
                        return Err(see_other(detail_href(&created.id)).into());
                    }
                }
            }
            Err(e) if e.status_code().is_client_error() => state.error = Some(e.to_string()),
            Err(e) => {
                tracing::error!(error = %e, "fc-web: create user failed");
                state.error = Some("Failed to create user".to_owned());
            }
        }
        state.form = form;
        state.posted = true;
    }

    let create = overlay.is_none().then_some(state);
    Ok(
        view! { user_list(open_id: open_id, create: create, overlay: overlay, dialog_error: String::new()) },
    )
}

#[component]
async fn create_drawer(cx: &Cx, state: CreateState, close_href: String) -> Result<impl View> {
    let f = state.form;
    let email = f.email.clone();
    let email_signal = signal(cx, move || email.clone());
    let setter = email_signal.clone();
    let (scope, client_id) = (f.scope.clone(), f.client_id.clone());
    // A fresh form sends the invitation (the API's default).
    let send_invitation = !state.posted || f.send_invitation.is_some();
    let return_link = f.return_invite_link.is_some();
    Ok(view! {
        sheet(open: true, attrs: attributes! {
            aria-label="Add user"
            data-drawer=""
            class="[&]:pointer-events-none [&]:bg-transparent [&]:backdrop-blur-none"
        },
            sheet_content(side: SheetSide::Right, attrs: attributes! {
                class=(class!(DRAWER_PANEL, DRAWER_DEFAULT, "group/create"))
            },
                <div class="flex items-start gap-3 p-5 pr-16">
                    dialog_header(attrs: attributes! { class="min-w-0 flex-1 gap-0.5" },
                        dialog_title("Add User")
                        dialog_description("Create a new platform user")
                    )
                </div>
                <a href=(&close_href) aria-label="Close" data-drawer-close=""
                    class=(class!(button_variants(ButtonVariant::Ghost, ButtonSize::Icon), "absolute top-4 right-4 [&]:rounded-full text-muted-foreground"))>
                    icon(data: iconify_icon!("lucide:x"), size: Length::rem(1.2))
                </a>
                <form id="user-create-form" method="post" action=(format!("{LIST}/new")) class="min-h-0 flex-1 overflow-y-auto px-5 pb-5">
                    <h3 class="mb-3 text-[16px] font-semibold text-foreground">"User Information"</h3>
                    <div class="flex flex-col gap-4">
                        field(
                            field_label(attrs: attributes! { for="user-new-name" }, "Full Name" <span class="text-destructive">"*"</span>)
                            input(attrs: attributes! {
                                id="user-new-name" name="name" value=(f.name.clone())
                                placeholder="e.g., John Smith" required="" maxlength="255"
                            })
                        )
                        field(
                            field_label(attrs: attributes! { for="user-new-email" }, "Email Address" <span class="text-destructive">"*"</span>)
                            input(attrs: attributes! {
                                id="user-new-email" name="email" type="email" value=(f.email.clone())
                                placeholder="e.g., john.smith@example.com" required=""
                                @change=$(|e: Event| setter.set(e.target.value))
                            })
                        )
                    </div>
                    domain_check(email: $(email_signal.get()), scope: scope, client_id: client_id, send_invitation: send_invitation, return_link: return_link)
                    // Outside the shard, so what was typed survives an email change.
                    field(attrs: attributes! { class="mt-4" },
                        field_label(attrs: attributes! { for="user-new-redirect" }, "After setting a password, go to (optional)")
                        input(attrs: attributes! {
                            id="user-new-redirect" name="invite_redirect_uri" type="url" value=(f.invite_redirect_uri.clone())
                            placeholder="https://app.example.com/welcome"
                        })
                        field_description("Internal users only: where the invite link lands after the password is set.")
                    )
                    if let Some(error) = state.error {
                        alert(variant: AlertVariant::Destructive, attrs: attributes! { role="alert" class="mt-4" },
                            icon(data: iconify_icon!("lucide:circle-alert"))
                            alert_description(attrs: attributes! { class="[&]:text-destructive" }, (error))
                        )
                    }
                </form>
                <footer class="flex items-center justify-end gap-2 border-t border-border px-5 py-3">
                    <a href=(&close_href) class=(button_variants(ButtonVariant::Ghost, ButtonSize::Md))>"Cancel"</a>
                    button(attrs: attributes! {
                        type="submit" form="user-create-form"
                        class="group-has-[[data-block-create]]/create:pointer-events-none group-has-[[data-block-create]]/create:opacity-50"
                    },
                        icon(data: iconify_icon!("lucide:check"), size: Length::rem(1.0))
                        "Create User"
                    )
                </footer>
            )
        )
    })
}

/// How a user with this email would be created (`checkEmailDomain`,
/// through its handler body), and the fields that depend on it: the
/// client picker, the tier and the invitation options. Re-rendered when
/// the email changes.
#[shard("/ui/(app)/users/new/domain")]
async fn domain_check(
    cx: &Cx,
    email: String,
    scope: String,
    client_id: String,
    send_invitation: bool,
    return_link: bool,
) -> Result<impl View> {
    let auth = auth(cx)?;
    permit(checks::can_write_principals(auth))?;
    let email = email.trim().to_owned();
    let deps = crate::deps(cx);
    let check = if email.contains('@') {
        admin::check_email_domain(&deps.users.principals, auth, &email)
            .await
            .ok()
    } else {
        None
    };
    let clients = reachable_clients(cx, auth).await?;
    let anchor = auth.is_anchor();

    // The client choices: active clients, confined to the domain's
    // allow-list when it has one (`clientOptions`).
    let (requires_client, derived, internal, exists, warning, provider, options) = match &check {
        Some(c) => {
            let allowed = &c.allowed_client_ids;
            let options: Vec<(String, String)> = clients
                .iter()
                .filter(|cl| cl.status == fc_platform::ClientStatus::Active)
                .filter(|cl| allowed.is_empty() || allowed.contains(&cl.id))
                .map(|cl| (cl.id.clone(), format!("{} ({})", cl.name, cl.identifier)))
                .collect();
            (
                c.requires_client_id,
                c.derived_scope.clone(),
                c.auth_provider == "INTERNAL",
                c.email_exists,
                c.warning.clone(),
                c.auth_provider.clone(),
                options,
            )
        }
        None => (
            false,
            String::new(),
            true,
            false,
            None,
            String::new(),
            Vec::new(),
        ),
    };
    // One allowed client is pre-selected, as the SPA does.
    let client_id = if client_id.is_empty() && options.len() == 1 && requires_client {
        options[0].0.clone()
    } else {
        client_id
    };
    let scope = if scope.is_empty() {
        derived.clone()
    } else {
        scope
    };
    let checked = check.is_some();
    let partner = derived == "PARTNER";
    let no_clients = requires_client && options.is_empty();

    Ok(view! {
        <div class="mt-4 flex flex-col gap-4">
            if exists {
                // Blocks Create User (the footer button reads this marker).
                <div data-block-create="">
                    field_error((warning.unwrap_or_default()))
                </div>
            }
            if checked && !exists {
                <div class="flex flex-col gap-4">
                    if anchor {
                        field(
                            field_label(attrs: attributes! { for="user-new-scope" }, "Type")
                            select(attrs: attributes! { id="user-new-scope" name="scope" },
                                for (value, label) in [("ANCHOR", "Anchor"), ("PARTNER", "Partner"), ("CLIENT", "Client")] {
                                    <option value=(value) selected=(scope == value)>(label)</option>
                                }
                            )
                            field_description("The domain suggests " (derived.clone()) "; the platform only grants a tier the domain backs.")
                        )
                    } else {
                        <input type="hidden" name="scope" value="CLIENT">
                    }
                    if requires_client {
                        field(
                            field_label(attrs: attributes! { for="user-new-client" }, "Client" <span class="text-destructive">"*"</span>)
                            select(attrs: attributes! { id="user-new-client" name="client_id" disabled=(no_clients) required="" },
                                <option value="" selected=(client_id.is_empty())>"Select a client"</option>
                                for (value, label) in options {
                                    <option value=(&value) selected=(value == client_id)>(label)</option>
                                }
                            )
                            if no_clients {
                                <div data-block-create="">
                                    field_error("No clients available to assign. Configure a client first.")
                                </div>
                            } else if partner {
                                field_description("This partner domain restricts users to specific clients.")
                            } else {
                                field_description("Required for non-anchor users — sets the user's home client.")
                            }
                        )
                    }
                </div>
                <div class="flex flex-col gap-3 rounded-xl border border-border p-4">
                    <h4 class="text-sm font-semibold">"Invitation"</h4>
                    if internal {
                        field(orientation: crate::components::field::FieldOrientation::Horizontal,
                            crate::components::field::field_content(
                                field_label(attrs: attributes! { for="user-new-send" }, "Send invitation email")
                                field_description("Email a one-time link so they set their own password.")
                            )
                            switch(attrs: attributes! { id="user-new-send" name="send_invitation" checked=(send_invitation) })
                        )
                        field(orientation: crate::components::field::FieldOrientation::Horizontal,
                            crate::components::field::field_content(
                                field_label(attrs: attributes! { for="user-new-link" }, "Return the invite link instead")
                                field_description("Show the 72-hour set-password link here, once, instead of emailing it.")
                            )
                            switch(attrs: attributes! { id="user-new-link" name="return_invite_link" checked=(return_link) })
                        )
                    } else {
                        <input type="hidden" name="send_invitation" value="on">
                        <p class="text-sm text-muted-foreground">"Federated users sign in at their identity provider; nothing is sent."</p>
                    }
                </div>
                alert(attrs: attributes! { class="border-[#bae6fd] bg-[#f0f9ff] text-[#0369a1]" },
                    if internal {
                        icon(data: iconify_icon!("lucide:mail"))
                        alert_description(attrs: attributes! { class="[&]:text-[#0369a1]" },
                            "The user will be emailed a one-time sign-in link to set their own password — we never see or store an admin-set password. Scope: "
                            <strong>(derived.clone())</strong> "."
                        )
                    } else {
                        icon(data: iconify_icon!("lucide:info"))
                        alert_description(attrs: attributes! { class="[&]:text-[#0369a1]" },
                            "This user will authenticate via their organization's identity provider (" (provider.clone()) ") — no password to set. Scope: "
                            <strong>(derived.clone())</strong> "."
                        )
                    }
                )
            }
            if !checked {
                <p class="text-sm text-muted-foreground">"Enter the email address to see how the user will sign in and which client they belong to."</p>
            }
        </div>
    })
}

// ------------------------------------------------------------- helpers

/// Flash the outcome and go to `to`, or back to `retry` when refused.
pub(crate) fn finish<T>(
    cx: &Cx,
    outcome: std::result::Result<T, PlatformError>,
    ok: &str,
    to: String,
    retry: String,
) -> Result<SeeOther> {
    match outcome {
        Ok(_) => {
            set_flash(cx, FlashKind::Success, ok);
            Ok(see_other(to))
        }
        Err(e) if e.status_code().is_client_error() => {
            set_flash(cx, FlashKind::Error, e.to_string());
            Ok(see_other(retry))
        }
        Err(e) => {
            tracing::error!(error = %e, "fc-web: user write failed");
            set_flash(cx, FlashKind::Error, "Something went wrong. Try again.");
            Ok(see_other(retry))
        }
    }
}
