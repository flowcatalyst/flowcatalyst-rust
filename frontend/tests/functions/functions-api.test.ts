/**
 * `api/functions.ts` against the Rust platform's function routes: the paths,
 * methods and bodies it sends, the raw-bytes artifact upload, 204s resolving
 * to undefined, and an error envelope's `details` reaching the caller.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { ApiError } from "@/api/client";
import { functionsApi } from "@/api/functions";

interface Call {
	url: string;
	method: string;
	headers: Record<string, string>;
	body: unknown;
}

let calls: Call[] = [];
let respond: () => Response = () => json({});

function json(body: unknown, status = 200): Response {
	return new Response(JSON.stringify(body), {
		status,
		headers: { "Content-Type": "application/json" },
	});
}

beforeEach(() => {
	calls = [];
	respond = () => json({});
	vi.stubGlobal(
		"fetch",
		vi.fn(async (url: string, init: RequestInit = {}) => {
			calls.push({
				url,
				method: init.method ?? "GET",
				headers: (init.headers ?? {}) as Record<string, string>,
				body: init.body,
			});
			return respond();
		}),
	);
});

afterEach(() => {
	vi.unstubAllGlobals();
});

describe("functionsApi", () => {
	it("lists with the address pattern, owner, status and 0-based page", async () => {
		respond = () => json({ data: [], page: 1, size: 20, total: 0, total_pages: 0 });
		const page = await functionsApi.list({
			address: "acme.*",
			clientId: "platform",
			status: "ACTIVE",
			page: 1,
			size: 20,
		});
		expect(calls[0]!.url).toBe(
			"/api/functions?address=acme.*&clientId=platform&status=ACTIVE&page=1&size=20",
		);
		expect(page.total_pages).toBe(0);
	});

	it("addresses a function by its dotted address in one path segment", async () => {
		await functionsApi.get("acme.orders.ship");
		await functionsApi.status("acme.orders.ship");
		await functionsApi.getVersion("acme.orders.ship", 3);
		await functionsApi.retireVersion("acme.orders.ship", 3);
		expect(calls.map((c) => `${c.method} ${c.url}`)).toEqual([
			"GET /api/functions/acme.orders.ship",
			"GET /api/functions/acme.orders.ship/status",
			"GET /api/functions/acme.orders.ship/versions/3",
			"POST /api/functions/acme.orders.ship/versions/3/retire",
		]);
	});

	it("resolves the 204 routes to undefined", async () => {
		respond = () => new Response(null, { status: 204 });
		await expect(functionsApi.update("a.b.c", { status: "DISABLED" })).resolves.toBeUndefined();
		await expect(functionsApi.delete("a.b.c")).resolves.toBeUndefined();
		await expect(functionsApi.deleteAlias("a.b.c", "qa")).resolves.toBeUndefined();
		await expect(functionsApi.setSecret("a.b.c", "API_KEY", { value: "s3cret" })).resolves.toBeUndefined();
		await expect(functionsApi.deleteSecret("a.b.c", "API_KEY")).resolves.toBeUndefined();
		await expect(functionsApi.releaseDomain("fn.acme.com")).resolves.toBeUndefined();
		expect(calls.map((c) => `${c.method} ${c.url}`)).toEqual([
			"PUT /api/functions/a.b.c",
			"DELETE /api/functions/a.b.c",
			"DELETE /api/functions/a.b.c/aliases/qa",
			"PUT /api/functions/a.b.c/secrets/API_KEY",
			"DELETE /api/functions/a.b.c/secrets/API_KEY",
			"DELETE /api/function-domains/fn.acme.com",
		]);
	});

	it("uploads an artifact as raw octet-stream bytes, not JSON", async () => {
		respond = () => json({ artifactRef: "platform://fn_1/abc", digest: "sha256:abc", bytes: 3 });
		const bytes = new Uint8Array([1, 2, 3]).buffer;
		const result = await functionsApi.uploadArtifact("a.b.c", "sha256:abc", bytes);
		expect(calls[0]!.method).toBe("PUT");
		expect(calls[0]!.url).toBe("/api/functions/a.b.c/artifacts/sha256%3Aabc");
		expect(calls[0]!.headers["Content-Type"]).toBe("application/octet-stream");
		expect(calls[0]!.body).toBe(bytes);
		expect(result.artifactRef).toBe("platform://fn_1/abc");
	});

	it("still sends JSON bodies with a JSON content type", async () => {
		await functionsApi.promote("a.b.c", 2);
		expect(calls[0]!.method).toBe("PUT");
		expect(calls[0]!.url).toBe("/api/functions/a.b.c/aliases/live");
		expect(calls[0]!.headers["Content-Type"]).toBe("application/json");
		expect(JSON.parse(calls[0]!.body as string)).toEqual({ version: 2 });
	});

	it("passes ?version= to the config and secret reads", async () => {
		await functionsApi.getConfig("a.b.c", 4);
		await functionsApi.listSecrets("a.b.c");
		expect(calls.map((c) => c.url)).toEqual([
			"/api/functions/a.b.c/config?version=4",
			"/api/functions/a.b.c/secrets",
		]);
	});

	it("always sends the required clientId when listing domains", async () => {
		respond = () => json([]);
		await functionsApi.listDomains("platform");
		await functionsApi.listRoutes({ address: "a.b.c" });
		expect(calls.map((c) => c.url)).toEqual([
			"/api/function-domains?clientId=platform",
			"/api/function-routes?address=a.b.c",
		]);
	});

	it("keeps the error envelope's code and details on the ApiError", async () => {
		respond = () =>
			json(
				{
					error: "MANIFEST_INVALID",
					message: "manifest is invalid",
					details: { errors: [{ message: "required", location: "body.manifest.entrypoint" }] },
				},
				400,
			);
		const err = await functionsApi
			.publishVersion(
				"a.b.c",
				{
					artifactRef: "platform://x",
					digest: "sha256:x",
					manifest: { runtime: "wasm", entrypoint: "wasi_http_incoming_handler" },
				},
				{ suppressGlobalErrorToast: true },
			)
			.catch((e: unknown) => e);
		expect(err).toBeInstanceOf(ApiError);
		expect((err as ApiError).code).toBe("MANIFEST_INVALID");
		expect((err as ApiError).status).toBe(400);
		expect((err as ApiError).details).toEqual({
			errors: [{ message: "required", location: "body.manifest.entrypoint" }],
		});
	});
});

describe("generated function types", () => {
	it("api/functions.ts takes its types from the generated function document", () => {
		const src = readFileSync(
			fileURLToPath(new URL("../../src/api/functions.ts", import.meta.url)),
			"utf-8",
		);
		expect(src).toMatch(/from "\.\/generated-functions"/);
		// No hand-written copy of a generated response type.
		expect(src).not.toMatch(/export (interface|type) (FunctionResponse|VersionResponse|PolicyResponse)\b/);
	});
});
