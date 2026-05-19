#!/usr/bin/env node
/**
 * Postbuild: rewrite extensionless relative imports in `dist/` to point at the
 * correct .js / .d.ts file, so the emitted ESM is resolvable by Node ESM.
 *
 * Background: tsconfig.json uses `module: "ESNext"` + `moduleResolution: "bundler"`,
 * which lets the SDK source author imports without `.js` (`from "./client"`).
 * tsc emits those verbatim, but Node's ESM loader requires explicit extensions.
 * Without this step, downstream consumers that import the SDK at runtime hit
 *   ERR_MODULE_NOT_FOUND: Cannot find module '.../dist/runner/pg-lock-provider'
 *
 * This script:
 *   - Walks dist/ recursively
 *   - Edits .js and .d.ts files
 *   - Resolves each `from "./x"` / `from "../x"` import target against the file's
 *     directory: `<path>.{js|d.ts}` first, else `<path>/index.{js|d.ts}`
 *   - Rewrites the import string in place
 *   - Skips absolute/package imports and imports that already have extensions
 *
 * Zero deps — runs against any Node ≥ 18.
 */

import { readdir, readFile, stat, writeFile } from 'node:fs/promises';
import { dirname, extname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const DIST_DIR = resolve(SCRIPT_DIR, '..', 'dist');

// Matches the import target portion of `from "..."`, `import("...")`, and bare
// `import "..."`. Captures the path string only. We rewrite the whole quoted
// literal so source maps and surrounding whitespace stay intact.
const IMPORT_LITERAL_RE = /(?<=\bfrom\s*|import\s*\(\s*|\bimport\s+)(["'])(\.\.?\/[^"']+)\1/g;

async function walk(dir) {
  const out = [];
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) {
      out.push(...(await walk(full)));
    } else if (entry.isFile() && (full.endsWith('.js') || full.endsWith('.d.ts'))) {
      out.push(full);
    }
  }
  return out;
}

async function exists(path) {
  try {
    await stat(path);
    return true;
  } catch {
    return false;
  }
}

async function resolveTarget(fileDir, importPath, isDts) {
  // Already has a known extension — leave it alone.
  const ext = extname(importPath);
  if (ext === '.js' || ext === '.mjs' || ext === '.cjs' || ext === '.json' || ext === '.d.ts') {
    return null;
  }

  const resolved = resolve(fileDir, importPath);
  const candidates = isDts
    ? [`${resolved}.d.ts`, join(resolved, 'index.d.ts')]
    : [`${resolved}.js`, join(resolved, 'index.js')];

  for (const candidate of candidates) {
    if (await exists(candidate)) {
      const relativePath = relative(fileDir, candidate);
      // For .d.ts files, the runtime import should still be the .js name;
      // for `.d.ts` declarations TS resolves them via siblings of the imported
      // module, so we always emit the `.js` extension form even in `.d.ts`.
      const importSuffix = candidate.endsWith('/index.js') || candidate.endsWith('/index.d.ts')
        ? '/index.js'
        : '.js';
      const trimmed = relativePath.replace(/\.(d\.ts|js)$/, '');
      const normalized = trimmed.replace(/\/index$/, '');
      const result = `${normalized.startsWith('.') ? normalized : `./${normalized}`}${importSuffix}`;
      return result;
    }
  }

  return null;
}

async function fixFile(path) {
  const original = await readFile(path, 'utf8');
  const fileDir = dirname(path);
  const isDts = path.endsWith('.d.ts');

  let mutated = original;
  const matches = [...original.matchAll(IMPORT_LITERAL_RE)];

  for (const match of matches.reverse()) {
    const [whole, quote, importPath] = match;
    const start = match.index ?? -1;
    if (start < 0) continue;

    const fixedPath = await resolveTarget(fileDir, importPath, isDts);
    if (!fixedPath) continue;

    const replacement = `${quote}${fixedPath}${quote}`;
    mutated = mutated.slice(0, start) + replacement + mutated.slice(start + whole.length);
  }

  if (mutated !== original) {
    await writeFile(path, mutated);
    return true;
  }
  return false;
}

async function main() {
  const files = await walk(DIST_DIR);
  let edited = 0;
  for (const file of files) {
    if (await fixFile(file)) edited += 1;
  }
  console.log(`fix-esm-extensions: edited ${edited}/${files.length} files in dist/`);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
