//! `/ui/platform/audit-log`: the platform audit log, newest first
//! (`AuditLogListPage.vue`).
//!
//! Same data and rules as `GET /api/audit-logs` (`audit/api.rs`): anchor
//! only, keyset pagination over `(performed_at, id)`, principal names
//! filled in afterwards. Filters are a GET form, so the URL is the list
//! state. The detail dialog's body is a shard: clicking a row sets a
//! browser signal and the shard re-renders on the server with that id, so
//! the list keeps its scroll position.

use std::collections::HashMap;

use fc_platform::audit::api::{
    AuditLogDetailResponse, enrich_principal_names, enrich_single_principal_name,
};
use fc_platform::checks;
use fc_platform::shared::api_common::{decode_cursor, encode_cursor};
use topcoat::{
    Result,
    context::Cx,
    icon::{icon, iconify::iconify_icon},
    router::{error::bad_request, page, query_params},
    runtime::{Event, shard, signal},
    view::{Length, View, view},
};

use crate::auth::{auth, permit, platform_error};
use crate::ui::{
    Btn, Severity, empty_state, filter_select, json_block, local_time, page_header, table_toolbar,
    tag,
};

const PAGE_SIZE: usize = 100;
const LIST: &str = "/ui/platform/audit-log";

#[query_params(error = bad_request)]
struct AuditQuery {
    entity_type: Option<String>,
    operation: Option<String>,
    after: Option<String>,
    /// The cursor page's number, for "Page N" (the cursor itself is opaque).
    page: Option<usize>,
}

/// "EventTypeCreated" -> "Event Type Created" (`formatOperationName`).
fn humanize(operation: &str) -> String {
    let mut out = String::with_capacity(operation.len() + 8);
    for (i, ch) in operation.chars().enumerate() {
        if i > 0 && ch.is_uppercase() {
            out.push(' ');
        }
        out.push(ch);
    }
    out
}

/// `getEntityTypeSeverity`.
fn entity_severity(entity_type: &str) -> Severity {
    match entity_type {
        "ClientAuthConfig" | "EventType" => Severity::Info,
        "Role" => Severity::Warn,
        "Principal" => Severity::Success,
        _ => Severity::Secondary,
    }
}

fn list_href(
    entity_type: Option<&str>,
    operation: Option<&str>,
    after: Option<&str>,
    page: usize,
) -> String {
    let mut q = form_urlencoded::Serializer::new(String::new());
    if let Some(v) = entity_type {
        q.append_pair("entity_type", v);
    }
    if let Some(v) = operation {
        q.append_pair("operation", v);
    }
    if let Some(v) = after {
        q.append_pair("after", v);
        q.append_pair("page", &page.to_string());
    }
    let q = q.finish();
    if q.is_empty() {
        "/ui/platform/audit-log".to_owned()
    } else {
        format!("/ui/platform/audit-log?{q}")
    }
}

/// Application and client display names for the ids on this page (the Vue
/// page resolves them from its filter option lists).
async fn names(
    cx: &Cx,
    app_ids: Vec<String>,
    client_ids: Vec<String>,
) -> Result<(HashMap<String, String>, HashMap<String, String>)> {
    let deps = crate::deps(cx);
    let (apps, clients) = tokio::try_join!(
        async {
            if app_ids.is_empty() {
                Ok(vec![])
            } else {
                deps.application_repo.find_all().await
            }
        },
        deps.client_repo.find_by_ids(&client_ids),
    )
    .map_err(platform_error)?;
    Ok((
        apps.into_iter().map(|a| (a.id, a.name)).collect(),
        clients.into_iter().map(|c| (c.id, c.name)).collect(),
    ))
}

#[page("/ui/(app)/platform/audit-log")]
async fn audit_log(cx: &Cx) -> Result<impl View> {
    let auth = auth(cx)?;
    permit(checks::require_anchor(auth))?;
    permit(checks::can_read_audit_logs(auth))?;
    let deps = crate::deps(cx);
    let query = query_params::<AuditQuery>(cx)?;
    let entity_type = query.entity_type.as_deref().filter(|s| !s.is_empty());
    let operation = query.operation.as_deref().filter(|s| !s.is_empty());

    let cursor = match query.after.as_deref() {
        Some(c) => Some(decode_cursor(c).map_err(|_| bad_request("Invalid cursor"))?),
        None => None,
    };

    let (entity_types, operations, mut logs) = tokio::try_join!(
        deps.audit_log_repo.find_distinct_entity_types(),
        deps.audit_log_repo.find_distinct_operations(),
        deps.audit_log_repo.search_with_cursor(
            entity_type,
            None,
            operation,
            None,
            cursor.as_ref(),
            (PAGE_SIZE as i64) + 1,
        ),
    )
    .map_err(platform_error)?;

    let has_more = logs.len() > PAGE_SIZE;
    logs.truncate(PAGE_SIZE);
    enrich_principal_names(&mut logs, &deps.principal_repo).await;

    let mut app_ids: Vec<String> = logs
        .iter()
        .filter_map(|l| l.application_id.clone())
        .collect();
    app_ids.sort();
    app_ids.dedup();
    let mut client_ids: Vec<String> = logs.iter().filter_map(|l| l.client_id.clone()).collect();
    client_ids.sort();
    client_ids.dedup();
    let (app_names, client_names) = names(cx, app_ids, client_ids).await?;

    let page = if query.after.is_some() {
        query.page.unwrap_or(2).max(2)
    } else {
        1
    };
    let older_href = has_more
        .then(|| logs.last().map(|l| encode_cursor(l.performed_at, &l.id)))
        .flatten()
        .map(|c| list_href(entity_type, operation, Some(&c), page + 1));
    // Newer is the previous page: the browser's history when there is one
    // (the cursor stack lives in the URLs), else the first page.
    let newer_href = (page > 1).then(|| list_href(entity_type, operation, None, 1));
    let active = usize::from(entity_type.is_some()) + usize::from(operation.is_some());

    let entity_options: Vec<(String, String)> =
        entity_types.into_iter().map(|t| (t.clone(), t)).collect();
    let operation_options: Vec<(String, String)> = operations
        .into_iter()
        .map(|o| {
            let label = humanize(&o);
            (o, label)
        })
        .collect();

    // The id of the entry the detail dialog shows; empty keeps it closed.
    // (Runtime expressions can read an `Option` but not build one.)
    let selected = signal(cx, String::new);

    Ok(view! {
        page_header(title: "Audit Log", subtitle: "View system activity and changes")

        <div class="fc-card-flush">
            table_toolbar(
                form_id: "audit-list",
                action: LIST,
                placeholder: "",
                search: None,
                active_filter_count: active,
                has_active_filters: active > 0,
                show_filters: true,
                filter_select(
                    name: "entity_type",
                    label: "Entity Type",
                    placeholder: "All Entity Types",
                    options: entity_options,
                    selected: entity_type.map(str::to_owned),
                )
                filter_select(
                    name: "operation",
                    label: "Operation",
                    placeholder: "All Operations",
                    options: operation_options,
                    selected: operation.map(str::to_owned),
                )
            )
            if logs.is_empty() {
                empty_state(message: "No audit logs found", clear_href: (active > 0).then(|| LIST.to_owned()))
            } else {
                <table class="fc-table fc-table-sm">
                    <thead>
                        <tr>
                            <th class="w-[15%]">"Time"</th>
                            <th class="w-[13%]">"Entity Type"</th>
                            <th class="w-[14%]">"Entity ID"</th>
                            <th class="w-[18%]">"Operation"</th>
                            <th class="w-[15%]">"Performed By"</th>
                            <th class="w-[13%]">"Application"</th>
                            <th class="w-[13%]">"Client"</th>
                            <th class="w-[5%]"></th>
                        </tr>
                    </thead>
                    <tbody>
                        for log in &logs {
                            let id = log.id.clone();
                            <tr class="fc-row-link" @click=$(|_e: Event| selected.set(id.clone()))>
                                <td class="text-[13px] whitespace-nowrap text-[#64748b]">local_time(at: log.performed_at)</td>
                                <td>tag(label: log.entity_type.clone(), severity: entity_severity(&log.entity_type))</td>
                                <td><code class="fc-mono-chip">(&log.entity_id)</code></td>
                                <td class="font-medium text-[#1e293b]">(humanize(&log.operation))</td>
                                <td class="text-[#475569]">(log.principal_name.clone().unwrap_or_else(|| "Unknown".to_owned()))</td>
                                <td class="text-[13px] text-[#334e68]">
                                    match &log.application_id {
                                        Some(a) => (app_names.get(a).cloned().unwrap_or_else(|| a.clone())),
                                        None => <span class="text-[#94a3b8]">"—"</span>,
                                    }
                                </td>
                                <td class="text-[13px] text-[#334e68]">
                                    match &log.client_id {
                                        Some(c) => (client_names.get(c).cloned().unwrap_or_else(|| c.clone())),
                                        None => <span class="text-[#94a3b8]">"—"</span>,
                                    }
                                </td>
                                <td>
                                    <span class="fc-icon-btn" title="View details">icon(data: iconify_icon!("lucide:eye"), size: Length::rem(1.0))</span>
                                </td>
                            </tr>
                        }
                    </tbody>
                </table>
            }
            // Cursor pager: aud_logs is unbounded, so there is no count.
            <nav class="flex items-center justify-center gap-4 border-t border-border px-4 py-2" aria-label="Pagination">
                match newer_href {
                    Some(href) => <a href=(href) class=(Btn::Text) onclick="if (history.length > 1) { event.preventDefault(); history.back() }">icon(data: iconify_icon!("lucide:chevron-left"), size: Length::rem(1.0)) "Newer"</a>,
                    None => <span class=(Btn::Text) aria-disabled="true">icon(data: iconify_icon!("lucide:chevron-left"), size: Length::rem(1.0)) "Newer"</span>,
                }
                <span class="text-[13px] text-[#64748b]">(format!("Page {page}"))</span>
                match older_href {
                    Some(href) => <a href=(href) class=(Btn::Text)>"Older" icon(data: iconify_icon!("lucide:chevron-right"), size: Length::rem(1.0))</a>,
                    None => <span class=(Btn::Text) aria-disabled="true">"Older" icon(data: iconify_icon!("lucide:chevron-right"), size: Length::rem(1.0))</span>,
                }
            </nav>
        </div>

        // Detail dialog. The overlay closes on a backdrop click or the X.
        <div
            class="fixed inset-0 z-50 flex items-start justify-center overflow-y-auto bg-[rgb(15_23_42/0.4)] p-4 pt-[8vh]"
            hidden=""
            :hidden=$(selected.get().is_empty())
            @click=$(|_e: Event| selected.set("".to_owned()))
        >
            <div
                role="dialog"
                aria-modal="true"
                aria-labelledby="audit-detail-title"
                class="fc-dialog w-[700px] max-w-full"
                onclick="event.stopPropagation()"
            >
                <div class="fc-dialog-header">
                    <span id="audit-detail-title">"Audit Log Details"</span>
                    <button type="button" class="fc-icon-btn" aria-label="Close" @click=$(|_e: Event| selected.set("".to_owned()))>
                        icon(data: iconify_icon!("lucide:x"), size: Length::rem(1.1))
                    </button>
                </div>
                <div class="fc-dialog-body pb-6">
                    audit_log_detail(id: $(selected.get()))
                </div>
            </div>
        </div>
    })
}

/// The detail dialog's body. A shard has its own endpoint, so it checks the
/// caller itself; the path keeps it under the `/ui/(app)` layer.
#[shard("/ui/(app)/platform/audit-log/detail")]
async fn audit_log_detail(cx: &Cx, id: String) -> Result<impl View> {
    let auth = auth(cx)?;
    permit(checks::require_anchor(auth))?;
    permit(checks::can_read_audit_logs(auth))?;
    let deps = crate::deps(cx);

    let log = if id.is_empty() {
        None
    } else {
        deps.audit_log_repo
            .find_by_id(&id)
            .await
            .map_err(platform_error)?
    };
    let (detail, performed_at, app_name, client_name) = match log {
        Some(mut log) => {
            enrich_single_principal_name(&mut log, &deps.principal_repo).await;
            let performed_at = log.performed_at;
            let (apps, clients) = names(
                cx,
                log.application_id.iter().cloned().collect(),
                log.client_id.iter().cloned().collect(),
            )
            .await?;
            let app_name = log
                .application_id
                .as_ref()
                .map(|a| apps.get(a).cloned().unwrap_or_else(|| a.clone()));
            let client_name = log
                .client_id
                .as_ref()
                .map(|c| clients.get(c).cloned().unwrap_or_else(|| c.clone()));
            // The API's detail DTO applies stored-secret redaction.
            (
                Some(AuditLogDetailResponse::from(log)),
                Some(performed_at),
                app_name,
                client_name,
            )
        }
        None => (None, None, None, None),
    };

    Ok(view! {
        match (detail, performed_at) {
            (Some(d), Some(at)) => {
                let data: Option<serde_json::Value> = d
                    .operation_json
                    .as_deref()
                    .and_then(|s| serde_json::from_str(s).ok());
                <div class="fc-dl-grid">
                    <span class="fc-dl-label">"Time"</span>
                    <span>local_time(at: at)</span>
                    <span class="fc-dl-label">"Entity Type"</span>
                    <span>tag(label: d.entity_type.clone(), severity: entity_severity(&d.entity_type))</span>
                    <span class="fc-dl-label">"Entity ID"</span>
                    <span><code class="fc-mono-chip">(d.entity_id.clone().unwrap_or_default())</code></span>
                    <span class="fc-dl-label">"Operation"</span>
                    <span>(humanize(&d.operation))</span>
                    <span class="fc-dl-label">"Performed By"</span>
                    <span>(d.principal_name.clone().unwrap_or_else(|| "Unknown".to_owned()))</span>
                    if let Some(pid) = d.principal_id.clone() {
                        <span class="fc-dl-label">"Principal ID"</span>
                        <span><code class="fc-mono-chip">(pid)</code></span>
                    }
                    if let Some(app) = app_name {
                        <span class="fc-dl-label">"Application"</span>
                        <span>(app)</span>
                    }
                    if let Some(client) = client_name {
                        <span class="fc-dl-label">"Client"</span>
                        <span>(client)</span>
                    }
                </div>
                if let Some(data) = data {
                    <h4 class="mt-6 mb-2 text-sm font-semibold text-[#334e68]">"Operation Data"</h4>
                    json_block(value: data)
                }
            }
            _ => "",
        }
    })
}
