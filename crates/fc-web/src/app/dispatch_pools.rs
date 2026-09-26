//! `/ui/dispatch-pools`: the SPA's `DispatchPoolListPage.vue` with
//! `DispatchPoolDetailDrawer.vue` (`/ui/dispatch-pools/{id}`, `?edit=true`
//! opens it editing) and `DispatchPoolCreateDrawer.vue`
//! (`/ui/dispatch-pools/new`). The pattern is `event_types.rs`'s.
//!
//! Reads: `can_read_dispatch_pools`, then `dispatch_pool::access`. The list
//! is Go's (`FindWithFilters`: every status, filtered by the status facet),
//! not Rust's `GET /api/dispatch-pools`, which lists ACTIVE pools only
//! when no client is given, so the SPA's Suspended / Archived filters
//! always come back empty there.
//!
//! Writes run the use cases `dispatch_pool/api.rs` runs, with its reach
//! rules (`dispatch_pool::access`) and execution context. Create, update,
//! suspend and activate also need Go's `CanWriteDispatchPools`
//! (`checks::can_write_dispatch_pools`), which the Rust handlers don't
//! check yet; delete needs anchor + `can_delete_dispatch_pools`, as there.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use fc_platform::dispatch_pool::access::{
    ensure_can_create, ensure_modifiable, ensure_visible, is_visible,
};
use fc_platform::dispatch_pool::operations::{
    ActivateDispatchPoolCommand, ActivateDispatchPoolUseCase, CreateDispatchPoolCommand,
    CreateDispatchPoolUseCase, DeleteDispatchPoolCommand, DeleteDispatchPoolUseCase,
    SuspendDispatchPoolCommand, SuspendDispatchPoolUseCase, UpdateDispatchPoolCommand,
    UpdateDispatchPoolUseCase,
};
use fc_platform::usecase::UseCase;
use fc_platform::{
    AuthContext, DispatchPool, DispatchPoolStatus, ExecutionContext, PlatformError, checks,
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

const LIST: &str = "/ui/dispatch-pools";
const FORM_ID: &str = "pool-list";

fn detail_href(id: &str) -> String {
    format!("{LIST}/{id}")
}

/// `getStatusSeverity`.
fn status_severity(status: DispatchPoolStatus) -> Severity {
    match status {
        DispatchPoolStatus::Active => Severity::Success,
        DispatchPoolStatus::Suspended => Severity::Warn,
        DispatchPoolStatus::Archived => Severity::Secondary,
    }
}

/// `CODE_PATTERN` in the create drawer: `^[a-z][a-z0-9-]*$`.
fn valid_code(code: &str) -> bool {
    let mut chars = code.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn local_date(at: DateTime<Utc>) -> (String, String) {
    (at.to_rfc3339(), at.format("%Y-%m-%d UTC").to_string())
}

fn local_time(at: DateTime<Utc>) -> (String, String) {
    (
        at.to_rfc3339(),
        at.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
    )
}

/// A positive whole number from a form field; blank is `None`.
fn parse_count(field: &str, label: &str) -> std::result::Result<Option<u32>, PlatformError> {
    let field = field.trim();
    if field.is_empty() {
        return Ok(None);
    }
    match field.parse::<u32>() {
        Ok(n) if n >= 1 => Ok(Some(n)),
        _ => Err(PlatformError::validation(format!(
            "{label} must be a whole number of at least 1"
        ))),
    }
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

#[page("/ui/(app)/dispatch-pools")]
async fn dispatch_pools(cx: &Cx) -> Result<impl View> {
    permit(checks::can_read_dispatch_pools(auth(cx)?))?;
    Ok(view! { pool_list(open_id: String::new(), create: None) })
}

#[page("/ui/(app)/dispatch-pools/{id}")]
async fn dispatch_pool_detail(cx: &Cx) -> Result<impl View> {
    permit(checks::can_read_dispatch_pools(auth(cx)?))?;
    let id = path_param::<Id>(cx).to_owned();
    Ok(view! { pool_list(open_id: id, create: None) })
}

#[component]
async fn pool_list(cx: &Cx, open_id: String, create: Option<CreateState>) -> Result<impl View> {
    let auth = auth(cx)?;
    let can_write = checks::can_write_dispatch_pools(auth).is_ok();
    let query = query_params::<ListQuery>(cx)?;
    let search = ListQuery::get(&query.q).to_owned();
    let status = ListQuery::get(&query.status).to_owned();

    let all = crate::deps(cx)
        .dispatch_pool_repo
        .find_all()
        .await
        .map_err(platform_error)?;
    let needle = search.to_lowercase();
    let rows: Vec<DispatchPool> = all
        .into_iter()
        .filter(|p| is_visible(auth, p))
        .filter(|p| status.is_empty() || p.status.as_str() == status)
        .filter(|p| {
            // globalFilterFields: code, name, clientIdentifier.
            needle.is_empty()
                || p.code.to_lowercase().contains(&needle)
                || p.name.to_lowercase().contains(&needle)
                || p.client_identifier
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
        page_header(title: "Dispatch Pools", subtitle: "Manage rate limiting and concurrency for dispatch jobs",
            if can_write {
                <a href=(create_href) class=(Btn::Primary)>
                    icon(data: iconify_icon!("lucide:plus"), size: Length::rem(1.0))
                    "Create Pool"
                </a>
            }
        )

        <div class="fc-card">
            table_toolbar(
                form_id: FORM_ID,
                action: LIST,
                placeholder: "Search pools...",
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
                    options: vec![
                        ("ACTIVE".to_owned(), "Active".to_owned()),
                        ("SUSPENDED".to_owned(), "Suspended".to_owned()),
                        ("ARCHIVED".to_owned(), "Archived".to_owned()),
                    ],
                    selected: (!status.is_empty()).then(|| status.clone()),
                )
            )
            if rows.is_empty() {
                empty_state(message: "No dispatch pools found", clear_href: has_active.then(|| LIST.to_owned()))
            } else {
                <table class="fc-table fc-table-striped">
                    <thead>
                        <tr>
                            <th>"Code"</th>
                            <th>"Name"</th>
                            <th>"Client Scope"</th>
                            <th>"Rate Limit"</th>
                            <th>"Concurrency"</th>
                            <th>"Status"</th>
                            <th>"Created"</th>
                            <th class="w-[80px]">"Actions"</th>
                        </tr>
                    </thead>
                    <tbody>
                        for p in &rows {
                            let id = p.id.clone();
                            let edit_id = p.id.clone();
                            let (created_iso, created_utc) = local_date(p.created_at);
                            <tr class="fc-row-link" @click=$(|_e: Event| { selected.set(id.clone()); editing.set(false) })>
                                <td>
                                    <a href=(detail_href(&p.id)) onclick="event.preventDefault()" class="no-underline">
                                        <code class="rounded bg-[#f1f5f9] px-2 py-0.5 text-[13px] text-[#1e293b]">(&p.code)</code>
                                    </a>
                                </td>
                                <td>(&p.name)</td>
                                <td><span class="text-[13px] text-[#475569]">(p.client_identifier.clone().unwrap_or_else(|| "Anchor-level".to_owned()))</span></td>
                                <td>
                                    match p.rate_limit {
                                        Some(n) => <span>(format!("{n}/min"))</span>,
                                        None => <span>"—"</span>,
                                    }
                                </td>
                                <td>(p.concurrency.to_string())</td>
                                <td>tag(label: p.status.as_str(), severity: status_severity(p.status))</td>
                                <td><time datetime=(created_iso) data-local="date">(created_utc)</time></td>
                                <td>
                                    <a
                                        href=(format!("{}?edit=true", detail_href(&p.id)))
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
            paginator(pager: pager, noun: "dispatch pools", href: page_href, form_id: FORM_ID)
        </div>

        drawer_frame(selected: frame_selected, label: "Dispatch pool",
            pool_drawer(id: $(selected.get()), editing: drawer_editing)
        )

        if let Some(create) = create {
            create_drawer(state: create, close_href: close_href)
        }
    })
}

// -------------------------------------------------------------- drawer

#[shard("/ui/(app)/dispatch-pools/drawer")]
async fn pool_drawer(cx: &Cx, id: String, editing: Signal<bool>) -> Result<impl View> {
    let auth = auth(cx)?;
    permit(checks::can_read_dispatch_pools(auth))?;
    let pool = if id.is_empty() {
        None
    } else {
        let pool = crate::deps(cx)
            .dispatch_pool_repo
            .find_by_id(&id)
            .await
            .map_err(platform_error)?
            .ok_or_not_found()?;
        permit(ensure_visible(auth, &pool))?;
        Some(pool)
    };

    Ok(view! {
        if let Some(pool) = pool {
            drawer_body(auth: auth.clone(), pool: pool, editing: editing)
        }
    })
}

#[component]
async fn drawer_body(
    auth: AuthContext,
    pool: DispatchPool,
    editing: Signal<bool>,
) -> Result<impl View> {
    let archived = pool.status == DispatchPoolStatus::Archived;
    let active = pool.status == DispatchPoolStatus::Active;
    let can_write = checks::can_write_dispatch_pools(&auth).is_ok();
    let editable = can_write && ensure_modifiable(&auth, &pool, "update").is_ok() && !archived;
    let can_suspend = can_write && ensure_modifiable(&auth, &pool, "suspend").is_ok();
    let can_activate = can_write && ensure_modifiable(&auth, &pool, "activate").is_ok();
    let deletable =
        checks::require_anchor(&auth).is_ok() && checks::can_delete_dispatch_pools(&auth).is_ok();
    let show_actions =
        !archived && ((active && can_suspend) || (!active && can_activate) || deletable);
    let base = detail_href(&pool.id);
    let form_id = "pool-edit-form";
    let scope = pool
        .client_identifier
        .clone()
        .unwrap_or_else(|| "Anchor-level (no client)".to_owned());
    let (created_iso, created_utc) = local_time(pool.created_at);
    let (updated_iso, updated_utc) = local_time(pool.updated_at);
    let (start, discard) = (editing.clone(), editing.clone());

    Ok(view! {
        drawer_header(title: pool.name.clone(), subtitle: Some(pool.code.clone()),
            tag(label: pool.status.as_str(), severity: status_severity(pool.status))
        )

        <div class="fc-drawer-body">
            <section class="fc-form-section">
                <header class="fc-section-header">
                    <h3 class="fc-section-title">"Pool Details"</h3>
                    if editable {
                        <button type="button" class=(Btn::TextPrimary) :hidden=$(editing.get()) @click=$(|_e: Event| start.set(true))>
                            icon(data: iconify_icon!("lucide:pencil"), size: Length::rem(1.0))
                            "Edit"
                        </button>
                    }
                </header>
                <div class="fc-section-body">
                    <div class="fc-detail-grid" :hidden=$(editing.get())>
                        detail_field(label: "Code", <code>(&pool.code)</code>)
                        detail_value(label: "Name", value: Some(pool.name.clone()))
                        if let Some(description) = pool.description.clone().filter(|d| !d.is_empty()) {
                            detail_value(label: "Description", value: Some(description), span: true)
                        }
                        detail_field(label: "Rate Limit",
                            match pool.rate_limit {
                                Some(n) => <span>(format!("{n} / minute"))</span>,
                                None => <span>"Unlimited (concurrency-only)"</span>,
                            }
                        )
                        detail_value(label: "Concurrency", value: Some(pool.concurrency.to_string()))
                        detail_value(label: "Client Scope", value: Some(scope))
                        detail_field(label: "Status",
                            tag(label: pool.status.as_str(), severity: status_severity(pool.status))
                        )
                        detail_field(label: "Created", <time datetime=(created_iso) data-local="">(created_utc)</time>)
                        detail_field(label: "Updated", <time datetime=(updated_iso) data-local="">(updated_utc)</time>)
                    </div>
                    if editable {
                        <form id=(form_id) method="post" action=(format!("{base}/update")) class="fc-form-grid" data-dirty-form="" data-dirty-key=(&pool.id) :hidden=$(!editing.get())>
                            form_field(label: "Name", for_id: "pool-name", span: true,
                                <input id="pool-name" name="name" class="fc-input" value=(&pool.name)>
                            )
                            form_field(label: "Description", for_id: "pool-description", span: true,
                                <textarea id="pool-description" name="description" class="fc-input" rows="3">(pool.description.clone().unwrap_or_default())</textarea>
                            )
                            form_field(label: "Rate Limit (per minute)", for_id: "pool-rate-limit", help: Some("Leave blank to run on concurrency only.".to_owned()),
                                <input id="pool-rate-limit" name="rate_limit" type="number" min="1" step="1" class="fc-input" placeholder="Unlimited" value=(pool.rate_limit.map(|n| n.to_string()).unwrap_or_default())>
                            )
                            form_field(label: "Concurrency", for_id: "pool-concurrency",
                                <input id="pool-concurrency" name="concurrency" type="number" min="1" step="1" class="fc-input" value=(pool.concurrency.to_string())>
                            )
                        </form>
                    }
                </div>
            </section>

            if show_actions {
                <section class="fc-form-section" :hidden=$(editing.get())>
                    <header class="fc-section-header">
                        <h3 class="fc-section-title">"Actions"</h3>
                    </header>
                    <div class="fc-danger-actions">
                        if !active && can_activate {
                            <div class="fc-danger-item">
                                <div>
                                    <strong>"Activate Pool"</strong>
                                    <p>"Enable this pool for processing dispatch jobs."</p>
                                </div>
                                <button type="button" class="fc-btn border-[#16a34a] bg-transparent text-[#16a34a] hover:bg-[#f0fdf4]" commandfor="pool-activate" command="show-modal">
                                    icon(data: iconify_icon!("lucide:circle-check"), size: Length::rem(1.0))
                                    "Activate"
                                </button>
                            </div>
                            confirm_dialog(
                                id: "pool-activate",
                                action: format!("{base}/activate"),
                                title: "Activate Pool",
                                message: "Activate this dispatch pool?",
                                confirm_label: "Activate",
                            )
                        }
                        if active && can_suspend {
                            <div class="fc-danger-item">
                                <div>
                                    <strong>"Suspend Pool"</strong>
                                    <p>"Temporarily stop processing jobs in this pool."</p>
                                </div>
                                <button type="button" class=(Btn::WarnOutline) commandfor="pool-suspend" command="show-modal">
                                    icon(data: iconify_icon!("lucide:pause"), size: Length::rem(1.0))
                                    "Suspend"
                                </button>
                            </div>
                            confirm_dialog(
                                id: "pool-suspend",
                                action: format!("{base}/suspend"),
                                title: "Suspend Pool",
                                message: "Suspend this dispatch pool? Jobs will not be processed.",
                                confirm_label: "Suspend",
                                warn: true,
                            )
                        }
                        if deletable {
                            <div class="fc-danger-item">
                                <div>
                                    <strong>"Delete Pool"</strong>
                                    <p>"Permanently deletes this pool. Cannot be undone."</p>
                                </div>
                                <button type="button" class=(Btn::DangerOutline) commandfor="pool-delete" command="show-modal">
                                    icon(data: iconify_icon!("lucide:trash-2"), size: Length::rem(1.0))
                                    "Delete"
                                </button>
                            </div>
                            confirm_dialog(
                                id: "pool-delete",
                                action: format!("{base}/delete"),
                                title: "Delete Pool",
                                message: "Delete this dispatch pool? This permanently deletes it and cannot be undone.",
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

#[derive(Clone, Deserialize)]
struct CreateForm {
    #[serde(default)]
    code: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    rate_limit: String,
    #[serde(default)]
    concurrency: String,
    #[serde(default)]
    anchor_level: Option<String>,
    #[serde(default)]
    client_id: String,
}

impl Default for CreateForm {
    fn default() -> Self {
        Self {
            code: String::new(),
            name: String::new(),
            description: String::new(),
            rate_limit: String::new(),
            // The drawer's default.
            concurrency: "10".to_owned(),
            anchor_level: None,
            client_id: String::new(),
        }
    }
}

#[derive(Clone, Default)]
struct CreateState {
    form: CreateForm,
    error: Option<String>,
    /// (id, name, identifier) of the clients the caller may list.
    clients: Vec<(String, String, String)>,
}

/// `DispatchPoolCreateDrawer.vue`. GET shows it; POST creates through
/// `CreateDispatchPoolUseCase` (what `POST /api/dispatch-pools` runs), then
/// opens the new pool's drawer, or shows the form again with the error.
#[page([GET, POST] "/ui/(app)/dispatch-pools/new")]
async fn create_pool(cx: &Cx, form: Option<Form<CreateForm>>) -> Result<impl View> {
    let auth = auth(cx)?;
    permit(checks::can_write_dispatch_pools(auth))?;
    let deps = crate::deps(cx);
    let mut state = CreateState::default();

    if method(cx) == Method::POST {
        let form = form.map(|Form(f)| f).unwrap_or_default();
        let code = form.code.trim();
        let client_id = if form.anchor_level.is_some() {
            None
        } else {
            Some(form.client_id.trim().to_owned()).filter(|c| !c.is_empty())
        };
        let counts = parse_count(&form.rate_limit, "Rate limit").and_then(|rate| {
            parse_count(&form.concurrency, "Concurrency").and_then(|c| {
                c.map(|c| (rate, c))
                    .ok_or_else(|| PlatformError::validation("Concurrency is required"))
            })
        });
        let outcome = if !(2..=100).contains(&code.len()) || !valid_code(code) {
            Err(PlatformError::validation(
                "Code: 2-100 characters, lowercase letters, numbers, hyphens only, starting with a letter",
            ))
        } else if form.name.trim().is_empty() || form.name.len() > 255 {
            Err(PlatformError::validation(
                "Name is required (at most 255 characters)",
            ))
        } else if let Err(e) = counts {
            Err(e)
        } else if let Err(e) = ensure_can_create(auth, client_id.as_deref()) {
            Err(e)
        } else {
            let (rate_limit, concurrency) = counts.unwrap_or_default();
            CreateDispatchPoolUseCase::new(
                deps.dispatch_pool_repo.clone(),
                deps.unit_of_work.clone(),
            )
            .run(
                CreateDispatchPoolCommand {
                    code: code.to_owned(),
                    name: form.name.clone(),
                    description: Some(form.description.clone()).filter(|d| !d.is_empty()),
                    client_id,
                    rate_limit: rate_limit.map(|r| r as i32),
                    concurrency: Some(concurrency as i32),
                    caller: None,
                },
                ExecutionContext::create(auth.principal_id.clone()),
            )
            .await
            .into_result()
            .map(|event| event.pool_id)
            .map_err(PlatformError::from)
        };
        match outcome {
            Ok(id) => {
                set_flash(cx, FlashKind::Success, "Dispatch pool created");
                return Err(see_other(detail_href(&id)).into());
            }
            Err(e) if e.status_code().is_client_error() => state.error = Some(e.to_string()),
            Err(e) => {
                tracing::error!(error = %e, "fc-web: create dispatch pool failed");
                state.error = Some("Failed to create dispatch pool".to_owned());
            }
        }
        state.form = form;
    }

    // The client picker lists what the caller's own read allows (an empty
    // list otherwise, as a refused search leaves it).
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
    Ok(view! { pool_list(open_id: String::new(), create: Some(state)) })
}

#[component]
async fn create_drawer(state: CreateState, close_href: String) -> Result<impl View> {
    let f = state.form;
    let h3 = "mb-4 text-[14px] font-semibold uppercase tracking-[0.05em] text-[#475569]";
    Ok(view! {
        <aside class="fc-drawer" role="complementary" aria-label="Create dispatch pool" data-drawer="">
            <a href=(&close_href) class="fc-icon-btn absolute top-[1.1rem] right-[1.1rem] z-10" aria-label="Close" data-drawer-close="">
                icon(data: iconify_icon!("lucide:x"), size: Length::rem(1.2))
            </a>
            drawer_header(title: "Create Dispatch Pool", subtitle: Some("Configure a new pool for dispatch jobs".to_owned()))
            <form id="pool-create-form" method="post" action=(format!("{LIST}/new")) class="fc-drawer-body">
                <div class="mb-8">
                    <h3 class=(h3)>"Basic Information"</h3>
                    <div class="flex flex-col gap-5">
                        form_field(label: "Code", for_id: "pool-new-code", required: true, help: Some("Unique identifier for this pool (2-100 characters)".to_owned()),
                            <input id="pool-new-code" name="code" class="fc-input" value=(f.code.clone()) placeholder="pool-code" required="" minlength="2" maxlength="100" pattern="[a-z][a-z0-9\\-]*" title="Lowercase letters, numbers, hyphens only. Must start with a letter.">
                        )
                        form_field(label: "Name", for_id: "pool-new-name", required: true, help: Some("At most 255 characters".to_owned()),
                            <input id="pool-new-name" name="name" class="fc-input" value=(f.name.clone()) placeholder="Pool display name" required="" maxlength="255">
                        )
                        form_field(label: "Description", for_id: "pool-new-description",
                            <textarea id="pool-new-description" name="description" class="fc-input" rows="3" placeholder="Optional description...">(f.description.clone())</textarea>
                        )
                    </div>
                </div>

                <div class="mb-8">
                    <h3 class=(h3)>"Rate Limiting"</h3>
                    <div class="grid grid-cols-2 gap-4 max-[640px]:grid-cols-1">
                        form_field(label: "Rate Limit (per minute)", for_id: "pool-new-rate", help: Some("Optional. Leave blank to run on concurrency only.".to_owned()),
                            <input id="pool-new-rate" name="rate_limit" type="number" min="1" step="1" class="fc-input" placeholder="Unlimited" value=(f.rate_limit.clone())>
                        )
                        form_field(label: "Concurrency", for_id: "pool-new-concurrency", required: true, help: Some("Maximum concurrent dispatches".to_owned()),
                            <input id="pool-new-concurrency" name="concurrency" type="number" min="1" step="1" class="fc-input" required="" value=(f.concurrency.clone())>
                        )
                    </div>
                </div>

                <div class="group mb-8">
                    <h3 class=(h3)>"Scope"</h3>
                    <div class="mb-5">
                        <label class="fc-checkbox-row" for="pool-new-anchor">
                            <input id="pool-new-anchor" name="anchor_level" type="checkbox" value="true" checked=(f.anchor_level.is_some())>
                            "Anchor-level pool (not client-scoped)"
                        </label>
                        <small class="fc-field-help">"Anchor-level pools are for dispatch jobs that are not scoped to a specific client."</small>
                    </div>
                    // Hidden while the anchor-level box is ticked, as the
                    // drawer's `v-if="!isAnchorLevel"`.
                    <div class="group-has-[#pool-new-anchor:checked]:hidden">
                        form_field(label: "Client", for_id: "pool-new-client", help: Some("If specified, this pool will only be used for jobs scoped to this client.".to_owned()),
                            <select id="pool-new-client" name="client_id" class="fc-select">
                                <option value="" selected=(f.client_id.is_empty())>"Search for a client (optional)"</option>
                                for (id, name, identifier) in state.clients {
                                    <option value=(&id) selected=(f.client_id == id)>(format!("{name} ({identifier})"))</option>
                                }
                            </select>
                        )
                    </div>
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
                <button type="submit" form="pool-create-form" class=(Btn::Primary)>
                    icon(data: iconify_icon!("lucide:check"), size: Length::rem(1.0))
                    "Create Pool"
                </button>
            </footer>
        </aside>
    })
}

// -------------------------------------------------------------- writes

/// Load the pool a write targets.
async fn load_pool(cx: &Cx) -> Result<DispatchPool> {
    let id = path_param::<Id>(cx);
    crate::deps(cx)
        .dispatch_pool_repo
        .find_by_id(id)
        .await
        .map_err(platform_error)?
        .ok_or_not_found()
        .map_err(Into::into)
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
            tracing::error!(error = %e, "fc-web: dispatch pool write failed");
            set_flash(cx, FlashKind::Error, "Something went wrong. Try again.");
            Ok(see_other(retry))
        }
    }
}

#[derive(Deserialize)]
struct UpdateForm {
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    rate_limit: String,
    #[serde(default)]
    concurrency: String,
}

#[route(POST "/ui/(app)/dispatch-pools/{id}/update")]
async fn update(cx: &Cx, Form(form): Form<UpdateForm>) -> Result<SeeOther> {
    let auth = auth(cx)?;
    permit(checks::can_write_dispatch_pools(auth))?;
    let pool = load_pool(cx).await?;
    permit(ensure_modifiable(auth, &pool, "update"))?;
    let deps = crate::deps(cx);
    // As the drawer's save: `description || undefined`, `rateLimit ||
    // undefined`, so a blank field leaves the stored value.
    let outcome = match parse_count(&form.rate_limit, "Rate limit")
        .and_then(|rate| parse_count(&form.concurrency, "Concurrency").map(|c| (rate, c)))
    {
        Err(e) => Err(e),
        Ok((rate_limit, concurrency)) => UpdateDispatchPoolUseCase::new(
            deps.dispatch_pool_repo.clone(),
            deps.unit_of_work.clone(),
        )
        .run(
            UpdateDispatchPoolCommand {
                id: pool.id.clone(),
                name: Some(form.name),
                description: Some(form.description).filter(|d| !d.is_empty()),
                rate_limit: rate_limit.map(|r| r as i32),
                concurrency: concurrency.map(|c| c as i32),
                caller: None,
            },
            ExecutionContext::create(auth.principal_id.clone()),
        )
        .await
        .into_result()
        .map(|_| ())
        .map_err(PlatformError::from),
    };
    let base = detail_href(&pool.id);
    let retry = format!("{base}?edit=true");
    finish(cx, outcome, "Pool updated", base, retry)
}

#[route(POST "/ui/(app)/dispatch-pools/{id}/suspend")]
async fn suspend(cx: &Cx) -> Result<SeeOther> {
    let auth = auth(cx)?;
    permit(checks::can_write_dispatch_pools(auth))?;
    let pool = load_pool(cx).await?;
    permit(ensure_modifiable(auth, &pool, "suspend"))?;
    let deps = crate::deps(cx);
    let outcome =
        SuspendDispatchPoolUseCase::new(deps.dispatch_pool_repo.clone(), deps.unit_of_work.clone())
            .run(
                SuspendDispatchPoolCommand {
                    id: pool.id.clone(),
                },
                ExecutionContext::create(auth.principal_id.clone()),
            )
            .await
            .into_result()
            .map(|_| ())
            .map_err(PlatformError::from);
    let base = detail_href(&pool.id);
    finish(cx, outcome, "Pool suspended", base.clone(), base)
}

#[route(POST "/ui/(app)/dispatch-pools/{id}/activate")]
async fn activate(cx: &Cx) -> Result<SeeOther> {
    let auth = auth(cx)?;
    permit(checks::can_write_dispatch_pools(auth))?;
    let pool = load_pool(cx).await?;
    permit(ensure_modifiable(auth, &pool, "activate"))?;
    let deps = crate::deps(cx);
    let outcome = ActivateDispatchPoolUseCase::new(
        deps.dispatch_pool_repo.clone(),
        deps.unit_of_work.clone(),
    )
    .run(
        ActivateDispatchPoolCommand {
            id: pool.id.clone(),
        },
        ExecutionContext::create(auth.principal_id.clone()),
    )
    .await
    .into_result()
    .map(|_| ())
    .map_err(PlatformError::from);
    let base = detail_href(&pool.id);
    finish(cx, outcome, "Pool activated", base.clone(), base)
}

#[route(POST "/ui/(app)/dispatch-pools/{id}/delete")]
async fn delete(cx: &Cx) -> Result<SeeOther> {
    let auth = auth(cx)?;
    permit(checks::require_anchor(auth))?;
    permit(checks::can_delete_dispatch_pools(auth))?;
    let pool = load_pool(cx).await?;
    let deps = crate::deps(cx);
    let outcome =
        DeleteDispatchPoolUseCase::new(deps.dispatch_pool_repo.clone(), deps.unit_of_work.clone())
            .run(
                DeleteDispatchPoolCommand {
                    id: pool.id.clone(),
                },
                ExecutionContext::create(auth.principal_id.clone()),
            )
            .await
            .into_result()
            .map(|_| ())
            .map_err(PlatformError::from);
    finish(
        cx,
        outcome,
        "Pool deleted",
        LIST.to_owned(),
        detail_href(&pool.id),
    )
}
