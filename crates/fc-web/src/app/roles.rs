//! `/ui/authorization/roles`: the SPA's `RoleListPage.vue` (list, the
//! "Create Role" dialog, row delete) with `RoleDetailDrawer.vue`
//! (`/ui/authorization/roles/{name}`, read-only).
//!
//! Editing a role's permissions is `RoleEditPage.vue`, a full page in the
//! SPA (the permission catalogue editor); the Edit actions open it there.
//!
//! Reads: `can_read_roles`, the `/api/roles` read gate and the SPA's
//! `platform:iam:role:view` route permission (the BFF list and get only
//! authenticate). Writes run the BFF's use cases behind its checks:
//! anchor *and* the role permission (`can_administer_roles`, owner ruling
//! 25) plus the permission ceiling (owner ruling 14).

use std::sync::Arc;

use fc_platform::permissions;
use fc_platform::role::ceiling;
use fc_platform::role::operations::{
    CreateRoleCommand, CreateRoleUseCase, DeleteRoleCommand, DeleteRoleUseCase,
};
use fc_platform::usecase::UseCase;
use fc_platform::{AuthContext, AuthRole, ExecutionContext, PlatformError, RoleSource, checks};
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
    runtime::{Event, shard, signal},
    view::{Length, View, component, view},
};

use crate::auth::{auth, permit, platform_error};
use crate::ui::drawer::DrawerSize;
use crate::ui::{
    Btn, FlashKind, Pager, Severity, confirm_dialog, detail_field, detail_value, drawer_frame,
    drawer_header, empty_state, filter_select, form_field, list_query, local_time, page_header,
    paginator, set_flash, table_toolbar, tag,
};

path_param!(name);

const LIST: &str = "/ui/authorization/roles";
const FORM_ID: &str = "role-list";
const CREATE_DIALOG: &str = "role-create";

fn detail_href(name: &str) -> String {
    format!("{LIST}/{name}")
}

/// The SPA's full-page editor (`RoleEditPage.vue`).
fn spa_edit_href(name: &str) -> String {
    format!("/authorization/roles/{}/edit", encode_segment(name))
}

/// `encodeURIComponent` for a role name (it contains ':').
fn encode_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// A path segment as the browser may send it (`%3A` for ':').
fn decode_segment(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_owned())
}

/// `getSourceSeverity`.
fn source_severity(source: RoleSource) -> Severity {
    match source {
        RoleSource::Code => Severity::Info,
        RoleSource::Database => Severity::Success,
        RoleSource::Sdk => Severity::Warn,
    }
}

/// The list's `getSourceLabel`.
fn source_label(source: RoleSource) -> &'static str {
    match source {
        RoleSource::Code => "Code",
        RoleSource::Database => "Admin",
        RoleSource::Sdk => "SDK",
    }
}

/// The drawer's `getSourceLabel` (and the filter's options).
fn source_long_label(source: RoleSource) -> &'static str {
    match source {
        RoleSource::Code => "Code-defined",
        RoleSource::Database => "Admin-created",
        RoleSource::Sdk => "SDK-registered",
    }
}

/// `getActionSeverity`.
fn action_severity(action: &str) -> Severity {
    match action {
        "view" => Severity::Info,
        "create" => Severity::Success,
        "update" => Severity::Warn,
        "delete" => Severity::Danger,
        _ => Severity::Secondary,
    }
}

/// "myapp:admin" -> "admin" (`BffRoleResponse.short_name`).
fn short_name(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

fn title(role: &AuthRole) -> String {
    if role.display_name.is_empty() {
        short_name(&role.name).to_owned()
    } else {
        role.display_name.clone()
    }
}

/// The BFF's lookup: a name (it has a ':') or an id.
async fn find_role(cx: &Cx, name_or_id: &str) -> Result<Option<AuthRole>> {
    let repo = &crate::deps(cx).auth_state.role_repo;
    if name_or_id.contains(':') {
        repo.find_by_name(name_or_id).await
    } else {
        repo.find_by_id(name_or_id).await
    }
    .map_err(platform_error)
}

// ---------------------------------------------------------------- list

#[query_params(error = bad_request)]
#[derive(Clone, Default)]
struct ListQuery {
    q: Option<String>,
    app: Option<String>,
    source: Option<String>,
    page: Option<usize>,
    rows: Option<usize>,
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
                ("source", Self::get(&self.source)),
                ("rows", &rows),
                ("page", &page),
            ],
        )
    }
}

#[page("/ui/(app)/authorization/roles")]
async fn roles(cx: &Cx) -> Result<impl View> {
    permit(checks::can_read_roles(auth(cx)?))?;
    Ok(view! { role_list(open_name: String::new(), create: None) })
}

#[page("/ui/(app)/authorization/roles/{name}")]
async fn role_detail(cx: &Cx) -> Result<impl View> {
    permit(checks::can_read_roles(auth(cx)?))?;
    let name = decode_segment(path_param::<Name>(cx));
    Ok(view! { role_list(open_name: name, create: None) })
}

/// The list, with the drawer open on `open_name` (when not empty) or the
/// create dialog open (when `create` is set).
#[component]
async fn role_list(cx: &Cx, open_name: String, create: Option<CreateState>) -> Result<impl View> {
    let auth = auth(cx)?;
    let can_create = checks::can_administer_roles(auth, permissions::iam::ROLE_CREATE).is_ok();
    let can_delete = checks::can_administer_roles(auth, permissions::iam::ROLE_DELETE).is_ok();
    let query = query_params::<ListQuery>(cx)?;
    let search = ListQuery::get(&query.q).to_owned();
    let app = ListQuery::get(&query.app).to_owned();
    let source = ListQuery::get(&query.source).to_owned();
    let deps = crate::deps(cx);

    let (all, applications) = tokio::try_join!(
        deps.auth_state.role_repo.find_all(),
        deps.application_repo.find_active(),
    )
    .map_err(platform_error)?;
    let app_options: Vec<(String, String)> =
        applications.into_iter().map(|a| (a.code, a.name)).collect();

    let needle = search.to_lowercase();
    let rows: Vec<AuthRole> = all
        .into_iter()
        .filter(|r| app.is_empty() || r.application_code == app)
        .filter(|r| source.is_empty() || r.source.as_str() == source)
        .filter(|r| {
            needle.is_empty()
                || r.name.to_lowercase().contains(&needle)
                || r.display_name.to_lowercase().contains(&needle)
                || r.description
                    .as_deref()
                    .is_some_and(|d| d.to_lowercase().contains(&needle))
        })
        .collect();
    let active_filters = [&app, &source].iter().filter(|f| !f.is_empty()).count();
    let has_active = active_filters > 0 || !search.is_empty();
    let pager = Pager::new(rows.len(), query.page, query.rows);
    let rows = pager.slice(rows);

    let selected = signal(cx, move || open_name.clone());
    let frame_selected = selected.clone();

    let q = Arc::new(query.clone());
    let page_href: Arc<dyn Fn(usize) -> String + Send + Sync> = {
        let q = q.clone();
        Arc::new(move |p| q.href(LIST, Some(p)))
    };
    let hidden = query
        .rows
        .map(|r| vec![("rows".to_owned(), r.to_string())])
        .unwrap_or_default();
    let sources: Vec<(String, String)> = [RoleSource::Code, RoleSource::Database, RoleSource::Sdk]
        .into_iter()
        .map(|s| (s.as_str().to_owned(), source_long_label(s).to_owned()))
        .collect();
    let create_open = create.is_some();
    let dialog_apps = app_options.clone();
    // The dialog preselects the first application, as `openCreateDialog`.
    let mut create = create.unwrap_or_default();
    if create.form.application_code.is_empty() && create.error.is_none() {
        create.form.application_code = app_options
            .first()
            .map(|(c, _)| c.clone())
            .unwrap_or_default();
    }

    Ok(view! {
        page_header(title: "Roles", subtitle: "Manage roles and their permissions",
            if can_create {
                <button type="button" class=(Btn::Primary) commandfor=(CREATE_DIALOG) command="show-modal">
                    icon(data: iconify_icon!("lucide:plus"), size: Length::rem(1.0))
                    "Create Role"
                </button>
            }
        )

        <div class="fc-card-flush">
            table_toolbar(
                form_id: FORM_ID,
                action: LIST,
                placeholder: "Search roles...",
                search: Some(search.clone()),
                active_filter_count: active_filters,
                has_active_filters: has_active,
                show_filters: true,
                show_search: true,
                hidden: hidden,
                filter_select(name: "app", label: "Application", placeholder: "All applications", options: app_options.clone(), selected: (!app.is_empty()).then(|| app.clone()))
                filter_select(name: "source", label: "Source", placeholder: "All sources", options: sources, selected: (!source.is_empty()).then(|| source.clone()))
            )
            if rows.is_empty() {
                empty_state(message: "No roles found", clear_href: has_active.then(|| LIST.to_owned()))
            } else {
                <table class="fc-table">
                    <thead>
                        <tr>
                            <th class="w-[25%]">"Role"</th>
                            <th class="w-[30%]">"Description"</th>
                            <th class="w-[10%]">"Permissions"</th>
                            <th class="w-[15%]">"Application"</th>
                            <th class="w-[10%]">"Source"</th>
                            <th class="w-[7%]">"Actions"</th>
                        </tr>
                    </thead>
                    <tbody>
                        for role in &rows {
                            let name = role.name.clone();
                            let editable = role.source == RoleSource::Database;
                            let delete_id = format!("role-delete-{}", role.id);
                            <tr class="fc-row-link" @click=$(|_e: Event| selected.set(name.clone()))>
                                <td>
                                    <a href=(detail_href(&role.name)) onclick="event.preventDefault()" class="flex flex-col gap-[2px] no-underline">
                                        <span class="font-medium text-[#1e293b]">(title(role))</span>
                                        <span class="font-mono text-[12px] text-[#64748b]">(&role.name)</span>
                                    </a>
                                </td>
                                <td>
                                    <span class="block max-w-[300px] truncate text-[13px] text-[#64748b]" title=(role.description.clone().unwrap_or_default())>
                                        (role.description.clone().filter(|d| !d.is_empty()).unwrap_or_else(|| "—".to_owned()))
                                    </span>
                                </td>
                                <td><span class="font-medium text-[#475569]">(role.permissions.len().to_string())</span></td>
                                <td>tag(label: role.application_code.clone(), severity: Severity::Secondary)</td>
                                <td>tag(label: source_label(role.source), severity: source_severity(role.source))</td>
                                <td>
                                    <div class="flex gap-1" onclick="event.stopPropagation()">
                                        if editable {
                                            <a href=(spa_edit_href(&role.name)) class="fc-icon-btn" title="Edit role">
                                                icon(data: iconify_icon!("lucide:pencil"), size: Length::rem(1.0))
                                            </a>
                                        }
                                        if editable && can_delete {
                                            <button type="button" class="fc-icon-btn text-[#dc2626]" title="Delete role" commandfor=(&delete_id) command="show-modal">
                                                icon(data: iconify_icon!("lucide:trash-2"), size: Length::rem(1.0))
                                            </button>
                                            confirm_dialog(
                                                id: delete_id.clone(),
                                                action: format!("{}/delete", detail_href(&role.name)),
                                                title: "Delete Role",
                                                message: format!("Are you sure you want to delete the role \"{}\"?", if role.display_name.is_empty() { role.name.clone() } else { role.display_name.clone() }),
                                                confirm_label: "Delete",
                                                danger: true,
                                            )
                                        }
                                    </div>
                                </td>
                            </tr>
                        }
                    </tbody>
                </table>
            }
            paginator(pager: pager, noun: "roles", href: page_href, form_id: FORM_ID)
        </div>

        drawer_frame(selected: frame_selected, label: "Role", size: DrawerSize::Wide,
            role_drawer(name: $(selected.get()))
        )

        if can_create {
            create_dialog(state: create, applications: dialog_apps, open: create_open)
        }
    })
}

// -------------------------------------------------------------- drawer

/// `RoleDetailDrawer.vue`: read-only. A shard has its own endpoint, so it
/// checks the caller itself; the path keeps it under the layer.
#[shard("/ui/(app)/authorization/roles/drawer")]
async fn role_drawer(cx: &Cx, name: String) -> Result<impl View> {
    let auth = auth(cx)?;
    permit(checks::can_read_roles(auth))?;
    let role = if name.is_empty() {
        None
    } else {
        Some(find_role(cx, &name).await?.ok_or_not_found()?)
    };
    Ok(view! {
        if let Some(role) = role {
            role_drawer_body(role: role)
        }
    })
}

#[component]
async fn role_drawer_body(role: AuthRole) -> Result<impl View> {
    // Only admin-created roles are editable.
    let can_edit = role.source == RoleSource::Database;
    let mut perms: Vec<String> = role.permissions.iter().cloned().collect();
    perms.sort();
    let count = perms.len();
    let rows: Vec<(String, [String; 4])> = perms
        .into_iter()
        .map(|p| {
            let mut parts = p.split(':').map(str::to_owned);
            let parsed = [
                parts.next().unwrap_or_default(),
                parts.next().unwrap_or_default(),
                parts.next().unwrap_or_default(),
                parts.next().unwrap_or_default(),
            ];
            (p, parsed)
        })
        .collect();

    Ok(view! {
        drawer_header(title: title(&role), subtitle: Some(role.name.clone()),
            tag(label: source_long_label(role.source), severity: source_severity(role.source))
        )
        <div class="fc-drawer-body">
            <section class="fc-form-section">
                <header class="fc-section-header">
                    <h3 class="fc-section-title">"Role Details"</h3>
                    if can_edit {
                        <a href=(spa_edit_href(&role.name)) class=(Btn::TextPrimary)>
                            icon(data: iconify_icon!("lucide:pencil"), size: Length::rem(1.0))
                            "Edit"
                        </a>
                    }
                </header>
                <div class="fc-section-body">
                    <div class="fc-detail-grid">
                        detail_field(label: "Role Name", <code>(&role.name)</code>)
                        detail_value(label: "Display Name", value: Some(role.display_name.clone()))
                        detail_field(label: "Application", tag(label: role.application_code.clone(), severity: Severity::Secondary))
                        detail_field(label: "Source", tag(label: source_long_label(role.source), severity: source_severity(role.source)))
                        detail_value(
                            label: "Description",
                            value: Some(role.description.clone().filter(|d| !d.is_empty()).unwrap_or_else(|| "No description provided".to_owned())),
                            span: true,
                        )
                        detail_field(label: "Created", local_time(at: role.created_at))
                        detail_field(label: "Updated", local_time(at: role.updated_at))
                    </div>
                </div>
            </section>

            <section class="fc-form-section">
                <header class="fc-section-header">
                    <h3 class="fc-section-title">(format!("Permissions ({count})"))</h3>
                </header>
                <div class="fc-section-body">
                    if rows.is_empty() {
                        <div class="flex flex-col items-center justify-center gap-3 p-12 text-[#64748b]">
                            icon(data: iconify_icon!("lucide:lock"), size: Length::px(32.0), attrs: topcoat::view::attributes! { class="text-[#cbd5e1]" })
                            <span>"This role has no permissions assigned"</span>
                        </div>
                    } else {
                        <table class="fc-table fc-table-sm fc-table-striped">
                            <thead>
                                <tr>
                                    <th class="w-[40%]">"Permission"</th>
                                    <th class="w-[15%]">"Application"</th>
                                    <th class="w-[15%]">"Context"</th>
                                    <th class="w-[15%]">"Aggregate"</th>
                                    <th class="w-[15%]">"Action"</th>
                                </tr>
                            </thead>
                            <tbody>
                                for (permission, [application, context, aggregate, action]) in rows {
                                    <tr>
                                        <td><span class="font-mono text-[13px] text-[#475569]">(permission)</span></td>
                                        <td>tag(label: application, severity: Severity::Secondary)</td>
                                        <td>(context)</td>
                                        <td>(aggregate)</td>
                                        <td>tag(label: action.clone(), severity: action_severity(&action))</td>
                                    </tr>
                                }
                            </tbody>
                        </table>
                    }
                </div>
            </section>
        </div>
    })
}

// -------------------------------------------------------------- create

#[derive(Clone, Default, Deserialize)]
struct CreateForm {
    #[serde(default)]
    application_code: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    description: String,
}

#[derive(Clone, Default)]
struct CreateState {
    form: CreateForm,
    error: Option<String>,
}

/// The "Create Role" dialog. Opened in place from the header button; the
/// create page renders it already open after a refused attempt.
#[component]
async fn create_dialog(
    state: CreateState,
    applications: Vec<(String, String)>,
    open: bool,
) -> Result<impl View> {
    let f = state.form;
    Ok(view! {
        <dialog id=(CREATE_DIALOG) class="fc-dialog w-[500px] max-w-[calc(100vw-2rem)]" closedby="any" aria-labelledby="role-create-title">
            <div class="fc-dialog-header">
                <span id="role-create-title">"Create Role"</span>
                <button type="button" class="fc-icon-btn" aria-label="Close" commandfor=(CREATE_DIALOG) command="close">
                    icon(data: iconify_icon!("lucide:x"), size: Length::rem(1.1))
                </button>
            </div>
            <form id="role-create-form" method="post" action=(format!("{LIST}/create")) class="fc-dialog-body flex flex-col gap-5 py-2">
                form_field(label: "Application", for_id: "role-app", required: true,
                    <select id="role-app" name="application_code" class="fc-select" required="">
                        <option value="" selected=(f.application_code.is_empty())>"Select application"</option>
                        for (code, name) in applications {
                            <option value=(&code) selected=(code == f.application_code)>(name)</option>
                        }
                    </select>
                )
                form_field(label: "Role Name", for_id: "role-name", required: true, help: Some("Will be prefixed with application code (e.g., \"myapp:admin\")".to_owned()),
                    <input id="role-name" name="name" class="fc-input" value=(f.name.clone()) placeholder="e.g., admin, viewer, manager" required="">
                )
                form_field(label: "Display Name", for_id: "role-display-name",
                    <input id="role-display-name" name="display_name" class="fc-input" value=(f.display_name.clone()) placeholder="e.g., Administrator">
                )
                form_field(label: "Description", for_id: "role-description",
                    <textarea id="role-description" name="description" class="fc-input" rows="3" placeholder="What this role grants access to">(f.description.clone())</textarea>
                )
                if let Some(error) = state.error {
                    <div class="fc-banner fc-banner-error" role="alert">(error)</div>
                }
            </form>
            <div class="fc-dialog-footer">
                <button type="button" class=(Btn::Outline) commandfor=(CREATE_DIALOG) command="close">
                    icon(data: iconify_icon!("lucide:x"), size: Length::rem(1.0))
                    "Cancel"
                </button>
                <button type="submit" form="role-create-form" class=(Btn::Primary)>
                    icon(data: iconify_icon!("lucide:check"), size: Length::rem(1.0))
                    "Create Role"
                </button>
            </div>
        </dialog>
        if open {
            // Re-open the dialog after a refused attempt (a native modal
            // can only be opened from script).
            <script>"document.getElementById('role-create')?.showModal()"</script>
        }
    })
}

/// "Create Role": `POST /bff/roles`'s checks and use case. GET shows the
/// list with the dialog open; a refused POST shows it again with the
/// values and the error.
#[page([GET, POST] "/ui/(app)/authorization/roles/create")]
async fn create_role(cx: &Cx, form: Option<Form<CreateForm>>) -> Result<impl View> {
    let auth = auth(cx)?;
    permit(checks::can_administer_roles(
        auth,
        permissions::iam::ROLE_CREATE,
    ))?;
    let mut state = CreateState::default();
    if method(cx) == Method::POST {
        let form = form.map(|Form(f)| f).unwrap_or_default();
        let outcome = run_create(cx, auth, &form).await;
        match outcome {
            Ok(name) => {
                set_flash(cx, FlashKind::Success, "Role created successfully");
                return Err(see_other(detail_href(&name)).into());
            }
            Err(e) if e.status_code().is_client_error() => state.error = Some(e.to_string()),
            Err(e) => {
                tracing::error!(error = %e, "fc-web: create role failed");
                state.error = Some("Failed to create role".to_owned());
            }
        }
        state.form = form;
    }
    Ok(view! { role_list(open_name: String::new(), create: Some(state)) })
}

async fn run_create(
    cx: &Cx,
    auth: &AuthContext,
    form: &CreateForm,
) -> std::result::Result<String, PlatformError> {
    // The dialog sends no permissions; the ceiling check is the handler's.
    let permissions_requested: Vec<String> = Vec::new();
    ceiling::require_permissions(Some(auth), permissions_requested.iter().map(String::as_str))?;
    let display_name = if form.display_name.trim().is_empty() {
        form.name.clone()
    } else {
        form.display_name.clone()
    };
    let deps = crate::deps(cx);
    let event =
        CreateRoleUseCase::new(deps.auth_state.role_repo.clone(), deps.unit_of_work.clone())
            .run(
                CreateRoleCommand {
                    application_code: form.application_code.clone(),
                    role_name: form.name.clone(),
                    display_name,
                    description: Some(form.description.clone()).filter(|d| !d.trim().is_empty()),
                    permissions: permissions_requested,
                    client_managed: false,
                    source: RoleSource::Database,
                    // Owner ruling 15, as the BFF.
                    cross_application: auth.has_permission(permissions::ADMIN_ALL),
                },
                ExecutionContext::from_auth(auth),
            )
            .await
            .into_result()?;
    Ok(event.name)
}

// -------------------------------------------------------------- writes

/// Delete: `DELETE /bff/roles/{name}`'s checks (anchor + role:delete, then
/// the ceiling over every permission the role holds) and use case.
#[route(POST "/ui/(app)/authorization/roles/{name}/delete")]
async fn delete_role(cx: &Cx) -> Result<SeeOther> {
    let auth = auth(cx)?;
    permit(checks::can_administer_roles(
        auth,
        permissions::iam::ROLE_DELETE,
    ))?;
    let name = decode_segment(path_param::<Name>(cx));
    let role = find_role(cx, &name).await?.ok_or_not_found()?;
    let outcome: std::result::Result<(), PlatformError> = async {
        ceiling::require_permissions(Some(auth), role.permissions.iter().map(String::as_str))?;
        let deps = crate::deps(cx);
        DeleteRoleUseCase::new(deps.auth_state.role_repo.clone(), deps.unit_of_work.clone())
            .run(
                DeleteRoleCommand {
                    role_id: role.id.clone(),
                },
                ExecutionContext::from_auth(auth),
            )
            .await
            .into_result()?;
        Ok(())
    }
    .await;
    match outcome {
        Ok(()) => {
            set_flash(cx, FlashKind::Success, "Role deleted successfully");
            Ok(see_other(LIST))
        }
        Err(e) if e.status_code().is_client_error() => {
            set_flash(cx, FlashKind::Error, e.to_string());
            Ok(see_other(detail_href(&role.name)))
        }
        Err(e) => {
            tracing::error!(error = %e, "fc-web: delete role failed");
            set_flash(cx, FlashKind::Error, "Something went wrong. Try again.");
            Ok(see_other(detail_href(&role.name)))
        }
    }
}
