//! `/ui/event-types` and `/ui/event-types/{id}` (`EventTypeListPage.vue`,
//! with the detail as a right-hand drawer as the Vue app shows it).
//!
//! Both URLs render the list; `/ui/event-types/{id}` also opens the drawer
//! for that event type, so a write can redirect straight back to it and the
//! drawer is linkable. Clicking a row opens the drawer in place: its body is
//! a shard keyed by a browser signal, so the list keeps its scroll position.
//!
//! Reads go to the repository, as the BFF does. Every write is a plain form
//! POST to a `#[route]` that runs the same use case as the BFF handler
//! (`shared/bff_event_types_api.rs`), so the domain event and audit row
//! come out of `UnitOfWork` exactly as they do for the Vue app, then
//! redirects back with a flash message (Post/Redirect/Get).

use fc_platform::event_type::access::{ensure_modifiable, ensure_visible};
use fc_platform::event_type::entity::{SchemaType, SpecVersionStatus};
use fc_platform::event_type::operations::{
    ArchiveEventTypeCommand, ArchiveEventTypeUseCase, DeleteEventTypeCommand,
    DeleteEventTypeUseCase, DeprecateSchemaCommand, DeprecateSchemaUseCase, FinaliseSchemaCommand,
    FinaliseSchemaUseCase, UpdateEventTypeCommand, UpdateEventTypeUseCase,
};
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
        content::Form,
        error::{RouterErrorExt, SeeOther, see_other},
        page, path_param, query_params, route,
    },
    runtime::{Event, shard, signal},
    view::{Length, View, component, view},
};

use crate::auth::{auth, permit, platform_error};
use crate::ui::{
    Btn, FlashKind, Severity, code_chips, confirm_dialog, empty_state, filter_select, json_block,
    page_header, search_input, set_flash, tag,
};

path_param!(id);

fn detail_href(id: &str) -> String {
    format!("/ui/event-types/{id}")
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

/// Archive once every schema is deprecated (or there are none), as
/// `EventTypeDetailPage.vue`'s `canArchive`.
fn can_archive(et: &EventType) -> bool {
    et.status == EventTypeStatus::Current
        && et
            .spec_versions
            .iter()
            .all(|sv| sv.status == SpecVersionStatus::Deprecated)
}

/// Delete when archived, or current with only unfinished schemas, as
/// `EventTypeDetailPage.vue`'s `canDelete`.
fn can_delete(et: &EventType) -> bool {
    et.status == EventTypeStatus::Archived
        || (et.status == EventTypeStatus::Current
            && et
                .spec_versions
                .iter()
                .all(|sv| sv.status == SpecVersionStatus::Finalising))
}

// ---------------------------------------------------------------- list

#[query_params(error = bad_request)]
struct ListQuery {
    q: Option<String>,
    application: Option<String>,
    status: Option<String>,
}

#[page("/ui/(app)/event-types")]
async fn event_types(cx: &Cx) -> Result<impl View> {
    permit(checks::can_read_event_types(auth(cx)?))?;
    Ok(view! { event_type_list(open_id: String::new()) })
}

#[page("/ui/(app)/event-types/{id}")]
async fn event_type_detail(cx: &Cx) -> Result<impl View> {
    permit(checks::can_read_event_types(auth(cx)?))?;
    let id = path_param::<Id>(cx).to_owned();
    Ok(view! { event_type_list(open_id: id) })
}

/// The list, with the drawer open on `open_id` when it isn't empty.
#[component]
async fn event_type_list(cx: &Cx, open_id: String) -> Result<impl View> {
    let auth = auth(cx)?;
    let query = query_params::<ListQuery>(cx)?;
    let search = query.q.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let application = query.application.as_deref().filter(|s| !s.is_empty());
    let status = query.status.as_deref().filter(|s| !s.is_empty());

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

    let mut applications: Vec<String> = visible.iter().map(|et| et.application.clone()).collect();
    applications.sort();
    applications.dedup();

    let needle = search.map(str::to_lowercase);
    let rows: Vec<EventType> = visible
        .into_iter()
        .filter(|et| application.is_none_or(|a| et.application == a))
        .filter(|et| status.is_none_or(|s| et.status.as_str() == s))
        .filter(|et| {
            needle.as_deref().is_none_or(|n| {
                et.code.to_lowercase().contains(n) || et.name.to_lowercase().contains(n)
            })
        })
        .collect();
    let filtered = search.is_some() || application.is_some() || status.is_some();
    let total = rows.len();

    // The id the drawer shows; empty keeps it closed. Starts open when the
    // URL names one.
    let selected = signal(cx, move || open_id.clone());

    Ok(view! {
        page_header(title: "Event Types", subtitle: "Manage event type definitions and schemas",
            <a href="/event-types/create" class=(Btn::Primary)>
                icon(data: iconify_icon!("lucide:plus"), size: Length::rem(1.0))
                "Create Event Type"
            </a>
        )

        <form method="get" action="/ui/event-types" class="fc-card mb-6">
            <div class="fc-filter-row">
                <div class="fc-filter-group">
                    <label class="fc-label" for="filter-q">"Search"</label>
                    search_input(name: "q", placeholder: "Search event types...", value: search.map(str::to_owned))
                </div>
                filter_select(
                    name: "application",
                    label: "Applications",
                    placeholder: "All Applications",
                    options: applications.into_iter().map(|a| (a.clone(), a)).collect(),
                    selected: application.map(str::to_owned),
                )
                filter_select(
                    name: "status",
                    label: "Status",
                    placeholder: "All Statuses",
                    options: vec![("CURRENT".to_owned(), "Current".to_owned()), ("ARCHIVED".to_owned(), "Archived".to_owned())],
                    selected: status.map(str::to_owned),
                )
                if filtered {
                    <a href="/ui/event-types" class="fc-btn fc-btn-text ml-auto">
                        icon(data: iconify_icon!("lucide:funnel-x"), size: Length::rem(1.0))
                        "Clear Filters"
                    </a>
                }
            </div>
        </form>

        <div class="fc-card-flush">
            if rows.is_empty() {
                empty_state(message: "No event types found")
            } else {
                <table class="fc-table">
                    <thead>
                        <tr>
                            <th class="w-[30%]">"Code"</th>
                            <th class="w-[20%]">"Name"</th>
                            <th>"Description"</th>
                            <th>"Schemas"</th>
                            <th>"Status"</th>
                            <th class="w-12"></th>
                        </tr>
                    </thead>
                    <tbody>
                        for et in &rows {
                            let id = et.id.clone();
                            <tr class="fc-row-link" @click=$(|_e: Event| selected.set(id.clone()))>
                                <td>
                                    // A real link for new tabs and no-JS; a plain
                                    // click opens the drawer in place.
                                    <a href=(detail_href(&et.id)) onclick="event.preventDefault()" class="no-underline">
                                        code_chips(code: et.code.clone())
                                    </a>
                                </td>
                                <td class="font-medium text-[#1e293b]">(&et.name)</td>
                                <td class="max-w-72 truncate text-[#64748b]" title=(et.description.clone().unwrap_or_default())>
                                    (et.description.clone().unwrap_or_else(|| "—".to_owned()))
                                </td>
                                <td>
                                    <div class="flex flex-wrap gap-1">
                                        for sv in &et.spec_versions {
                                            <span title=(sv.status.as_str())>tag(label: sv.version.clone(), severity: schema_severity(sv.status))</span>
                                        }
                                        if et.spec_versions.is_empty() {
                                            <span class="text-[13px] text-[#94a3b8]">"No schemas"</span>
                                        }
                                    </div>
                                </td>
                                <td>tag(label: et.status.as_str(), severity: status_severity(et.status))</td>
                                <td><span class="fc-icon-btn" title="View details">icon(data: iconify_icon!("lucide:chevron-right"), size: Length::rem(1.0))</span></td>
                            </tr>
                        }
                    </tbody>
                </table>
            }
            <div class="fc-table-footer">
                <span>(format!("{total} event types"))</span>
            </div>
        </div>

        <div hidden="" :hidden=$(selected.get().is_empty())>
            <div class="fc-drawer-backdrop" @click=$(|_e: Event| selected.set("".to_owned()))></div>
            <aside class="fc-drawer" role="dialog" aria-modal="true" aria-label="Event type">
                <button
                    type="button"
                    class="fc-icon-btn absolute top-5 right-5 z-10"
                    aria-label="Close"
                    @click=$(|_e: Event| selected.set("".to_owned()))
                >
                    icon(data: iconify_icon!("lucide:x"), size: Length::rem(1.2))
                </button>
                event_type_drawer(id: $(selected.get()))
            </aside>
        </div>
    })
}

// -------------------------------------------------------------- drawer

/// The drawer's content for one event type. A shard has its own endpoint,
/// so it checks the caller itself; the path keeps it under the layer.
#[shard("/ui/(app)/event-types/drawer")]
async fn event_type_drawer(cx: &Cx, id: String) -> Result<impl View> {
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
            drawer_body(auth: auth.clone(), et: et)
        }
    })
}

#[component]
async fn drawer_body(auth: AuthContext, et: EventType) -> Result<impl View> {
    let can_write = checks::can_write_event_types(&auth).is_ok();
    let can_modify = can_write && ensure_modifiable(&auth, &et, "modify").is_ok();
    let archived = et.status == EventTypeStatus::Archived;
    let editable = can_modify && !archived;
    let base = detail_href(&et.id);
    let archivable = can_archive(&et);
    let deletable = can_delete(&et);

    Ok(view! {
        <header class="flex flex-col gap-1 px-6 pt-5 pb-4 pr-14">
            <div class="flex items-center gap-3">
                <h2 class="text-xl font-semibold text-[#1e293b]">(&et.name)</h2>
                tag(label: et.status.as_str(), severity: status_severity(et.status))
            </div>
            <p class="text-sm text-[#64748b]">(&et.code)</p>
        </header>

        <div class="fc-drawer-body">
            <section class="flex flex-col gap-4">
                <h3 class="fc-section-title">"Event Type Details"</h3>
                <form id="et-edit-form" method="post" action=(format!("{base}/update")) class="flex flex-col gap-4">
                    <div class="flex flex-col gap-1.5">
                        <label class="fc-label" for="et-name">"Name"</label>
                        <input id="et-name" name="name" class="fc-input" value=(&et.name) required="" maxlength="100" disabled=(!editable)>
                    </div>
                    <div class="flex flex-col gap-1.5">
                        <label class="fc-label" for="et-description">"Description"</label>
                        <textarea id="et-description" name="description" class="fc-input" rows="3" disabled=(!editable)>(et.description.clone().unwrap_or_default())</textarea>
                    </div>
                    <p class="text-sm text-[#64748b]">
                        "Client scoped: "
                        <span class="font-medium text-[#1e293b]">(if et.client_scoped { "Yes" } else { "No" })</span>
                    </p>
                </form>
            </section>

            <hr class="my-6 border-border">

            <section class="flex flex-col gap-3">
                <div class="flex items-center justify-between">
                    <h3 class="fc-section-title">"Schema Versions"</h3>
                    if can_write && !archived {
                        <a href=(format!("/event-types/{}/add-schema", et.id)) class=(Btn::Link)>
                            icon(data: iconify_icon!("lucide:plus"), size: Length::rem(1.0))
                            "Add Schema"
                        </a>
                    }
                </div>
                if et.spec_versions.is_empty() {
                    <p class="text-sm text-[#94a3b8]">"No schema versions yet."</p>
                } else {
                    <table class="fc-table">
                        <thead>
                            <tr>
                                <th>"Version"</th>
                                <th>"MIME Type"</th>
                                <th>"Schema Type"</th>
                                <th>"Status"</th>
                                <th>"Actions"</th>
                            </tr>
                        </thead>
                        <tbody>
                            for sv in et.spec_versions.clone() {
                                schema_row(base: base.clone(), sv: sv, can_write: can_write)
                            }
                        </tbody>
                    </table>
                }
            </section>

            if can_modify {
                <hr class="my-6 border-border">
                <section class="flex flex-col gap-3">
                    <h3 class="fc-danger-title">"Danger Zone"</h3>
                    if !archived {
                        <div class="fc-danger-item">
                            <div>
                                <p class="font-semibold text-[#1e293b]">"Archive Event Type"</p>
                                <p class="text-sm text-[#64748b]">"Requires all schemas to be deprecated first."</p>
                            </div>
                            <button type="button" class=(Btn::WarnOutline) disabled=(!archivable) commandfor="et-archive" command="show-modal">"Archive"</button>
                        </div>
                        confirm_dialog(
                            id: "et-archive",
                            action: format!("{base}/archive"),
                            title: "Archive Event Type",
                            message: format!("Archive \"{}\"? It stops accepting new schema versions.", et.name),
                            confirm_label: "Archive",
                        )
                    }
                    <div class="fc-danger-item">
                        <div>
                            <p class="font-semibold text-[#1e293b]">"Delete Event Type"</p>
                            <p class="text-sm text-[#64748b]">"Permanently delete this event type."</p>
                        </div>
                        <button type="button" class=(Btn::DangerOutline) disabled=(!deletable) commandfor="et-delete" command="show-modal">"Delete"</button>
                    </div>
                    confirm_dialog(
                        id: "et-delete",
                        action: format!("{base}/delete"),
                        title: "Delete Event Type",
                        message: format!("Permanently delete \"{}\"? This cannot be undone.", et.name),
                        confirm_label: "Delete",
                        danger: true,
                    )
                </section>
            }
        </div>

        if editable {
            <footer class="fc-drawer-footer">
                <button type="submit" form="et-edit-form" class=(Btn::Primary)>
                    icon(data: iconify_icon!("lucide:check"), size: Length::rem(1.0))
                    "Save"
                </button>
            </footer>
        }
    })
}

/// One schema version: its row, its actions, and the dialogs they open.
#[component]
async fn schema_row(base: String, sv: SpecVersion, can_write: bool) -> Result<impl View> {
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
            <td class="font-mono">(&sv.version)</td>
            <td><code class="fc-mono-chip">(&sv.mime_type)</code></td>
            <td>(schema_type_label(sv.schema_type))</td>
            <td>tag(label: sv.status.as_str(), severity: schema_severity(sv.status))</td>
            <td>
                <div class="flex items-center gap-1">
                    <button type="button" class="fc-icon-btn text-[#0284c7]" title="View schema" commandfor=(&view_id) command="show-modal">
                        icon(data: iconify_icon!("lucide:eye"), size: Length::rem(1.0))
                    </button>
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

                <dialog id=(&view_id) class="fc-dialog w-[48rem] max-w-[calc(100vw-2rem)]" closedby="any" aria-label=(format!("Schema {version}"))>
                    <div class="fc-dialog-header">
                        <span>(format!("Schema {version}"))</span>
                        <button type="button" class="fc-icon-btn" aria-label="Close" commandfor=(&view_id) command="close">
                            icon(data: iconify_icon!("lucide:x"), size: Length::rem(1.1))
                        </button>
                    </div>
                    <div class="fc-dialog-body pb-6">
                        match sv.schema_content.clone() {
                            Some(schema) => json_block(value: schema),
                            None => <p class="text-sm text-[#64748b]">"No schema content."</p>,
                        }
                    </div>
                </dialog>
                if can_write && sv.status == SpecVersionStatus::Finalising {
                    confirm_dialog(
                        id: finalise_id.clone(),
                        action: format!("{base}/schemas/finalise"),
                        title: "Finalise Schema",
                        message: format!("Finalise version {version}? It becomes the current schema."),
                        confirm_label: "Finalise",
                        fields: fields.clone(),
                    )
                }
                if can_write && sv.status == SpecVersionStatus::Current {
                    confirm_dialog(
                        id: deprecate_id.clone(),
                        action: format!("{base}/schemas/deprecate"),
                        title: "Deprecate Schema",
                        message: format!("Deprecate version {version}? Producers should move to a newer version."),
                        confirm_label: "Deprecate",
                        fields: fields.clone(),
                    )
                }
            </td>
        </tr>
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

/// Finish a write: flash the outcome and go back to `to`. Business-rule
/// refusals (4xx) are shown to the user; anything else is logged and shown
/// generically.
fn finish(
    cx: &Cx,
    outcome: std::result::Result<(), PlatformError>,
    ok: &str,
    to: String,
) -> Result<SeeOther> {
    match outcome {
        Ok(()) => set_flash(cx, FlashKind::Success, ok),
        Err(e) if e.status_code().is_client_error() => {
            set_flash(cx, FlashKind::Error, e.to_string())
        }
        Err(e) => {
            tracing::error!(error = %e, "fc-web: event type write failed");
            set_flash(cx, FlashKind::Error, "Something went wrong. Try again.");
        }
    }
    Ok(see_other(to))
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
    let description = form.description.trim();
    let outcome =
        UpdateEventTypeUseCase::new(deps.event_type_repo.clone(), deps.unit_of_work.clone())
            .run(
                UpdateEventTypeCommand {
                    event_type_id: et.id.clone(),
                    name: Some(form.name.trim().to_owned()),
                    description: Some(description.to_owned()),
                },
                ExecutionContext::from_auth(auth),
            )
            .await
            .into_result()
            .map(|_| ())
            .map_err(PlatformError::from);
    finish(cx, outcome, "Event type updated.", detail_href(&et.id))
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
    finish(
        cx,
        outcome,
        &format!("Version {} finalised.", form.version),
        detail_href(&et.id),
    )
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
    finish(
        cx,
        outcome,
        &format!("Version {} deprecated.", form.version),
        detail_href(&et.id),
    )
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
    finish(cx, outcome, "Event type archived.", detail_href(&et.id))
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
    let to = if outcome.is_ok() {
        "/ui/event-types".to_owned()
    } else {
        detail_href(&et.id)
    };
    finish(cx, outcome, "Event type deleted.", to)
}
