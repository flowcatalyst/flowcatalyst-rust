/**
 * Audit logs must never store passwords or secrets (docs/spec/audit-redaction.md).
 * `tests/fixtures/audit-redaction-vectors.json` is a byte-identical copy of
 * the canonical `docs/spec/audit-redaction-vectors.json` — every case here
 * must pass in every SDK and in the platform.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

import {
	auditMaskedFieldsOf,
	redactAuditData,
	type AuditMasked,
} from "../src/outbox/audit-redaction.js";
import { CreateAuditLogDto } from "../src/outbox/create-audit-log-dto.js";
import type { OutboxDriver, OutboxMessage } from "../src/outbox/types.js";
import { BaseDomainEvent } from "../src/usecase/domain-event.js";
import { ExecutionContext } from "../src/usecase/execution-context.js";
import { OutboxUnitOfWork } from "../src/usecase/outbox-unit-of-work.js";
import { isSuccess } from "../src/usecase/result.js";

interface Vector {
	name: string;
	input: Record<string, unknown>;
	masked: string[];
	expected: Record<string, unknown>;
}

const here = dirname(fileURLToPath(import.meta.url));
const vectors: Vector[] = JSON.parse(
	readFileSync(join(here, "fixtures", "audit-redaction-vectors.json"), "utf8"),
);

test("the fixture copy is byte-identical to the canonical spec vectors", () => {
	const canonicalPath = join(here, "..", "..", "..", "docs", "spec", "audit-redaction-vectors.json");
	const canonical = readFileSync(canonicalPath, "utf8");
	const copy = readFileSync(join(here, "fixtures", "audit-redaction-vectors.json"), "utf8");
	assert.equal(copy, canonical);
});

test("redactAuditData never mutates its input", () => {
	const input = { password: "hunter2", nested: { token: "t" } };
	const before = JSON.stringify(input);
	redactAuditData(input);
	assert.equal(JSON.stringify(input), before);
});

for (const vector of vectors) {
	test(`vector: ${vector.name}`, () => {
		const actual = redactAuditData(vector.input, vector.masked);
		assert.deepStrictEqual(actual, vector.expected);
	});
}

test("a password in operationData never appears in the outbox payload string", () => {
	const dto = CreateAuditLogDto.create("Principal", "p_1", "CREATE").withOperationData({
		email: "a@b.c",
		password: "hunter2",
		webhookCredentials: { token: "tok", signingSecret: "s3" },
	});

	const payload = JSON.stringify(dto.toPayload());

	assert.ok(!payload.includes("hunter2"), "password value must not appear in the payload");
	assert.ok(!payload.includes("tok\""), "nested token value must not appear in the payload");
	assert.ok(!payload.includes("s3\""), "nested secret value must not appear in the payload");
	assert.ok(payload.includes("a@b.c"), "non-secret fields must survive redaction");
});

test("CreateAuditLogDto.withOperationData masks a caller-declared field the name rule would keep", () => {
	const dto = CreateAuditLogDto.create("PlatformConfig", "cfg_1", "SET_PROPERTY").withOperationData(
		{ property: "key", value: "sk_live_123", valueType: "SECRET" },
		["value"],
	);

	const operationData = JSON.parse(dto.toPayload().operationData as string);
	assert.equal(operationData.value, "***");
	assert.equal(operationData.valueType, "SECRET", "unmasked sibling fields survive");
});

test("a nested Date is serialised as JSON would, not walked into an empty object", () => {
	const when = new Date("2026-09-24T12:00:00.000Z");
	const actual = redactAuditData({ scheduledFor: when, nested: { at: when, token: "t" } });
	assert.deepStrictEqual(actual, {
		scheduledFor: "2026-09-24T12:00:00.000Z",
		nested: { at: "2026-09-24T12:00:00.000Z", token: "***" },
	});
});

class SetPropertyCommand implements AuditMasked {
	constructor(
		readonly property: string,
		readonly value: string,
		readonly valueType: string,
		readonly apiKey: string = "k",
	) {}
	auditMaskedFields(): readonly string[] {
		return this.valueType === "PLAIN" ? [] : ["value"];
	}
}

class PropertySet extends BaseDomainEvent<{ property: string }> {
	constructor(ctx: ExecutionContext) {
		super(
			{
				eventType: "shop:config:property:set",
				specVersion: "1.0",
				source: "shop:config",
				subject: "config.property.cfg_1",
				messageGroup: "config:property:cfg_1",
			},
			ctx,
			{ property: "stripe" },
		);
	}
}

test("auditMaskedFieldsOf reads a command's declaration, or none", () => {
	assert.deepStrictEqual(auditMaskedFieldsOf(new SetPropertyCommand("a", "b", "SECRET")), ["value"]);
	assert.deepStrictEqual(auditMaskedFieldsOf(new SetPropertyCommand("a", "b", "PLAIN")), []);
	assert.deepStrictEqual(auditMaskedFieldsOf({ value: "x" }), []);
	assert.deepStrictEqual(auditMaskedFieldsOf(null), []);
});

test("the outbox unit of work redacts the audit row and honours AuditMasked", async () => {
	const written: OutboxMessage[] = [];
	const driver: OutboxDriver = {
		insert: async (message) => {
			written.push(message);
		},
		insertBatch: async (messages) => {
			written.push(...messages);
		},
	};
	const uow = OutboxUnitOfWork.fromDriver(driver, "clt_1", { auditEnabled: true });
	const ctx = ExecutionContext.create("prn_1");

	for (const valueType of ["SECRET", "PLAIN"]) {
		const result = await uow.emitEvent(
			new PropertySet(ctx),
			new SetPropertyCommand("stripe", "sk_live_123", valueType),
		);
		assert.ok(isSuccess(result));
	}

	const audits = written.filter((m) => m.type === "AUDIT_LOG");
	assert.equal(audits.length, 2);
	const data = audits.map(
		(m) => JSON.parse(JSON.parse(m.payload).operationData) as Record<string, unknown>,
	);
	assert.deepStrictEqual(data[0], {
		property: "stripe",
		value: "***",
		valueType: "SECRET",
		apiKey: "***",
	});
	assert.deepStrictEqual(data[1], {
		property: "stripe",
		value: "sk_live_123",
		valueType: "PLAIN",
		apiKey: "***",
	});
});
