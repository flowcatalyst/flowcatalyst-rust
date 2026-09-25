//! `/ui/subscriptions`: the SPA's `SubscriptionListPage.vue` with its
//! drawers, `SubscriptionDetailDrawer.vue` (`/ui/subscriptions/{id}`,
//! `?edit=true` opens it editing) and `SubscriptionCreateDrawer.vue`
//! (`/ui/subscriptions/new`). The pattern is `event_types.rs`'s.
//!
//! Reads go to the repository, as the `/api/subscriptions` handlers do, with
//! the same access rules (`subscription::access`). Writes run the handlers'
//! use cases. Two deliberate differences from those handlers, both towards
//! Go (the SPA's reference):
//!
//! - The list shows paused subscriptions too. `GET /api/subscriptions`
//!   without a client filter reads `find_active`, so the SPA's own Status
//!   filter can never show a paused one.
//! - Client identifier, pool code, source and spec versions come from the
//!   stored subscription; the API response blanks them.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use fc_platform::dispatch_job::entity::parse_dispatch_mode;
use fc_platform::subscription::access::{
    ensure_can_create, ensure_modifiable, ensure_visible, is_listed,
};
use fc_platform::subscription::entity::{DispatchMode, SubscriptionStatus};
use fc_platform::subscription::operations::{
    CreateSubscriptionCommand, CreateSubscriptionUseCase, DeleteSubscriptionCommand,
    DeleteSubscriptionUseCase, EventTypeBindingInput, PauseSubscriptionCommand,
    PauseSubscriptionUseCase, ResumeSubscriptionCommand, ResumeSubscriptionUseCase,
    UpdateSubscriptionCommand, UpdateSubscriptionUseCase,
};
use fc_platform::usecase::UseCase;
use fc_platform::{AuthContext, ExecutionContext, PlatformError, Subscription, checks};
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
    drawer_header, empty_state, filter_select, form_field, list_query, page_header, paginator,
    set_flash, table_toolbar, tag,
};

path_param!(id);

const LIST: &str = "/ui/subscriptions";
const FORM_ID: &str = "sub-list";

fn detail_href(id: &str) -> String {
    format!("{LIST}/{id}")
}

/// `getStatusSeverity`.
fn status_severity(status: SubscriptionStatus) -> Severity {
    match status {
        SubscriptionStatus::Active => Severity::Success,
        SubscriptionStatus::Paused => Severity::Warn,
    }
}

/// `getModeLabel`.
fn mode_label(mode: DispatchMode) -> &'static str {
    match mode {
        DispatchMode::Immediate => "Immediate",
        DispatchMode::NextOnError => "Next on Error",
        DispatchMode::BlockOnError => "Block on Error",
    }
}

const MODES: [DispatchMode; 3] = [
    DispatchMode::Immediate,
    DispatchMode::NextOnError,
    DispatchMode::BlockOnError,
];

/// A date in the viewer's locale (`toLocaleDateString()`); `ui.js`
/// rewrites the UTC fallback.
#[component]
async fn local_date(at: DateTime<Utc>) -> Result<impl View> {
    Ok(view! {
        <time datetime=(at.to_rfc3339()) data-local="date">(at.format("%Y-%m-%d UTC").to_string())</time>
    })
}

/// Fill in the pool code and client identifier where the stored row has
/// none (the create use case doesn't denormalise them), as Go's responses
/// carry them. Two batch lookups, whatever the number of subscriptions.
async fn enrich(cx: &Cx, subs: &mut [Subscription]) -> Result<()> {
    let deps = crate::deps(cx);
    let pool_ids: Vec<String> = subs
        .iter()
        .filter(|s| s.dispatch_pool_code.is_none())
        .filter_map(|s| s.dispatch_pool_id.clone())
        .collect();
    let client_ids: Vec<String> = subs
        .iter()
        .filter(|s| s.client_identifier.is_none())
        .filter_map(|s| s.client_id.clone())
        .collect();
    let (pools, clients) = tokio::try_join!(
        async {
            if pool_ids.is_empty() {
                Ok(Vec::new())
            } else {
                deps.dispatch_pool_repo.find_by_ids(&pool_ids).await
            }
        },
        async {
            if client_ids.is_empty() {
                Ok(Vec::new())
            } else {
                deps.client_repo.find_by_ids(&client_ids).await
            }
        },
    )
    .map_err(platform_error)?;
    let pools: std::collections::HashMap<String, String> =
        pools.into_iter().map(|p| (p.id, p.code)).collect();
    let clients: std::collections::HashMap<String, String> =
        clients.into_iter().map(|c| (c.id, c.identifier)).collect();
    for s in subs.iter_mut() {
        if s.dispatch_pool_code.is_none() {
            s.dispatch_pool_code = s
                .dispatch_pool_id
                .as_ref()
                .and_then(|id| pools.get(id).cloned());
        }
        if s.client_identifier.is_none() {
            s.client_identifier = s.client_id.as_ref().and_then(|id| clients.get(id).cloned());
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- list

#[query_params(error = bad_request)]
#[derive(Clone, Default)]
struct ListQuery {
    q: Option<String>,
    app: Option<String>,
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
                ("app", Self::get(&self.app)),
                ("status", Self::get(&self.status)),
                ("rows", &rows),
                ("page", &page),
            ],
        )
    }
}

#[page("/ui/(app)/subscriptions")]
async fn subscriptions(cx: &Cx) -> Result<impl View> {
    permit(checks::can_read_subscriptions(auth(cx)?))?;
    Ok(view! { subscription_list(open_id: String::new(), create: None) })
}

#[page("/ui/(app)/subscriptions/{id}")]
async fn subscription_detail(cx: &Cx) -> Result<impl View> {
    permit(checks::can_read_subscriptions(auth(cx)?))?;
    let id = path_param::<Id>(cx).to_owned();
    Ok(view! { subscription_list(open_id: id, create: None) })
}

/// The list, with the detail drawer open on `open_id` (when not empty) or
/// the create drawer (when `create` is set).
#[component]
async fn subscription_list(
    cx: &Cx,
    open_id: String,
    create: Option<CreateState>,
) -> Result<impl View> {
    let auth = auth(cx)?;
    let can_create = checks::can_write_subscriptions(auth).is_ok();
    let query = query_params::<ListQuery>(cx)?;
    let search = ListQuery::get(&query.q).to_owned();
    let app = ListQuery::get(&query.app).to_owned();
    let status = ListQuery::get(&query.status).to_owned();

    let mut visible: Vec<Subscription> = crate::deps(cx)
        .subscription_repo
        .find_all()
        .await
        .map_err(platform_error)?
        .into_iter()
        .filter(|s| is_listed(auth, s))
        .collect();
    enrich(cx, &mut visible).await?;

    let mut applications: Vec<String> = visible
        .iter()
        .filter_map(|s| s.application_code.clone())
        .collect();
    applications.sort();
    applications.dedup();

    // globalFilterFields: code, name, connectionId, applicationCode,
    // clientIdentifier.
    let needle = search.to_lowercase();
    let rows: Vec<Subscription> = visible
        .into_iter()
        .filter(|s| app.is_empty() || s.application_code.as_deref() == Some(app.as_str()))
        .filter(|s| status.is_empty() || s.status.as_str() == status)
        .filter(|s| {
            needle.is_empty()
                || [
                    Some(&s.code),
                    Some(&s.name),
                    s.connection_id.as_ref(),
                    s.application_code.as_ref(),
                    s.client_identifier.as_ref(),
                ]
                .into_iter()
                .flatten()
                .any(|f| f.to_lowercase().contains(&needle))
        })
        .collect();
    let active_filters = [&app, &status].iter().filter(|f| !f.is_empty()).count();
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
        page_header(title: "Subscriptions", subtitle: "Manage event subscriptions and webhook routing",
            if can_create {
                <a href=(create_href) class=(Btn::Primary)>
                    icon(data: iconify_icon!("lucide:plus"), size: Length::rem(1.0))
                    "Create Subscription"
                </a>
            }
        )

        <div class="fc-card-flush">
            table_toolbar(
                form_id: FORM_ID,
                action: LIST,
                placeholder: "Search subscriptions...",
                search: Some(search.clone()),
                active_filter_count: active_filters,
                has_active_filters: has_active,
                show_filters: true,
                show_search: true,
                hidden: hidden,
                filter_select(name: "app", label: "Application", placeholder: "All applications", options: applications.into_iter().map(|a| (a.clone(), a)).collect(), selected: (!app.is_empty()).then(|| app.clone()))
                filter_select(
                    name: "status",
                    label: "Status",
                    placeholder: "All statuses",
                    options: vec![("ACTIVE".to_owned(), "Active".to_owned()), ("PAUSED".to_owned(), "Paused".to_owned())],
                    selected: (!status.is_empty()).then(|| status.clone()),
                )
            )
            if rows.is_empty() {
                empty_state(message: "No subscriptions found", clear_href: has_active.then(|| LIST.to_owned()))
            } else {
                <table class="fc-table fc-table-striped">
                    <thead>
                        <tr>
                            <th>"Code"</th>
                            <th>"Application"</th>
                            <th>"Name"</th>
                            <th>"Scope"</th>
                            <th>"Event Types"</th>
                            <th>"Pool"</th>
                            <th>"Mode"</th>
                            <th>"Status"</th>
                            <th>"Created"</th>
                            <th class="w-[80px]">"Actions"</th>
                        </tr>
                    </thead>
                    <tbody>
                        for s in &rows {
                            let id = s.id.clone();
                            let edit_id = s.id.clone();
                            let count = s.event_types.len();
                            <tr class="fc-row-link" @click=$(|_e: Event| { selected.set(id.clone()); editing.set(false) })>
                                <td>
                                    <a href=(detail_href(&s.id)) onclick="event.preventDefault()" class="no-underline">
                                        <code class="rounded bg-[#f1f5f9] px-2 py-0.5 text-[13px] text-[#1e293b]">(&s.code)</code>
                                    </a>
                                </td>
                                <td>
                                    match s.application_code.clone() {
                                        Some(code) => <code class="rounded bg-[#fef3c7] px-2 py-0.5 text-[12px] text-[#92400e]">(code)</code>,
                                        None => <span class="text-[#94a3b8]">"—"</span>,
                                    }
                                </td>
                                <td>(&s.name)</td>
                                <td><span class="text-[13px] text-[#64748b]">(s.client_identifier.clone().unwrap_or_else(|| "Anchor-level".to_owned()))</span></td>
                                <td><span class="text-[13px]">(format!("{count} event type{}", if count == 1 { "" } else { "s" }))</span></td>
                                <td>
                                    if let Some(pool) = s.dispatch_pool_code.clone() {
                                        <code class="rounded bg-[#e0f2fe] px-2 py-0.5 text-[12px] text-[#0369a1]">(pool)</code>
                                    }
                                </td>
                                <td><span class="text-[12px] text-[#64748b]">(mode_label(s.mode))</span></td>
                                <td>tag(label: s.status.as_str(), severity: status_severity(s.status))</td>
                                <td>local_date(at: s.created_at)</td>
                                <td>
                                    <a
                                        href=(format!("{}?edit=true", detail_href(&s.id)))
                                        class="fc-icon-btn"
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
            paginator(pager: pager, noun: "subscriptions", href: page_href, form_id: FORM_ID)
        </div>

        drawer_frame(selected: frame_selected, label: "Subscription", size: DrawerSize::Wide,
            subscription_drawer(id: $(selected.get()), editing: drawer_editing)
        )

        if let Some(create) = create {
            create_drawer(state: create, close_href: close_href)
        }
    })
}

// -------------------------------------------------------------- drawer

/// The drawer's content for one subscription. A shard has its own
/// endpoint, so it checks the caller itself; the path keeps it under the
/// layer.
#[shard("/ui/(app)/subscriptions/drawer")]
async fn subscription_drawer(cx: &Cx, id: String, editing: Signal<bool>) -> Result<impl View> {
    let auth = auth(cx)?;
    permit(checks::can_read_subscriptions(auth))?;
    let sub = if id.is_empty() {
        None
    } else {
        let sub = crate::deps(cx)
            .subscription_repo
            .find_by_id(&id)
            .await
            .map_err(platform_error)?
            .ok_or_not_found()?;
        permit(ensure_visible(auth, &sub))?;
        let mut one = [sub];
        enrich(cx, &mut one).await?;
        let [sub] = one;
        Some(sub)
    };

    Ok(view! {
        if let Some(sub) = sub {
            drawer_body(auth: auth.clone(), sub: sub, editing: editing)
        }
    })
}

#[component]
async fn drawer_body(
    auth: AuthContext,
    sub: Subscription,
    editing: Signal<bool>,
) -> Result<impl View> {
    let can_write = checks::can_write_subscriptions(&auth).is_ok();
    // Update and delete also need an anchor user for an anchor-level
    // subscription; pause and resume only visibility (the handlers' rules).
    let editable = can_write && ensure_modifiable(&auth, &sub, "modify").is_ok();
    let deletable = checks::can_delete_subscriptions(&auth).is_ok()
        && ensure_modifiable(&auth, &sub, "delete").is_ok();
    let active = sub.status == SubscriptionStatus::Active;
    let base = detail_href(&sub.id);
    let form_id = "sub-edit-form";
    let (start, discard) = (editing.clone(), editing.clone());
    let actions_editing = editing.clone();
    let seconds = |n: i32| Some(format!("{n} seconds"));
    let scope = sub
        .client_identifier
        .clone()
        .unwrap_or_else(|| "Anchor-level (no client)".to_owned());
    let event_count = sub.event_types.len();
    let mode = sub.mode;

    Ok(view! {
        drawer_header(title: sub.name.clone(), subtitle: Some(sub.code.clone()),
            tag(label: sub.status.as_str(), severity: status_severity(sub.status))
        )

        <div class="fc-drawer-body">
            <section class="fc-form-section">
                <header class="fc-section-header">
                    <h3 class="fc-section-title">"Subscription Details"</h3>
                    if editable {
                        <button type="button" class=(Btn::TextPrimary) :hidden=$(editing.get()) @click=$(|_e: Event| start.set(true))>
                            icon(data: iconify_icon!("lucide:pencil"), size: Length::rem(1.0))
                            "Edit"
                        </button>
                    }
                </header>
                <div class="fc-section-body">
                    <div class="fc-detail-grid" :hidden=$(editing.get())>
                        detail_field(label: "Code", <code>(&sub.code)</code>)
                        detail_value(label: "Name", value: Some(sub.name.clone()))
                        if let Some(description) = sub.description.clone().filter(|d| !d.is_empty()) {
                            detail_value(label: "Description", value: Some(description), span: true)
                        }
                        detail_value(label: "Client Scope", value: Some(scope))
                        detail_value(label: "Source", value: Some(sub.source.as_str().to_owned()))
                        detail_field(label: "Endpoint", span: true, <code class="break-all text-[13px]">(&sub.endpoint)</code>)
                        if let Some(conn) = sub.connection_id.clone().filter(|c| !c.is_empty()) {
                            detail_field(label: "Connection", span: true, <code>(conn)</code>)
                        }
                        detail_field(label: "Queue", <code>(sub.queue.clone().unwrap_or_default())</code>)
                        detail_field(label: "Dispatch Pool", <code>(sub.dispatch_pool_code.clone().unwrap_or_default())</code>)
                        detail_value(label: "Mode", value: Some(mode_label(sub.mode).to_owned()))
                        detail_value(label: "Max Age", value: seconds(sub.max_age_seconds))
                        detail_value(label: "Delay", value: seconds(sub.delay_seconds))
                        detail_value(label: "Timeout", value: seconds(sub.timeout_seconds))
                        detail_value(label: "Sequence", value: Some(sub.sequence.to_string()))
                        detail_field(label: "Status", tag(label: sub.status.as_str(), severity: status_severity(sub.status)))
                        detail_field(label: "Created", crate::ui::local_time(at: sub.created_at))
                        detail_field(label: "Updated", crate::ui::local_time(at: sub.updated_at))
                    </div>
                    if editable {
                        <form id=(form_id) method="post" action=(format!("{base}/update")) class="fc-form-grid" data-dirty-form="" data-dirty-key=(&sub.id) :hidden=$(!editing.get())>
                            form_field(label: "Name", for_id: "sub-name", span: true,
                                <input id="sub-name" name="name" class="fc-input" value=(&sub.name) required="">
                            )
                            form_field(label: "Description", for_id: "sub-description", span: true,
                                <textarea id="sub-description" name="description" class="fc-input" rows="3">(sub.description.clone().unwrap_or_default())</textarea>
                            )
                            form_field(label: "Endpoint URL", for_id: "sub-endpoint", span: true,
                                <input id="sub-endpoint" name="endpoint" type="url" class="fc-input" value=(&sub.endpoint) required="">
                            )
                            form_field(label: "Connection ID", for_id: "sub-connection",
                                <input id="sub-connection" name="connection_id" class="fc-input" value=(sub.connection_id.clone().unwrap_or_default())>
                            )
                            form_field(label: "Timeout (seconds)", for_id: "sub-timeout",
                                <input id="sub-timeout" name="timeout_seconds" type="number" min="1" class="fc-input" value=(sub.timeout_seconds.to_string())>
                            )
                            form_field(label: "Mode", for_id: "sub-mode",
                                <select id="sub-mode" name="mode" class="fc-select">
                                    for m in MODES {
                                        <option value=(m.as_str()) selected=(m == mode)>(mode_label(m))</option>
                                    }
                                </select>
                            )
                        </form>
                    }
                </div>
            </section>

            // The last visible section loses its rule while Actions is hidden.
            <section class="fc-form-section [&:has(+[hidden]:last-child)]:border-b-0 [&:has(+[hidden]:last-child)]:pb-0">
                <header class="fc-section-header">
                    <h3 class="fc-section-title">(format!("Event Types ({event_count})"))</h3>
                </header>
                <div class="fc-section-body">
                    <table class="fc-table fc-table-sm fc-table-striped">
                        <thead>
                            <tr>
                                <th>"Event Type Code"</th>
                                <th>"Spec Version"</th>
                            </tr>
                        </thead>
                        <tbody>
                            for binding in sub.event_types.clone() {
                                <tr>
                                    <td>(binding.event_type_code)</td>
                                    <td>(binding.spec_version.unwrap_or_default())</td>
                                </tr>
                            }
                            if sub.event_types.is_empty() {
                                <tr><td colspan="2" class="text-[#64748b]">"No event types configured"</td></tr>
                            }
                        </tbody>
                    </table>
                </div>
            </section>

            if can_write || deletable {
                <section class="fc-form-section" :hidden=$(actions_editing.get())>
                    <header class="fc-section-header">
                        <h3 class="fc-section-title">"Actions"</h3>
                    </header>
                    <div class="fc-danger-actions">
                        if can_write && active {
                            <div class="fc-danger-item">
                                <div>
                                    <strong>"Pause Subscription"</strong>
                                    <p>"Stop creating dispatch jobs for this subscription."</p>
                                </div>
                                <button type="button" class=(Btn::WarnOutline) commandfor="sub-pause" command="show-modal">
                                    icon(data: iconify_icon!("lucide:pause"), size: Length::rem(1.0))
                                    "Pause"
                                </button>
                            </div>
                            confirm_dialog(
                                id: "sub-pause",
                                action: format!("{base}/pause"),
                                title: "Pause Subscription",
                                message: "Pause this subscription? It will stop creating dispatch jobs.",
                                confirm_label: "Pause",
                                warn: true,
                            )
                        }
                        if can_write && !active {
                            <div class="fc-danger-item">
                                <div>
                                    <strong>"Resume Subscription"</strong>
                                    <p>"Re-enable dispatch job creation."</p>
                                </div>
                                <button type="button" class="fc-btn border-[#16a34a] bg-transparent text-[#16a34a] hover:bg-[#f0fdf4]" commandfor="sub-resume" command="show-modal">
                                    icon(data: iconify_icon!("lucide:play"), size: Length::rem(1.0))
                                    "Resume"
                                </button>
                            </div>
                            confirm_dialog(
                                id: "sub-resume",
                                action: format!("{base}/resume"),
                                title: "Resume Subscription",
                                message: "Resume this subscription?",
                                confirm_label: "Resume",
                            )
                        }
                        if deletable {
                            <div class="fc-danger-item">
                                <div>
                                    <strong>"Delete Subscription"</strong>
                                    <p>"Permanently delete this subscription. Cannot be undone."</p>
                                </div>
                                <button type="button" class=(Btn::DangerOutline) commandfor="sub-delete" command="show-modal">
                                    icon(data: iconify_icon!("lucide:trash-2"), size: Length::rem(1.0))
                                    "Delete"
                                </button>
                            </div>
                            confirm_dialog(
                                id: "sub-delete",
                                action: format!("{base}/delete"),
                                title: "Delete Subscription",
                                message: "Delete this subscription? This action cannot be undone.",
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

/// The create form's values. Event types arrive as repeated `event_type`
/// fields, so the form is read as pairs.
#[derive(Clone, Default)]
struct CreateForm {
    code: String,
    name: String,
    description: String,
    event_types: Vec<String>,
    endpoint: String,
    connection_id: String,
    dispatch_pool_id: String,
    mode: String,
    timeout_seconds: String,
    client_id: String,
}

impl CreateForm {
    fn from_pairs(pairs: Vec<(String, String)>) -> Self {
        let mut f = Self::default();
        for (k, v) in pairs {
            match k.as_str() {
                "code" => f.code = v,
                "name" => f.name = v,
                "description" => f.description = v,
                "event_type" => f.event_types.push(v),
                "endpoint" => f.endpoint = v,
                "connection_id" => f.connection_id = v,
                "dispatch_pool_id" => f.dispatch_pool_id = v,
                "mode" => f.mode = v,
                "timeout_seconds" => f.timeout_seconds = v,
                "client_id" => f.client_id = v,
                _ => {}
            }
        }
        f
    }
}

/// An option in the event-type picker: code, name, current version, and
/// whether it is client-scoped.
#[derive(Clone)]
struct EventTypeOption {
    code: String,
    name: String,
    version: String,
    client_scoped: bool,
}

#[derive(Clone, Default)]
struct CreateState {
    form: CreateForm,
    error: Option<String>,
    event_types: Vec<EventTypeOption>,
    /// (id, name, code, detail)
    connections: Vec<(String, String, String)>,
    pools: Vec<(String, String, String)>,
    /// (id, label); anchor users may leave it empty (anchor-level).
    clients: Vec<(String, String)>,
    anchor: bool,
}

fn valid_code(code: &str) -> bool {
    let mut chars = code.chars();
    (2..=100).contains(&code.len())
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn valid_endpoint(url: &str) -> bool {
    (url.starts_with("http://") || url.starts_with("https://")) && url.len() > "https://".len()
}

/// `SubscriptionCreateDrawer.vue`. GET shows it; POST creates through
/// `CreateSubscriptionUseCase` (the `POST /api/subscriptions` handler's),
/// then opens the new subscription's drawer, or shows the form again with
/// the error.
#[page([GET, POST] "/ui/(app)/subscriptions/new")]
async fn create_subscription(
    cx: &Cx,
    form: Option<Form<Vec<(String, String)>>>,
) -> Result<impl View> {
    let auth = auth(cx)?;
    permit(checks::can_write_subscriptions(auth))?;
    let deps = crate::deps(cx);
    let mut state = CreateState {
        anchor: auth.is_anchor(),
        ..CreateState::default()
    };

    if method(cx) == Method::POST {
        let form = CreateForm::from_pairs(form.map(|Form(f)| f).unwrap_or_default());
        let code = form.code.trim();
        let endpoint = form.endpoint.trim();
        let client_id = Some(form.client_id.trim()).filter(|c| !c.is_empty());
        let timeout = form.timeout_seconds.trim();
        let outcome = if !valid_code(code) {
            Err(PlatformError::validation(
                "Lowercase letters, numbers, hyphens only. Must start with a letter (2-100 characters).",
            ))
        } else if form.name.trim().is_empty() || form.name.len() > 255 {
            Err(PlatformError::validation(
                "Name is required (at most 255 characters)",
            ))
        } else if form.event_types.is_empty() {
            Err(PlatformError::validation("Select at least one event type"))
        } else if !valid_endpoint(endpoint) {
            Err(PlatformError::validation(
                "Must be a valid HTTP or HTTPS URL",
            ))
        } else if form.dispatch_pool_id.trim().is_empty() {
            Err(PlatformError::validation("Select a dispatch pool"))
        } else if !timeout.is_empty() && timeout.parse::<u32>().is_ok_and(|t| t < 1) {
            Err(PlatformError::validation(
                "Timeout must be at least 1 second",
            ))
        } else if let Err(e) = ensure_can_create(auth, client_id) {
            Err(e)
        } else {
            CreateSubscriptionUseCase::new(
                deps.subscription_repo.clone(),
                deps.service_account_repo.clone(),
                deps.connection_repo.clone(),
                deps.unit_of_work.clone(),
            )
            .run(
                CreateSubscriptionCommand {
                    code: code.to_owned(),
                    name: form.name.trim().to_owned(),
                    description: Some(form.description.trim().to_owned()).filter(|d| !d.is_empty()),
                    client_id: client_id.map(str::to_owned),
                    endpoint: endpoint.to_owned(),
                    connection_id: Some(form.connection_id.trim().to_owned())
                        .filter(|c| !c.is_empty()),
                    event_types: form
                        .event_types
                        .iter()
                        .map(|code| EventTypeBindingInput {
                            event_type_code: code.clone(),
                            filter: None,
                        })
                        .collect(),
                    dispatch_pool_id: Some(form.dispatch_pool_id.trim().to_owned()),
                    service_account_id: None,
                    mode: Some(parse_dispatch_mode(Some(form.mode.as_str()))),
                    max_retries: None,
                    timeout_seconds: timeout.parse().ok(),
                    data_only: false,
                    caller: Some(auth.clone()),
                },
                ExecutionContext::from_auth(auth),
            )
            .await
            .into_result()
            .map(|event| event.subscription_id)
            .map_err(PlatformError::from)
        };
        match outcome {
            Ok(id) => {
                set_flash(cx, FlashKind::Success, "Subscription created");
                return Err(see_other(detail_href(&id)).into());
            }
            Err(e) if e.status_code().is_client_error() => state.error = Some(e.to_string()),
            Err(e) => {
                tracing::error!(error = %e, "fc-web: create subscription failed");
                state.error = Some("Failed to create subscription".to_owned());
            }
        }
        state.form = form;
    }

    // Lookups: event types with a CURRENT schema, active pools and
    // connections, and the clients the caller can reach.
    let (event_types, pools, connections, clients) = tokio::try_join!(
        deps.event_type_repo.find_all(),
        deps.dispatch_pool_repo.find_active(),
        deps.connection_repo.find_by_status("ACTIVE"),
        deps.client_repo.find_all(),
    )
    .map_err(platform_error)?;
    state.event_types = event_types
        .into_iter()
        .filter(|et| {
            et.client_id
                .as_deref()
                .is_none_or(|c| auth.can_access_client(c))
        })
        .filter_map(|et| {
            let version = et
                .spec_versions
                .iter()
                .find(|sv| {
                    sv.status == fc_platform::event_type::entity::SpecVersionStatus::Current
                })?
                .version
                .clone();
            Some(EventTypeOption {
                code: et.code,
                name: et.name,
                version,
                client_scoped: et.client_scoped,
            })
        })
        .collect();
    state.event_types.sort_by(|a, b| a.code.cmp(&b.code));
    state.pools = pools
        .into_iter()
        .map(|p| {
            let rate = p
                .rate_limit
                .map(|r| r.to_string())
                .unwrap_or_else(|| "—".to_owned());
            (
                p.id,
                p.name,
                format!("{} ({rate}/min, {} concurrent)", p.code, p.concurrency),
            )
        })
        .collect();
    state.connections = connections
        .into_iter()
        .map(|c| (c.id, c.name, c.code))
        .collect();
    state.clients = clients
        .into_iter()
        .filter(|c| auth.can_access_client(&c.id))
        .map(|c| (c.id, format!("{} ({})", c.name, c.identifier)))
        .collect();
    Ok(view! { subscription_list(open_id: String::new(), create: Some(state)) })
}

#[component]
async fn create_drawer(state: CreateState, close_href: String) -> Result<impl View> {
    let f = state.form;
    let mode = if f.mode.is_empty() {
        DispatchMode::NextOnError.as_str().to_owned()
    } else {
        f.mode.clone()
    };
    let timeout = if f.timeout_seconds.is_empty() {
        "30".to_owned()
    } else {
        f.timeout_seconds.clone()
    };
    Ok(view! {
        <aside class="fc-drawer" role="complementary" aria-label="Create subscription" data-drawer="">
            <a href=(&close_href) class="fc-icon-btn absolute top-[1.1rem] right-[1.1rem] z-10" aria-label="Close" data-drawer-close="">
                icon(data: iconify_icon!("lucide:x"), size: Length::rem(1.2))
            </a>
            drawer_header(title: "Create Subscription", subtitle: Some("Configure a new event subscription for webhook delivery".to_owned()))
            <form id="sub-create-form" method="post" action=(format!("{LIST}/new")) class="fc-drawer-body">
                <section class="fc-form-section">
                    <header class="fc-section-header"><h3 class="fc-section-title">"Basic Information"</h3></header>
                    <div class="flex flex-col gap-4">
                        form_field(label: "Code", for_id: "sub-new-code", required: true, help: Some("Unique identifier for this subscription (2-100 characters)".to_owned()),
                            <input id="sub-new-code" name="code" class="fc-input" value=(f.code.clone()) placeholder="subscription-code" required="" minlength="2" maxlength="100" pattern="[a-z][a-z0-9\\-]*" title="Lowercase letters, numbers, hyphens only. Must start with a letter.">
                        )
                        form_field(label: "Name", for_id: "sub-new-name", required: true, help: Some("At most 255 characters".to_owned()),
                            <input id="sub-new-name" name="name" class="fc-input" value=(f.name.clone()) placeholder="Subscription display name" required="" maxlength="255">
                        )
                        form_field(label: "Description", for_id: "sub-new-description",
                            <textarea id="sub-new-description" name="description" class="fc-input" rows="3" placeholder="Optional description...">(f.description.clone())</textarea>
                        )
                    </div>
                </section>

                <section class="fc-form-section">
                    <header class="fc-section-header"><h3 class="fc-section-title">"Event Types"</h3></header>
                    <div class="fc-form-field">
                        <span class="fc-field-label">"Event Types " <span class="fc-required">"*"</span></span>
                        <div class="max-h-64 overflow-y-auto rounded-[2px] border border-[#64748b] p-2">
                            for et in state.event_types {
                                <label class="flex cursor-pointer items-start gap-2 rounded px-1.5 py-1 hover:bg-[#f1f5f9]">
                                    <input type="checkbox" name="event_type" value=(&et.code) checked=(f.event_types.contains(&et.code)) class="mt-1 accent-[#059669]">
                                    <span class="flex flex-col">
                                        <span class="text-[14px] text-[#1e293b]">(&et.name)</span>
                                        <span class="font-mono text-[12px] text-[#64748b]">
                                            (format!("{} (v{})", et.code, et.version))
                                            if et.client_scoped { " · client-scoped" }
                                        </span>
                                    </span>
                                </label>
                            }
                        </div>
                        <small class="fc-field-help">"Select which event types this subscription will receive (event types with a current schema)."</small>
                    </div>
                </section>

                <section class="fc-form-section">
                    <header class="fc-section-header"><h3 class="fc-section-title">"Delivery Configuration"</h3></header>
                    <div class="flex flex-col gap-4">
                        form_field(label: "Endpoint URL", for_id: "sub-new-endpoint", required: true, help: Some("The webhook URL where events will be delivered".to_owned()),
                            <input id="sub-new-endpoint" name="endpoint" type="url" class="fc-input" value=(f.endpoint.clone()) placeholder="https://example.com/webhook" required="">
                        )
                        form_field(label: "Connection", for_id: "sub-new-connection", help: Some("The connection used for delivering events".to_owned()),
                            <select id="sub-new-connection" name="connection_id" class="fc-select">
                                <option value="" selected=(f.connection_id.is_empty())>"Select a connection"</option>
                                for (id, name, code) in state.connections {
                                    <option value=(&id) selected=(f.connection_id == id)>(format!("{name} — {code}"))</option>
                                }
                            </select>
                        )
                        form_field(label: "Dispatch Pool", for_id: "sub-new-pool", required: true, help: Some("Rate-limiting pool for this subscription's dispatch jobs".to_owned()),
                            <select id="sub-new-pool" name="dispatch_pool_id" class="fc-select" required="">
                                <option value="" selected=(f.dispatch_pool_id.is_empty())>"Select a dispatch pool"</option>
                                for (id, name, detail) in state.pools {
                                    <option value=(&id) selected=(f.dispatch_pool_id == id)>(format!("{name} — {detail}"))</option>
                                }
                            </select>
                        )
                        form_field(label: "Mode", for_id: "sub-new-mode", help: Some("How to handle dispatch failures".to_owned()),
                            <select id="sub-new-mode" name="mode" class="fc-select">
                                for m in MODES {
                                    <option value=(m.as_str()) selected=(m.as_str() == mode)>(mode_label(m))</option>
                                }
                            </select>
                        )
                    </div>
                </section>

                <section class="fc-form-section">
                    <header class="fc-section-header"><h3 class="fc-section-title">"Timing"</h3></header>
                    <div class="fc-form-grid">
                        form_field(label: "Timeout (seconds)", for_id: "sub-new-timeout", help: Some("Request timeout for webhook calls".to_owned()),
                            <input id="sub-new-timeout" name="timeout_seconds" type="number" min="1" class="fc-input" value=(timeout)>
                        )
                    </div>
                </section>

                <section class="fc-form-section">
                    <header class="fc-section-header"><h3 class="fc-section-title">"Scope"</h3></header>
                    form_field(
                        label: "Client",
                        for_id: "sub-new-client",
                        help: Some(if state.anchor {
                            "Leave empty for an anchor-level subscription, or select a specific client.".to_owned()
                        } else {
                            "The client this subscription belongs to.".to_owned()
                        }),
                        <select id="sub-new-client" name="client_id" class="fc-select" required=(!state.anchor)>
                            <option value="" selected=(f.client_id.is_empty())>(if state.anchor { "Anchor-level (no client)" } else { "Select a client" })</option>
                            for (id, label) in state.clients {
                                <option value=(&id) selected=(f.client_id == id)>(label)</option>
                            }
                        </select>
                    )
                </section>

                if let Some(error) = state.error {
                    <div class="fc-banner fc-banner-error" role="alert">(error)</div>
                }
            </form>
            <footer class="fc-drawer-footer">
                <a href=(&close_href) class=(Btn::Outline)>
                    icon(data: iconify_icon!("lucide:x"), size: Length::rem(1.0))
                    "Cancel"
                </a>
                <button type="submit" form="sub-create-form" class=(Btn::Primary)>
                    icon(data: iconify_icon!("lucide:check"), size: Length::rem(1.0))
                    "Create Subscription"
                </button>
            </footer>
        </aside>
    })
}

// -------------------------------------------------------------- writes

/// Load the subscription a write targets, after `check` (the handler's
/// permission check).
async fn load_for_write(cx: &Cx, check: fc_platform::Result<()>) -> Result<Subscription> {
    permit(check)?;
    let id = path_param::<Id>(cx);
    crate::deps(cx)
        .subscription_repo
        .find_by_id(id)
        .await
        .map_err(platform_error)?
        .ok_or_not_found()
        .map_err(Into::into)
}

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
            tracing::error!(error = %e, "fc-web: subscription write failed");
            set_flash(cx, FlashKind::Error, "Something went wrong. Try again.");
            Ok(see_other(retry))
        }
    }
}

#[derive(Deserialize)]
struct UpdateForm {
    name: String,
    #[serde(default)]
    description: String,
    endpoint: String,
    #[serde(default)]
    connection_id: String,
    #[serde(default)]
    timeout_seconds: String,
    #[serde(default)]
    mode: String,
}

/// Save the drawer's edit form: `UpdateSubscriptionUseCase`, with the
/// fields the SPA sends (`description || undefined`, `timeoutSeconds ||
/// undefined`). `mode` goes through the use case as the SPA sends it; the
/// `PUT /api/subscriptions/{id}` handler drops it (a parity gap).
#[route(POST "/ui/(app)/subscriptions/{id}/update")]
async fn update(cx: &Cx, Form(form): Form<UpdateForm>) -> Result<SeeOther> {
    let auth = auth(cx)?;
    let sub = load_for_write(cx, checks::can_write_subscriptions(auth)).await?;
    permit(ensure_modifiable(auth, &sub, "modify"))?;
    let deps = crate::deps(cx);
    let outcome = UpdateSubscriptionUseCase::new(
        deps.subscription_repo.clone(),
        deps.service_account_repo.clone(),
        deps.connection_repo.clone(),
        deps.unit_of_work.clone(),
    )
    .run(
        UpdateSubscriptionCommand {
            subscription_id: sub.id.clone(),
            name: Some(form.name.trim().to_owned()),
            description: Some(form.description.trim().to_owned()).filter(|d| !d.is_empty()),
            endpoint: Some(form.endpoint.trim().to_owned()),
            connection_id: Some(form.connection_id.trim().to_owned()),
            event_types: None,
            dispatch_pool_id: None,
            service_account_id: None,
            mode: Some(form.mode.as_str())
                .filter(|m| !m.is_empty())
                .map(|m| parse_dispatch_mode(Some(m))),
            max_retries: None,
            timeout_seconds: form.timeout_seconds.trim().parse().ok().filter(|t| *t > 0),
            data_only: None,
            caller: Some(auth.clone()),
        },
        ExecutionContext::from_auth(auth),
    )
    .await
    .into_result()
    .map(|_| ())
    .map_err(PlatformError::from);
    let base = detail_href(&sub.id);
    let retry = format!("{base}?edit=true");
    finish(cx, outcome, "Subscription updated", base, retry)
}

#[route(POST "/ui/(app)/subscriptions/{id}/pause")]
async fn pause(cx: &Cx) -> Result<SeeOther> {
    let auth = auth(cx)?;
    let sub = load_for_write(cx, checks::can_write_subscriptions(auth)).await?;
    permit(ensure_visible(auth, &sub))?;
    let deps = crate::deps(cx);
    let outcome =
        PauseSubscriptionUseCase::new(deps.subscription_repo.clone(), deps.unit_of_work.clone())
            .run(
                PauseSubscriptionCommand {
                    subscription_id: sub.id.clone(),
                },
                ExecutionContext::from_auth(auth),
            )
            .await
            .into_result()
            .map(|_| ())
            .map_err(PlatformError::from);
    let base = detail_href(&sub.id);
    finish(cx, outcome, "Subscription paused", base.clone(), base)
}

#[route(POST "/ui/(app)/subscriptions/{id}/resume")]
async fn resume(cx: &Cx) -> Result<SeeOther> {
    let auth = auth(cx)?;
    let sub = load_for_write(cx, checks::can_write_subscriptions(auth)).await?;
    permit(ensure_visible(auth, &sub))?;
    let deps = crate::deps(cx);
    let outcome =
        ResumeSubscriptionUseCase::new(deps.subscription_repo.clone(), deps.unit_of_work.clone())
            .run(
                ResumeSubscriptionCommand {
                    subscription_id: sub.id.clone(),
                },
                ExecutionContext::from_auth(auth),
            )
            .await
            .into_result()
            .map(|_| ())
            .map_err(PlatformError::from);
    let base = detail_href(&sub.id);
    finish(cx, outcome, "Subscription resumed", base.clone(), base)
}

#[route(POST "/ui/(app)/subscriptions/{id}/delete")]
async fn delete(cx: &Cx) -> Result<SeeOther> {
    let auth = auth(cx)?;
    let sub = load_for_write(cx, checks::can_delete_subscriptions(auth)).await?;
    permit(ensure_modifiable(auth, &sub, "delete"))?;
    let deps = crate::deps(cx);
    let outcome =
        DeleteSubscriptionUseCase::new(deps.subscription_repo.clone(), deps.unit_of_work.clone())
            .run(
                DeleteSubscriptionCommand {
                    subscription_id: sub.id.clone(),
                },
                ExecutionContext::from_auth(auth),
            )
            .await
            .into_result()
            .map(|_| ())
            .map_err(PlatformError::from);
    finish(
        cx,
        outcome,
        "Subscription deleted",
        LIST.to_owned(),
        detail_href(&sub.id),
    )
}
