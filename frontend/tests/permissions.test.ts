/**
 * Access rules shared by the route guards, the sidebar, and pages.
 *
 * userHasPermission reads the session user's own permission list. The
 * Portal Apps page used the permissions store's hasPermission instead, whose
 * list nothing ever populates — so its write controls never rendered for
 * anyone. These pin the helper that replaced it, and the profile-only rule
 * for users without a platform role.
 */

import { describe, expect, it } from "vitest";
import { canAccessPath, isRoleless, userHasPermission } from "@/stores/permissions";

const portalAdmin = {
	roles: ["platform:portal-administrator"],
	permissions: ["platform:iam:portal-user:view", "platform:iam:portal-user:manage"],
};
const superAdmin = { roles: ["platform:super-admin"], permissions: ["platform:*:*:*"] };
const roleless = { roles: [], permissions: [] };

describe("userHasPermission", () => {
	it("matches an exact permission", () => {
		expect(userHasPermission(portalAdmin, "platform:iam:portal-user:manage")).toBe(true);
	});
	it("matches through a wildcard", () => {
		expect(userHasPermission(superAdmin, "platform:iam:portal-user:manage")).toBe(true);
	});
	it("refuses a permission the user lacks", () => {
		expect(
			userHasPermission({ roles: ["r"], permissions: ["platform:iam:portal-user:view"] },
				"platform:iam:portal-user:manage"),
		).toBe(false);
	});
	it("refuses a missing user", () => {
		expect(userHasPermission(null, "platform:iam:portal-user:manage")).toBe(false);
	});
});

describe("role-less users see only their profile", () => {
	it("recognises a role-less user", () => {
		expect(isRoleless(roleless)).toBe(true);
		expect(isRoleless(portalAdmin)).toBe(false);
		expect(isRoleless(null)).toBe(false);
	});
	it("closes every route but /profile — mapped or not", () => {
		expect(canAccessPath(roleless, "/profile")).toBe(true);
		expect(canAccessPath(roleless, "/dashboard")).toBe(false);
		expect(canAccessPath(roleless, "/identity/portal-users")).toBe(false);
		expect(canAccessPath(roleless, "/some/unmapped/route")).toBe(false);
	});
	it("still opens mapped routes to users holding the permission", () => {
		expect(canAccessPath(portalAdmin, "/identity/portal-apps")).toBe(true);
		expect(canAccessPath(portalAdmin, "/dashboard")).toBe(false);
	});
});
