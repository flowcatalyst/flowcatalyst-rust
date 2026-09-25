/**
 * The SDK must refuse to emit unqualified codes: the platform facets on the
 * four `application:subdomain:aggregate:action` segments and resolves
 * delivery signing credentials from the application segment, so a bare
 * one-word code renders as `name:::`, facets under the wrong application,
 * and delivers unsigned.
 */
import { describe, it } from "node:test";
import assert from "node:assert/strict";

import { CreateDispatchJobDto, CreateEventDto } from "../src/outbox/index.js";

describe("qualified-code enforcement", () => {
	it("accepts fully qualified codes", () => {
		CreateDispatchJobDto.create(
			"fulfil-go",
			"fulfil-go:fulfilment:part:create-pick",
			"https://example.com/hook",
			"{}",
			"pool-1",
		);
		CreateEventDto.create("fulfil-go:fulfilment:part:picked", { ok: true });
	});

	it("rejects bare and partially qualified codes", () => {
		for (const bad of ["create-pick", "a:b:c", "a:b:c:d:e", "a::c:d", " : : : "]) {
			assert.throws(
				() => CreateDispatchJobDto.create("src", bad, "https://x", "{}", "p"),
				/fully qualified/,
				`dispatch code '${bad}' must be rejected`,
			);
			assert.throws(
				() => CreateEventDto.create(bad, {}),
				/fully qualified/,
				`event type '${bad}' must be rejected`,
			);
		}
	});
});
