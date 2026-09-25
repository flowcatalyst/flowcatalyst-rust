import { defineConfig } from "@hey-api/openapi-ts";

// Default to the snapshotted JSON spec (refreshed by `just regen-sdks`).
const livePort = process.env.FC_API_PORT ?? "8080";
const openApiInput =
	process.env.OPENAPI_LIVE === "true"
		? `http://localhost:${livePort}/q/openapi`
		: "./openapi/openapi.json";

// The function API is a second, separate document: Java's
// `functions.openapi.json` plus Rust's backward-compatible additions, which
// the platform serves verbatim at `GET /api/openapi-functions.json`
// (crates/fc-platform/src/function/openapi.rs).
// Types only — `api/functions.ts` wraps them over the hand-rolled
// `api/client.ts`, as Java's SPA does (docs/spec/function-ui.md §1 there).
const functionsOpenApiInput =
	"../crates/fc-platform/resources/openapi/functions.openapi.json";

export default defineConfig([
	{
		input: openApiInput,
		output: {
			path: "src/api/generated",
		},
		postProcess: [],
		plugins: ["@hey-api/typescript", "@hey-api/sdk", "@hey-api/client-fetch"],
	},
	{
		input: functionsOpenApiInput,
		output: {
			path: "src/api/generated-functions",
		},
		postProcess: [],
		plugins: ["@hey-api/typescript"],
	},
]);
