# fc-sdk

FlowCatalyst SDK for Rust — domain event patterns, outbox integration,
and a platform API client. In-workspace crate; not published to
crates.io. Used by FlowCatalyst's own binaries (`fc-router`,
`fc-outbox-processor`, …) and available to any Rust app inside this
workspace that wants to publish events or consume webhooks.

## Add to your crate

In a workspace member's `Cargo.toml`:

```toml
[dependencies]
fc-sdk = { path = "../../crates/fc-sdk" }
```

Default features pull in the Postgres outbox writer, the in-memory
cache backend, and the distributed-lock primitive. The full feature
list lives in [`Cargo.toml`](Cargo.toml); the most common toggles:

| Feature | Pulls in | When to enable |
|---|---|---|
| `outbox-postgres` (default) | `sqlx/postgres` | Postgres outbox writer |
| `outbox-sqlite` | `sqlx/sqlite` | SQLite outbox writer (dev / tests) |
| `outbox-mysql` | `sqlx/mysql` | MySQL outbox writer |
| `client` | `reqwest` | Platform API HTTP client |
| `auth` | `reqwest`, `jsonwebtoken`, … | JWT verification + client_credentials grant helpers |
| `webhook` | `hmac`, `sha2`, `hex` | Webhook signature verification |
| `cache-redis` | `redis` | Redis cache backend |
| `lock-redis` | `redis` | Redis distributed lock |

## Local development with `fc-dev`

To exercise this SDK locally you need a FlowCatalyst control plane to
talk to. `fc-dev` is the official one-binary dev environment.

Inside this workspace, you can run it directly from source — no
install step:

```bash
# From the repo root:
cargo run --bin fc-dev          # API on http://localhost:8080

# or via the justfile:
just dev                        # cargo-watch + hot reload
```

For installing the release build (for projects outside this workspace),
see [INSTALL.md](../../INSTALL.md).

If your app publishes events via the **outbox pattern** (the SDK's
`outbox-postgres` / `-sqlite` / `-mysql` features), you also need
`fc-dev outbox` running as a sidecar — it polls the app's
`outbox_messages` table and forwards events to the platform.

```bash
# In your app's directory (where fc-sdk is a dependency):

# Once: write FC_OUTBOX_DB_URL / FC_OUTBOX_API_URL / FC_OUTBOX_TOKEN
# into ./.env (0600 perms; no secrets on argv or shell history).
fc-dev outbox init

# Daily: reads .env, auto-creates the `outbox_messages` table on
# first run, then polls.
fc-dev outbox poll
```

Complete reference: [fc-dev CLI docs](../../docs/developers/fc-dev.md).

## Documentation

| Topic | Location |
|---|---|
| Concepts (events, subscriptions, dispatch modes) | [docs/developers/concepts.md](../../docs/developers/concepts.md) |
| Publishing events | [docs/developers/publishing-events.md](../../docs/developers/publishing-events.md) |
| Receiving webhooks | [docs/developers/receiving-webhooks.md](../../docs/developers/receiving-webhooks.md) |
| Outbox internals | [docs/architecture/outbox-processor.md](../../docs/architecture/outbox-processor.md) |
| Workspace overview | [docs/architecture/shared-crates.md](../../docs/architecture/shared-crates.md) |

## License

Copyright (c) 2026 Belac (FlowCatalyst).

[MPL-2.0](https://www.mozilla.org/en-US/MPL/2.0/) — see [LICENSE](LICENSE).
File-level copyleft: changes to the SDK's own files must be shared; your
application that depends on it is not affected. (The platform itself, in
the rest of this repository, is AGPL-3.0-or-later.)
