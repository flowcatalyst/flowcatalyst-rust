//! The document, the authenticated app frame (sidebar + user menu), and the
//! authentication layer. The frame mirrors the SPA's `MainLayout.vue` /
//! `AppSidebar.vue` / `SidebarProfile.vue`.

use fc_platform::shared::public_api::{LoginThemeResponse, load_login_theme};
use topcoat::{
    Result,
    context::{Cx, memoize},
    cookie::{Cookies, cookies},
    icon::{icon, iconify::iconify_icon},
    router::{
        Body, Method, Next, Slot,
        error::{SeeOther, redirect, see_other, unauthorized},
        layer, layout,
        request::{original_parts, uri},
        response::Response,
        route,
    },
    tailwind,
    view::{Length, View, attributes, error_boundary, view},
};

use super::nav;
use crate::auth::{auth, authenticate};
use crate::ui::{TrustedHtml, default_logo, flash::take_flash, flash_banner};
use fc_platform::auth::oidc_login_api::{AuthMethod, resolve_auth_method};

/// Persists the sidebar's collapsed state (the Vue app keeps it in
/// localStorage; a cookie lets the server render it without a flash).
const SIDEBAR_COOKIE: &str = "fc_sidebar";

/// The platform theme (logo, brand), shared by the sidebar and login page:
/// what the Vue app's `appTheme` store loads from `/api/public/login-theme`.
#[memoize]
pub(crate) async fn theme(cx: &Cx) -> LoginThemeResponse {
    load_login_theme(&crate::deps(cx).platform_config_repo).await
}

/// Every `/ui` page: the HTML document, styles, the Topcoat runtime.
#[layout("/ui")]
async fn document(cx: &Cx, slot: Slot<'_>) -> Result<impl View> {
    let collapsed = cookies(cx)
        .get(SIDEBAR_COOKIE)
        .is_some_and(|c| c.value() == "collapsed");
    Ok(view! {
        <!DOCTYPE html>
        <html lang="en" class=(collapsed.then_some("fc-collapsed"))>
            <head>
                <meta charset="utf-8">
                <meta name="viewport" content="width=device-width, initial-scale=1">
                <title>"FlowCatalyst Platform"</title>
                <link rel="icon" type="image/svg+xml" href="/favicon.svg">
                topcoat::dev::script()
                topcoat::runtime::script()
                <link rel="stylesheet" href=(tailwind::stylesheet!())>
                <script type="module" src=(topcoat::asset::asset!("./ui.js"))></script>
            </head>
            <body>(slot)</body>
        </html>
    })
}

/// Authentication for everything under `/ui/(app)`: pages, routes, and the
/// shards and procedures registered there. Resolves the caller once and
/// hands the [`AuthContext`] down; handlers still check permissions.
#[layer("/ui/(app)")]
async fn require_session(cx: &Cx, body: Body, next: Next<'_>) -> Result<Response> {
    match authenticate(cx).await? {
        Some(auth) => {
            let cx = cx.with(auth);
            next.run(&cx, body).await
        }
        None => {
            // A browser navigation goes to the login page and comes back;
            // a form post, shard or procedure call is just refused.
            let original = original_parts(cx);
            if original.method == Method::GET {
                let next_path = original
                    .uri
                    .path_and_query()
                    .map_or("/ui", |pq| pq.as_str());
                let target = form_urlencoded::Serializer::new(String::new())
                    .append_pair("next", next_path)
                    .finish();
                Err(redirect(format!("/ui/login?{target}")).into())
            } else {
                Err(unauthorized().into())
            }
        }
    }
}

/// `/ui` itself: the first ported page the caller may open (the SPA's
/// post-login landing, `landingPath`, over fc-web's pages), else the SPA.
#[route(GET "/ui/(app)")]
async fn home(cx: &Cx) -> Result<SeeOther> {
    let auth = auth(cx)?;
    let target = nav::PORTED
        .iter()
        .find(|route| nav::can_access(auth, route))
        .map_or_else(|| "/profile".to_owned(), |route| nav::href(route));
    Ok(see_other(target))
}

/// "Andrew Graaff" -> "AG", as `stores/auth.ts` `userInitials`.
fn initials(name: &str) -> String {
    let parts: Vec<&str> = name.split_whitespace().collect();
    match parts.as_slice() {
        [] => "?".to_owned(),
        [one] => one.chars().take(2).collect::<String>().to_uppercase(),
        [first, .., last] => format!(
            "{}{}",
            first.chars().next().unwrap_or_default(),
            last.chars().next().unwrap_or_default()
        )
        .to_uppercase(),
    }
}

// ------------------------------------------------------------- the frame

/// The signed-in frame: sidebar (logo, navigation, user menu) and content.
#[layout("/ui/(app)")]
async fn app_frame(cx: &Cx, slot: Slot<'_>) -> Result<impl View> {
    let auth = auth(cx)?;
    let flash = take_flash(cx);
    let theme = theme(cx).await.clone();
    let path = uri(cx).path().to_owned();

    let display_name = if !auth.name.is_empty() {
        auth.name.clone()
    } else {
        auth.email.clone().unwrap_or_else(|| "Unknown".to_owned())
    };
    let user_initials = initials(&display_name);
    let email = auth.email.clone().unwrap_or_default();
    // `stores/auth.ts` `isPlatformAdmin`.
    let platform_admin = auth.roles.iter().any(|r| r.starts_with("platform:"));
    let logo_height = theme.logo_height.unwrap_or(40);

    let groups = nav::visible_groups(auth);
    // SidebarProfile's Reset Password: an account whose domain signs in
    // through an external IdP gets a notice instead of the reset page.
    let external_idp = {
        let deps = crate::deps(cx);
        matches!(
            resolve_auth_method(
                &deps.anchor_domain_repo,
                &deps.edm_repo,
                &deps.idp_repo,
                &email
            )
            .await,
            Ok(AuthMethod::External { .. })
        )
    };

    Ok(view! {
        <aside class="fc-sidebar">
            <div class="fc-sidebar-header">
                <a href="/dashboard" class="fc-sidebar-logo" style=(format!("height: {logo_height}px"))>
                    if let Some(url) = theme.logo_url.clone() {
                        <img src=(url) alt=(theme.brand_name.clone().unwrap_or_else(|| "Logo".to_owned()))>
                    } else if let Some(svg) = theme.logo_svg.clone() {
                        (TrustedHtml(svg))
                    } else {
                        <span style=(format!("width: {logo_height}px; height: {logo_height}px"))>(default_logo())</span>
                    }
                </a>
                <button
                    type="button"
                    class="fc-collapse-btn"
                    aria-label="Collapse sidebar"
                    onclick=(format!(
                        "const c=document.documentElement.classList.toggle('fc-collapsed');\
                         document.cookie='{SIDEBAR_COOKIE}='+(c?'collapsed':'')+';path=/ui;max-age=31536000;samesite=lax'"
                    ))
                >
                    icon(data: iconify_icon!("lucide:chevron-left"), size: Length::rem(1.0))
                </button>
            </div>

            <nav class="fc-sidebar-nav">
                for (label, items) in groups {
                    <div class="fc-nav-group">
                        <span class="fc-nav-group-label">(label)</span>
                        for item in items {
                            if item.children.is_empty() {
                                <a
                                    href=(nav::href(item.route))
                                    class="fc-nav-item"
                                    title=(item.label)
                                    aria-current=(nav::is_current(item.route, &path).then_some("page"))
                                >
                                    icon(data: item.icon.clone())
                                    <span class="fc-nav-label">(item.label)</span>
                                </a>
                            } else {
                                let children = nav::visible_children(item, auth);
                                let open = children.iter().any(|c| nav::is_current(c.route, &path));
                                <details open=(open)>
                                    <summary class="fc-nav-item" title=(item.label)>
                                        icon(data: item.icon.clone())
                                        <span class="fc-nav-label">(item.label)</span>
                                        icon(data: iconify_icon!("lucide:chevron-right"), attrs: attributes! { class="fc-nav-chevron" })
                                    </summary>
                                    <div class="fc-nav-children">
                                        for child in children {
                                            <a
                                                href=(nav::href(child.route))
                                                class="fc-nav-child"
                                                aria-current=(nav::is_current(child.route, &path).then_some("page"))
                                            >
                                                icon(data: child.icon.clone())
                                                <span class="fc-nav-label">(child.label)</span>
                                            </a>
                                        }
                                    </div>
                                </details>
                            }
                        }
                    </div>
                }
            </nav>

            <div class="fc-sidebar-footer">
                <button type="button" class="fc-user-trigger" popovertarget="fc-user-menu">
                    <span class="fc-avatar">(&user_initials)</span>
                    <span class="fc-user-text min-w-0 flex-1">
                        <span class="fc-user-name">(&display_name)</span>
                        <span class="fc-user-email">(&email)</span>
                    </span>
                    icon(data: iconify_icon!("lucide:chevron-up"), size: Length::px(11.0), attrs: attributes! { class="fc-user-text shrink-0 text-white/50" })
                </button>
            </div>
        </aside>

        <div id="fc-user-menu" popover="" class="fc-user-menu">
            <div class="flex items-center gap-3 p-4">
                <span class="fc-avatar fc-avatar-lg">(&user_initials)</span>
                <div class="min-w-0">
                    <p class="truncate text-[15px] font-semibold text-[#1e293b]">(&display_name)</p>
                    <p class="truncate text-[13px] text-[#64748b]">(&email)</p>
                    if platform_admin {
                        <span class="mt-1 inline-block rounded px-2 py-0.5 text-[11px] font-medium bg-[#dbeafe] text-[#1d4ed8]">"Platform Admin"</span>
                    }
                </div>
            </div>
            <div class="border-t border-border p-2">
                <a href="/profile" class="fc-menu-item">icon(data: iconify_icon!("lucide:user"), size: Length::rem(1.1)) "Profile"</a>
                if external_idp {
                    <button type="button" class="fc-menu-item" commandfor="fc-idp-notice" command="show-modal">icon(data: iconify_icon!("lucide:key-round"), size: Length::rem(1.1)) "Reset Password"</button>
                } else {
                    <a href="/auth/reset-password" class="fc-menu-item">icon(data: iconify_icon!("lucide:key-round"), size: Length::rem(1.1)) "Reset Password"</a>
                }
            </div>
            <div class="border-t border-border p-2">
                <form method="post" action="/ui/logout">
                    <button type="submit" class="fc-menu-item danger">icon(data: iconify_icon!("lucide:log-out"), size: Length::rem(1.1)) "Sign Out"</button>
                </form>
            </div>
            <div class="flex justify-between border-t border-border px-4 py-2.5 text-xs text-[#94a3b8]">
                <span>"Version"</span>
                <span class="text-[#64748b]">(env!("CARGO_PKG_VERSION"))</span>
            </div>
        </div>
        <dialog id="fc-idp-notice" class="fc-dialog w-[400px] max-w-[calc(100vw-2rem)]" closedby="any" aria-labelledby="fc-idp-notice-title">
            <div class="fc-dialog-header"><span id="fc-idp-notice-title">"External Identity Provider"</span></div>
            <div class="fc-dialog-body flex flex-col gap-3 text-[#475569]">
                <p>"Your account is managed by an external identity provider."</p>
                <p>"To reset your password, please visit your organization's identity provider portal."</p>
            </div>
            <div class="fc-dialog-footer">
                <button type="button" class="fc-btn fc-btn-secondary" commandfor="fc-idp-notice" command="close">"Close"</button>
            </div>
        </dialog>

        <div class="fc-main">
            <main class="p-[24px]">
                flash_banner(flash: flash)
                error_boundary(
                    fallback: |error| {
                        use topcoat::router::StatusCode;
                        use topcoat::router::error::{ForbiddenError, NotFoundError};
                        let (status, message) = if error.downcast_ref::<ForbiddenError>().is_some() {
                            (StatusCode::FORBIDDEN, "You don't have permission to view this page.")
                        } else if error.downcast_ref::<NotFoundError>().is_some() {
                            (StatusCode::NOT_FOUND, "Not found.")
                        } else {
                            return Err(error);
                        };
                        Ok(view! {
                            (status)
                            <div class="fc-card text-[#64748b]">(message)</div>
                        })
                    },
                    (slot)
                )
            </main>
        </div>
    })
}

/// Sign out: clear the session cookie, back to the login page.
#[route(POST "/ui/logout")]
async fn logout(cx: &Cx) -> Result<SeeOther> {
    let cookie = crate::deps(cx).auth_state.session_cookie.clear_cookie();
    cookies(cx).add(cookie);
    Ok(see_other("/ui/login"))
}
