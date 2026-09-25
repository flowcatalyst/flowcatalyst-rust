import { hasPermission as matchPermission, hasAnyPermission } from "@/utils/permissions";
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

	// Computed: the platform's matching rule (`@/utils/permissions`).
	const hasPermission = computed(
		() => (permission: string) =>
			matchPermission(userPermissions.value, permission),
	);

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
 * What each page needs, keyed by route path; a detail page inherits its
 * list's entry (see [`getRoutePermission`]), and the sidebar hides an item
 * whose route the user cannot enter.
 *
 * Each entry is the permission the platform checks on the page's main
 * endpoint (the list read, or the create for a `new`/`create` page). Where
 * the Rust handler checks only authentication or anchor scope today, the
 * entry is the platform catalogue's permission for that resource: the one Go
 * (the reference the backend is converging on) checks, and the one roles
 * grant. An array means any one of them (the backend's
 * `has_any_permission`).
 *
 * `/dashboard` and `/profile` stay ungated: the dashboard is where a denied
 * navigation lands.
 */
export const ROUTE_PERMISSIONS: Record<string, RoutePermission> = {
	// Identity & Access
	"/users": "platform:iam:user:view",
	"/users/new": "platform:iam:user:create",
	"/identity/service-accounts": "platform:iam:service-account:view",
	"/identity/service-accounts/new": "platform:iam:service-account:create",
	"/authentication/identity-providers": "platform:iam:idp:view",
	"/authentication/identity-providers/new": "platform:iam:idp:create",
	"/authentication/email-domain-mappings": "platform:iam:email-domain-mapping:view",
	"/authentication/email-domain-mappings/new":
		"platform:iam:email-domain-mapping:create",
	"/authentication/oauth-clients": "platform:auth:oauth-client:view",
	"/authentication/oauth-clients/new": "platform:auth:oauth-client:create",
	"/authorization/roles": "platform:iam:role:view",
	"/authorization/permissions": "platform:iam:permission:view",

	// Platform
	"/applications": "platform:admin:application:view",
	"/applications/new": "platform:admin:application:create",
	"/clients": "platform:admin:client:view",
	"/clients/new": "platform:admin:client:create",
	"/platform/cors": "platform:admin:cors-origin:view",
	"/platform/audit-log": "platform:admin:audit-log:view",
	"/platform/login-attempts": "platform:admin:login-attempt:view",
	"/platform/settings/theme": "platform:admin:config:view",
	"/platform/debug/events": "platform:messaging:event:view-raw",
	"/platform/debug/dispatch-jobs": "platform:messaging:dispatch-job:view-raw",

	// Messaging
	"/events": "platform:messaging:event:view",
	"/event-types": "platform:messaging:event-type:view",
	"/event-types/create": [
		"platform:messaging:event-type:create",
		"platform:messaging:event-type:update",
		"platform:messaging:event-type:delete",
	],
	"/subscriptions": "platform:messaging:subscription:view",
	"/subscriptions/new": [
		"platform:messaging:subscription:create",
		"platform:messaging:subscription:update",
		"platform:messaging:subscription:delete",
	],
	"/connections": "platform:messaging:connection:view",
	"/connections/new": "platform:messaging:connection:create",
	"/dispatch-pools": "platform:messaging:dispatch-pool:view",
	"/dispatch-pools/new": "platform:messaging:dispatch-pool:create",
	"/dispatch-jobs": "platform:messaging:dispatch-job:view",
	"/scheduled-jobs": "platform:messaging:scheduled-job:view",
	"/scheduled-jobs/create": "platform:messaging:scheduled-job:create",

	// Functions
	"/functions": "platform:function:function:view",
	"/functions/new": "platform:function:function:manage",
	"/function-domains": "platform:function:function:view",
	"/function-policies": "platform:function:policy:manage",

	// Developer
	"/developer": [
		"platform:developer:application-openapi:view",
		"platform:developer:application-openapi:manage",
	],
	"/processes": [
		"platform:messaging:process:view",
		"platform:application-service:process:view",
	],
	"/processes/create": "platform:messaging:process:create",
};

/**
 * A route's (or nav item's) requirement: one permission code, or several of
 * which any one will do (the backend's `has_any_permission`).
 */
export type RoutePermission = string | readonly string[];

/**
 * The SPA's one access rule, used by the route guard, the sidebar and in-page
 * action gating alike, so a page never hides what the guard admits:
 * - no requirement: allowed;
 * - the user's effective permissions (from `/auth/me`) grant any of the
 *   required codes, wildcards matched as the platform matches them;
 * - a backend that predates `permissions` (`user.permissions === null`):
 *   the old rule, a platform admin role reaches everything and no other
 *   user holds a code.
 *
 * The server enforces every permission regardless; this only decides what
 * the SPA shows.
 */
export function userCan(
	user: { roles: string[]; permissions: string[] | null } | null | undefined,
	required: RoutePermission | undefined,
): boolean {
	if (required === undefined) return true;
	if (!user) return false;
	if (user.permissions === null) return isPlatformAdminRole(user.roles);
	return hasAnyPermission(
		user.permissions,
		typeof required === "string" ? [required] : required,
	);
}

/**
 * Whether a role list carries a platform admin role: the SPA's rule before
 * `/auth/me` carried permissions, kept as the fallback for a backend that
 * does not send them (see [`userCan`]).
 */
export function isPlatformAdminRole(roles: string[]): boolean {
	return roles.some(
		(role) =>
			role === "platform:super-admin" ||
			role === "platform:admin" ||
			(role.toLowerCase().includes("platform") &&
				role.toLowerCase().includes("admin")),
	);
}

/** [`userCan`] for one permission: gating an in-page action. */
export function userHasPermission(
	user: { roles: string[]; permissions: string[] | null } | null | undefined,
	permission: string,
): boolean {
	return userCan(user, permission);
}

/**
 * Anchor-or-partner scope: `/auth/me` only carries a `clientId` for a
 * CLIENT-scope principal, so a principal without one may act for other
 * owners (the platform, or a client it picks). The server's reach rule
 * still decides what each call may touch.
 */
export function isUnscopedUser(
	user: { clientId: string | null } | null | undefined,
): boolean {
	return !!user && !user.clientId;
}

/**
 * The requirement for a route path: its own entry, else the entry of its
 * nearest mapped ancestor, so `/applications/app_1` and
 * `/processes/prc_1/edit` need what `/applications` and `/processes` need.
 * `undefined` for a path with no mapped ancestor (ungated).
 */
export function getRoutePermission(path: string): RoutePermission | undefined {
	const segments = path.split("/").filter(Boolean);
	for (let n = segments.length; n > 0; n--) {
		const required = ROUTE_PERMISSIONS["/" + segments.slice(0, n).join("/")];
		if (required !== undefined) return required;
	}
	return undefined;
}
