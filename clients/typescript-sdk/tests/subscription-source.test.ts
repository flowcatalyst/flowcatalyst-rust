/**
 * `source` on `SubscriptionResponse` is generated as `string | null`, not a
 * closed union — a `FUNCTION`-sourced subscription (function promotion, see
 * docs/spec) must round-trip the same as any other source value.
 */
import { test } from "node:test";
import assert from "node:assert/strict";

import type { SubscriptionResponse } from "../src/generated/types.gen.js";

test("SubscriptionResponse accepts a FUNCTION source", () => {
	const raw = JSON.stringify({
		id: "sub_1",
		code: "orders-shipped",
		name: "Orders Shipped",
		endpoint: "https://example.com/webhook",
		source: "FUNCTION",
		status: "ACTIVE",
		mode: "IMMEDIATE",
		clientScoped: false,
		customConfig: [],
		dataOnly: true,
		delaySeconds: 0,
		eventTypes: [],
		maxAgeSeconds: 86400,
		maxRetries: 0,
		sequence: 99,
		timeoutSeconds: 30,
		createdAt: "2026-01-01T00:00:00Z",
		updatedAt: "2026-01-01T00:00:00Z",
	});

	const sub: SubscriptionResponse = JSON.parse(raw);
	assert.equal(sub.source, "FUNCTION");
});
