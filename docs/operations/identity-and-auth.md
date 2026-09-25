# Identity and Authentication

How to configure FlowCatalyst's authentication for operators. Covers OIDC IDP setup, anchor domains, JWT keys, password policy, MFA (forthcoming).

For architecture: [../architecture/auth-and-oidc.md](../architecture/auth-and-oidc.md).

---

## Authentication paths

FlowCatalyst accepts three sources of identity, all of which converge on a FC-issued JWT:

1. **OIDC bridge** — your IDP authenticates the user (Entra, Keycloak, etc.); FC validates the IDP's ID token and issues a FC token enriched with FC claims (scope, clients, roles).
2. **Local password** — for tenants without an IDP, FC stores Argon2id hashes and authenticates directly.
3. **Service account (OAuth client_credentials)** — for machine-to-machine integrations.

Configuration is per-tenant (well, per-email-domain). One platform can serve tenants on multiple IDPs, plus tenants on local password, plus service accounts.

---

## Setting up an OIDC IDP

For each IDP you want to bridge:

### 1. Register a client in your IDP

Configure a redirect URI:

```
https://platform.example.com/auth/oidc/login/callback
```

Note the issuer URL, client ID, and client secret. For Entra ID specifically, use `https://login.microsoftonline.com/{tenant_id}/v2.0` as the issuer.

### 2. Add the IDP in FlowCatalyst

`POST /api/identity-providers` (anchor-only). Fields:

| Field | Description |
|---|---|
| `name` | Display name |
| `type` | `entra`, `keycloak`, `google`, etc. |
| `oidc_issuer_url` | The IDP's discovery URL or issuer |
| `oidc_client_id` | Client ID from the IDP |
| `oidc_client_secret` | Client secret (stored encrypted; returned exactly once) |
| `scopes_requested` | `["openid", "profile", "email"]` typical |

The platform stores `oidc_client_secret` encrypted with `FLOWCATALYST_APP_KEY` (AES-256-GCM).

### 3. Map an email domain to the IDP

`POST /api/email-domain-mappings`:

| Field | Description |
|---|---|
| `email_domain` | e.g. `acme.com` |
| `identity_provider_id` | from step 2 |
| `scope_type` | `Client`, `Partner`, `Anchor` |
| `client_id` | for `Client` scope; the tenant the user belongs to |
| `role_assignments` | role IDs to grant on first login |
| `auto_create_principal` | `true` to auto-provision on first login |

When a user `alice@acme.com` logs in:

1. `POST /auth/check-domain { email: "alice@acme.com" }` → returns `{ method: "oidc", provider_id: "idp_..." }`.
2. Frontend redirects to the IDP's authorization endpoint.
3. After IDP auth: callback to `/auth/oidc/login/callback?code=...&state=...`.
4. Platform exchanges code for tokens at the IDP, validates ID token against IDP's JWKS.
5. Looks up `iam_principals` by email. Creates or syncs per `auto_create_principal`.
6. Applies role assignments from the mapping.
7. Issues FC session JWT.

### IDP-specific notes

**Entra ID (Azure AD):**

- Use issuer `https://login.microsoftonline.com/{tenant_id}/v2.0` (with `/v2.0` suffix).
- Configure redirect URI as "Web" platform type, not "SPA".
- Grant API permissions: `openid`, `profile`, `email`, `User.Read`.
- **Guest accounts (`#EXT#`):** explicitly rejected by FC. The IDP returns ID tokens with `#EXT#` in claims for cross-tenant guest users; we treat these as untrusted and refuse. If you need to support cross-tenant access, do it via separate FC principals, not Entra guests.

**Keycloak:**

- Use the realm's full URL as issuer: `https://keycloak.example.com/realms/myrealm`.
- Confidential client; secret-managed.
- Optional: configure Keycloak's "User Attributes" mapper to expose group membership in the ID token, then add an `idp_role_mappings` entry in FC to translate group → role.

**Google Workspace:**

- Issuer `https://accounts.google.com`.
- Domain restriction via Workspace admin (FC's email-domain mapping handles the rest).
- Service accounts use `client_credentials` separately — not via OIDC.

---

## Anchor domains

Anchor domains are email domains whose users get **Anchor scope** automatically — platform admin access to all clients.

`POST /api/anchor-domains` (anchor-only):

```json
{ "email_domain": "yourcompany.com" }
```

Now any user whose email matches this domain logs in with `scope=ANCHOR`, `clients=["*"]`. Permission resolution still applies — anchors don't bypass permission checks, they just have access to all clients.

Manage this list carefully. Adding a domain grants every future user from that domain platform-admin access. The first installation typically adds the operator's company domain; you might also add `flowcatalyst-team.example` if FC engineering needs cross-tenant access for support.

---

## Local password authentication

For tenants without an IDP. Set the `iam_principals.password_hash` column (Argon2id) and don't map their email domain to an IDP. Users log in via `/auth/login`.

Defaults follow OWASP recommendations: memory ≥ 47 MiB, time cost ≥ 1, parallelism 1. Existing hashes embed their parameters, so changing defaults for new hashes doesn't break existing logins.

### Password reset

Users request reset via `/auth/password-reset/request`. Platform emails a single-use token (via `email_service`). User submits new password at `/auth/password-reset/confirm` with the token.

Configure email delivery:

| Variable | Description |
|---|---|
| `FC_SMTP_HOST` | SMTP server |
| `FC_SMTP_PORT` | port (587 typical) |
| `FC_SMTP_USERNAME`, `FC_SMTP_PASSWORD` | auth |
| `FC_SMTP_FROM` | from address |
| `FC_SMTP_TLS` | `starttls` (default), `tls`, `none` |

For Amazon SES: use SMTP credentials (not API). Set `FC_SMTP_HOST=email-smtp.eu-west-1.amazonaws.com`, port 587.

### Backoff

`iam_login_attempts` records failed attempts per (email, IP). After repeated failures, exponential backoff. Brute force is non-viable.

### MFA

Not yet shipped. The data model is planned around `iam_user_authenticators` so adding MFA doesn't require a migration when it happens. WebAuthn is the intended first authenticator type (passkey + security key support); the `webauthn` aggregate exists in the codebase already as the data model.

Note: passkeys are intended only for tenants **without** an external IDP (the IDP owns identity for federated users). An `email_domain_mapping` to an **OIDC** identity provider gates passkeys off; a mapping to an INTERNAL provider (Go's `fcdev init` creates one for the anchor domain) does not.

---

## Service accounts

For machine-to-machine. Create via `POST /api/service-accounts`:

| Field | Description |
|---|---|
| `code` | Identifier |
| `application_id` | Which application this account belongs to |
| `role_assignments` | Roles to grant |

Then create an OAuth client for the service account: `POST /api/oauth-clients`:

| Field | Description |
|---|---|
| `service_account_id` | from above |
| `name` | Display name |
| `redirect_uris` | Empty for client_credentials |
| `grant_types` | `["client_credentials"]` |

The platform returns a `client_secret` exactly once. Store it; you can regenerate later but not retrieve the original.

The service uses these credentials via the standard OAuth flow:

```
POST /oauth/token
  grant_type=client_credentials
  client_id=svc_acme_orders
  client_secret=<plaintext>
```

The FC token returned can be used as `Authorization: Bearer <token>` on subsequent API calls.

---

## JWT key management

The platform signs FC tokens with one RSA key pair. Loaded at startup from one of:

1. **Files** (preferred for production): `FC_JWT_PRIVATE_KEY_PATH`, `FC_JWT_PUBLIC_KEY_PATH`.
2. **Inline env**: `FLOWCATALYST_JWT_PRIVATE_KEY`, `FLOWCATALYST_JWT_PUBLIC_KEY`.
3. **Auto-gen** (dev only): if neither set, generates a 2048-bit pair on first start.

Generate:

```sh
openssl genrsa -out jwt-private.pem 2048
openssl rsa -in jwt-private.pem -pubout -out jwt-public.pem
```

The public key is exposed at `/.well-known/jwks.json` so receivers can validate FC tokens themselves (if they need to).

### Rotation

See [secrets-and-rotation.md](secrets-and-rotation.md#jwt-signing-key-rotation). Briefly:

1. Generate K1 alongside K0.
2. Deploy with `FC_JWT_PRIVATE_KEY_PATH=K1`, `FC_JWT_PUBLIC_KEY_PATH=K1`, `FC_JWT_PUBLIC_KEY_PATH_PREVIOUS=K0`. Old tokens still validate.
3. Wait for `FC_ACCESS_TOKEN_EXPIRY_SECS` (default 1 h) + buffer.
4. Remove `FC_JWT_PUBLIC_KEY_PATH_PREVIOUS`. Restart.

---

## Permissions and roles

Built-in roles are defined in code (`crates/fc-platform/src/role/entity.rs::roles`) and seeded at every startup. Default catalogue:

| Role | Scope | Typical use |
|---|---|---|
| `admin` | platform-wide | full access (assigned to anchors; rarely to others) |
| `editor` | platform | read + write on events, subscriptions, dispatch jobs |
| `viewer` | platform | read-only |
| (application-specific roles, defined per application) | per application | varies |

Custom roles can be added via:

- **Admin UI**: `POST /api/roles` with source `Database`. Persistent, manually-defined.
- **SDK sync**: `POST /api/sync/roles` from an application. Source `Sdk`. Synced declaratively from the application's manifest.

The role catalogue (built-in) is **never** edited via UI — it's source-of-truth-in-code. Code roles, database roles, and SDK roles coexist; `role_sync_service` reconciles each source against the DB.

Permissions are colon-separated 4-tuples: `application:resource:entity:action`. Wildcards anywhere. Examples:

```
platform:event:*:read
platform:subscription:*:write
*:audit_log:*:read
billing:invoice:*:create
```

Assigning permissions to a role: `POST /api/roles/{role_id}/permissions`.

---

## Multi-tenancy

Three scope types (see [../architecture/auth-and-oidc.md](../architecture/auth-and-oidc.md)):

- **Anchor**: platform admin, sees all clients. Granted via anchor domain match.
- **Partner**: serves multiple specified clients. Configured explicitly per principal via `iam_client_access_grants`.
- **Client**: belongs to one client. Default scope from email-domain-mapping.

Anchor and partner principals can switch active client in the UI (`POST /auth/client/switch`), re-issuing their session JWT with a narrowed `clients` claim. Client principals can't switch — they always see exactly one tenant.

---

## Token expiry

| Token | TTL env | Default |
|---|---|---|
| Access token (programmable) | `FC_ACCESS_TOKEN_EXPIRY_SECS` | 1 h |
| Session cookie (browser) | `FC_SESSION_TOKEN_EXPIRY_SECS` | 8 h |
| Refresh token | `FC_REFRESH_TOKEN_EXPIRY_SECS` | 30 d |

Session lifetime is the most user-visible. 8 h covers a working day without re-login; longer is convenient but inflates the blast radius of a compromised cookie. Most operators leave the defaults.

Refresh tokens implement single-use rotation: each refresh issues a new refresh token and invalidates the previous. Replay of an already-rotated refresh token triggers a token-family revoke (the entire chain rooted at the original login is invalidated). This is the standard OAuth best-practice for confidential clients with stolen-token detection.

---

## Disabling self-service registration

FlowCatalyst doesn't expose self-service signup. New principals are created via:

- Operator action (`POST /api/principals`).
- Automatic provisioning on OIDC login when `email_domain_mappings.auto_create_principal = true`.
- Service-account creation (`POST /api/service-accounts`) followed by OAuth client creation.

To require manual approval for all new users, set `auto_create_principal = false` on all email-domain mappings. Operators then manually create `iam_principals` rows as users come in.

---

## Code references

- Auth service: `crates/fc-platform/src/auth/auth_service.rs`.
- OIDC login flow: `crates/fc-platform/src/auth/oidc_login_api.rs`.
- OAuth (incl. client_credentials): `crates/fc-platform/src/auth/oauth_api.rs`.
- JWKS cache: `crates/fc-platform/src/auth/jwks_cache.rs`.
- Email-domain mapping: `crates/fc-platform/src/email_domain_mapping/`.
- Identity provider: `crates/fc-platform/src/identity_provider/`.
- Anchor domain: `crates/fc-platform/src/client/anchor_domain/`.
- Password reset: `crates/fc-platform/src/auth/password_reset_api.rs`.
- Login backoff: `crates/fc-platform/src/auth/login_backoff.rs`.
- Role catalogue (built-ins): `crates/fc-platform/src/role/entity.rs::roles`.
- Role sync service: `crates/fc-platform/src/shared/role_sync_service.rs`.
- WebAuthn (data model for MFA, not yet active): `crates/fc-platform/src/webauthn/`.
