/**
 * The pool drawer's edit can clear the description and the rate limit
 * (docs/platform-bug-fixes.md, dispatch pools item): it sent
 * `value || undefined`, which the API reads as "leave it", so neither could
 * be removed. The API clears a blank description and a zero rate limit.
 */

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

describe("dispatch pool edit body", () => {
	it("sends blanks as clears, not as omitted", () => {
		const src = readFileSync(
			fileURLToPath(
				new URL(
					"../src/pages/dispatch-pools/DispatchPoolDetailDrawer.vue",
					import.meta.url,
				),
			),
			"utf8",
		);
		expect(src).not.toMatch(
			/description: editDescription\.value \|\| undefined/,
		);
		expect(src).not.toMatch(/rateLimit: editRateLimit\.value \|\| undefined/);
		expect(src).toContain("description: editDescription.value,");
		expect(src).toContain("rateLimit: editRateLimit.value ?? 0,");
	});
});
