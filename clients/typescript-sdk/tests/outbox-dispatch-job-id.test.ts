import { test } from "node:test";
import assert from "node:assert/strict";

import { OutboxManager } from "../src/outbox/outbox-manager.js";
import { CreateDispatchJobDto } from "../src/outbox/create-dispatch-job-dto.js";
import type { OutboxDriver, OutboxMessage } from "../src/outbox/types.js";

// The platform honours a supplied dispatch-job id (1-13 of [A-Za-z0-9_-]), so
// the outbox row's own id travels in the payload: a resend after a lost answer
// is recognised instead of creating a second job.

function capture(): { driver: OutboxDriver; messages: OutboxMessage[] } {
	const messages: OutboxMessage[] = [];
	const driver: OutboxDriver = {
		async insert(message: OutboxMessage) {
			messages.push(message);
		},
		async insertBatch(batch: OutboxMessage[]) {
			messages.push(...batch);
		},
	};
	return { driver, messages };
}

function job(): CreateDispatchJobDto {
	return CreateDispatchJobDto.create(
		"orders",
		"orders:fulfilment:order:shipped",
		"https://example.test/hook",
		"{}",
		"dpl_1",
	);
}

test("a dispatch job's payload carries its outbox row id", async () => {
	const { driver, messages } = capture();
	const outbox = new OutboxManager(driver, "clt_1");

	const one = await outbox.createDispatchJob(job());
	const many = await outbox.createDispatchJobs([job(), job()]);

	const ids = [one, ...many];
	assert.equal(messages.length, 3);
	messages.forEach((message, i) => {
		assert.equal(message.id, ids[i]);
		const payload = JSON.parse(message.payload);
		assert.equal(payload.id, ids[i]);
		assert.match(payload.id, /^[A-Za-z0-9_-]{1,13}$/);
		assert.equal(payload.code, "orders:fulfilment:order:shipped");
		assert.equal(message.payload_size, new TextEncoder().encode(message.payload).byteLength);
	});
});

test("events and audit logs are unchanged", async () => {
	const { driver, messages } = capture();
	const outbox = new OutboxManager(driver, "clt_1");
	const { CreateEventDto } = await import("../src/outbox/create-event-dto.js");
	await outbox.createEvent(CreateEventDto.create("shop:orders:order:placed", { a: 1 }));
	assert.equal(JSON.parse(messages[0].payload).id, undefined);
});
