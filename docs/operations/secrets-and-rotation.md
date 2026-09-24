# Secrets and Rotation

Database credentials are resolved by `fc-platform/src/shared/database.rs`; other secrets come from environment variables or are stored encrypted by `fc-platform/src/shared/encryption_service.rs`. This document covers what's a secret, where to put it, and how rotation works.

---

## What's secret

| Item | Where it goes | Rotation impact |
|---|---|---|
| Database password | AWS Secrets Manager (preferred) or env var | Live rotation supported via `start_secret_refresh` |
| `FLOWCATALYST_APP_KEY` (AES key for OIDC client secrets at rest) | Long-lived secret store | Rotation requires re-encrypting `oauth_clients` rows; not currently automated |
| JWT signing keys (RSA private + public) | File or env var | Rotation via `FC_JWT_PUBLIC_KEY_PATH_PREVIOUS` |
| `FC_API_TOKEN` for outbox / external SDKs | Service account, in your secret store | Per-token rotation (regenerate via API) |
| OIDC client secrets for outbound bridge (e.g. Entra) | Encrypted via `FLOWCATALYST_APP_KEY` in `oauth_clients.encrypted_client_secret` | Rotated by editing the IDP record |
| Webhook signing secrets (per connection) | Encrypted in `msg_connections` | Rotated by editing the connection |
| Redis password | `FC_STANDBY_REDIS_URL` (or `REDIS_URL`) | Restart-only |
| AWS credentials | AWS default credential chain (IAM role / IRSA / instance profile) | Auto-rotated by AWS |

---

## Database password rotation

The interesting case. Production uses AWS Secrets Manager with RDS-managed rotation.

### Initial setup

1. Create the RDS instance with a master password managed by Secrets Manager.
2. Create a separate application user in Postgres. Don't use the master for the app.
3. Create a separate Secrets Manager secret for the app user. Enable rotation (single-user is the safe default; rotates ~every 30 days).
4. Configure the platform:

```sh
DB_HOST=fc-prod.cluster-xyz.eu-west-1.rds.amazonaws.com
DB_NAME=flowcatalyst
DB_SECRET_ARN=arn:aws:secretsmanager:eu-west-1:123456789012:secret:flowcatalyst/db/app-AbCdEf
DB_SECRET_PROVIDER=aws
DB_SECRET_REFRESH_INTERVAL_MS=300000    # 5 min
```

At startup, `resolve_database_url` reads the secret once, builds the connection URL, and registers a background refresh task (`start_secret_refresh`) that re-reads the secret every 5 minutes. When the secret's content changes, the refresh task updates the pool's `connect_options` so new connections use the fresh credentials.

### What "rotation" looks like in practice

```
T0    RDS rotates the password. Secret value updated in Secrets Manager.
      Existing pool connections continue to work (Postgres doesn't enforce password on existing sessions).
      New connections opened during this window fail with auth errors.

T0+up to 5min   `start_secret_refresh` polls SM, gets the new value.
                Pool's connect_options updated.
                Subsequent new connections succeed.

After T0+5min: pool fully recovered. Existing connections continue until reset (~30 min idle timeout, then reconnect uses new creds).
```

The 5-minute worst case is acceptable because:

- The platform has multiple existing connections actively serving traffic.
- New connections opened during the gap retry with backoff.
- The user impact is at most "API errors for the brief window when a transient connection is needed" — usually invisible.

If you need tighter, lower `DB_SECRET_REFRESH_INTERVAL_MS`. Don't go below 30 s — Secrets Manager API calls are not free, and faster polling buys very little.

### Critical: register refresh for every pool

The main pool gets refresh registration automatically. The stream processor uses a **separate, dedicated pool** (4 connections) and **also** registers refresh — see `bin/fc-server/src/main.rs::spawn_stream_processor`. Without that registration the stream pool would silently break on rotation while the main pool kept working.

If you add a new dedicated pool (a custom subsystem, a per-tenant pool, etc.), you must register refresh for it too:

```rust
if let Some(provider) = secret_provider {
    fc_platform::shared::database::start_secret_refresh(
        provider, my_pool.clone(), database_url, refresh_interval
    );
}
```

This is one of the easier production foot-guns to step on. Worth a checklist item during code review.

### Non-AWS

For self-hosted Postgres without rotation: set `DB_PASSWORD` (or use `FC_DATABASE_URL` with the password inline) and accept that rotation is a restart-only operation. Schedule it with whatever change-management you use.

---

## JWT signing key rotation

The platform signs FC tokens with a single RSA key. Rotation is supported via a "previous key" window.

### Generate keys

```sh
openssl genrsa -out jwt-private.pem 2048
openssl rsa  -in jwt-private.pem -pubout -out jwt-public.pem
```

2048 bits is the minimum; 4096 bits if you're nervous, accepting the modest CPU cost on every token verify.

### Rotation procedure

```
Initial state:  active = K0, previous = none.

Step 1: Generate K1.

Step 2: Deploy K1 alongside K0:
        FC_JWT_PRIVATE_KEY_PATH=/secrets/k1.pem
        FC_JWT_PUBLIC_KEY_PATH=/secrets/k1.pub.pem
        FC_JWT_PUBLIC_KEY_PATH_PREVIOUS=/secrets/k0.pub.pem

Step 3: Restart all platform nodes (rolling).
        New tokens minted under K1. Old tokens (minted under K0) still validate.

Step 4: Wait for FC_ACCESS_TOKEN_EXPIRY_SECS (default 1 h) + buffer.
        Old K0 tokens all expired.

Step 5: Drop previous-key env var. Restart.
```

The grace period (step 4) only needs to cover access tokens — refresh tokens are stored server-side (`oauth_oidc_payloads`) and revalidated on use, so they're not affected by JWT rotation.

### Why support previous key

Without it, JWT rotation requires invalidating every issued token instantly — every user has to re-authenticate. With it, rotation is a no-user-impact operation: just deploy, wait an hour, clean up.

### Key generation in dev

If neither `FC_JWT_PRIVATE_KEY_PATH` nor `FLOWCATALYST_JWT_PRIVATE_KEY` is set, fc-server auto-generates a 2048-bit pair at first start and writes to `.jwt-keys/`. **Dev only.** In production a restart would generate fresh keys and invalidate every token — never let this happen accidentally.

---

## `FLOWCATALYST_APP_KEY` rotation

The AES-256 symmetric key used by `EncryptionService` to encrypt OIDC client secrets in `oauth_clients` (and other sensitive fields). Stored in your secret store, injected via env var.

**Rotation is not currently automated.** Procedure if you need it:

1. Generate a new 32-byte key, base64-encode.
2. Write a one-off migration that decrypts every `oauth_clients.encrypted_client_secret` with the old key and re-encrypts with the new.
3. Run the migration during a maintenance window.
4. Update `FLOWCATALYST_APP_KEY` everywhere and restart.

In practice the app key has been treated as long-lived. Treat it as you would a Postgres master password — controlled access, rotated rarely, kept in step with the rest of your security posture.

**If you lose `FLOWCATALYST_APP_KEY`, the encrypted client secrets are unrecoverable.** They can be regenerated (the regenerate-secret endpoint on `/api/oauth-clients/:id`), but every dependent service account needs to receive the new plaintext. Plan for it.

---

## Webhook signing secrets

Each `msg_connections` row has a `signing_secret` (HMAC key). The router computes HMAC-SHA256 over `timestamp + body` and sends:

```
X-FLOWCATALYST-SIGNATURE: <hex>
X-FLOWCATALYST-TIMESTAMP: <ISO-8601 with milliseconds>
```

The receiver validates by recomputing.

Rotation: edit the connection in the admin UI, paste a new secret. The connection's status briefly transitions during the update; in-flight messages signed with the old secret may still be in flight when the receiver upgrades to the new — design your receiver to accept either during the rotation window if downtime isn't acceptable.

Typical pattern (receiver-side):

```python
# Pseudo-code: accept either of two secrets during rotation
def verify(timestamp, body, signature):
    for secret in [PRIMARY_SECRET, FALLBACK_SECRET]:  # FALLBACK_SECRET = None outside rotation
        if hmac.compare_digest(hmac.HMAC(secret, timestamp + body).hexdigest(), signature):
            return True
    return False
```

---

## API tokens for service accounts

Created via the platform admin UI (`/api/oauth-clients`). Each service account has a client ID + client secret. The secret is returned exactly once at creation and once on each call to `/api/oauth-clients/:id/regenerate-secret`. After that the platform only has the encrypted form.

Stored, rotated, and consumed via your secret store of choice.

For an outbox processor:

```sh
FC_API_TOKEN=$(aws secretsmanager get-secret-value \
    --secret-id /flowcatalyst/outbox/api-token \
    --query SecretString --output text)
```

To rotate: regenerate via API, store the new value, redeploy the consumer with the new token. The old token continues to work until revoked (per the OAuth standard).

---

## Encryption at rest

Beyond what's described above:

- **Postgres data at rest** is the underlying storage's responsibility. RDS does this transparently when "Storage encryption" is enabled at provisioning. Self-hosted: use filesystem-level encryption (LUKS, AWS EBS encryption, etc.).
- **Database backups** are encrypted by RDS when the source is encrypted. Logical backups (`pg_dump`) are not — handle them as sensitive.
- **Frontend bundle** isn't sensitive (it's served to every user). No encryption concern.

---

## Audit

Database-secret reads go through AWS Secrets Manager, which logs each `GetSecretValue` call via CloudTrail. Use that to detect unexpected access patterns.

Within the platform, `aud_logs` records every operator action — including secret-regeneration calls (`POST /api/oauth-clients/:id/regenerate-secret`). The admin UI's audit log view exposes this.

---

## Code references

- Database resolution: `bin/fc-server/src/main.rs::resolve_database_url`.
- Secret refresh task: `crates/fc-platform/src/shared/database.rs::start_secret_refresh`.
- AWS secret provider: `crates/fc-platform/src/shared/database.rs::AwsSecretProvider`.
- JWT key loading: `crates/fc-platform/src/auth/auth_service.rs::AuthService::new`.
- Encryption service: `crates/fc-platform/src/shared/encryption_service.rs`.
- Webhook signing: `crates/fc-router/src/mediator.rs::sign`.
