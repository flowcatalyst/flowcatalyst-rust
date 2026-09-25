import {
	START_LOCATION,
	type NavigationGuardNext,
	type RouteLocationNormalized,
} from "vue-router";
import { useAuthStore } from "@/stores/auth";
import {
	usePermissionsStore,
	getRoutePermission,
	canAccessPath,
	canSeeScope,
	userScope,
	landingPath,
} from "@/stores/permissions";
import { usePlatformConfigStore } from "@/stores/platformConfig";
import { checkSession, oauthAuthorizeUrl } from "@/api/auth";

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

		// Check if this is an OAuth flow - redirect to /oauth/authorize to
		// complete it (shared field list — see api/auth.ts). The session
		// cookie is sent and the auth code issued.
		if (to.query['oauth'] === "true") {
			window.location.href = oauthAuthorizeUrl((field) => {
				const value = to.query[field];
				return typeof value === "string" ? value : null;
			});
			return;
		}

		// Normal case - go to the dashboard, or the profile if the user has no
		// roles (replace to avoid a back-button loop).
		next({ path: landingPath(authStore.user), replace: true });
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
		_from: RouteLocationNormalized,
		next: NavigationGuardNext,
	): void => {
		const authStore = useAuthStore();
		const permissionsStore = usePermissionsStore();
		const permissions = authStore.user?.permissions || [];

		if (permissions.includes(requiredPermission)) {
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

		// Direct the user to their profile — somewhere they can always access.
		next({ path: "/profile", replace: true });
	};
}

/**
 * Global navigation guard that checks route permissions.
 * This should be registered as a global beforeEach guard.
 */
export function createRoutePermissionGuard() {
	return async (
		to: RouteLocationNormalized,
		from: RouteLocationNormalized,
		next: NavigationGuardNext,
	): Promise<void> => {
		const authStore = useAuthStore();
		const permissionsStore = usePermissionsStore();

		// A cold page load reaches this global guard BEFORE the route's
		// authGuard has hydrated the session, so isAuthenticated is still
		// false. Settle the session first on authenticated routes — otherwise
		// the permission checks below never run for the very navigation a
		// role-less user types in (e.g. /dashboard would stay put instead of
		// landing on /profile). authGuard then finds the session ready.
		if (!authStore.isAuthenticated && authStore.isLoading && requiresAuth(to)) {
			await checkSession();
		}

		// Unauthenticated users pass through — authGuard redirects to login.
		if (!authStore.isAuthenticated) {
			next();
			return;
		}

		// Profile is always reachable — it's where we send users who can't go
		// elsewhere, so never bounce them away from it.
		if (to.path === "/profile") {
			next();
			return;
		}

		// Scope-restricted routes (e.g. the platform vs client-scoped user pages)
		// send the wrong-scope user to their own equivalent instead of a bare
		// denial, so a client-admin who follows a /users link lands on their page.
		const requiredScope = (to.meta as { scope?: "anchor" | "client" }).scope;
		if (!canSeeScope(authStore.user, requiredScope)) {
			next({
				path:
					userScope(authStore.user) === "client"
						? "/client-administration/users"
						: "/users",
				replace: true,
			});
			return;
		}

		// Accessible routes (no requirement, or the user holds a matching
		// permission — wildcards included) are allowed through.
		if (canAccessPath(authStore.user, to.path)) {
			next();
			return;
		}

		// Only surface the "permission denied" modal when the user DELIBERATELY
		// navigated to a forbidden page from somewhere in the app. An automatic
		// landing — the initial page load, straight after login, or a redirect
		// such as "/" -> "/dashboard" — should quietly route a no-access user to
		// their profile instead of greeting them with a denial dialog.
		const isAutomaticLanding =
			from === START_LOCATION ||
			from.path.startsWith("/auth") ||
			to.redirectedFrom != null;
		if (!isAutomaticLanding) {
			permissionsStore.showPermissionDenied({
				type: "route",
				message: "You do not have permission to access this page.",
				requiredPermission: getRoutePermission(to.path) ?? "",
				path: to.fullPath,
			});
		}

		// Direct the user to their profile — somewhere they can always access.
		next({ path: "/profile", replace: true });
	};
}

/** Whether any matched route record is guarded by authGuard. */
function requiresAuth(to: RouteLocationNormalized): boolean {
	return to.matched.some((record) => {
		const guard = record.beforeEnter;
		return guard === authGuard || (Array.isArray(guard) && guard.includes(authGuard));
	});
}
