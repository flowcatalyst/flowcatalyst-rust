/**
 * The dispatch job drawer offers Sign only to users holding
 * dispatch-job:view-raw, which POST /api/dispatch-jobs/{id}/sign requires
 * (bug 20 in docs/ui/README.md, docs/platform-bug-fixes.md): everyone saw it
 * and most got a 403.
 */

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { userHasPermission } from "@/stores/permissions";
import { DISPATCH_JOB_VIEW_RAW } from "@/utils/dispatch-job-permissions";

describe("Sign", () => {
	it("is the API's view-raw permission", () => {
		expect(DISPATCH_JOB_VIEW_RAW).toBe(
			"platform:messaging:dispatch-job:view-raw",
		);
		expect(
			userHasPermission(
				{ permissions: ["platform:messaging:dispatch-job:view"] },
				DISPATCH_JOB_VIEW_RAW,
			),
		).toBe(false);
		expect(
			userHasPermission(
				{ permissions: ["platform:messaging:*:*"] },
				DISPATCH_JOB_VIEW_RAW,
			),
		).toBe(true);
	});

	it("is shown only with that permission", () => {
		const src = readFileSync(
			fileURLToPath(
				new URL(
					"../src/pages/dispatch-jobs/DispatchJobDetailDrawer.vue",
					import.meta.url,
				),
			),
			"utf8",
		);
		expect(src).toMatch(/<Button\s+v-if="canSign"\s+label="Sign"/);
		expect(src).toContain(
			"userHasPermission(authStore.user, DISPATCH_JOB_VIEW_RAW)",
		);
	});
});
