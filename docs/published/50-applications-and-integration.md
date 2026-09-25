# Applications & Integration

An **application** is the unit of integration: a system that publishes
events, consumes them, registers its own resources, and authenticates its
users and machines through the platform. This page covers how an
application plugs in.

## The application's footprint

| Resource | Registered how |
|---|---|
| Event types (+ schemas) | SDK sync at boot |
| Subscriptions | SDK sync |
| Roles (+ permissions) | SDK sync — role names are prefixed with the application code |
| Scheduled jobs | SDK sync |
| Processes | SDK sync |
| Dispatch pools | SDK sync or admin UI |
| OpenAPI spec | SDK sync — versioned, with automatic change notes between versions |
| Documentation pages | SDK sync — Markdown, shown under Platform → Documentation |
| Users | SDK principal sync, invite APIs, or JIT on federated login |

**Sync is declarative**: the application's code is the source of truth,
and each sync makes the platform match it. Syncs are idempotent — safe to
run on every deploy. Where partial adoption matters, `removeUnlisted`
controls whether unlisted SDK-sourced resources are pruned; documentation
sync always replaces the whole set.

## Credentials an application holds

- **A confidential OAuth client** (client_credentials) for its service
  account — the machine identity used to call the platform API and sync
  endpoints. Application-scoped service accounts are confined to their own
  application server-side.
- **Webhook credentials** — a bearer auth token and an HMAC-SHA256 signing
  secret the platform uses when delivering to the application. Rotatable
  from the admin UI; new values are shown once.
- **An interactive OAuth client** (authorization code + PKCE) when the
  application signs users in through the platform. Client configuration
  pins redirect URIs (exact match), grant types, PKCE, and — for
  first-party apps — whether interactive logins receive API-capable
  tokens.

## Clients and entitlements

A tenant client's access to an application is an explicit **client
config**. Entitlements bound what client-scoped users can be granted:
a client administrator can assign any application their client is entitled
to — not merely the apps the administrator personally uses.

## SDKs

- **TypeScript SDK** — platform client, Fastify auth plugin (OIDC session
  + Bearer verification, RBAC helpers, portal-plane mode), outbox helpers,
  and generated types pinned to the platform's OpenAPI contract.
- **Laravel SDK** — attribute-driven registration (`#[AsEventType]`,
  `#[AsPermission]`, …) scanned and synced by `flowcatalyst:sync`, an
  OIDC bridge for native Laravel login, event publishing with an outbox,
  and webhook verification.

Both SDKs speak the same wire contract; the platform's OpenAPI document
(`/openapi.json`) is the source the generated clients are built from.

## Identifiers

Every platform entity carries a typed, sortable TSID with a 3-character
prefix — `clt_` clients, `app_` applications, `prn_` principals, `evt_`
event types, `sjb_` scheduled jobs, `ptu_` portal users, and so on. The
prefix makes ids self-describing in logs and prevents cross-type
confusion; ids are always platform-generated.

## Documentation sync

Applications ship their own docs the same way they ship event types:

```
POST /api/applications/{appCode}/docs/sync
{ "docs": [ { "slug": "getting-started", "content": "# Getting Started\n..." } ] }
```

- Markdown, with Mermaid fences rendered as diagrams in the admin UI.
- Title comes from the first `# ` heading (or an explicit `title`).
- The payload is the complete set — pages not listed are removed.
- Requires the `platform:application-service:docs:sync` permission (part
  of the application service-account role) and access to the application.
- Pages appear under **Platform → Documentation**, grouped by application,
  visible to administrators.
