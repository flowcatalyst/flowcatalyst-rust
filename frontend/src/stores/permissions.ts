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
 * Route permission requirements mapping.
 * Maps route paths to required permissions.
 */
export const ROUTE_PERMISSIONS: Record<string, string> = {
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
	"/authentication/identity-providers": "platform:iam:identity-provider:view",
	"/authentication/identity-providers/new":
		"platform:iam:identity-provider:create",

	// Authentication - Email Domain Mappings
	"/authentication/email-domain-mappings":
		"platform:iam:email-domain-mapping:view",
	"/authentication/email-domain-mappings/new":
		"platform:iam:email-domain-mapping:create",

	// Authentication - OAuth Clients
	"/authentication/oauth-clients": "platform:iam:oauth-client:view",
	"/authentication/oauth-clients/new": "platform:iam:oauth-client:create",
	// Client-admin reset-approval queue — gated to user-management roles.
	"/authentication/reset-approvals": "platform:iam:user:update",

	// Event Types
	"/event-types": "platform:messaging:event-type:view",
	"/event-types/create": "platform:messaging:event-type:create",

	// Subscriptions
	"/subscriptions": "platform:messaging:subscription:view",
	"/subscriptions/new": "platform:messaging:subscription:create",

	// Dispatch Pools
	"/dispatch-pools": "platform:messaging:dispatch-pool:view",
	"/dispatch-pools/new": "platform:messaging:dispatch-pool:create",

	// Dispatch Jobs
	"/dispatch-jobs": "platform:messaging:dispatch-job:view",

	// Audit Log
	"/platform/audit-log": "platform:admin:audit:view",

	// Developer portal
	"/developer": "platform:developer:application-openapi:view",

	// Service Accounts
	"/identity/service-accounts": "platform:iam:service-account:view",
	"/identity/service-accounts/new": "platform:iam:service-account:create",

	// Connections
	"/connections": "platform:messaging:connection:view",
	"/connections/new": "platform:messaging:connection:create",

	// Processes
	"/processes": "platform:messaging:process:view",
	"/processes/create": "platform:messaging:process:create",

	// Scheduled Jobs
	"/scheduled-jobs": "platform:messaging:scheduled-job:view",
	"/scheduled-jobs/create": "platform:messaging:scheduled-job:create",

	// Events (messaging events)
	"/events": "platform:messaging:event:view",

	// Portal plane (client-delegable via platform:portal-administrator)
	"/identity/portal-users": "platform:iam:portal-user:view",
	"/identity/portal-apps": "platform:iam:portal-user:view",

	// Platform admin + debug pages (anchor-only on the backend; platform
	// admins bypass via the role check in the route guard, everyone else is
	// blocked — matching how /clients is handled).
	"/platform/cors": "platform:admin:cors:view",
	"/platform/login-attempts": "platform:admin:login-attempt:view",
	"/platform/settings/theme": "platform:admin:settings:view",
	"/platform/settings/names": "platform:admin:settings:view",
	"/platform/debug/events": "platform:messaging:event:view-raw",
	"/platform/debug/dispatch-jobs": "platform:messaging:dispatch-job:view",
};

/**
 * Get the required permission for a route path.
 * Handles dynamic routes like /applications/:id
 */
export function getRoutePermission(path: string): string | undefined {
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
 * what "accessible" means.
 */
export function canAccessPath(
	user: { permissions?: string[]; roles?: string[] } | null | undefined,
	path: string,
): boolean {
	// A user with no platform role sees nothing but their profile (the
	// backend enforces the same rule — 403 NO_PLATFORM_ROLE), so even
	// unmapped routes are closed to them.
	if (isRoleless(user)) return path === "/profile";
	const required = getRoutePermission(path);
	if (!required) return true;
	const perms = user?.permissions ?? [];
	return perms.some((p) => permissionMatches(p, required));
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
	user: { clientId?: string | null } | null | undefined,
): "anchor" | "client" {
	return user && user.clientId == null ? "anchor" : "client";
}

/**
 * May a user see/visit something gated to a particular scope? An undefined
 * requirement is open to everyone (subject to permissions elsewhere).
 */
export function canSeeScope(
	user: { clientId?: string | null } | null | undefined,
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
		| { permissions?: string[]; clientId?: string | null }
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
