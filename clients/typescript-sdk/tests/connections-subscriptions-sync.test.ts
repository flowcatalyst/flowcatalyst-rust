import { test } from "node:test";
import assert from "node:assert/strict";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import type { Result } from "neverthrow";
import { FlowCatalystClient, type SdkError } from "../src/index.js";
import { defineApplication } from "../src/sync/definitions.js";
import type { DefinitionSyncError } from "../src/sync/definition-synchronizer.js";

/**
 * Assert that `result` failed with the embedded-category-failure error (as
 * opposed to a harder `SdkError` such as an auth/network failure), and
 * return its carried partial result — the whole point of that error shape
 * being a caller can still see what DID sync.
 */
function expectPartialFailure<T>(
	result: Result<T, SdkError | DefinitionSyncError<T>>,
): T {
	assert.ok(result.isErr(), "expected a failed result");
	if (!result.isErr()) throw new Error("unreachable");
	assert.equal(
		result.error.type,
		"partial_failure",
		`expected a partial_failure error, got: ${JSON.stringify(result.error)}`,
	);
	if (result.error.type !== "partial_failure") throw new Error("unreachable");
	return result.error.partial;
}

// ── test harness ──────────────────────────────────────────────────────────
//
// A tiny HTTP server that records every request (in arrival order) and lets
// each test decide how to respond, based on the URL/body it just saw. Mirrors
// the pattern in scheduled-jobs-sync.test.ts and principals-sync-users.test.ts.

interface RecordedRequest {
	url: string;
	method: string;
	body: Record<string, unknown>;
}

type Responder = (req: RecordedRequest) => { status: number; json: unknown };

/**
 * Default responder for the connections/subscriptions/roles/event-types/
 * dispatch-pools/processes sync endpoints — they all share the
 * `{created, updated, deleted, syncedCodes}` response shape. Counts the
 * posted list's length as `created` and echoes its codes/names, which is all
 * these tests need.
 */
function okSyncResponse(body: Record<string, unknown>): unknown {
	const listKey = [
		"connections",
		"subscriptions",
		"roles",
		"eventTypes",
		"pools",
		"processes",
	].find((k) => Array.isArray(body[k]));
	const list = listKey
		? (body[listKey] as Array<{ code?: string; name?: string }>)
		: [];
	return {
		applicationCode: "app",
		created: list.length,
		updated: 0,
		deleted: 0,
		syncedCodes: list.map((x) => x.code ?? x.name ?? ""),
	};
}

async function withServer<T>(
	responder: Responder,
	fn: (client: FlowCatalystClient, requests: RecordedRequest[]) => Promise<T>,
): Promise<T> {
	const requests: RecordedRequest[] = [];
	const server: Server = createServer((req, res) => {
		let raw = "";
		req.on("data", (c) => (raw += c));
		req.on("end", () => {
			const body = raw ? (JSON.parse(raw) as Record<string, unknown>) : {};
			const recorded: RecordedRequest = {
				url: req.url ?? "",
				method: req.method ?? "",
				body,
			};
			requests.push(recorded);
			const { status, json } = responder(recorded);
			res.writeHead(status, { "content-type": "application/json" });
			res.end(JSON.stringify(json));
		});
	});
	await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
	const port = (server.address() as AddressInfo).port;
	try {
		const client = new FlowCatalystClient({
			baseUrl: `http://127.0.0.1:${port}`,
			accessToken: "test-token",
		});
		return await fn(client, requests);
	} finally {
		await new Promise<void>((resolve) => server.close(() => resolve()));
	}
}

function conn(overrides: { code: string; client?: string }) {
	return { code: overrides.code, name: overrides.code, client: overrides.client };
}

function sub(overrides: {
	code: string;
	target?: string;
	connectionCode?: string;
	sharedConnection?: boolean;
	client?: string;
}) {
	return {
		code: overrides.code,
		name: overrides.code,
		target: overrides.target ?? "https://example.test/webhook",
		connectionCode: overrides.connectionCode,
		sharedConnection: overrides.sharedConnection,
		client: overrides.client,
		eventTypes: [],
	};
}

// ── ordering: connections before subscriptions ──────────────────────────

test("connections are synced before subscriptions", async () => {
	await withServer(
		(r) => ({ status: 200, json: okSyncResponse(r.body) }),
		async (client, requests) => {
			const set = defineApplication("orders")
				.withConnections([conn({ code: "conn-a" })])
				.withSubscriptions([sub({ code: "sub-a", connectionCode: "conn-a" })])
				.build();

			const result = await client.definitions().sync(set);
			assert.ok(result.isOk(), JSON.stringify(result));
			assert.equal(requests.length, 2);
			assert.match(requests[0]?.url ?? "", /\/connections\/sync/);
			assert.match(requests[1]?.url ?? "", /\/subscriptions\/sync/);
		},
	);
});

// ── grouping by client ────────────────────────────────────────────────────

test("groups connections and subscriptions by client: one call per scope, global first, never merged", async () => {
	await withServer(
		(r) => ({ status: 200, json: okSyncResponse(r.body) }),
		async (client, requests) => {
			const set = defineApplication("orders")
				.withConnections([
					conn({ code: "conn-global" }),
					conn({ code: "conn-a", client: "client-a" }),
					conn({ code: "conn-b", client: "client-b" }),
				])
				.withSubscriptions([
					sub({ code: "sub-global", connectionCode: "conn-global" }),
					sub({ code: "sub-a", connectionCode: "conn-a", client: "client-a" }),
					sub({ code: "sub-b", connectionCode: "conn-b", client: "client-b" }),
				])
				.build();

			const result = await client.definitions().sync(set);
			assert.ok(result.isOk(), JSON.stringify(result));
			assert.equal(requests.length, 6, "3 connection calls + 3 subscription calls");

			const urls = requests.map((r) => r.url.replace(/\?.*/, ""));
			assert.deepEqual(urls, [
				"/api/applications/orders/connections/sync",
				"/api/applications/orders/subscriptions/sync",
				"/api/applications/orders/connections/sync",
				"/api/applications/orders/subscriptions/sync",
				"/api/applications/orders/connections/sync",
				"/api/applications/orders/subscriptions/sync",
			]);

			// Global scope first, and omits the clientId key entirely.
			assert.ok(!("clientId" in (requests[0]?.body ?? {})));
			assert.ok(!("clientId" in (requests[1]?.body ?? {})));

			assert.equal(requests[2]?.body["clientId"], "client-a");
			assert.equal(requests[3]?.body["clientId"], "client-a");
			assert.equal(requests[4]?.body["clientId"], "client-b");
			assert.equal(requests[5]?.body["clientId"], "client-b");

			// Never merged: each scope's connections list contains only its own row.
			const globalConns = requests[0]?.body["connections"] as Array<{ code: string }>;
			assert.deepEqual(globalConns.map((c) => c.code), ["conn-global"]);
			const clientAConns = requests[2]?.body["connections"] as Array<{ code: string }>;
			assert.deepEqual(clientAConns.map((c) => c.code), ["conn-a"]);
		},
	);
});

// ── wire hygiene ──────────────────────────────────────────────────────────

test("client never appears inside a posted entry; sharedConnection only when true", async () => {
	await withServer(
		(r) => ({ status: 200, json: okSyncResponse(r.body) }),
		async (client, requests) => {
			const set = defineApplication("orders")
				.withConnections([conn({ code: "conn-a", client: "client-a" })])
				.withSubscriptions([
					sub({
						code: "sub-shared",
						connectionCode: "shared-conn",
						sharedConnection: true,
						client: "client-a",
					}),
					sub({ code: "sub-own", connectionCode: "conn-a", client: "client-a" }),
				])
				.build();

			const result = await client.definitions().sync(set);
			assert.ok(result.isOk(), JSON.stringify(result));

			const connectionCall = requests.find((r) => r.url.includes("/connections/sync"));
			const connections = connectionCall?.body["connections"] as Array<Record<string, unknown>>;
			assert.ok(!("client" in (connections?.[0] ?? {})), "client must not ride inside a connection entry");

			const subscriptionCall = requests.find((r) => r.url.includes("/subscriptions/sync"));
			const subscriptions = subscriptionCall?.body["subscriptions"] as Array<Record<string, unknown>>;
			for (const entry of subscriptions ?? []) {
				assert.ok(!("client" in entry), "client must not ride inside a subscription entry");
				assert.ok(!("_targetBaseUrl" in entry), "_targetBaseUrl must never reach the wire");
			}
			const sharedEntry = subscriptions.find((e) => e["code"] === "sub-shared");
			const ownEntry = subscriptions.find((e) => e["code"] === "sub-own");
			assert.equal(sharedEntry?.["sharedConnection"], true);
			assert.ok(!("sharedConnection" in (ownEntry ?? {})), "sharedConnection omitted when false");
		},
	);
});

// ── THE critical behaviour: syncGrouped merges same-scope sets into one call ──

test("syncGrouped merges two sets for the same application and scope into ONE call each (global, then client)", async () => {
	await withServer(
		(r) => ({ status: 200, json: okSyncResponse(r.body) }),
		async (client, requests) => {
			// Two global sets for the same application, each with one connection
			// and one role — must land in exactly one connections call, one
			// subscriptions... (no subscriptions here) and one roles call,
			// containing both.
			const setOne = defineApplication("orders")
				.withConnections([conn({ code: "conn-1" })])
				.withRoles([{ name: "role-one" }]);
			const setTwo = defineApplication("orders")
				.withConnections([conn({ code: "conn-2" })])
				.withRoles([{ name: "role-two" }]);

			const results = await client
				.definitions()
				.syncGrouped([setOne.build(), setTwo.build()], { removeUnlisted: true });
			assert.ok(results.isOk(), JSON.stringify(results));

			const connectionCalls = requests.filter((r) => r.url.includes("/connections/sync"));
			const roleCalls = requests.filter((r) => r.url.includes("/roles/sync"));
			assert.equal(connectionCalls.length, 1, "exactly one connections call for the merged scope");
			assert.equal(roleCalls.length, 1, "exactly one roles call for the merged scope");

			const mergedConnections = connectionCalls[0]?.body["connections"] as Array<{ code: string }>;
			assert.deepEqual(
				mergedConnections.map((c) => c.code).sort(),
				["conn-1", "conn-2"],
			);
			const mergedRoles = roleCalls[0]?.body["roles"] as Array<{ name: string }>;
			assert.deepEqual(
				mergedRoles.map((r) => r.name).sort(),
				["role-one", "role-two"],
			);
		},
	);
});

test("syncGrouped merges two sets for the same application AND client into ONE call", async () => {
	await withServer(
		(r) => ({ status: 200, json: okSyncResponse(r.body) }),
		async (client, requests) => {
			const setOne = defineApplication("orders")
				.forClient("acme")
				.withConnections([conn({ code: "conn-1" })]);
			const setTwo = defineApplication("orders")
				.forClient("acme")
				.withConnections([conn({ code: "conn-2" })]);

			const results = await client
				.definitions()
				.syncGrouped([setOne.build(), setTwo.build()], { removeUnlisted: true });
			assert.ok(results.isOk(), JSON.stringify(results));

			const connectionCalls = requests.filter((r) => r.url.includes("/connections/sync"));
			assert.equal(connectionCalls.length, 1, "exactly one connections call for the merged client scope");
			assert.equal(connectionCalls[0]?.body["clientId"], "acme");
			const mergedConnections = connectionCalls[0]?.body["connections"] as Array<{ code: string }>;
			assert.deepEqual(
				mergedConnections.map((c) => c.code).sort(),
				["conn-1", "conn-2"],
			);
		},
	);
});

// ── mixed ordering across merged scopes ──────────────────────────────────

test("syncGrouped orders global, then each client (merged), in first-appearance order", async () => {
	await withServer(
		(r) => ({ status: 200, json: okSyncResponse(r.body) }),
		async (client, requests) => {
			const global = defineApplication("orders").withConnections([conn({ code: "conn-global" })]);
			const clientAOne = defineApplication("orders")
				.forClient("client-a")
				.withConnections([conn({ code: "conn-a1" })]);
			const clientATwo = defineApplication("orders")
				.forClient("client-a")
				.withConnections([conn({ code: "conn-a2" })]);
			const clientB = defineApplication("orders")
				.forClient("client-b")
				.withConnections([conn({ code: "conn-b" })]);

			const results = await client
				.definitions()
				.syncGrouped(
					[global.build(), clientAOne.build(), clientATwo.build(), clientB.build()],
					{ removeUnlisted: true },
				);
			assert.ok(results.isOk(), JSON.stringify(results));

			const connectionCalls = requests.filter((r) => r.url.includes("/connections/sync"));
			assert.equal(connectionCalls.length, 3, "global + client-a (merged) + client-b");
			assert.ok(!("clientId" in (connectionCalls[0]?.body ?? {})));
			assert.equal(connectionCalls[1]?.body["clientId"], "client-a");
			assert.equal(connectionCalls[2]?.body["clientId"], "client-b");

			const clientAConns = connectionCalls[1]?.body["connections"] as Array<{ code: string }>;
			assert.deepEqual(
				clientAConns.map((c) => c.code).sort(),
				["conn-a1", "conn-a2"],
			);
		},
	);
});

// ── target resolution ─────────────────────────────────────────────────────

test("a per-set targetBaseUrl wins per row after merging; synchronizer default used otherwise; absolute target untouched", async () => {
	await withServer(
		(r) => ({ status: 200, json: okSyncResponse(r.body) }),
		async (client, requests) => {
			const setWithBase = defineApplication("orders")
				.forClient("acme", "https://acme.example.com")
				.withConnections([conn({ code: "acme-conn", client: "acme" })])
				.withSubscriptions([
					sub({ code: "acme-path", target: "/webhooks/orders", connectionCode: "acme-conn", client: "acme" }),
				]);
			const setWithoutBase = defineApplication("orders")
				.forClient("acme")
				.withSubscriptions([
					sub({
						code: "acme-absolute",
						target: "https://explicit.example.com/hook",
						connectionCode: "acme-conn",
						client: "acme",
					}),
				]);

			const results = await client
				.definitions({ subscriptionTargetBaseUrl: "https://synchronizer-default.example.com" })
				.syncGrouped([setWithBase.build(), setWithoutBase.build()]);
			assert.ok(results.isOk(), JSON.stringify(results));

			const subscriptionCall = requests.find((r) => r.url.includes("/subscriptions/sync"));
			const entries = subscriptionCall?.body["subscriptions"] as Array<{ code: string; target: string }>;
			const pathEntry = entries.find((e) => e.code === "acme-path");
			const absoluteEntry = entries.find((e) => e.code === "acme-absolute");
			assert.equal(pathEntry?.target, "https://acme.example.com/webhooks/orders");
			assert.equal(absoluteEntry?.target, "https://explicit.example.com/hook");
		},
	);
});

test("synchronizer-level subscriptionTargetBaseUrl resolves a path when no set-level base is configured", async () => {
	await withServer(
		(r) => ({ status: 200, json: okSyncResponse(r.body) }),
		async (client, requests) => {
			const set = defineApplication("orders").withSubscriptions([
				sub({ code: "orders-path", target: "/webhooks/orders" }),
			]);

			const result = await client
				.definitions({ subscriptionTargetBaseUrl: "https://default.example.com" })
				.sync(set.build());
			assert.ok(result.isOk(), JSON.stringify(result));

			const subscriptionCall = requests.find((r) => r.url.includes("/subscriptions/sync"));
			const entries = subscriptionCall?.body["subscriptions"] as Array<{ target: string }>;
			assert.equal(entries[0]?.target, "https://default.example.com/webhooks/orders");
		},
	);
});

test("a path target with no base available fails that subscription's scope locally, sends nothing, and fails the overall call", async () => {
	await withServer(
		(r) => ({ status: 200, json: okSyncResponse(r.body) }),
		async (client, requests) => {
			const set = defineApplication("orders")
				.withSubscriptions([sub({ code: "no-base-sub", target: "/webhooks/orders" })])
				.build();

			const result = await client.definitions().sync(set);
			assert.ok(result.isErr(), "target resolution failure must surface as Err");
			assert.equal(requests.length, 0, "no subscriptions call is sent");

			const partial = expectPartialFailure(result);
			const subscriptions = partial.subscriptions;
			assert.ok("error" in subscriptions);
			if ("error" in subscriptions) {
				assert.match(subscriptions.error ?? "", /no-base-sub/);
			}
		},
	);
});

// ── duplicate codes within a scope ────────────────────────────────────────

test("duplicate code within a scope across two merged sets fails that type+scope locally, fails the overall call, but the unrelated type still synced", async () => {
	await withServer(
		(r) => ({ status: 200, json: okSyncResponse(r.body) }),
		async (client, requests) => {
			const setOne = defineApplication("orders")
				.withConnections([conn({ code: "dup-conn" })])
				.withRoles([{ name: "role-one" }]);
			const setTwo = defineApplication("orders")
				.withConnections([conn({ code: "dup-conn" })])
				.withRoles([{ name: "role-two" }]);

			const results = await client
				.definitions()
				.syncGrouped([setOne.build(), setTwo.build()], { removeUnlisted: true });
			assert.ok(results.isErr(), "a duplicate code must fail the overall syncGrouped call");

			const connectionCalls = requests.filter((r) => r.url.includes("/connections/sync"));
			const roleCalls = requests.filter((r) => r.url.includes("/roles/sync"));
			assert.equal(connectionCalls.length, 0, "no call is sent for the duplicated scope");
			assert.equal(roleCalls.length, 1, "roles, an unrelated type, still syncs");

			const partialByApp = expectPartialFailure(results);
			const ordersResult = partialByApp["orders"];
			assert.ok(ordersResult, "the partial result for orders must still be retrievable from the error");
			const connectionsResult = ordersResult.connections;
			assert.ok("error" in connectionsResult);
			if ("error" in connectionsResult) {
				assert.match(connectionsResult.error ?? "", /dup-conn/);
				assert.match(connectionsResult.error ?? "", /orders/);
			}
			const rolesResult = ordersResult.roles;
			assert.ok(!("error" in rolesResult), "roles must show as a clean success in the partial result");
			if (!("error" in rolesResult) && "created" in rolesResult) {
				assert.equal(rolesResult.created, 2, "both merged sets' roles actually synced");
			}
		},
	);
});

// ── connection failure skips that scope's subscriptions, but sibling scopes still sync ──

test("a connection sync HTTP failure in one scope fails the overall call, but a sibling scope's calls still happen and its counts survive in the partial result", async () => {
	await withServer(
		(r) => {
			// Only the GLOBAL connections call fails; client-a's own connections
			// call (and everything else) succeeds normally.
			if (r.url.includes("/connections/sync") && !("clientId" in r.body)) {
				return { status: 500, json: { error: "INTERNAL", message: "boom" } };
			}
			return { status: 200, json: okSyncResponse(r.body) };
		},
		async (client, requests) => {
			const set = defineApplication("orders")
				.withConnections([
					conn({ code: "conn-global" }),
					conn({ code: "conn-a", client: "client-a" }),
				])
				.withSubscriptions([
					sub({ code: "sub-global", connectionCode: "conn-global" }),
					sub({ code: "sub-a", connectionCode: "conn-a", client: "client-a" }),
				])
				.build();

			const result = await client.definitions().sync(set);
			assert.ok(result.isErr(), "a scope's connection sync failure must fail the overall call");

			// The global scope's connections call failed and its subscriptions
			// call was skipped (2 requests), but client-a — a SIBLING scope —
			// still got both of its calls (2 more requests): 4 total, not 1.
			assert.equal(requests.length, 3, "global connections (failed) + client-a connections + client-a subscriptions");
			const clientACalls = requests.filter((r) => r.body["clientId"] === "client-a");
			assert.equal(clientACalls.length, 2, "the sibling scope's connections AND subscriptions calls both happened");

			const partial = expectPartialFailure(result);
			assert.ok("error" in partial.connections);
			if ("error" in partial.connections) {
				assert.match(partial.connections.error ?? "", /boom/);
				assert.equal(partial.connections.created, 1, "client-a's connection still counts as created");
			}
			assert.ok("error" in partial.subscriptions);
			if ("error" in partial.subscriptions) {
				assert.match(partial.subscriptions.error ?? "", /connection sync failed/);
				assert.equal(partial.subscriptions.created, 1, "client-a's subscription still counts as created");
			}
		},
	);
});

// ── syncGrouped runs every application to completion ──────────────────────

test("syncGrouped runs every application to completion: a failure in the first application does not stop the second", async () => {
	await withServer(
		(r) => {
			// Only "billing"'s connections call fails.
			if (r.url.includes("/applications/billing/connections/sync")) {
				return { status: 500, json: { error: "INTERNAL", message: "billing boom" } };
			}
			return { status: 200, json: okSyncResponse(r.body) };
		},
		async (client, requests) => {
			const billing = defineApplication("billing").withConnections([conn({ code: "billing-conn" })]);
			const orders = defineApplication("orders").withConnections([conn({ code: "orders-conn" })]);

			const results = await client.definitions().syncGrouped([billing.build(), orders.build()]);
			assert.ok(results.isErr(), "a failed application must fail the overall syncGrouped call");

			// "orders" (the second application) was still synced despite
			// "billing" (the first) failing.
			const ordersCalls = requests.filter((r) => r.url.includes("/applications/orders/"));
			assert.equal(ordersCalls.length, 1, "the second application's call still happened");

			const partialByApp = expectPartialFailure(results);
			assert.ok("error" in partialByApp["billing"]!.connections);
			assert.ok(partialByApp["orders"], "the second application's result is retrievable from the error too");
			assert.ok(!("error" in partialByApp["orders"]!.connections));
			if (!("error" in partialByApp["orders"]!.connections) && "created" in partialByApp["orders"]!.connections) {
				assert.equal(partialByApp["orders"]!.connections.created, 1);
			}
		},
	);
});

// ── forClient() scopes a whole set ────────────────────────────────────────

test("DefinitionSet.forClient() scopes every connection/subscription that doesn't set its own client", async () => {
	await withServer(
		(r) => ({ status: 200, json: okSyncResponse(r.body) }),
		async (client, requests) => {
			const set = defineApplication("orders")
				.forClient("acme")
				.withConnections([{ code: "acme-conn", name: "Acme Conn" }])
				.build();

			const result = await client.definitions().sync(set);
			assert.ok(result.isOk(), JSON.stringify(result));
			assert.equal(requests.length, 1);
			assert.equal(requests[0]?.body["clientId"], "acme");
		},
	);
});
