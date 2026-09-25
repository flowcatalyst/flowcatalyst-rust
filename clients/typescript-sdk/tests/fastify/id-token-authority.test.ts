/**
 * The interactive login's authority comes from the ID token.
 *
 * FlowCatalyst mints an IDENTITY access token for `authorization_code` by
 * default — no scope, no roles, no applications — so that a user signing in
 * cannot hand the app an API credential. The user's real authority travels on
 * the ID token, narrowed to the OAuth client's applications. An app that reads
 * the principal from the access token therefore sees a role-less user and
 * denies everything it guards.
 *
 * These tests pin that the plugin reads the ID token, that it still works for
 * exchanges that return none, and that a token it cannot tie to this client
 * and this subject fails the login rather than quietly downgrading it.
 */

import { strict as assert } from "node:assert";
import { after, before, describe, it } from "node:test";
import Fastify, { type FastifyInstance } from "fastify";
import {
	flowcatalystAuth,
	defineRbac,
	generateSessionSecret,
} from "../../src/fastify/index.js";
import { mergeIdTokenAuthority } from "../../src/fastify/oidc/claims.js";
import type {
	FcAccessTokenClaims,
	FcIdTokenClaims,
} from "../../src/fastify/oidc/claims.js";
import { startMockIssuer, type MockIssuer } from "./mock-issuer.js";

let issuer: MockIssuer;
let app: FastifyInstance;
let appBase: string;

const CLIENT_ID = "clt_router";

before(async () => {
	issuer = await startMockIssuer();

	app = Fastify({ logger: false });
	await app.register(flowcatalystAuth, {
		baseUrl: issuer.baseUrl,
		clientId: CLIENT_ID,
		clientSecret: "secret_router",
		cookie: { secret: generateSessionSecret(), secure: false, sameSite: "lax" },
		rbac: defineRbac()
			.role("router:executor").grants("router:solves:solve:run")
			.build(),
	});
	app.get("/whoami", { preHandler: app.fc.requireAuth() }, async (req) => ({
		id: req.principal!.id,
		name: req.principal!.name,
		roles: req.principal!.roles,
		canRun: req.principal!.hasPermissionTo(["router:solves:solve:run"]),
	}));

	await app.listen({ port: 0, host: "127.0.0.1" });
	const addr = app.server.address();
	if (!addr || typeof addr === "string") throw new Error("failed to bind test app");
	appBase = `http://127.0.0.1:${addr.port}`;
});

after(async () => {
	await app.close();
	await issuer.stop();
});

/** The identity access token the platform mints by default: zero authority. */
function identityAccessToken(sub: string, name: string): Promise<string> {
	return issuer.signAccessToken({
		sub,
		name,
		roles: [],
		applications: [],
		clients: [],
	});
}

/** Drive login → authorize → callback. Returns the callback response. */
async function login(): Promise<Response> {
	const loginRes = await fetch(`${appBase}/auth/login?returnTo=%2Fwhoami`, {
		redirect: "manual",
	});
	const authorizeUrl = loginRes.headers.get("location")!;
	const stateCookie = pickCookie(loginRes, "fc_oauth_state")!;
	const authRes = await fetch(authorizeUrl, { redirect: "manual" });
	return await fetch(authRes.headers.get("location")!, {
		headers: { cookie: stateCookie },
		redirect: "manual",
	});
}

describe("interactive login authority", () => {
	it("takes the user's roles from the ID token when the access token is identity-only", async () => {
		issuer.setNextTokens({
			accessToken: await identityAccessToken("prn_ops", "Ops User"),
			idToken: await issuer.signIdToken({
				sub: "prn_ops",
				aud: CLIENT_ID,
				roles: ["router:executor"],
				applications: ["router"],
				tier: "ANCHOR",
			}),
			refreshToken: "rt_ops",
			expiresIn: 600,
		});

		const cbRes = await login();
		assert.equal(cbRes.status, 302);
		const session = pickCookie(cbRes, "fc_session");
		assert.ok(session, "expected a session cookie");

		const me = await fetch(`${appBase}/whoami`, { headers: { cookie: session } });
		const body = (await me.json()) as Record<string, unknown>;
		assert.equal(body.id, "prn_ops");
		assert.equal(body.name, "Ops User", "name comes from the access token");
		assert.deepEqual(body.roles, ["router:executor"]);
		assert.equal(body.canRun, true, "RBAC must resolve off the ID token's roles");
	});

	it("falls back to the access token when the exchange returns no ID token", async () => {
		issuer.setNextTokens({
			accessToken: await issuer.signAccessToken({
				sub: "prn_api",
				name: "Api Access User",
				roles: ["router:executor"],
			}),
			refreshToken: "rt_api",
			expiresIn: 600,
		});

		const cbRes = await login();
		assert.equal(cbRes.status, 302);
		const session = pickCookie(cbRes, "fc_session")!;
		const body = (await (
			await fetch(`${appBase}/whoami`, { headers: { cookie: session } })
		).json()) as Record<string, unknown>;
		assert.deepEqual(body.roles, ["router:executor"]);
		assert.equal(body.canRun, true);
	});

	it("rejects an ID token addressed to a different client", async () => {
		issuer.setNextTokens({
			accessToken: await identityAccessToken("prn_aud", "Aud User"),
			idToken: await issuer.signIdToken({
				sub: "prn_aud",
				aud: "clt_someone_else",
				roles: ["router:executor"],
			}),
			refreshToken: "rt_aud",
			expiresIn: 600,
		});

		const cbRes = await login();
		assert.equal(cbRes.status, 500, "must fail the login, not downgrade it");
		assert.equal(pickCookie(cbRes, "fc_session"), null);
	});

	it("rejects an ID token describing a different subject", async () => {
		issuer.setNextTokens({
			accessToken: await identityAccessToken("prn_real", "Real User"),
			idToken: await issuer.signIdToken({
				sub: "prn_other",
				aud: CLIENT_ID,
				roles: ["router:executor"],
			}),
			refreshToken: "rt_sub",
			expiresIn: 600,
		});

		const cbRes = await login();
		assert.equal(cbRes.status, 500);
		assert.equal(pickCookie(cbRes, "fc_session"), null);
	});
});

describe("mergeIdTokenAuthority", () => {
	const access = {
		sub: "prn_1",
		iss: "https://fc.test",
		aud: "flowcatalyst",
		exp: 0,
		iat: 0,
		type: "USER",
		tier: "CLIENT",
		name: "From Access Token",
		clients: [],
		roles: [],
		applications: [],
	} as unknown as FcAccessTokenClaims;

	it("keeps the access token's name when the ID token omits it", () => {
		const merged = mergeIdTokenAuthority(access, {
			sub: "prn_1",
			roles: ["a"],
		} as unknown as FcIdTokenClaims);
		assert.equal(merged.name, "From Access Token");
		assert.deepEqual(merged.roles, ["a"]);
	});

	it("lets the ID token's tier and email win", () => {
		const merged = mergeIdTokenAuthority(access, {
			sub: "prn_1",
			tier: "ANCHOR",
			email: "u@example.com",
		} as unknown as FcIdTokenClaims);
		assert.equal(merged.tier, "ANCHOR");
		assert.equal(merged.email, "u@example.com");
	});

	it("throws when the subjects differ", () => {
		assert.throws(
			() =>
				mergeIdTokenAuthority(access, {
					sub: "prn_2",
				} as unknown as FcIdTokenClaims),
			/subject/,
		);
	});
});

function pickCookie(res: Response, name: string): string | null {
	for (const raw of res.headers.getSetCookie?.() ?? []) {
		if (raw.startsWith(`${name}=`)) return raw.split(";")[0]!;
	}
	return null;
}
