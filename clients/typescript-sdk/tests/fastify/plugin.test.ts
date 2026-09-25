/**
 * End-to-end test of `flowcatalystAuth` against a mock OIDC issuer.
 *
 * Boots a Fastify app + mock issuer in-process, then drives the full
 * authorization-code dance with fetch. Covers:
 *
 *   - Bearer-token verification (Authorization header → request.principal)
 *   - Browser flow: login redirect → callback → session cookie issued
 *   - Authenticated request via session cookie
 *   - 401 JSON for missing Bearer on API routes
 *   - 302 redirect for missing session on web routes
 *   - Logout clears the cookie
 *   - RBAC catalogue applied to permission checks
 */

import { strict as assert } from "node:assert";
import { after, before, describe, it } from "node:test";
import Fastify, { type FastifyInstance } from "fastify";
import {
	flowcatalystAuth,
	defineRbac,
	generateSessionSecret,
} from "../../src/fastify/index.js";
import { startMockIssuer, type MockIssuer } from "./mock-issuer.js";

let issuer: MockIssuer;
let app: FastifyInstance;
let appBase: string;

const sessionSecret = generateSessionSecret();
const FC_CLIENT_ID = "clt_app";
const FC_CLIENT_SECRET = "secret_app";

before(async () => {
	issuer = await startMockIssuer();

	const rbac = defineRbac()
		.role("billing-admin").grants("invoice:create", "invoice:read")
		.role("billing-viewer").grants("invoice:read")
		.role("support").grants("ticket:*")
		.build();

	app = Fastify({ logger: false });
	await app.register(flowcatalystAuth, {
		baseUrl: issuer.baseUrl,
		clientId: FC_CLIENT_ID,
		clientSecret: FC_CLIENT_SECRET,
		cookie: {
			secret: sessionSecret,
			secure: false, // tests run over http
			sameSite: "lax",
		},
		rbac,
	});

	app.get("/whoami", { preHandler: app.fc.requireAuth() }, async (req) => ({
		id: req.principal!.id,
		name: req.principal!.name,
		roles: req.principal!.roles,
		canRead: req.principal!.hasPermissionTo(["invoice:read"]),
		canVoid: req.principal!.hasPermissionTo(["invoice:void"]),
		ticketAny: req.principal!.hasPermissionTo(["ticket:close"]),
	}));

	app.get("/api/secret", { preHandler: app.fc.requireBearer() }, async () => ({
		ok: true,
	}));

	app.get("/dashboard", { preHandler: app.fc.requireSession() }, async () => ({
		ok: true,
	}));

	app.post("/auth/logout-extra", async (req, reply) => {
		await app.fc.logout(req, reply, { redirectTo: "/bye" });
	});

	await app.listen({ port: 0, host: "127.0.0.1" });
	const addr = app.server.address();
	if (!addr || typeof addr === "string") {
		throw new Error("failed to bind test app");
	}
	appBase = `http://127.0.0.1:${addr.port}`;
});

after(async () => {
	await app.close();
	await issuer.stop();
});

describe("Bearer flow", () => {
	it("accepts a valid Bearer token and exposes the principal", async () => {
		const token = await issuer.signAccessToken({
			sub: "prn_bearer",
			name: "Bearer User",
			roles: ["billing-viewer"],
		});
		const res = await fetch(`${appBase}/whoami`, {
			headers: { Authorization: `Bearer ${token}` },
		});
		assert.equal(res.status, 200);
		const body = (await res.json()) as Record<string, unknown>;
		assert.equal(body.id, "prn_bearer");
		assert.equal(body.canRead, true);
		assert.equal(body.canVoid, false);
	});

	it("401s JSON when no Bearer is supplied on an API route", async () => {
		const res = await fetch(`${appBase}/api/secret`, {
			headers: { Accept: "application/json" },
			redirect: "manual",
		});
		assert.equal(res.status, 401);
		assert.ok((res.headers.get("www-authenticate") ?? "").includes("Bearer"));
	});

	it("permission wildcard resolves through RBAC", async () => {
		const token = await issuer.signAccessToken({
			sub: "prn_support",
			name: "Support Agent",
			roles: ["support"],
		});
		const res = await fetch(`${appBase}/whoami`, {
			headers: { Authorization: `Bearer ${token}` },
		});
		const body = (await res.json()) as Record<string, unknown>;
		assert.equal(body.ticketAny, true);
	});

	it("ignores a tampered Bearer (drops principal, falls through to guard)", async () => {
		const token = await issuer.signAccessToken({
			sub: "prn_x",
			name: "X",
		});
		const tampered = `${token.slice(0, -3)}aaa`;
		const res = await fetch(`${appBase}/api/secret`, {
			headers: { Authorization: `Bearer ${tampered}`, Accept: "application/json" },
			redirect: "manual",
		});
		assert.equal(res.status, 401);
	});
});

describe("Session flow", () => {
	it("redirects unauthenticated browser to /auth/login", async () => {
		const res = await fetch(`${appBase}/dashboard`, {
			headers: { Accept: "text/html" },
			redirect: "manual",
		});
		assert.equal(res.status, 302);
		assert.ok(res.headers.get("location")?.startsWith("/auth/login"));
	});

	it("401s JSON (no redirect) when an XHR/fetch call misses the session", async () => {
		const res = await fetch(`${appBase}/dashboard`, {
			headers: { Accept: "application/json" },
			redirect: "manual",
		});
		assert.equal(res.status, 401);
		assert.deepEqual(await res.json(), { error: "unauthorized" });
	});

	it("full OIDC flow: login → callback → authenticated request → logout", async () => {
		// 1. Hit /auth/login — expect 302 to issuer's /oauth/authorize and a state cookie.
		issuer.setNextTokens({
			accessToken: await issuer.signAccessToken({
				sub: "prn_session",
				name: "Session User",
				roles: ["billing-admin"],
				email: "session@example.com",
			}),
			refreshToken: "rt_session",
			expiresIn: 600,
		});
		issuer.setNextCode("code_canary");

		const loginRes = await fetch(`${appBase}/auth/login?returnTo=%2Fwhoami`, {
			redirect: "manual",
		});
		assert.equal(loginRes.status, 302);
		const authorizeUrl = loginRes.headers.get("location");
		assert.ok(authorizeUrl?.startsWith(`${issuer.baseUrl}/oauth/authorize`));
		const stateCookie = pickCookie(loginRes, "fc_oauth_state");
		assert.ok(stateCookie, "expected fc_oauth_state cookie");

		// 2. Hit the issuer's authorize URL — it 302s to our /auth/callback with code+state.
		const authRes = await fetch(authorizeUrl!, { redirect: "manual" });
		assert.equal(authRes.status, 302);
		const callbackUrl = authRes.headers.get("location");
		assert.ok(callbackUrl?.startsWith(`${appBase}/auth/callback`));

		// 3. Hit /auth/callback carrying the state cookie. Plugin exchanges the
		//    code, verifies the access token, sets the session cookie, and 302s
		//    back to returnTo.
		const cbRes = await fetch(callbackUrl!, {
			headers: { cookie: stateCookie! },
			redirect: "manual",
		});
		assert.equal(cbRes.status, 302);
		assert.equal(cbRes.headers.get("location"), "/whoami");
		assert.equal(issuer.lastSeenCode(), "code_canary");

		const sessionCookie = pickCookie(cbRes, "fc_session");
		assert.ok(sessionCookie, "expected fc_session cookie");

		// 4. Use the session cookie to hit a guarded route.
		const meRes = await fetch(`${appBase}/whoami`, {
			headers: { cookie: sessionCookie!, Accept: "application/json" },
		});
		assert.equal(meRes.status, 200);
		const body = (await meRes.json()) as Record<string, unknown>;
		assert.equal(body.id, "prn_session");
		assert.equal(body.canRead, true);
		assert.equal(body.canVoid, false);

		// 5. Logout via decorator with redirectTo.
		const logoutRes = await fetch(`${appBase}/auth/logout-extra`, {
			method: "POST",
			headers: { cookie: sessionCookie! },
			redirect: "manual",
		});
		assert.equal(logoutRes.status, 302);
		assert.equal(logoutRes.headers.get("location"), "/bye");
		const cleared = logoutRes.headers.getSetCookie?.() ?? [];
		assert.ok(
			cleared.some((c) => c.startsWith("fc_session=") && c.includes("Expires=")),
			`expected fc_session to be cleared, got ${JSON.stringify(cleared)}`,
		);
	});
});

/**
 * Extract a single Set-Cookie value as `name=value` (no attrs) for echoing
 * back on the next request. Uses node-fetch's getSetCookie() helper.
 */
function pickCookie(res: Response, name: string): string | null {
	const cookies = res.headers.getSetCookie?.() ?? [];
	for (const c of cookies) {
		if (c.startsWith(`${name}=`)) {
			const pair = c.split(";")[0];
			return pair ?? null;
		}
	}
	return null;
}
