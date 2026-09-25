import { defineConfig } from "@hey-api/openapi-ts";

// Default to the snapshotted JSON spec (refreshed by `just regen-sdks`).
// FC_API_PORT lets you override the live port (defaults to 8080, matching fcdev).
const livePort = process.env.FC_API_PORT ?? "8080";
const openApiInput =
	process.env.OPENAPI_LIVE === "true"
		? `http://localhost:${livePort}/q/openapi`
		: "./openapi/openapi.json";

export default defineConfig({
	input: openApiInput,
	output: {
		path: "src/generated",
		// Emit relative imports with `.js` extensions so the generated
		// code is directly Node-ESM-resolvable. Without this, NodeNext
		// module resolution rejects `from "./client"` at compile time
		// and Node's ESM loader rejects it at runtime.
		importFileExtension: ".js",
	},
	plugins: ["@hey-api/typescript", "@hey-api/sdk", "@hey-api/client-fetch"],
	postProcess: ["prettier"],
});
