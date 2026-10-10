/**
 * GET /api/clients returns every client and has no paging parameters (bug 16
 * in docs/ui/README.md, docs/platform-bug-fixes.md): the client list's
 * server "pages" repeated the same list, and the user drawer's page loop never
 * ended from 100 clients. clientsApi.list sends no page/pageSize.
 */

import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, it, vi } from "vitest";
import { clientsApi } from "@/api/clients";

describe("clientsApi.list", () => {
	afterEach(() => vi.unstubAllGlobals());

	it("asks for the whole list, with no paging parameters", async () => {
		const urls: string[] = [];
		vi.stubGlobal(
			"fetch",
			vi.fn(async (input: RequestInfo | URL) => {
				urls.push(String(input));
				return new Response(JSON.stringify({ clients: [], total: 0 }), {
					status: 200,
					headers: { "Content-Type": "application/json" },
				});
			}),
		);
		await clientsApi.list();
		await clientsApi.list({ status: "ACTIVE" });
		expect(urls).toHaveLength(2);
		for (const u of urls) {
			expect(u).not.toMatch(/page/);
		}
		expect(urls[1]).toContain("status=ACTIVE");
	});
});

function sources(dir: string): string[] {
	return readdirSync(dir).flatMap((name) => {
		const path = join(dir, name);
		if (statSync(path).isDirectory()) return sources(path);
		return /\.(vue|ts)$/.test(name) ? [path] : [];
	});
}

describe("clientsApi.list callers", () => {
	it("never page the client list", () => {
		const src = fileURLToPath(new URL("../src", import.meta.url));
		const offenders = sources(src).filter((file) =>
			/clientsApi\.list\(\s*\{[^}]*\bpage/.test(readFileSync(file, "utf8")),
		);
		expect(offenders).toEqual([]);
	});
});
