/**
 * Sync Platform Events and the role editors are offered only to users the BFF
 * lets use them (docs/platform-bug-fixes.md, item 6 of the follow-up): the
 * BFF checks anchor scope and the permission for the operation, and the SPA
 * showed them to everyone, who mostly got a 403.
 */

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import {
	canAssignUserRoles,
	canCreateRoles,
	canDeleteRoles,
	canSyncPlatformEventTypes,
	canSyncPlatformRoles,
	canUpdateRoles,
} from "@/utils/admin-permissions";

const anchor = (...permissions: string[]) => ({ clientId: null, permissions });
const clientUser = (...permissions: string[]) => ({
	clientId: "clt_acme",
	permissions,
});

function source(path: string): string {
	return readFileSync(
		fileURLToPath(new URL(`../src/${path}`, import.meta.url)),
		"utf8",
	);
}

describe("Sync Platform Events", () => {
	it("needs anchor scope and event-type sync or manage", () => {
		expect(
			canSyncPlatformEventTypes(anchor("platform:messaging:event-type:sync")),
		).toBe(true);
		expect(
			canSyncPlatformEventTypes(anchor("platform:messaging:event-type:manage")),
		).toBe(true);
		expect(canSyncPlatformEventTypes(anchor("platform:messaging:*:*"))).toBe(
			true,
		);
		expect(
			canSyncPlatformEventTypes(
				anchor(
					"platform:messaging:event-type:view",
					"platform:messaging:event-type:create",
				),
			),
		).toBe(false);
		expect(
			canSyncPlatformEventTypes(
				clientUser("platform:messaging:event-type:sync"),
			),
		).toBe(false);
		expect(canSyncPlatformEventTypes(null)).toBe(false);
	});

	it("is shown only to them on the event type list and the dashboard", () => {
		const list = source("pages/event-types/EventTypeListPage.vue");
		expect(list).toMatch(
			/<Button\s+v-if="canSyncPlatform"\s+label="Sync Platform Events"/,
		);
		expect(list).toContain("canSyncPlatformEventTypes(authStore.user)");

		const dashboard = source("pages/DashboardPage.vue");
		expect(dashboard).toContain("canSyncPlatformEventTypes(authStore.user)");
		expect(dashboard).toMatch(
			/<div v-if="canSyncEvents" class="sync-card">\s*<div class="sync-icon bg-amber">/,
		);
		expect(dashboard).toContain(
			"canSyncEvents.value ? syncPlatformEvents() : Promise.resolve()",
		);
	});
});

describe("role editors", () => {
	it("each needs anchor scope and the role permission for its operation", () => {
		const all = anchor(
			"platform:iam:role:create",
			"platform:iam:role:update",
			"platform:iam:role:delete",
		);
		expect(canCreateRoles(all)).toBe(true);
		expect(canUpdateRoles(all)).toBe(true);
		expect(canDeleteRoles(all)).toBe(true);
		expect(canSyncPlatformRoles(all)).toBe(true);
		expect(canSyncPlatformRoles(anchor("platform:iam:*:*"))).toBe(true);

		const updateOnly = anchor(
			"platform:iam:role:view",
			"platform:iam:role:update",
		);
		expect(canCreateRoles(updateOnly)).toBe(false);
		expect(canUpdateRoles(updateOnly)).toBe(true);
		expect(canDeleteRoles(updateOnly)).toBe(false);
		expect(canSyncPlatformRoles(updateOnly)).toBe(false);

		const client = clientUser(
			"platform:iam:role:create",
			"platform:iam:role:update",
		);
		expect(canCreateRoles(client)).toBe(false);
		expect(canUpdateRoles(client)).toBe(false);
	});

	it("are shown only to those users", () => {
		const list = source("pages/authorization/RoleListPage.vue");
		expect(list).toMatch(/<Button v-if="canCreate" label="Create Role"/);
		expect(list).toContain(`v-if="data.source === 'DATABASE' && canUpdate"`);
		expect(list).toContain(`v-if="data.source === 'DATABASE' && canDelete"`);

		const drawer = source("pages/authorization/RoleDetailDrawer.vue");
		expect(drawer).toContain(
			`role.value?.source === "DATABASE" && canUpdateRoles(authStore.user)`,
		);

		const editor = source("pages/authorization/RoleEditPage.vue");
		expect(editor).toContain(
			`role.value.source !== "DATABASE" || !canUpdateRoles(authStore.user)`,
		);

		const permissions = source("pages/authorization/PermissionListPage.vue");
		expect(permissions).toMatch(
			/<Button v-if="canCreatePermission" label="Create Permission"/,
		);
		expect(permissions).toContain("canCreateRoles(authStore.user)");

		const dashboard = source("pages/DashboardPage.vue");
		expect(dashboard).toContain("canSyncPlatformRoles(authStore.user)");
		expect(dashboard).toMatch(
			/<div v-if="canSyncRoles" class="sync-card">\s*<div class="sync-icon bg-purple">/,
		);
	});
});

describe("user role assignment", () => {
	it("needs user:assign-roles, not user:update (owner ruling 2026-10-11)", () => {
		expect(canAssignUserRoles(anchor("platform:iam:user:assign-roles"))).toBe(
			true,
		);
		expect(canAssignUserRoles(anchor("platform:iam:*:*"))).toBe(true);
		expect(
			canAssignUserRoles(
				anchor(
					"platform:iam:user:create",
					"platform:iam:user:update",
					"platform:iam:user:delete",
				),
			),
		).toBe(false);
		// A client administrator assigns its client's application roles.
		expect(
			canAssignUserRoles(clientUser("platform:iam:user:assign-roles")),
		).toBe(true);
		expect(canAssignUserRoles(clientUser("platform:iam:user:update"))).toBe(
			false,
		);
		expect(canAssignUserRoles(null)).toBe(false);
	});

	it("hides the role controls from users without it", () => {
		const body = source("pages/users/UserDetailBody.vue");
		expect(body).toContain("canAssignUserRoles(authStore.user)");
		expect(body).toMatch(
			/<template v-if="canAssignRoles" #actions>\s*<Button label="Manage Roles"/,
		);
		expect(body).toContain(
			'<Button v-if="canAssignRoles" label="Assign Roles"',
		);

		const developers = source("pages/developer/DeveloperUsersListPage.vue");
		expect(developers).toContain("canAssignUserRoles(authStore.user)");
		expect(
			developers.match(
				/<Button v-if="canAssignRoles" label="Grant Developer Role"/g,
			),
		).toHaveLength(2);
		expect(developers).toMatch(
			/v-if="canAssignRoles"\s*v-tooltip\.top="'Remove Developer Role'"/,
		);

		const importer = source("components/UserImportDialog.vue");
		expect(importer).toContain("canAssignUserRoles(authStore.user)");
		expect(importer).toContain('<p v-if="!canAssignRoles" class="dialog-note">');
	});
});
