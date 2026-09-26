//! The sidebar: `frontend/src/config/navigation.ts`, gated by the SPA's
//! own access rule (`stores/permissions.ts` `canAccessPath` +
//! `canSeeScope`), evaluated per caller on the server.
//!
//! Items name the SPA route. A section fc-web has ported opens at
//! `/ui<route>` (see [`PORTED`]); every other item opens the SPA.

use fc_platform::AuthContext;
use fc_platform::UserScope;
use topcoat::icon::{IconData, iconify::iconify_icon};

/// SPA routes fc-web serves at `/ui<route>`.
pub const PORTED: &[&str] = &[
    "/users",
    "/event-types",
    "/subscriptions",
    "/connections",
    "/dispatch-pools",
    "/clients",
    "/applications",
    "/authorization/roles",
    "/events",
    "/dispatch-jobs",
    "/platform/audit-log",
];

/// `NavItem.scope`: an item for one audience only.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Audience {
    Anchor,
    Client,
}

pub struct NavItem {
    pub label: &'static str,
    pub icon: IconData,
    /// The SPA route; empty for a parent.
    pub route: &'static str,
    pub scope: Option<Audience>,
    pub children: &'static [NavItem],
}

pub struct NavGroup {
    pub label: &'static str,
    pub items: &'static [NavItem],
}

macro_rules! item {
    ($label:literal, $icon:literal, $route:literal) => {
        NavItem {
            label: $label,
            icon: iconify_icon!($icon),
            route: $route,
            scope: None,
            children: &[],
        }
    };
    ($label:literal, $icon:literal, $route:literal, $scope:expr) => {
        NavItem {
            label: $label,
            icon: iconify_icon!($icon),
            route: $route,
            scope: Some($scope),
            children: &[],
        }
    };
}

/// `NAVIGATION_CONFIG`, PrimeIcons swapped for their Lucide equivalents.
pub const NAV: &[NavGroup] = &[
    NavGroup {
        label: "Overview",
        items: &[item!(
            "Dashboard",
            "lucide:house",
            "/dashboard",
            Audience::Anchor
        )],
    },
    NavGroup {
        label: "Identity & Access",
        items: &[
            item!(
                "User Management",
                "lucide:users",
                "/users",
                Audience::Anchor
            ),
            item!(
                "Service Accounts",
                "lucide:server",
                "/identity/service-accounts"
            ),
            item!(
                "Developer Users",
                "lucide:code",
                "/identity/developer-users",
                Audience::Anchor
            ),
            item!(
                "Identity Providers",
                "lucide:id-card",
                "/authentication/identity-providers"
            ),
            item!(
                "Email Domains",
                "lucide:mail",
                "/authentication/email-domain-mappings"
            ),
            item!(
                "OAuth Clients",
                "lucide:key-round",
                "/authentication/oauth-clients"
            ),
            item!(
                "Roles",
                "lucide:shield",
                "/authorization/roles",
                Audience::Anchor
            ),
            item!(
                "Permissions",
                "lucide:lock",
                "/authorization/permissions",
                Audience::Anchor
            ),
        ],
    },
    NavGroup {
        label: "Client Administration",
        items: &[
            item!(
                "User Management",
                "lucide:users",
                "/client-administration/users",
                Audience::Client
            ),
            item!(
                "Reset Approvals",
                "lucide:shield",
                "/authentication/reset-approvals"
            ),
        ],
    },
    NavGroup {
        label: "Portal",
        items: &[
            item!("Portal Apps", "lucide:app-window", "/identity/portal-apps"),
            item!("Portal Users", "lucide:globe", "/identity/portal-users"),
        ],
    },
    NavGroup {
        label: "Platform",
        items: &[
            item!("Applications", "lucide:layout-grid", "/applications"),
            item!("Clients", "lucide:building-2", "/clients"),
            item!("CORS Origins", "lucide:link", "/platform/cors"),
            item!("Audit Log", "lucide:history", "/platform/audit-log"),
            item!(
                "Documentation",
                "lucide:book",
                "/platform/docs",
                Audience::Anchor
            ),
            item!(
                "Login Attempts",
                "lucide:log-in",
                "/platform/login-attempts"
            ),
            NavItem {
                label: "Settings",
                icon: iconify_icon!("lucide:settings"),
                route: "",
                scope: None,
                children: &[
                    item!("Theme", "lucide:palette", "/platform/settings/theme"),
                    item!("Names", "lucide:tag", "/platform/settings/names"),
                ],
            },
            NavItem {
                label: "Debug",
                icon: iconify_icon!("lucide:wrench"),
                route: "",
                scope: None,
                children: &[
                    item!("Raw Events", "lucide:database", "/platform/debug/events"),
                    item!(
                        "Raw Dispatch Jobs",
                        "lucide:database",
                        "/platform/debug/dispatch-jobs"
                    ),
                ],
            },
        ],
    },
    NavGroup {
        label: "Messaging",
        items: &[
            item!("Events", "lucide:inbox", "/events"),
            item!("Event Types", "lucide:zap", "/event-types"),
            item!("Subscriptions", "lucide:bell", "/subscriptions"),
            item!("Connections", "lucide:link", "/connections"),
            item!("Dispatch Pools", "lucide:database", "/dispatch-pools"),
            item!("Dispatch Jobs", "lucide:send", "/dispatch-jobs"),
            item!("Scheduled Jobs", "lucide:clock", "/scheduled-jobs"),
        ],
    },
    NavGroup {
        label: "Functions",
        items: &[
            item!("Functions", "lucide:code", "/functions"),
            item!("Function Domains", "lucide:globe", "/function-domains"),
            item!("Function Policies", "lucide:shield", "/function-policies"),
        ],
    },
    NavGroup {
        label: "Developer",
        items: &[
            item!("Applications", "lucide:book", "/developer"),
            item!("Processes", "lucide:network", "/processes"),
        ],
    },
];

/// `ROUTE_PERMISSIONS` for the routes the sidebar links (list pages; a
/// detail page inherits its list's entry). Any one code will do.
fn route_permission(route: &str) -> Option<&'static [&'static str]> {
    Some(match route {
        "/dashboard" => &["platform:*:*:*"],
        "/applications" => &["platform:admin:application:view"],
        "/clients" => &["platform:admin:client:view"],
        "/users" | "/client-administration/users" => &["platform:iam:user:view"],
        "/authorization/roles" => &["platform:iam:role:view"],
        "/authorization/permissions" => &["platform:iam:permission:view"],
        "/authentication/identity-providers" => &["platform:iam:idp:view"],
        "/authentication/email-domain-mappings" => &["platform:iam:email-domain-mapping:view"],
        "/authentication/oauth-clients" => &["platform:auth:oauth-client:view"],
        "/authentication/reset-approvals" => &["platform:iam:user:update"],
        "/event-types" => &["platform:messaging:event-type:view"],
        "/subscriptions" => &["platform:messaging:subscription:view"],
        "/dispatch-pools" => &["platform:messaging:dispatch-pool:view"],
        "/dispatch-jobs" => &["platform:messaging:dispatch-job:view"],
        "/platform/audit-log" => &["platform:admin:audit-log:view"],
        "/developer" => &[
            "platform:developer:application-openapi:view",
            "platform:developer:application-openapi:manage",
        ],
        "/identity/service-accounts" => &["platform:iam:service-account:view"],
        "/connections" => &["platform:messaging:connection:view"],
        "/processes" => &[
            "platform:messaging:process:view",
            "platform:application-service:process:view",
        ],
        "/scheduled-jobs" => &["platform:messaging:scheduled-job:view"],
        "/events" => &["platform:messaging:event:view"],
        "/functions" | "/function-domains" => &["platform:function:function:view"],
        "/function-policies" => &["platform:function:policy:manage"],
        "/identity/portal-users" | "/identity/portal-apps" => &["platform:iam:portal-user:view"],
        "/platform/cors" => &["platform:admin:cors-origin:view"],
        "/platform/login-attempts" => &["platform:admin:login-attempt:view"],
        "/platform/settings/theme" | "/platform/settings/names" => &["platform:admin:config:view"],
        "/platform/debug/events" => &["platform:messaging:event:view-raw"],
        "/platform/debug/dispatch-jobs" => &["platform:messaging:dispatch-job:view-raw"],
        _ => return None,
    })
}

/// `ANCHOR_ROUTES`: pages whose main endpoint also needs anchor reach.
const ANCHOR_ROUTES: &[&str] = &[
    "/clients",
    "/authentication/identity-providers",
    "/authentication/email-domain-mappings",
    "/authentication/oauth-clients",
    "/platform/cors",
    "/platform/audit-log",
    "/platform/login-attempts",
    "/function-policies",
];

/// `isRoleless`: signed in, but no role and no permission.
pub fn is_roleless(auth: &AuthContext) -> bool {
    auth.roles.is_empty() && auth.permissions.is_empty()
}

/// `canAccessPath` for a sidebar route.
pub fn can_access(auth: &AuthContext, route: &str) -> bool {
    if is_roleless(auth) {
        return route == "/profile";
    }
    if ANCHOR_ROUTES.contains(&route) && !auth.is_anchor() {
        return false;
    }
    route_permission(route).is_none_or(|any| auth.has_any_permission(any))
}

/// `canSeeScope`: the tier decides (the SPA reads `/auth/me`'s `scope`).
fn can_see(auth: &AuthContext, scope: Option<Audience>) -> bool {
    match scope {
        None => true,
        Some(Audience::Anchor) => auth.scope == UserScope::Anchor,
        Some(Audience::Client) => auth.scope != UserScope::Anchor,
    }
}

/// Where an item opens: fc-web's page when ported, else the SPA's.
pub fn href(route: &str) -> String {
    if PORTED.contains(&route) {
        format!("/ui{route}")
    } else {
        route.to_owned()
    }
}

/// `visibleItem`: a leaf the caller can reach, or a parent with at least
/// one such child (keeping only those).
pub fn visible_children<'a>(item: &'a NavItem, auth: &AuthContext) -> Vec<&'a NavItem> {
    item.children
        .iter()
        .filter(|c| can_see(auth, c.scope) && can_access(auth, c.route))
        .collect()
}

pub fn is_visible(item: &NavItem, auth: &AuthContext) -> bool {
    if !can_see(auth, item.scope) {
        return false;
    }
    if item.children.is_empty() {
        can_access(auth, item.route)
    } else {
        !visible_children(item, auth).is_empty()
    }
}

/// The groups with their visible items; empty groups dropped.
pub fn visible_groups(auth: &AuthContext) -> Vec<(&'static str, Vec<&'static NavItem>)> {
    NAV.iter()
        .map(|g| {
            (
                g.label,
                g.items
                    .iter()
                    .filter(|i| is_visible(i, auth))
                    .collect::<Vec<_>>(),
            )
        })
        .filter(|(_, items)| !items.is_empty())
        .collect()
}

/// `isActive`: the route or anything under it.
pub fn is_current(route: &str, path: &str) -> bool {
    let href = href(route);
    !route.is_empty() && (path == href || path.starts_with(&format!("{href}/")))
}
