import type { NavigationGuardNext, RouteLocationNormalized } from "vue-router";
import { useAuthStore } from "@/stores/auth";
import {
	usePermissionsStore,
	getRoutePermission,
	userCan,
} from "@/stores/permissions";
import { usePlatformConfigStore } from "@/stores/platformConfig";
import { checkSession } from "@/api/auth";

/**
 * Guard that ensures user is authenticated.
 * Redirects to login if not authenticated.
 * Also loads platform configuration on first navigation.
 */
export async function authGuard(
	to: RouteLocationNormalized,
	_from: RouteLocationNormalized,
	next: NavigationGuardNext,
): Promise<void> {
	const authStore = useAuthStore();
	const platformConfigStore = usePlatformConfigStore();

	// Load platform config on first navigation (public endpoint, no auth required)
	if (!platformConfigStore.isLoaded) {
		await platformConfigStore.loadConfig();
	}

	// If already authenticated, allow access
	if (authStore.isAuthenticated) {
		next();
		return;
	}

	// If still loading initial session check, wait for it
	if (authStore.isLoading) {
		const isAuthenticated = await checkSession();
		if (isAuthenticated) {
			next();
			return;
		}
	}

	// Not authenticated - redirect to login
	next({
		path: "/auth/login",
		query: { redirect: to.fullPath },
		replace: true,
	});
}

/**
 * Guard that ensures user is NOT authenticated.
 * Used for login page - redirects to dashboard if already logged in.
 *
 * Special handling for OAuth flow: if oauth=true is in query params and user
 * is already authenticated, redirect to /oauth/authorize to complete the flow.
 */
export async function guestGuard(
	to: RouteLocationNormalized,
	_from: RouteLocationNormalized,
	next: NavigationGuardNext,
): Promise<void> {
	const authStore = useAuthStore();

	// Check session first if still loading
	if (authStore.isLoading) {
		await checkSession();
	}

	// If authenticated, handle redirect
	if (authStore.isAuthenticated) {
		// Check if this is an OIDC interaction flow - complete it
		const interactionUid = to.query['interaction'];
		if (interactionUid && typeof interactionUid === "string") {
			window.location.href = `/oidc/interaction/${interactionUid}/login`;
			return;
		}

		// Check if this is an OAuth flow - redirect to /oauth/authorize to complete it
		if (to.query['oauth'] === "true") {
			const oauthParams = new URLSearchParams();
			const oauthFields = [
				"response_type",
				"client_id",
				"redirect_uri",
				"scope",
				"state",
				"code_challenge",
				"code_challenge_method",
				"nonce",
			];
			for (const field of oauthFields) {
				const value = to.query[field];
				if (value && typeof value === "string") {
					oauthParams.set(field, value);
				}
			}
			// Redirect to OAuth authorize - the session cookie will be sent and auth code issued
			window.location.href = `/oauth/authorize?${oauthParams.toString()}`;
			return;
		}

		// Normal case - redirect to dashboard (replace to avoid back-button loop)
		next({ path: "/dashboard", replace: true });
		return;
	}

	next();
}

/**
 * Guard factory for role-based access.
 */
export function roleGuard(requiredRole: string) {
	return (
		_to: RouteLocationNormalized,
		_from: RouteLocationNormalized,
		next: NavigationGuardNext,
	): void => {
		const authStore = useAuthStore();
		const roles = authStore.user?.roles || [];

		if (roles.includes(requiredRole)) {
			next();
			return;
		}

		// Redirect to unauthorized or dashboard
		next("/dashboard");
	};
}

/**
 * Guard factory for permission-based access.
 */
export function permissionGuard(requiredPermission: string) {
	return (
		to: RouteLocationNormalized,
		from: RouteLocationNormalized,
		next: NavigationGuardNext,
	): void => {
		const authStore = useAuthStore();
		const permissionsStore = usePermissionsStore();

		if (userCan(authStore.user, requiredPermission)) {
			next();
			return;
		}

		// Show permission denied modal
		permissionsStore.showPermissionDenied({
			type: "route",
			message: "You do not have permission to access this page.",
			requiredPermission,
			path: to.fullPath,
		});

		// Stay on current page or go to dashboard if no history
		if (from.name) {
			next(false);
		} else {
			next("/dashboard");
		}
	};
}

/**
 * Global navigation guard that checks route permissions.
 * This should be registered as a global beforeEach guard.
 */
export function createRoutePermissionGuard() {
	return (
		to: RouteLocationNormalized,
		from: RouteLocationNormalized,
		next: NavigationGuardNext,
	): void => {
		const authStore = useAuthStore();
		const permissionsStore = usePermissionsStore();

		// Skip for unauthenticated users (authGuard will handle)
		if (!authStore.isAuthenticated) {
			next();
			return;
		}

		// Skip for routes without permission requirements
		const requiredPermission = getRoutePermission(to.path);
		if (!requiredPermission) {
			next();
			return;
		}

		// The user's permissions grant one of the route's (wildcards
		// included); a backend without `permissions` falls back to the
		// admin-role rule (see `userCan`).
		if (userCan(authStore.user, requiredPermission)) {
			next();
			return;
		}

		// Show permission denied modal
		permissionsStore.showPermissionDenied({
			type: "route",
			message: "You do not have permission to access this page.",
			requiredPermission:
				typeof requiredPermission === "string"
					? requiredPermission
					: requiredPermission.join(" or "),
			path: to.fullPath,
		});

		// Stay on current page or go to dashboard if no history
		if (from.name) {
			next(false);
		} else {
			next("/dashboard");
		}
	};
}
