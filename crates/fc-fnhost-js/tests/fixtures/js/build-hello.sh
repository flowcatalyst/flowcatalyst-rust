#!/bin/sh
# Rebuilds hello.mjs from templates/function-ts (as `fc-dev fn init --lang ts
# hello` renders it) with the template's own esbuild, then refreshes
# SHA256SUMS. Needs node and npm (network for `npm install`).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../../../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cp -R "$root/templates/function-ts/." "$work/"
for f in package.json src/index.ts README.md; do
  sed 's/{{project-name}}/hello/g' "$work/$f" > "$work/$f.tmp" && mv "$work/$f.tmp" "$work/$f"
done
(cd "$work" && npm install --no-audit --no-fund >/dev/null && npm run typecheck && npm run build)
cp "$work/dist/function.mjs" "$here/hello.mjs"
cd "$here" && shasum -a 256 *.mjs > SHA256SUMS
