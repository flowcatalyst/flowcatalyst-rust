/**
 * The process list's pencil and the full-page editor refuse an archived
 * process (bug 24 in docs/ui/README.md, docs/platform-bug-fixes.md): the
 * pencil opened the editor although the drawer hides Edit for it.
 */

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { canEditProcess } from "@/utils/process-rules";

const page = (name: string) =>
	readFileSync(
		fileURLToPath(new URL(`../src/pages/processes/${name}`, import.meta.url)),
		"utf8",
	);

describe("canEditProcess", () => {
	it("refuses an archived process", () => {
		expect(canEditProcess({ status: "ARCHIVED" })).toBe(false);
		expect(canEditProcess({ status: "CURRENT" })).toBe(true);
		expect(canEditProcess(null)).toBe(false);
	});

	it("gates the row pencil and the editor", () => {
		expect(page("ProcessListPage.vue")).toContain(
			':disabled="!canEditProcess(data)"',
		);
		expect(page("ProcessEditPage.vue")).toContain("if (!canEditProcess(p))");
	});
});
