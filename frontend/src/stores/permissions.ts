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
	// Applications
	"/applications": "platform:admin:application:view",
	"/applications/new": "platform:admin:application:create",

	// Clients
	"/clients": "platform:admin:client:view",
	"/clients/new": "platform:admin:client:create",

	// Users
	"/users": "platform:iam:user:view",
	"/users/new": "platform:iam:user:create",

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

	// Functions
	"/functions": "platform:function:function:view",
	"/functions/new": "platform:function:function:manage",
	"/function-domains": "platform:function:domain:manage",
	"/function-policies": "platform:function:policy:manage",
};

/**
 * Whether a role list carries a platform admin role. The route guard lets
 * these through every permission check; in-page action gating uses the same
 * rule so a page never hides an action from a user the guard admitted.
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

/**
 * The route guard's rule, for gating an in-page action: a platform admin
 * role, or the permission itself (or `*`). The server enforces every
 * permission regardless; this only decides what the page shows.
 */
export function userHasPermission(
	user: { roles: string[]; permissions: string[] } | null | undefined,
	permission: string,
): boolean {
	if (!user) return false;
	if (isPlatformAdminRole(user.roles)) return true;
	return (
		user.permissions.includes(permission) || user.permissions.includes("*")
	);
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
 * Get the required permission for a route path.
 * Handles dynamic routes like /applications/:id
 */
export function getRoutePermission(path: string): string | undefined {
	// First check exact match
	if (ROUTE_PERMISSIONS[path]) {
		return ROUTE_PERMISSIONS[path];
	}

	// Check for dynamic routes (e.g., /applications/123 -> /applications)
	// Handle detail pages - they typically need view permission
	const segments = path.split("/").filter(Boolean);
	if (segments.length >= 2) {
		// Check if last segment looks like an ID (not a keyword like 'new' or 'create')
		const lastSegment = segments[segments.length - 1];
		if (
			lastSegment !== "new" &&
			lastSegment !== "create" &&
			lastSegment !== "edit"
		) {
			// Try base path
			const basePath = "/" + segments.slice(0, -1).join("/");
			if (ROUTE_PERMISSIONS[basePath]) {
				return ROUTE_PERMISSIONS[basePath];
			}
		}
	}

	return undefined;
}
