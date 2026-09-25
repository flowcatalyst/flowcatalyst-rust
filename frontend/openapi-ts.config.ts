import { defineConfig } from "@hey-api/openapi-ts";

// The SPA is Go's (see PROVENANCE.md) and is typed against Go's API contract:
// `openapi/openapi.json` is a verbatim copy of flowcatalyst-go's committed
// OpenAPI lockfile (`api/openapi.lock.json`), the document Go generates these
// same types from. The Rust platform serves Go's paths and shapes (it is a
// drop-in replacement), so the SPA keeps Go's contract rather than Rust's own
// `/q/openapi` document, whose schema names still differ. Refresh the copy
// from Go when Go's contract moves. OPENAPI_LIVE=true points at a running
// server instead (useful to see how far Rust's document has converged).
const livePort = process.env.FC_API_PORT ?? "8080";
const openApiInput =
	process.env.OPENAPI_LIVE === "true"
		? `http://localhost:${livePort}/q/openapi`
		: "./openapi/openapi.json";

export default defineConfig({
	input: openApiInput,
	output: {
		path: "src/api/generated",
	},
	postProcess: [],
	// Types only: the app's transport is the hand-rolled api/client.ts
	// (toasts, 401 handling, field errors).
	plugins: ["@hey-api/typescript"],
});
