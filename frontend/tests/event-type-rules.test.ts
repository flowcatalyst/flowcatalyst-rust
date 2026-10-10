/**
 * Event type lifecycle rules (src/utils/event-type-rules.ts): the list's
 * pencil and the drawer's edit, archive and delete (bugs 5 and 7 in
 * docs/ui/README.md, docs/platform-bug-fixes.md). Archive and delete match
 * the server's use cases.
 */

import { describe, expect, it } from "vitest";
import type { EventType } from "@/api/event-types";
import {
	canArchiveEventType,
	canDeleteEventType,
	canEditEventType,
} from "@/utils/event-type-rules";

type Rules = Pick<EventType, "status" | "specVersions">;

function et(status: string, ...versions: string[]): Rules {
	return {
		status,
		specVersions: versions.map((s) => ({ status: s })),
	} as unknown as Rules;
}

describe("canEditEventType", () => {
	it("refuses an archived event type (the row pencil no longer opens it in edit mode)", () => {
		expect(canEditEventType(et("ARCHIVED", "DEPRECATED"))).toBe(false);
	});
	it("allows a current event type", () => {
		expect(canEditEventType(et("CURRENT", "CURRENT"))).toBe(true);
	});
	it("refuses nothing loaded", () => {
		expect(canEditEventType(null)).toBe(false);
	});
});

describe("canArchiveEventType", () => {
	it("needs every version deprecated", () => {
		expect(canArchiveEventType(et("CURRENT"))).toBe(true);
		expect(canArchiveEventType(et("CURRENT", "DEPRECATED", "DEPRECATED"))).toBe(
			true,
		);
		expect(canArchiveEventType(et("CURRENT", "DEPRECATED", "CURRENT"))).toBe(
			false,
		);
		expect(canArchiveEventType(et("ARCHIVED", "DEPRECATED"))).toBe(false);
	});
});

describe("canDeleteEventType", () => {
	it("allows archived, or current with every version finalising", () => {
		expect(canDeleteEventType(et("ARCHIVED", "DEPRECATED"))).toBe(true);
		expect(canDeleteEventType(et("CURRENT"))).toBe(true);
		expect(canDeleteEventType(et("CURRENT", "FINALISING"))).toBe(true);
		expect(canDeleteEventType(et("CURRENT", "FINALISING", "CURRENT"))).toBe(
			false,
		);
	});
});
