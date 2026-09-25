import { strict as assert } from "node:assert";
import { describe, it } from "node:test";
import { defineRbac } from "../../src/fastify/rbac.js";

describe("RBAC catalogue", () => {
	it("unions permissions across roles", () => {
		const rbac = defineRbac()
			.role("a").grants("p1", "p2")
			.role("b").grants("p2", "p3")
			.build();
		assert.deepEqual([...rbac.resolve(["a", "b"])].sort(), ["p1", "p2", "p3"]);
	});

	it("ignores unknown roles silently", () => {
		const rbac = defineRbac().role("a").grants("p1").build();
		assert.deepEqual(rbac.resolve(["a", "ghost"]), ["p1"]);
	});

	it("multiple grants on the same role accumulate", () => {
		const rbac = defineRbac()
			.role("a").grants("p1", "p2")
			.role("a").grants("p3")
			.build();
		assert.deepEqual([...rbac.resolve(["a"])].sort(), ["p1", "p2", "p3"]);
	});

	it("rejects empty role name and empty permission", () => {
		const builder = defineRbac();
		assert.throws(() => builder.role(""));
		assert.throws(() => builder.role("a").grants(""));
	});

	it("returns empty array when no roles supplied", () => {
		const rbac = defineRbac().role("a").grants("p1").build();
		assert.deepEqual(rbac.resolve([]), []);
	});

	it("frozen catalogue is decoupled from builder mutations", () => {
		const builder = defineRbac().role("a").grants("p1");
		const cat1 = builder.build();
		builder.role("a").grants("p2");
		// Already-built catalogue should not see the later grant.
		assert.deepEqual(cat1.resolve(["a"]), ["p1"]);
	});
});

describe("qualified vs bare role names", () => {
	// FlowCatalyst names roles "{applicationCode}:{role}", but the claim has
	// reached apps in either form depending on the mint path. An app declares
	// one spelling; both must resolve.
	it("resolves a qualified claim against a bare declaration", () => {
		const rbac = defineRbac()
			.role("bidder")
			.grants("rfp:bid:submit")
			.build();

		assert.deepEqual(rbac.resolve(["rfp:bidder"]), ["rfp:bid:submit"]);
		assert.deepEqual(rbac.resolve(["bidder"]), ["rfp:bid:submit"]);
	});

	it("resolves a bare claim against a qualified declaration", () => {
		const rbac = defineRbac()
			.role("hr:hr-manager")
			.grants("hr:register:role:view")
			.build();

		assert.deepEqual(rbac.resolve(["hr-manager"]), ["hr:register:role:view"]);
		assert.deepEqual(rbac.resolve(["hr:hr-manager"]), [
			"hr:register:role:view",
		]);
	});

	it("keeps a legacy multi-segment role's inner colon", () => {
		const rbac = defineRbac()
			.role("dashboard:user")
			.grants("logistics_portal:dashboard:view")
			.build();

		assert.deepEqual(rbac.resolve(["logistics_portal:dashboard:user"]), [
			"logistics_portal:dashboard:view",
		]);
	});

	it("does not cross-grant when two roles share a bare name", () => {
		// Ambiguous: guessing could hand an "admin" claim the platform-wide
		// role's permissions. Exact matching only.
		const rbac = defineRbac()
			.role("admin")
			.grants("app:thing:read")
			.role("hr:admin")
			.grants("hr:everything:write")
			.build();

		assert.deepEqual(rbac.resolve(["admin"]), ["app:thing:read"]);
		assert.deepEqual(rbac.resolve(["hr:admin"]), ["hr:everything:write"]);
	});

	it("still ignores a foreign app's role", () => {
		const rbac = defineRbac().role("bidder").grants("rfp:bid:submit").build();

		assert.deepEqual(rbac.resolve(["hr:bidder-ish"]), []);
		assert.deepEqual(rbac.resolve(["unrelated"]), []);
	});
});
