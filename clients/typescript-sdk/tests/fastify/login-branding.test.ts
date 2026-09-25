/**
 * Branded sign-in: the optional `client` identifier that names which
 * FlowCatalyst client's login theme the sign-in pages should wear.
 *
 * It is cosmetic — it selects colours and a logo, never who may sign in —
 * so the contract is deliberately forgiving: absent by default, overridable
 * per request, and never sent on the portal plane, which has its own login
 * surface.
 */

import { strict as assert } from "node:assert";
import { after, before, describe, it } from "node:test";
import Fastify, { type FastifyInstance } from "fastify";
import { flowcatalystAuth, generateSessionSecret } from "../../src/fastify/index.js";
import { buildAuthorizeUrl, generateAuthCodeBag } from "../../src/fastify/oidc/flow.js";
import { startMockIssuer, type MockIssuer } from "./mock-issuer.js";

const endpoints = {
	issuer: "https://fc.example",
	authorizationEndpoint: "https://fc.example/oauth/authorize",
	tokenEndpoint: "https://fc.example/oauth/token",
	jwksUri: "https://fc.example/.well-known/jwks.json",
} as Parameters<typeof buildAuthorizeUrl>[0]["endpoints"];

function baseOpts() {
	return {
		endpoints,
		clientId: "oac_1",
		redirectUri: "https://app.example/auth/callback",
		scope: "openid profile email",
		bag: generateAuthCodeBag("/"),
	};
}

describe("buildAuthorizeUrl — client branding param", () => {
	it("omits `client` when not supplied", async () => {
		const url = new URL(await buildAuthorizeUrl(baseOpts()));
		assert.equal(url.searchParams.get("client"), null);
		// The rest of the request must be untouched.
		assert.equal(url.searchParams.get("response_type"), "code");
		assert.equal(url.searchParams.get("client_id"), "oac_1");
		assert.equal(url.searchParams.get("code_challenge_method"), "S256");
	});

	it("sends `client` when supplied, alongside the OAuth client_id", async () => {
		const url = new URL(await buildAuthorizeUrl({ ...baseOpts(), client: "acme" }));
		assert.equal(url.searchParams.get("client"), "acme");
		// `client` (tenant slug) and `client_id` (OAuth client) are distinct.
		assert.equal(url.searchParams.get("client_id"), "oac_1");
	});

	it("percent-encodes the identifier rather than injecting raw", async () => {
		const url = await buildAuthorizeUrl({ ...baseOpts(), client: "a&redirect_uri=evil" });
		assert.ok(!url.includes("client=a&redirect_uri=evil"), `raw injection in ${url}`);
		assert.equal(new URL(url).searchParams.get("redirect_uri"), "https://app.example/auth/callback");
	});
});

describe("login route — client branding", () => {
	let issuer: MockIssuer;
	let app: FastifyInstance;
	let appBase: string;
	const sessionSecret = generateSessionSecret();

	before(async () => {
		issuer = await startMockIssuer();
		app = Fastify({ logger: false });

		// Platform plane, configured with a default client.
		await app.register(async (scoped) => {
			await scoped.register(flowcatalystAuth, {
				baseUrl: issuer.baseUrl,
				clientId: "oac_platform",
				clientSecret: "secret_platform",
				client: "acme",
				cookie: { name: "branded_session", secret: sessionSecret, secure: false },
				routes: {
					login: "/branded/auth/login",
					callback: "/branded/auth/callback",
					logout: "/branded/auth/logout",
				},
			});
		});

		// Portal plane, also configured with a client — which must be ignored.
		await app.register(async (scoped) => {
			await scoped.register(flowcatalystAuth, {
				baseUrl: issuer.baseUrl,
				clientId: "oac_portal",
				clientSecret: "secret_portal",
				portal: true,
				client: "acme",
				cookie: { name: "portal_session", secret: sessionSecret, secure: false },
				routes: {
					login: "/portal/auth/login",
					callback: "/portal/auth/callback",
					logout: "/portal/auth/logout",
				},
			});
		});

		await app.listen({ port: 0 });
		const addr = app.server.address();
		appBase = `http://127.0.0.1:${typeof addr === "object" && addr ? addr.port : 0}`;
	});

	after(async () => {
		await app?.close();
		await issuer?.stop();
	});

	async function authorizeUrlFor(path: string): Promise<URL> {
		const res = await fetch(`${appBase}${path}`, { redirect: "manual" });
		assert.equal(res.status, 302, `expected a redirect from ${path}`);
		return new URL(res.headers.get("location")!);
	}

	it("carries the configured client on the platform login redirect", async () => {
		const url = await authorizeUrlFor("/branded/auth/login");
		assert.equal(url.searchParams.get("client"), "acme");
	});

	it("lets a per-request ?client= override the configured default", async () => {
		const url = await authorizeUrlFor("/branded/auth/login?client=other-co");
		assert.equal(url.searchParams.get("client"), "other-co");
	});

	it("never sends client on the portal plane", async () => {
		const url = await authorizeUrlFor("/portal/auth/login");
		assert.ok(url.pathname.endsWith("/portal/authorize"), `not the portal endpoint: ${url}`);
		assert.equal(url.searchParams.get("client"), null);
	});

	it("ignores a ?client= injected at the portal login route", async () => {
		const url = await authorizeUrlFor("/portal/auth/login?client=acme");
		assert.equal(url.searchParams.get("client"), null);
	});
});
