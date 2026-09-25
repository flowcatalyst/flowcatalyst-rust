import { test } from "node:test";
import assert from "node:assert/strict";

import {
	generate,
	generateWithPrefix,
	isValid,
} from "../src/outbox/tsid.js";
import { CreateEventDto } from "../src/outbox/create-event-dto.js";
import { CreateAuditLogDto } from "../src/outbox/create-audit-log-dto.js";
import { CreateDispatchJobDto } from "../src/outbox/create-dispatch-job-dto.js";

// ─── Branded TSIDs ──────────────────────────────────────────────────────────

test("generateWithPrefix produces a branded id: <prefix>_<raw>", () => {
	const id = generateWithPrefix("cmt");
	assert.match(id, /^cmt_[0-9A-HJKMNP-TV-Z]{13}$/i);
	assert.ok(isValid(id.slice(4)), "the part after the prefix is a valid TSID");
});

test("generate stays raw (no prefix)", () => {
	const id = generate();
	assert.equal(id.length, 13);
	assert.ok(isValid(id));
});

test("generateWithPrefix rejects an empty or underscored prefix", () => {
	assert.throws(() => generateWithPrefix(""));
	assert.throws(() => generateWithPrefix("cm_t"));
});

// ─── Client-centric linkage on the DTOs ─────────────────────────────────────

test("CreateEventDto.withClientCode is emitted in the payload", () => {
	const base = CreateEventDto.create("shop:orders:order:placed", { a: 1 });
	assert.equal(base.toPayload().clientCode, undefined, "omitted when unset");

	const withClient = base.withClientCode("acme");
	assert.equal(withClient.toPayload().clientCode, "acme");
});

test("CreateAuditLogDto carries applicationCode + clientCode", () => {
	const base = CreateAuditLogDto.create("order", "ord_1", "placed");
	const p0 = base.toPayload();
	assert.equal(p0.applicationCode, undefined);
	assert.equal(p0.clientCode, undefined);

	const linked = base.withApplicationCode("shop").withClientCode("acme");
	const p1 = linked.toPayload();
	assert.equal(p1.applicationCode, "shop");
	assert.equal(p1.clientCode, "acme");
});

test("CreateDispatchJobDto carries an explicit queue but omits an unset one", () => {
	const base = CreateDispatchJobDto.create(
		"svc",
		"app:sub:agg:act",
		"https://example.com/hook",
		'{"k":1}',
		"pool-1",
	);
	assert.equal(base.toPayload().queue, undefined, "an unset queue must be omitted, never defaulted");

	const withQueue = base.withQueue("HIGH_PRIORITY");
	assert.equal(withQueue.toPayload().queue, "HIGH_PRIORITY");
});
