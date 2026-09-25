import { test } from "node:test";
import assert from "node:assert/strict";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { FlowCatalystClient } from "../src/index.js";
import { defineApplication } from "../src/sync/definitions.js";

// Scheduled-job sync takes one `clientId` per HTTP call (not per job), so jobs
// with a single shared clientId — the common case — must post it at the top
// level of the body, and it must NOT ride along inside each job object (the
// platform API rejects unknown per-job fields).
test("syncScheduledJobs posts clientId at the top level, not inside job objects", async () => {
	const requests: Array<{ url?: string; body: unknown }> = [];
	const server: Server = createServer((req, res) => {
		let raw = "";
		req.on("data", (c) => (raw += c));
		req.on("end", () => {
			requests.push({ url: req.url, body: JSON.parse(raw) });
			res.writeHead(200, { "content-type": "application/json" });
			res.end(
				JSON.stringify({
					applicationCode: "orders",
					created: ["sj_1"],
					updated: [],
					archived: [],
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
		const set = defineApplication("orders")
			.withScheduledJobs([
				{
					code: "nightly-report",
					name: "Nightly Report",
					crons: ["0 0 2 * * *"],
					clientId: "clt_abc",
				},
			])
			.build();

		const result = await client.definitions().sync(set);

		assert.ok(result.isOk(), `expected ok result, got ${JSON.stringify(result)}`);
		assert.equal(requests.length, 1);
		assert.equal(requests[0].url, "/api/applications/orders/scheduled-jobs/sync");
		const body = requests[0].body as Record<string, unknown>;
		assert.equal(body.clientId, "clt_abc");
		const jobs = body.jobs as Array<Record<string, unknown>>;
		assert.equal(jobs.length, 1);
		assert.ok(
			!("clientId" in jobs[0]),
			`clientId must not appear inside a job object, got: ${JSON.stringify(jobs[0])}`,
		);
		assert.equal(jobs[0].code, "nightly-report");

		const inner = result.isOk() ? result.value.scheduledJobs : undefined;
		assert.ok(inner && "created" in inner);
		if (inner && "created" in inner) {
			assert.equal(inner.created, 1);
		}
	} finally {
		await new Promise<void>((r) => server.close(() => r()));
	}
});

// Jobs with different clientIds in one withScheduledJobs() call must be
// partitioned into separate sync requests — the platform API only accepts one
// clientId per call — and the per-request results summed into one total.
test("syncScheduledJobs issues one request per distinct clientId and sums results", async () => {
	const requests: Array<{ body: { clientId?: string; jobs: Array<{ code: string }> } }> = [];
	const server: Server = createServer((req, res) => {
		let raw = "";
		req.on("data", (c) => (raw += c));
		req.on("end", () => {
			requests.push({ body: JSON.parse(raw) });
			res.writeHead(200, { "content-type": "application/json" });
			res.end(
				JSON.stringify({
					applicationCode: "orders",
					created: ["sj_1"],
					updated: [],
					archived: [],
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
		const set = defineApplication("orders")
			.withScheduledJobs([
				{ code: "job-a", name: "Job A", crons: ["0 0 2 * * *"], clientId: "clt_one" },
				{ code: "job-b", name: "Job B", crons: ["0 0 3 * * *"], clientId: "clt_two" },
			])
			.build();

		const result = await client.definitions().sync(set);

		assert.ok(result.isOk(), `expected ok result, got ${JSON.stringify(result)}`);
		assert.equal(requests.length, 2);
		const clientIds = requests.map((r) => r.body.clientId).sort();
		assert.deepEqual(clientIds, ["clt_one", "clt_two"]);
		for (const r of requests) {
			assert.equal(r.body.jobs.length, 1);
		}

		const inner = result.isOk() ? result.value.scheduledJobs : undefined;
		assert.ok(inner && "created" in inner);
		if (inner && "created" in inner) {
			assert.equal(inner.created, 2);
		}
	} finally {
		await new Promise<void>((r) => server.close(() => r()));
	}
});
