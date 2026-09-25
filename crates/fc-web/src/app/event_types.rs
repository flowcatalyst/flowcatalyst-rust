//! `/ui/event-types`: the SPA's `EventTypeListPage.vue` with its drawers,
//! `EventTypeDetailDrawer.vue` (`/ui/event-types/{id}`, `?edit=true` opens
//! it editing) and `EventTypeCreateDrawer.vue` (`/ui/event-types/create`).
//!
//! This is the worked example of the list + drawer pattern:
//!
//! - Every URL renders the list. `{id}` also opens the detail drawer, so a
//!   write redirects straight back to it and the drawer is linkable.
//! - The list owns two signals: `selected` (the open record's id, empty
//!   when closed) and `editing`. Clicking a row sets them; the drawer is a
//!   shard keyed by `selected`, so the list keeps its scroll position and
//!   stays clickable underneath (the drawer is non-modal).
//! - The drawer's read view and edit form are both rendered; `editing`
//!   toggles them in the browser. Save/Discard follow the form's dirty
//!   state (`ui.js`, `data-dirty-form`).
//! - Reads go to the repository, as the BFF does. Every write is a form
//!   POST to a `#[route]` that runs the BFF's use case, then redirects
//!   back with a flash message (Post/Redirect/Get). Creating is a page
//!   that re-renders the form with the error on failure.

use std::sync::Arc;

use fc_platform::event_type::access::{ensure_can_create, ensure_modifiable, ensure_visible};
use fc_platform::event_type::entity::{SchemaType, SpecVersionStatus};
use fc_platform::event_type::operations::{
    ArchiveEventTypeCommand, ArchiveEventTypeUseCase, CreateEventTypeCommand,
    CreateEventTypeUseCase, DeleteEventTypeCommand, DeleteEventTypeUseCase, DeprecateSchemaCommand,
    DeprecateSchemaUseCase, FinaliseSchemaCommand, FinaliseSchemaUseCase, SyncEventTypesUseCase,
    UpdateEventTypeCommand, UpdateEventTypeUseCase,
};
use fc_platform::shared::bff_event_types_api::platform_sync_command;
use fc_platform::usecase::UseCase;
use fc_platform::{
    AuthContext, EventType, EventTypeStatus, ExecutionContext, PlatformError, SpecVersion, checks,
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
use crate::ui::drawer::DrawerSize;
use crate::ui::{
    Btn, FlashKind, Pager, Severity, code_chips, confirm_dialog, detail_field, detail_value,
    drawer_frame, drawer_header, empty_state, filter_select, form_field, json_block, list_query,
    page_header, paginator, set_flash, table_toolbar, tag,
};

path_param!(id);

const LIST: &str = "/ui/event-types";
const FORM_ID: &str = "et-list";

fn detail_href(id: &str) -> String {
    format!("{LIST}/{id}")
}

fn status_severity(status: EventTypeStatus) -> Severity {
    match status {
        EventTypeStatus::Current => Severity::Success,
        EventTypeStatus::Archived => Severity::Secondary,
    }
}

/// `getSchemaStatusSeverity`.
fn schema_severity(status: SpecVersionStatus) -> Severity {
    match status {
        SpecVersionStatus::Current => Severity::Success,
        SpecVersionStatus::Finalising => Severity::Info,
        SpecVersionStatus::Deprecated => Severity::Warn,
    }
}

/// `formatSchemaType`.
fn schema_type_label(schema_type: SchemaType) -> &'static str {
    match schema_type {
        SchemaType::JsonSchema => "JSON Schema",
        SchemaType::Proto => "Protocol Buffers",
        SchemaType::Xsd => "XML Schema",
    }
}

/// `canArchive`: current, and every schema deprecated (or none).
fn can_archive(et: &EventType) -> bool {
    et.status == EventTypeStatus::Current
        && et
            .spec_versions
            .iter()
            .all(|sv| sv.status == SpecVersionStatus::Deprecated)
}

/// `canDelete`: archived, or current with only unfinished schemas (or none).
fn can_delete(et: &EventType) -> bool {
    et.status == EventTypeStatus::Archived
        || (et.status == EventTypeStatus::Current
            && et
                .spec_versions
                .iter()
                .all(|sv| sv.status == SpecVersionStatus::Finalising))
}

/// `isValidSegment` in the create drawer.
fn valid_segment(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.')
}

// ---------------------------------------------------------------- list

#[query_params(error = bad_request)]
#[derive(Clone, Default)]
struct ListQuery {
    q: Option<String>,
    application: Option<String>,
    subdomain: Option<String>,
    aggregate: Option<String>,
    status: Option<String>,
    page: Option<usize>,
    rows: Option<usize>,
    edit: Option<String>,
}

impl ListQuery {
    fn get(value: &Option<String>) -> &str {
        value.as_deref().map(str::trim).unwrap_or_default()
    }

    /// The list's URL state, without the page.
    fn pairs(&self) -> Vec<(&'static str, String)> {
        vec![
            ("q", Self::get(&self.q).to_owned()),
            ("application", Self::get(&self.application).to_owned()),
            ("subdomain", Self::get(&self.subdomain).to_owned()),
            ("aggregate", Self::get(&self.aggregate).to_owned()),
            ("status", Self::get(&self.status).to_owned()),
            ("rows", self.rows.map(|r| r.to_string()).unwrap_or_default()),
        ]
    }

    fn href(&self, path: &str, page: Option<usize>) -> String {
        let mut pairs = self.pairs();
        if let Some(p) = page.filter(|p| *p > 1) {
            pairs.push(("page", p.to_string()));
        }
        let refs: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        list_query(path, &refs)
    }
}

#[page("/ui/(app)/event-types")]
async fn event_types(cx: &Cx) -> Result<impl View> {
    permit(checks::can_read_event_types(auth(cx)?))?;
    Ok(view! { event_type_list(open_id: String::new(), create: None) })
}

#[page("/ui/(app)/event-types/{id}")]
async fn event_type_detail(cx: &Cx) -> Result<impl View> {
    permit(checks::can_read_event_types(auth(cx)?))?;
    let id = path_param::<Id>(cx).to_owned();
    Ok(view! { event_type_list(open_id: id, create: None) })
}

/// The list, with the detail drawer open on `open_id` (when not empty) or
/// the create drawer (when `create` is set).
#[component]
async fn event_type_list(
    cx: &Cx,
    open_id: String,
    create: Option<CreateState>,
) -> Result<impl View> {
    let auth = auth(cx)?;
    let can_write = checks::can_write_event_types(auth).is_ok();
    let query = query_params::<ListQuery>(cx)?;
    let search = ListQuery::get(&query.q).to_owned();
    let application = ListQuery::get(&query.application).to_owned();
    let subdomain = ListQuery::get(&query.subdomain).to_owned();
    let aggregate = ListQuery::get(&query.aggregate).to_owned();
    let status = ListQuery::get(&query.status).to_owned();

    let all = crate::deps(cx)
        .event_type_repo
        .find_all()
        .await
        .map_err(platform_error)?;
    // Client-owned types only for callers with access to the client (the
    // BFF list's rule).
    let visible: Vec<EventType> = all
        .into_iter()
        .filter(|et| {
            et.client_id
                .as_deref()
                .is_none_or(|c| auth.can_access_client(c))
        })
        .collect();

    // Facet options narrow with the facets above them, as the BFF's
    // filter endpoints do.
    let distinct = |f: &dyn Fn(&EventType) -> bool, g: &dyn Fn(&EventType) -> String| {
        let mut v: Vec<String> = visible.iter().filter(|et| f(et)).map(g).collect();
        v.sort();
        v.dedup();
        v.into_iter().map(|s| (s.clone(), s)).collect::<Vec<_>>()
    };
    let applications = distinct(&|_| true, &|et| et.application.clone());
    let subdomains = distinct(
        &|et| application.is_empty() || et.application == application,
        &|et| et.subdomain.clone(),
    );
    let aggregates = distinct(
        &|et| {
            (application.is_empty() || et.application == application)
                && (subdomain.is_empty() || et.subdomain == subdomain)
        },
        &|et| et.aggregate.clone(),
    );

    // The toolbar search matches the SPA's globalFilterFields.
    let needle = search.to_lowercase();
    let rows: Vec<EventType> = visible
        .into_iter()
        .filter(|et| application.is_empty() || et.application == application)
        .filter(|et| subdomain.is_empty() || et.subdomain == subdomain)
        .filter(|et| aggregate.is_empty() || et.aggregate == aggregate)
        .filter(|et| status.is_empty() || et.status.as_str() == status)
        .filter(|et| {
            needle.is_empty()
                || [
                    &et.code,
                    &et.name,
                    &et.event_name,
                    &et.application,
                    &et.subdomain,
                    &et.aggregate,
                ]
                .iter()
                .any(|f| f.to_lowercase().contains(&needle))
        })
        .collect();
    let active_filters = [&application, &subdomain, &aggregate, &status]
        .iter()
        .filter(|f| !f.is_empty())
        .count();
    let has_active = active_filters > 0 || !search.is_empty();
    let pager = Pager::new(rows.len(), query.page, query.rows);
    let rows = pager.slice(rows);

    // The drawer's state. `editing` starts from `?edit=true`.
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
    let create_href = q.href(&format!("{LIST}/create"), None);

    Ok(view! {
        page_header(title: "Event Types", subtitle: "Manage event type definitions and schemas",
            if can_write {
                <form method="post" action=(format!("{LIST}/sync-platform"))>
                    <button type="submit" class=(Btn::Secondary)>
                        icon(data: iconify_icon!("lucide:refresh-cw"), size: Length::rem(1.0))
                        "Sync Platform Events"
                    </button>
                </form>
                <a href=(create_href) class=(Btn::Primary)>
                    icon(data: iconify_icon!("lucide:plus"), size: Length::rem(1.0))
                    "Create Event Type"
                </a>
            }
        )

        <div class="fc-card-flush">
            table_toolbar(
                form_id: FORM_ID,
                action: LIST,
                placeholder: "Search event types...",
                search: Some(search.clone()),
                active_filter_count: active_filters,
                has_active_filters: has_active,
                show_filters: true,
                show_search: true,
                hidden: hidden,
                filter_select(name: "application", label: "Applications", placeholder: "All Applications", options: applications, selected: (!application.is_empty()).then(|| application.clone()))
                filter_select(name: "subdomain", label: "Subdomains", placeholder: "All Subdomains", options: subdomains, selected: (!subdomain.is_empty()).then(|| subdomain.clone()))
                filter_select(name: "aggregate", label: "Aggregates", placeholder: "All Aggregates", options: aggregates, selected: (!aggregate.is_empty()).then(|| aggregate.clone()))
                filter_select(
                    name: "status",
                    label: "Status",
                    placeholder: "All Statuses",
                    options: vec![("CURRENT".to_owned(), "Current".to_owned()), ("ARCHIVED".to_owned(), "Archived".to_owned())],
                    selected: (!status.is_empty()).then(|| status.clone()),
                )
            )
            if rows.is_empty() {
                empty_state(message: "No event types found", clear_href: has_active.then(|| LIST.to_owned()))
            } else {
                <table class="fc-table fc-table-caps">
                    <thead>
                        <tr>
                            <th class="w-[30%]">"Code"</th>
                            <th class="w-[20%]">"Name"</th>
                            <th class="w-[25%]">"Description"</th>
                            <th class="w-[10%]">"Schemas"</th>
                            <th class="w-[10%]">"Status"</th>
                            <th class="w-[5%]"></th>
                        </tr>
                    </thead>
                    <tbody>
                        for et in &rows {
                            let id = et.id.clone();
                            let edit_id = et.id.clone();
                            <tr class="fc-row-link" @click=$(|_e: Event| { selected.set(id.clone()); editing.set(false) })>
                                <td>
                                    // A real link for new tabs and no-JS; a plain
                                    // click opens the drawer in place.
                                    <a href=(detail_href(&et.id)) onclick="event.preventDefault()" class="no-underline">
                                        code_chips(code: et.code.clone())
                                    </a>
                                </td>
                                <td class="font-medium text-[#1e293b]">(&et.name)</td>
                                <td>
                                    <span class="block max-w-[250px] truncate text-[13px] text-[#64748b]" title=(et.description.clone().unwrap_or_default())>
                                        (et.description.clone().filter(|d| !d.is_empty()).unwrap_or_else(|| "—".to_owned()))
                                    </span>
                                </td>
                                <td>
                                    <div class="flex flex-wrap gap-1">
                                        for sv in &et.spec_versions {
                                            <span title=(sv.status.as_str())>tag(label: sv.version.clone(), severity: schema_severity(sv.status))</span>
                                        }
                                        if et.spec_versions.is_empty() {
                                            <span class="text-[12px] italic text-[#94a3b8]">"No schemas"</span>
                                        }
                                    </div>
                                </td>
                                <td>tag(label: et.status.as_str(), severity: status_severity(et.status))</td>
                                <td class="text-right">
                                    <a
                                        href=(format!("{}?edit=true", detail_href(&et.id)))
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
            paginator(pager: pager, noun: "event types", href: page_href, form_id: FORM_ID)
        </div>

        drawer_frame(selected: frame_selected, label: "Event type", size: DrawerSize::Wide,
            event_type_drawer(id: $(selected.get()), editing: drawer_editing)
        )

        if let Some(create) = create {
            create_drawer(state: create, close_href: close_href)
        }
    })
}

// -------------------------------------------------------------- drawer

/// The drawer's content for one event type. A shard has its own endpoint,
/// so it checks the caller itself; the path keeps it under the layer.
#[shard("/ui/(app)/event-types/drawer")]
async fn event_type_drawer(cx: &Cx, id: String, editing: Signal<bool>) -> Result<impl View> {
    let auth = auth(cx)?;
    permit(checks::can_read_event_types(auth))?;
    let et = if id.is_empty() {
        None
    } else {
        let et = crate::deps(cx)
            .event_type_repo
            .find_by_id(&id)
            .await
            .map_err(platform_error)?
            .ok_or_not_found()?;
        permit(ensure_visible(auth, &et))?;
        Some(et)
    };

    Ok(view! {
        if let Some(et) = et {
            drawer_body(auth: auth.clone(), et: et, editing: editing)
        }
    })
}

#[component]
async fn drawer_body(auth: AuthContext, et: EventType, editing: Signal<bool>) -> Result<impl View> {
    let can_write = checks::can_write_event_types(&auth).is_ok();
    let can_modify = can_write && ensure_modifiable(&auth, &et, "modify").is_ok();
    let current = et.status == EventTypeStatus::Current;
    let editable = can_modify && current;
    let base = detail_href(&et.id);
    let archivable = can_archive(&et);
    let deletable = can_delete(&et);
    let form_id = "et-edit-form";
    let add_schema_href = format!("/event-types/{}/add-schema", et.id);
    let (start, discard) = (editing.clone(), editing.clone());

    Ok(view! {
        drawer_header(title: et.name.clone(), subtitle: Some(et.code.clone()),
            tag(label: et.status.as_str(), severity: status_severity(et.status))
        )

        <div class="fc-drawer-body">
            <section class="fc-form-section">
                <header class="fc-section-header">
                    <h3 class="fc-section-title">"Event Type Details"</h3>
                    if editable {
                        <button type="button" class=(Btn::TextPrimary) :hidden=$(editing.get()) @click=$(|_e: Event| start.set(true))>
                            icon(data: iconify_icon!("lucide:pencil"), size: Length::rem(1.0))
                            "Edit"
                        </button>
                    }
                </header>
                <div class="fc-section-body">
                    <div class="fc-detail-grid" :hidden=$(editing.get())>
                        detail_field(label: "Code", span: true, code_chips(code: et.code.clone()))
                        detail_value(label: "Name", value: Some(et.name.clone()))
                        detail_value(label: "Description", value: et.description.clone())
                        detail_field(label: "Client Scoped",
                            if et.client_scoped {
                                tag(label: "Yes", severity: Severity::Info)
                            } else {
                                tag(label: "No", severity: Severity::Secondary)
                            }
                        )
                    </div>
                    if editable {
                        <form id=(form_id) method="post" action=(format!("{base}/update")) class="fc-form-grid" data-dirty-form="" data-dirty-key=(&et.id) :hidden=$(!editing.get())>
                            form_field(label: "Name", for_id: "et-name", span: true,
                                <input id="et-name" name="name" class="fc-input" value=(&et.name) required="" maxlength="100">
                            )
                            form_field(label: "Description", for_id: "et-description", span: true,
                                <textarea id="et-description" name="description" class="fc-input" rows="3">(et.description.clone().unwrap_or_default())</textarea>
                            )
                        </form>
                    }
                </div>
            </section>

            <section class="fc-form-section">
                <header class="fc-section-header">
                    <h3 class="fc-section-title">"Schema Versions"</h3>
                    if can_write && current {
                        <a href=(&add_schema_href) class=(Btn::TextPrimary)>
                            icon(data: iconify_icon!("lucide:plus"), size: Length::rem(1.0))
                            "Add Schema"
                        </a>
                    }
                </header>
                <div class="fc-section-body">
                    if et.spec_versions.is_empty() {
                        <div class="fc-empty">
                            icon(data: iconify_icon!("lucide:file"), size: Length::px(48.0))
                            <span>"No schema versions defined yet."</span>
                            if can_write && current {
                                <a href=(&add_schema_href) class=(Btn::Primary)>
                                    icon(data: iconify_icon!("lucide:plus"), size: Length::rem(1.0))
                                    "Add First Schema"
                                </a>
                            }
                        </div>
                    } else {
                        <table class="fc-table fc-table-sm">
                            <thead>
                                <tr>
                                    <th class="w-[15%]">"Version"</th>
                                    <th class="w-[20%]">"MIME Type"</th>
                                    <th class="w-[20%]">"Schema Type"</th>
                                    <th class="w-[15%]">"Status"</th>
                                    <th class="w-[30%]">"Actions"</th>
                                </tr>
                            </thead>
                            <tbody>
                                for sv in et.spec_versions.clone() {
                                    schema_row(base: base.clone(), code: et.code.clone(), sv: sv, can_write: can_write)
                                }
                            </tbody>
                        </table>
                    }
                </div>
            </section>

            if can_modify {
                <section class="fc-form-section fc-danger-zone">
                    <header class="fc-section-header">
                        <h3 class="fc-section-title">"Danger Zone"</h3>
                    </header>
                    <div class="fc-danger-actions">
                        if current {
                            <div class="fc-danger-item">
                                <div>
                                    <strong>"Archive Event Type"</strong>
                                    <p>"Requires all schemas to be deprecated first."</p>
                                </div>
                                <button type="button" class=(Btn::WarnOutline) disabled=(!archivable) commandfor="et-archive" command="show-modal">"Archive"</button>
                            </div>
                            confirm_dialog(
                                id: "et-archive",
                                action: format!("{base}/archive"),
                                title: "Archive Event Type",
                                message: "Archive this event type? No new events can be created for archived types.",
                                confirm_label: "Archive",
                                warn: true,
                            )
                        }
                        <div class="fc-danger-item">
                            <div>
                                <strong>"Delete Event Type"</strong>
                                <p>"Permanently delete this event type."</p>
                            </div>
                            <button type="button" class=(Btn::DangerOutline) disabled=(!deletable) commandfor="et-delete" command="show-modal">"Delete"</button>
                        </div>
                        confirm_dialog(
                            id: "et-delete",
                            action: format!("{base}/delete"),
                            title: "Delete Event Type",
                            message: "Delete this event type? This cannot be undone.",
                            confirm_label: "Delete",
                            danger: true,
                        )
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

/// One schema version: its row, its actions, and the dialogs they open.
#[component]
async fn schema_row(
    base: String,
    code: String,
    sv: SpecVersion,
    can_write: bool,
) -> Result<impl View> {
    let key = sv
        .version
        .replace(|c: char| !c.is_ascii_alphanumeric(), "-");
    let view_id = format!("schema-view-{key}");
    let finalise_id = format!("schema-finalise-{key}");
    let deprecate_id = format!("schema-deprecate-{key}");
    let fields = vec![("version".to_owned(), sv.version.clone())];
    let version = sv.version.clone();
    Ok(view! {
        <tr>
            <td class="font-mono font-medium">(&sv.version)</td>
            <td><code class="fc-mono-chip text-[13px]">(&sv.mime_type)</code></td>
            <td>(schema_type_label(sv.schema_type))</td>
            <td>tag(label: sv.status.as_str(), severity: schema_severity(sv.status))</td>
            <td>
                <div class="flex items-center gap-1">
                    if sv.schema_content.is_some() {
                        <button type="button" class="fc-icon-btn text-[#059669]" title="View Schema" commandfor=(&view_id) command="show-modal">
                            icon(data: iconify_icon!("lucide:eye"), size: Length::rem(1.0))
                        </button>
                    }
                    if can_write && sv.status == SpecVersionStatus::Finalising {
                        <button type="button" class="fc-icon-btn text-[#16a34a]" title="Finalise" commandfor=(&finalise_id) command="show-modal">
                            icon(data: iconify_icon!("lucide:check"), size: Length::rem(1.0))
                        </button>
                    }
                    if can_write && sv.status == SpecVersionStatus::Current {
                        <button type="button" class="fc-icon-btn text-[#ea580c]" title="Deprecate" commandfor=(&deprecate_id) command="show-modal">
                            icon(data: iconify_icon!("lucide:ban"), size: Length::rem(1.0))
                        </button>
                    }
                </div>

                if let Some(schema) = sv.schema_content.clone() {
                    <dialog id=(&view_id) class="fc-dialog w-[48rem] max-w-[calc(100vw-2rem)]" closedby="any" aria-label=(format!("Schema {version}"))>
                        <div class="fc-dialog-header">
                            <div class="min-w-0">
                                <span>(format!("Schema v{version}"))</span>
                                <p class="text-[13px] font-normal text-[#64748b]">(&code)</p>
                            </div>
                            <button type="button" class="fc-icon-btn" aria-label="Close" commandfor=(&view_id) command="close">
                                icon(data: iconify_icon!("lucide:x"), size: Length::rem(1.1))
                            </button>
                        </div>
                        <div class="fc-dialog-body pb-6">
                            json_block(value: schema)
                        </div>
                    </dialog>
                }
                if can_write && sv.status == SpecVersionStatus::Finalising {
                    confirm_dialog(
                        id: finalise_id.clone(),
                        action: format!("{base}/schemas/finalise"),
                        title: "Finalise Schema",
                        message: format!("Finalise schema version {version}? This makes it the current version."),
                        confirm_label: "Finalise",
                        fields: fields.clone(),
                    )
                }
                if can_write && sv.status == SpecVersionStatus::Current {
                    confirm_dialog(
                        id: deprecate_id.clone(),
                        action: format!("{base}/schemas/deprecate"),
                        title: "Deprecate Schema",
                        message: format!("Deprecate schema version {version}?"),
                        confirm_label: "Deprecate",
                        warn: true,
                        fields: fields.clone(),
                    )
                }
            </td>
        </tr>
    })
}

// -------------------------------------------------------------- create

#[derive(Clone, Default, Deserialize)]
struct CreateForm {
    #[serde(default)]
    application: String,
    #[serde(default)]
    subdomain: String,
    #[serde(default)]
    aggregate: String,
    #[serde(default)]
    event: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
}

/// What the create drawer shows: the values typed so far, the error from
/// the last attempt, and the application codes to suggest.
#[derive(Clone, Default)]
struct CreateState {
    form: CreateForm,
    error: Option<String>,
    app_codes: Vec<String>,
}

/// `EventTypeCreateDrawer.vue`. GET shows it; POST creates through
/// `CreateEventTypeUseCase` (the BFF's create), then opens the new type's
/// drawer, or shows the form again with the error.
#[page([GET, POST] "/ui/(app)/event-types/create")]
async fn create_event_type(cx: &Cx, form: Option<Form<CreateForm>>) -> Result<impl View> {
    let auth = auth(cx)?;
    permit(checks::can_write_event_types(auth))?;
    let deps = crate::deps(cx);
    let mut state = CreateState::default();

    if method(cx) == Method::POST {
        let form = form.map(|Form(f)| f).unwrap_or_default();
        let segments = [
            form.application.trim(),
            form.subdomain.trim(),
            form.aggregate.trim(),
            form.event.trim(),
        ];
        let outcome = if !segments.iter().all(|s| valid_segment(s)) {
            Err(PlatformError::validation(
                "Each code segment is required: lowercase letters, numbers, hyphens, dots only",
            ))
        } else if form.name.trim().is_empty() || form.name.len() > 100 {
            Err(PlatformError::validation(
                "Name is required (at most 100 characters)",
            ))
        } else if form.description.len() > 255 {
            Err(PlatformError::validation(
                "Description is at most 255 characters",
            ))
        } else if let Err(e) = ensure_can_create(auth, None) {
            // Anchor-level, as the SPA's create (it sends no client).
            Err(e)
        } else {
            CreateEventTypeUseCase::new(deps.event_type_repo.clone(), deps.unit_of_work.clone())
                .run(
                    CreateEventTypeCommand {
                        code: segments.join(":"),
                        name: form.name.trim().to_owned(),
                        description: Some(form.description.trim().to_owned())
                            .filter(|d| !d.is_empty()),
                        client_id: None,
                        schema: None,
                    },
                    ExecutionContext::from_auth(auth),
                )
                .await
                .into_result()
                .map(|event| event.event_type_id)
                .map_err(PlatformError::from)
        };
        match outcome {
            Ok(id) => {
                set_flash(cx, FlashKind::Success, "Event type created");
                return Err(see_other(detail_href(&id)).into());
            }
            Err(e) if e.status_code().is_client_error() => state.error = Some(e.to_string()),
            Err(e) => {
                tracing::error!(error = %e, "fc-web: create event type failed");
                state.error = Some("Failed to create event type".to_owned());
            }
        }
        state.form = form;
    }

    let mut app_codes: Vec<String> = deps
        .application_repo
        .find_all()
        .await
        .map_err(platform_error)?
        .into_iter()
        .map(|a| a.code)
        .collect();
    app_codes.sort();
    state.app_codes = app_codes;
    Ok(view! { event_type_list(open_id: String::new(), create: Some(state)) })
}

#[component]
async fn create_drawer(state: CreateState, close_href: String) -> Result<impl View> {
    let f = state.form;
    let segments: Vec<(&'static str, &'static str, String, &'static str, bool)> = vec![
        (
            "Application",
            "application",
            f.application.clone(),
            "e.g., operant",
            true,
        ),
        (
            "Subdomain",
            "subdomain",
            f.subdomain.clone(),
            "e.g., execution",
            false,
        ),
        (
            "Aggregate",
            "aggregate",
            f.aggregate.clone(),
            "e.g., trip",
            false,
        ),
        ("Event", "event", f.event.clone(), "e.g., started", false),
    ];
    let or = |v: &str, d: &'static str| {
        if v.is_empty() {
            d.to_owned()
        } else {
            v.to_owned()
        }
    };
    let preview = format!(
        "{}:{}:{}:{}",
        or(&f.application, "app"),
        or(&f.subdomain, "subdomain"),
        or(&f.aggregate, "aggregate"),
        or(&f.event, "event"),
    );
    Ok(view! {
        <aside class="fc-drawer fc-drawer-wide" role="complementary" aria-label="Create event type" data-drawer="">
            <a href=(&close_href) class="fc-icon-btn absolute top-[1.1rem] right-[1.1rem] z-10" aria-label="Close" data-drawer-close="">
                icon(data: iconify_icon!("lucide:x"), size: Length::rem(1.2))
            </a>
            drawer_header(title: "Create Event Type", subtitle: Some("Define a new event type with its code and metadata".to_owned()))
            <form id="et-create-form" method="post" action=(format!("{LIST}/create")) class="fc-drawer-body">
                <section class="fc-form-section">
                    <header class="fc-section-header">
                        <div>
                            <h3 class="fc-section-title">"Event Type Code"</h3>
                            <p class="fc-section-description">"Format: " <code class="fc-mono-chip">"app:subdomain:aggregate:event"</code></p>
                        </div>
                    </header>
                    <div class="flex flex-wrap items-end gap-2">
                        for (i, (label, name, value, placeholder, required)) in segments.into_iter().enumerate() {
                            if i > 0 {
                                <span class="pb-2 text-lg text-[#94a3b8]">":"</span>
                            }
                            <div class="flex min-w-[8rem] flex-1 flex-col">
                                <label class="fc-field-label" for=(format!("et-{name}"))>
                                    (label)
                                    if required { " " <span class="fc-required">"*"</span> }
                                </label>
                                <input
                                    id=(format!("et-{name}"))
                                    name=(name)
                                    class="fc-input"
                                    value=(value)
                                    placeholder=(placeholder)
                                    required=""
                                    pattern="[a-z0-9.\\-]+"
                                    title="Lowercase letters, numbers, hyphens, dots only"
                                    list=((name == "application").then_some("et-app-codes"))
                                >
                            </div>
                        }
                        <datalist id="et-app-codes">
                            for code in state.app_codes {
                                <option value=(code)></option>
                            }
                        </datalist>
                    </div>
                    <div class="mt-4 rounded-md border border-border bg-[#f8fafc] p-3">
                        <span class="fc-field-label">"Generated Code:"</span>
                        code_chips(code: preview)
                    </div>
                </section>

                <section class="fc-form-section">
                    <header class="fc-section-header"><h3 class="fc-section-title">"Metadata"</h3></header>
                    <div class="flex flex-col gap-4">
                        form_field(label: "Name", for_id: "et-new-name", required: true, help: Some("At most 100 characters".to_owned()),
                            <input id="et-new-name" name="name" class="fc-input" value=(f.name.clone()) placeholder="Human-friendly name" required="" maxlength="100">
                        )
                        form_field(label: "Description", for_id: "et-new-description", help: Some("At most 255 characters".to_owned()),
                            <textarea id="et-new-description" name="description" class="fc-input" rows="3" placeholder="Optional description" maxlength="255">(f.description.clone())</textarea>
                        )
                    </div>
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
                <button type="submit" form="et-create-form" class=(Btn::Primary)>
                    icon(data: iconify_icon!("lucide:check"), size: Length::rem(1.0))
                    "Create Event Type"
                </button>
            </footer>
        </aside>
    })
}

// -------------------------------------------------------------- writes

/// Load the event type a write targets, after the caller's permission check.
async fn load_for_write(cx: &Cx, auth: &AuthContext) -> Result<EventType> {
    permit(checks::can_write_event_types(auth))?;
    let id = path_param::<Id>(cx);
    crate::deps(cx)
        .event_type_repo
        .find_by_id(id)
        .await
        .map_err(platform_error)?
        .ok_or_not_found()
        .map_err(Into::into)
}

/// Finish a write: flash the outcome and go to `to`, or back to `retry`
/// when it was refused. Business-rule refusals (4xx) are shown to the
/// user; anything else is logged and shown generically.
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
            tracing::error!(error = %e, "fc-web: event type write failed");
            set_flash(cx, FlashKind::Error, "Something went wrong. Try again.");
            Ok(see_other(retry))
        }
    }
}

#[derive(Deserialize)]
struct UpdateForm {
    name: String,
    description: String,
}

#[route(POST "/ui/(app)/event-types/{id}/update")]
async fn update(cx: &Cx, Form(form): Form<UpdateForm>) -> Result<SeeOther> {
    let auth = auth(cx)?;
    let et = load_for_write(cx, auth).await?;
    permit(ensure_modifiable(auth, &et, "modify"))?;
    let deps = crate::deps(cx);
    let outcome =
        UpdateEventTypeUseCase::new(deps.event_type_repo.clone(), deps.unit_of_work.clone())
            .run(
                UpdateEventTypeCommand {
                    event_type_id: et.id.clone(),
                    name: Some(form.name.trim().to_owned()),
                    description: Some(form.description.trim().to_owned()),
                },
                ExecutionContext::from_auth(auth),
            )
            .await
            .into_result()
            .map(|_| ())
            .map_err(PlatformError::from);
    let base = detail_href(&et.id);
    let retry = format!("{base}?edit=true");
    finish(cx, outcome, "Event type updated", base, retry)
}

#[derive(Deserialize)]
struct VersionForm {
    version: String,
}

#[route(POST "/ui/(app)/event-types/{id}/schemas/finalise")]
async fn finalise_schema(cx: &Cx, Form(form): Form<VersionForm>) -> Result<SeeOther> {
    let auth = auth(cx)?;
    let et = load_for_write(cx, auth).await?;
    permit(ensure_visible(auth, &et))?;
    let deps = crate::deps(cx);
    let outcome =
        FinaliseSchemaUseCase::new(deps.event_type_repo.clone(), deps.unit_of_work.clone())
            .run(
                FinaliseSchemaCommand {
                    event_type_id: et.id.clone(),
                    version: form.version.clone(),
                },
                ExecutionContext::from_auth(auth),
            )
            .await
            .into_result()
            .map(|_| ())
            .map_err(PlatformError::from);
    let base = detail_href(&et.id);
    let ok = format!("Schema {} finalised", form.version);
    finish(cx, outcome, &ok, base.clone(), base)
}

#[route(POST "/ui/(app)/event-types/{id}/schemas/deprecate")]
async fn deprecate_schema(cx: &Cx, Form(form): Form<VersionForm>) -> Result<SeeOther> {
    let auth = auth(cx)?;
    let et = load_for_write(cx, auth).await?;
    permit(ensure_visible(auth, &et))?;
    let deps = crate::deps(cx);
    let outcome =
        DeprecateSchemaUseCase::new(deps.event_type_repo.clone(), deps.unit_of_work.clone())
            .run(
                DeprecateSchemaCommand {
                    event_type_id: et.id.clone(),
                    version: form.version.clone(),
                },
                ExecutionContext::from_auth(auth),
            )
            .await
            .into_result()
            .map(|_| ())
            .map_err(PlatformError::from);
    let base = detail_href(&et.id);
    let ok = format!("Schema {} deprecated", form.version);
    finish(cx, outcome, &ok, base.clone(), base)
}

#[route(POST "/ui/(app)/event-types/{id}/archive")]
async fn archive(cx: &Cx) -> Result<SeeOther> {
    let auth = auth(cx)?;
    let et = load_for_write(cx, auth).await?;
    permit(ensure_modifiable(auth, &et, "archive"))?;
    let deps = crate::deps(cx);
    let outcome =
        ArchiveEventTypeUseCase::new(deps.event_type_repo.clone(), deps.unit_of_work.clone())
            .run(
                ArchiveEventTypeCommand {
                    event_type_id: et.id.clone(),
                },
                ExecutionContext::from_auth(auth),
            )
            .await
            .into_result()
            .map(|_| ())
            .map_err(PlatformError::from);
    let base = detail_href(&et.id);
    finish(cx, outcome, "Event type archived", base.clone(), base)
}

#[route(POST "/ui/(app)/event-types/{id}/delete")]
async fn delete(cx: &Cx) -> Result<SeeOther> {
    let auth = auth(cx)?;
    let et = load_for_write(cx, auth).await?;
    permit(ensure_modifiable(auth, &et, "delete"))?;
    let deps = crate::deps(cx);
    let outcome =
        DeleteEventTypeUseCase::new(deps.event_type_repo.clone(), deps.unit_of_work.clone())
            .run(
                DeleteEventTypeCommand {
                    event_type_id: et.id.clone(),
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
        "Event type deleted",
        LIST.to_owned(),
        detail_href(&et.id),
    )
}

/// "Sync Platform Events": the BFF's `sync-platform`, the same use case and
/// command, with its summary as the flash.
#[route(POST "/ui/(app)/event-types/sync-platform")]
async fn sync_platform(cx: &Cx) -> Result<SeeOther> {
    let auth = auth(cx)?;
    permit(checks::can_write_event_types(auth))?;
    let deps = crate::deps(cx);
    let outcome =
        SyncEventTypesUseCase::new(deps.event_type_repo.clone(), deps.unit_of_work.clone())
            .run(platform_sync_command(), ExecutionContext::from_auth(auth))
            .await
            .into_result()
            .map_err(PlatformError::from);
    match outcome {
        Ok(event) => {
            let counts = |pairs: &[(u32, &str)]| {
                pairs
                    .iter()
                    .filter(|(n, _)| *n > 0)
                    .map(|(n, label)| format!("{n} {label}"))
                    .collect::<Vec<_>>()
            };
            let total = event.synced_codes.len();
            let parts = counts(&[
                (event.created, "created"),
                (event.updated, "updated"),
                (event.deleted, "deleted"),
            ]);
            let schema_parts = counts(&[
                (event.schemas_created, "created"),
                (event.schemas_updated, "updated"),
                (event.schemas_unchanged, "unchanged"),
            ]);
            let schema_total =
                event.schemas_created + event.schemas_updated + event.schemas_unchanged;
            let mut message = if parts.is_empty() {
                format!("Platform Events Synced: {total} event types up to date")
            } else {
                format!(
                    "Platform Events Synced: {} ({total} total)",
                    parts.join(", ")
                )
            };
            if schema_total > 0 {
                message.push_str(&format!(
                    ". Schemas: {} ({schema_total} total)",
                    schema_parts.join(", ")
                ));
            }
            set_flash(cx, FlashKind::Success, message);
        }
        Err(e) => {
            tracing::error!(error = %e, "fc-web: platform event type sync failed");
            set_flash(cx, FlashKind::Error, "Platform sync failed. Try again.");
        }
    }
    Ok(see_other(LIST))
}
