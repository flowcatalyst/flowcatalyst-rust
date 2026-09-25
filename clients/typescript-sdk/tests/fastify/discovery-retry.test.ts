import { test } from "node:test";
import assert from "node:assert/strict";

import { createOidcClient } from "../../src/fastify/oidc/discovery.js";

// A failed discovery must not poison the memo: the platform being briefly
// unreachable at first use would otherwise 500 every login in the process
// until restart. Rejection propagates to the caller, then the next call
// retries; a SUCCESSFUL discovery stays memoized.
test("discovery retries after a failed fetch and memoizes success", async () => {
	const originalFetch = globalThis.fetch;
	let calls = 0;
	globalThis.fetch = (async () => {
		calls++;
		if (calls === 1) {
			throw new TypeError("fetch failed");
		}
		return new Response(
			JSON.stringify({
				issuer: "https://fc.test",
				authorization_endpoint: "https://fc.test/oauth/authorize",
				token_endpoint: "https://fc.test/oauth/token",
				jwks_uri: "https://fc.test/.well-known/jwks.json",
			}),
			{ status: 200, headers: { "content-type": "application/json" } },
		);
	}) as typeof fetch;
	try {
		const client = createOidcClient({ baseUrl: "https://fc.test" });

		await assert.rejects(client.endpoints(), /fetch failed/);

		// Second call retries instead of replaying the poisoned promise.
		const endpoints = await client.endpoints();
		assert.equal(endpoints.issuer, "https://fc.test");
		assert.equal(calls, 2);

		// Success IS memoized — no third fetch.
		await client.endpoints();
		assert.equal(calls, 2);
	} finally {
		globalThis.fetch = originalFetch;
	}
});
