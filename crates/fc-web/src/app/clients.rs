//! `/ui/clients`: the SPA's `ClientListPage.vue` with its drawers,
//! `ClientDetailDrawer.vue` (`/ui/clients/{id}`, `?edit=true` opens it
//! editing) and `ClientCreateDrawer.vue` (`/ui/clients/new`). Login
//! branding is a full page in the SPA (`/clients/{id}/theme`); the drawer
//! links there.
//!
//! Same pattern as `event_types.rs`. Reads mirror `client/api.rs`
//! (`can_read_clients`, and a non-anchor caller only sees the clients it
//! can access); every write runs the use case its API handler runs, behind
//! the same permission check.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use fc_platform::application::operations::{
    UpdateClientApplicationsCommand, UpdateClientApplicationsUseCase,
};
use fc_platform::client::access::ensure_visible;
use fc_platform::client::operations::{
    ActivateClientCommand, ActivateClientUseCase, CreateClientCommand, CreateClientUseCase,
    DeleteClientCommand, DeleteClientUseCase, SuspendClientCommand, SuspendClientUseCase,
    UpdateClientCommand, UpdateClientUseCase,
};
use fc_platform::usecase::UseCase;
use fc_platform::{AuthContext, Client, ClientStatus, ExecutionContext, PlatformError, checks};
use serde::Deserialize;
use topcoat::{
    Result,
    context::Cx,
    icon::{icon, iconify::iconify_icon},
    router::{
        Method,
        content::Form,
        error::{RouterErrorExt, SeeOther, see_other},
        page, path_param, query_params,
        request::method,
        route,
    },
    runtime::{Event, Signal, shard, signal},
    view::{Length, View, component, view},
};

use crate::auth::{auth, permit, platform_error};
use crate::ui::drawer::DrawerSize;
use crate::ui::{
    Btn, FlashKind, Pager, Severity, confirm_dialog, detail_field, detail_value, drawer_frame,
    drawer_header, form_field, list_query, local_time, page_header, set_flash, table_toolbar, tag,
};

path_param!(id);

const LIST: &str = "/ui/clients";
const FORM_ID: &str = "clients-list";

/// PrimeVue's outlined `severity="success"` button.
const SUCCESS_OUTLINE: &str =
    "fc-btn border-[#16a34a] bg-transparent text-[#16a34a] hover:bg-[#f0fdf4]";

fn detail_href(id: &str) -> String {
    format!("{LIST}/{id}")
}

/// `getStatusSeverity`.
fn status_severity(status: ClientStatus) -> Severity {
    match status {
        ClientStatus::Active => Severity::Success,
        ClientStatus::Suspended => Severity::Warn,
        ClientStatus::Inactive => Severity::Secondary,
    }
}

/// `formatDate` in the list: the date only, in the viewer's locale
/// (`ui.js` rewrites it).
#[component]
pub(crate) async fn local_date(at: DateTime<Utc>) -> Result<impl View> {
    Ok(view! {
        <time datetime=(at.to_rfc3339()) data-local-date="">(at.format("%Y-%m-%d").to_string())</time>
    })
}

// ---------------------------------------------------------------- list

#[query_params(error = bad_request)]
#[derive(Clone, Default)]
struct ListQuery {
    q: Option<String>,
    page: Option<usize>,
    edit: Option<String>,
}

#[page("/ui/(app)/clients")]
async fn clients(cx: &Cx) -> Result<impl View> {
    permit(checks::can_read_clients(auth(cx)?))?;
    Ok(view! { client_list(open_id: String::new(), create: None) })
}

#[page("/ui/(app)/clients/{id}")]
async fn client_detail(cx: &Cx) -> Result<impl View> {
    permit(checks::can_read_clients(auth(cx)?))?;
    let id = path_param::<Id>(cx).to_owned();
    Ok(view! { client_list(open_id: id, create: None) })
}

#[component]
async fn client_list(cx: &Cx, open_id: String, create: Option<CreateState>) -> Result<impl View> {
    let auth = auth(cx)?;
    let can_create = checks::can_create_clients(auth).is_ok();
    let query = query_params::<ListQuery>(cx)?;
    let search = query
        .q
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .to_owned();

    // `list_clients`: every client, filtered to the caller's access.
    let all = crate::deps(cx)
        .client_repo
        .list(None)
        .await
        .map_err(platform_error)?;
    let needle = search.to_lowercase();
    let rows: Vec<Client> = all
        .into_iter()
        .filter(|c| auth.is_anchor() || auth.can_access_client(&c.id))
        .filter(|c| {
            needle.is_empty()
                || c.identifier.to_lowercase().contains(&needle)
                || c.name.to_lowercase().contains(&needle)
        })
        .collect();
    let has_active = !search.is_empty();
    // The SPA pages a window of 100 with Previous / Next (the API reports no
    // total).
    let pager = Pager::new(rows.len(), query.page, None);
    let rows = pager.slice(rows);
    let page_href = |p: usize| {
        let page = if p > 1 { p.to_string() } else { String::new() };
        list_query(LIST, &[("q", &search), ("page", &page)])
    };
    let prev_href = (pager.page > 1).then(|| page_href(pager.page - 1));
    let next_href = (pager.page < pager.pages()).then(|| page_href(pager.page + 1));

    let start_editing = query.edit.as_deref() == Some("true") && !open_id.is_empty();
    let selected = signal(cx, move || open_id.clone());
    let editing = signal(cx, move || start_editing);
    let (frame_selected, drawer_editing) = (selected.clone(), editing.clone());
    let close_href = list_query(LIST, &[("q", &search)]);
    let create_href = list_query(&format!("{LIST}/new"), &[("q", &search)]);

    Ok(view! {
        page_header(title: "Clients", subtitle: "Manage customer clients and their configurations",
            if can_create {
                <a href=(create_href) class=(Btn::Primary)>
                    icon(data: iconify_icon!("lucide:plus"), size: Length::rem(1.0))
                    "Create Client"
                </a>
            }
        )

        <div class="fc-card">
            <div class="-m-4 mb-0">
                table_toolbar(
                    form_id: FORM_ID,
                    action: LIST,
                    placeholder: "Search clients...",
                    search: Some(search.clone()),
                    has_active_filters: has_active,
                    show_search: true,
                )
            </div>
            <table class="fc-table fc-table-striped">
                <thead>
                    <tr>
                        <th>"Identifier"</th>
                        <th>"Name"</th>
                        <th>"Status"</th>
                        <th>"Created"</th>
                        <th class="w-[80px]">"Actions"</th>
                    </tr>
                </thead>
                <tbody>
                    if rows.is_empty() {
                        <tr><td colspan="5">"No clients found"</td></tr>
                    }
                    for c in &rows {
                        let id = c.id.clone();
                        let edit_id = c.id.clone();
                        <tr class="fc-row-link" @click=$(|_e: Event| { selected.set(id.clone()); editing.set(false) })>
                            <td>
                                <a href=(detail_href(&c.id)) onclick="event.preventDefault()" class="no-underline">
                                    <code class="rounded bg-[#f1f5f9] px-2 py-0.5 text-[13px] text-[#1e293b]">(&c.identifier)</code>
                                </a>
                            </td>
                            <td>(&c.name)</td>
                            <td>tag(label: c.status.as_str(), severity: status_severity(c.status))</td>
                            <td>local_date(at: c.created_at)</td>
                            <td>
                                <a
                                    href=(format!("{}?edit=true", detail_href(&c.id)))
                                    class="fc-icon-btn text-[#059669]"
                                    title="Edit"
                                    onclick="event.preventDefault(); event.stopPropagation()"
                                    @click=$(|_e: Event| { selected.set(edit_id.clone()); editing.set(true) })
                                >
                                    icon(data: iconify_icon!("lucide:pencil"), size: Length::rem(1.0))
                                </a>
                            </td>
                        </tr>
                    }
                </tbody>
            </table>
            if prev_href.is_some() || next_href.is_some() {
                <div class="mt-3 flex justify-end gap-2">
                    if let Some(href) = prev_href {
                        <a href=(href) class=(Btn::TextPrimary)>
                            icon(data: iconify_icon!("lucide:chevron-left"), size: Length::rem(1.0))
                            "Previous"
                        </a>
                    }
                    if let Some(href) = next_href {
                        <a href=(href) class=(Btn::TextPrimary)>
                            "Next"
                            icon(data: iconify_icon!("lucide:chevron-right"), size: Length::rem(1.0))
                        </a>
                    }
                </div>
            }
        </div>

        drawer_frame(selected: frame_selected, label: "Client", size: DrawerSize::Wide,
            client_drawer(id: $(selected.get()), editing: drawer_editing)
        )

        if let Some(create) = create {
            create_drawer(state: create, close_href: close_href)
        }
    })
}

// -------------------------------------------------------------- drawer

/// One application in the drawer's Applications section.
#[derive(Clone)]
struct AppRow {
    id: String,
    code: String,
    name: String,
    active: bool,
    enabled: bool,
}

#[shard("/ui/(app)/clients/drawer")]
async fn client_drawer(cx: &Cx, id: String, editing: Signal<bool>) -> Result<impl View> {
    let auth = auth(cx)?;
    permit(checks::can_read_clients(auth))?;
    let loaded = if id.is_empty() {
        None
    } else {
        // `get_client` + `get_client_applications`.
        permit(ensure_visible(auth, &id))?;
        let deps = crate::deps(cx);
        let (client, apps, configs) = tokio::try_join!(
            deps.client_repo.find_by_id(&id),
            deps.application_repo.find_all(),
            deps.application_client_config_repo.find_by_client(&id),
        )
        .map_err(platform_error)?;
        let client = client.ok_or_not_found()?;
        let enabled: std::collections::HashSet<&str> = configs
            .iter()
            .filter(|c| c.enabled)
            .map(|c| c.application_id.as_str())
            .collect();
        let apps: Vec<AppRow> = apps
            .into_iter()
            .map(|a| AppRow {
                enabled: enabled.contains(a.id.as_str()),
                id: a.id,
                code: a.code,
                name: a.name,
                active: a.active,
            })
            .collect();
        Some((client, apps))
    };

    Ok(view! {
        if let Some((client, apps)) = loaded {
            drawer_body(auth: auth.clone(), client: client, apps: apps, editing: editing)
        }
    })
}

#[component]
async fn drawer_body(
    auth: AuthContext,
    client: Client,
    apps: Vec<AppRow>,
    editing: Signal<bool>,
) -> Result<impl View> {
    let can_update = checks::can_update_clients(&auth).is_ok();
    let base = detail_href(&client.id);
    let form_id = "client-edit-form";
    let (start, discard) = (editing.clone(), editing.clone());
    let status = client.status;
    let available = apps.iter().filter(|a| !a.enabled).count();
    let enabled = apps.len() - available;
    let copy_id = client.id.clone();

    Ok(view! {
        drawer_header(title: client.name.clone(), subtitle: Some(client.identifier.clone()),
            tag(label: status.as_str(), severity: status_severity(status))
        )

        <div class="fc-drawer-body">
            <section class="fc-form-section">
                <header class="fc-section-header">
                    <h3 class="fc-section-title">"Client Details"</h3>
                    if can_update {
                        <button type="button" class=(Btn::TextPrimary) :hidden=$(editing.get()) @click=$(|_e: Event| start.set(true))>
                            icon(data: iconify_icon!("lucide:pencil"), size: Length::rem(1.0))
                            "Edit"
                        </button>
                    }
                </header>
                <div class="fc-section-body">
                    <div class="fc-detail-grid" :hidden=$(editing.get())>
                        detail_field(label: "Client ID",
                            <span class="inline-flex items-center gap-1">
                                <code>(&client.id)</code>
                                <button type="button" class="fc-icon-btn h-7 w-7 text-[#059669]" title="Copy client ID" data-copy=(copy_id) onclick="navigator.clipboard.writeText(this.dataset.copy)">
                                    icon(data: iconify_icon!("lucide:copy"), size: Length::rem(0.9))
                                </button>
                            </span>
                        )
                        detail_field(label: "Identifier", <code>(&client.identifier)</code>)
                        detail_value(label: "Name", value: Some(client.name.clone()))
                        detail_field(label: "Status", tag(label: status.as_str(), severity: status_severity(status)))
                        if client.status_reason.as_deref().is_some_and(|r| !r.is_empty()) {
                            detail_value(label: "Status Reason", value: client.status_reason.clone())
                        }
                        detail_field(label: "Created", local_time(at: client.created_at))
                        detail_field(label: "Updated", local_time(at: client.updated_at))
                    </div>
                    if can_update {
                        <form id=(form_id) method="post" action=(format!("{base}/update")) class="fc-form-grid" data-dirty-form="" data-dirty-key=(&client.id) :hidden=$(!editing.get())>
                            form_field(label: "Name", for_id: "client-name", span: true,
                                <input id="client-name" name="name" class="fc-input" value=(&client.name) required="" maxlength="255">
                            )
                        </form>
                    }
                </div>
            </section>

            <section class="fc-form-section">
                <form method="post" action=(format!("{base}/applications"))>
                    <header class="fc-section-header">
                        <h3 class="fc-section-title">"Applications"</h3>
                        if can_update {
                            <button type="submit" class=(Btn::Primary)>
                                icon(data: iconify_icon!("lucide:save"), size: Length::rem(1.0))
                                "Save"
                            </button>
                        }
                    </header>
                    <div class="fc-section-body">
                        <div class="grid grid-cols-2 gap-4">
                            app_list(title: format!("Available ({available})"), apps: apps.iter().filter(|a| !a.enabled).cloned().collect(), editable: can_update)
                            app_list(title: format!("Enabled ({enabled})"), apps: apps.iter().filter(|a| a.enabled).cloned().collect(), editable: can_update)
                        </div>
                        <p class="mt-3 text-[13px] text-[#64748b]">
                            "Tick applications to enable them for this client, untick to disable them. Click Save to apply changes."
                        </p>
                    </div>
                </form>
            </section>

            <section class="fc-form-section">
                <header class="fc-section-header"><h3 class="fc-section-title">"Login Branding"</h3></header>
                <div class="fc-danger-actions">
                    <div class="fc-danger-item">
                        <div>
                            <strong>"Login Branding"</strong>
                            <p>"Customize the sign-in, forgot-password and reset-password pages for this client."</p>
                        </div>
                        <a href=(format!("/clients/{}/theme", client.id)) class=(Btn::PrimaryOutline)>
                            icon(data: iconify_icon!("lucide:palette"), size: Length::rem(1.0))
                            "Edit Branding"
                        </a>
                    </div>
                </div>
            </section>

            <section class="fc-form-section" :hidden=$(editing.get())>
                <header class="fc-section-header"><h3 class="fc-section-title">"Actions"</h3></header>
                <div class="fc-danger-actions">
                    if status != ClientStatus::Active {
                        <div class="fc-danger-item">
                            <div>
                                <strong>"Activate Client"</strong>
                                <p>"Make this client active and accessible."</p>
                            </div>
                            <button type="button" class=(SUCCESS_OUTLINE) commandfor="client-activate" command="show-modal">"Activate"</button>
                        </div>
                        confirm_dialog(
                            id: "client-activate",
                            action: format!("{base}/activate"),
                            title: "Activate Client",
                            message: "Activate this client?",
                            confirm_label: "Activate",
                        )
                    }
                    if status == ClientStatus::Active {
                        <div class="fc-danger-item">
                            <div>
                                <strong>"Suspend Client"</strong>
                                <p>"Temporarily disable access to this client."</p>
                            </div>
                            <button type="button" class=(Btn::WarnOutline) commandfor="client-suspend" command="show-modal">"Suspend"</button>
                        </div>
                        confirm_dialog(
                            id: "client-suspend",
                            action: format!("{base}/suspend"),
                            title: "Suspend Client",
                            message: "Suspend this client? Users will not be able to access it.",
                            confirm_label: "Suspend",
                            warn: true,
                        )
                    }
                    if status != ClientStatus::Inactive {
                        <div class="fc-danger-item">
                            <div>
                                <strong>"Deactivate Client"</strong>
                                <p>"Soft delete this client. Can be reactivated later."</p>
                            </div>
                            <button type="button" class=(Btn::DangerOutline) commandfor="client-deactivate" command="show-modal">"Deactivate"</button>
                        </div>
                        confirm_dialog(
                            id: "client-deactivate",
                            action: format!("{base}/deactivate"),
                            title: "Deactivate Client",
                            message: "Deactivate this client? This is a soft delete.",
                            confirm_label: "Deactivate",
                            danger: true,
                        )
                    }
                </div>
            </section>
        </div>

        if can_update {
            <footer class="fc-drawer-footer" :hidden=$(!editing.get())>
                <button type="reset" form=(form_id) class=(Btn::Outline) hidden="" data-dirty-discard="" @click=$(|_e: Event| discard.set(false))>"Discard"</button>
                <button type="submit" form=(form_id) class=(Btn::Primary) disabled="" data-dirty-save="">"Save"</button>
            </footer>
        }
    })
}

/// One side of the Applications picker (`PickList` in the SPA): the
/// applications, each with a checkbox that says whether it is enabled.
#[component]
async fn app_list(title: String, apps: Vec<AppRow>, editable: bool) -> Result<impl View> {
    Ok(view! {
        <div class="flex flex-col overflow-hidden rounded-md border border-border">
            <div class="border-b border-border bg-[#f8fafc] px-3 py-2 text-[14px] font-semibold text-[#1e293b]">(title)</div>
            <div class="h-[300px] overflow-y-auto">
                for app in apps {
                    <label class="flex cursor-pointer items-center justify-between gap-3 border-b border-[#f1f5f9] px-3 py-2 hover:bg-[#f8fafc]">
                        <span class="flex items-center gap-3">
                            <input type="checkbox" name=(format!("app:{}", app.id)) value="on" checked=(app.enabled) disabled=(!editable) class="h-4 w-4 accent-[#059669]">
                            <span class="flex flex-col gap-0.5">
                                <span class="font-medium">(&app.name)</span>
                                <span class="font-mono text-[12px] text-[#64748b]">(&app.code)</span>
                            </span>
                        </span>
                        if !app.active {
                            <span class="fc-tag fc-tag-secondary text-[11px]">"Inactive"</span>
                        }
                    </label>
                }
            </div>
        </div>
    })
}

// -------------------------------------------------------------- create

#[derive(Clone, Default, Deserialize)]
struct CreateForm {
    #[serde(default)]
    name: String,
    #[serde(default)]
    identifier: String,
}

#[derive(Clone, Default)]
struct CreateState {
    form: CreateForm,
    error: Option<String>,
}

/// `IDENTIFIER_PATTERN` (`^[a-z][a-z0-9-]*$`), 2–100 characters.
fn valid_identifier(s: &str) -> bool {
    (2..=100).contains(&s.len())
        && s.starts_with(|c: char| c.is_ascii_lowercase())
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// `ClientCreateDrawer.vue`: `CreateClientUseCase`, as `POST /api/clients`.
#[page([GET, POST] "/ui/(app)/clients/new")]
async fn create_client(cx: &Cx, form: Option<Form<CreateForm>>) -> Result<impl View> {
    let auth = auth(cx)?;
    permit(checks::can_create_clients(auth))?;
    let deps = crate::deps(cx);
    let mut state = CreateState::default();

    if method(cx) == Method::POST {
        let form = form.map(|Form(f)| f).unwrap_or_default();
        let name = form.name.trim();
        let outcome = if name.is_empty() || form.name.len() > 255 {
            Err(PlatformError::validation(
                "Name is required (at most 255 characters)",
            ))
        } else if !valid_identifier(&form.identifier) {
            Err(PlatformError::validation(
                "Identifier: lowercase letters, numbers, hyphens only. Must start with a letter (2-100 characters).",
            ))
        } else {
            CreateClientUseCase::new(deps.client_repo.clone(), deps.unit_of_work.clone())
                .run(
                    CreateClientCommand {
                        name: form.name.clone(),
                        identifier: form.identifier.clone(),
                    },
                    ExecutionContext::from_auth(auth),
                )
                .await
                .into_result()
                .map(|event| event.client_id)
                .map_err(PlatformError::from)
        };
        match outcome {
            Ok(id) => {
                set_flash(cx, FlashKind::Success, "Client created");
                return Err(see_other(detail_href(&id)).into());
            }
            Err(e) if e.status_code().is_client_error() => state.error = Some(e.to_string()),
            Err(e) => {
                tracing::error!(error = %e, "fc-web: create client failed");
                state.error = Some("Failed to create client".to_owned());
            }
        }
        state.form = form;
    }

    Ok(view! { client_list(open_id: String::new(), create: Some(state)) })
}

#[component]
async fn create_drawer(state: CreateState, close_href: String) -> Result<impl View> {
    let f = state.form;
    Ok(view! {
        <aside class="fc-drawer" role="complementary" aria-label="Create client" data-drawer="">
            <a href=(&close_href) class="fc-icon-btn absolute top-[1.1rem] right-[1.1rem] z-10" aria-label="Close" data-drawer-close="">
                icon(data: iconify_icon!("lucide:x"), size: Length::rem(1.2))
            </a>
            drawer_header(title: "Create Client", subtitle: Some("Add a new customer client to the platform".to_owned()))
            <form id="client-create-form" method="post" action=(format!("{LIST}/new")) class="fc-drawer-body flex flex-col gap-5">
                form_field(label: "Name", for_id: "client-new-name", required: true, help: Some("At most 255 characters".to_owned()),
                    <input id="client-new-name" name="name" class="fc-input" value=(f.name.clone()) placeholder="Client display name" required="" maxlength="255">
                )
                form_field(label: "Identifier", for_id: "client-new-identifier", required: true, help: Some("Unique identifier used in URLs and configurations (2-100 characters)".to_owned()),
                    <input
                        id="client-new-identifier" name="identifier" class="fc-input" value=(f.identifier.clone())
                        placeholder="client-slug" required="" minlength="2" maxlength="100"
                        pattern="[a-z][a-z0-9\\-]*"
                        title="Lowercase letters, numbers, hyphens only. Must start with a letter."
                    >
                )
                if let Some(error) = state.error {
                    <div class="fc-banner fc-banner-error" role="alert">(error)</div>
                }
            </form>
            <footer class="fc-drawer-footer">
                <a href=(&close_href) class=(Btn::Outline)>
                    icon(data: iconify_icon!("lucide:x"), size: Length::rem(1.0))
                    "Cancel"
                </a>
                <button type="submit" form="client-create-form" class=(Btn::Primary)>
                    icon(data: iconify_icon!("lucide:check"), size: Length::rem(1.0))
                    "Create Client"
                </button>
            </footer>
        </aside>
    })
}

// -------------------------------------------------------------- writes

/// Flash the outcome and go to `to`, or back to `retry` when refused.
fn finish(
    cx: &Cx,
    outcome: std::result::Result<(), PlatformError>,
    ok: &str,
    to: String,
    retry: String,
) -> Result<SeeOther> {
    match outcome {
        Ok(()) => {
            set_flash(cx, FlashKind::Success, ok);
            Ok(see_other(to))
        }
        Err(e) if e.status_code().is_client_error() => {
            set_flash(cx, FlashKind::Error, e.to_string());
            Ok(see_other(retry))
        }
        Err(e) => {
            tracing::error!(error = %e, "fc-web: client write failed");
            set_flash(cx, FlashKind::Error, "Something went wrong. Try again.");
            Ok(see_other(retry))
        }
    }
}

fn target(cx: &Cx) -> String {
    path_param::<Id>(cx).to_owned()
}

#[derive(Deserialize)]
struct UpdateForm {
    name: String,
}

/// `PUT /api/clients/{id}`.
#[route(POST "/ui/(app)/clients/{id}/update")]
async fn update(cx: &Cx, Form(form): Form<UpdateForm>) -> Result<SeeOther> {
    let auth = auth(cx)?;
    permit(checks::can_update_clients(auth))?;
    let id = target(cx);
    let deps = crate::deps(cx);
    let outcome = UpdateClientUseCase::new(deps.client_repo.clone(), deps.unit_of_work.clone())
        .run(
            UpdateClientCommand {
                client_id: id.clone(),
                name: Some(form.name),
            },
            ExecutionContext::from_auth(auth),
        )
        .await
        .into_result()
        .map(|_| ())
        .map_err(PlatformError::from);
    let base = detail_href(&id);
    let retry = format!("{base}?edit=true");
    finish(cx, outcome, "Client updated", base, retry)
}

/// `POST /api/clients/{id}/activate`.
#[route(POST "/ui/(app)/clients/{id}/activate")]
async fn activate(cx: &Cx) -> Result<SeeOther> {
    let auth = auth(cx)?;
    permit(checks::can_activate_clients(auth))?;
    let id = target(cx);
    let deps = crate::deps(cx);
    let outcome = ActivateClientUseCase::new(deps.client_repo.clone(), deps.unit_of_work.clone())
        .run(
            ActivateClientCommand {
                client_id: id.clone(),
            },
            ExecutionContext::from_auth(auth),
        )
        .await
        .into_result()
        .map(|_| ())
        .map_err(PlatformError::from);
    let base = detail_href(&id);
    finish(cx, outcome, "Client activated", base.clone(), base)
}

/// `POST /api/clients/{id}/suspend`, with the SPA's reason.
#[route(POST "/ui/(app)/clients/{id}/suspend")]
async fn suspend(cx: &Cx) -> Result<SeeOther> {
    let auth = auth(cx)?;
    permit(checks::can_suspend_clients(auth))?;
    let id = target(cx);
    let deps = crate::deps(cx);
    let outcome = SuspendClientUseCase::new(deps.client_repo.clone(), deps.unit_of_work.clone())
        .run(
            SuspendClientCommand {
                client_id: id.clone(),
                reason: "Manual suspension".to_owned(),
            },
            ExecutionContext::from_auth(auth),
        )
        .await
        .into_result()
        .map(|_| ())
        .map_err(PlatformError::from);
    let base = detail_href(&id);
    finish(cx, outcome, "Client suspended", base.clone(), base)
}

/// `POST /api/clients/{id}/deactivate`: a soft delete through
/// `DeleteClientUseCase`, as the API does (the SPA's reason, "Manual
/// deactivation", is only logged there). The drawer closes, as the SPA's.
#[route(POST "/ui/(app)/clients/{id}/deactivate")]
async fn deactivate(cx: &Cx) -> Result<SeeOther> {
    let auth = auth(cx)?;
    permit(checks::can_deactivate_clients(auth))?;
    let id = target(cx);
    let deps = crate::deps(cx);
    let outcome = DeleteClientUseCase::new(deps.client_repo.clone(), deps.unit_of_work.clone())
        .run(
            DeleteClientCommand {
                client_id: id.clone(),
            },
            ExecutionContext::from_auth(auth),
        )
        .await
        .into_result()
        .map(|_| ())
        .map_err(PlatformError::from);
    if outcome.is_ok() {
        tracing::info!(client_id = %id, principal_id = %auth.principal_id, reason = "Manual deactivation", "Client deactivated");
    }
    finish(
        cx,
        outcome,
        "Client deactivated",
        LIST.to_owned(),
        detail_href(&id),
    )
}

/// `PUT /api/clients/{id}/applications`: the ticked applications become
/// the client's enabled set.
#[route(POST "/ui/(app)/clients/{id}/applications")]
async fn applications(cx: &Cx, Form(form): Form<HashMap<String, String>>) -> Result<SeeOther> {
    let auth = auth(cx)?;
    permit(checks::can_update_clients(auth))?;
    let id = target(cx);
    let deps = crate::deps(cx);
    let enabled_application_ids: Vec<String> = form
        .keys()
        .filter_map(|k| k.strip_prefix("app:"))
        .map(str::to_owned)
        .collect();
    let outcome = UpdateClientApplicationsUseCase::new(
        deps.application_repo.clone(),
        deps.client_repo.clone(),
        deps.application_client_config_repo.clone(),
        deps.unit_of_work.clone(),
    )
    .run(
        UpdateClientApplicationsCommand {
            client_id: id.clone(),
            enabled_application_ids,
        },
        ExecutionContext::from_auth(auth),
    )
    .await
    .into_result()
    .map(|_| ())
    .map_err(PlatformError::from);
    let base = detail_href(&id);
    finish(cx, outcome, "Applications updated", base.clone(), base)
}
