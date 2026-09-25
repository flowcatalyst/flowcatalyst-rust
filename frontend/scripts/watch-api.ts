#!/usr/bin/env node

/**
 * Watch for Java backend changes and regenerate API types.
 * Uses Node.js native APIs for file watching and child processes.
 *
 * Uses the live OpenAPI endpoint (http://localhost:8080/q/openapi) for
 * real-time type generation during development.
 */

import { watch } from "fs";
import { spawn } from "child_process";

// This script only ever looks at `.java` filenames (below), so it is
// inherently Java-backend-specific — there is no Go source tree to fall
// back to the way openapi-ts.config.ts falls back to the Go lockfile path.
const JAVA_SOURCE_DIR = "../server/src/main/java";
const DEBOUNCE_MS = 3000;

let debounceTimer: ReturnType<typeof setTimeout> | null = null;
let isGenerating = false;

async function generateTypes(): Promise<void> {
	if (isGenerating) return;

	isGenerating = true;
	const start = performance.now();

	try {
		console.log("Regenerating API types from live endpoint...");

		// Use live endpoint for dev watch mode
		const proc = spawn("npm", ["run", "api:generate:live"], {
			stdio: "inherit",
		});

		const exitCode = await new Promise<number>((resolve) => {
			proc.on("close", (code) => resolve(code ?? 1));
		});

		if (exitCode === 0) {
			console.log(
				`✓ Types regenerated (${Math.round(performance.now() - start)}ms)`,
			);
		} else {
			console.error(`✗ Generation failed with exit code ${exitCode}`);
		}
	} catch (error) {
		console.error("✗ Failed to generate types:", error);
	} finally {
		isGenerating = false;
	}
}

// Initial generation
console.log("Generating initial API types...");
await generateTypes();

// Watch for changes
console.log(`\nWatching ${JAVA_SOURCE_DIR} for changes...`);
console.log("Press Ctrl+C to stop\n");

const watcher = watch(
	JAVA_SOURCE_DIR,
	{ recursive: true },
	(event, filename) => {
		if (!filename?.endsWith(".java")) return;
		if (filename.includes("target") || filename.includes("build")) return;

		console.log(`Changed: ${filename}`);

		if (debounceTimer) {
			clearTimeout(debounceTimer);
		}

		debounceTimer = setTimeout(generateTypes, DEBOUNCE_MS);
	},
);

// Graceful shutdown
process.on("SIGINT", () => {
	console.log("\nStopping watcher...");
	watcher.close();
	process.exit(0);
});
