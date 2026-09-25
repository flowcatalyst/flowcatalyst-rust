import { test } from "node:test";
import assert from "node:assert/strict";
import {
	APP_CODE_ENV,
	defineApplication,
	defineApplicationFromEnv,
} from "../src/sync/definitions.js";

// An application code can be given directly or inherited from the environment.
// There is deliberately no per-definition override: the set a definition is
// built into IS its application, and a codebase owning several applications
// builds one set each and passes them to syncMany.

test("defineApplication takes the application code directly", () => {
	const set = defineApplication("orders").build();
	assert.equal(set.applicationCode, "orders");
});

test("defineApplicationFromEnv inherits the code from the environment", () => {
	const previous = process.env[APP_CODE_ENV];
	process.env[APP_CODE_ENV] = "measurement";
	try {
		const set = defineApplicationFromEnv().build();
		assert.equal(set.applicationCode, "measurement");
	} finally {
		if (previous === undefined) {
			delete process.env[APP_CODE_ENV];
		} else {
			process.env[APP_CODE_ENV] = previous;
		}
	}
});

// A missing code must fail loudly here rather than becoming a request to
// /api/applications/undefined/... much later.
test("defineApplicationFromEnv throws when the variable is unset or empty", () => {
	const previous = process.env[APP_CODE_ENV];
	try {
		delete process.env[APP_CODE_ENV];
		assert.throws(() => defineApplicationFromEnv(), new RegExp(APP_CODE_ENV));

		process.env[APP_CODE_ENV] = "";
		assert.throws(() => defineApplicationFromEnv(), new RegExp(APP_CODE_ENV));
	} finally {
		if (previous === undefined) {
			delete process.env[APP_CODE_ENV];
		} else {
			process.env[APP_CODE_ENV] = previous;
		}
	}
});
