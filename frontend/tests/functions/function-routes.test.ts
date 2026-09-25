/**
 * The function pages' route permissions, nav entries and in-page gating rule.
 */
import { describe, expect, it } from "vitest";
import {
	getRoutePermission,
	isUnscopedUser,
	userHasPermission,
} from "@/stores/permissions";
import { NAVIGATION_CONFIG } from "@/config/navigation";

describe("function route permissions", () => {
	it("maps each function route to the platform's permission", () => {
		expect(getRoutePermission("/functions")).toBe("platform:function:function:view");
		expect(getRoutePermission("/functions/acme.orders.ship")).toBe("platform:function:function:view");
		expect(getRoutePermission("/functions/new")).toBe("platform:function:function:manage");
		expect(getRoutePermission("/function-domains")).toBe("platform:function:domain:manage");
		expect(getRoutePermission("/function-domains/fn.acme.com")).toBe("platform:function:domain:manage");
		expect(getRoutePermission("/function-policies")).toBe("platform:function:policy:manage");
		expect(getRoutePermission("/function-policies/platform")).toBe("platform:function:policy:manage");
	});

	it("gates in-page actions as the route guard does", () => {
		const base = { clientId: null, permissions: [] as string[] };
		expect(userHasPermission({ ...base, roles: ["platform:super-admin"] }, "platform:function:alias:promote")).toBe(true);
		expect(userHasPermission({ ...base, roles: ["acme:viewer"] }, "platform:function:alias:promote")).toBe(false);
		expect(
			userHasPermission(
				{ ...base, roles: ["acme:viewer"], permissions: ["platform:function:alias:promote"] },
				"platform:function:alias:promote",
			),
		).toBe(true);
		expect(userHasPermission(null, "platform:function:function:view")).toBe(false);
	});

	it("treats a user without a home client as unscoped", () => {
		expect(isUnscopedUser({ clientId: null })).toBe(true);
		expect(isUnscopedUser({ clientId: "clt_1" })).toBe(false);
		expect(isUnscopedUser(null)).toBe(false);
	});

	it("lists the three function pages in one nav group", () => {
		const group = NAVIGATION_CONFIG.find((g) => g.label === "Functions");
		expect(group?.items.map((i) => i.route)).toEqual([
			"/functions",
			"/function-domains",
			"/function-policies",
		]);
	});
});
