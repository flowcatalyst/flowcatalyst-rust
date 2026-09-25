//! `/ui/connections`: the SPA's `ConnectionListPage.vue` with
//! `ConnectionDetailDrawer.vue` (`/ui/connections/{id}`, `?edit=true`
//! opens it editing) and `ConnectionCreateDrawer.vue`
//! (`/ui/connections/new`). The pattern is `event_types.rs`'s.
//!
//! Reads go to the repository with the API's rules (`can_read_connections`,
//! then `connection::access`). Writes run the use cases `connection/api.rs`
//! runs, with its checks (`require_anchor` plus the connection
//! permission) and its execution context.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use fc_platform::connection::access::{ensure_visible, is_visible};
use fc_platform::connection::operations::{
    CreateConnectionCommand, CreateConnectionUseCase, DeleteConnectionCommand,
    DeleteConnectionUseCase, UpdateConnectionCommand, UpdateConnectionUseCase,
};
use fc_platform::usecase::UseCase;
use fc_platform::{
    AuthContext, Connection, ConnectionStatus, ExecutionContext, PlatformError, checks,
};
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
use crate::ui::{
    Btn, FlashKind, Pager, Severity, confirm_dialog, detail_field, detail_value, drawer_frame,
    drawer_header, empty_state, filter_select, form_field, list_query, page_header, paginator,
    set_flash, table_toolbar, tag,
};

path_param!(id);

const LIST: &str = "/ui/connections";
const FORM_ID: &str = "conn-list";

fn detail_href(id: &str) -> String {
    format!("{LIST}/{id}")
}

/// `getStatusSeverity`.
fn status_severity(status: ConnectionStatus) -> Severity {
    match status {
        ConnectionStatus::Active => Severity::Success,
        ConnectionStatus::Paused => Severity::Warn,
    }
}

/// `CODE_PATTERN` in the create drawer: `^[a-z][a-z0-9-]*$`.
fn valid_code(code: &str) -> bool {
    let mut chars = code.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// A date as the SPA's list shows it (`toLocaleDateString`, in the browser).
fn local_date(at: DateTime<Utc>) -> (String, String) {
    (at.to_rfc3339(), at.format("%Y-%m-%d UTC").to_string())
}

/// A timestamp as the SPA's drawer shows it (`toLocaleString`).
fn local_time(at: DateTime<Utc>) -> (String, String) {
    (
        at.to_rfc3339(),
        at.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
    )
}

/// The write checks `connection/api.rs` runs before update, pause and
/// activate.
fn can_update(auth: &AuthContext) -> bool {
    checks::require_anchor(auth).is_ok() && checks::can_update_connections(auth).is_ok()
}

fn can_delete(auth: &AuthContext) -> bool {
    checks::require_anchor(auth).is_ok() && checks::can_delete_connections(auth).is_ok()
}

fn can_create(auth: &AuthContext) -> bool {
    checks::require_anchor(auth).is_ok() && checks::can_create_connections(auth).is_ok()
}

// ---------------------------------------------------------------- list

#[query_params(error = bad_request)]
#[derive(Clone, Default)]
struct ListQuery {
    q: Option<String>,
    status: Option<String>,
    page: Option<usize>,
    rows: Option<usize>,
    edit: Option<String>,
}

impl ListQuery {
    fn get(value: &Option<String>) -> &str {
        value.as_deref().map(str::trim).unwrap_or_default()
    }

    fn href(&self, path: &str, page: Option<usize>) -> String {
        let rows = self.rows.map(|r| r.to_string()).unwrap_or_default();
        let page = page
            .filter(|p| *p > 1)
            .map(|p| p.to_string())
            .unwrap_or_default();
        list_query(
            path,
            &[
                ("q", Self::get(&self.q)),
                ("status", Self::get(&self.status)),
                ("rows", &rows),
                ("page", &page),
            ],
        )
    }
}

#[page("/ui/(app)/connections")]
async fn connections(cx: &Cx) -> Result<impl View> {
    permit(checks::can_read_connections(auth(cx)?))?;
    Ok(view! { connection_list(open_id: String::new(), create: None) })
}

#[page("/ui/(app)/connections/{id}")]
async fn connection_detail(cx: &Cx) -> Result<impl View> {
    permit(checks::can_read_connections(auth(cx)?))?;
    let id = path_param::<Id>(cx).to_owned();
    Ok(view! { connection_list(open_id: id, create: None) })
}

#[component]
async fn connection_list(
    cx: &Cx,
    open_id: String,
    create: Option<CreateState>,
) -> Result<impl View> {
    let auth = auth(cx)?;
    let query = query_params::<ListQuery>(cx)?;
    let search = ListQuery::get(&query.q).to_owned();
    let status = ListQuery::get(&query.status).to_owned();

    // `GET /api/connections` with no filters, then Go's
    // `FilterClientScoped`.
    let all = crate::deps(cx)
        .connection_repo
        .find_with_filters(None, None, None)
        .await
        .map_err(platform_error)?;
    let needle = search.to_lowercase();
    let rows: Vec<Connection> = all
        .into_iter()
        .filter(|c| is_visible(auth, c))
        .filter(|c| status.is_empty() || c.status.as_str() == status)
        .filter(|c| {
            // globalFilterFields: code, name, clientIdentifier.
            needle.is_empty()
                || c.code.to_lowercase().contains(&needle)
                || c.name.to_lowercase().contains(&needle)
                || c.client_identifier
                    .as_deref()
                    .is_some_and(|i| i.to_lowercase().contains(&needle))
        })
        .collect();
    let active_filters = usize::from(!status.is_empty());
    let has_active = active_filters > 0 || !search.is_empty();
    let pager = Pager::new(rows.len(), query.page, query.rows);
    let rows = pager.slice(rows);

    let start_editing = query.edit.as_deref() == Some("true") && !open_id.is_empty();
    let selected = signal(cx, move || open_id.clone());
    let editing = signal(cx, move || start_editing);
    let (frame_selected, drawer_editing) = (selected.clone(), editing.clone());

    let q = Arc::new(query.clone());
    let page_href: Arc<dyn Fn(usize) -> String + Send + Sync> = {
        let q = q.clone();
        Arc::new(move |p| q.href(LIST, Some(p)))
    };
    let close_href = q.href(LIST, query.page);
    let hidden = query
        .rows
        .map(|r| vec![("rows".to_owned(), r.to_string())])
        .unwrap_or_default();
    let create_href = q.href(&format!("{LIST}/new"), None);

    Ok(view! {
        page_header(title: "Connections", subtitle: "Manage webhook connections for event delivery",
            if can_create(auth) {
                <a href=(create_href) class=(Btn::Primary)>
                    icon(data: iconify_icon!("lucide:plus"), size: Length::rem(1.0))
                    "Create Connection"
                </a>
            }
        )

        <div class="fc-card">
            table_toolbar(
                form_id: FORM_ID,
                action: LIST,
                placeholder: "Search connections...",
                search: Some(search.clone()),
                active_filter_count: active_filters,
                has_active_filters: has_active,
                show_filters: true,
                show_search: true,
                hidden: hidden,
                filter_select(
                    name: "status",
                    label: "Status",
                    placeholder: "All statuses",
                    options: vec![("ACTIVE".to_owned(), "Active".to_owned()), ("PAUSED".to_owned(), "Paused".to_owned())],
                    selected: (!status.is_empty()).then(|| status.clone()),
                )
            )
            if rows.is_empty() {
                empty_state(message: "No connections found", clear_href: has_active.then(|| LIST.to_owned()))
            } else {
                <table class="fc-table fc-table-striped">
                    <thead>
                        <tr>
                            <th>"Code"</th>
                            <th>"Name"</th>
                            <th>"Scope"</th>
                            <th>"Status"</th>
                            <th>"Created"</th>
                            <th class="w-[80px]">"Actions"</th>
                        </tr>
                    </thead>
                    <tbody>
                        for c in &rows {
                            let id = c.id.clone();
                            let edit_id = c.id.clone();
                            let (created_iso, created_utc) = local_date(c.created_at);
                            <tr class="fc-row-link" @click=$(|_e: Event| { selected.set(id.clone()); editing.set(false) })>
                                <td>
                                    <a href=(detail_href(&c.id)) onclick="event.preventDefault()" class="no-underline">
                                        <code class="rounded bg-[#f1f5f9] px-2 py-0.5 text-[13px] text-[#1e293b]">(&c.code)</code>
                                    </a>
                                </td>
                                <td>(&c.name)</td>
                                <td><span class="text-[13px] text-[#475569]">(c.client_identifier.clone().unwrap_or_else(|| "Anchor-level".to_owned()))</span></td>
                                <td>tag(label: c.status.as_str(), severity: status_severity(c.status))</td>
                                <td><time datetime=(created_iso) data-local="date">(created_utc)</time></td>
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
            }
            paginator(pager: pager, noun: "connections", href: page_href, form_id: FORM_ID)
        </div>

        drawer_frame(selected: frame_selected, label: "Connection",
            connection_drawer(id: $(selected.get()), editing: drawer_editing)
        )

        if let Some(create) = create {
            create_drawer(state: create, close_href: close_href)
        }
    })
}

// -------------------------------------------------------------- drawer

#[shard("/ui/(app)/connections/drawer")]
async fn connection_drawer(cx: &Cx, id: String, editing: Signal<bool>) -> Result<impl View> {
    let auth = auth(cx)?;
    permit(checks::can_read_connections(auth))?;
    let conn = if id.is_empty() {
        None
    } else {
        let conn = crate::deps(cx)
            .connection_repo
            .find_by_id(&id)
            .await
            .map_err(platform_error)?
            .ok_or_not_found()?;
        permit(ensure_visible(auth, &conn))?;
        Some(conn)
    };

    Ok(view! {
        if let Some(conn) = conn {
            drawer_body(auth: auth.clone(), conn: conn, editing: editing)
        }
    })
}

#[component]
async fn drawer_body(
    auth: AuthContext,
    conn: Connection,
    editing: Signal<bool>,
) -> Result<impl View> {
    let editable = can_update(&auth);
    let deletable = can_delete(&auth);
    let base = detail_href(&conn.id);
    let form_id = "conn-edit-form";
    let scope = conn
        .client_identifier
        .clone()
        .unwrap_or_else(|| "Anchor-level (no client)".to_owned());
    let (created_iso, created_utc) = local_time(conn.created_at);
    let (updated_iso, updated_utc) = local_time(conn.updated_at);
    let paused = conn.status == ConnectionStatus::Paused;
    let (start, discard) = (editing.clone(), editing.clone());

    Ok(view! {
        drawer_header(title: conn.name.clone(), subtitle: Some(conn.code.clone()),
            tag(label: conn.status.as_str(), severity: status_severity(conn.status))
        )

        <div class="fc-drawer-body">
            <section class="fc-form-section">
                <header class="fc-section-header">
                    <h3 class="fc-section-title">"Connection Details"</h3>
                    if editable {
                        <button type="button" class=(Btn::TextPrimary) :hidden=$(editing.get()) @click=$(|_e: Event| start.set(true))>
                            icon(data: iconify_icon!("lucide:pencil"), size: Length::rem(1.0))
                            "Edit"
                        </button>
                    }
                </header>
                <div class="fc-section-body">
                    <div class="fc-detail-grid" :hidden=$(editing.get())>
                        detail_field(label: "Code", <code>(&conn.code)</code>)
                        detail_value(label: "Name", value: Some(conn.name.clone()))
                        if let Some(description) = conn.description.clone().filter(|d| !d.is_empty()) {
                            detail_value(label: "Description", value: Some(description), span: true)
                        }
                        if let Some(external_id) = conn.external_id.clone().filter(|d| !d.is_empty()) {
                            detail_field(label: "External ID", <code>(external_id)</code>)
                        }
                        detail_value(label: "Service Account", value: Some(conn.service_account_id.clone()))
                        detail_value(label: "Scope", value: Some(scope))
                        detail_field(label: "Status",
                            tag(label: conn.status.as_str(), severity: status_severity(conn.status))
                        )
                        detail_field(label: "Created", <time datetime=(created_iso) data-local="">(created_utc)</time>)
                        detail_field(label: "Updated", <time datetime=(updated_iso) data-local="">(updated_utc)</time>)
                    </div>
                    if editable {
                        <form id=(form_id) method="post" action=(format!("{base}/update")) class="fc-form-grid" data-dirty-form="" data-dirty-key=(&conn.id) :hidden=$(!editing.get())>
                            form_field(label: "Name", for_id: "conn-name", span: true,
                                <input id="conn-name" name="name" class="fc-input" value=(&conn.name)>
                            )
                            form_field(label: "Description", for_id: "conn-description", span: true,
                                <textarea id="conn-description" name="description" class="fc-input" rows="3">(conn.description.clone().unwrap_or_default())</textarea>
                            )
                            form_field(label: "External ID", for_id: "conn-external-id", span: true,
                                <input id="conn-external-id" name="external_id" class="fc-input" value=(conn.external_id.clone().unwrap_or_default())>
                            )
                        </form>
                    }
                </div>
            </section>

            if editable || deletable {
                <section class="fc-form-section" :hidden=$(editing.get())>
                    <header class="fc-section-header">
                        <h3 class="fc-section-title">"Actions"</h3>
                    </header>
                    <div class="fc-danger-actions">
                        if editable && paused {
                            <div class="fc-danger-item">
                                <div>
                                    <strong>"Activate Connection"</strong>
                                    <p>"Enable this connection for event delivery."</p>
                                </div>
                                <button type="button" class="fc-btn border-[#16a34a] bg-transparent text-[#16a34a] hover:bg-[#f0fdf4]" commandfor="conn-activate" command="show-modal">"Activate"</button>
                            </div>
                            confirm_dialog(
                                id: "conn-activate",
                                action: format!("{base}/activate"),
                                title: "Activate Connection",
                                message: "Activate this connection?",
                                confirm_label: "Activate",
                            )
                        }
                        if editable && !paused {
                            <div class="fc-danger-item">
                                <div>
                                    <strong>"Pause Connection"</strong>
                                    <p>"Temporarily stop event delivery through this connection."</p>
                                </div>
                                <button type="button" class=(Btn::WarnOutline) commandfor="conn-pause" command="show-modal">
                                    icon(data: iconify_icon!("lucide:pause"), size: Length::rem(1.0))
                                    "Pause"
                                </button>
                            </div>
                            confirm_dialog(
                                id: "conn-pause",
                                action: format!("{base}/pause"),
                                title: "Pause Connection",
                                message: "Pause this connection? Subscriptions using it will stop dispatching.",
                                confirm_label: "Pause",
                                warn: true,
                            )
                        }
                        if deletable {
                            <div class="fc-danger-item">
                                <div>
                                    <strong>"Delete Connection"</strong>
                                    <p>"Permanently delete this connection. Cannot be undone."</p>
                                </div>
                                <button type="button" class=(Btn::DangerOutline) commandfor="conn-delete" command="show-modal">
                                    icon(data: iconify_icon!("lucide:trash-2"), size: Length::rem(1.0))
                                    "Delete"
                                </button>
                            </div>
                            confirm_dialog(
                                id: "conn-delete",
                                action: format!("{base}/delete"),
                                title: "Delete Connection",
                                message: "Delete this connection? This action cannot be undone.",
                                confirm_label: "Delete",
                                danger: true,
                            )
                        }
                    </div>
                </section>
            }
        </div>

        if editable {
            <footer class="fc-drawer-footer" :hidden=$(!editing.get())>
                <button type="reset" form=(form_id) class=(Btn::Outline) hidden="" data-dirty-discard="" @click=$(|_e: Event| discard.set(false))>"Discard"</button>
                <button type="submit" form=(form_id) class=(Btn::Primary) disabled="" data-dirty-save="">"Save"</button>
            </footer>
        }
    })
}

// -------------------------------------------------------------- create

#[derive(Clone, Default, Deserialize)]
struct CreateForm {
    #[serde(default)]
    code: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    external_id: String,
    #[serde(default)]
    service_account_id: String,
    #[serde(default)]
    client_id: String,
}

#[derive(Clone, Default)]
struct CreateState {
    form: CreateForm,
    error: Option<String>,
    /// (id, name, code) of the active service accounts the caller may list.
    service_accounts: Vec<(String, String, String)>,
    /// (id, name, identifier) of the clients the caller may list.
    clients: Vec<(String, String, String)>,
}

/// `ConnectionCreateDrawer.vue`. GET shows it; POST creates through
/// `CreateConnectionUseCase` (what `POST /api/connections` runs), then opens
/// the new connection's drawer, or shows the form again with the error.
#[page([GET, POST] "/ui/(app)/connections/new")]
async fn create_connection(cx: &Cx, form: Option<Form<CreateForm>>) -> Result<impl View> {
    let auth = auth(cx)?;
    permit(checks::require_anchor(auth))?;
    permit(checks::can_create_connections(auth))?;
    let deps = crate::deps(cx);
    let mut state = CreateState::default();

    if method(cx) == Method::POST {
        let form = form.map(|Form(f)| f).unwrap_or_default();
        let code = form.code.trim();
        let outcome = if !(2..=100).contains(&code.len()) || !valid_code(code) {
            Err(PlatformError::validation(
                "Code: 2-100 characters, lowercase letters, numbers, hyphens only, starting with a letter",
            ))
        } else if form.name.trim().is_empty() || form.name.len() > 255 {
            Err(PlatformError::validation(
                "Name is required (at most 255 characters)",
            ))
        } else if form.service_account_id.is_empty() {
            Err(PlatformError::validation("Select a service account"))
        } else {
            let opt = |s: &str| Some(s.trim().to_owned()).filter(|s| !s.is_empty());
            CreateConnectionUseCase::new(
                deps.connection_repo.clone(),
                deps.service_account_repo.clone(),
                deps.unit_of_work.clone(),
            )
            .run(
                CreateConnectionCommand {
                    code: code.to_owned(),
                    name: form.name.clone(),
                    description: opt(&form.description),
                    service_account_id: form.service_account_id.clone(),
                    external_id: opt(&form.external_id),
                    client_id: opt(&form.client_id),
                    caller: Some(auth.clone()),
                },
                ExecutionContext::create(&auth.principal_id),
            )
            .await
            .into_result()
            .map(|event| event.connection_id)
            .map_err(PlatformError::from)
        };
        match outcome {
            Ok(id) => {
                set_flash(cx, FlashKind::Success, "Connection created");
                return Err(see_other(detail_href(&id)).into());
            }
            Err(e) if e.status_code().is_client_error() => state.error = Some(e.to_string()),
            Err(e) => {
                tracing::error!(error = %e, "fc-web: create connection failed");
                state.error = Some("Failed to create connection".to_owned());
            }
        }
        state.form = form;
    }

    // The drawer's lookups, as the SPA loads them: what the caller's own
    // reads allow (an empty list otherwise, as a refused fetch leaves it).
    if checks::can_read_service_accounts(auth).is_ok() {
        state.service_accounts = deps
            .service_account_repo
            .find_active()
            .await
            .map_err(platform_error)?
            .into_iter()
            .map(|sa| (sa.id, sa.name, sa.code))
            .collect();
    }
    if checks::can_read_clients(auth).is_ok() {
        state.clients = deps
            .client_repo
            .find_all()
            .await
            .map_err(platform_error)?
            .into_iter()
            .map(|c| (c.id, c.name, c.identifier))
            .collect();
    }
    Ok(view! { connection_list(open_id: String::new(), create: Some(state)) })
}

#[component]
async fn create_drawer(state: CreateState, close_href: String) -> Result<impl View> {
    let f = state.form;
    let h3 = "mb-4 text-[14px] font-semibold uppercase tracking-[0.05em] text-[#475569]";
    Ok(view! {
        <aside class="fc-drawer" role="complementary" aria-label="Create connection" data-drawer="">
            <a href=(&close_href) class="fc-icon-btn absolute top-[1.1rem] right-[1.1rem] z-10" aria-label="Close" data-drawer-close="">
                icon(data: iconify_icon!("lucide:x"), size: Length::rem(1.2))
            </a>
            drawer_header(title: "Create Connection", subtitle: Some("Configure a new webhook connection for event delivery".to_owned()))
            <form id="conn-create-form" method="post" action=(format!("{LIST}/new")) class="fc-drawer-body">
                <div class="mb-8">
                    <h3 class=(h3)>"Basic Information"</h3>
                    <div class="flex flex-col gap-5">
                        form_field(label: "Code", for_id: "conn-new-code", required: true, help: Some("Unique identifier for this connection (2-100 characters)".to_owned()),
                            <input id="conn-new-code" name="code" class="fc-input" value=(f.code.clone()) placeholder="connection-code" required="" minlength="2" maxlength="100" pattern="[a-z][a-z0-9\\-]*" title="Lowercase letters, numbers, hyphens only. Must start with a letter.">
                        )
                        form_field(label: "Name", for_id: "conn-new-name", required: true, help: Some("At most 255 characters".to_owned()),
                            <input id="conn-new-name" name="name" class="fc-input" value=(f.name.clone()) placeholder="Connection display name" required="" maxlength="255">
                        )
                        form_field(label: "Description", for_id: "conn-new-description",
                            <textarea id="conn-new-description" name="description" class="fc-input" rows="3" placeholder="Optional description...">(f.description.clone())</textarea>
                        )
                    </div>
                </div>

                <div class="mb-8">
                    <h3 class=(h3)>"Additional"</h3>
                    form_field(label: "External ID", for_id: "conn-new-external-id", help: Some("An optional external reference for this connection".to_owned()),
                        <input id="conn-new-external-id" name="external_id" class="fc-input" value=(f.external_id.clone()) placeholder="Optional external identifier">
                    )
                </div>

                <div class="mb-8">
                    <h3 class=(h3)>"Service Account"</h3>
                    form_field(label: "Service Account", for_id: "conn-new-sa", required: true,
                        <select id="conn-new-sa" name="service_account_id" class="fc-select" required="">
                            <option value="" selected=(f.service_account_id.is_empty())>"Select a service account"</option>
                            for (id, name, code) in state.service_accounts {
                                <option value=(&id) selected=(f.service_account_id == id)>(format!("{name} ({code})"))</option>
                            }
                        </select>
                    )
                </div>

                <div class="mb-8">
                    <h3 class=(h3)>"Scope"</h3>
                    form_field(label: "Client", for_id: "conn-new-client", help: Some("Leave empty for an anchor-level connection, or select a specific client.".to_owned()),
                        <select id="conn-new-client" name="client_id" class="fc-select">
                            <option value="" selected=(f.client_id.is_empty())>"Anchor-level (leave empty) or select a client"</option>
                            for (id, name, identifier) in state.clients {
                                <option value=(&id) selected=(f.client_id == id)>(format!("{name} ({identifier})"))</option>
                            }
                        </select>
                    )
                </div>

                if let Some(error) = state.error {
                    <div class="fc-banner fc-banner-error mb-4" role="alert">(error)</div>
                }
            </form>
            <footer class="fc-drawer-footer">
                <a href=(&close_href) class=(Btn::Outline)>
                    icon(data: iconify_icon!("lucide:x"), size: Length::rem(1.0))
                    "Cancel"
                </a>
                <button type="submit" form="conn-create-form" class=(Btn::Primary)>
                    icon(data: iconify_icon!("lucide:check"), size: Length::rem(1.0))
                    "Create Connection"
                </button>
            </footer>
        </aside>
    })
}

// -------------------------------------------------------------- writes

/// Load the connection a write targets, after the caller's checks. The API
/// looks the connection up inside the use case; here it is needed for the
/// redirect, and a connection the caller can't see is refused as the read
/// would be.
async fn load_for_write(cx: &Cx, auth: &AuthContext) -> Result<Connection> {
    let id = path_param::<Id>(cx);
    let conn = crate::deps(cx)
        .connection_repo
        .find_by_id(id)
        .await
        .map_err(platform_error)?
        .ok_or_not_found()?;
    permit(ensure_visible(auth, &conn))?;
    Ok(conn)
}

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
            tracing::error!(error = %e, "fc-web: connection write failed");
            set_flash(cx, FlashKind::Error, "Something went wrong. Try again.");
            Ok(see_other(retry))
        }
    }
}

/// `UpdateConnectionUseCase`, as `PUT /api/connections/{id}` and the
/// pause / activate endpoints run it.
async fn run_update(
    cx: &Cx,
    auth: &AuthContext,
    command: UpdateConnectionCommand,
) -> std::result::Result<(), PlatformError> {
    let deps = crate::deps(cx);
    UpdateConnectionUseCase::new(deps.connection_repo.clone(), deps.unit_of_work.clone())
        .run(command, ExecutionContext::create(&auth.principal_id))
        .await
        .into_result()
        .map(|_| ())
        .map_err(PlatformError::from)
}

fn status_command(id: &str, status: ConnectionStatus) -> UpdateConnectionCommand {
    UpdateConnectionCommand {
        connection_id: id.to_owned(),
        name: None,
        description: None,
        external_id: None,
        status: Some(status),
        service_account_id: None,
    }
}

#[derive(Deserialize)]
struct UpdateForm {
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    external_id: String,
}

#[route(POST "/ui/(app)/connections/{id}/update")]
async fn update(cx: &Cx, Form(form): Form<UpdateForm>) -> Result<SeeOther> {
    let auth = auth(cx)?;
    permit(checks::require_anchor(auth))?;
    permit(checks::can_update_connections(auth))?;
    let conn = load_for_write(cx, auth).await?;
    // As the drawer's save: `description || undefined`, so a blank field
    // leaves the stored value.
    let opt = |s: &str| Some(s.to_owned()).filter(|s| !s.is_empty());
    let outcome = run_update(
        cx,
        auth,
        UpdateConnectionCommand {
            connection_id: conn.id.clone(),
            name: Some(form.name),
            description: opt(&form.description),
            external_id: opt(&form.external_id),
            status: None,
            service_account_id: None,
        },
    )
    .await;
    let base = detail_href(&conn.id);
    let retry = format!("{base}?edit=true");
    finish(cx, outcome, "Connection updated", base, retry)
}

#[route(POST "/ui/(app)/connections/{id}/pause")]
async fn pause(cx: &Cx) -> Result<SeeOther> {
    let auth = auth(cx)?;
    permit(checks::require_anchor(auth))?;
    permit(checks::can_update_connections(auth))?;
    let conn = load_for_write(cx, auth).await?;
    let outcome = run_update(cx, auth, status_command(&conn.id, ConnectionStatus::Paused)).await;
    let base = detail_href(&conn.id);
    finish(cx, outcome, "Connection paused", base.clone(), base)
}

#[route(POST "/ui/(app)/connections/{id}/activate")]
async fn activate(cx: &Cx) -> Result<SeeOther> {
    let auth = auth(cx)?;
    permit(checks::require_anchor(auth))?;
    permit(checks::can_update_connections(auth))?;
    let conn = load_for_write(cx, auth).await?;
    let outcome = run_update(cx, auth, status_command(&conn.id, ConnectionStatus::Active)).await;
    let base = detail_href(&conn.id);
    finish(cx, outcome, "Connection activated", base.clone(), base)
}

#[route(POST "/ui/(app)/connections/{id}/delete")]
async fn delete(cx: &Cx) -> Result<SeeOther> {
    let auth = auth(cx)?;
    permit(checks::require_anchor(auth))?;
    permit(checks::can_delete_connections(auth))?;
    let conn = load_for_write(cx, auth).await?;
    let deps = crate::deps(cx);
    let outcome = DeleteConnectionUseCase::new(
        deps.connection_repo.clone(),
        deps.subscription_repo.clone(),
        deps.unit_of_work.clone(),
    )
    .run(
        DeleteConnectionCommand {
            connection_id: conn.id.clone(),
        },
        ExecutionContext::create(&auth.principal_id),
    )
    .await
    .into_result()
    .map(|_| ())
    .map_err(PlatformError::from);
    finish(
        cx,
        outcome,
        "Connection deleted",
        LIST.to_owned(),
        detail_href(&conn.id),
    )
}
