//! `/ui/dispatch-jobs`: the SPA's `DispatchJobListPage.vue` with
//! `DispatchJobDetailDrawer.vue` (`/ui/dispatch-jobs/{id}`, a two-thirds
//! working panel: the job and its payload beside every attempt).
//!
//! A firehose table: the most recent `?size=` rows of the read projection
//! (`msg_dispatch_jobs_read`) matching the filters, no pagination (owner
//! rule). Reads use the same repository calls and client-scope rules as
//! `GET /api/dispatch-jobs`, `/{id}` and `/{id}/attempts`
//! (`caller_reach::read_client_filter` / `ensure_row_visible`), behind the
//! same `can_read_dispatch_jobs` check.
//!
//! Attempts come from `msg_dispatch_job_attempts` (`find_attempts`); the
//! job row carries none.
//!
//! Read-only for now: the SPA's Requeue (list, bulk and drawer) and Sign
//! actions call `POST /api/dispatch-jobs/requeue` and `/{id}/sign`, which
//! the Rust platform doesn't have yet, so they are left out.

use fc_platform::dispatch_job::entity::{DispatchAttempt, parse_dispatch_status};
use fc_platform::dispatch_job::repository::RecordedAttempt;
use fc_platform::shared::caller_reach::{ensure_row_visible, read_client_filter};
use fc_platform::{DispatchJob, PlatformError, checks};
use topcoat::{
    Result,
    context::Cx,
    icon::{icon, iconify::iconify_icon},
    router::{error::RouterErrorExt, page, path_param, query_params},
    runtime::{Event, shard, signal},
    view::{Length, View, component, view},
};

use super::events::{client_options, facet_select};
use crate::auth::{auth, permit, platform_error};
use crate::ui::drawer::DrawerSize;
use crate::ui::list::ResultSize;
use crate::ui::{
    Severity, code_chips, drawer_frame, drawer_header, empty_state, list_query, local_time,
    page_header, table_toolbar, tag,
};

path_param!(id);

const LIST: &str = "/ui/dispatch-jobs";
const FORM_ID: &str = "jobs-list";

fn detail_href(id: &str) -> String {
    format!("{LIST}/{id}")
}

/// The list's `getSeverity`.
fn list_severity(status: &str) -> Severity {
    match status {
        "COMPLETED" => Severity::Success,
        "PENDING" | "QUEUED" => Severity::Info,
        "PROCESSING" => Severity::Warn,
        "FAILED" => Severity::Danger,
        _ => Severity::Secondary,
    }
}

/// The drawer's `statusSeverity` (unknown statuses read as info there).
fn drawer_severity(status: &str) -> Severity {
    match status {
        "COMPLETED" => Severity::Success,
        "FAILED" => Severity::Danger,
        "PROCESSING" => Severity::Warn,
        "CANCELLED" | "EXPIRED" => Severity::Secondary,
        _ => Severity::Info,
    }
}

/// `attemptSeverity`: success green; a 429 or a 2xx that still failed
/// amber; anything else red.
fn attempt_severity(a: &DispatchAttempt) -> Severity {
    if a.success {
        return Severity::Success;
    }
    match a.response_code.unwrap_or(0) {
        429 | 200..=299 => Severity::Warn,
        _ => Severity::Danger,
    }
}

/// `formatJson`: pretty JSON when it parses, else as-is; "-" when empty.
fn format_json(data: Option<&str>) -> String {
    match data {
        None | Some("") => "-".to_owned(),
        Some(s) => serde_json::from_str::<serde_json::Value>(s)
            .ok()
            .and_then(|v| serde_json::to_string_pretty(&v).ok())
            .unwrap_or_else(|| s.to_owned()),
    }
}

/// `requestLine`: what an attempt sent, from its recorded request summary.
fn request_line(r: Option<&serde_json::Value>) -> String {
    let Some(r) = r else {
        return "not recorded".to_owned();
    };
    let text = |k: &str| r.get(k).and_then(|v| v.as_str()).unwrap_or_default();
    let flag = |k: &str| r.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
    if !text("unsignedReason").is_empty() {
        return format!("UNSIGNED — {}", text("unsignedReason"));
    }
    let mut parts = vec![];
    if flag("signature") {
        let by = text("signedBy");
        parts.push(format!(
            "signed by {}",
            if by.is_empty() { "?" } else { by }
        ));
    }
    if flag("bearer") {
        parts.push("bearer".to_owned());
    }
    if parts.is_empty() {
        "no credentials".to_owned()
    } else {
        parts.join(" + ")
    }
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
    code: Option<String>,
    status: Option<String>,
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

#[page("/ui/(app)/dispatch-jobs")]
async fn dispatch_jobs(cx: &Cx) -> Result<impl View> {
    permit(checks::can_read_dispatch_jobs(auth(cx)?))?;
    Ok(view! { job_list(open_id: String::new()) })
}

#[page("/ui/(app)/dispatch-jobs/{id}")]
async fn dispatch_job_detail(cx: &Cx) -> Result<impl View> {
    permit(checks::can_read_dispatch_jobs(auth(cx)?))?;
    let id = path_param::<Id>(cx).to_owned();
    Ok(view! { job_list(open_id: id) })
}

#[component]
async fn job_list(cx: &Cx, open_id: String) -> Result<impl View> {
    let auth = auth(cx)?;
    let query = query_params::<ListQuery>(cx)?;
    let search = trimmed(&query.q);
    let client = trimmed(&query.client);
    let application = trimmed(&query.application);
    let subdomain = trimmed(&query.subdomain);
    let aggregate = trimmed(&query.aggregate);
    let code = trimmed(&query.code);
    let status = trimmed(&query.status);
    let size = ResultSize::new(query.size, "Result size — most recent N jobs");
    let limit = size.value;

    // An unknown (or miscased) status is a 400, as `GET /api/dispatch-jobs`.
    let statuses = one(&status)
        .iter()
        .map(|s| Ok(parse_dispatch_status(s)?.as_str().to_owned()))
        .collect::<std::result::Result<Vec<_>, PlatformError>>()
        .map_err(platform_error)?;

    let deps = crate::deps(cx);
    let repo = &deps.dispatch_job_repo;
    let rows = match permit(read_client_filter(auth, one(&client)))? {
        None => vec![],
        Some(client_ids) => repo
            .find_read_with_cursor(
                &client_ids,
                &statuses,
                &one(&application),
                &one(&subdomain),
                &one(&aggregate),
                &one(&code),
                Some(search.as_str()).filter(|s| !s.is_empty()),
                None,
                limit as i64,
            )
            .await
            .map_err(platform_error)?,
    };
    // `GET /api/dispatch-jobs/filter-options`: distinct values of the read
    // projection.
    let (applications, subdomains, aggregates, codes, status_values) = tokio::try_join!(
        repo.find_distinct_applications(),
        repo.find_distinct_subdomains(),
        repo.find_distinct_aggregates(),
        repo.find_distinct_codes(),
        repo.find_distinct_statuses(),
    )
    .map_err(platform_error)?;
    let clients = client_options(cx, auth).await?;

    let active_filters = [
        &client,
        &application,
        &subdomain,
        &aggregate,
        &code,
        &status,
    ]
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
        page_header(title: "Dispatch Jobs", subtitle: "Monitor webhook dispatch jobs and delivery status")

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
                facet_select(name: "client", label: "Client", placeholder: "All Clients", options: clients, selected: client.clone(), clears: vec!["application", "subdomain", "aggregate", "code"])
                facet_select(name: "application", label: "Application", placeholder: "All Applications", options: to_options(applications), selected: application.clone(), clears: vec!["subdomain", "aggregate", "code"])
                facet_select(name: "subdomain", label: "Subdomain", placeholder: "All Subdomains", options: to_options(subdomains), selected: subdomain.clone(), clears: vec!["aggregate", "code"])
                facet_select(name: "aggregate", label: "Aggregate", placeholder: "All Aggregates", options: to_options(aggregates), selected: aggregate.clone(), clears: vec!["code"])
                facet_select(name: "code", label: "Code", placeholder: "All Codes", options: to_options(codes), selected: code.clone())
                facet_select(name: "status", label: "Status", placeholder: "All Statuses", options: to_options(status_values), selected: status.clone())
            )
            if rows.is_empty() {
                empty_state(message: "No dispatch jobs found", clear_href: has_active.then(|| clear_href.clone()))
            } else {
                <div class="overflow-x-auto">
                    <table class="fc-table fc-table-striped min-w-[60rem]">
                        <thead>
                            <tr>
                                <th class="w-[11rem]">"Job ID"</th>
                                <th>"Code"</th>
                                <th class="w-[10rem]">"Client"</th>
                                <th class="w-[9rem]">"Group"</th>
                                <th class="w-[8rem]">"Status"</th>
                                <th class="w-[10rem]">"Created"</th>
                                <th class="w-[7rem]">"Actions"</th>
                            </tr>
                        </thead>
                        <tbody>
                            for job in &rows {
                                let id = job.id.clone();
                                let eye_id = job.id.clone();
                                let status = job.status.as_str();
                                <tr>
                                    <td>
                                        // The SPA opens the drawer from the id link and the
                                        // eye, not the whole row.
                                        <a
                                            href=(detail_href(&job.id))
                                            class="cursor-pointer font-mono text-[0.875rem] text-[var(--primary)] no-underline hover:underline"
                                            onclick="event.preventDefault()"
                                            @click=$(|_e: Event| selected.set(id.clone()))
                                        >(&job.id)</a>
                                    </td>
                                    <td>code_chips(code: job.code.clone())</td>
                                    <td>
                                        if let Some(cid) = &job.client_id {
                                            <span class="font-mono text-[0.875rem]" title="Client id (identifier not resolved)">(cid)</span>
                                        } else {
                                            <span class="fc-muted text-[0.875rem]">"platform"</span>
                                        }
                                    </td>
                                    <td>
                                        if let Some(group) = &job.message_group {
                                            <span class="inline-block max-w-[8rem] truncate font-mono text-[0.875rem]" title=(group)>(group)</span>
                                        } else {
                                            <span class="fc-muted text-[0.875rem]">"-"</span>
                                        }
                                    </td>
                                    <td>tag(label: status, severity: list_severity(status))</td>
                                    <td class="text-[0.875rem]">local_time(at: job.created_at)</td>
                                    <td>
                                        <a
                                            href=(detail_href(&job.id))
                                            class="fc-icon-btn text-[var(--primary)]"
                                            title="View payload and attempts"
                                            onclick="event.preventDefault()"
                                            @click=$(|_e: Event| selected.set(eye_id.clone()))
                                        >
                                            icon(data: iconify_icon!("lucide:eye"), size: Length::rem(1.0))
                                        </a>
                                    </td>
                                </tr>
                            }
                        </tbody>
                    </table>
                </div>
            }
            // No pagination: dispatch jobs ingest at high rates and "page 2"
            // is meaningless. Adjust size or narrow the filters to see more.
            <div class="px-4 pt-3 pb-3 text-center text-[0.8125rem] text-[#64748b]">
                (format!("Showing {count} dispatch jobs (newest first)"))
                if count == limit {
                    " (size limit reached — narrow filters or increase size)"
                }
            </div>
        </div>

        drawer_frame(selected: frame_selected, label: "Dispatch job", size: DrawerSize::TwoThirds,
            job_drawer(id: $(selected.get()))
        )
    })
}

// -------------------------------------------------------------- drawer

/// `DispatchJobDetailDrawer.vue`. The shard checks the caller as
/// `GET /api/dispatch-jobs/{id}` (and `/attempts`) do.
#[shard("/ui/(app)/dispatch-jobs/drawer")]
async fn job_drawer(cx: &Cx, id: String) -> Result<impl View> {
    let auth = auth(cx)?;
    permit(checks::can_read_dispatch_jobs(auth))?;
    let found = if id.is_empty() {
        None
    } else {
        let repo = &crate::deps(cx).dispatch_job_repo;
        let (job, attempts) = tokio::try_join!(repo.find_by_id(&id), repo.find_attempts(&id))
            .map_err(platform_error)?;
        let job = job.ok_or_not_found()?;
        permit(ensure_row_visible(
            auth,
            job.client_id.as_deref(),
            "dispatch job",
        ))?;
        Some((job, attempts))
    };
    Ok(view! {
        if let Some((job, attempts)) = found {
            job_body(job: job, recorded: attempts)
        }
    })
}

#[component]
async fn job_body(job: DispatchJob, recorded: Vec<RecordedAttempt>) -> Result<impl View> {
    let status = job.status.as_str();
    let dash = |v: Option<&str>| {
        v.filter(|s| !s.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| "-".to_owned())
    };
    // (label, value, mono)
    let facts: Vec<(&'static str, String, bool)> = vec![
        ("Code", job.code.clone(), true),
        ("Client", dash(job.client_id.as_deref()), true),
        ("Message group", dash(job.message_group.as_deref()), true),
        ("Mode", job.mode.as_str().to_owned(), false),
        ("Target", job.target_url.clone(), true),
        ("Subscription", dash(job.subscription_id.as_deref()), true),
        ("Event", dash(job.event_id.as_deref()), true),
        (
            "Attempts",
            format!("{} / {}", job.attempt_count, job.max_retries),
            false,
        ),
    ];
    let payload = format_json(job.payload.as_deref());
    // The API lists attempts oldest-first; an operator reads the latest
    // answer first. Position, not the stored attemptNumber, is the label.
    let total = recorded.len();
    let attempts: Vec<(usize, RecordedAttempt)> = recorded
        .into_iter()
        .rev()
        .enumerate()
        .map(|(i, a)| (total - i, a))
        .collect();

    Ok(view! {
        drawer_header(title: job.code.clone(), subtitle: Some(job.id.clone()))
        <div class="fc-drawer-body">
            <div class="flex flex-col gap-4">
                <div class="flex items-center gap-2">
                    tag(label: status, severity: drawer_severity(status))
                </div>

                <div class="grid items-start gap-8 [grid-template-columns:minmax(0,5fr)_minmax(0,7fr)] max-[1100px]:grid-cols-1">
                    <section class="flex min-w-0 flex-col gap-2">
                        <h3 class="mb-1 text-[0.9375rem] font-semibold text-[#1e293b]">"Job"</h3>
                        for (label, value, mono) in facts {
                            <div class="flex items-baseline gap-3">
                                <label class="flex-[0_0_7rem] text-[0.8125rem] text-[#64748b]">(label)</label>
                                <span class=(if mono { "min-w-0 font-mono break-all" } else { "min-w-0" })>(value)</span>
                            </div>
                        }
                        <div class="flex items-baseline gap-3">
                            <label class="flex-[0_0_7rem] text-[0.8125rem] text-[#64748b]">"Scheduled for"</label>
                            <span>match job.scheduled_for { Some(at) => local_time(at: at), None => "-" }</span>
                        </div>
                        <div class="flex items-baseline gap-3">
                            <label class="flex-[0_0_7rem] text-[0.8125rem] text-[#64748b]">"Created"</label>
                            <span>local_time(at: job.created_at)</span>
                        </div>
                        <div class="flex items-baseline gap-3">
                            <label class="flex-[0_0_7rem] text-[0.8125rem] text-[#64748b]">"Completed"</label>
                            <span>match job.completed_at { Some(at) => local_time(at: at), None => "-" }</span>
                        </div>
                        if let Some(error) = job.last_error.clone() {
                            <div class="flex items-baseline gap-3">
                                <label class="flex-[0_0_7rem] text-[0.8125rem] text-[#64748b]">"Last error"</label>
                                <span class="min-w-0 text-[#dc2626]">(error)</span>
                            </div>
                        }

                        if !job.metadata.is_empty() {
                            <div class="mt-2 flex flex-col gap-1">
                                <label class="text-[0.8125rem] text-[#64748b]">"Additional data"</label>
                                <div class="flex flex-col gap-1">
                                    for m in job.metadata.clone() {
                                        <div class="flex items-center gap-2 text-[0.8125rem]">
                                            <span class="flex-none text-[#64748b]">(m.key)</span>
                                            <span class="min-w-0">(m.value)</span>
                                        </div>
                                    }
                                </div>
                            </div>
                        }

                        <div class="mt-2 flex flex-col gap-1">
                            <div class="flex items-center justify-between">
                                <label class="text-[0.8125rem] text-[#64748b]">"Payload"</label>
                                <button
                                    type="button"
                                    class="fc-icon-btn text-[var(--primary)]"
                                    title="Copy payload"
                                    onclick="navigator.clipboard.writeText(this.closest('div').nextElementSibling.textContent)"
                                >
                                    icon(data: iconify_icon!("lucide:copy"), size: Length::rem(0.95))
                                </button>
                            </div>
                            <pre class="m-0 max-h-[40vh] overflow-auto rounded-md bg-[#f8fafc] p-3 text-[0.8125rem] break-all whitespace-pre-wrap">(payload)</pre>
                        </div>
                    </section>

                    <section class="flex min-w-0 flex-col gap-2">
                        <h3 class="mb-1 text-[0.9375rem] font-semibold text-[#1e293b]">"Attempts"</h3>
                        if attempts.is_empty() {
                            <p class="fc-muted">"No attempts yet."</p>
                        }
                        for (n, a) in attempts {
                            attempt_card(n: n, attempt: a.attempt, request: a.request_info)
                        }
                    </section>
                </div>
            </div>
        </div>
    })
}

#[component]
async fn attempt_card(
    n: usize,
    attempt: DispatchAttempt,
    request: Option<serde_json::Value>,
) -> Result<impl View> {
    let field = |k: &str| {
        request
            .as_ref()
            .and_then(|r| r.get(k))
            .and_then(|v| v.as_str())
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
    };
    let sent = request_line(request.as_ref());
    let timestamp = field("timestamp");
    let headers = request
        .as_ref()
        .and_then(|r| r.get("headers"))
        .and_then(|h| h.as_array())
        .map(|h| {
            h.iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|h| !h.is_empty());
    let code_label = match (attempt.response_code, attempt.error_type.as_ref()) {
        (Some(code), _) => code.to_string(),
        (None, Some(t)) => t.as_str().to_owned(),
        (None, None) => "no response".to_owned(),
    };
    let severity = attempt_severity(&attempt);
    let run_note = format!(
        "Attempt {} of its run — a requeue starts a new run numbered from 1",
        attempt.attempt_number
    );
    let response = attempt
        .response_body
        .as_deref()
        .filter(|b| !b.is_empty())
        .map(|b| format_json(Some(b)));
    Ok(view! {
        <div class="flex flex-col gap-1.5 rounded-md border border-border p-3">
            <div class="flex items-center gap-2">
                <span title=(run_note)>tag(label: format!("#{n}"), severity: Severity::Secondary)</span>
                tag(label: code_label, severity: severity)
                <span class="text-[0.8125rem]">local_time(at: attempt.attempted_at)</span>
                if let Some(ms) = attempt.duration_millis {
                    <span class="fc-muted text-[0.8125rem]">(format!("{ms}ms"))</span>
                }
            </div>
            <div class="flex items-baseline gap-3">
                <label class="flex-[0_0_7rem] text-[0.8125rem] text-[#64748b]">"Sent"</label>
                <span class="min-w-0">(sent)</span>
            </div>
            if let Some(ts) = timestamp {
                <div class="flex items-baseline gap-3">
                    <label class="flex-[0_0_7rem] text-[0.8125rem] text-[#64748b]">"Timestamp"</label>
                    <span class="min-w-0 font-mono">(ts)</span>
                </div>
            }
            if let Some(headers) = headers {
                <div class="flex items-baseline gap-3">
                    <label class="flex-[0_0_7rem] text-[0.8125rem] text-[#64748b]">"Headers"</label>
                    <span class="min-w-0 font-mono text-[0.75rem]">(headers)</span>
                </div>
            }
            if let Some(error) = attempt.error_message.clone() {
                <div class="flex items-baseline gap-3">
                    <label class="flex-[0_0_7rem] text-[0.8125rem] text-[#64748b]">"Error"</label>
                    <span class="min-w-0 text-[#dc2626]">(error)</span>
                </div>
            }
            if let Some(body) = response {
                <div class="mt-2 flex flex-col gap-1">
                    <label class="text-[0.8125rem] text-[#64748b]">"Response"</label>
                    <pre class="m-0 max-h-[14rem] overflow-auto rounded-md bg-[#f8fafc] p-3 text-[0.75rem] break-all whitespace-pre-wrap">(body)</pre>
                </div>
            }
        </div>
    })
}
