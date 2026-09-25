//! The document, the authenticated app frame (sidebar + user menu), and the
//! authentication layer. The frame mirrors `MainLayout.vue` /
//! `AppSidebar.vue` / `UserMenu.vue`.

use fc_platform::AuthContext;
use fc_platform::checks;
use fc_platform::shared::public_api::{LoginThemeResponse, load_login_theme};
use topcoat::{
    Result,
    context::{Cx, memoize},
    cookie::{Cookies, cookies},
    icon::{IconData, icon, iconify::iconify_icon},
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

use crate::auth::{auth, authenticate};
use crate::ui::{TrustedHtml, default_logo, flash::take_flash, flash_banner};

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

/// `/ui` itself: the first trial page the caller may open.
#[route(GET "/ui/(app)")]
async fn home(cx: &Cx) -> Result<SeeOther> {
    let auth = auth(cx)?;
    let target = NAV
        .iter()
        .flat_map(|group| group.items)
        .flat_map(|item| std::iter::once(item).chain(item.children))
        .find(|item| item.href.starts_with("/ui/") && (item.visible)(auth))
        .map_or("/dashboard", |item| item.href);
    Ok(see_other(target))
}

// ------------------------------------------------------------ navigation

struct NavItem {
    label: &'static str,
    icon: IconData,
    href: &'static str,
    visible: fn(&AuthContext) -> bool,
    children: &'static [NavItem],
}

struct NavGroup {
    label: &'static str,
    items: &'static [NavItem],
}

/// A leaf item gated by any one of `perms` (the Vue app's
/// `ROUTE_PERMISSIONS`, `stores/permissions.ts`).
macro_rules! nav {
    ($label:literal, $icon:literal, $href:literal, [$($perm:literal),*]) => {
        NavItem {
            label: $label,
            icon: iconify_icon!($icon),
            href: $href,
            visible: |a| a.has_any_permission(&[$($perm),*]),
            children: &[],
        }
    };
    ($label:literal, $icon:literal, $href:literal, $visible:expr) => {
        NavItem { label: $label, icon: iconify_icon!($icon), href: $href, visible: $visible, children: &[] }
    };
}

fn always(_: &AuthContext) -> bool {
    true
}

fn any_child(_: &AuthContext) -> bool {
    // A parent's visibility is its children's; see `visible_children`.
    true
}

/// `frontend/src/config/navigation.ts`, filtered per caller on the server.
/// The two trial pages point at `/ui/...`; the rest open the Vue app.
const NAV: &[NavGroup] = &[
    NavGroup {
        label: "Overview",
        items: &[nav!("Dashboard", "lucide:house", "/dashboard", always)],
    },
    NavGroup {
        label: "Identity & Access",
        items: &[
            nav!("User Management", "lucide:users", "/users", ["platform:iam:user:view"]),
            nav!("Service Accounts", "lucide:server", "/identity/service-accounts", ["platform:iam:service-account:view"]),
            nav!("Identity Providers", "lucide:id-card", "/authentication/identity-providers", ["platform:iam:idp:view"]),
            nav!("Email Domains", "lucide:mail", "/authentication/email-domain-mappings", ["platform:iam:email-domain-mapping:view"]),
            nav!("OAuth Clients", "lucide:key-round", "/authentication/oauth-clients", ["platform:auth:oauth-client:view"]),
            nav!("Roles", "lucide:shield", "/authorization/roles", ["platform:iam:role:view"]),
            nav!("Permissions", "lucide:lock", "/authorization/permissions", ["platform:iam:permission:view"]),
        ],
    },
    NavGroup {
        label: "Platform",
        items: &[
            nav!("Applications", "lucide:layout-grid", "/applications", ["platform:admin:application:view"]),
            nav!("Clients", "lucide:building-2", "/clients", ["platform:admin:client:view"]),
            nav!("CORS Origins", "lucide:link", "/platform/cors", ["platform:admin:cors-origin:view"]),
            // The audit log API is anchor-only (`audit/api.rs`).
            nav!("Audit Log", "lucide:history", "/ui/audit-log", |a| checks::require_anchor(a).is_ok()),
            nav!("Login Attempts", "lucide:log-in", "/platform/login-attempts", ["platform:admin:login-attempt:view"]),
            NavItem {
                label: "Settings",
                icon: iconify_icon!("lucide:settings"),
                href: "",
                visible: any_child,
                children: &[nav!("Theme", "lucide:palette", "/platform/settings/theme", ["platform:admin:config:view"])],
            },
            NavItem {
                label: "Debug",
                icon: iconify_icon!("lucide:wrench"),
                href: "",
                visible: any_child,
                children: &[
                    nav!("Raw Events", "lucide:database", "/platform/debug/events", ["platform:messaging:event:view-raw"]),
                    nav!("Raw Dispatch Jobs", "lucide:database", "/platform/debug/dispatch-jobs", ["platform:messaging:dispatch-job:view-raw"]),
                ],
            },
        ],
    },
    NavGroup {
        label: "Messaging",
        items: &[
            nav!("Events", "lucide:inbox", "/events", ["platform:messaging:event:view"]),
            nav!("Event Types", "lucide:zap", "/ui/event-types", |a| checks::can_read_event_types(a).is_ok()),
            nav!("Subscriptions", "lucide:bell", "/subscriptions", ["platform:messaging:subscription:view"]),
            nav!("Connections", "lucide:link-2", "/connections", ["platform:messaging:connection:view"]),
            nav!("Dispatch Pools", "lucide:database", "/dispatch-pools", ["platform:messaging:dispatch-pool:view"]),
            nav!("Dispatch Jobs", "lucide:send", "/dispatch-jobs", ["platform:messaging:dispatch-job:view"]),
            nav!("Scheduled Jobs", "lucide:clock", "/scheduled-jobs", ["platform:messaging:scheduled-job:view"]),
        ],
    },
    NavGroup {
        label: "Functions",
        items: &[
            nav!("Functions", "lucide:square-function", "/functions", ["platform:function:function:view"]),
            nav!("Function Domains", "lucide:globe", "/function-domains", ["platform:function:function:view"]),
            nav!("Function Policies", "lucide:shield-check", "/function-policies", ["platform:function:policy:manage"]),
        ],
    },
    NavGroup {
        label: "Developer",
        items: &[
            nav!("Applications", "lucide:book-open", "/developer", ["platform:developer:application-openapi:view", "platform:developer:application-openapi:manage"]),
            nav!("Processes", "lucide:network", "/processes", ["platform:messaging:process:view", "platform:application-service:process:view"]),
        ],
    },
];

fn visible_children<'a>(item: &'a NavItem, auth: &AuthContext) -> Vec<&'a NavItem> {
    item.children.iter().filter(|c| (c.visible)(auth)).collect()
}

fn is_visible(item: &NavItem, auth: &AuthContext) -> bool {
    if item.children.is_empty() {
        (item.visible)(auth)
    } else {
        !visible_children(item, auth).is_empty()
    }
}

fn is_current(href: &str, path: &str) -> bool {
    !href.is_empty() && (path == href || path.starts_with(&format!("{href}/")))
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

    let groups: Vec<(&'static str, Vec<&'static NavItem>)> = NAV
        .iter()
        .map(|g| (g.label, g.items.iter().filter(|i| is_visible(i, auth)).collect::<Vec<_>>()))
        .filter(|(_, items)| !items.is_empty())
        .collect();

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
                                    href=(item.href)
                                    class="fc-nav-item"
                                    title=(item.label)
                                    aria-current=(is_current(item.href, &path).then_some("page"))
                                >
                                    icon(data: item.icon.clone())
                                    <span class="fc-nav-label">(item.label)</span>
                                </a>
                            } else {
                                let children = visible_children(item, auth);
                                let open = children.iter().any(|c| is_current(c.href, &path));
                                <details open=(open)>
                                    <summary class="fc-nav-item" title=(item.label)>
                                        icon(data: item.icon.clone())
                                        <span class="fc-nav-label">(item.label)</span>
                                        icon(data: iconify_icon!("lucide:chevron-right"), attrs: attributes! { class="fc-nav-chevron" })
                                    </summary>
                                    <div class="fc-nav-children">
                                        for child in children {
                                            <a
                                                href=(child.href)
                                                class="fc-nav-child"
                                                aria-current=(is_current(child.href, &path).then_some("page"))
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
                        <span class="block truncate text-sm font-medium">(&display_name)</span>
                        <span class="block truncate text-xs text-white/60">(&email)</span>
                    </span>
                    icon(data: iconify_icon!("lucide:chevron-up"), size: Length::rem(1.0), attrs: attributes! { class="fc-user-text text-white/60" })
                </button>
            </div>
        </aside>

        <div id="fc-user-menu" popover="" class="fc-user-menu">
            <div class="flex items-center gap-3 p-4">
                <span class="fc-avatar fc-avatar-lg">(&user_initials)</span>
                <div class="min-w-0">
                    <p class="truncate text-base font-semibold text-[#1e293b]">(&display_name)</p>
                    <p class="truncate text-sm text-[#64748b]">(&email)</p>
                    if platform_admin {
                        <span class="mt-1 inline-block rounded bg-[#dbeafe] px-2 py-0.5 text-xs font-medium text-[#1d4ed8]">"Platform Admin"</span>
                    }
                </div>
            </div>
            <div class="border-t border-border p-1.5">
                <a href="/profile" class="fc-menu-item">icon(data: iconify_icon!("lucide:user"), size: Length::rem(1.1)) "Profile"</a>
                <a href="/auth/reset-password" class="fc-menu-item">icon(data: iconify_icon!("lucide:key-round"), size: Length::rem(1.1)) "Reset Password"</a>
            </div>
            <div class="border-t border-border p-1.5">
                <form method="post" action="/ui/logout">
                    <button type="submit" class="fc-menu-item danger">icon(data: iconify_icon!("lucide:log-out"), size: Length::rem(1.1)) "Sign Out"</button>
                </form>
            </div>
            <div class="flex justify-between border-t border-border px-4 py-2.5 text-xs text-[#94a3b8]">
                <span>"Version"</span>
                <span>(env!("CARGO_PKG_VERSION"))</span>
            </div>
        </div>

        <div class="fc-main">
            <main class="p-4">
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
