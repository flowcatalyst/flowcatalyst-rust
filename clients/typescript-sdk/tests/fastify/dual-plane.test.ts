/**
 * Two flowcatalystAuth registrations in one app — the client-portal shape:
 * an encapsulated "portal" plane registered first, and a root-context
 * (fastify-plugin-wrapped) "management" plane registered second, each with
 * its own session cookie.
 *
 * Regression under test: the root-context instance's onRequest hook runs on
 * EVERY route, including the portal plane's. Before the first-plane-wins
 * guard it overwrote the portal principal with the management session's
 * platform user whenever BOTH cookies were present — so an administrator
 * who had just completed a portal login was rejected by the app's
 * membership gate (non-ptu_ principal) while a plain customer sailed
 * through.
 */

import { strict as assert } from "node:assert";
import { after, before, describe, it } from "node:test";
import Fastify, { type FastifyInstance } from "fastify";
import fp from "fastify-plugin";
import {
	flowcatalystAuth,
	generateSessionSecret,
} from "../../src/fastify/index.js";
import { startMockIssuer, type MockIssuer } from "./mock-issuer.js";

let issuer: MockIssuer;
let app: FastifyInstance;
let appBase: string;

const sessionSecret = generateSessionSecret();

before(async () => {
	issuer = await startMockIssuer();
	app = Fastify({ logger: false });

	// Portal plane: plain register (encapsulated context), registered FIRST.
	await app.register(async (portalApp) => {
		await portalApp.register(flowcatalystAuth, {
			baseUrl: issuer.baseUrl,
			clientId: "clt_portal",
			clientSecret: "secret_portal",
			cookie: { name: "plane_a_session", secret: sessionSecret, secure: false },
			routes: {
				login: "/a/auth/login",
				callback: "/a/auth/callback",
				logout: "/a/auth/logout",
			},
		});
		portalApp.get(
			"/a/whoami",
			{ preHandler: portalApp.fc.requireSession() },
			async (req) => ({ id: req.principal!.id }),
		);
	});

	// Management plane: fp()-wrapped (root context), registered SECOND — its
	// onRequest hook therefore runs on the portal routes above too.
	await app.register(fp(flowcatalystAuth), {
		baseUrl: issuer.baseUrl,
		clientId: "clt_mgmt",
		clientSecret: "secret_mgmt",
		cookie: { name: "plane_b_session", secret: sessionSecret, secure: false },
		routes: {
			login: "/b/auth/login",
			callback: "/b/auth/callback",
			logout: "/b/auth/logout",
		},
	});

	await app.listen({ port: 0, host: "127.0.0.1" });
	const addr = app.server.address();
	if (!addr || typeof addr === "string") throw new Error("failed to bind");
	appBase = `http://127.0.0.1:${addr.port}`;
});

after(async () => {
	await app.close();
	await issuer.stop();
});

/** Complete the code dance on one plane, returning its session cookie. */
async function login(
	loginPath: string,
	cookieName: string,
	sub: string,
): Promise<string> {
	issuer.setNextTokens({
		accessToken: await issuer.signAccessToken({ sub, name: sub }),
		refreshToken: `rt_${sub}`,
		expiresIn: 600,
	});
	const loginRes = await fetch(`${appBase}${loginPath}`, { redirect: "manual" });
	assert.equal(loginRes.status, 302);
	const stateCookie = pickCookie(loginRes, "fc_oauth_state");
	assert.ok(stateCookie, "expected state cookie");

	const authRes = await fetch(loginRes.headers.get("location")!, {
		redirect: "manual",
	});
	assert.equal(authRes.status, 302);

	const cbRes = await fetch(authRes.headers.get("location")!, {
		headers: { cookie: stateCookie! },
		redirect: "manual",
	});
	assert.equal(cbRes.status, 302);
	const session = pickCookie(cbRes, cookieName);
	assert.ok(session, `expected ${cookieName} cookie`);
	return session!;
}

describe("dual-plane auth (portal + management in one app)", () => {
	it("management session must not clobber the portal principal on portal routes", async () => {
		const portalCookie = await login("/a/auth/login", "plane_a_session", "ptu_portal1");
		const adminCookie = await login("/b/auth/login", "plane_b_session", "usr_admin1");

		// Portal cookie alone: the plain customer case.
		const alone = await fetch(`${appBase}/a/whoami`, {
			headers: { cookie: portalCookie, Accept: "application/json" },
		});
		assert.equal(alone.status, 200);
		assert.equal(((await alone.json()) as { id: string }).id, "ptu_portal1");

		// BOTH cookies — the administrator-testing-the-portal case. The portal
		// plane resolved this request first; the management hook must leave it
		// alone. (Pre-fix this returned usr_admin1 and real apps then 403'd.)
		const both = await fetch(`${appBase}/a/whoami`, {
			headers: {
				cookie: `${portalCookie}; ${adminCookie}`,
				Accept: "application/json",
			},
		});
		assert.equal(both.status, 200);
		assert.equal(((await both.json()) as { id: string }).id, "ptu_portal1");
	});
});

function pickCookie(res: Response, name: string): string | null {
	const cookies = res.headers.getSetCookie?.() ?? [];
	for (const c of cookies) {
		if (c.startsWith(`${name}=`)) return c.split(";")[0] ?? null;
	}
	return null;
}
