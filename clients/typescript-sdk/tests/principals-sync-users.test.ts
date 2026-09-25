import { test } from "node:test";
import assert from "node:assert/strict";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { FlowCatalystClient } from "../src/index.js";

// The application-less user sync posts to /api/principals/sync (NO appCode) and
// carries each user's pre-hashed password verbatim under `passwordHash`.
test("syncUsers posts to /api/principals/sync with passwordHash and no appCode", async () => {
	let seen: { method?: string; url?: string; body?: string } = {};
	const server: Server = createServer((req, res) => {
		let body = "";
		req.on("data", (c) => (body += c));
		req.on("end", () => {
			seen = { method: req.method, url: req.url, body };
			res.writeHead(200, { "content-type": "application/json" });
			res.end(
				JSON.stringify({
					created: 1,
					updated: 0,
					deleted: 0,
					syncedEmails: ["a@example.com"],
				}),
			);
		});
	});
	await new Promise<void>((r) => server.listen(0, "127.0.0.1", r));
	const port = (server.address() as AddressInfo).port;

	try {
		const client = new FlowCatalystClient({
			baseUrl: `http://127.0.0.1:${port}`,
			accessToken: "test-token",
		});
		const res = await client.principals().syncUsers([
			{
				email: "a@example.com",
				name: "A",
				roles: ["admin"],
				passwordHash: "$2y$10$abcdefghijklmnopqrstuv",
			},
		]);

		assert.ok(res.isOk(), `expected ok result, got ${JSON.stringify(res)}`);
		assert.equal(seen.method, "POST");
		assert.equal(seen.url, "/api/principals/sync");
		assert.ok(!seen.url?.includes("applications"), "no application scope in the path");
		assert.ok(
			seen.body?.includes('"passwordHash":"$2y$10$abcdefghijklmnopqrstuv"'),
			`passwordHash must be in the body, got: ${seen.body}`,
		);
	} finally {
		await new Promise<void>((r) => server.close(() => r()));
	}
});

// Owner decision 22 of 2026-09-25: a sync uses passwordHash only to create a
// user, and names the existing users whose hash it ignored.
test("both principal syncs surface passwordHashIgnored", async () => {
	const server: Server = createServer((req, res) => {
		req.resume();
		req.on("end", () => {
			res.writeHead(200, { "content-type": "application/json" });
			res.end(
				JSON.stringify(
					req.url?.startsWith("/api/applications/")
						? {
								applicationCode: "app",
								created: 0,
								updated: 1,
								deleted: 0,
								syncedCodes: ["a@example.com"],
								passwordHashIgnored: ["a@example.com"],
							}
						: {
								created: 0,
								updated: 1,
								deleted: 0,
								syncedEmails: ["a@example.com"],
								passwordHashIgnored: ["a@example.com"],
							},
				),
			);
		});
	});
	await new Promise<void>((r) => server.listen(0, "127.0.0.1", r));
	const port = (server.address() as AddressInfo).port;

	try {
		const client = new FlowCatalystClient({
			baseUrl: `http://127.0.0.1:${port}`,
			accessToken: "test-token",
		});
		const users = [
			{ email: "a@example.com", name: "A", roles: [], passwordHash: "$2y$10$abcdefghijklmnopqrstuv" },
		];
		const platformWide = await client.principals().syncUsers(users);
		assert.deepEqual(platformWide._unsafeUnwrap().passwordHashIgnored, ["a@example.com"]);
		const appScoped = await client.principals().sync("app", users);
		assert.deepEqual(appScoped._unsafeUnwrap().passwordHashIgnored, ["a@example.com"]);
	} finally {
		await new Promise<void>((r) => server.close(() => r()));
	}
});
