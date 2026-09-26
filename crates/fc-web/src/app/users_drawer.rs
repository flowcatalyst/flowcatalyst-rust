//! The user drawer (`UserDetailDrawer.vue` + `UserDetailBody.vue`) and
//! every write it makes. See `users.rs` for the page around it.
//!
//! Sections, as the SPA: User Information (read view, or the edit form:
//! name, and for an anchor the tier and client), Client Access (anchor
//! callers; home client, grants, grant dialog, revoke), Roles (the
//! assignments and the Manage Roles picker), Application Access (the
//! all-applications switch, the grants and the picker), Account Actions
//! (password reset email, direct password reset, two-factor reset,
//! developer credential, activate / deactivate, delete).
//!
//! Each write is a form POST to a `#[route]` here that calls the
//! principal API's handler body with the caller's `AuthContext`, then
//! redirects back to the drawer with a flash. Two answers render instead
//! of redirecting: a direct password reset the platform refuses reopens
//! its dialog with the reason, and a new developer secret is shown once in
//! a dialog (never through a cookie or a URL).

use std::collections::{HashMap, HashSet};

use fc_platform::developer_credential::DEVELOPER_ROLE;
use fc_platform::developer_credential::api::{revoke_credential, set_credential};
use fc_platform::mfa::admin_api::reset_user_two_factor;
use fc_platform::principal::admin;
use fc_platform::principal::api::{
    ResetPasswordRequest, SetApplicationAccessRequest, UpdatePrincipalRequest,
};
use fc_platform::principal::go_api::{ClientAssociationRequest, client_association};
use fc_platform::{AuthContext, PlatformError, checks};
use serde::Deserialize;
use topcoat::{
    Result,
    context::Cx,
    icon::{icon, iconify::iconify_icon},
    router::{
        content::Form,
        error::{SeeOther, see_other},
        page, path_param, route,
    },
    runtime::{Event, Signal, shard, signal},
    view::{Length, View, attributes, class, component, view},
};

use super::users::{
    LIST, client_names, detail_href, finish, local_date, reachable_clients, status_tag, tag,
    user_list, user_type,
};
use crate::auth::{auth, permit, platform_error};
use crate::components::alert::{AlertVariant, alert, alert_description};
use crate::components::alert_dialog::alert_dialog;
use crate::components::badge::{BadgeVariant, badge};
use crate::components::button::{ButtonSize, ButtonVariant, button, button_variants};
use crate::components::checkbox::checkbox;
use crate::components::dialog::{
    dialog, dialog_content, dialog_description, dialog_footer, dialog_header, dialog_title,
};
use crate::components::field::{
    FieldOrientation, field, field_content, field_description, field_label, field_title,
};
use crate::components::input::input;
use crate::components::select::select;
use crate::components::switch::switch;
use crate::components::table::{
    table, table_body, table_cell, table_head, table_header, table_row,
};
use crate::ui::{FlashKind, set_flash};

path_param!(id);
path_param!(client);

pub(crate) fn target(cx: &Cx) -> String {
    path_param::<Id>(cx).to_owned()
}

/// A modal dialog's overlay: the SPA's dark mask instead of Topcoat's
/// frosted page colour. Opened with `commandfor` + `command="show-modal"`
/// (focus trap and Escape come from the browser); a click on the mask
/// closes it (`data-light-dismiss`, `ui.js`).
pub(crate) const MODAL: &str =
    "[&]:w-screen [&]:bg-[rgb(15_23_42/0.4)] [&]:backdrop-blur-none backdrop:bg-transparent";

// ------------------------------------------------------------- overlays

/// A one-time value the page shows in a dialog when it loads.
#[derive(Clone)]
pub(crate) enum Overlay {
    /// A new developer client secret.
    Secret(DeveloperSecret),
    /// A create-user answer's set-password link.
    Invite(InviteLink),
}

#[derive(Clone)]
pub(crate) struct DeveloperSecret {
    pub client_id: String,
    pub secret: String,
}

#[derive(Clone)]
pub(crate) struct InviteLink {
    pub email: String,
    pub link: String,
}

/// The one-time dialog, opened on load. Like the SPA's credential dialogs,
/// only its own button closes it.
#[component]
pub(crate) async fn overlay_dialog(overlay: Overlay) -> Result<impl View> {
    let (title, note, rows) = match overlay {
        Overlay::Secret(s) => (
            "Developer Credential",
            "Save this secret now. It will not be shown again.",
            vec![("Client ID", s.client_id), ("Client Secret", s.secret)],
        ),
        Overlay::Invite(i) => (
            "Invite Link",
            "Send this link to the user. It sets their password and expires in 72 hours; it will not be shown again.",
            vec![("Email", i.email), ("Set-password link", i.link)],
        ),
    };
    Ok(view! {
        dialog(open: false, attrs: attributes! {
            id="user-once" data-open-on-load="" aria-labelledby="user-once-title" class=(MODAL)
        },
            dialog_content(attrs: attributes! { class="[&]:max-w-[550px]" },
                dialog_header(dialog_title(attrs: attributes! { id="user-once-title" }, (title)))
                alert(attrs: attributes! { class="border-[#fed7aa] bg-[#fff7ed] text-[#c2410c]" },
                    icon(data: iconify_icon!("lucide:triangle-alert"))
                    alert_description(attrs: attributes! { class="[&]:text-[#c2410c]" }, (note))
                )
                for (label, value) in rows {
                    field(
                        field_title((label))
                        <div class="flex items-center gap-2">
                            input(attrs: attributes! { value=(&value) readonly="" class="font-mono" })
                            button(variant: ButtonVariant::Outline, size: ButtonSize::Icon, attrs: attributes! {
                                type="button" aria-label=(format!("Copy {label}")) data-copy=(&value)
                                onclick="navigator.clipboard.writeText(this.dataset.copy)"
                            },
                                icon(data: iconify_icon!("lucide:copy"), size: Length::rem(1.0))
                            )
                        </div>
                    )
                }
                dialog_footer(
                    button(attrs: attributes! { type="button" commandfor="user-once" command="close" }, "Done")
                )
            )
        )
    })
}

// --------------------------------------------------------------- drawer

/// Everything the drawer shows, loaded in one go.
struct Loaded {
    user: fc_platform::principal::api::PrincipalResponse,
    roles: Vec<fc_platform::principal::api::RoleAssignmentDto>,
    apps: fc_platform::principal::api::ApplicationAccessListResponse,
    available_apps: Vec<(String, String, String)>,
    grants: Vec<(String, String, String, Option<String>)>,
    home_client: Option<(String, String)>,
    clients: Vec<(String, String, String)>,
    role_defs: Vec<(String, String, String)>,
}

#[shard("/ui/(app)/users/drawer")]
pub(crate) async fn user_drawer(
    cx: &Cx,
    id: String,
    editing: Signal<bool>,
    dialog_error: String,
) -> Result<impl View> {
    let auth = auth(cx)?;
    permit(checks::can_read_principals(auth))?;
    let loaded = if id.is_empty() {
        None
    } else {
        Some(load(cx, auth, &id).await?)
    };
    Ok(view! {
        if let Some(loaded) = loaded {
            drawer_body(auth: auth.clone(), data: loaded, editing: editing, dialog_error: dialog_error)
        }
    })
}

/// The SPA's loads (`get`, `getRoles`, `getApplicationAccess`,
/// `getClientAccess` for an anchor, the role and client lists), through the
/// handler bodies, concurrently.
async fn load(cx: &Cx, auth: &AuthContext, id: &str) -> Result<Loaded> {
    let principals = &crate::deps(cx).users.principals;
    let can_read_roles = checks::can_read_roles(auth).is_ok();
    let anchor = auth.is_anchor();
    let (user, roles, apps, available, grants, clients, role_defs) = tokio::try_join!(
        async {
            admin::detail(principals, auth, id)
                .await
                .map_err(platform_error)
        },
        async {
            admin::role_assignments(principals, auth, id)
                .await
                .map_err(platform_error)
        },
        async {
            admin::application_access(principals, auth, id)
                .await
                .map_err(platform_error)
        },
        async {
            admin::available_applications(principals, auth, id)
                .await
                .map_err(platform_error)
        },
        async {
            if anchor {
                admin::client_grants(principals, auth, id)
                    .await
                    .map(|g| g.grants)
                    .map_err(platform_error)
            } else {
                Ok(Vec::new())
            }
        },
        reachable_clients(cx, auth),
        async {
            if can_read_roles {
                principals
                    .role_repo
                    .find_all()
                    .await
                    .map_err(platform_error)
            } else {
                Ok(Vec::new())
            }
        },
    )?;
    let names = client_names(&clients);
    let label = |cid: &str| {
        names
            .get(cid)
            .cloned()
            .unwrap_or_else(|| (cid.to_owned(), String::new()))
    };
    let home_client = user.client_id.as_deref().map(label);
    let grants = grants
        .into_iter()
        .map(|g| {
            let (name, identifier) = label(&g.client_id);
            (g.client_id, name, identifier, Some(g.granted_at))
        })
        .collect();
    Ok(Loaded {
        home_client,
        grants,
        roles: roles.roles,
        available_apps: available
            .applications
            .into_iter()
            .map(|a| (a.id, a.name, a.code))
            .collect(),
        apps,
        clients: clients
            .into_iter()
            .map(|c| (c.id, c.name, c.identifier))
            .collect(),
        role_defs: role_defs
            .into_iter()
            .map(|r| (r.name, r.display_name, r.application_code))
            .collect(),
        user,
    })
}

/// A drawer section (`FcFormSection flat`): the title, its actions, and
/// the body, with the SPA's rule between sections.
const SECTION: &str = "border-b border-border pb-5 mb-4 last:mb-0 last:border-b-0 last:pb-0";
const SECTION_HEADER: &str = "mb-3 flex items-start justify-between gap-3";
const SECTION_TITLE: &str = "text-[16px] font-semibold text-foreground";
/// An `action-item`: Topcoat's horizontal field on the SPA's grey card.
const ACTION_ITEM: &str = "rounded-xl border border-[#e5e7eb] bg-[#fafafa] p-4";

#[component]
async fn drawer_body(
    cx: &Cx,
    auth: AuthContext,
    data: Loaded,
    editing: Signal<bool>,
    dialog_error: String,
) -> Result<impl View> {
    let Loaded {
        user,
        roles,
        apps,
        available_apps,
        grants,
        home_client,
        clients,
        role_defs,
    } = data;
    let can_write = checks::can_write_principals(&auth).is_ok();
    let can_assign =
        checks::can_assign_principal_roles(&auth).is_ok() && checks::can_read_roles(&auth).is_ok();
    let can_grant = checks::can_grant_client_access(&auth).is_ok();
    let can_revoke = checks::can_revoke_client_access(&auth).is_ok();
    let can_delete = checks::can_delete_principals(&auth).is_ok();
    let anchor_caller = auth.is_anchor();

    let base = detail_href(&user.id);
    let kind = user_type(&user.scope);
    let internal = user.idp_type.as_deref() == Some("INTERNAL");
    let has_email = user.email.as_deref().is_some_and(|e| !e.is_empty());
    let is_anchor_user = user.is_anchor_user;
    let email = user.email.clone().unwrap_or_default();
    let name = user.name.clone();

    // Second factors (`twoFactorSummary`).
    let factors = user.two_factor_methods.clone().unwrap_or_default();
    let has_2fa = !factors.is_empty();
    let factor_summary = if has_2fa {
        factors
            .iter()
            .map(|m| match m.as_str() {
                "TOTP" => "Authenticator app",
                "EMAIL_PIN" => "Email code",
                other => other,
            })
            .collect::<Vec<_>>()
            .join(", ")
    } else {
        "None".to_owned()
    };

    // Roles the user can be given (`assignableRoles`): every role for an
    // anchor or an all-applications user, else the roles of applications
    // they can reach, plus whatever they already hold.
    let assigned: HashSet<String> = roles.iter().map(|r| r.role_name.clone()).collect();
    let app_codes: HashSet<String> = apps
        .applications
        .iter()
        .map(|a| a.application_code.clone())
        .collect();
    let unrestricted = user.scope == "ANCHOR" || apps.all_applications;
    let mut picker_roles: Vec<(String, String)> = role_defs
        .iter()
        .filter(|(name, _, app)| unrestricted || app_codes.contains(app) || assigned.contains(name))
        .map(|(name, display, _)| (name.clone(), display.clone()))
        .collect();
    let hidden_roles = role_defs.len() - picker_roles.len();
    for name in &assigned {
        if !picker_roles.iter().any(|(n, _)| n == name) {
            picker_roles.push((name.clone(), short_role(name)));
        }
    }
    picker_roles.sort_by_key(|(_, display)| display.to_lowercase());
    let display_of: HashMap<String, String> = role_defs
        .iter()
        .map(|(n, d, _)| (n.clone(), d.clone()))
        .collect();

    let granted_ids: HashSet<String> = grants.iter().map(|g| g.0.clone()).collect();
    let grantable: Vec<(String, String)> = clients
        .iter()
        .filter(|(cid, _, _)| {
            Some(cid.as_str()) != user.client_id.as_deref() && !granted_ids.contains(cid)
        })
        .map(|(cid, name, identifier)| (cid.clone(), format!("{name} ({identifier})")))
        .collect();
    let client_options: Vec<(String, String)> = clients
        .iter()
        .map(|(cid, name, _)| (cid.clone(), name.clone()))
        .collect();

    let app_grant_ids: HashSet<String> = apps
        .applications
        .iter()
        .map(|a| a.application_id.clone())
        .collect();
    let all_apps = apps.all_applications;
    // (name, code) per granted application.
    let app_rows: Vec<(String, String)> = apps
        .applications
        .into_iter()
        .map(|a| {
            let name = if a.application_name.is_empty() {
                a.application_id
            } else {
                a.application_name
            };
            (name, a.application_code)
        })
        .collect();

    let developer = user.roles.iter().any(|r| r == DEVELOPER_ROLE) || user.has_developer_credential;
    let credential_set = user.has_developer_credential;
    let credential_at = user.developer_credential_updated_at.clone();

    // The edit form's tier select drives which client field shows.
    let scope_now = user.scope.clone();
    let scope_seed = scope_now.clone();
    let edit_scope = signal(cx, move || scope_seed.clone());
    let form_id = "user-edit-form";
    let (start, discard) = (editing.clone(), editing.clone());
    let reset_error = (!dialog_error.is_empty()).then_some(dialog_error);
    let reset_failed = reset_error.is_some();

    Ok(view! {
        // EntityDrawer's header: the title and subtitle, the status tags
        // beside them (`#header-extra`); the close button is the frame's.
        <div class="flex items-center gap-3 p-5 pr-16">
            dialog_header(attrs: attributes! { class="min-w-0 gap-0.5" },
                dialog_title(attrs: attributes! { class="truncate" }, (name.clone()))
                if !email.is_empty() {
                    dialog_description(attrs: attributes! { class="truncate text-[13px]" }, (email.clone()))
                }
            )
            <div class="flex shrink-0 items-center gap-2">
                tag(label: kind.label, variant: kind.variant, star: kind.star)
                status_tag(active: user.active)
            </div>
        </div>

        <div class="min-h-0 flex-1 overflow-y-auto px-5 pb-5" data-drawer-body="">
            // ---------------------------------------------- User Information
            <section class=(SECTION)>
                <header class=(SECTION_HEADER)>
                    <h3 class=(SECTION_TITLE)>"User Information"</h3>
                    if can_write {
                        <div class="flex items-center gap-2">
                            button(variant: ButtonVariant::Text, size: ButtonSize::Sm, attrs: attributes! {
                                type="button" :hidden=$(editing.get()) @click=$(|_e: Event| start.set(true))
                            },
                                icon(data: iconify_icon!("lucide:pencil"), size: Length::rem(1.0))
                                "Edit"
                            )
                            button(variant: ButtonVariant::Ghost, size: ButtonSize::Sm, attrs: attributes! {
                                type="reset" form=(form_id) hidden="" data-dirty-discard=""
                                @click=$(|_e: Event| discard.set(false))
                            }, "Discard")
                            button(size: ButtonSize::Sm, attrs: attributes! {
                                type="submit" form=(form_id) disabled="" data-dirty-save="" :hidden=$(!editing.get())
                            },
                                icon(data: iconify_icon!("lucide:check"), size: Length::rem(1.0))
                                "Save"
                            )
                        </div>
                    }
                </header>
                <dl class="grid grid-cols-2 gap-x-5 gap-y-3 @container" :hidden=$(editing.get())>
                    detail(label: "Name", value: name.clone())
                    detail(label: "Email", value: email.clone())
                    detail(label: "Authentication", value: match user.idp_type.as_deref() {
                        Some("INTERNAL") => "Internal".to_owned(),
                        Some(other) => other.to_owned(),
                        None => String::new(),
                    })
                    <div class="min-w-0">
                        <dt class="mb-1 text-[13px] font-medium text-[#475569]">"Type"</dt>
                        <dd>tag(label: kind.label, variant: kind.variant, star: kind.star)</dd>
                    </div>
                    if user.scope == "CLIENT" {
                        detail(label: "Client", value: home_client.clone().map(|c| c.0).unwrap_or_default())
                    }
                    <div class="min-w-0">
                        <dt class="mb-1 text-[13px] font-medium text-[#475569]">"Created"</dt>
                        <dd class="text-base">local_date(at: Some(user.created_at.clone()))</dd>
                    </div>
                </dl>
                if can_write {
                    <form id=(form_id) method="post" action=(format!("{base}/update")) class="grid grid-cols-2 gap-x-5 gap-y-4"
                        data-dirty-form="" data-dirty-key=(&user.id) :hidden=$(!editing.get())>
                        field(
                            field_label(attrs: attributes! { for="user-edit-name" }, "Name" <span class="text-destructive">"*"</span>)
                            input(attrs: attributes! { id="user-edit-name" name="name" value=(name.clone()) required="" maxlength="255" })
                        )
                        if anchor_caller {
                            field(
                                field_label(attrs: attributes! { for="user-edit-scope" }, "Type")
                                select(attrs: attributes! {
                                    id="user-edit-scope" name="scope"
                                    @change=$(|e: Event| edit_scope.set(e.target.value))
                                },
                                    for (value, label) in [("ANCHOR", "Anchor"), ("PARTNER", "Partner"), ("CLIENT", "Client")] {
                                        <option value=(value) selected=(scope_now == value)>(label)</option>
                                    }
                                )
                            )
                            field(attrs: attributes! { class="col-span-2" :hidden=$(edit_scope.get() == "ANCHOR") },
                                field_label(attrs: attributes! { for="user-edit-client" },
                                    <span :hidden=$(edit_scope.get() != "PARTNER")>"Client to grant"</span>
                                    <span :hidden=$(edit_scope.get() == "PARTNER")>"Client"</span>
                                )
                                select(attrs: attributes! { id="user-edit-client" name="client_id" },
                                    <option value="" selected=(user.client_id.is_none())>"Select client"</option>
                                    for (value, label) in client_options {
                                        <option value=(&value) selected=(user.client_id.as_deref() == Some(value.as_str()))>(label)</option>
                                    }
                                )
                            )
                        }
                    </form>
                }
            </section>

            // -------------------------------------------------- Client Access
            if anchor_caller {
                <section class=(SECTION)>
                    <header class=(SECTION_HEADER)>
                        <h3 class=(SECTION_TITLE)>"Client Access"</h3>
                        if can_grant && !is_anchor_user {
                            button(variant: ButtonVariant::Text, size: ButtonSize::Sm, attrs: attributes! {
                                type="button" commandfor="user-grant-client" command="show-modal"
                            },
                                icon(data: iconify_icon!("lucide:plus"), size: Length::rem(1.0))
                                "Add Client"
                            )
                        }
                    </header>
                    if is_anchor_user {
                        alert(attrs: attributes! { class="border-[#fcd34d] bg-[#fffbeb] text-[#92400e]" },
                            icon(data: iconify_icon!("lucide:star"), attrs: attributes! { class="text-[#f59e0b]" })
                            alert_description(attrs: attributes! { class="[&]:text-[#92400e]" },
                                "This user has an anchor domain email and automatically has access to all clients."
                            )
                        )
                    } else {
                        if let Some((home_name, home_identifier)) = home_client.clone() {
                            <h4 class="mb-2 text-[13px] font-semibold text-muted-foreground">"Home Client"</h4>
                            <div class="mb-4 flex items-center justify-between rounded-lg border border-border bg-[#f8fafc] px-3 py-2">
                                <span class="flex flex-col">
                                    <span class="font-medium">(home_name)</span>
                                    <span class="font-mono text-[12px] text-muted-foreground">(home_identifier)</span>
                                </span>
                                badge(variant: BadgeVariant::Secondary, "Home")
                            </div>
                        }
                        if home_client.is_none() && grants.is_empty() {
                            <div class="flex flex-col items-start gap-2 text-muted-foreground">
                                <p>"This user has no client access configured."</p>
                                if can_grant {
                                    button(variant: ButtonVariant::Text, size: ButtonSize::Sm, attrs: attributes! {
                                        type="button" commandfor="user-grant-client" command="show-modal"
                                    },
                                        icon(data: iconify_icon!("lucide:plus"), size: Length::rem(1.0))
                                        "Grant Client Access"
                                    )
                                }
                            </div>
                        }
                        if !grants.is_empty() {
                            <h4 class="mb-2 text-[13px] font-semibold text-muted-foreground">"Granted Access"</h4>
                            small_table(
                                table_header(
                                    table_row(
                                        table_head("Client")
                                        table_head("Granted")
                                        table_head(attrs: attributes! { class="w-[80px]" }, "")
                                    )
                                )
                                table_body(
                                    for (cid, cname, cident, granted_at) in grants {
                                        table_row(
                                            table_cell(
                                                <span class="flex flex-col">
                                                    <span class="font-medium">(cname)</span>
                                                    <span class="font-mono text-[12px] text-muted-foreground">(cident)</span>
                                                </span>
                                            )
                                            table_cell(local_date(at: granted_at))
                                            table_cell(
                                                if can_revoke {
                                                    <form method="post" action=(format!("{base}/client-access/{cid}/revoke"))>
                                                        button(variant: ButtonVariant::Ghost, size: ButtonSize::Icon, attrs: attributes! {
                                                            type="submit" aria-label="Revoke access" title="Revoke access"
                                                            class="[&]:rounded-full text-destructive"
                                                        },
                                                            icon(data: iconify_icon!("lucide:trash-2"), size: Length::rem(1.0))
                                                        )
                                                    </form>
                                                }
                                            )
                                        )
                                    }
                                )
                            )
                        }
                    }
                </section>
            }

            // ---------------------------------------------------------- Roles
            <section class=(SECTION)>
                <header class=(SECTION_HEADER)>
                    <h3 class=(SECTION_TITLE)>"Roles"</h3>
                    if can_assign {
                        button(variant: ButtonVariant::Text, size: ButtonSize::Sm, attrs: attributes! {
                            type="button" commandfor="user-roles" command="show-modal"
                        },
                            icon(data: iconify_icon!("lucide:pencil"), size: Length::rem(1.0))
                            "Manage Roles"
                        )
                    }
                </header>
                if roles.is_empty() {
                    <div class="flex flex-col items-start gap-2 text-muted-foreground">
                        <p>"No roles assigned to this user."</p>
                        if can_assign {
                            button(variant: ButtonVariant::Text, size: ButtonSize::Sm, attrs: attributes! {
                                type="button" commandfor="user-roles" command="show-modal"
                            },
                                icon(data: iconify_icon!("lucide:plus"), size: Length::rem(1.0))
                                "Assign Roles"
                            )
                        }
                    </div>
                } else {
                    small_table(
                        table_header(
                            table_row(
                                table_head("Role")
                                table_head("Source")
                                table_head("Assigned")
                            )
                        )
                        table_body(
                            for r in roles {
                                let source_variant = if r.assignment_source == "ADMIN_ASSIGNED" || r.assignment_source == "ADMIN" {
                                    BadgeVariant::Info
                                } else {
                                    BadgeVariant::Secondary
                                };
                                table_row(
                                    table_cell(
                                        <span class="flex flex-col">
                                            <span class="font-medium">(short_role(&r.role_name))</span>
                                            <span class="font-mono text-[12px] text-muted-foreground">(r.role_name.clone())</span>
                                        </span>
                                    )
                                    table_cell(badge(variant: source_variant, (r.assignment_source.clone())))
                                    table_cell(local_date(at: Some(r.assigned_at.clone())))
                                )
                            }
                        )
                    )
                }
            </section>

            // ---------------------------------------------- Application Access
            <section class=(SECTION)>
                <header class=(SECTION_HEADER)>
                    <h3 class=(SECTION_TITLE)>"Application Access"</h3>
                    if can_write && !all_apps {
                        button(variant: ButtonVariant::Text, size: ButtonSize::Sm, attrs: attributes! {
                            type="button" commandfor="user-apps" command="show-modal"
                        },
                            icon(data: iconify_icon!("lucide:pencil"), size: Length::rem(1.0))
                            "Manage Applications"
                        )
                    }
                </header>
                if can_write {
                    // The all-applications switch saves on change.
                    <form method="post" action=(format!("{base}/all-applications")) class="pt-1 pb-4">
                        <input type="hidden" name="submitted" value="1">
                        field(orientation: FieldOrientation::Horizontal, attrs: attributes! { class="items-start" },
                            switch(attrs: attributes! {
                                id="user-all-apps" name="all" checked=(all_apps) onchange="this.form.requestSubmit()"
                                class="mt-0.5"
                            })
                            field_content(attrs: attributes! { class="gap-0.5" },
                                field_label(attrs: attributes! { for="user-all-apps" class="text-[14px] font-semibold" }, "Access to all applications")
                                field_description(attrs: attributes! { class="text-[12px]" }, "This user can access every application, present and future.")
                            )
                        )
                    </form>
                }
                if !all_apps {
                    if app_rows.is_empty() {
                        <div class="flex flex-col items-start gap-2 text-muted-foreground">
                            <p>"No application access granted to this user."</p>
                            if can_write {
                                button(variant: ButtonVariant::Text, size: ButtonSize::Sm, attrs: attributes! {
                                    type="button" commandfor="user-apps" command="show-modal"
                                },
                                    icon(data: iconify_icon!("lucide:plus"), size: Length::rem(1.0))
                                    "Grant Application Access"
                                )
                            }
                        </div>
                    } else {
                        small_table(
                            table_header(table_row(table_head("Application")))
                            table_body(
                                for (app_name, app_code) in app_rows {
                                    table_row(
                                        table_cell(
                                            <span class="flex flex-col">
                                                <span class="font-medium">(app_name)</span>
                                                <span class="font-mono text-[12px] text-muted-foreground">(app_code)</span>
                                            </span>
                                        )
                                    )
                                }
                            )
                        )
                    }
                }
            </section>

            // ------------------------------------------------ Account Actions
            if can_write || can_delete {
                <section class=(SECTION)>
                    <header class=(SECTION_HEADER)><h3 class=(SECTION_TITLE)>"Account Actions"</h3></header>
                    <div class="flex flex-col gap-3">
                        if can_write && internal && has_email {
                            action_item(title: "Send Password Reset".to_owned(), description: "Email the user a single-use link to set a new password.",
                                button(variant: ButtonVariant::Outline, attrs: attributes! { type="button" commandfor="user-send-reset" command="show-modal" },
                                    icon(data: iconify_icon!("lucide:mail"), size: Length::rem(1.0))
                                    "Send Email"
                                )
                            )
                        }
                        if can_write && internal {
                            action_item(title: "Reset Password".to_owned(), description: "Set a new password directly (use when the user can't receive email).",
                                button(variant: ButtonVariant::Outline, attrs: attributes! { type="button" commandfor="user-reset-password" command="show-modal" },
                                    icon(data: iconify_icon!("lucide:key-round"), size: Length::rem(1.0))
                                    "Reset Password"
                                )
                            )
                            <div class=(ACTION_ITEM)>
                                field(orientation: FieldOrientation::Horizontal,
                                    field_content(
                                        field_title("Two-Factor Authentication"
                                            badge(variant: if has_2fa { BadgeVariant::Success } else { BadgeVariant::Secondary }, attrs: attributes! { class="ml-2" }, (factor_summary.clone()))
                                        )
                                        field_description(attrs: attributes! { class="text-[13px]" },
                                            if has_2fa {
                                                "Enrolled factors, recovery codes and trusted devices — reset to clear them and re-trigger 2FA onboarding at next sign-in."
                                            } else {
                                                "No second factor enrolled. Without an authenticator app, a self-service \"Forgot password\" is diverted to admin approval instead of emailing a reset link."
                                            }
                                        )
                                    )
                                    button(variant: ButtonVariant::Outline, attrs: attributes! {
                                        type="button" commandfor="user-reset-2fa" command="show-modal" disabled=(!has_2fa)
                                    },
                                        icon(data: iconify_icon!("lucide:smartphone"), size: Length::rem(1.0))
                                        "Reset 2FA"
                                    )
                                )
                            </div>
                        }
                        if can_write && developer {
                            <div class=(ACTION_ITEM)>
                                field(orientation: FieldOrientation::Horizontal,
                                    field_content(
                                        field_title("Developer Credential"
                                            badge(variant: if credential_set { BadgeVariant::Success } else { BadgeVariant::Secondary }, attrs: attributes! { class="ml-2" },
                                                (if credential_set { "Set" } else { "Not set" })
                                            )
                                        )
                                        field_description(attrs: attributes! { class="text-[13px]" },
                                            "A client_credentials secret for the developer's own API access (client ID = user ID)."
                                            if let Some(at) = credential_at {
                                                " Last set " local_date(at: Some(at)) "."
                                            }
                                        )
                                    )
                                    <div class="flex shrink-0 gap-2">
                                        <form method="post" action=(format!("{base}/developer-credential"))>
                                            button(variant: ButtonVariant::Outline, attrs: attributes! { type="submit" },
                                                icon(data: iconify_icon!("lucide:key-round"), size: Length::rem(1.0))
                                                (if credential_set { "Rotate" } else { "Generate" })
                                            )
                                        </form>
                                        if credential_set {
                                            button(variant: ButtonVariant::DestructiveOutline, attrs: attributes! { type="button" commandfor="user-revoke-credential" command="show-modal" }, "Revoke")
                                        }
                                    </div>
                                )
                            </div>
                        }
                        if can_write {
                            <form method="post" action=(format!("{base}/{}", if user.active { "deactivate" } else { "activate" }))>
                                action_item(
                                    title: (if user.active { "Deactivate User" } else { "Activate User" }).to_owned(),
                                    description: if user.active { "Prevent this user from signing in." } else { "Allow this user to sign in again." },
                                    if user.active {
                                        button(variant: ButtonVariant::DestructiveOutline, attrs: attributes! { type="submit" },
                                            icon(data: iconify_icon!("lucide:ban"), size: Length::rem(1.0))
                                            "Deactivate"
                                        )
                                    } else {
                                        button(variant: ButtonVariant::SuccessOutline, attrs: attributes! { type="submit" },
                                            icon(data: iconify_icon!("lucide:check"), size: Length::rem(1.0))
                                            "Activate"
                                        )
                                    }
                                )
                            </form>
                        }
                        if can_delete {
                            action_item(title: "Delete User".to_owned(), description: "Permanently remove this user. Cannot be undone.",
                                button(variant: ButtonVariant::DestructiveOutline, attrs: attributes! { type="button" commandfor="user-delete" command="show-modal" },
                                    icon(data: iconify_icon!("lucide:trash-2"), size: Length::rem(1.0))
                                    "Delete"
                                )
                            )
                        }
                    </div>
                </section>
            }
        </div>

        // ---------------------------------------------------------- dialogs
        if anchor_caller && can_grant && !is_anchor_user {
            dialog(open: false, attrs: attributes! { id="user-grant-client" aria-labelledby="user-grant-client-title" data-light-dismiss="" class=(MODAL) },
                dialog_content(attrs: attributes! { class="[&]:max-w-[450px]" },
                    dialog_header(dialog_title(attrs: attributes! { id="user-grant-client-title" }, "Grant Client Access"))
                    dialog_close(target: "user-grant-client")
                    <form id="user-grant-client-form" method="post" action=(format!("{base}/client-access"))>
                        field(
                            field_label(attrs: attributes! { for="user-grant-client-id" }, "Client")
                            select(attrs: attributes! { id="user-grant-client-id" name="client_id" required="" },
                                <option value="">"Select a client"</option>
                                for (value, label) in grantable {
                                    <option value=(&value)>(label)</option>
                                }
                            )
                        )
                    </form>
                    dialog_footer(
                        button(variant: ButtonVariant::Ghost, attrs: attributes! { type="button" commandfor="user-grant-client" command="close" }, "Cancel")
                        button(attrs: attributes! { type="submit" form="user-grant-client-form" },
                            icon(data: iconify_icon!("lucide:check"), size: Length::rem(1.0))
                            "Grant Access"
                        )
                    )
                )
            )
        }

        if can_assign {
            picker_dialog(
                id: "user-roles",
                title: "Manage Roles",
                action: format!("{base}/roles"),
                key: user.id.clone(),
                prefix: "role",
                available_title: "Available Roles",
                selected_title: "Selected Roles",
                filter_placeholder: "Filter roles...",
                empty: "No roles found",
                none_selected: "No roles selected",
                save_label: "Save Roles",
                items: picker_roles
                    .into_iter()
                    .map(|(name, display)| {
                        let checked = assigned.contains(&name);
                        let display = display_of.get(&name).cloned().filter(|d| !d.is_empty()).unwrap_or(display);
                        (name.clone(), display, name, checked)
                    })
                    .collect(),
                hint: (hidden_roles > 0).then(|| format!(
                    "{hidden_roles} role{} hidden because their application isn't enabled for this user. Add the application under Application Access to make them available here.",
                    if hidden_roles == 1 { "" } else { "s" }
                )),
            )
        }

        if can_write {
            picker_dialog(
                id: "user-apps",
                title: "Manage Application Access",
                action: format!("{base}/applications"),
                key: user.id.clone(),
                prefix: "app",
                available_title: "Available Applications",
                selected_title: "Selected Applications",
                filter_placeholder: "Filter applications...",
                empty: "No applications found",
                none_selected: "No applications selected",
                save_label: "Save Application Access",
                items: available_apps
                    .into_iter()
                    .map(|(id, name, code)| {
                        let checked = app_grant_ids.contains(&id);
                        (id, name, code, checked)
                    })
                    .collect(),
                hint: None,
            )

            dialog(open: false, attrs: attributes! { id="user-send-reset" aria-labelledby="user-send-reset-title" data-light-dismiss="" class=(MODAL) },
                dialog_content(attrs: attributes! { class="[&]:max-w-[480px]" },
                    dialog_header(dialog_title(attrs: attributes! { id="user-send-reset-title" }, "Send Password Reset Email"))
                    dialog_close(target: "user-send-reset")
                    <p>"Send a password reset email to " <strong>(name.clone())</strong> " (" <code>(email.clone())</code> ")?"</p>
                    alert(attrs: attributes! { class="border-[#bae6fd] bg-[#f0f9ff] text-[#0369a1]" },
                        icon(data: iconify_icon!("lucide:info"))
                        alert_description(attrs: attributes! { class="[&]:text-[#0369a1]" },
                            "The user will receive a single-use link valid for 15 minutes. They will set their own password — you will not see or handle it. Any previously-issued reset tokens for this user will be invalidated."
                        )
                    )
                    <form id="user-send-reset-form" method="post" action=(format!("{base}/send-password-reset"))></form>
                    dialog_footer(
                        button(variant: ButtonVariant::Ghost, attrs: attributes! { type="button" commandfor="user-send-reset" command="close" }, "Cancel")
                        button(attrs: attributes! { type="submit" form="user-send-reset-form" },
                            icon(data: iconify_icon!("lucide:mail"), size: Length::rem(1.0))
                            "Send Email"
                        )
                    )
                )
            )

            dialog(open: false, attrs: attributes! {
                id="user-reset-password" aria-labelledby="user-reset-password-title" data-light-dismiss=""
                data-show-on-load=(reset_failed) class=(MODAL)
            },
                dialog_content(attrs: attributes! { class="[&]:max-w-[480px]" },
                    dialog_header(dialog_title(attrs: attributes! { id="user-reset-password-title" }, "Reset Password"))
                    dialog_close(target: "user-reset-password")
                    <p>
                        "Set a new password for " <strong>(name.clone())</strong>
                        if !email.is_empty() { " (" <code>(email.clone())</code> ")" }
                        "."
                    </p>
                    alert(attrs: attributes! { class="border-[#fed7aa] bg-[#fff7ed] text-[#c2410c]" },
                        icon(data: iconify_icon!("lucide:triangle-alert"))
                        alert_description(attrs: attributes! { class="[&]:text-[#c2410c]" },
                            "The user will need to sign in with this new password immediately. Only use this when the user can't receive the password-reset email (e.g. lost inbox access)."
                        )
                    )
                    <form id="user-reset-password-form" method="post" action=(format!("{base}/reset-password")) class="flex flex-col gap-4">
                        field(
                            field_label(attrs: attributes! { for="user-new-password" }, "New password")
                            // Another user's password: `new-password` stops the
                            // browser offering the admin's own.
                            input(attrs: attributes! {
                                id="user-new-password" name="new_password" type="password" autocomplete="new-password"
                                placeholder="At least 8 characters" required="" aria-invalid=(reset_failed.then_some("true"))
                            })
                        )
                        field(
                            field_label(attrs: attributes! { for="user-confirm-password" }, "Confirm password")
                            input(attrs: attributes! {
                                id="user-confirm-password" name="confirm_password" type="password" autocomplete="new-password" required=""
                            })
                        )
                        if let Some(error) = reset_error {
                            alert(variant: AlertVariant::Destructive, attrs: attributes! { role="alert" },
                                icon(data: iconify_icon!("lucide:circle-alert"))
                                alert_description(attrs: attributes! { class="[&]:text-destructive" }, (error))
                            )
                        }
                    </form>
                    dialog_footer(
                        button(variant: ButtonVariant::Ghost, attrs: attributes! { type="button" commandfor="user-reset-password" command="close" }, "Cancel")
                        button(attrs: attributes! { type="submit" form="user-reset-password-form" },
                            icon(data: iconify_icon!("lucide:key-round"), size: Length::rem(1.0))
                            "Set Password"
                        )
                    )
                )
            )

            confirm(
                id: "user-reset-2fa",
                title: "Reset Two-Factor",
                message: format!("Reset two-factor authentication for \"{name}\"? Enrolled factors, recovery codes and trusted devices are cleared; 2FA onboarding re-triggers at their next sign-in."),
                action: format!("{base}/reset-2fa"),
                confirm_label: "Reset 2FA",
            )
            if credential_set {
                confirm(
                    id: "user-revoke-credential",
                    title: "Revoke Developer Credential",
                    message: format!("Revoke the developer credential of \"{name}\"? Tokens it already issued stay valid until they expire; new ones can't be minted."),
                    action: format!("{base}/developer-credential/revoke"),
                    confirm_label: "Revoke",
                )
            }
        }

        if can_delete {
            confirm(
                id: "user-delete",
                title: "Delete User",
                message: format!("Are you sure you want to delete \"{name}\"? This action cannot be undone. The user will be permanently removed."),
                action: format!("{base}/delete"),
                confirm_label: "Delete",
            )
        }
    })
}

/// The dialog header's close button (PrimeVue's `closable` X), for a
/// Topcoat dialog opened as a modal.
#[component]
async fn dialog_close(target: &'static str) -> Result<impl View> {
    Ok(view! {
        button(variant: ButtonVariant::Ghost, size: ButtonSize::Icon, attrs: attributes! {
            type="button" commandfor=(target) command="close" aria-label="Close"
            class="absolute top-3 right-3 [&]:size-8 [&]:rounded-full text-muted-foreground"
        },
            icon(data: iconify_icon!("lucide:x"), size: Length::rem(1.1))
        )
    })
}

/// `roleName.split(':').pop()`.
fn short_role(name: &str) -> String {
    name.rsplit(':').next().unwrap_or(name).to_owned()
}

/// A read-only value (`FcDetailField`); empty shows "—".
#[component]
async fn detail(label: &'static str, value: String) -> Result<impl View> {
    let value = if value.is_empty() {
        "—".to_owned()
    } else {
        value
    };
    Ok(view! {
        <div class="min-w-0">
            <dt class="mb-1 text-[13px] font-medium text-[#475569]">(label)</dt>
            <dd class="text-base break-words text-foreground">(value)</dd>
        </div>
    })
}

/// The drawer's small tables (`DataTable size="small"`, with the SPA's
/// heading colours from `main.css`).
#[component]
async fn small_table(child: topcoat::view::Child<'_>) -> Result<impl View> {
    Ok(view! {
        table(attrs: attributes! { class="[&_td]:px-2 [&_td]:py-1.5 [&_th]:px-2 [&_th]:h-8 [&_th]:bg-[#f8fafc] [&_th]:font-semibold [&_th]:text-[#334e68] [&_tbody_tr]:hover:bg-transparent" },
            (child)
        )
    })
}

/// One of the SPA's `action-item`s: Topcoat's horizontal field (title and
/// description beside the control) on a grey card.
#[component]
async fn action_item(
    title: String,
    description: &'static str,
    child: topcoat::view::Child<'_>,
) -> Result<impl View> {
    Ok(view! {
        <div class=(ACTION_ITEM)>
            field(orientation: FieldOrientation::Horizontal,
                field_content(
                    field_title((title))
                    field_description(attrs: attributes! { class="text-[13px]" }, (description))
                )
                <div class="shrink-0">(child)</div>
            )
        </div>
    })
}

/// A confirm-then-POST dialog (`useConfirm` + `ConfirmDialog`), built from
/// Topcoat's alert dialog. Open it with `commandfor=(id)
/// command="show-modal"`.
#[component]
async fn confirm(
    id: &'static str,
    title: &'static str,
    message: String,
    action: String,
    confirm_label: &'static str,
) -> Result<impl View> {
    let title_id = format!("{id}-title");
    let desc_id = format!("{id}-desc");
    Ok(view! {
        alert_dialog(open: false, attrs: attributes! {
            id=(id) aria-labelledby=(&title_id) aria-describedby=(&desc_id) class=(MODAL)
        },
            dialog_content(attrs: attributes! { class="[&]:max-w-[450px]" },
                dialog_header(
                    dialog_title(attrs: attributes! { id=(&title_id) }, (title))
                )
                dialog_close(target: id)
                <div class="flex items-start gap-3">
                    icon(data: iconify_icon!("lucide:triangle-alert"), size: Length::rem(1.5), attrs: attributes! { class="mt-0.5 shrink-0 text-destructive" })
                    dialog_description(attrs: attributes! { id=(&desc_id) class="[&]:text-[#475569]" }, (message))
                </div>
                <form method="post" action=(action)>
                    dialog_footer(
                        button(variant: ButtonVariant::Ghost, attrs: attributes! { type="button" commandfor=(id) command="close" }, "Cancel")
                        button(variant: ButtonVariant::Destructive, attrs: attributes! { type="submit" }, (confirm_label))
                    )
                </form>
            )
        )
    })
}

/// The SPA's dual-pane pickers (Manage Roles, Manage Application Access):
/// Topcoat checkboxes on the left, filtered as you type; what's ticked
/// mirrored on the right, where × unticks it. Save stays disabled until
/// the selection changes. `ui.js` does the filtering and the mirror; the
/// form alone works without it.
#[component]
async fn picker_dialog(
    id: &'static str,
    title: &'static str,
    action: String,
    key: String,
    prefix: &'static str,
    available_title: &'static str,
    selected_title: &'static str,
    filter_placeholder: &'static str,
    empty: &'static str,
    none_selected: &'static str,
    save_label: &'static str,
    /// (value, name, code, checked)
    items: Vec<(String, String, String, bool)>,
    hint: Option<String>,
) -> Result<impl View> {
    let form_id = format!("{id}-form");
    let title_id = format!("{id}-title");
    let list_id = format!("{id}-available");
    let count = items.iter().filter(|i| i.3).count();
    let selected: Vec<(String, String, String, bool)> = items.clone();
    Ok(view! {
        dialog(open: false, attrs: attributes! { id=(id) aria-labelledby=(&title_id) class=(MODAL) },
            dialog_content(attrs: attributes! { class="[&]:max-w-[700px]" },
                dialog_header(dialog_title(attrs: attributes! { id=(&title_id) }, (title)))
                dialog_close(target: id)
                <form id=(&form_id) method="post" action=(action) data-dirty-form="" data-dirty-key=(key) data-mirror=""
                    class="grid grid-cols-1 gap-4 sm:grid-cols-2">
                    <div class="flex min-w-0 flex-col overflow-hidden rounded-lg border border-border">
                        <div class="flex flex-col gap-2 border-b border-border bg-[#f8fafc] p-3">
                            <h4 class="text-[12px] font-semibold tracking-wider text-[#475569] uppercase">(available_title)</h4>
                            input(attrs: attributes! {
                                type="search" placeholder=(filter_placeholder) aria-label=(filter_placeholder)
                                data-filter-list=(&list_id)
                            })
                        </div>
                        <div id=(&list_id) class="h-[300px] overflow-y-auto p-1">
                            for (value, name, code, checked) in items {
                                let cb = format!("{id}-{value}");
                                <label for=(&cb) data-filter-text=(format!("{} {}", name.to_lowercase(), code.to_lowercase()))
                                    class="flex cursor-pointer items-center justify-between gap-3 rounded-md px-2 py-1.5 hover:bg-foreground/5 has-[:checked]:bg-primary/10">
                                    <span class="flex min-w-0 flex-col">
                                        <span class="truncate font-medium">(name)</span>
                                        <span class="truncate font-mono text-[12px] text-muted-foreground">(code)</span>
                                    </span>
                                    checkbox(attrs: attributes! { id=(&cb) name=(format!("{prefix}:{value}")) checked=(checked) })
                                </label>
                            }
                            <p data-filter-empty="" hidden="" class="p-3 text-center text-muted-foreground">(empty)</p>
                        </div>
                        if let Some(hint) = hint {
                            <p class="border-t border-border p-3 text-[12px] text-muted-foreground">(hint)</p>
                        }
                    </div>
                    <div class="flex min-w-0 flex-col overflow-hidden rounded-lg border border-border">
                        <div class="border-b border-border bg-[#f8fafc] p-3">
                            <h4 class="text-[12px] font-semibold tracking-wider text-[#475569] uppercase">(selected_title) " (" <span data-mirror-count="">(count.to_string())</span> ")"</h4>
                        </div>
                        <div class="h-[300px] overflow-y-auto p-1">
                            for (value, name, code, checked) in selected {
                                let cb = format!("{id}-{value}");
                                <div data-mirror-of=(&cb) hidden=(!checked) class="flex items-center justify-between gap-3 rounded-md px-2 py-1.5">
                                    <span class="flex min-w-0 flex-col">
                                        <span class="truncate font-medium">(name)</span>
                                        <span class="truncate font-mono text-[12px] text-muted-foreground">(code)</span>
                                    </span>
                                    <label for=(&cb) title="Remove" aria-label="Remove"
                                        class=(class!(button_variants(ButtonVariant::Ghost, ButtonSize::Icon), "[&]:size-7 [&]:rounded-full cursor-pointer text-destructive"))>
                                        icon(data: iconify_icon!("lucide:x"), size: Length::rem(0.9))
                                    </label>
                                </div>
                            }
                            <p data-mirror-empty="" hidden=(count > 0) class="p-3 text-center text-muted-foreground">(none_selected)</p>
                        </div>
                    </div>
                </form>
                dialog_footer(
                    button(variant: ButtonVariant::Ghost, attrs: attributes! { type="button" commandfor=(id) command="close" }, "Cancel")
                    button(attrs: attributes! { type="submit" form=(&form_id) disabled="" data-dirty-save="" },
                        icon(data: iconify_icon!("lucide:check"), size: Length::rem(1.0))
                        (save_label)
                    )
                )
            )
        )
    })
}

// --------------------------------------------------------------- writes

fn principals(cx: &Cx) -> &fc_platform::principal::PrincipalsState {
    &crate::deps(cx).users.principals
}

#[derive(Deserialize)]
struct UpdateForm {
    name: String,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    client_id: Option<String>,
}

/// `saveUser`: the name through `PUT /api/principals/{id}`, then, when an
/// anchor changed the tier or client, `PUT …/client-association` with the
/// SPA's intent (`*` for anchor, `CHANGE_CLIENT`, `TO_PARTNER`).
#[route(POST "/ui/(app)/users/{id}/update")]
async fn save_user(cx: &Cx, Form(form): Form<UpdateForm>) -> Result<SeeOther> {
    let auth = auth(cx)?;
    permit(checks::can_write_principals(auth))?;
    let id = target(cx);
    let deps = crate::deps(cx);
    let outcome: std::result::Result<(), PlatformError> = async {
        if form.name.trim().is_empty() {
            return Err(PlatformError::validation("Name is required"));
        }
        // The name goes on every save, as the SPA sends it; an unchanged
        // name is a no-op update (Go). A tier or client change then goes to
        // the association.
        let updated = admin::update(
            principals(cx),
            auth,
            &id,
            UpdatePrincipalRequest {
                name: Some(form.name.clone()),
                first_name: None,
                last_name: None,
                active: None,
                scope: None,
                client_id: None,
                email: None,
            },
        )
        .await?;
        let Some(scope) = form.scope.filter(|s| !s.is_empty()) else {
            return Ok(());
        };
        let client_id = form.client_id.filter(|c| !c.is_empty());
        let changed = scope != updated.scope || client_id != updated.client_id;
        if !changed {
            return Ok(());
        }
        let (client_id, mode) = match (scope.as_str(), client_id) {
            ("ANCHOR", _) => ("*".to_owned(), None),
            ("CLIENT", Some(c)) => (c, Some("CHANGE_CLIENT".to_owned())),
            ("PARTNER", Some(c)) => (c, Some("TO_PARTNER".to_owned())),
            _ => {
                return Err(PlatformError::validation(
                    "Select a client for CLIENT/PARTNER scope",
                ));
            }
        };
        client_association(
            &deps.users.principal_go,
            auth,
            &id,
            ClientAssociationRequest { client_id, mode },
        )
        .await?;
        Ok(())
    }
    .await;
    let base = detail_href(&id);
    finish(
        cx,
        outcome,
        "User updated successfully",
        base.clone(),
        format!("{base}?edit=true"),
    )
}

/// The ticked `prefix:value` checkboxes of a picker form.
fn ticked(form: &HashMap<String, String>, prefix: &str) -> Vec<String> {
    let prefix = format!("{prefix}:");
    let mut out: Vec<String> = form
        .keys()
        .filter_map(|k| k.strip_prefix(&prefix))
        .map(str::to_owned)
        .collect();
    out.sort();
    out
}

/// `PUT /api/principals/{id}/roles` (`saveRoles`): the ticked roles become
/// the user's set, bounded by the caller's role ceiling (a refusal is
/// shown).
#[route(POST "/ui/(app)/users/{id}/roles")]
async fn save_roles(cx: &Cx, Form(form): Form<HashMap<String, String>>) -> Result<SeeOther> {
    let auth = auth(cx)?;
    permit(checks::can_assign_principal_roles(auth))?;
    let id = target(cx);
    let outcome = admin::set_roles(principals(cx), auth, &id, ticked(&form, "role")).await;
    let message = match &outcome {
        Ok(r) => match (r.added.len(), r.removed.len()) {
            (0, 0) => "Roles updated".to_owned(),
            (a, 0) => format!("Added {a} role(s)"),
            (0, r) => format!("Removed {r} role(s)"),
            (a, r) => format!("Added {a} role(s), removed {r} role(s)"),
        },
        Err(_) => String::new(),
    };
    let base = detail_href(&id);
    finish(cx, outcome, &message, base.clone(), base)
}

#[derive(Deserialize)]
struct GrantForm {
    client_id: String,
}

/// `POST /api/principals/{id}/client-access`.
#[route(POST "/ui/(app)/users/{id}/client-access")]
async fn grant_client(cx: &Cx, Form(form): Form<GrantForm>) -> Result<SeeOther> {
    let auth = auth(cx)?;
    permit(checks::can_grant_client_access(auth))?;
    let id = target(cx);
    let outcome = if form.client_id.trim().is_empty() {
        Err(PlatformError::validation("Select a client"))
    } else {
        admin::grant_client_access(principals(cx), auth, &id, form.client_id).await
    };
    let base = detail_href(&id);
    finish(cx, outcome, "Client access granted", base.clone(), base)
}

/// `DELETE /api/principals/{id}/client-access/{clientId}`.
#[route(POST "/ui/(app)/users/{id}/client-access/{client}/revoke")]
async fn revoke_client(cx: &Cx) -> Result<SeeOther> {
    let auth = auth(cx)?;
    permit(checks::can_revoke_client_access(auth))?;
    let id = target(cx);
    let client = path_param::<Client>(cx).to_owned();
    let outcome = admin::revoke_client_access(principals(cx), auth, &id, &client).await;
    let base = detail_href(&id);
    finish(cx, outcome, "Client access revoked", base.clone(), base)
}

/// `PUT /api/principals/{id}/application-access` (`saveApps`), leaving the
/// all-applications flag as it is.
#[route(POST "/ui/(app)/users/{id}/applications")]
async fn save_applications(cx: &Cx, Form(form): Form<HashMap<String, String>>) -> Result<SeeOther> {
    let auth = auth(cx)?;
    permit(checks::can_write_principals(auth))?;
    let id = target(cx);
    let outcome = admin::set_application_access(
        principals(cx),
        auth,
        &id,
        SetApplicationAccessRequest {
            application_ids: ticked(&form, "app"),
            all_applications: None,
        },
    )
    .await;
    let message = match &outcome {
        Ok(r) => match (r.added, r.removed) {
            (0, 0) => "Application access updated".to_owned(),
            (a, 0) => format!("Added {a} app(s)"),
            (0, r) => format!("Removed {r} app(s)"),
            (a, r) => format!("Added {a} app(s), removed {r} app(s)"),
        },
        Err(_) => String::new(),
    };
    let base = detail_href(&id);
    finish(cx, outcome, &message, base.clone(), base)
}

#[derive(Deserialize)]
struct AllAppsForm {
    #[serde(default)]
    all: Option<String>,
    /// Always sent, so an unticked switch still posts a body.
    #[serde(default)]
    #[allow(dead_code)]
    submitted: Option<String>,
}

/// `onToggleAllApplications`: the same PUT with the current grants kept, so
/// switching back off restores them. Only an all-applications
/// administrator may switch it on (the platform refuses anyone else).
#[route(POST "/ui/(app)/users/{id}/all-applications")]
async fn toggle_all_applications(cx: &Cx, Form(form): Form<AllAppsForm>) -> Result<SeeOther> {
    let auth = auth(cx)?;
    permit(checks::can_write_principals(auth))?;
    let id = target(cx);
    let on = form.all.is_some();
    let outcome = async {
        let current = admin::application_access(principals(cx), auth, &id).await?;
        admin::set_application_access(
            principals(cx),
            auth,
            &id,
            SetApplicationAccessRequest {
                application_ids: current
                    .applications
                    .into_iter()
                    .map(|a| a.application_id)
                    .collect(),
                all_applications: Some(on),
            },
        )
        .await
    }
    .await;
    let base = detail_href(&id);
    let message = if on {
        "Granted access to all applications"
    } else {
        "Restricted to specific applications"
    };
    finish(cx, outcome, message, base.clone(), base)
}

/// `POST /api/principals/{id}/activate`.
#[route(POST "/ui/(app)/users/{id}/activate")]
async fn activate_user(cx: &Cx) -> Result<SeeOther> {
    let auth = auth(cx)?;
    permit(checks::can_write_principals(auth))?;
    let id = target(cx);
    let outcome = admin::activate(principals(cx), auth, &id).await;
    let base = detail_href(&id);
    finish(cx, outcome, "User activated", base.clone(), base)
}

/// `POST /api/principals/{id}/deactivate`.
#[route(POST "/ui/(app)/users/{id}/deactivate")]
async fn deactivate_user(cx: &Cx) -> Result<SeeOther> {
    let auth = auth(cx)?;
    permit(checks::can_write_principals(auth))?;
    let id = target(cx);
    let outcome = admin::deactivate(principals(cx), auth, &id).await;
    let base = detail_href(&id);
    finish(cx, outcome, "User deactivated", base.clone(), base)
}

/// `POST /api/principals/{id}/send-password-reset` (no body: the plain
/// reset email).
#[route(POST "/ui/(app)/users/{id}/send-password-reset")]
async fn send_password_reset(cx: &Cx) -> Result<SeeOther> {
    let auth = auth(cx)?;
    permit(checks::can_write_principals(auth))?;
    let id = target(cx);
    let outcome = admin::send_password_reset(principals(cx), auth, &id, false).await;
    let base = detail_href(&id);
    finish(cx, outcome, "Password reset email sent", base.clone(), base)
}

#[derive(Deserialize, Default)]
struct ResetPasswordForm {
    #[serde(default)]
    new_password: String,
    #[serde(default)]
    confirm_password: String,
}

/// `POST /api/principals/{id}/reset-password` (`resetPasswordDirect`). A
/// refusal (the password policy, a federated user) renders the drawer with
/// the dialog open and the reason in it, as the SPA does; the password
/// never goes into a cookie or a URL.
#[page(POST "/ui/(app)/users/{id}/reset-password")]
async fn reset_user_password(cx: &Cx, Form(form): Form<ResetPasswordForm>) -> Result<impl View> {
    let auth = auth(cx)?;
    permit(checks::can_write_principals(auth))?;
    let id = target(cx);
    let outcome = if form.new_password.is_empty() {
        Err(PlatformError::validation("Password is required"))
    } else if form.new_password != form.confirm_password {
        Err(PlatformError::validation("Passwords do not match"))
    } else {
        admin::reset_password(
            principals(cx),
            auth,
            &id,
            ResetPasswordRequest {
                new_password: form.new_password,
                enforce_password_complexity: None,
            },
        )
        .await
    };
    let error = match outcome {
        Ok(_) => {
            set_flash(cx, FlashKind::Success, "Password reset successfully");
            return Err(see_other(detail_href(&id)).into());
        }
        Err(e) if e.status_code().is_client_error() => e.to_string(),
        Err(e) => {
            tracing::error!(error = %e, "fc-web: reset password failed");
            "Failed to reset password".to_owned()
        }
    };
    Ok(view! { user_list(open_id: id, create: None, overlay: None, dialog_error: error) })
}

/// `POST /api/principals/{id}/reset-2fa`.
#[route(POST "/ui/(app)/users/{id}/reset-2fa")]
async fn reset_2fa(cx: &Cx) -> Result<SeeOther> {
    let auth = auth(cx)?;
    permit(checks::can_write_principals(auth))?;
    let id = target(cx);
    let outcome = reset_user_two_factor(&crate::deps(cx).users.two_factor, auth, &id).await;
    let base = detail_href(&id);
    finish(
        cx,
        outcome,
        "Two-factor authentication reset",
        base.clone(),
        base,
    )
}

/// `POST /api/principals/{id}/developer-credential`: renders the drawer
/// with the new secret in a dialog, shown once.
#[page(POST "/ui/(app)/users/{id}/developer-credential")]
async fn generate_developer_credential(cx: &Cx) -> Result<impl View> {
    let auth = auth(cx)?;
    permit(checks::can_write_principals(auth))?;
    let id = target(cx);
    let outcome = set_credential(&crate::deps(cx).users.developer_credentials, auth, &id).await;
    let secret = match outcome {
        Ok(r) => DeveloperSecret {
            client_id: r.id,
            secret: r.client_secret,
        },
        Err(e) => {
            let retry = detail_href(&id);
            return Err(finish(cx, Err::<(), _>(e), "", retry.clone(), retry)?.into());
        }
    };
    Ok(
        view! { user_list(open_id: id, create: None, overlay: Some(Overlay::Secret(secret)), dialog_error: String::new()) },
    )
}

/// `DELETE /api/principals/{id}/developer-credential`.
#[route(POST "/ui/(app)/users/{id}/developer-credential/revoke")]
async fn revoke_developer_credential(cx: &Cx) -> Result<SeeOther> {
    let auth = auth(cx)?;
    permit(checks::can_write_principals(auth))?;
    let id = target(cx);
    let outcome = revoke_credential(&crate::deps(cx).users.developer_credentials, auth, &id).await;
    let base = detail_href(&id);
    finish(
        cx,
        outcome,
        "Developer credential revoked",
        base.clone(),
        base,
    )
}

/// `DELETE /api/principals/{id}`; the drawer closes.
#[route(POST "/ui/(app)/users/{id}/delete")]
async fn delete_user(cx: &Cx) -> Result<SeeOther> {
    let auth = auth(cx)?;
    permit(checks::can_delete_principals(auth))?;
    let id = target(cx);
    let outcome = admin::delete(principals(cx), auth, &id).await;
    finish(
        cx,
        outcome,
        "User deleted",
        LIST.to_owned(),
        detail_href(&id),
    )
}
