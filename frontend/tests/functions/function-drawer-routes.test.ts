// @vitest-environment jsdom
/**
 * The function pages follow Go's list-and-drawer idiom: create, domain claim,
 * domain detail and policy detail are child routes of their list (rendered
 * in the list's drawer outlet); a function's own page and the manifest
 * editor stay full pages.
 */
import { describe, expect, it } from "vitest";
import router from "@/router";

function resolved(path: string): string[] {
	return router.resolve(path).matched.map((r) => String(r.name ?? r.path));
}

describe("function routes", () => {
	it("opens create, claim and details in a drawer over their list", () => {
		expect(resolved("/functions/new")).toContain("functions");
		expect(resolved("/functions/new").at(-1)).toBe("function-create");
		expect(resolved("/function-domains/new").slice(-2)).toEqual([
			"function-domains",
			"function-domain-claim",
		]);
		expect(resolved("/function-domains/fn.acme.com").slice(-2)).toEqual([
			"function-domains",
			"function-domain-detail",
		]);
		expect(resolved("/function-policies/platform").slice(-2)).toEqual([
			"function-policies",
			"function-policy-detail",
		]);
	});

	it("keeps the function page and manifest editor as full pages", () => {
		expect(resolved("/functions/acme.orders.ship").at(-1)).toBe(
			"function-detail",
		);
		expect(resolved("/functions/acme.orders.ship")).not.toContain("functions");
		expect(resolved("/functions/acme.orders.ship/manifest").at(-1)).toBe(
			"function-manifest-editor",
		);
	});
});
