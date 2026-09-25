import { test } from "node:test";
import assert from "node:assert/strict";
import {
	defineApplication,
	permission,
	permissionToString,
} from "../src/sync/index.js";

test("permissionToString resolves a PermissionInput against a default app", () => {
	const p = permission({ context: "posts", aggregate: "post", action: "view" });
	assert.equal(permissionToString(p, "blog"), "blog:posts:post:view");
});

test("permissionToString lower-cases already-formatted strings", () => {
	assert.equal(
		permissionToString("Already:Formatted:Perm:CODE"),
		"already:formatted:perm:code",
	);
});

test("permissionToString throws when no application can be determined", () => {
	assert.throws(() =>
		permissionToString({ context: "a", aggregate: "b", action: "c" }),
	);
});

test("build() resolves role permission factories to wire strings", () => {
	const view = permission({ context: "posts", aggregate: "post", action: "view" });
	const edit = permission({
		context: "posts",
		aggregate: "post",
		action: "edit",
		application: "override",
	});

	const set = defineApplication("blog")
		.withPermissions([view, edit])
		.withRoles([
			{ name: "editor", permissions: [view, edit, "literal:perm:code:HERE"] },
		])
		.build();

	assert.deepEqual(set.roles?.[0]?.permissions, [
		"blog:posts:post:view",
		"override:posts:post:edit",
		"literal:perm:code:here",
	]);

	// Standalone catalogue gets the application segment defaulted.
	assert.equal(set.permissions?.[0]?.application, "blog");
	assert.equal(set.permissions?.[1]?.application, "override");
});
