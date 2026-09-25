/**
 * Rust additions to Go's page gating (owner decision #8: the SPA gates pages
 * by permission and hides nav items the user can't use).
 *
 * - The route → permission map uses the platform catalogue's codes. Go's SPA
 *   named several that no role grants (e.g. `platform:iam:identity-provider:view`
 *   for `platform:iam:idp:view`), so only a super-admin ever saw those pages.
 * - Pages whose endpoints need anchor reach (the backend's `anchorWith`
 *   reads) also need anchor tier, read from /auth/me's `scope` (a Rust
 *   addition to Go's body). An unknown tier is not held against the user.
 * - The tier, when sent, decides the anchor/client split (a partner with no
 *   home client is not an anchor user).
 */

import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { START_LOCATION, type RouteLocationNormalized } from "vue-router";
import type { User } from "@/stores/auth";

vi.mock("@/api/auth", () => ({
	checkSession: vi.fn(),
	oauthAuthorizeUrl: vi.fn(),
}));

import { useAuthStore } from "@/stores/auth";
import {
	canAccessPath,
	describeRoutePermission,
	getRoutePermission,
	isUnscopedUser,
	landingPath,
	requiresAnchor,
	usePermissionsStore,
	userScope,
} from "@/stores/permissions";
import { authGuard, createRoutePermissionGuard } from "@/router/guards";

function user(over: Partial<User>): User {
	return {
		id: "prn_1",
		email: "u@x.test",
		name: "U",
		clientId: null,
		roles: ["platform:admin"],
		permissions: ["platform:*:*:*", "*"],
		ssoManaged: false,
		scope: "ANCHOR",
		...over,
	};
}

function route(path: string): RouteLocationNormalized {
	return {
		path,
		fullPath: path,
		name: undefined,
		params: {},
		query: {},
		hash: "",
		meta: {},
		matched: [{ beforeEnter: authGuard }],
		redirectedFrom: undefined,
	} as unknown as RouteLocationNormalized;
}

describe("route permissions use the catalogue's codes", () => {
	it("maps pages and their detail pages", () => {
		expect(getRoutePermission("/authentication/identity-providers/idp_1")).toBe(
			"platform:iam:idp:view",
		);
		expect(getRoutePermission("/authentication/oauth-clients")).toBe(
			"platform:auth:oauth-client:view",
		);
		expect(getRoutePermission("/platform/audit-log")).toBe(
			"platform:admin:audit-log:view",
		);
		expect(getRoutePermission("/platform/cors")).toBe(
			"platform:admin:cors-origin:view",
		);
		expect(getRoutePermission("/platform/settings/names")).toBe(
			"platform:admin:config:view",
		);
	});

	it("admits any one of several codes, and describes them all", () => {
		const processViewer = user({
			permissions: ["platform:application-service:process:view"],
		});
		expect(canAccessPath(processViewer, "/processes/prc_1")).toBe(true);
		expect(canAccessPath(processViewer, "/events")).toBe(false);
		expect(describeRoutePermission("/processes")).toBe(
			"platform:messaging:process:view or platform:application-service:process:view",
		);
	});

	it("opens a page to a non-super-admin holding its catalogue permission", () => {
		const idpViewer = user({ permissions: ["platform:iam:idp:view"] });
		expect(canAccessPath(idpViewer, "/authentication/identity-providers")).toBe(
			true,
		);
	});
});

describe("anchor-only pages", () => {
	it("marks the anchorWith pages and their detail pages", () => {
		for (const path of [
			"/clients",
			"/clients/clt_1",
			"/authentication/identity-providers/idp_1",
			"/authentication/email-domain-mappings",
			"/authentication/oauth-clients/new",
			"/platform/cors",
			"/platform/audit-log",
			"/platform/login-attempts",
		]) {
			expect(requiresAnchor(path), path).toBe(true);
		}
		for (const path of [
			"/users",
			"/applications",
			"/events",
			"/dashboard",
			"/profile",
		]) {
			expect(requiresAnchor(path), path).toBe(false);
		}
	});

	it("needs anchor tier and the permission; an unknown tier is not held against the user", () => {
		expect(canAccessPath(user({}), "/clients")).toBe(true);
		expect(canAccessPath(user({ scope: "PARTNER" }), "/clients")).toBe(false);
		expect(
			canAccessPath(
				user({ scope: "CLIENT", clientId: "clt_1" }),
				"/clients/clt_1",
			),
		).toBe(false);
		expect(canAccessPath(user({ scope: null }), "/clients")).toBe(true);
		// Tier is reach, not authority: anchor still needs the permission.
		expect(
			canAccessPath(
				user({ permissions: ["platform:iam:user:view"] }),
				"/clients",
			),
		).toBe(false);
		// Pages that are not anchor-only follow the permission alone.
		expect(
			canAccessPath(
				user({
					scope: "CLIENT",
					clientId: "clt_1",
					permissions: ["platform:iam:user:view"],
				}),
				"/client-administration/users",
			),
		).toBe(true);
	});
});

describe("tier decides scope when /auth/me sends it", () => {
	it("treats a partner without a home client as client-scoped", () => {
		expect(userScope(user({ scope: "PARTNER", clientId: null }))).toBe(
			"client",
		);
		expect(userScope(user({ scope: "ANCHOR", clientId: "clt_home" }))).toBe(
			"anchor",
		);
		expect(userScope(user({ scope: null, clientId: null }))).toBe("anchor");
		expect(userScope(user({ scope: null, clientId: "clt_1" }))).toBe("client");
	});

	it("lets anchor and partner users act for other clients", () => {
		expect(
			isUnscopedUser(user({ scope: "PARTNER", clientId: "clt_home" })),
		).toBe(true);
		expect(isUnscopedUser(user({ scope: "CLIENT", clientId: null }))).toBe(
			false,
		);
		expect(isUnscopedUser(user({ scope: null, clientId: null }))).toBe(true);
	});

	it("never lands a partner on the anchor dashboard", () => {
		expect(landingPath(user({ scope: "PARTNER" }))).toBe(
			"/client-administration/users",
		);
		expect(landingPath(user({}))).toBe("/dashboard");
	});
});

describe("route guard", () => {
	beforeEach(() => setActivePinia(createPinia()));

	it("sends a client-tier user holding the permission away from an anchor-only page", async () => {
		useAuthStore().setUser(user({ scope: "CLIENT", clientId: "clt_1" }));
		const next = vi.fn();
		await createRoutePermissionGuard()(
			route("/clients"),
			route("/events"),
			next,
		);
		expect(next).toHaveBeenCalledWith({ path: "/profile", replace: true });
		expect(usePermissionsStore().permissionDenied?.path).toBe("/clients");
	});

	it("lets an anchor user through", async () => {
		useAuthStore().setUser(user({}));
		const next = vi.fn();
		await createRoutePermissionGuard()(route("/clients"), START_LOCATION, next);
		expect(next).toHaveBeenCalledWith();
	});
});
