import { test } from "node:test";
import assert from "node:assert/strict";
import { mockFetch } from "./helpers/mock-fetch.js";

const APP = {
	id: "app_1",
	code: "orders",
	name: "Orders",
	type: "APPLICATION",
	active: true,
	hasLoginClient: false,
	createdAt: "2026-01-01T00:00:00Z",
	updatedAt: "2026-01-01T00:00:00Z",
};

test("applications.getServiceAccount reads the application, then GET /api/service-accounts/{id}", async () => {
	const m = mockFetch({
		"GET /api/applications/app_1": { body: { ...APP, serviceAccountId: "sa_9" } },
		"GET /api/service-accounts/sa_9": {
			body: {
				id: "sa_9",
				code: "orders-sa",
				name: "Orders SA",
				active: true,
				clientIds: [],
				authType: "OAUTH",
				roles: ["orders:app"],
				createdAt: "2026-01-01T00:00:00Z",
				updatedAt: "2026-01-01T00:00:00Z",
			},
		},
	});
	try {
		const res = await m.client.applications().getServiceAccount("app_1");
		assert.ok(res.isOk(), JSON.stringify(res));
		assert.equal(res._unsafeUnwrap().id, "sa_9");
		assert.deepEqual(res._unsafeUnwrap().roles, ["orders:app"]);
		assert.deepEqual(
			m.requests.map((r) => `${r.method} ${r.path}`),
			["GET /api/applications/app_1", "GET /api/service-accounts/sa_9"],
		);
	} finally {
		m.restore();
	}
});

test("applications.getServiceAccount is not_found when the application has none", async () => {
	const m = mockFetch({ "GET /api/applications/app_1": { body: APP } });
	try {
		const res = await m.client.applications().getServiceAccount("app_1");
		assert.ok(res.isErr());
		assert.equal(res._unsafeUnwrapErr().type, "not_found");
		assert.equal(m.requests.length, 1, "no service-account request");
		assert.ok(!m.requests.some((r) => r.path.endsWith("/service-account")));
	} finally {
		m.restore();
	}
});

test("applications.getClientConfig decodes Go's ClientConfigResponse", async () => {
	const m = mockFetch({
		"GET /api/applications/app_1/clients/clt_1": {
			body: {
				id: "acc_1",
				applicationId: "app_1",
				clientId: "clt_1",
				enabled: true,
				configJson: { theme: "dark" },
				createdAt: "2026-01-01T00:00:00Z",
				updatedAt: "2026-01-01T00:00:00Z",
			},
		},
	});
	try {
		const res = await m.client.applications().getClientConfig("app_1", "clt_1");
		assert.ok(res.isOk(), JSON.stringify(res));
		const c = res._unsafeUnwrap();
		assert.deepEqual(c.configJson, { theme: "dark" });
		assert.deepEqual(c.config, { theme: "dark" }, "deprecated alias");
		assert.equal(m.requests[0]?.method, "GET");
	} finally {
		m.restore();
	}
});

test("applications.updateClientConfig still sends PUT (deprecated, Rust platform only)", async () => {
	const m = mockFetch({
		"PUT /api/applications/app_1/clients/clt_1": {
			body: { id: "acc_1", applicationId: "app_1", clientId: "clt_1", enabled: false },
		},
	});
	try {
		const res = await m.client
			.applications()
			.updateClientConfig("app_1", "clt_1", { enabled: false });
		assert.ok(res.isOk(), JSON.stringify(res));
		assert.deepEqual(m.requests[0]?.body, { enabled: false });
	} finally {
		m.restore();
	}
});

test("applications.listRoles decodes Go's { roles: string[] }", async () => {
	const m = mockFetch({
		"GET /api/applications/by-id/app_1/roles": {
			body: { roles: ["orders:admin", "orders:viewer"] },
		},
	});
	try {
		const res = await m.client.applications().listRoles("app_1");
		assert.ok(res.isOk(), JSON.stringify(res));
		assert.deepEqual(res._unsafeUnwrap().roles, ["orders:admin", "orders:viewer"]);
	} finally {
		m.restore();
	}
});

test("applications.listClients reads Go's items / configJson", async () => {
	const m = mockFetch({
		"GET /api/applications/app_1/clients": {
			body: {
				items: [
					{
						id: "acc_1",
						applicationId: "app_1",
						clientId: "clt_1",
						enabled: true,
						baseUrlOverride: "https://acme.example",
						configJson: { a: 1 },
						createdAt: "2026-01-01T00:00:00Z",
						updatedAt: "2026-01-01T00:00:00Z",
					},
				],
			},
		},
	});
	try {
		const res = await m.client.applications().listClients("app_1");
		assert.ok(res.isOk(), JSON.stringify(res));
		const r = res._unsafeUnwrap();
		assert.equal(r.items.length, 1);
		assert.deepEqual(r.items[0]?.configJson, { a: 1 });
		assert.deepEqual(r.items[0]?.config, { a: 1 });
		assert.equal(r.clientConfigs, r.items, "deprecated alias is the same array");
		assert.equal(r.total, 1);
	} finally {
		m.restore();
	}
});

test("applications.provisionServiceAccount keeps Go's nested one-time secret", async () => {
	const m = mockFetch({
		"POST /api/applications/app_1/provision-service-account": {
			status: 201,
			body: {
				message: "Service account provisioned",
				serviceAccount: {
					principalId: "prn_1",
					name: "Orders SA",
					oauthClient: { id: "oac_1", clientId: "orders-sa", clientSecret: "s3cret" },
				},
			},
		},
	});
	try {
		const res = await m.client.applications().provisionServiceAccount("app_1");
		assert.ok(res.isOk(), JSON.stringify(res));
		const r = res._unsafeUnwrap();
		assert.equal(r.serviceAccount?.oauthClient.clientSecret, "s3cret");
		assert.equal(r.serviceAccount?.principalId, "prn_1");
		assert.equal(r.clientSecret, "s3cret", "deprecated flat copy");
		assert.equal(r.clientId, "orders-sa", "deprecated flat copy");
	} finally {
		m.restore();
	}
});

test("applications create answers { id }; update / enable / disable resolve void on 204", async () => {
	const m = mockFetch({
		"POST /api/applications": { status: 201, body: { id: "app_2" } },
		"PUT /api/applications/app_2": { status: 204 },
		"POST /api/applications/app_2/clients/clt_1/enable": { status: 204 },
		"POST /api/applications/app_2/clients/clt_1/disable": { status: 204 },
	});
	try {
		const apps = m.client.applications();
		const created = await apps.create({ code: "billing", name: "Billing" });
		assert.deepEqual(created._unsafeUnwrap(), { id: "app_2" });
		assert.deepEqual(m.requests[0]?.body, { code: "billing", name: "Billing" });

		const updated = await apps.update("app_2", { name: "Billing 2" });
		assert.ok(updated.isOk());
		assert.equal(updated._unsafeUnwrap(), undefined);

		assert.equal((await apps.enableForClient("app_2", "clt_1"))._unsafeUnwrap(), undefined);
		assert.equal((await apps.disableForClient("app_2", "clt_1"))._unsafeUnwrap(), undefined);
		assert.deepEqual(
			m.requests.map((r) => `${r.method} ${r.path}`),
			[
				"POST /api/applications",
				"PUT /api/applications/app_2",
				"POST /api/applications/app_2/clients/clt_1/enable",
				"POST /api/applications/app_2/clients/clt_1/disable",
			],
		);
	} finally {
		m.restore();
	}
});

test("applications.list sends Go's type / active filters", async () => {
	const m = mockFetch({
		"GET /api/applications": { body: { applications: [APP], total: 1 } },
	});
	try {
		const res = await m.client.applications().list({ type: "APPLICATION", active: "true" });
		assert.ok(res.isOk(), JSON.stringify(res));
		assert.equal(m.requests[0]?.query.get("type"), "APPLICATION");
		assert.equal(m.requests[0]?.query.get("active"), "true");
		assert.equal(res._unsafeUnwrap().applications[0]?.hasLoginClient, false);
	} finally {
		m.restore();
	}
});
