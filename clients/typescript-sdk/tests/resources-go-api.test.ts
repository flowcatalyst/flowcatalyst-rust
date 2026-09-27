import { test } from "node:test";
import assert from "node:assert/strict";
import { mockFetch } from "./helpers/mock-fetch.js";
import { sync } from "../src/index.js";

const calls = (m: { requests: { method: string; path: string }[] }) =>
	m.requests.map((r) => `${r.method} ${r.path}`);

// ── event types ──────────────────────────────────────────────────────────

test("eventTypes.update always sends name (reads it when the caller omits it) and resolves void", async () => {
	const m = mockFetch({
		"GET /api/event-types/et_1": {
			body: { id: "et_1", code: "orders:o:order:created", name: "Order Created" },
		},
		"PUT /api/event-types/et_1": { status: 204 },
	});
	try {
		const ets = m.client.eventTypes();
		const withName = await ets.update("et_1", { name: "Renamed", description: "d" });
		assert.equal(withName._unsafeUnwrap(), undefined);
		assert.deepEqual(m.requests[0]?.body, { name: "Renamed", description: "d" });

		// An untyped caller that leaves out `name`.
		const withoutName = await ets.update("et_1", {
			description: "only",
		} as unknown as Parameters<typeof ets.update>[1]);
		assert.ok(withoutName.isOk(), JSON.stringify(withoutName));
		assert.deepEqual(calls(m), [
			"PUT /api/event-types/et_1",
			"GET /api/event-types/et_1",
			"PUT /api/event-types/et_1",
		]);
		assert.deepEqual(m.requests[2]?.body, { description: "only", name: "Order Created" });
	} finally {
		m.restore();
	}
});

test("eventTypes.create answers { id }; delete and the deprecated archive send DELETE", async () => {
	const m = mockFetch({
		"POST /api/event-types": { status: 201, body: { id: "et_2" } },
		"DELETE /api/event-types/et_2": { status: 204 },
		"GET /api/event-types/by-code/orders:o:order:created": {
			body: { id: "et_2", code: "orders:o:order:created", eventName: "created", name: "Order Created" },
		},
	});
	try {
		const ets = m.client.eventTypes();
		const created = await ets.create({ code: "orders:o:order:created", name: "Order Created" });
		assert.deepEqual(created._unsafeUnwrap(), { id: "et_2" });
		assert.equal((await ets.delete("et_2"))._unsafeUnwrap(), undefined);
		assert.ok((await ets.archive("et_2")).isOk());
		const byCode = await ets.getByCode("orders:o:order:created");
		assert.equal(byCode._unsafeUnwrap().eventName, "created");
		assert.deepEqual(calls(m), [
			"POST /api/event-types",
			"DELETE /api/event-types/et_2",
			"DELETE /api/event-types/et_2",
			"GET /api/event-types/by-code/orders:o:order:created",
		]);
	} finally {
		m.restore();
	}
});

test("eventTypes.sync sends only code / name / description (Go's strict item)", async () => {
	const m = mockFetch({
		"POST /api/applications/orders/event-types/sync": {
			body: { applicationCode: "orders", created: 1, updated: 0, deleted: 0, syncedCodes: ["orders:o:order:created"] },
		},
	});
	try {
		const res = await m.client.eventTypes().sync(
			"orders",
			[
				{
					code: "orders:o:order:created",
					name: "Order Created",
					description: "d",
					schema: { type: "object" },
					clientId: "clt_1",
				} as unknown as { code: string; name: string },
			],
			true,
		);
		assert.ok(res.isOk(), JSON.stringify(res));
		assert.deepEqual(m.requests[0]?.body, {
			eventTypes: [{ code: "orders:o:order:created", name: "Order Created", description: "d" }],
		});
		assert.equal(m.requests[0]?.query.get("removeUnlisted"), "true");
	} finally {
		m.restore();
	}
});

test("definitions().sync sends event types reduced to Go's strict item", async () => {
	const m = mockFetch({
		"POST /api/applications/orders/event-types/sync": {
			body: { applicationCode: "orders", created: 1, updated: 0, deleted: 0, syncedCodes: [] },
		},
	});
	try {
		const set = sync
			.defineApplication("orders")
			.withEventTypes([
				{
					code: "orders:o:order:created",
					name: "Order Created",
					schema: { type: "object" },
				} as unknown as { code: string; name: string },
			])
			.build();
		const res = await m.client.definitions().sync(set);
		assert.ok(res.isOk(), JSON.stringify(res));
		const req = m.requests.find((r) => r.path.endsWith("/event-types/sync"));
		assert.deepEqual(req?.body, {
			eventTypes: [{ code: "orders:o:order:created", name: "Order Created" }],
		});
	} finally {
		m.restore();
	}
});

// ── processes ────────────────────────────────────────────────────────────

test("processes: create answers { id }, update resolves void, archive is POST …/archive", async () => {
	const m = mockFetch({
		"POST /api/processes": { status: 201, body: { id: "prc_1" } },
		"PUT /api/processes/prc_1": { status: 204 },
		"POST /api/processes/prc_1/archive": { status: 204 },
	});
	try {
		const p = m.client.processes();
		const created = await p.create({
			code: "orders:fulfilment:ship",
			name: "Ship",
			body: "graph TD; A-->B",
			diagramType: "mermaid",
			tags: ["ops"],
		});
		assert.deepEqual(created._unsafeUnwrap(), { id: "prc_1" });
		assert.deepEqual(m.requests[0]?.body, {
			code: "orders:fulfilment:ship",
			name: "Ship",
			body: "graph TD; A-->B",
			diagramType: "mermaid",
			tags: ["ops"],
		});
		assert.equal((await p.update("prc_1", { name: "Ship 2" }))._unsafeUnwrap(), undefined);
		assert.equal((await p.archive("prc_1"))._unsafeUnwrap(), undefined);
		assert.deepEqual(calls(m), [
			"POST /api/processes",
			"PUT /api/processes/prc_1",
			"POST /api/processes/prc_1/archive",
		]);
	} finally {
		m.restore();
	}
});

// ── connections ──────────────────────────────────────────────────────────

test("connections.update always sends name (reads it when omitted) and resolves void", async () => {
	const m = mockFetch({
		"GET /api/connections/con_1": {
			body: {
				id: "con_1",
				code: "erp",
				name: "ERP",
				status: "ACTIVE",
				serviceAccountId: "sa_1",
				source: "API",
				createdAt: "2026-01-01T00:00:00Z",
				updatedAt: "2026-01-01T00:00:00Z",
			},
		},
		"PUT /api/connections/con_1": { status: 204 },
	});
	try {
		const res = await m.client.connections().update("con_1", { status: "PAUSED" });
		assert.ok(res.isOk(), JSON.stringify(res));
		assert.equal(res._unsafeUnwrap(), undefined);
		assert.deepEqual(calls(m), ["GET /api/connections/con_1", "PUT /api/connections/con_1"]);
		assert.deepEqual(m.requests[1]?.body, { status: "PAUSED", name: "ERP" });

		const named = await m.client.connections().update("con_1", { name: "ERP 2" });
		assert.ok(named.isOk());
		assert.equal(m.requests.length, 3, "no GET when name is given");
		assert.deepEqual(m.requests[2]?.body, { name: "ERP 2" });
	} finally {
		m.restore();
	}
});

test("connections.list decodes Go's { connections, total } with source", async () => {
	const m = mockFetch({
		"GET /api/connections": {
			body: {
				connections: [
					{ id: "con_1", code: "erp", name: "ERP", status: "ACTIVE", serviceAccountId: "sa_1", source: "CODE", applicationCode: "orders", createdAt: "x", updatedAt: "x" },
				],
				total: 1,
			},
		},
	});
	try {
		const res = await m.client.connections().list({ clientId: "clt_1" });
		assert.equal(res._unsafeUnwrap().connections[0]?.source, "CODE");
		assert.equal(m.requests[0]?.query.get("clientId"), "clt_1");
	} finally {
		m.restore();
	}
});

// ── dispatch pools ───────────────────────────────────────────────────────

test("dispatchPools.list reads pools and Go's concurrency", async () => {
	const m = mockFetch({
		"GET /api/dispatch-pools": {
			body: {
				pools: [
					{ id: "dp_1", code: "default", name: "Default", status: "ACTIVE", concurrency: 8, rateLimit: 600, createdAt: "x", updatedAt: "x" },
				],
				total: 1,
			},
		},
	});
	try {
		const res = await m.client.dispatchPools().list({ status: "ACTIVE" });
		assert.ok(res.isOk(), JSON.stringify(res));
		const pool = res._unsafeUnwrap().pools[0];
		assert.equal(pool?.concurrency, 8);
		assert.equal(pool?.maxConcurrency, 8, "deprecated alias");
		assert.equal(pool?.rateLimit, 600);
	} finally {
		m.restore();
	}
});

test("dispatchPools: create sends concurrency and answers { id }; archive is POST …/archive; 204s resolve void", async () => {
	const m = mockFetch({
		"POST /api/dispatch-pools": { status: 201, body: { id: "dp_2" } },
		"PUT /api/dispatch-pools/dp_2": { status: 204 },
		"POST /api/dispatch-pools/dp_2/archive": { status: 204 },
		"POST /api/dispatch-pools/dp_2/suspend": { status: 204 },
		"POST /api/dispatch-pools/dp_2/activate": { status: 204 },
	});
	try {
		const pools = m.client.dispatchPools();
		const created = await pools.create({
			code: "bulk",
			name: "Bulk",
			maxConcurrency: 4,
			rateLimitWindow: 60,
			applicationCode: "orders",
		});
		assert.deepEqual(created._unsafeUnwrap(), { id: "dp_2" });
		assert.deepEqual(m.requests[0]?.body, { code: "bulk", name: "Bulk", concurrency: 4 });

		assert.equal((await pools.update("dp_2", { concurrency: 2 }))._unsafeUnwrap(), undefined);
		assert.deepEqual(m.requests[1]?.body, { concurrency: 2 });
		assert.equal((await pools.archive("dp_2"))._unsafeUnwrap(), undefined);
		assert.equal((await pools.suspend("dp_2"))._unsafeUnwrap(), undefined);
		assert.equal((await pools.activate("dp_2"))._unsafeUnwrap(), undefined);
		assert.deepEqual(calls(m), [
			"POST /api/dispatch-pools",
			"PUT /api/dispatch-pools/dp_2",
			"POST /api/dispatch-pools/dp_2/archive",
			"POST /api/dispatch-pools/dp_2/suspend",
			"POST /api/dispatch-pools/dp_2/activate",
		]);
	} finally {
		m.restore();
	}
});

test("dispatchPools.sync sends Go's strict item and reads deleted", async () => {
	const m = mockFetch({
		"POST /api/applications/orders/dispatch-pools/sync": {
			body: { applicationCode: "orders", created: 1, updated: 0, deleted: 2, syncedCodes: ["bulk"] },
		},
	});
	try {
		const res = await m.client
			.dispatchPools()
			.sync("orders", [{ code: "bulk", name: "Bulk", description: null, concurrency: 4, rateLimit: null }]);
		assert.ok(res.isOk(), JSON.stringify(res));
		assert.deepEqual(m.requests[0]?.body, {
			pools: [{ code: "bulk", name: "Bulk", concurrency: 4 }],
		});
		assert.equal(res._unsafeUnwrap().deleted, 2);
		assert.equal(res._unsafeUnwrap().removed, 2, "deprecated alias");
	} finally {
		m.restore();
	}
});

// ── subscriptions / roles / clients / principals ─────────────────────────

test("subscriptions: create answers { id }; update / pause / resume resolve void", async () => {
	const m = mockFetch({
		"POST /api/subscriptions": { status: 201, body: { id: "sub_1" } },
		"PUT /api/subscriptions/sub_1": { status: 204 },
		"POST /api/subscriptions/sub_1/pause": { status: 204 },
		"POST /api/subscriptions/sub_1/resume": { status: 204 },
	});
	try {
		const subs = m.client.subscriptions();
		const created = await subs.create({ code: "s", name: "S", endpoint: "https://x.example/hook" });
		assert.deepEqual(created._unsafeUnwrap(), { id: "sub_1" });
		assert.equal((await subs.update("sub_1", { name: "S2" }))._unsafeUnwrap(), undefined);
		assert.equal((await subs.pause("sub_1"))._unsafeUnwrap(), undefined);
		assert.equal((await subs.resume("sub_1"))._unsafeUnwrap(), undefined);
	} finally {
		m.restore();
	}
});

test("roles: create answers { id }; update resolves void", async () => {
	const m = mockFetch({
		"POST /api/roles": { status: 201, body: { id: "rol_1" } },
		"PUT /api/roles/orders:admin": { status: 204 },
	});
	try {
		const roles = m.client.roles();
		const created = await roles.create({
			applicationCode: "orders",
			roleName: "admin",
			displayName: "Admin",
			clientManaged: false,
		});
		assert.deepEqual(created._unsafeUnwrap(), { id: "rol_1" });
		assert.equal((await roles.update("orders:admin", { displayName: "A" }))._unsafeUnwrap(), undefined);
	} finally {
		m.restore();
	}
});

test("clients: create { id }; update / updateApplications / enable / disable void; status changes { message }", async () => {
	const m = mockFetch({
		"POST /api/clients": { status: 201, body: { id: "clt_1" } },
		"PUT /api/clients/clt_1": { status: 204 },
		"PUT /api/clients/clt_1/applications": { status: 204 },
		"POST /api/clients/clt_1/applications/app_1/enable": { status: 204 },
		"POST /api/clients/clt_1/applications/app_1/disable": { status: 204 },
		"POST /api/clients/clt_1/activate": { body: { message: "Client activated" } },
		"POST /api/clients/clt_1/deactivate": { body: { message: "Client deactivated" } },
		"POST /api/clients/clt_1/suspend": { body: { message: "Client suspended" } },
	});
	try {
		const c = m.client.clients();
		assert.deepEqual((await c.create({ name: "Acme", identifier: "acme" }))._unsafeUnwrap(), { id: "clt_1" });
		assert.equal((await c.update("clt_1", { name: "Acme 2" }))._unsafeUnwrap(), undefined);
		assert.equal(
			(await c.updateApplications("clt_1", { enabledApplicationIds: ["app_1"] }))._unsafeUnwrap(),
			undefined,
		);
		assert.equal((await c.enableApplication("clt_1", "app_1"))._unsafeUnwrap(), undefined);
		assert.equal((await c.disableApplication("clt_1", "app_1"))._unsafeUnwrap(), undefined);
		assert.equal((await c.activate("clt_1"))._unsafeUnwrap().message, "Client activated");
		assert.equal((await c.deactivate("clt_1", "why"))._unsafeUnwrap().message, "Client deactivated");
		assert.deepEqual(m.requests[6]?.body, { reason: "why" });
		assert.equal((await c.suspend("clt_1", "why"))._unsafeUnwrap().message, "Client suspended");
	} finally {
		m.restore();
	}
});

test("principals: activate / deactivate answer { message }; findByEmail sends q", async () => {
	const m = mockFetch({
		"POST /api/principals/prn_1/activate": { body: { message: "Principal activated" } },
		"POST /api/principals/prn_1/deactivate": { body: { message: "Principal deactivated" } },
		"GET /api/principals": {
			body: {
				principals: [
					{ id: "prn_1", email: "a@example.com" },
					{ id: "prn_2", email: "b@example.com" },
				],
				total: 2,
			},
		},
	});
	try {
		const p = m.client.principals();
		assert.equal((await p.activate("prn_1"))._unsafeUnwrap().message, "Principal activated");
		assert.equal((await p.deactivate("prn_1"))._unsafeUnwrap().message, "Principal deactivated");
		const found = await p.findByEmail("A@example.com");
		assert.equal(m.requests[2]?.query.get("q"), "A@example.com");
		assert.equal(m.requests[2]?.query.get("email"), null);
		assert.deepEqual(found._unsafeUnwrap().principals.map((x) => x.id), ["prn_1"]);
	} finally {
		m.restore();
	}
});

// ── audit logs ───────────────────────────────────────────────────────────

test("auditLogs.list / recent use Go's cursor paging and clientIds", async () => {
	const page = {
		auditLogs: [{ id: "aud_1", entityType: "Client", entityId: "clt_1", operation: "ClientCreated", performedAt: "x" }],
		hasMore: true,
		nextCursor: "c2",
	};
	const m = mockFetch({
		"GET /api/audit-logs": { body: page },
		"GET /api/audit-logs/recent": { body: { ...page, hasMore: false, nextCursor: undefined } },
	});
	try {
		const logs = m.client.auditLogs();
		const res = await logs.list({ after: "c1", pageSize: 20, clientIds: "clt_1,clt_2" });
		assert.equal(res._unsafeUnwrap().nextCursor, "c2");
		assert.equal(res._unsafeUnwrap().hasMore, true);
		assert.equal(m.requests[0]?.query.get("after"), "c1");
		assert.equal(m.requests[0]?.query.get("pageSize"), "20");
		assert.equal(m.requests[0]?.query.get("clientIds"), "clt_1,clt_2");

		const recent = await logs.recent({ pageSize: 5 });
		assert.ok(recent.isOk(), JSON.stringify(recent));
		assert.equal(m.requests[1]?.query.get("pageSize"), "5");
		assert.ok((await logs.recent()).isOk(), "recent() still takes no arguments");
	} finally {
		m.restore();
	}
});

// ── scheduled jobs ───────────────────────────────────────────────────────

test("scheduledJobs lists read Go's total_pages", async () => {
	const m = mockFetch({
		"GET /api/scheduled-jobs": {
			body: { data: [], page: 0, size: 20, total: 41, total_pages: 3 },
		},
		"GET /api/scheduled-jobs/sj_1/instances": {
			body: { data: [], page: 1, size: 10, total: 11, total_pages: 2 },
		},
	});
	try {
		const jobs = await m.client.scheduledJobs().list({ status: "ACTIVE", page: 0, size: 20 });
		assert.equal(jobs._unsafeUnwrap().totalPages, 3);
		assert.equal(jobs._unsafeUnwrap().total_pages, 3);
		assert.equal(m.requests[0]?.query.get("status"), "ACTIVE");
		const instances = await m.client.scheduledJobs().listInstances("sj_1", { page: 1, size: 10 });
		assert.equal(instances._unsafeUnwrap().totalPages, 2);
	} finally {
		m.restore();
	}
});

test("scheduledJobs.create always sends concurrent / tracksCompletion; log defaults level to INFO", async () => {
	const m = mockFetch({
		"POST /api/scheduled-jobs": { status: 201, body: { id: "sj_1" } },
		"POST /api/scheduled-jobs/instances/sji_1/log": { status: 204 },
		"POST /api/scheduled-jobs/sj_1/fire": {
			status: 202,
			body: { id: "sji_2", instanceId: "sji_2", scheduledJobId: "sj_1" },
		},
	});
	try {
		const sj = m.client.scheduledJobs();
		const created = await sj.create({ code: "nightly", name: "Nightly", crons: ["0 0 2 * * *"] });
		assert.deepEqual(created._unsafeUnwrap(), { id: "sj_1" });
		assert.deepEqual(m.requests[0]?.body, {
			code: "nightly",
			name: "Nightly",
			crons: ["0 0 2 * * *"],
			concurrent: false,
			tracksCompletion: false,
		});
		const kept = await sj.create({
			code: "n2",
			name: "N2",
			crons: ["0 0 2 * * *"],
			concurrent: true,
			tracksCompletion: true,
		});
		assert.ok(kept.isOk());
		assert.equal((m.requests[1]?.body as { concurrent: boolean }).concurrent, true);

		assert.ok((await sj.logForInstance("sji_1", { message: "hi" })).isOk());
		assert.deepEqual(m.requests[2]?.body, { message: "hi", level: "INFO" });

		const fired = await sj.fire("sj_1");
		assert.equal(fired._unsafeUnwrap().instanceId, "sji_2");
	} finally {
		m.restore();
	}
});

// ── router ───────────────────────────────────────────────────────────────

test("router.inPipeline reads Go's top-level poolCode / queueId", async () => {
	const m = mockFetch({
		"GET /monitoring/in-flight-messages/check": {
			body: { messageId: "m1", inPipeline: true, poolCode: "default", queueId: "q1" },
		},
	});
	try {
		const res = await m.client.router().inPipeline("m1");
		assert.ok(res.isOk(), JSON.stringify(res));
		const r = res._unsafeUnwrap();
		assert.equal(r.inPipeline, true);
		assert.equal(r.poolCode, "default");
		assert.equal(r.queueId, "q1");
		assert.equal(r.detail, undefined);
		assert.equal(m.requests[0]?.query.get("messageId"), "m1");
		assert.equal(m.requests[0]?.authorization, "Bearer test-token");
	} finally {
		m.restore();
	}
});

test("router.inPipeline lifts poolCode / queueId from a legacy detail object", async () => {
	const m = mockFetch({
		"GET /monitoring/in-flight-messages/check": {
			body: {
				messageId: "m1",
				inPipeline: true,
				detail: { messageId: "m1", brokerMessageId: null, queueId: "q9", poolCode: "p9", elapsedTimeMs: 5, addedToInPipelineAt: "x" },
			},
		},
	});
	try {
		const r = (await m.client.router().inPipeline("m1"))._unsafeUnwrap();
		assert.equal(r.poolCode, "p9");
		assert.equal(r.queueId, "q9");
	} finally {
		m.restore();
	}
});
