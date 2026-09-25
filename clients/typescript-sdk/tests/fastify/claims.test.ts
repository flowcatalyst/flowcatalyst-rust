import { test } from "node:test";
import assert from "node:assert/strict";

import { claimsToSnapshot, type FcAccessTokenClaims } from "../../src/fastify/oidc/claims.js";

const base = {
	sub: "prn_1",
	iss: "https://fc.test",
	aud: "https://fc.test",
	exp: 0,
	iat: 0,
	type: "USER" as const,
	name: "Pat",
	clients: [] as string[],
	roles: [] as string[],
	applications: [] as string[],
};

// Go layout: tier carries the tenancy tier; scope is a permission list.
test("Go layout: tier claim wins; permission scope is not the tier", () => {
	const snap = claimsToSnapshot(
		{ ...base, tier: "ANCHOR", scope: "platform:iam:portal-user:manage x:y" } as FcAccessTokenClaims,
		"bearer",
	);
	assert.equal(snap.scope, "anchor");
});

// The crash case the portal app hit: a permission-less identity token has
// NO scope claim at all — must not throw, must default to least authority.
test("absent scope + absent tier does not crash and defaults to client", () => {
	const snap = claimsToSnapshot({ ...base } as FcAccessTokenClaims, "session");
	assert.equal(snap.scope, "client");
});

// clients:["*"] marks anchor even without a tier claim.
test("wildcard clients implies anchor when tier is absent", () => {
	const snap = claimsToSnapshot(
		{ ...base, clients: ["*"] } as FcAccessTokenClaims,
		"bearer",
	);
	assert.equal(snap.scope, "anchor");
});

// Legacy Rust-era layout: the tier travelled in `scope`.
test("legacy scope-as-tier still maps", () => {
	const snap = claimsToSnapshot(
		{ ...base, scope: "PARTNER" } as FcAccessTokenClaims,
		"bearer",
	);
	assert.equal(snap.scope, "partner");
});

// Portal-plane logins: the id_token's portal claims survive the merge onto
// the identity access token and surface as principal.portal.
test("portal claims surface as principal.portal", async () => {
	const { mergeIdTokenAuthority } = await import("../../src/fastify/oidc/claims.js");
	const merged = mergeIdTokenAuthority(
		{ ...base, sub: "ptu_1" } as FcAccessTokenClaims,
		{
			...base,
			sub: "ptu_1",
			portal_client_id: "clt_1",
			portal_app_code: "customer-portal",
			portal_app_id: "pta_1",
		},
	);
	const snap = claimsToSnapshot(merged, "session");
	assert.deepEqual(snap.portal, { clientId: "clt_1", appCode: "customer-portal", appId: "pta_1" });

	const platform = claimsToSnapshot({ ...base } as FcAccessTokenClaims, "session");
	assert.equal(platform.portal, undefined, "non-portal logins carry no portal block");
});
