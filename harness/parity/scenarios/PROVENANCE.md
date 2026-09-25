# Scenario provenance

These 45 files are copied verbatim from the Java repository:

- source: `flowcatalyst-javalin/parity/scenarios/**`
- commit: `65988b51ce9ebd8d7aa97dfeacc0ecf9d6d52d5d` (`git -C ../flowcatalyst-javalin rev-parse HEAD`, 2026-09-25)
- `../surface.json` comes from the same commit (`parity/surface.json`).

Do not edit them here. To pick up new or changed Java scenarios, re-copy the
directory and update the commit above:

```sh
rsync -a --delete --exclude PROVENANCE.md ../flowcatalyst-javalin/parity/scenarios/ harness/parity/scenarios/
cp ../flowcatalyst-javalin/parity/surface.json harness/parity/surface.json
git -C ../flowcatalyst-javalin rev-parse HEAD   # record it above
```

The files keep Java's format (`docs/spec/parity-harness.md` §3 in the Java
repo) so they stay copyable. Their `why` notes and `covers` claims were
written for Go vs Java; many mention Java rulings, which do not apply to a
Go-vs-Rust run (see `../expected-diffs.json`).
