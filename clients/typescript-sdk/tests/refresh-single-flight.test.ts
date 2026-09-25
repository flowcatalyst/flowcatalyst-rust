import { test } from "node:test";
import assert from "node:assert/strict";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { refreshAccessToken } from "../src/fastify/oidc/flow.js";

// Owner ruling 2026-09-25 (backlog item 5): concurrent refreshes of one session
// share one exchange, and a refresh just after it reuses its result, so the SDK
// never presents a rotated-out refresh token and needs no server-side leeway.
test("concurrent and just-after refreshes of the same token make one token request", async () => {
	let requests = 0;
	const server: Server = createServer((req, res) => {
		req.resume();
		req.on("end", () => {
			requests++;
			setTimeout(() => {
				res.writeHead(200, { "content-type": "application/json" });
				res.end(JSON.stringify({ access_token: `at-${requests}`, token_type: "Bearer", expires_in: 3600, refresh_token: `rt-${requests}` }));
			}, 50);
		});
	});
	await new Promise<void>((r) => server.listen(0, "127.0.0.1", r));
	const port = (server.address() as AddressInfo).port;
	const endpoints = {
		issuer: "http://issuer.test",
		authorizationEndpoint: "http://issuer.test/authorize",
		tokenEndpoint: `http://127.0.0.1:${port}/oauth/token`,
		verify: async () => ({ sub: "u1" }),
		verifyIdToken: async () => ({ sub: "u1" }),
	} as unknown as Parameters<typeof refreshAccessToken>[0]["endpoints"];
	const opts = { endpoints, clientId: "c", clientSecret: "s", refreshToken: `rt-old-${Date.now()}` };
	try {
		const [a, b] = await Promise.all([refreshAccessToken(opts), refreshAccessToken(opts)]);
		assert.equal(requests, 1, "joined the in-flight exchange");
		assert.equal(a.accessToken, b.accessToken);
		const c = await refreshAccessToken(opts);
		assert.equal(requests, 1, "reused the result just after");
		assert.equal(c.refreshToken, a.refreshToken);
		await refreshAccessToken({ ...opts, refreshToken: `rt-other-${Date.now()}` });
		assert.equal(requests, 2, "a different refresh token is its own exchange");
	} finally {
		server.close();
	}
});
