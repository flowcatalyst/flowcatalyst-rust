/**
 * The subscription forms offer the queue values the server accepts (bug 10
 * in docs/ui/README.md, docs/platform-bug-fixes.md): the SPA's free-text
 * Queue field failed with 400 INVALID_QUEUE for anything but these.
 */

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { QUEUE_OPTIONS, queueFormValue } from "@/utils/subscription-queue";

describe("subscription queue", () => {
	it("offers exactly the server's values", () => {
		expect(QUEUE_OPTIONS.map((o) => o.value)).toEqual([
			"DEFAULT",
			"HIGH_PRIORITY",
		]);
	});

	it("maps a stored queue to a form value (unset is DEFAULT)", () => {
		expect(queueFormValue(null)).toBe("DEFAULT");
		expect(queueFormValue("")).toBe("DEFAULT");
		expect(queueFormValue("HIGH_PRIORITY")).toBe("HIGH_PRIORITY");
		expect(queueFormValue("high_priority")).toBe("HIGH_PRIORITY");
	});

	it("is not a free-text field in the create and edit forms", () => {
		for (const file of [
			"SubscriptionCreateDrawer.vue",
			"SubscriptionDetailDrawer.vue",
		]) {
			const src = readFileSync(
				fileURLToPath(
					new URL(`../src/pages/subscriptions/${file}`, import.meta.url),
				),
				"utf8",
			);
			expect(src).not.toMatch(/<InputText[^>]*v-model="(queue|editQueue)"/);
			expect(src).toContain("QUEUE_OPTIONS");
		}
	});
});
