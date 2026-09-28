# Software supply chain

Rust is the platform's only backend language (owner, 2026-09-28), so every
crate the binaries link is code we ship. This page is the policy for those
crates, the checks that enforce it, and how to read what a release says about
itself.

## Policy

1. **crates.io only.** No git or path dependencies outside this repository,
   and no other registries. `deny.toml`'s `[sources]` enforces it; a git
   dependency needs an `allow-git` entry with a reason, and should be
   temporary.
2. **Locked builds.** Release, Docker and CI builds use `--locked`: the
   versions and checksums in `Cargo.lock`, or the build fails. A dependency
   changes only in a commit that changes `Cargo.lock`.
3. **No known vulnerabilities, unsound or unmaintained crates.** RustSec
   advisories fail CI (`cargo deny check advisories`), direct or transitive,
   dev-dependencies included. An exception is an `ignore` entry in
   `deny.toml` with the reason the advisory cannot reach us (or why no fix
   exists yet); revisit it when the reason changes.
4. **Licences.** The platform is AGPL-3.0-or-later; it may link permissive
   licences, MPL-2.0 and the data licences listed in `deny.toml`. The crates
   applications link into their own code — the Rust SDK `fc-sdk`
   (Apache-2.0) and the function guest crates `fc-function-abi`,
   `fc-function-model`, `fc-function-pdk` (MPL-2.0) — may not depend on
   AGPL, GPL or LGPL code at all (`deny-sdk.toml`, normal dependencies only).
   Any other copyleft licence needs review before it is allowed.
5. **One version per crate.** Two versions of one crate fail the check unless
   `deny.toml`'s `[bans] skip` lists the older one with the reason (usually:
   which dependency still needs it). Remove the entry when that dependency
   moves.
6. **Banned crates.** `native-tls` and its wrappers (TLS is rustls), OpenSSL
   outside webauthn-rs's attestation, and a few crates with maintained
   replacements (`deny.toml`'s `[bans] deny`).
7. **Every third-party crate is audited or exempt.** `cargo vet` passes only
   when each crate version is covered by an audit — ours in
   `supply-chain/audits.toml`, or one imported from the organisations in
   `supply-chain/config.toml` — or by an exemption. The exemptions recorded
   on 2026-09-28 are the starting baseline, not an endorsement: they are the
   tail to audit over time, and no new exemption is added without a reason.
8. **Every release binary says what it contains.** fc-server,
   fc-outbox-processor and fc-dev are built with `cargo auditable`, which
   embeds the dependency list in the binary, and ship with a CycloneDX SBOM.

## Where it runs

| Check | Where | Command |
|---|---|---|
| Advisories, licences, bans, sources | CI `supply-chain` job, every push and PR | `cargo deny check` |
| SDK and guest-crate licences | CI `supply-chain` job | `cargo deny --manifest-path crates/fc-sdk/Cargo.toml --all-features --exclude-dev --config deny-sdk.toml check licenses` (and the three guest crates) |
| Audits | CI `supply-chain` job | `cargo vet --locked` |
| Embedded dependency list | Dockerfile (`fc-server`), `publish-docker.yml` `release-binaries` (fc-server, fc-outbox-processor), `release-fc-dev.yml` (fc-dev) | `cargo auditable build --release --locked` |
| SBOM | the same release jobs | `cargo cyclonedx --format json --spec-version 1.5 --describe binaries --no-build-deps --target <triple>` |

The tools are pinned by version in the workflows and the Dockerfile:
cargo-deny 0.20.2 (release archive, SHA-256 checked), cargo-vet 0.10.2,
cargo-auditable 0.7.6 and cargo-cyclonedx 0.5.9 (`cargo install --locked`,
which builds them from crates.io against their own lockfiles).

To run them locally without installing anything globally:

```sh
cargo install --locked --root target/tools \
  cargo-deny@0.20.2 cargo-vet@0.10.2 cargo-auditable@0.7.6 \
  cargo-cyclonedx@0.5.9 rust-audit-info@0.5.4
export PATH="$PWD/target/tools/bin:$PATH"
cargo deny check
cargo vet
```

## Adding or updating a dependency

1. **Do you need it?** Check whether the workspace already has a crate that
   does the job (`Cargo.toml`'s `[workspace.dependencies]`), and whether the
   standard library does. A crate used in one trivial place is usually a
   function to write instead.
2. **Prefer the well-known organisations.** About half of what fc-server
   links comes from 18 of them (RustCrypto, the Bytecode Alliance,
   rust-lang, tokio-rs, hyperium, rustls, dtolnay, unicode-org, AWS, …);
   their crates are the ones the imported audits cover. A crate from a
   single maintainer needs a reason.
3. **Avoid new build scripts and proc-macros** where an alternative exists.
   Both run arbitrary code on the build machine (and, in CI, next to its
   credentials). A `build.rs` that downloads something (V8's prebuilt
   archive, the embedded PostgreSQL) also breaks offline builds.
4. **Turn features off.** Declare it `default-features = false` with the
   features you use, and enable database drivers per crate (the workspace
   `sqlx` has none). Check `cargo tree -e features -i <crate>` for what
   you pulled in.
5. **Declare it once**, in `[workspace.dependencies]`, and inherit it
   (`{ workspace = true }`), unless it is only a crate's dev-dependency.
6. **Run the checks:**
   - `cargo deny check` — a new duplicate version fails `bans`: upgrade the
     other user, or add a `skip` entry saying why both are needed. A new
     licence fails `licenses`: add it to `allow` only if it is compatible
     (see the policy above; ask when in doubt).
   - `cargo vet` — an unaudited crate version fails. Look for an existing
     audit first (`cargo vet suggest`, `cargo vet import` of another
     well-known organisation). Otherwise audit it (`cargo vet inspect` /
     `cargo vet diff` from the last audited version, then
     `cargo vet certify`), or, if that is not possible now, add an exemption
     with `cargo vet add-exemption <crate> <version>` and say why in the
     commit message.
7. **Commit `Cargo.lock` and `supply-chain/`** with the manifest change.

A patch update of an existing dependency is the same, usually just
`cargo vet` (a `cargo vet diff` of a small delta).

## Reading the SBOM and the embedded dependency list

Each release asset has a CycloneDX 1.5 JSON SBOM beside it
(`<binary>-<version>-<target>.cdx.json`). `metadata.component` is the binary;
`components` lists every crate compiled into it, for that target, with its
version, licence, `purl` (`pkg:cargo/<name>@<version>`) and crates.io
SHA-256 hash; `dependencies` is the graph. Build dependencies are left out
(`--no-build-deps`); proc-macros are still listed.

The SBOM is **conservative**: cargo-cyclonedx resolves features across the
whole workspace, so it can list crates the binary does not contain (for
fc-outbox-processor on 2026-09-28, about 45 of 316 entries: compression
codecs, QUIC, the MongoDB crypto bindings, …). Nothing the binary contains is missing. The
dependency list embedded in the binary (below) is exact; use it when the
difference matters.

```sh
# Crates and versions.
jq -r '.components[] | "\(.name) \(.version)"' fc-server-*.cdx.json
# Licences.
jq -r '.components[] | .licenses[]?.expression // .licenses[]?.license.id' fc-server-*.cdx.json | sort | uniq -c
# Feed it to a scanner.
grype sbom:fc-server-v1.2.3-x86_64-unknown-linux-gnu.cdx.json
```

The binaries also carry their dependency list themselves (cargo-auditable,
a compressed JSON section), so a binary found on a host can be checked
without its SBOM:

```sh
# RustSec advisories for what it contains. fc-server with V8 is ~160 MB,
# over cargo-audit's default 100 MB read limit (and `0` does not mean
# unlimited in cargo-audit 0.22), so raise it:
cargo audit bin --max-binary-size 1000000000 /app/fc-server
rust-audit-info /app/fc-server | jq '.packages | length'   # the raw list
```

Run `cargo audit bin` from the repository root so it picks up
`.cargo/audit.toml`'s ignores.

For the Docker image: `docker create` a container from it and
`docker cp <id>:/app/fc-server .`, then run the same commands.

## Offline, vendored builds

Not enabled yet; the steps, for when it is:

1. `cargo vendor --locked vendor/ > .cargo/vendor.toml` in a stage that has
   network, and verify the vendored checksums against `Cargo.lock` (cargo
   does this on every build from `vendor/`).
2. Build with `cargo build --release --locked --offline --config
   .cargo/vendor.toml`, in a Docker `RUN --network=none` step, so no build
   script can reach the network.

What stops it being a one-line change in the Dockerfile's cargo-chef flow:

- **V8.** fc-server's default `js` feature links deno's `v8` crate, whose
  build script downloads a prebuilt `librusty_v8` archive from GitHub. An
  offline build must fetch it beforehand (per target architecture, checksum
  pinned) and point `RUSTY_V8_ARCHIVE` at it. `--no-default-features`
  builds of fc-server have no such script.
- **cargo-chef cooks the whole workspace**, fc-dev included, whose
  `postgresql_embedded` build script downloads PostgreSQL. Cooking only
  fc-server (`cargo chef cook --bin fc-server`) avoids it.
- `cargo vendor` of the full lockfile is over 900 crates; it wants its own
  cached layer.

Until then, Docker and release builds are `--locked`: they cannot pick up a
version or checksum other than `Cargo.lock`'s, but build scripts can still
reach the network.

## Current numbers (2026-09-28)

Crates each binary links on Linux x86_64, first-party crates included
(`cargo tree -p <bin> -e normal,build --target x86_64-unknown-linux-gnu`,
unique name and version):

| | main before this work | now |
|---|---|---|
| `fc-server` | 615 | 605 |
| `fc-server --no-default-features` (no V8) | 556 | 545 |
| `fc-dev` | 671 | 666 |
| `fc-outbox-processor` | 278 | 291 |
| `fc-router` (library) | 326 | 318 |
| `Cargo.lock` (all features, dev included) | 943 | 924 |

fc-outbox-processor grew with the MongoDB driver's 3.9 release, taken for
its DNS resolver fix (hickory 0.26): it moves to the newer RustCrypto
generation (sha2 0.11, hmac 0.13, md-5 0.11) beside the one sqlx uses.

Of fc-server's 589 third-party crates, 76 have a build script and 45 are
proc-macros (main: 599, 81 and 45). Crates with two versions in fc-server:
35 (main: 41); `deny.toml` lists every duplicate in the whole graph (73)
with the crates that still hold the older version.

`cargo vet` (whole graph, all features, dev included): 902 third-party
crate versions; 179 fully covered by imported audits, 6 partially, 717
exempt. What fc-server ships: 589, of which 133 audited, 4 partially, 452
exempt.

RustSec (`cargo audit`): 25 vulnerabilities and 11 warnings on main; 3 and
4 now before ignores, 0 and 0 with seven reasoned ignores
(`.cargo/audit.toml`; the reasons are in `deny.toml`, which lists six:
cargo-deny never meets `instant` on our targets). Of those seven:

- linked into fc-server and fc-dev: rsa (Marvin; no fix released; RSA signs
  and verifies, never decrypts);
- linked into fc-dev only: quick-xml (2 vulnerabilities; self_update 0.41's
  S3 backend, which fc-dev never uses) and number_prefix (unmaintained;
  formats the `fc-dev upgrade` progress bar);
- build time only: paste (unmaintained proc-macro, utoipa-axum 0.2);
- in no shipped binary: instant (wasm targets only) and rustls-pemfile
  (lapin, fc-queue's `activemq` feature, which no binary enables).
