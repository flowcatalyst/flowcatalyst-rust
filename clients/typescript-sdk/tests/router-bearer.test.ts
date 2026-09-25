import { test } from "node:test";
import assert from "node:assert/strict";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { FlowCatalystClient } from "../src/index.js";

// docs/spec/router-api-auth.md rule 8: the router verifies the same platform
// bearer token, so both in-flight checks must carry it.
test("the router's in-flight checks send the platform bearer token", async () => {
	const seen: { url?: string; authorization?: string }[] = [];
	const router: Server = createServer((req, res) => {
		let body = "";
		req.on("data", (c) => (body += c));
		req.on("end", () => {
			seen.push({ url: req.url, authorization: req.headers.authorization });
			res.writeHead(200, { "content-type": "application/json" });
			res.end(
				req.url?.startsWith("/monitoring/in-flight-messages/check-batch")
					? JSON.stringify({ m1: true })
					: JSON.stringify({ messageId: "m1", inPipeline: false }),
			);
		});
	});
	await new Promise<void>((r) => router.listen(0, "127.0.0.1", r));
	const port = (router.address() as AddressInfo).port;

	try {
		const client = new FlowCatalystClient({
			baseUrl: "http://127.0.0.1:1",
			routerBaseUrl: `http://127.0.0.1:${port}`,
			accessToken: "tok-1",
		});
		const single = await client.router().inPipeline("m1");
		assert.ok(single.isOk(), JSON.stringify(single));
		const batch = await client.router().inPipelineBatch(["m1"]);
		assert.ok(batch.isOk(), JSON.stringify(batch));
		assert.deepEqual(batch._unsafeUnwrap(), { m1: true });

		assert.equal(seen.length, 2);
		for (const request of seen) {
			assert.equal(request.authorization, "Bearer tok-1", request.url);
		}
	} finally {
		router.close();
	}
});
