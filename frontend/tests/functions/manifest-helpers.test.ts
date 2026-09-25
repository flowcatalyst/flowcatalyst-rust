// @vitest-environment jsdom
/**
 * The manifest editor's pure helpers (ported from Java's SPA) and the
 * display helpers the function pages share.
 */
import { describe, expect, it } from "vitest";
import {
	TOP_ORDER,
	exportManifest,
	newManifestModel,
	parseManifestText,
} from "@/pages/functions/manifestModel";
import { renderPlanLines } from "@/pages/functions/manifestPlanText";
import { detailErrors, heartbeatAge, shortDigest } from "@/pages/functions/format";
import { setPendingManifestSeed, takePendingManifestSeed } from "@/pages/functions/manifestSeed";
import type { PromotePlanResponse } from "@/api/functions";

const SAMPLE = `{
  "$schema": "https://example.test/api/schemas/function-manifest.json",
  "httpAllow": ["api.example.com"],
  "runtime": "wasm",
  "entrypoint": "wasi_http_incoming_handler",
  "endpoints": [{ "auth": "platform", "path": "/hello", "methods": ["GET"] }],
  "config": ["GREETING"],
  "subscriptions": [{ "path": "/on-order", "eventType": "acme:orders:order:created" }]
}`;

describe("manifestModel", () => {
	it("drops $schema on parse, re-adds it first on export, keys in schema order", () => {
		const model = parseManifestText(SAMPLE);
		expect(model).not.toHaveProperty("$schema");

		const exported = exportManifest(model);
		const keys = Object.keys(exported);
		expect(keys[0]).toBe("$schema");
		const rest = keys.slice(1);
		const order = TOP_ORDER as readonly string[];
		expect([...rest].sort((a, b) => order.indexOf(a) - order.indexOf(b))).toEqual(rest);
		expect(rest).toEqual(["runtime", "entrypoint", "endpoints", "subscriptions", "config", "httpAllow"]);
		// Nested entries follow their own schema order too.
		expect(Object.keys((exported["endpoints"] as object[])[0]!)).toEqual(["path", "auth", "methods"]);
	});

	it("round-trips every field it imported", () => {
		const model = parseManifestText(SAMPLE);
		const { $schema: _schema, ...roundTripped } = exportManifest(model);
		expect(roundTripped).toEqual(model);
	});

	it("starts a wasm function from a component template, not an Extism module", () => {
		expect(newManifestModel("wasm")).toEqual({
			runtime: "wasm",
			entrypoint: "wasi_http_incoming_handler",
			endpoints: [{ path: "/hello", auth: "platform", methods: ["GET"] }],
		});
		expect(newManifestModel("jvm").entrypoint).toBe("com.example.fn.Handler");
	});
});

describe("renderPlanLines", () => {
	const base: PromotePlanResponse = {
		alias: "live",
		toVersion: 2,
		settingsMissing: [],
		httpOnly: false,
		conflicts: [],
	};

	it("renders create, update, delete, conflicts and missing settings", () => {
		const lines = renderPlanLines({
			...base,
			pool: { action: "create", key: "p", changedFields: [] },
			subscriptions: [
				{ action: "update", triggerKey: "t", eventType: "a:b:c:d", changedFields: ["path"] },
				{ action: "delete", triggerKey: "u", eventType: "a:b:c:e", changedFields: [] },
			],
			schedules: [{ action: "create", triggerKey: "s", cron: "0 * * * *", changedFields: [] }],
			publicRoutes: {
				action: "replace",
				added: [{ hostname: "fn.acme.com", pathPrefix: "/", aliasPrefixes: [] }],
				removed: [],
			},
			conflicts: [{ code: "PUBLIC_ROUTE_TAKEN", message: "taken" }],
			settingsMissing: ["API_KEY"],
		});
		expect(lines).toEqual([
			"+ pool (create)",
			"~ subscription a:b:c:d (update: path)",
			"- subscription a:b:c:e (delete)",
			'+ schedule "0 * * * *" (create)',
			"+ route fn.acme.com/",
			"! conflict: PUBLIC_ROUTE_TAKEN: taken",
			"! settings missing: API_KEY",
		]);
	});

	it("says no changes for an empty live plan and names an HTTP-only alias", () => {
		expect(renderPlanLines(base)).toEqual(["no changes"]);
		expect(renderPlanLines({ ...base, alias: "qa", httpOnly: true })).toEqual([
			"! named alias — no wiring change",
		]);
	});
});

describe("function page helpers", () => {
	it("shortens a digest but keeps a short one whole", () => {
		expect(shortDigest(`sha256:${"a".repeat(8)}${"0".repeat(50)}${"b".repeat(6)}`)).toBe(
			"sha256:aaaaaaaa…bbbbbb",
		);
		expect(shortDigest("sha256:abc")).toBe("sha256:abc");
	});

	it("renders a heartbeat's age", () => {
		const now = Date.parse("2026-09-25T12:00:00Z");
		expect(heartbeatAge("2026-09-25T11:59:30Z", now)).toBe("30s ago");
		expect(heartbeatAge("2026-09-25T11:55:00Z", now)).toBe("5m ago");
		expect(heartbeatAge("2026-09-25T09:00:00Z", now)).toBe("3h ago");
		expect(heartbeatAge(undefined, now)).toBe("—");
	});

	it("reads details.errors, ignoring malformed entries", () => {
		expect(detailErrors({ errors: [{ message: "bad", location: "query.size" }, { nope: 1 }] })).toEqual([
			{ message: "bad", location: "query.size" },
		]);
		expect(detailErrors(undefined)).toEqual([]);
	});

	it("hands an imported manifest to the editor once, for its own function only", () => {
		setPendingManifestSeed("a.b.c", newManifestModel());
		expect(takePendingManifestSeed("x.y.z")).toBeNull();
		expect(takePendingManifestSeed("a.b.c")).not.toBeNull();
		expect(takePendingManifestSeed("a.b.c")).toBeNull();
	});
});
