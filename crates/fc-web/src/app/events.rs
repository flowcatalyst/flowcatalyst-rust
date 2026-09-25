//! `/ui/events`: the SPA's `EventListPage.vue` with `EventDetailDrawer.vue`
//! (`/ui/events/{id}`).
//!
//! A firehose table: the most recent `?size=` rows of the read projection
//! (`msg_events_read`) matching the filters, no pagination (owner rule).
//! Read-only: the SPA has no event actions. Reads use the same repository
//! calls and client-scope rules as `GET /api/events` and
//! `GET /api/events/{id}` (`caller_reach::read_client_filter` /
//! `ensure_row_visible`), behind the same `can_read_events` check.

use fc_platform::shared::caller_reach::{ensure_row_visible, read_client_filter};
use fc_platform::{AuthContext, Event, EventRead, checks};
use topcoat::{
    Result,
    context::Cx,
    router::{error::RouterErrorExt, page, path_param, query_params},
    runtime::{Event as DomEvent, shard, signal},
    view::{View, component, view},
};

use crate::auth::{auth, permit, platform_error};
use crate::ui::drawer::DrawerSize;
use crate::ui::list::ResultSize;
use crate::ui::{
    Severity, drawer_frame, drawer_header, empty_state, list_query, local_time, page_header,
    table_toolbar, tag,
};

path_param!(id);

const LIST: &str = "/ui/events";
const FORM_ID: &str = "events-list";

fn detail_href(id: &str) -> String {
    format!("{LIST}/{id}")
}

/// `truncateId`: the first ten characters, then "...".
pub(crate) fn truncate_id(id: &str) -> String {
    if id.chars().count() > 10 {
        format!("{}...", id.chars().take(10).collect::<String>())
    } else {
        id.to_owned()
    }
}

/// The clients the caller may filter by: `ClientFilter`'s options (the
/// active clients, as `/bff/filter-options/clients` lists them for the
/// caller), as `(id, name)`.
pub(crate) async fn client_options(cx: &Cx, auth: &AuthContext) -> Result<Vec<(String, String)>> {
    let clients = crate::deps(cx)
        .client_repo
        .find_active()
        .await
        .map_err(platform_error)?;
    Ok(clients
        .into_iter()
        .filter(|c| auth.is_anchor() || auth.can_access_client(&c.id))
        .map(|c| (c.id, c.name))
        .collect())
}

/// A filter Select in the Filters popover. Changing a parent facet clears
/// the facets below it (`clears`), as the SPA's cascading handlers do, then
/// submits the list form.
#[component]
pub(crate) async fn facet_select(
    name: &'static str,
    #[into] label: String,
    #[into] placeholder: String,
    options: Vec<(String, String)>,
    selected: String,
    #[default] clears: Vec<&'static str>,
) -> Result<impl View> {
    let id = format!("filter-{name}");
    let onchange = format!(
        "for (const n of {:?}) {{ const el = this.form.elements[n]; if (el) el.value = ''; }} this.form.requestSubmit()",
        clears
    );
    Ok(view! {
        <div class="fc-form-field">
            <label class="fc-field-label" for=(&id)>(label)</label>
            <select id=(&id) name=(name) class="fc-select" onchange=(onchange)>
                <option value="" selected=(selected.is_empty())>(placeholder)</option>
                for (value, label) in options {
                    <option value=(&value) selected=(selected == value)>(label)</option>
                }
            </select>
        </div>
    })
}

// ---------------------------------------------------------------- list

#[query_params(error = bad_request)]
#[derive(Clone, Default)]
struct ListQuery {
    q: Option<String>,
    client: Option<String>,
    application: Option<String>,
    subdomain: Option<String>,
    aggregate: Option<String>,
    r#type: Option<String>,
    size: Option<usize>,
}

fn trimmed(value: &Option<String>) -> String {
    value
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .to_owned()
}

fn one(value: &str) -> Vec<String> {
    if value.is_empty() {
        vec![]
    } else {
        vec![value.to_owned()]
    }
}

fn to_options(values: Vec<String>) -> Vec<(String, String)> {
    values.into_iter().map(|v| (v.clone(), v)).collect()
}

#[page("/ui/(app)/events")]
async fn events(cx: &Cx) -> Result<impl View> {
    permit(checks::can_read_events(auth(cx)?))?;
    Ok(view! { event_list(open_id: String::new()) })
}

#[page("/ui/(app)/events/{id}")]
async fn event_detail(cx: &Cx) -> Result<impl View> {
    permit(checks::can_read_events(auth(cx)?))?;
    let id = path_param::<Id>(cx).to_owned();
    Ok(view! { event_list(open_id: id) })
}

#[component]
async fn event_list(cx: &Cx, open_id: String) -> Result<impl View> {
    let auth = auth(cx)?;
    let query = query_params::<ListQuery>(cx)?;
    let search = trimmed(&query.q);
    let client = trimmed(&query.client);
    let application = trimmed(&query.application);
    let subdomain = trimmed(&query.subdomain);
    let aggregate = trimmed(&query.aggregate);
    let event_type = trimmed(&query.r#type);
    let size = ResultSize::new(query.size, "Result size — most recent N events");
    let limit = size.value;

    let deps = crate::deps(cx);
    // `GET /api/events`: the caller's client scope, then the facets.
    let rows: Vec<EventRead> = match permit(read_client_filter(auth, one(&client)))? {
        None => vec![],
        Some(client_ids) => deps
            .event_repo
            .find_read_with_cursor(
                &client_ids,
                &one(&application),
                &one(&subdomain),
                &one(&aggregate),
                &one(&event_type),
                None,
                Some(search.as_str()).filter(|s| !s.is_empty()),
                None,
                limit as i64,
            )
            .await
            .map_err(platform_error)?,
    };
    let (options, clients) = tokio::try_join!(
        async {
            deps.event_repo
                .read_filter_options()
                .await
                .map_err(platform_error)
        },
        client_options(cx, auth),
    )?;

    let active_filters = [&client, &application, &subdomain, &aggregate, &event_type]
        .iter()
        .filter(|f| !f.is_empty())
        .count();
    let has_active = active_filters > 0 || !search.is_empty();
    let size_pair = (limit != ResultSize::DEFAULT).then(|| limit.to_string());
    let clear_href = list_query(LIST, &[("size", size_pair.as_deref().unwrap_or(""))]);
    let count = rows.len();

    let selected = signal(cx, move || open_id.clone());
    let frame_selected = selected.clone();

    Ok(view! {
        page_header(title: "Events", subtitle: "Browse events from the event store")

        <div class="fc-card-flush">
            table_toolbar(
                form_id: FORM_ID,
                action: LIST,
                placeholder: "Search by source...",
                search: Some(search.clone()),
                active_filter_count: active_filters,
                has_active_filters: has_active,
                show_filters: true,
                show_search: true,
                size: Some(size),
                show_refresh: true,
                clear_href: Some(clear_href.clone()),
                facet_select(name: "client", label: "Client", placeholder: "All Clients", options: clients, selected: client.clone(), clears: vec!["application", "subdomain", "aggregate", "type"])
                facet_select(name: "application", label: "Application", placeholder: "All Applications", options: to_options(options.applications), selected: application.clone(), clears: vec!["subdomain", "aggregate", "type"])
                facet_select(name: "subdomain", label: "Subdomain", placeholder: "All Subdomains", options: to_options(options.subdomains), selected: subdomain.clone(), clears: vec!["aggregate", "type"])
                facet_select(name: "aggregate", label: "Aggregate", placeholder: "All Aggregates", options: to_options(options.aggregates), selected: aggregate.clone(), clears: vec!["type"])
                facet_select(name: "type", label: "Event Type", placeholder: "All Types", options: to_options(options.types), selected: event_type.clone())
            )
            if rows.is_empty() {
                empty_state(message: "No events found", clear_href: has_active.then(|| clear_href.clone()))
            } else {
                <div class="overflow-x-auto">
                    <table class="fc-table fc-table-striped min-w-[60rem]">
                        <thead>
                            <tr>
                                <th class="w-[10rem]">"Event ID"</th>
                                <th>"Type"</th>
                                <th>"Source"</th>
                                <th>"Subject"</th>
                                <th class="w-[10rem]">"Client"</th>
                                <th class="w-[12rem]">"Time"</th>
                            </tr>
                        </thead>
                        <tbody>
                            for e in &rows {
                                let id = e.id.clone();
                                <tr class="fc-row-link" @click=$(|_e: DomEvent| selected.set(id.clone()))>
                                    <td>
                                        <a href=(detail_href(&e.id)) onclick="event.preventDefault()" class="font-mono text-[0.875rem] text-inherit no-underline">(truncate_id(&e.id))</a>
                                    </td>
                                    <td>tag(label: e.event_type.clone(), severity: Severity::Info)</td>
                                    <td>(&e.source)</td>
                                    <td>
                                        <span class="inline-block max-w-[200px] truncate text-[0.875rem]" title=(e.subject.clone().unwrap_or_default())>
                                            (e.subject.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| "-".to_owned()))
                                        </span>
                                    </td>
                                    <td>
                                        if let Some(cid) = &e.client_id {
                                            <span class="font-mono text-[0.875rem]" title=(cid)>(truncate_id(cid))</span>
                                        } else {
                                            <span class="fc-muted">"-"</span>
                                        }
                                    </td>
                                    <td class="text-[0.875rem]">local_time(at: e.time)</td>
                                </tr>
                            }
                        </tbody>
                    </table>
                </div>
            }
            // No pagination: events ingest at high rates and "page 2" is
            // meaningless. Adjust size or narrow the filters to see more.
            <div class="px-4 pt-3 pb-3 text-center text-[0.8125rem] text-[#64748b]">
                (format!("Showing the {count} most recent events"))
                if count == limit {
                    " (size limit reached — narrow filters or increase size)"
                }
            </div>
        </div>

        drawer_frame(selected: frame_selected, label: "Event", size: DrawerSize::Wide,
            event_drawer(id: $(selected.get()))
        )
    })
}

// -------------------------------------------------------------- drawer

/// `EventDetailDrawer.vue`: read-only, no footer. The shard checks the
/// caller as `GET /api/events/{id}` does.
#[shard("/ui/(app)/events/drawer")]
async fn event_drawer(cx: &Cx, id: String) -> Result<impl View> {
    let auth = auth(cx)?;
    permit(checks::can_read_events(auth))?;
    let found = if id.is_empty() {
        None
    } else {
        let deps = crate::deps(cx);
        let (event, read) = tokio::try_join!(
            async {
                deps.event_repo
                    .find_by_id(&id)
                    .await
                    .map_err(platform_error)
            },
            async {
                deps.event_repo
                    .find_read_by_id(&id)
                    .await
                    .map_err(platform_error)
            },
        )?;
        let event = event.ok_or_not_found()?;
        permit(ensure_row_visible(
            auth,
            event.client_id.as_deref(),
            "event",
        ))?;
        Some((event, read))
    };
    Ok(view! {
        if let Some((event, read)) = found {
            event_body(event: event, read: read)
        }
    })
}

/// "Data" as the SPA's `formatData`: pretty JSON, or a string that parses
/// as JSON pretty-printed, else as-is.
fn format_data(data: &serde_json::Value) -> String {
    match data {
        serde_json::Value::Null => "-".to_owned(),
        serde_json::Value::String(s) => serde_json::from_str::<serde_json::Value>(s)
            .ok()
            .and_then(|v| serde_json::to_string_pretty(&v).ok())
            .unwrap_or_else(|| s.clone()),
        other => serde_json::to_string_pretty(other).unwrap_or_default(),
    }
}

#[component]
async fn event_body(event: Event, read: Option<EventRead>) -> Result<impl View> {
    let dash = |v: Option<&str>| {
        v.filter(|s| !s.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| "-".to_owned())
    };
    let rows: Vec<(&'static str, String, bool)> = vec![
        ("Application", dash(event.application()), false),
        ("Subdomain", dash(event.subdomain()), false),
        ("Aggregate", dash(event.aggregate()), false),
        ("Source", event.source.clone(), false),
        ("Subject", dash(event.subject.as_deref()), false),
    ];
    let ids: Vec<(&'static str, String, bool)> = vec![
        ("Message Group", dash(event.message_group.as_deref()), false),
        (
            "Correlation ID",
            dash(event.correlation_id.as_deref()),
            true,
        ),
        ("Causation ID", dash(event.causation_id.as_deref()), true),
        (
            "Deduplication ID",
            dash(event.deduplication_id.as_deref()),
            true,
        ),
    ];
    let data = format_data(&event.data);
    Ok(view! {
        drawer_header(title: event.event_type.clone(), subtitle: Some(event.id.clone()))
        <div class="fc-drawer-body">
            <div class="flex flex-col gap-3 text-[14px]">
                <div class="flex gap-4">
                    <label class="min-w-[120px] font-semibold text-[#64748b]">"ID"</label>
                    <span class="font-mono">(&event.id)</span>
                </div>
                <div class="flex gap-4">
                    <label class="min-w-[120px] font-semibold text-[#64748b]">"Type"</label>
                    tag(label: event.event_type.clone(), severity: Severity::Info)
                </div>
                for (label, value, _mono) in rows {
                    <div class="flex gap-4">
                        <label class="min-w-[120px] font-semibold text-[#64748b]">(label)</label>
                        <span>(value)</span>
                    </div>
                }
                <div class="flex gap-4">
                    <label class="min-w-[120px] font-semibold text-[#64748b]">"Time"</label>
                    <span>local_time(at: event.time)</span>
                </div>
                <div class="flex gap-4">
                    <label class="min-w-[120px] font-semibold text-[#64748b]">"Client ID"</label>
                    if let Some(cid) = &event.client_id {
                        <span class="font-mono">(cid)</span>
                    } else {
                        <span class="fc-muted">"-"</span>
                    }
                </div>
                for (label, value, mono) in ids {
                    <div class="flex gap-4">
                        <label class="min-w-[120px] font-semibold text-[#64748b]">(label)</label>
                        <span class=(mono.then_some("font-mono"))>(value)</span>
                    </div>
                }
                <div class="flex gap-4">
                    <label class="min-w-[120px] font-semibold text-[#64748b]">"Projected At"</label>
                    if let Some(read) = &read {
                        <span>local_time(at: read.projected_at)</span>
                    } else {
                        <span>"-"</span>
                    }
                </div>
                <div class="mt-2">
                    <label class="mb-2 block font-semibold text-[#64748b]">"Data"</label>
                    <pre class="max-h-[300px] overflow-x-auto rounded-md border border-border bg-[#f8fafc] p-4 font-mono text-[0.875rem] break-words whitespace-pre-wrap">(data)</pre>
                </div>
                if !event.context_data.is_empty() {
                    <div class="mt-2">
                        <label class="mb-2 block font-semibold text-[#64748b]">"Context Data"</label>
                        <div class="rounded-md border border-border bg-[#f8fafc] p-3">
                            for cd in event.context_data.clone() {
                                <div class="py-1">
                                    <span class="mr-2 font-medium">(format!("{}:", cd.key))</span>
                                    <span class="font-mono">(cd.value)</span>
                                </div>
                            }
                        </div>
                    </div>
                }
            </div>
        </div>
    })
}
