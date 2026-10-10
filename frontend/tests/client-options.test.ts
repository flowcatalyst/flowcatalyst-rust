/**
 * The client select offers the clients the caller can access
 * (docs/platform-bug-fixes.md, dispatch pools/connections item): it searched
 * GET /api/clients/search, which is anchor-only, so a client-scoped user
 * could not pick its own client and could create no pool, connection or
 * subscription.
 */

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { matchClientOptions } from "@/utils/client-options";

const options = [
	{ value: "clt_acme", label: "Acme Corp" },
	{ value: "clt_globex", label: "Globex" },
];

describe("matchClientOptions", () => {
	it("matches name or id, ignoring case", () => {
		expect(matchClientOptions(options, "acme")).toEqual([options[0]]);
		expect(matchClientOptions(options, "CLT_GLO")).toEqual([options[1]]);
		expect(matchClientOptions(options, "")).toEqual(options);
		expect(matchClientOptions(options, "", 1)).toEqual([options[0]]);
	});
});

describe("ClientSelect", () => {
	it("uses the caller's accessible clients, not the anchor-only search", () => {
		const src = readFileSync(
			fileURLToPath(
				new URL("../src/components/ClientSelect.vue", import.meta.url),
			),
			"utf8",
		);
		expect(src).not.toContain("clientsApi.search");
		expect(src).toContain("useClientOptions");
	});
});
