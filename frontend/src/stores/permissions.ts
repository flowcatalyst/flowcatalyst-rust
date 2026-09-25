import { defineStore } from "pinia";
import { ref, computed } from "vue";

/**
 * Permission denied event payload
 */
export interface PermissionDeniedEvent {
	type: "api" | "route";
	message: string;
	requiredPermission?: string;
	path?: string;
}

export const usePermissionsStore = defineStore("permissions", () => {
	// State
	const userPermissions = ref<string[]>([]);
	const permissionDenied = ref<PermissionDeniedEvent | null>(null);
	const showPermissionModal = ref(false);

	// Computed
	const hasPermission = computed(() => (permission: string) => {
		// Super admin check - if user has platform:super-admin role, they have all permissions
		// This is handled server-side, but we can also check for wildcard
		if (userPermissions.value.includes("*")) {
			return true;
		}
		if (userPermissions.value.includes(permission)) {
			return true;
		}
		// 4-level wildcard pattern matching, mirroring the backend
		// (see role::entity::matches_pattern). A held permission like
		// "platform:*:*:*" grants `platform:developer:application-openapi:view`.
		const required = permission.split(":");
		for (const held of userPermissions.value) {
			const heldParts = held.split(":");
			if (heldParts.length !== required.length) continue;
			let match = true;
			for (let i = 0; i < heldParts.length; i++) {
				if (heldParts[i] !== "*" && heldParts[i] !== required[i]) {
					match = false;
					break;
				}
			}
			if (match) return true;
		}
		return false;
	});

	// Actions
	function setPermissions(permissions: string[]) {
		userPermissions.value = permissions;
	}

	function clearPermissions() {
		userPermissions.value = [];
	}

	function showPermissionDenied(event: PermissionDeniedEvent) {
		permissionDenied.value = event;
		showPermissionModal.value = true;
	}

	function hidePermissionDenied() {
		showPermissionModal.value = false;
		// Clear after animation
		setTimeout(() => {
			permissionDenied.value = null;
		}, 300);
	}

	function handleApiError(status: number, message?: string) {
		if (status === 401) {
			showPermissionDenied({
				type: "api",
				message: "Your session has expired. Please log in again.",
			});
		} else if (status === 403) {
			showPermissionDenied({
				type: "api",
				message:
					message || "You do not have permission to perform this action.",
			});
		}
	}

	return {
		// State
		userPermissions,
		permissionDenied,
		showPermissionModal,
		// Computed
		hasPermission,
		// Actions
		setPermissions,
		clearPermissions,
		showPermissionDenied,
		hidePermissionDenied,
		handleApiError,
	};
});

/**
 * A route's requirement: one permission code, or several of which any one
 * will do (the backend's `has_any_permission`, e.g. a create page whose form
 * also edits).
 */
export type RoutePermission = string | readonly string[];

/**
 * Route permission requirements mapping.
 * Maps route paths to required permissions — the platform catalogue's codes
 * (`role::entity` in Rust, `seed/permissions.go` in Go; the two agree).
 */
export const ROUTE_PERMISSIONS: Record<string, RoutePermission> = {
	// Dashboard — platform-wide stats + role sync, anchor/super-admin only
	// (mirrors the backend /bff/dashboard/stats IsAdmin gate). Non-admins are
	// routed to their profile instead of landing on a page that 403s.
	"/dashboard": "platform:*:*:*",

	// Applications
	"/applications": "platform:admin:application:view",
	"/applications/new": "platform:admin:application:create",

	// Clients
	"/clients": "platform:admin:client:view",
	"/clients/new": "platform:admin:client:create",

	// Users
	"/users": "platform:iam:user:view",
	"/users/new": "platform:iam:user:create",

	// Client-scoped user management (client-administrators) — same permission as
	// the platform users page, separated by scope (see canSeeScope / nav config).
	"/client-administration/users": "platform:iam:user:view",

	// Authorization
	"/authorization/roles": "platform:iam:role:view",
	"/authorization/permissions": "platform:iam:permission:view",

	// Authentication - Identity Providers
	"/authentication/identity-providers": "platform:iam:idp:view",
	"/authentication/identity-providers/new":
		"platform:iam:idp:create",

	// Authentication - Email Domain Mappings
	"/authentication/email-domain-mappings":
		"platform:iam:email-domain-mapping:view",
	"/authentication/email-domain-mappings/new":
		"platform:iam:email-domain-mapping:create",

	// Authentication - OAuth Clients
	"/authentication/oauth-clients": "platform:auth:oauth-client:view",
	"/authentication/oauth-clients/new": "platform:auth:oauth-client:create",
	// Client-admin reset-approval queue — gated to user-management roles.
	"/authentication/reset-approvals": "platform:iam:user:update",

	// Event Types
	"/event-types": "platform:messaging:event-type:view",
	"/event-types/create": [
		"platform:messaging:event-type:create",
		"platform:messaging:event-type:update",
		"platform:messaging:event-type:delete",
	],

	// Subscriptions
	"/subscriptions": "platform:messaging:subscription:view",
	"/subscriptions/new": [
		"platform:messaging:subscription:create",
		"platform:messaging:subscription:update",
		"platform:messaging:subscription:delete",
	],

	// Dispatch Pools
	"/dispatch-pools": "platform:messaging:dispatch-pool:view",
	"/dispatch-pools/new": "platform:messaging:dispatch-pool:create",

	// Dispatch Jobs
	"/dispatch-jobs": "platform:messaging:dispatch-job:view",

	// Audit Log
	"/platform/audit-log": "platform:admin:audit-log:view",

	// Developer portal
	"/developer": [
		"platform:developer:application-openapi:view",
		"platform:developer:application-openapi:manage",
	],

	// Service Accounts
	"/identity/service-accounts": "platform:iam:service-account:view",
	"/identity/service-accounts/new": "platform:iam:service-account:create",

	// Connections
	"/connections": "platform:messaging:connection:view",
	"/connections/new": "platform:messaging:connection:create",

	// Processes
	"/processes": [
		"platform:messaging:process:view",
		"platform:application-service:process:view",
	],
	"/processes/create": "platform:messaging:process:create",

	// Scheduled Jobs
	"/scheduled-jobs": "platform:messaging:scheduled-job:view",
	"/scheduled-jobs/create": "platform:messaging:scheduled-job:create",

	// Events (messaging events)
	"/events": "platform:messaging:event:view",

	// Functions (the list reads with function:view; function:domain:manage
	// and the other function permissions gate in-page actions)
	"/functions": "platform:function:function:view",
	"/functions/new": "platform:function:function:manage",
	"/function-domains": "platform:function:function:view",
	"/function-domains/new": "platform:function:domain:manage",
	"/function-policies": "platform:function:policy:manage",

	// Portal plane (client-delegable via platform:portal-administrator)
	"/identity/portal-users": "platform:iam:portal-user:view",
	"/identity/portal-apps": "platform:iam:portal-user:view",

	// Platform admin + debug pages (anchor-only on the backend; platform
	// admins bypass via the role check in the route guard, everyone else is
	// blocked — matching how /clients is handled).
	"/platform/cors": "platform:admin:cors-origin:view",
	"/platform/login-attempts": "platform:admin:login-attempt:view",
	"/platform/settings/theme": "platform:admin:config:view",
	"/platform/settings/names": "platform:admin:config:view",
	"/platform/debug/events": "platform:messaging:event:view-raw",
	"/platform/debug/dispatch-jobs": "platform:messaging:dispatch-job:view-raw",
};

/**
 * Get the required permission for a route path.
 * Handles dynamic routes like /applications/:id
 */
export function getRoutePermission(path: string): RoutePermission | undefined {
	// Exact match first.
	if (ROUTE_PERMISSIONS[path]) {
		return ROUTE_PERMISSIONS[path];
	}

	// Otherwise walk up the path and inherit the nearest mapped ancestor's
	// permission. This guards every detail/sub page (e.g.
	// /scheduled-jobs/:id/instances, /connections/:id) under its base resource
	// rather than letting deep routes fall through unguarded. Create/edit pages
	// keep their own mapping via the exact match above and otherwise inherit the
	// base view permission as a floor.
	const segments = path.split("/").filter(Boolean);
	for (let i = segments.length - 1; i >= 1; i--) {
		const prefix = "/" + segments.slice(0, i).join("/");
		if (ROUTE_PERMISSIONS[prefix]) {
			return ROUTE_PERMISSIONS[prefix];
		}
	}

	return undefined;
}

/**
 * Does a single held permission satisfy a required one? Mirrors the backend
 * 4-segment wildcard match (e.g. "platform:*:*:*" or "platform:iam:*:*"), so a
 * super-admin's "*"/"platform:*:*:*" grants everything while a client-admin's
 * "platform:iam:user:view" matches only that exact resource/action.
 */
function permissionMatches(held: string, required: string): boolean {
	if (held === "*" || held === required) return true;
	const h = held.split(":");
	const r = required.split(":");
	if (h.length !== r.length) return false;
	for (let i = 0; i < h.length; i++) {
		if (h[i] !== "*" && h[i] !== r[i]) return false;
	}
	return true;
}

/**
 * Can the given user reach a route? A route with no permission requirement is
 * always accessible; otherwise the user must hold a permission that matches the
 * route's requirement. This is the single source of truth used by the route
 * guards, the post-login landing choice, and the sidebar so all three agree on
 * what "accessible" means. An anchor-only page (ANCHOR_ROUTES) also needs
 * anchor tier.
 */
export function canAccessPath(
	user:
		| { permissions?: string[]; roles?: string[]; scope?: string | null }
		| null
		| undefined,
	path: string,
): boolean {
	// A user with no platform role sees nothing but their profile (the
	// backend enforces the same rule — 403 NO_PLATFORM_ROLE), so even
	// unmapped routes are closed to them.
	if (isRoleless(user)) return path === "/profile";
	if (requiresAnchor(path) && lacksAnchor(user)) return false;
	const required = getRoutePermission(path);
	if (!required) return true;
	const perms = user?.permissions ?? [];
	const anyOf = typeof required === "string" ? [required] : required;
	return anyOf.some((r) => perms.some((p) => permissionMatches(p, r)));
}

/** The requirement as one string, for the permission-denied dialog. */
export function describeRoutePermission(path: string): string {
	const required = getRoutePermission(path);
	if (required === undefined) return "";
	return typeof required === "string" ? required : required.join(" or ");
}

/**
 * Pages whose main endpoint needs anchor reach on top of its permission (the
 * backend's `anchorWith` reads: clients, identity providers, email-domain
 * mappings, OAuth clients, CORS origins, login attempts; Rust also keeps
 * anchor on audit logs and function policies). A detail or `new` page inherits its list's entry, as
 * with ROUTE_PERMISSIONS. Owner decision #8: the SPA hides what the user
 * can't use, so a client-tier user holding the permission doesn't see a page
 * whose every call 403s.
 */
export const ANCHOR_ROUTES: string[] = [
	"/clients",
	"/authentication/identity-providers",
	"/authentication/email-domain-mappings",
	"/authentication/oauth-clients",
	"/platform/cors",
	"/platform/audit-log",
	"/platform/login-attempts",
	"/function-policies",
];

/** Whether `path` (or its nearest listed ancestor) is an anchor-only page. */
export function requiresAnchor(path: string): boolean {
	const segments = path.split("/").filter(Boolean);
	for (let n = segments.length; n > 0; n--) {
		if (ANCHOR_ROUTES.includes("/" + segments.slice(0, n).join("/"))) return true;
	}
	return false;
}

/**
 * Whether the user is known not to have anchor reach. The tier comes from
 * `/auth/me`'s `scope` (ANCHOR / PARTNER / CLIENT; a Rust addition to Go's
 * body). A backend that doesn't send it is not held against the user: the
 * server refuses what it must.
 */
export function lacksAnchor(
	user: { scope?: string | null } | null | undefined,
): boolean {
	return !!user?.scope && user.scope !== "ANCHOR";
}

/**
 * Does the user hold a permission (wildcards included)? Reads the session
 * user's own permission list — the same source canAccessPath uses. (The
 * store's hasPermission reads userPermissions, which nothing populates.)
 */
export function userHasPermission(
	user: { permissions?: string[] } | null | undefined,
	permission: string,
): boolean {
	return (user?.permissions ?? []).some((p) => permissionMatches(p, permission));
}

/**
 * A signed-in user holding no platform role and no permission — e.g. an SSO
 * user provisioned on first login but never granted access. They may only
 * reach their own profile.
 */
export function isRoleless(
	user: { permissions?: string[]; roles?: string[] } | null | undefined,
): boolean {
	return (
		!!user &&
		(user.roles ?? []).length === 0 &&
		(user.permissions ?? []).length === 0
	);
}

/**
 * A user's coarse scope, inferred from whether they have a home client. Anchor
 * users (platform admins) have no home client; client- and partner-scoped users
 * (client-administrators) do. Used to split the platform user-management page
 * from the client-scoped one.
 */
export function userScope(
	user: { clientId?: string | null; scope?: string | null } | null | undefined,
): "anchor" | "client" {
	// The tier /auth/me reports decides when present (a partner without a
	// home client is not an anchor user); otherwise infer from the home client.
	if (user?.scope) return user.scope === "ANCHOR" ? "anchor" : "client";
	return user && user.clientId == null ? "anchor" : "client";
}

/**
 * Anchor-or-partner reach: a principal that may act for other owners (the
 * platform, or a client it picks) — pages offer a client picker to these.
 * Decided by the tier /auth/me reports; without it, "no home client".
 */
export function isUnscopedUser(
	user: { clientId?: string | null; scope?: string | null } | null | undefined,
): boolean {
	if (!user) return false;
	if (user.scope) return user.scope !== "CLIENT";
	return !user.clientId;
}

/**
 * May a user see/visit something gated to a particular scope? An undefined
 * requirement is open to everyone (subject to permissions elsewhere).
 */
export function canSeeScope(
	user: { clientId?: string | null; scope?: string | null } | null | undefined,
	required: "anchor" | "client" | undefined,
): boolean {
	if (!required) return true;
	return userScope(user) === required;
}

/**
 * The best landing page for a freshly-authenticated user: the dashboard
 * (anchor/admin) if reachable, else a client-administrator's own user-management
 * page, else the profile — which is always reachable. A user with no access ends
 * up on profile rather than a page that immediately 403s.
 */
export function landingPath(
	user:
		| { permissions?: string[]; clientId?: string | null; scope?: string | null }
		| null
		| undefined,
): string {
	// The dashboard is anchor-scoped, so never land a client-scoped user there
	// even if their permissions happen to match — send them to their own
	// user-management page, falling back to the always-reachable profile.
	if (canSeeScope(user, "anchor") && canAccessPath(user, "/dashboard")) {
		return "/dashboard";
	}
	if (
		userScope(user) === "client" &&
		canAccessPath(user, "/client-administration/users")
	) {
		return "/client-administration/users";
	}
	return "/profile";
}
