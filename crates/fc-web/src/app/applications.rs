//! `/ui/applications`: the SPA's `ApplicationListPage.vue` with its
//! drawers, `ApplicationDetailDrawer.vue` (`/ui/applications/{id}`,
//! `?edit=true` opens it editing) and `ApplicationCreateDrawer.vue`
//! (`/ui/applications/new`).
//!
//! Same pattern as `event_types.rs`. Reads need `can_read_applications`;
//! every write needs anchor scope plus the permission its API handler
//! checks (`application/api.rs`) and runs the same use case, or the same
//! orchestration for the multi-aggregate ones (delete, deactivate,
//! provisioning).
//!
//! Provisioning a service account or a CONFIDENTIAL login client returns a
//! secret that can only be read once: those POSTs render the page with the
//! credentials dialog open instead of redirecting, so the secret never
//! travels through a cookie or a URL.

use std::sync::Arc;

use fc_platform::application::api::{
    LoginClientCredentialsResponse, ProvisionLoginClientRequest, ServiceAccountCredentialsResponse,
    app_has_login_client, deactivate_application_cascade, delete_application_cascade,
    provision_application_login_client, provision_application_service_account,
};
use fc_platform::application::operations::{
    ActivateApplicationCommand, ActivateApplicationUseCase, CreateApplicationCommand,
    CreateApplicationUseCase, UpdateApplicationCommand, UpdateApplicationUseCase,
};
use fc_platform::auth::operations::CreateOAuthClientUseCase;
use fc_platform::usecase::UseCase;
use fc_platform::{
    Application, ApplicationType, AuthContext, ExecutionContext, PlatformError, checks, permissions,
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

use super::clients::local_date;
use crate::auth::{auth, permit, platform_error};
use crate::ui::drawer::DrawerSize;
use crate::ui::{
    Btn, FlashKind, Pager, Severity, confirm_dialog, detail_field, detail_value, drawer_frame,
    drawer_header, filter_select, form_field, list_query, local_time, page_header, paginator,
    set_flash, table_toolbar, tag,
};

path_param!(id);

const LIST: &str = "/ui/applications";
const FORM_ID: &str = "apps-list";

/// PrimeVue's outlined `severity="success"` button.
const SUCCESS_OUTLINE: &str =
    "fc-btn border-[#16a34a] bg-transparent text-[#16a34a] hover:bg-[#f0fdf4]";

fn detail_href(id: &str) -> String {
    format!("{LIST}/{id}")
}

/// The Type column's tag: Integration `info`, Application `primary`.
#[component]
async fn type_tag(application_type: ApplicationType) -> Result<impl View> {
    Ok(view! {
        match application_type {
            ApplicationType::Integration => tag(label: "Integration", severity: Severity::Info),
            ApplicationType::Application => <span class="fc-tag fc-tag-primary">"Application"</span>,
        }
    })
}

#[component]
async fn active_tag(active: bool) -> Result<impl View> {
    Ok(view! {
        if active {
            tag(label: "Active", severity: Severity::Success)
        } else {
            tag(label: "Inactive", severity: Severity::Secondary)
        }
    })
}

// ---------------------------------------------------------------- list

#[query_params(error = bad_request)]
#[derive(Clone, Default)]
struct ListQuery {
    q: Option<String>,
    r#type: Option<String>,
    active: Option<String>,
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
                ("type", Self::get(&self.r#type)),
                ("active", Self::get(&self.active)),
                ("rows", &rows),
                ("page", &page),
            ],
        )
    }
}

#[page("/ui/(app)/applications")]
async fn applications(cx: &Cx) -> Result<impl View> {
    permit(checks::can_read_applications(auth(cx)?))?;
    Ok(view! { application_list(open_id: String::new(), overlay: Overlay::None) })
}

#[page("/ui/(app)/applications/{id}")]
async fn application_detail(cx: &Cx) -> Result<impl View> {
    permit(checks::can_read_applications(auth(cx)?))?;
    let id = path_param::<Id>(cx).to_owned();
    Ok(view! { application_list(open_id: id, overlay: Overlay::None) })
}

/// What sits over the list besides the detail drawer.
#[derive(Clone)]
enum Overlay {
    None,
    Create(CreateState),
    ServiceAccount(String, ServiceAccountCredentials),
    LoginClient(String, LoginClientCredentials),
}

#[derive(Clone)]
struct ServiceAccountCredentials {
    client_id: String,
    client_secret: String,
    name: String,
}

#[derive(Clone)]
struct LoginClientCredentials {
    client_type: String,
    client_id: String,
    client_secret: Option<String>,
    redirect_uris: Vec<String>,
}

#[component]
async fn application_list(cx: &Cx, open_id: String, overlay: Overlay) -> Result<impl View> {
    let auth = auth(cx)?;
    let can_create =
        checks::require_anchor(auth).is_ok() && checks::can_write_applications(auth).is_ok();
    let query = query_params::<ListQuery>(cx)?;
    let search = ListQuery::get(&query.q).to_owned();
    let app_type = ListQuery::get(&query.r#type).to_owned();
    let active = ListQuery::get(&query.active).to_owned();

    // `list_applications`: every application, ordered by code.
    let mut all = crate::deps(cx)
        .application_repo
        .find_all()
        .await
        .map_err(platform_error)?;
    all.sort_by(|a, b| a.code.cmp(&b.code));
    let needle = search.to_lowercase();
    let rows: Vec<Application> = all
        .into_iter()
        .filter(|a| app_type.is_empty() || a.application_type.as_str() == app_type)
        .filter(|a| active.is_empty() || (if a.active { "ACTIVE" } else { "INACTIVE" }) == active)
        .filter(|a| {
            needle.is_empty()
                || a.code.to_lowercase().contains(&needle)
                || a.name.to_lowercase().contains(&needle)
                || a.description
                    .as_deref()
                    .is_some_and(|d| d.to_lowercase().contains(&needle))
        })
        .collect();
    let active_filters = [&app_type, &active]
        .iter()
        .filter(|f| !f.is_empty())
        .count();
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
    let create_href = q.href(&format!("{LIST}/new"), None);
    let hidden = query
        .rows
        .map(|r| vec![("rows".to_owned(), r.to_string())])
        .unwrap_or_default();

    Ok(view! {
        page_header(title: "Applications", subtitle: "Manage applications in the platform ecosystem",
            if can_create {
                <a href=(create_href) class=(Btn::Primary)>
                    icon(data: iconify_icon!("lucide:plus"), size: Length::rem(1.0))
                    "Create Application"
                </a>
            }
        )

        <div class="fc-card">
            <div class="-m-4 mb-0">
                table_toolbar(
                    form_id: FORM_ID,
                    action: LIST,
                    placeholder: "Search applications...",
                    search: Some(search.clone()),
                    active_filter_count: active_filters,
                    has_active_filters: has_active,
                    show_filters: true,
                    show_search: true,
                    hidden: hidden,
                    filter_select(
                        name: "type",
                        label: "Type",
                        placeholder: "All types",
                        options: vec![("APPLICATION".to_owned(), "Application".to_owned()), ("INTEGRATION".to_owned(), "Integration".to_owned())],
                        selected: (!app_type.is_empty()).then(|| app_type.clone()),
                    )
                    filter_select(
                        name: "active",
                        label: "Status",
                        placeholder: "All statuses",
                        options: vec![("ACTIVE".to_owned(), "Active".to_owned()), ("INACTIVE".to_owned(), "Inactive".to_owned())],
                        selected: (!active.is_empty()).then(|| active.clone()),
                    )
                )
            </div>
            <table class="fc-table fc-table-striped">
                <thead>
                    <tr>
                        <th>"Code"</th>
                        <th>"Name"</th>
                        <th>"Type"</th>
                        <th>"Description"</th>
                        <th>"Status"</th>
                        <th>"Created"</th>
                        <th class="w-[80px]">"Actions"</th>
                    </tr>
                </thead>
                <tbody>
                    if rows.is_empty() {
                        <tr><td colspan="7">"No applications found"</td></tr>
                    }
                    for app in &rows {
                        let id = app.id.clone();
                        let edit_id = app.id.clone();
                        <tr class="fc-row-link" @click=$(|_e: Event| { selected.set(id.clone()); editing.set(false) })>
                            <td>
                                <a href=(detail_href(&app.id)) onclick="event.preventDefault()" class="no-underline">
                                    <code class="rounded bg-[#f1f5f9] px-2 py-0.5 text-[13px] text-[#1e293b]">(&app.code)</code>
                                </a>
                            </td>
                            <td>(&app.name)</td>
                            <td>type_tag(application_type: app.application_type)</td>
                            <td class="text-[14px] text-[#64748b]">(app.description.clone().filter(|d| !d.is_empty()).unwrap_or_else(|| "—".to_owned()))</td>
                            <td>active_tag(active: app.active)</td>
                            <td>local_date(at: app.created_at)</td>
                            <td>
                                <a
                                    href=(format!("{}?edit=true", detail_href(&app.id)))
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
            <div class="-m-4 mt-0">
                paginator(pager: pager, noun: "applications", href: page_href, form_id: FORM_ID)
            </div>
        </div>

        drawer_frame(selected: frame_selected, label: "Application", size: DrawerSize::Wide,
            application_drawer(id: $(selected.get()), editing: drawer_editing)
        )

        match overlay {
            Overlay::None => {}
            Overlay::Create(state) => create_drawer(state: state, close_href: close_href),
            Overlay::ServiceAccount(app_id, creds) => service_account_dialog(done_href: detail_href(&app_id), creds: creds),
            Overlay::LoginClient(app_id, creds) => login_client_dialog(done_href: detail_href(&app_id), creds: creds),
        }
    })
}

// -------------------------------------------------------------- drawer

#[shard("/ui/(app)/applications/drawer")]
async fn application_drawer(cx: &Cx, id: String, editing: Signal<bool>) -> Result<impl View> {
    let auth = auth(cx)?;
    permit(checks::can_read_applications(auth))?;
    let loaded = if id.is_empty() {
        None
    } else {
        // `get_application`, with `hasLoginClient`.
        let deps = crate::deps(cx);
        let app = deps
            .application_repo
            .find_by_id(&id)
            .await
            .map_err(platform_error)?
            .ok_or_not_found()?;
        let has_login_client = app_has_login_client(&deps.oauth_client_repo, &app.id)
            .await
            .map_err(platform_error)?;
        Some((app, has_login_client))
    };

    Ok(view! {
        if let Some((app, has_login_client)) = loaded {
            drawer_body(auth: auth.clone(), app: app, has_login_client: has_login_client, editing: editing)
        }
    })
}

#[component]
async fn drawer_body(
    auth: AuthContext,
    app: Application,
    has_login_client: bool,
    editing: Signal<bool>,
) -> Result<impl View> {
    let anchor = checks::require_anchor(&auth).is_ok();
    let can_write = anchor && checks::can_write_applications(&auth).is_ok();
    let can_delete = anchor && checks::can_delete_applications(&auth).is_ok();
    let can_update = auth.has_permission(permissions::admin::APPLICATION_UPDATE);
    let can_provision_sa =
        anchor && can_update && auth.has_permission(permissions::admin::SERVICE_ACCOUNT_CREATE);
    let can_provision_login =
        anchor && can_update && auth.has_permission(permissions::auth::OAUTH_CLIENT_CREATE);
    let base = detail_href(&app.id);
    let form_id = "app-edit-form";
    let (start, discard) = (editing.clone(), editing.clone());
    let logo = app.logo.as_ref().map(|_| {
        app.logo_mime_type
            .clone()
            .unwrap_or_else(|| "Configured".to_owned())
    });

    Ok(view! {
        drawer_header(title: app.name.clone(), subtitle: Some(app.code.clone()),
            active_tag(active: app.active)
        )

        <div class="fc-drawer-body">
            <section class="fc-form-section">
                <header class="fc-section-header">
                    <h3 class="fc-section-title">"Application Details"</h3>
                    if can_write {
                        <button type="button" class=(Btn::TextPrimary) :hidden=$(editing.get()) @click=$(|_e: Event| start.set(true))>
                            icon(data: iconify_icon!("lucide:pencil"), size: Length::rem(1.0))
                            "Edit"
                        </button>
                    }
                </header>
                <div class="fc-section-body">
                    <div class="fc-detail-grid" :hidden=$(editing.get())>
                        detail_field(label: "Code", <code>(&app.code)</code>)
                        detail_value(label: "Name", value: Some(app.name.clone()))
                        detail_value(label: "Description", value: app.description.clone(), span: true)
                        detail_value(label: "Default Base URL", value: app.default_base_url.clone())
                        detail_value(label: "Icon URL", value: app.icon_url.clone())
                        detail_value(label: "Website", value: app.website.clone())
                        detail_value(label: "Logo", value: logo)
                        detail_field(label: "Created", local_time(at: app.created_at))
                        detail_field(label: "Updated", local_time(at: app.updated_at))
                    </div>
                    if can_write {
                        // Website and logo are not in the platform's update
                        // command (`UpdateApplicationCommand`), so they are
                        // not offered here.
                        <form id=(form_id) method="post" action=(format!("{base}/update")) class="fc-form-grid" data-dirty-form="" data-dirty-key=(&app.id) :hidden=$(!editing.get())>
                            form_field(label: "Name", for_id: "app-name", span: true,
                                <input id="app-name" name="name" class="fc-input" value=(&app.name) required="" maxlength="100">
                            )
                            form_field(label: "Description", for_id: "app-description", span: true,
                                <textarea id="app-description" name="description" class="fc-input" rows="3">(app.description.clone().unwrap_or_default())</textarea>
                            )
                            form_field(label: "Default Base URL", for_id: "app-base-url", span: true,
                                <input id="app-base-url" name="default_base_url" class="fc-input" value=(app.default_base_url.clone().unwrap_or_default()) placeholder="https://example.com">
                            )
                            form_field(label: "Icon URL", for_id: "app-icon-url",
                                <input id="app-icon-url" name="icon_url" class="fc-input" value=(app.icon_url.clone().unwrap_or_default()) placeholder="https://example.com/icon.png">
                            )
                        </form>
                    }
                </div>
            </section>

            <section class="fc-form-section">
                <header class="fc-section-header"><h3 class="fc-section-title">"Service Account"</h3></header>
                <div class="fc-section-body">
                    if let Some(sa_id) = app.service_account_id.clone() {
                        <div class="fc-detail-grid">
                            detail_field(label: "Status", tag(label: "Provisioned", severity: Severity::Success))
                            detail_field(label: "Principal ID", <code>(sa_id)</code>)
                        </div>
                        info_message(text: "Service account credentials are managed in the OAuth Clients section. The client secret can only be viewed at creation time or when rotated.")
                    } else {
                        <div class="fc-danger-item">
                            <div>
                                <strong>"Provision Service Account"</strong>
                                <p>"Create a service account with OAuth credentials for machine-to-machine authentication."</p>
                            </div>
                            if can_provision_sa {
                                <form method="post" action=(format!("{base}/provision-service-account"))>
                                    <button type="submit" class=(Btn::Primary)>
                                        icon(data: iconify_icon!("lucide:plus"), size: Length::rem(1.0))
                                        "Provision"
                                    </button>
                                </form>
                            }
                        </div>
                    }
                </div>
            </section>

            <section class="fc-form-section">
                <header class="fc-section-header"><h3 class="fc-section-title">"Login Client"</h3></header>
                <div class="fc-section-body">
                    if has_login_client {
                        <div class="fc-detail-grid">
                            detail_field(label: "Status", tag(label: "Provisioned", severity: Severity::Success))
                        </div>
                        info_message(text: "Login client settings (redirect URIs, allowed origins, secret rotation) are managed in the OAuth Clients section.")
                    } else {
                        <form method="post" action=(format!("{base}/provision-login-client")) class="flex flex-col gap-4">
                            <div>
                                <strong class="mb-1 block">"Provision Login Client"</strong>
                                <p class="text-[13px] text-[#64748b]">"Create an OAuth client for user authentication via OIDC (authorization_code grant). Required if your application has a UI that users log into."</p>
                            </div>
                            form_field(label: "Client Type", for_id: "login-client-type",
                                <select id="login-client-type" name="client_type" class="fc-select" disabled=(!can_provision_login)>
                                    <option value="PUBLIC" selected="">"PUBLIC — SPA / native app (PKCE only)"</option>
                                    <option value="CONFIDENTIAL">"CONFIDENTIAL — server-rendered app (has client secret)"</option>
                                </select>
                            )
                            form_field(label: "Redirect URIs", for_id: "login-redirect-uris", required: true, help: Some("Allowed callback URLs for OAuth redirects, one per line. Add at least one to provision.".to_owned()),
                                <textarea id="login-redirect-uris" name="redirect_uris" class="fc-input font-mono" rows="3" placeholder="https://app.example.com/callback" required="" disabled=(!can_provision_login)></textarea>
                            )
                            if can_provision_login {
                                <div>
                                    <button type="submit" class=(Btn::Primary)>
                                        icon(data: iconify_icon!("lucide:plus"), size: Length::rem(1.0))
                                        "Provision Login Client"
                                    </button>
                                </div>
                            }
                        </form>
                    }
                </div>
            </section>

            <section class="fc-form-section" :hidden=$(editing.get())>
                <header class="fc-section-header"><h3 class="fc-section-title">"Actions"</h3></header>
                <div class="fc-danger-actions">
                    if !app.active {
                        <div class="fc-danger-item">
                            <div>
                                <strong>"Activate Application"</strong>
                                <p>"Make this application available for use."</p>
                            </div>
                            <button type="button" class=(SUCCESS_OUTLINE) disabled=(!can_write) commandfor="app-activate" command="show-modal">"Activate"</button>
                        </div>
                        confirm_dialog(
                            id: "app-activate",
                            action: format!("{base}/activate"),
                            title: "Activate Application",
                            message: "Activate this application?",
                            confirm_label: "Activate",
                        )
                    } else {
                        <div class="fc-danger-item">
                            <div>
                                <strong>"Deactivate Application"</strong>
                                <p>"Prevent new event types from using this application."</p>
                            </div>
                            <button type="button" class=(Btn::WarnOutline) disabled=(!can_write) commandfor="app-deactivate" command="show-modal">"Deactivate"</button>
                        </div>
                        confirm_dialog(
                            id: "app-deactivate",
                            action: format!("{base}/deactivate"),
                            title: "Deactivate Application",
                            message: "Deactivate this application? It will no longer be available for new event types.",
                            confirm_label: "Deactivate",
                            warn: true,
                        )
                    }
                    <div class="fc-danger-item">
                        <div>
                            <strong>"Delete Application"</strong>
                            <p>"Permanently delete this application. Cannot be undone."</p>
                        </div>
                        <button type="button" class=(Btn::DangerOutline) disabled=(app.active || !can_delete) commandfor="app-delete" command="show-modal">"Delete"</button>
                    </div>
                    confirm_dialog(
                        id: "app-delete",
                        action: format!("{base}/delete"),
                        title: "Delete Application",
                        message: "Delete this application? This cannot be undone.",
                        confirm_label: "Delete",
                        danger: true,
                    )
                </div>
            </section>
        </div>

        if can_write {
            <footer class="fc-drawer-footer" :hidden=$(!editing.get())>
                <button type="reset" form=(form_id) class=(Btn::Outline) hidden="" data-dirty-discard="" @click=$(|_e: Event| discard.set(false))>"Discard"</button>
                <button type="submit" form=(form_id) class=(Btn::Primary) disabled="" data-dirty-save="">"Save"</button>
            </footer>
        }
    })
}

/// PrimeVue `Message severity="info"`.
#[component]
async fn info_message(#[into] text: String) -> Result<impl View> {
    Ok(view! {
        <div class="mt-3 flex items-start gap-2 rounded-md border border-[#bae6fd] bg-[#f0f9ff] px-3 py-2.5 text-[14px] text-[#0369a1]" role="note">
            icon(data: iconify_icon!("lucide:info"), size: Length::rem(1.1), attrs: topcoat::view::attributes! { class="mt-0.5 shrink-0" })
            <span>(text)</span>
        </div>
    })
}

// ------------------------------------------------------- credentials

/// A credential row with a copy button.
#[component]
async fn credential(
    #[into] label: String,
    #[into] value: String,
    #[default] copy: bool,
) -> Result<impl View> {
    Ok(view! {
        <div class="flex flex-col gap-1">
            <span class="fc-field-label">(label)</span>
            <div class="flex items-center gap-2 rounded-md bg-[#f8fafc] px-3 py-2">
                <code class="break-all">(value.clone())</code>
                if copy {
                    <button type="button" class="fc-icon-btn ml-auto h-7 w-7 shrink-0 text-[#059669]" title="Copy" data-copy=(value) onclick="navigator.clipboard.writeText(this.dataset.copy)">
                        icon(data: iconify_icon!("lucide:copy"), size: Length::rem(0.9))
                    </button>
                }
            </div>
        </div>
    })
}

/// "Service Account Provisioned": shown once, not closable except by
/// acknowledging, as the SPA's dialog.
#[component]
async fn service_account_dialog(
    done_href: String,
    creds: ServiceAccountCredentials,
) -> Result<impl View> {
    Ok(view! {
        <dialog id="app-credentials" class="fc-dialog w-[550px] max-w-[calc(100vw-2rem)]" data-open-on-load="" aria-labelledby="app-credentials-title">
            <div class="fc-dialog-header"><span id="app-credentials-title">"Service Account Provisioned"</span></div>
            <div class="fc-dialog-body flex flex-col gap-4">
                <div class="rounded-md border border-[#fed7aa] bg-[#fff7ed] px-3 py-2.5 text-[14px] text-[#c2410c]" role="alert">
                    "Save these credentials now. The client secret will not be shown again."
                </div>
                credential(label: "Client ID", value: creds.client_id, copy: true)
                credential(label: "Client Secret", value: creds.client_secret, copy: true)
                credential(label: "Service Account", value: creds.name)
            </div>
            <div class="fc-dialog-footer">
                <a href=(done_href) class=(Btn::Primary)>
                    icon(data: iconify_icon!("lucide:check"), size: Length::rem(1.0))
                    "I've saved the credentials"
                </a>
            </div>
        </dialog>
    })
}

#[component]
async fn login_client_dialog(
    done_href: String,
    creds: LoginClientCredentials,
) -> Result<impl View> {
    let confidential = creds.client_type == "CONFIDENTIAL";
    Ok(view! {
        <dialog id="app-credentials" class="fc-dialog w-[550px] max-w-[calc(100vw-2rem)]" data-open-on-load="" aria-labelledby="app-credentials-title">
            <div class="fc-dialog-header"><span id="app-credentials-title">"Login Client Provisioned"</span></div>
            <div class="fc-dialog-body flex flex-col gap-4">
                if confidential {
                    <div class="rounded-md border border-[#fed7aa] bg-[#fff7ed] px-3 py-2.5 text-[14px] text-[#c2410c]" role="alert">
                        "Save these credentials now. The client secret will not be shown again."
                    </div>
                } else {
                    info_message(text: "PUBLIC clients use PKCE — there is no client secret. Configure your app with the client ID below.")
                }
                credential(label: "Client ID", value: creds.client_id, copy: true)
                if let Some(secret) = creds.client_secret {
                    credential(label: "Client Secret", value: secret, copy: true)
                }
                credential(label: "Client Type", value: creds.client_type.clone())
                credential(label: "Redirect URIs", value: creds.redirect_uris.join(", "))
            </div>
            <div class="fc-dialog-footer">
                <a href=(done_href) class=(Btn::Primary)>
                    icon(data: iconify_icon!("lucide:check"), size: Length::rem(1.0))
                    "I've saved the credentials"
                </a>
            </div>
        </dialog>
    })
}

// -------------------------------------------------------------- create

#[derive(Clone, Default, Deserialize)]
struct CreateForm {
    #[serde(default)]
    r#type: String,
    #[serde(default)]
    code: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    default_base_url: String,
    #[serde(default)]
    icon_url: String,
}

#[derive(Clone, Default)]
struct CreateState {
    form: CreateForm,
    error: Option<String>,
}

/// `CODE_PATTERN` (`^[a-z][a-z0-9-]*$`).
fn valid_code(s: &str) -> bool {
    s.starts_with(|c: char| c.is_ascii_lowercase())
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn non_empty(s: &str) -> Option<String> {
    Some(s.trim().to_owned()).filter(|v| !v.is_empty())
}

/// `ApplicationCreateDrawer.vue`: `CreateApplicationUseCase`, as `POST
/// /api/applications` (anchor + `can_write_applications`).
#[page([GET, POST] "/ui/(app)/applications/new")]
async fn create_application(cx: &Cx, form: Option<Form<CreateForm>>) -> Result<impl View> {
    let auth = auth(cx)?;
    permit(checks::require_anchor(auth))?;
    permit(checks::can_write_applications(auth))?;
    let deps = crate::deps(cx);
    let mut state = CreateState::default();

    if method(cx) == Method::POST {
        let form = form.map(|Form(f)| f).unwrap_or_default();
        let application_type = match form.r#type.as_str() {
            "INTEGRATION" => ApplicationType::Integration,
            _ => ApplicationType::Application,
        };
        let outcome = if !valid_code(&form.code) {
            Err(PlatformError::validation(
                "Code must start with a letter, use only lowercase letters, numbers, and hyphens",
            ))
        } else if form.name.trim().is_empty() || form.name.len() > 100 {
            Err(PlatformError::validation(
                "Name is required (at most 100 characters)",
            ))
        } else {
            CreateApplicationUseCase::new(deps.application_repo.clone(), deps.unit_of_work.clone())
                .run(
                    CreateApplicationCommand {
                        code: form.code.clone(),
                        name: form.name.clone(),
                        description: non_empty(&form.description),
                        application_type: Some(application_type),
                        default_base_url: non_empty(&form.default_base_url),
                        icon_url: non_empty(&form.icon_url),
                        website: None,
                        logo: None,
                        logo_mime_type: None,
                    },
                    ExecutionContext::from_auth(auth),
                )
                .await
                .into_result()
                .map(|event| event.application_id)
                .map_err(PlatformError::from)
        };
        match outcome {
            Ok(id) => {
                set_flash(cx, FlashKind::Success, "Application created");
                return Err(see_other(detail_href(&id)).into());
            }
            Err(e) if e.status_code().is_client_error() => state.error = Some(e.to_string()),
            Err(e) => {
                tracing::error!(error = %e, "fc-web: create application failed");
                state.error = Some("Failed to create application".to_owned());
            }
        }
        state.form = form;
    }

    Ok(view! { application_list(open_id: String::new(), overlay: Overlay::Create(state)) })
}

#[component]
async fn create_drawer(state: CreateState, close_href: String) -> Result<impl View> {
    let f = state.form;
    let integration = f.r#type == "INTEGRATION";
    Ok(view! {
        <aside class="fc-drawer" role="complementary" aria-label="Create application" data-drawer="">
            <a href=(&close_href) class="fc-icon-btn absolute top-[1.1rem] right-[1.1rem] z-10" aria-label="Close" data-drawer-close="">
                icon(data: iconify_icon!("lucide:x"), size: Length::rem(1.2))
            </a>
            drawer_header(title: "Create Application", subtitle: Some("Add a new application to the platform".to_owned()))
            <form id="app-create-form" method="post" action=(format!("{LIST}/new")) class="fc-drawer-body">
                <section class="fc-form-section">
                    <header class="fc-section-header"><h3 class="fc-section-title">"Application Identity"</h3></header>
                    <div class="flex flex-col gap-4">
                        <div class="fc-form-field">
                            <span class="fc-field-label">"Type " <span class="fc-required">"*"</span></span>
                            // PrimeVue `SelectButton`: a segmented pair.
                            <div class="inline-flex overflow-hidden rounded-[2px] border border-[#64748b]" role="radiogroup" aria-label="Type">
                                <label class="cursor-pointer px-3 py-1.5 has-[:checked]:bg-[#059669] has-[:checked]:text-white">
                                    <input type="radio" name="type" value="APPLICATION" class="sr-only" checked=(!integration)>
                                    "Application"
                                </label>
                                <label class="cursor-pointer border-l border-[#64748b] px-3 py-1.5 has-[:checked]:bg-[#059669] has-[:checked]:text-white">
                                    <input type="radio" name="type" value="INTEGRATION" class="sr-only" checked=(integration)>
                                    "Integration"
                                </label>
                            </div>
                            <small class="fc-field-help">"Application: user-facing application that users can log into. Integration: third-party adapter or connector for integrations."</small>
                        </div>
                        form_field(label: "Code", for_id: "app-new-code", required: true, help: Some("Unique identifier for the application. Cannot be changed after creation.".to_owned()),
                            <input
                                id="app-new-code" name="code" class="fc-input" value=(f.code.clone()) placeholder="e.g., operant" required=""
                                pattern="[a-z][a-z0-9\\-]*" title="Must start with a letter, use only lowercase letters, numbers, and hyphens"
                            >
                        )
                        form_field(label: "Name", for_id: "app-new-name", required: true, help: Some("At most 100 characters".to_owned()),
                            <input id="app-new-name" name="name" class="fc-input" value=(f.name.clone()) placeholder="Human-friendly name" required="" maxlength="100">
                        )
                        form_field(label: "Description", for_id: "app-new-description",
                            <textarea id="app-new-description" name="description" class="fc-input" rows="3" placeholder="Optional description">(f.description.clone())</textarea>
                        )
                    </div>
                </section>
                <section class="fc-form-section">
                    <header class="fc-section-header"><h3 class="fc-section-title">"Configuration"</h3></header>
                    <div class="flex flex-col gap-4">
                        form_field(label: "Default Base URL", for_id: "app-new-base-url", help: Some("Base URL for API calls to this application".to_owned()),
                            <input id="app-new-base-url" name="default_base_url" class="fc-input" value=(f.default_base_url.clone()) placeholder="https://example.com">
                        )
                        form_field(label: "Icon URL", for_id: "app-new-icon-url", help: Some("URL to the application's icon image".to_owned()),
                            <input id="app-new-icon-url" name="icon_url" class="fc-input" value=(f.icon_url.clone()) placeholder="https://example.com/icon.png">
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
                <button type="submit" form="app-create-form" class=(Btn::Primary)>
                    icon(data: iconify_icon!("lucide:check"), size: Length::rem(1.0))
                    "Create Application"
                </button>
            </footer>
        </aside>
    })
}

// -------------------------------------------------------------- writes

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
            tracing::error!(error = %e, "fc-web: application write failed");
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
    #[serde(default)]
    description: String,
    #[serde(default)]
    default_base_url: String,
    #[serde(default)]
    icon_url: String,
}

/// `PUT /api/applications/{id}`. The SPA sends empty fields as absent.
#[route(POST "/ui/(app)/applications/{id}/update")]
async fn update(cx: &Cx, Form(form): Form<UpdateForm>) -> Result<SeeOther> {
    let auth = auth(cx)?;
    permit(checks::require_anchor(auth))?;
    permit(checks::can_write_applications(auth))?;
    let id = target(cx);
    let deps = crate::deps(cx);
    let outcome =
        UpdateApplicationUseCase::new(deps.application_repo.clone(), deps.unit_of_work.clone())
            .run(
                UpdateApplicationCommand {
                    id: id.clone(),
                    name: Some(form.name),
                    description: non_empty(&form.description),
                    default_base_url: non_empty(&form.default_base_url),
                    icon_url: non_empty(&form.icon_url),
                    website: None,
                    logo: None,
                    logo_mime_type: None,
                },
                ExecutionContext::from_auth(auth),
            )
            .await
            .into_result()
            .map(|_| ())
            .map_err(PlatformError::from);
    let base = detail_href(&id);
    let retry = format!("{base}?edit=true");
    finish(cx, outcome, "Application updated", base, retry)
}

/// `POST /api/applications/{id}/activate`.
#[route(POST "/ui/(app)/applications/{id}/activate")]
async fn activate(cx: &Cx) -> Result<SeeOther> {
    let auth = auth(cx)?;
    permit(checks::require_anchor(auth))?;
    permit(checks::can_write_applications(auth))?;
    let id = target(cx);
    let deps = crate::deps(cx);
    let outcome =
        ActivateApplicationUseCase::new(deps.application_repo.clone(), deps.unit_of_work.clone())
            .run(
                ActivateApplicationCommand { id: id.clone() },
                ExecutionContext::from_auth(auth),
            )
            .await
            .into_result()
            .map(|_| ())
            .map_err(PlatformError::from);
    let base = detail_href(&id);
    finish(cx, outcome, "Application activated", base.clone(), base)
}

/// `POST /api/applications/{id}/deactivate`: the application, its
/// service accounts and their OAuth clients, in one transaction.
#[route(POST "/ui/(app)/applications/{id}/deactivate")]
async fn deactivate(cx: &Cx) -> Result<SeeOther> {
    let auth = auth(cx)?;
    permit(checks::require_anchor(auth))?;
    permit(checks::can_write_applications(auth))?;
    let id = target(cx);
    let deps = crate::deps(cx);
    let outcome = deactivate_application_cascade(
        &deps.unit_of_work,
        &deps.service_account_repo,
        &deps.oauth_client_repo,
        &deps.application_repo,
        &id,
        &auth.principal_id,
    )
    .await;
    let base = detail_href(&id);
    finish(cx, outcome, "Application deactivated", base.clone(), base)
}

/// `DELETE /api/applications/{id}`: with its service accounts, in one
/// transaction. The drawer closes, as the SPA's.
#[route(POST "/ui/(app)/applications/{id}/delete")]
async fn delete(cx: &Cx) -> Result<SeeOther> {
    let auth = auth(cx)?;
    permit(checks::require_anchor(auth))?;
    permit(checks::can_delete_applications(auth))?;
    let id = target(cx);
    let deps = crate::deps(cx);
    let outcome = delete_application_cascade(
        &deps.unit_of_work,
        &deps.service_account_repo,
        &deps.application_repo,
        &id,
        &auth.principal_id,
    )
    .await;
    finish(
        cx,
        outcome,
        "Application deleted",
        LIST.to_owned(),
        detail_href(&id),
    )
}

/// The provisioning permissions of `application/api.rs` (after its anchor
/// check): what the operation creates, and application update.
fn permit_provision(auth: &AuthContext, create_permission: &str) -> Result<()> {
    permit(checks::require_permission(auth, create_permission))?;
    permit(checks::require_permission(
        auth,
        permissions::admin::APPLICATION_UPDATE,
    ))
}

/// A refused provisioning: back to the drawer with the reason.
fn provision_refused(cx: &Cx, id: &str, e: PlatformError) -> topcoat::Error {
    if e.status_code().is_client_error() {
        set_flash(cx, FlashKind::Error, e.to_string());
    } else {
        tracing::error!(error = %e, "fc-web: provisioning failed");
        set_flash(cx, FlashKind::Error, "Something went wrong. Try again.");
    }
    see_other(detail_href(id)).into()
}

/// `POST /api/applications/{id}/provision-service-account`. Renders the
/// page with the credentials dialog (the secret is shown once).
#[page(POST "/ui/(app)/applications/{id}/provision-service-account")]
async fn provision_service_account(cx: &Cx) -> Result<impl View> {
    let auth = auth(cx)?;
    permit(checks::require_anchor(auth))?;
    permit_provision(auth, permissions::admin::SERVICE_ACCOUNT_CREATE)?;
    let id = target(cx);
    let deps = crate::deps(cx);
    let creds: ServiceAccountCredentialsResponse = provision_application_service_account(
        &deps.unit_of_work,
        &deps.application_repo,
        &deps.service_account_repo,
        &deps.client_repo,
        &deps.oauth_client_repo,
        &id,
        &auth.principal_id,
    )
    .await
    .map_err(|e| provision_refused(cx, &id, e))?;
    let creds = ServiceAccountCredentials {
        client_id: creds.oauth_client.client_id,
        client_secret: creds.oauth_client.client_secret.unwrap_or_default(),
        name: creds.name,
    };
    Ok(view! { application_list(open_id: id.clone(), overlay: Overlay::ServiceAccount(id, creds)) })
}

#[derive(Deserialize)]
struct LoginClientForm {
    #[serde(default)]
    client_type: String,
    #[serde(default)]
    redirect_uris: String,
}

/// `POST /api/applications/{id}/provision-login-client`. Renders the page
/// with the credentials dialog (a CONFIDENTIAL secret is shown once).
#[page(POST "/ui/(app)/applications/{id}/provision-login-client")]
async fn provision_login_client(cx: &Cx, Form(form): Form<LoginClientForm>) -> Result<impl View> {
    let auth = auth(cx)?;
    permit(checks::require_anchor(auth))?;
    permit_provision(auth, permissions::auth::OAUTH_CLIENT_CREATE)?;
    let id = target(cx);
    let deps = crate::deps(cx);
    // The SPA's chip list: one URI per line here, de-duplicated.
    let mut redirect_uris: Vec<String> = Vec::new();
    for uri in form
        .redirect_uris
        .lines()
        .map(str::trim)
        .filter(|u| !u.is_empty())
    {
        if !redirect_uris.iter().any(|u| u == uri) {
            redirect_uris.push(uri.to_owned());
        }
    }
    let creds: LoginClientCredentialsResponse = provision_application_login_client(
        &CreateOAuthClientUseCase::new(deps.oauth_client_repo.clone(), deps.unit_of_work.clone()),
        &deps.application_repo,
        &deps.oauth_client_repo,
        &id,
        &auth.principal_id,
        ProvisionLoginClientRequest {
            client_type: non_empty(&form.client_type),
            redirect_uris,
            allowed_origins: Vec::new(),
        },
    )
    .await
    .map_err(|e| provision_refused(cx, &id, e))?;
    let creds = LoginClientCredentials {
        client_type: creds.client_type,
        client_id: creds.oauth_client.client_id,
        client_secret: creds.oauth_client.client_secret,
        redirect_uris: creds.redirect_uris,
    };
    Ok(view! { application_list(open_id: id.clone(), overlay: Overlay::LoginClient(id, creds)) })
}
