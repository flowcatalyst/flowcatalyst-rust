# Authentication & OIDC

FlowCatalyst is a multi-tenant control plane. Every request that mutates state needs an authenticated principal with the right permissions for the right client. The auth subsystem covers three sources of identity, one common token shape, and a deliberately narrow OAuth/OIDC surface.

Source: `crates/fc-platform/src/auth/`, plus the JWT signing keys configured at process startup. This document is the architecture view; operator-level setup (how to configure an IDP, where to put RSA keys, key rotation) lives in [operations/identity-and-auth.md](../operations/identity-and-auth.md).

---

## What we are and aren't

We are: a **domain-aware token bridge** plus **OAuth 2.0 / OIDC provider** with a deliberately small surface.

- We **issue** FlowCatalyst tokens enriched with FC-specific claims (`scope`, `clients`, `roles`, `applications`).
- We **consume** tokens from customer-side identity providers (Entra ID, Keycloak, Google Workspace) and map external identity to internal principals.
- We **manage** session cookies for the FlowCatalyst frontend.
- We **support** OAuth client_credentials grant for service accounts.

We are not a general-purpose OP. The narrow scope is the security argument; widening it requires a real customer requirement and a security review. The deliberately-out-of-scope list (per `architecture-direction.md`):

- Implicit flow (`response_type=token`).
- Hybrid flow.
- `grant_type=password` (resource owner password credentials).
- Dynamic client registration (RFC 7591).
- CIBA, FAPI 2.0, token exchange (RFC 8693), UMA.
- Front-channel logout.

The narrowness keeps the attack surface small and the maintenance load low.

---

## Three identity sources

```
                ┌──────────────────────────────────────────────────┐
                │              FC Auth                              │
                │                                                   │
   Local        │  password_login                                   │
   password ───▶│       │                                           │
                │       ▼                                           │
                │  auth_service::verify_password (argon2id)         │
                │  iam_login_attempts (backoff)                     │
                │       │                                           │
   OIDC         │       │                                           │
   bridge       │  oidc_callback                                    │
   (Entra,    ──▶       │                                           │
    Keycloak,  │       ▼                                           │
    Google)    │  jwks_cache::validate_id_token                     │
                │  email_domain_mapping → scope + roles             │
                │       │                                           │
   Service      │       │                                           │
   account    ──▶  oauth_token (client_credentials)                 │
   (machine)   │       │                                           │
                │       ▼                                           │
                │  oauth_clients::verify (encrypted secret)         │
                │       │                                           │
                │       ▼                                           │
                │  AuthService::generate_access_token               │
                │  RS256 signed JWT                                 │
                └──────────────────────────────────────────────────┘
                                  │
                                  ▼ FC token returned
```

All three converge on the same token-issuance code (`auth_service::generate_access_token`). Downstream code can't tell which source produced a token — it sees only the claims.

### Local password (`auth/auth_api.rs`)

For clients without their own IDP. Argon2id-hashed passwords stored on `iam_principals`. Login flow:

1. `POST /auth/login` with `{ email, password }`.
2. Look up principal; if not found, return 401.
3. Check `iam_login_attempts` backoff for this email + IP. If locked, 429.
4. `argon2::verify_encoded` — constant-time comparison.
5. Success: issue session JWT, set HttpOnly cookie. Emit `UserLoggedIn` via UoW.
6. Failure: record attempt with `iam_login_attempts`. After N failures, exponential backoff (5s, 10s, 30s, …).

Password rotation: `auth/password_reset_api.rs` — emails a single-use token that authorises a password reset within 30 minutes.

Argon2id parameters track OWASP defaults: memory ≥ 47 MiB, time cost ≥ 1, parallelism 1. Changing these is a migration concern — existing hashes embed the parameters, so old hashes still validate even after the defaults change for new ones.

### OIDC bridge (`auth/oidc_login_api.rs`, `auth/jwks_cache.rs`)

For clients with their own IDP. The platform doesn't replace the IDP — it bridges from "user authenticated by Entra" to "user authenticated within FlowCatalyst with the right roles".

Flow:

1. **Domain check.** `POST /auth/check-domain { email: "alice@acme.com" }` looks up `tnt_email_domain_mappings` for the domain. If matched, returns `{ method: "oidc", provider_id: "idp_..." }`. Otherwise `{ method: "internal" }` (local password).
2. **Redirect.** Frontend builds the authorization URL for the IDP using `oauth_oidc_login_states` to store a CSRF token and the nonce. The state row is single-use.
3. **Callback.** `GET /auth/oidc/login/callback?code=...&state=...`:
   - Look up the state row using `DELETE ... RETURNING state, nonce, ...` — atomic single-use consumption. Race-free even under retries.
   - Exchange the code for tokens at the IDP's token endpoint.
   - Validate the ID token's signature against the IDP's JWKS (cached, see below).
   - Validate `iss`, `aud`, `exp`, `nonce`. Reject `#EXT#` Entra guest accounts (they're cross-tenant artifacts that shouldn't grant access to our customers' data).
4. **Principal sync.** `auth/oidc_sync_service.rs::sync_or_create_principal`:
   - Look up `iam_principals` by email. If absent and the `EmailDomainMapping` allows auto-create, create one. If absent and auto-create disabled, return 403.
   - Apply the mapping's role assignments to the principal.
5. **Issue session JWT** + emit `UserLoggedIn` via UoW.

### JWKS cache

`auth/jwks_cache.rs::JwksCache`:

- Per-issuer cache: `HashMap<issuer_url, (Jwks, fetched_at)>`.
- TTL configurable, default 5 minutes.
- On miss: fetch `{issuer}/.well-known/openid-configuration`, follow `jwks_uri`, parse.
- On `kid` not found in cached JWKS: force refresh and retry once (handles IDP key rotation between cache windows).

Trade-off: cache TTL determines how quickly an IDP's key rotation propagates. Five minutes is the worst-case lag for a rotated-out key that the cache still holds; new keys are picked up immediately because of the kid-miss refresh.

### Service accounts (`auth/oauth_api.rs`)

For machine-to-machine. Pure OAuth 2.0 `client_credentials`:

```
POST /oauth/token
  grant_type=client_credentials
  client_id=svc_acme_orders
  client_secret=<plaintext>
```

The service uses `oauth_clients` for credential storage. The secret is encrypted at rest (`EncryptionService::encrypt` with `FLOWCATALYST_APP_KEY`); the plaintext is returned exactly once at creation or regeneration. Rotation: `POST /api/oauth-clients/:id/regenerate-secret` invalidates the old secret and returns the new plaintext.

Same flow as login: validate → issue FC token with the service account's principal + role assignments → return.

---

## Token shape

The platform always issues RS256-signed JWTs with these claims:

```jsonc
{
  // standard
  "iss": "https://platform.example.com",
  "sub": "usr_0HZXEQ6A2B3C4",
  "aud": ["flowcatalyst"],
  "exp": 1716000000,
  "iat": 1715996400,
  "jti": "tok_0HZXEQ7D5E6F7",

  // FC-specific
  "scope": "ANCHOR" | "PARTNER" | "CLIENT",
  "clients": ["*"] | ["clt_abc:acme", "clt_def:globex"] | ["clt_abc:acme"],
  "roles": ["rol_admin", "rol_dispatch_writer"],
  "applications": ["platform", "billing", "orders"],
  "email": "alice@acme.com"
}
```

Key fields:

- **`scope`** — coarse-grained access band. Anchor sees everything; Partner sees their assigned clients; Client sees their own.
- **`clients`** — Anchor `["*"]`; Partner explicit list; Client single-element list with their own client ID.
- **`roles`** — flat list of role IDs. The permission resolution cache turns these into a flattened permission set.
- **`applications`** — applications the principal has access to (covered by `iam_principal_application_access` junction).

The middleware extracts and verifies the token, builds an `AuthContext { principal, scope, clients, permissions, applications }`, and inserts it into request extensions. Handlers call `auth.has_permission(...)` / `auth.can_access_client(...)` against this context.

### Refresh tokens

Issued alongside access tokens for browser sessions. Stored in `oauth_oidc_payloads` (oidc-provider compatible JSONB schema). Single-use rotation: each refresh issues a new refresh token and revokes the previous. Replay of an already-rotated refresh token is detected via parent chain — when this happens we revoke the entire token family (the rotation tree from the original login).

### Session tokens (cookies)

The frontend uses session cookies, not bearer tokens. Cookies are:

- `HttpOnly` — JS cannot read them.
- `Secure` in production.
- `SameSite=Lax` by default; `Strict` available via `FC_SESSION_COOKIE_SAME_SITE`.
- Path scoped to `/`.

The cookie carries a session token (FC JWT) with longer TTL than an access token (default 8 h vs 1 h). API endpoints accept either bearer or cookie — middleware checks both. Cookie auth wins if both are present.

---

## Key management

JWT signing uses a single RSA key pair. Loaded by `AuthService::new` from one of three sources, in priority:

1. **File paths.** `FC_JWT_PRIVATE_KEY_PATH` + `FC_JWT_PUBLIC_KEY_PATH`. PEM-encoded.
2. **Inline env.** `FLOWCATALYST_JWT_PRIVATE_KEY` + `FLOWCATALYST_JWT_PUBLIC_KEY`. PEM in env variable.
3. **Auto-generation.** If neither set, generate a 2048-bit pair at startup and persist to `.jwt-keys/`. **Dev only** — fine for fc-dev, lethal in prod because every restart rotates the key.

### Key rotation

`FC_JWT_PUBLIC_KEY_PATH_PREVIOUS` holds the previous public key during a rotation window:

```
T0:  active = K0, previous = none.
T1:  rotate. active = K1, previous = K0. Tokens minted under K0 still validate.
T2:  K0 tokens have expired. Remove previous.
```

Validation tries the active key first; on failure tries previous (if set). Signing always uses active. Rotation cadence is the operator's call; 90-day rolls are common.

---

## Tenancy: anchor / partner / client

Three scope bands, encoded in JWT `scope`:

| Scope | Access | `clients` claim | Typical example |
|---|---|---|---|
| Anchor | All clients, all applications | `["*"]` | Platform admin |
| Partner | Assigned clients, allowed applications | `["clt_a:acme", "clt_b:globex"]` | Integration partner who manages 5 customer tenants |
| Client | Their own client | `["clt_a:acme"]` | End user of one tenant |

Scope resolution at login time:

1. **Anchor domain match.** If the email's domain is in `tnt_anchor_domains`, scope = Anchor.
2. **Email domain mapping.** Otherwise consult `tnt_email_domain_mappings`. The mapping defines a `scope_type` (`Client` or `Partner`) and a `client_id` (for Client) or list (for Partner).
3. **Fallback.** No mapping → reject. Anonymous logins are not supported.

Anchor and partner principals can switch clients in the UI. `/auth/client/switch?client_id=clt_a` re-issues their session JWT with that client in the `clients` list narrowed; their underlying assignment is unchanged but the active token only sees the selected client. This is "current-acting-as" — useful when a partner is troubleshooting a specific tenant.

---

## Permission resolution

Roles are bags of permissions. A principal's effective permissions are the union of every role's permissions. Permission strings are 4-segment colon-separated: `application:resource:entity:action`, with `*` wildcards at any segment.

Examples granted to built-in roles:

```
platform:event:*:read              # viewer: read any event
platform:subscription:*:write      # editor: write any subscription
*:*:*:*                            # full admin (only for built-in admin role)
platform:scheduled_job:*:fire      # firing manual scheduled-job runs
```

Wildcard matching is segment-by-segment. `platform:event:*:read` matches `platform:event:order:read` and `platform:event:invoice:read`, but not `platform:dispatch_job:*:read`.

Permission check at the handler level uses convenience wrappers from `shared/authorization_service.rs::checks`:

```rust
async fn create_subscription_handler(
    State(state): State<...>,
    auth: AuthContext,
    Json(req): Json<CreateSubscriptionRequest>,
) -> Result<...> {
    can_create_subscriptions(&auth)?;
    auth.require_client_access(&req.client_id)?;
    // ...
}
```

The convention is documented in CLAUDE.md and the [platform-control-plane.md](platform-control-plane.md) doc.

### Role catalogue

Defined once in `role/entity.rs::roles()`. The same list is consumed by:

- `seed_builtin_roles` at startup — upserts these into `iam_roles`.
- `role_sync_service` — diffs against DB for ad-hoc syncs.

There are no parallel constants elsewhere in the code. Catalogue mutations are made by editing `entity.rs` and shipping a deploy.

---

## Maintenance vigilance

Wire-protocol correctness is a permanent cost. The places we have to keep airtight (from `architecture-direction.md`):

- **JWT signing/verification** — clock skew, `kid` handling, `alg` confusion. Use a single library (`jsonwebtoken`) consistently; never decode without verifying.
- **Refresh token rotation** — single-use, parent-chain detection of replay, revoke-on-reuse.
- **Authorization code one-shot consumption** — `find_and_consume_state` uses `DELETE ... RETURNING`. Don't regress.
- **JWKS rotation** — cached tokens minted under the old kid must validate until expiry. Kid-miss refresh handles this.
- **Login attempt rate limiting** — `iam_login_attempts` table exists. Local password and service-account paths both consult it.

The auth surface is small enough to security-review annually as one focused pass. The narrowness of scope (no implicit, no hybrid, no token exchange, no dynamic registration) keeps the review tractable.

---

## Code references

- Auth service: `crates/fc-platform/src/auth/auth_service.rs`.
- Token validation middleware: `crates/fc-platform/src/shared/middleware.rs::AuthLayer`.
- Password login: `crates/fc-platform/src/auth/auth_api.rs`.
- OAuth: `crates/fc-platform/src/auth/oauth_api.rs`.
- OIDC bridge: `crates/fc-platform/src/auth/oidc_login_api.rs`, `oidc_sync_service.rs`.
- JWKS cache: `crates/fc-platform/src/auth/jwks_cache.rs`.
- Password reset: `crates/fc-platform/src/auth/password_reset_api.rs`.
- Login backoff: `crates/fc-platform/src/auth/login_backoff.rs`.
- Permission checks: `crates/fc-platform/src/shared/authorization_service.rs::checks`.
- Role catalogue: `crates/fc-platform/src/role/entity.rs::roles`.
- Encryption (oauth client secrets, sensitive payloads): `crates/fc-platform/src/shared/encryption_service.rs`.
