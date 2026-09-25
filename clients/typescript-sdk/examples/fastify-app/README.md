# fastify-app — live example against fcdev

A minimal Fastify app authenticated by FlowCatalyst's OIDC server +
client-credentials tokens, using `@flowcatalyst/sdk/fastify`.

## Run

```bash
# 1. Start fcdev from the workspace root.
cargo run -p fcdev

# 2. Register a confidential web client at FlowCatalyst with redirect URI
#    http://localhost:4000/auth/callback. Copy the client id + secret.

# 3. Generate a 32-byte session secret:
node -e "import('@flowcatalyst/sdk/fastify').then(m => console.log(m.generateSessionSecret()))"

# 4. Run.
cd clients/typescript-sdk/examples/fastify-app
export FC_BASE_URL=http://localhost:8080
export FC_CLIENT_ID=clt_xxx
export FC_CLIENT_SECRET=xxx
export SESSION_SECRET=<paste from step 3>
pnpm install
pnpm start
```

Then:

- Browse `http://localhost:4000/dashboard` — bounces through FlowCatalyst's
  login UI, lands back on the dashboard with a session cookie set.
- Mint a client-credentials token from FlowCatalyst and curl an API route:
  ```
  curl -H "Authorization: Bearer $TOKEN" http://localhost:4000/api/me
  ```
- `POST http://localhost:4000/auth/logout-and-redirect` clears the cookie
  and sends the user to FC's platform-logout page so the OIDC session is
  killed as well.

## What this exercises

| Route                                | Guard                  | Behavior                                  |
|--------------------------------------|------------------------|-------------------------------------------|
| `GET /`                              | none                   | public                                    |
| `GET /dashboard`                     | `requireSession()`     | 302 → `/auth/login` if no cookie          |
| `GET /api/me`                        | `requireBearer()`      | 401 JSON if no valid Bearer token         |
| `POST /api/orders`                   | `requireBearer()`      | 401 then 403 on missing permission        |
| `GET /whoami`                        | `requireAuth()`        | 302 for browsers, 401 for machines        |
| `POST /auth/logout-and-redirect`     | none                   | clears cookie + 302 to FC logout page     |

## How permissions are wired

`server.ts` declares a local RBAC catalogue mapping roles (carried on the
FlowCatalyst access token) to permissions (defined in this app's code):

```ts
defineRbac()
  .role("operant:admin").grants("operant:*")
  .role("operant:viewer").grants("operant:read")
  .role("billing-admin").grants("invoice:create", "invoice:read", "invoice:void")
  .build();
```

`principal.hasPermissionTo(["invoice:create"])` looks up the principal's
roles in this catalogue locally — no platform round-trip.

## Custom post-auth logic

The app registers a normal Fastify `preHandler` after the plugin that
logs every authenticated request. Real apps would upsert a local user
record here, attach tenant context, etc.
