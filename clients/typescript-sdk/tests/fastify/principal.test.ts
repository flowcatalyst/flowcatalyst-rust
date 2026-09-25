import { strict as assert } from "node:assert";
import { describe, it } from "node:test";
import { buildPrincipal } from "../../src/fastify/principal.js";
import {
	parseApplicationsClaim,
	parseClientsClaim,
} from "../../src/fastify/oidc/claims.js";
import { defineRbac } from "../../src/fastify/rbac.js";

/**
 * Real tokens carry `clients` as `"{id}:{code}"` pairs (or `["*"]`), never
 * bare ids — `snap()` mirrors that, running the given raw pairs through the
 * same {@link parseClientsClaim} the SDK uses when building a snapshot from
 * a token, so these tests exercise the shape `buildPrincipal` actually sees.
 */
function snap(over: Partial<Parameters<typeof buildPrincipal>[0]["snapshot"]> = {}) {
	const clients = over.clients ?? ["clt_a:acme"];
	return {
		id: "prn_x",
		type: "USER" as const,
		scope: "client" as const,
		name: "Test User",
		clients,
		...parseClientsClaim(clients),
		roles: ["billing-admin"],
		applications: ["billing"],
		mechanism: "bearer" as const,
		sessionData: {},
		...over,
	};
}

describe("Principal helpers", () => {
	it("hasRole / hasRoles / hasAnyRole", () => {
		const p = buildPrincipal({
			snapshot: snap({ roles: ["a", "b", "c"] }),
			rbac: undefined,
		});
		assert.equal(p.hasRole("a"), true);
		assert.equal(p.hasRole("z"), false);
		assert.equal(p.hasRoles(["a", "b"]), true);
		assert.equal(p.hasRoles(["a", "z"]), false);
		assert.equal(p.hasAnyRole(["a", "z"]), true);
		assert.equal(p.hasAnyRole(["x", "z"]), false);
	});

	it("permissions resolve through RBAC catalogue", () => {
		const rbac = defineRbac()
			.role("billing-admin").grants("invoice:create", "invoice:read")
			.build();
		const p = buildPrincipal({ snapshot: snap(), rbac });
		assert.equal(p.hasPermissionTo(["invoice:read"]), true);
		assert.equal(p.hasPermissionTo(["invoice:create", "invoice:read"]), true);
		assert.equal(p.hasPermissionTo(["invoice:void"]), false);
		assert.equal(p.hasAnyPermissionTo(["invoice:void", "invoice:read"]), true);
	});

	it("permission wildcards match at any segment depth", () => {
		const rbac = defineRbac()
			.role("admin").grants("billing:*")
			.role("super").grants("*")
			.build();
		const admin = buildPrincipal({
			snapshot: snap({ roles: ["admin"] }),
			rbac,
		});
		assert.equal(admin.hasPermissionTo(["billing:invoice:read"]), true);
		assert.equal(admin.hasPermissionTo(["billing:read"]), true);
		assert.equal(admin.hasPermissionTo(["ticket:read"]), false);

		const sup = buildPrincipal({ snapshot: snap({ roles: ["super"] }), rbac });
		assert.equal(sup.hasPermissionTo(["anything:goes:here"]), true);
	});

	it("anchor scope grants implicit cross-client access", () => {
		const p = buildPrincipal({
			snapshot: snap({ scope: "anchor", clients: ["*"] }),
			rbac: undefined,
		});
		assert.equal(p.isAnchor(), true);
		assert.equal(p.canAccessClient("clt_anything"), true);
	});

	it("non-anchor only sees its own clients", () => {
		const p = buildPrincipal({
			snapshot: snap({ clients: ["clt_a:acme", "clt_b:widgets"] }),
			rbac: undefined,
		});
		assert.equal(p.isAnchor(), false);
		assert.equal(p.canAccessClient("clt_a"), true);
		assert.equal(p.canAccessClient("clt_x"), false);
	});

	// T1: `clients` entries are "{id}:{code}" pairs (ClaimShapes), so
	// canAccessClient must match either half — not the raw pair, which never
	// equals a real caller-supplied id or code.
	it("canAccessClient matches the id half of a claim pair", () => {
		const p = buildPrincipal({
			snapshot: snap({ clients: ["clt_a:acme"] }),
			rbac: undefined,
		});
		assert.equal(p.canAccessClient("clt_a"), true);
	});

	it("canAccessClient matches the code half of a claim pair", () => {
		const p = buildPrincipal({
			snapshot: snap({ clients: ["clt_a:acme"] }),
			rbac: undefined,
		});
		assert.equal(p.canAccessClient("acme"), true);
	});

	it("canAccessClient rejects an id/code unrelated to any claim pair", () => {
		const p = buildPrincipal({
			snapshot: snap({ clients: ["clt_a:acme"] }),
			rbac: undefined,
		});
		assert.equal(p.canAccessClient("clt_z"), false);
		assert.equal(p.canAccessClient("unrelated"), false);
	});

	it("canAccessClient still grants everything on the \"*\" anchor claim", () => {
		const p = buildPrincipal({
			snapshot: snap({ clients: ["*"] }),
			rbac: undefined,
		});
		assert.equal(p.canAccessClient("clt_anything"), true);
		assert.equal(p.canAccessClient("any_code"), true);
	});

	it("permission set is empty when no rbac is provided", () => {
		const p = buildPrincipal({ snapshot: snap(), rbac: undefined });
		assert.equal(p.hasPermissionTo(["anything"]), false);
		assert.equal(p.hasAnyPermissionTo(["a", "b"]), false);
	});
});

describe("applications claim", () => {
	// The claim mirrors `clients`: "{id}:{code}" pairs, or "*" for every
	// application. Apps want ids, codes and the flag separately.
	it("splits id:code pairs", () => {
		const parsed = parseApplicationsClaim(["app_1:alpha", "app_2:beta"]);
		assert.deepEqual(parsed.applications, ["app_1", "app_2"]);
		assert.deepEqual(parsed.applicationCodes, ["alpha", "beta"]);
		assert.equal(parsed.allApplications, false);
	});

	it("reads the wildcard as all-applications", () => {
		const parsed = parseApplicationsClaim(["*"]);
		assert.equal(parsed.allApplications, true);
		assert.deepEqual(parsed.applications, []);
	});

	it("still accepts bare ids from a pre-upgrade token", () => {
		const parsed = parseApplicationsClaim(["app_1", "app_2"]);
		assert.deepEqual(parsed.applications, ["app_1", "app_2"]);
		assert.deepEqual(parsed.applicationCodes, []);
	});

	it("honours the deprecated all_applications boolean", () => {
		// A platform that sets the boolean but not the sentinel must still
		// read as all-applications.
		assert.equal(parseApplicationsClaim([], true).allApplications, true);
	});

	it("keeps a colon inside an application code", () => {
		const parsed = parseApplicationsClaim(["app_1:a:b"]);
		assert.deepEqual(parsed.applications, ["app_1"]);
		assert.deepEqual(parsed.applicationCodes, ["a:b"]);
	});
});

describe("clients claim", () => {
	it("splits id:code pairs", () => {
		const parsed = parseClientsClaim(["clt_a:acme", "clt_b:widgets"]);
		assert.deepEqual(parsed.clientIds, ["clt_a", "clt_b"]);
		assert.deepEqual(parsed.clientCodes, ["acme", "widgets"]);
	});

	it("drops the \"*\" anchor sentinel from the parsed halves", () => {
		const parsed = parseClientsClaim(["*"]);
		assert.deepEqual(parsed.clientIds, []);
		assert.deepEqual(parsed.clientCodes, []);
	});

	it("still accepts a bare id from a pre-upgrade token", () => {
		const parsed = parseClientsClaim(["clt_a"]);
		assert.deepEqual(parsed.clientIds, ["clt_a"]);
		assert.deepEqual(parsed.clientCodes, []);
	});

	it("keeps a colon inside a client code", () => {
		const parsed = parseClientsClaim(["clt_a:a:b"]);
		assert.deepEqual(parsed.clientIds, ["clt_a"]);
		assert.deepEqual(parsed.clientCodes, ["a:b"]);
	});
});
