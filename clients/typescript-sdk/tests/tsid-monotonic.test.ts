import { test } from "node:test";
import assert from "node:assert/strict";
import { generate } from "../src/outbox/tsid.js";

test("tsid is strictly increasing within one process, even within a millisecond", () => {
	const ids = Array.from({ length: 10_000 }, () => generate());
	for (let i = 1; i < ids.length; i++) {
		assert.ok(ids[i - 1] < ids[i], `${ids[i - 1]} !< ${ids[i]}`);
	}
	assert.ok(ids.every((id) => id.length === 13));
});
