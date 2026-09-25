/**
 * Anchor-only pages (decision #8 with the /auth/me tier): the SPA reads the
 * caller's `scope` from /auth/me and hides the pages whose endpoints need
 * anchor reach (Go's `anchorWith` reads) from a PARTNER or CLIENT user, even
 * one holding the permission. An unknown tier (older backend) is not held
 * against the user. Also pins how /auth/me's Go-shaped body maps to a user.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import type { RouteLocationNormalized } from "vue-router";
import {
	canEnterRoute,
	isUnscopedUser,
	requiresAnchor,
	usePermissionsStore,
} from "@/stores/permissions";
import { NAVIGATION_CONFIG, visibleNavigation } from "@/config/navigation";
import { useAuthStore, type User, type UserScope } from "@/stores/auth";

vi.mock("@/router", () => ({ default: { push: vi.fn() } }));
const { mapLoginResponseToUser } = await import("@/api/auth");
const { createRoutePermissionGuard } = await import("@/router/guards");

function user(scope: UserScope | null, permissions: string[] = ["platform:*:*:*"]): User {
	return {
		id: "p1",
		email: "u@acme.test",
		name: "U",
		clientId: scope === "CLIENT" ? "clt_1" : null,
		roles: ["platform:super-admin"],
		permissions,
		scope,
	};
}

function routes(groups: ReturnType<typeof visibleNavigation>): string[] {
	return groups.flatMap((g) =>
		g.items.flatMap((i) => (i.children ? i.children.map((c) => c.route!) : [i.route!])),
	);
}

describe("anchor-only pages", () => {
	it("marks Go's anchorWith pages and their detail pages", () => {
		for (const path of [
			"/clients",
			"/clients/new",
			"/clients/clt_1",
			"/authentication/identity-providers/idp_1",
			"/authentication/email-domain-mappings",
			"/authentication/oauth-clients",
			"/platform/cors",
			"/platform/audit-log",
			"/platform/login-attempts",
			"/function-policies/acme",
		]) {
			expect(requiresAnchor(path), path).toBe(true);
		}
		for (const path of ["/users", "/applications", "/events", "/dashboard", "/profile"]) {
			expect(requiresAnchor(path), path).toBe(false);
		}
	});

	it("needs anchor tier and the permission; an unknown tier is not held against the user", () => {
		expect(canEnterRoute(user("ANCHOR"), "/clients")).toBe(true);
		expect(canEnterRoute(user("PARTNER"), "/clients")).toBe(false);
		expect(canEnterRoute(user("CLIENT"), "/clients/clt_1")).toBe(false);
		expect(canEnterRoute(user(null), "/clients")).toBe(true);
		// Tier is reach, not authority: anchor still needs the permission.
		expect(canEnterRoute(user("ANCHOR", ["platform:iam:user:view"]), "/clients")).toBe(false);
		// Pages that are not anchor-only follow the permission alone.
		expect(canEnterRoute(user("CLIENT", ["platform:iam:user:view"]), "/users")).toBe(true);
	});

	it("hides anchor-only nav items from a non-anchor user", () => {
		const anchor = routes(visibleNavigation(NAVIGATION_CONFIG, user("ANCHOR")));
		const client = routes(visibleNavigation(NAVIGATION_CONFIG, user("CLIENT")));
		expect(anchor).toContain("/clients");
		expect(anchor).toContain("/platform/audit-log");
		expect(client).not.toContain("/clients");
		expect(client).not.toContain("/authentication/oauth-clients");
		expect(client).not.toContain("/platform/login-attempts");
		expect(client).toContain("/users");
		expect(client).toContain("/events");
	});

	describe("route guard", () => {
		beforeEach(() => setActivePinia(createPinia()));

		it("refuses an anchor-only page to a CLIENT user holding its permission", () => {
			useAuthStore().setUser(user("CLIENT"));
			const next = vi.fn();
			createRoutePermissionGuard()(
				{ path: "/clients", fullPath: "/clients" } as RouteLocationNormalized,
				{ name: undefined } as RouteLocationNormalized,
				next,
			);
			expect(next).toHaveBeenCalledWith("/dashboard");
			expect(usePermissionsStore().permissionDenied?.path).toBe("/clients");
		});

		it("lets an anchor user through", () => {
			useAuthStore().setUser(user("ANCHOR"));
			const next = vi.fn();
			createRoutePermissionGuard()(
				{ path: "/clients", fullPath: "/clients" } as RouteLocationNormalized,
				{ name: "dashboard" } as RouteLocationNormalized,
				next,
			);
			expect(next).toHaveBeenCalledWith();
		});
	});

	it("decides unscoped by tier when /auth/me sends it", () => {
		expect(isUnscopedUser({ clientId: "clt_home", scope: "PARTNER" })).toBe(true);
		expect(isUnscopedUser({ clientId: null, scope: "CLIENT" })).toBe(false);
		expect(isUnscopedUser({ clientId: null, scope: "ANCHOR" })).toBe(true);
	});
});

describe("/auth/me mapping", () => {
	it("reads Go's body: null permissions are none, and the tier", () => {
		const mapped = mapLoginResponseToUser({
			status: "",
			principalId: "prn_1",
			name: "N",
			email: "n@acme.test",
			roles: [],
			permissions: null,
			clientId: null,
			ssoManaged: false,
			scope: "CLIENT",
		} as Parameters<typeof mapLoginResponseToUser>[0]);
		expect(mapped.permissions).toEqual([]);
		expect(mapped.scope).toBe("CLIENT");
		expect(mapped.id).toBe("prn_1");
	});

	it("keeps the admin-role fallback for a backend that predates permissions", () => {
		const mapped = mapLoginResponseToUser({
			principalId: "prn_1",
			name: "N",
			email: "n@acme.test",
			roles: ["platform:super-admin"],
			clientId: null,
		});
		expect(mapped.permissions).toBeNull();
		expect(mapped.scope).toBeNull();
	});
});
