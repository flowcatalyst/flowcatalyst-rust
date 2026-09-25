/**
 * SPA permission gating (owner decision #8): the platform's matching rule,
 * the route → permission map, the sidebar filter and the route guard, with
 * the admin-role fallback for a backend whose `/auth/me` has no
 * `permissions`.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import type { RouteLocationNormalized } from "vue-router";
import { hasAnyPermission, hasPermission, matchesPattern } from "@/utils/permissions";
import { getRoutePermission, userCan, usePermissionsStore } from "@/stores/permissions";
import { NAVIGATION_CONFIG, visibleNavigation } from "@/config/navigation";
import { useAuthStore, type User } from "@/stores/auth";

vi.mock("@/api/auth", () => ({ checkSession: vi.fn() }));
const { createRoutePermissionGuard } = await import("@/router/guards");

function user(permissions: string[] | null, roles: string[] = []): User {
	return { id: "p1", email: "u@acme.test", name: "U", clientId: null, roles, permissions };
}

function routes(groups: ReturnType<typeof visibleNavigation>): string[] {
	return groups.flatMap((g) =>
		g.items.flatMap((i) => (i.children ? i.children.map((c) => c.route!) : [i.route!])),
	);
}

describe("permission matching (role/entity.rs matches_pattern)", () => {
	it("matches four levels, each held level * or equal", () => {
		expect(matchesPattern("platform:iam:user:view", "platform:*:*:*")).toBe(true);
		expect(matchesPattern("platform:iam:user:view", "platform:iam:*:*")).toBe(true);
		expect(matchesPattern("platform:iam:user:view", "platform:*:user:view")).toBe(true);
		expect(matchesPattern("platform:iam:user:view", "*:*:*:*")).toBe(true);
		expect(matchesPattern("platform:iam:user:view", "platform:admin:*:*")).toBe(false);
		expect(matchesPattern("platform:iam:user:view", "platform:iam:user:create")).toBe(false);
	});

	it("never matches a pattern or a permission that is not four levels", () => {
		expect(matchesPattern("platform:iam:user:view", "platform:*")).toBe(false);
		expect(matchesPattern("platform:iam:user:view", "*")).toBe(false);
		expect(matchesPattern("platform:admin", "platform:admin:*:*")).toBe(false);
	});

	it("grants on an exact code or a pattern, and any-of on a list", () => {
		expect(hasPermission(["platform:iam:user:view"], "platform:iam:user:view")).toBe(true);
		expect(hasPermission(["platform:messaging:*:view"], "platform:messaging:event:view")).toBe(
			true,
		);
		expect(hasPermission(["platform:messaging:*:view"], "platform:messaging:event:create")).toBe(
			false,
		);
		expect(hasPermission([], "platform:iam:user:view")).toBe(false);
		// A bare `*` is not a wildcard on the platform either.
		expect(hasPermission(["*"], "platform:iam:user:view")).toBe(false);
		expect(
			hasAnyPermission(
				["platform:application-service:process:view"],
				["platform:messaging:process:view", "platform:application-service:process:view"],
			),
		).toBe(true);
		expect(hasAnyPermission(["platform:iam:user:view"], [])).toBe(false);
	});

	it("backs the permissions store's hasPermission", () => {
		setActivePinia(createPinia());
		const store = usePermissionsStore();
		store.setPermissions(["platform:function:*:*"]);
		expect(store.hasPermission("platform:function:alias:promote")).toBe(true);
		expect(store.hasPermission("platform:iam:user:view")).toBe(false);
	});
});

describe("route permissions", () => {
	it("maps pages to the platform's permission and details to their list's", () => {
		expect(getRoutePermission("/users")).toBe("platform:iam:user:view");
		expect(getRoutePermission("/users/prn_1")).toBe("platform:iam:user:view");
		expect(getRoutePermission("/users/new")).toBe("platform:iam:user:create");
		expect(getRoutePermission("/platform/audit-log")).toBe("platform:admin:audit-log:view");
		expect(getRoutePermission("/authentication/oauth-clients")).toBe(
			"platform:auth:oauth-client:view",
		);
		expect(getRoutePermission("/authentication/identity-providers/idp_1")).toBe(
			"platform:iam:idp:view",
		);
		expect(getRoutePermission("/processes/prc_1/edit")).toEqual([
			"platform:messaging:process:view",
			"platform:application-service:process:view",
		]);
		expect(getRoutePermission("/developer/applications/app_1/versions")).toEqual([
			"platform:developer:application-openapi:view",
			"platform:developer:application-openapi:manage",
		]);
		expect(getRoutePermission("/dashboard")).toBeUndefined();
		expect(getRoutePermission("/profile")).toBeUndefined();
	});

	it("uses the permissions when /auth/me sends them, else the admin-role rule", () => {
		expect(userCan(user(["platform:iam:user:view"]), "platform:iam:user:view")).toBe(true);
		expect(userCan(user(["platform:iam:user:view"]), "platform:iam:role:view")).toBe(false);
		// A role name no longer stands in for permissions once they are sent.
		expect(userCan(user([], ["platform:super-admin"]), "platform:iam:user:view")).toBe(false);
		expect(userCan(user(null, ["platform:super-admin"]), "platform:iam:user:view")).toBe(true);
		expect(userCan(user(null, ["acme:viewer"]), "platform:iam:user:view")).toBe(false);
		expect(userCan(user([]), undefined)).toBe(true);
		expect(userCan(null, "platform:iam:user:view")).toBe(false);
	});
});

describe("sidebar", () => {
	const all = routes(visibleNavigation(NAVIGATION_CONFIG, user(["platform:*:*:*"])));

	it("shows everything to a super admin, by permission or by the fallback", () => {
		expect(all).toContain("/users");
		expect(all).toContain("/platform/debug/events");
		expect(
			routes(visibleNavigation(NAVIGATION_CONFIG, user(null, ["platform:super-admin"]))),
		).toEqual(all);
	});

	it("hides the items a user cannot open, and emptied parents and groups", () => {
		const groups = visibleNavigation(
			NAVIGATION_CONFIG,
			user(["platform:messaging:event:view", "platform:messaging:event-type:view"]),
		);
		expect(routes(groups)).toEqual(["/dashboard", "/events", "/event-types"]);
		expect(groups.map((g) => g.label)).toEqual(["Overview", "Messaging"]);
		// No Settings / Debug parents with nothing under them.
		expect(groups.flatMap((g) => g.items).some((i) => i.children)).toBe(false);
	});

	it("keeps a parent with the children the user may open", () => {
		const groups = visibleNavigation(
			NAVIGATION_CONFIG,
			user(["platform:messaging:dispatch-job:view-raw"]),
		);
		const debug = groups.flatMap((g) => g.items).find((i) => i.label === "Debug");
		expect(debug?.children?.map((c) => c.route)).toEqual(["/platform/debug/dispatch-jobs"]);
	});

	it("shows only the ungated items to a user with no permissions", () => {
		expect(routes(visibleNavigation(NAVIGATION_CONFIG, user([])))).toEqual(["/dashboard"]);
	});
});

describe("route guard", () => {
	beforeEach(() => setActivePinia(createPinia()));

	function navigate(path: string, fromName?: string) {
		const next = vi.fn();
		createRoutePermissionGuard()(
			{ path, fullPath: path } as RouteLocationNormalized,
			{ name: fromName } as RouteLocationNormalized,
			next,
		);
		return next;
	}

	it("redirects a direct visit to a page the user cannot open to the dashboard", () => {
		useAuthStore().setUser(user(["platform:messaging:event:view"]));
		expect(navigate("/users")).toHaveBeenCalledWith("/dashboard");
		const denied = usePermissionsStore().permissionDenied;
		expect(denied?.requiredPermission).toBe("platform:iam:user:view");
		expect(denied?.path).toBe("/users");
	});

	it("stays put when navigating in-app to a page the user cannot open", () => {
		useAuthStore().setUser(user(["platform:messaging:event:view"]));
		expect(navigate("/processes", "events")).toHaveBeenCalledWith(false);
		expect(usePermissionsStore().permissionDenied?.requiredPermission).toBe(
			"platform:messaging:process:view or platform:application-service:process:view",
		);
	});

	it("lets through a permitted page, a wildcard grant and an ungated page", () => {
		useAuthStore().setUser(user(["platform:messaging:*:view"]));
		expect(navigate("/events/evt_1")).toHaveBeenCalledWith();
		expect(navigate("/dashboard")).toHaveBeenCalledWith();
	});

	it("falls back to the admin role when /auth/me has no permissions", () => {
		useAuthStore().setUser(user(null, ["platform:super-admin"]));
		expect(navigate("/users")).toHaveBeenCalledWith();
		useAuthStore().setUser(user(null, ["acme:viewer"]));
		expect(navigate("/users")).toHaveBeenCalledWith("/dashboard");
	});
});
